//! Trusted composition and lifecycle for one authenticated application scope.
use super::{host::*, transport::TransportFactory, *};
use gateway_application::{connector_admission::*, connectors::*};
use gateway_domain::SourceId;
use std::{
    collections::BTreeMap,
    sync::{Mutex, RwLock},
};

/// Operator-supplied runtime adapters. Project input and discovery cannot add
/// these definitions. The registry independently pins their manifest and scope.
pub struct PluginConfig {
    pub host: HostConfig,
    pub transport: Box<dyn TransportFactory>,
    pub policy: Arc<dyn ConnectorAdmission>,
    pub mapping: Box<dyn ConnectorMapping>,
    pub observer: Arc<dyn Observer>,
}
struct Entry {
    host: McpConnectorHost,
    admission: Arc<ScopedConnectorAdmission>,
    lifecycle: Mutex<()>,
}
/// Private hosts prevent callers from retaining removed runtimes. Every rebuild
/// installs fresh sessions/inventories and permanently revokes the old generation.
pub struct PluginLifecycleManager {
    registry: ConnectorRegistry,
    binding: ConnectorScopeBinding,
    entries: RwLock<BTreeMap<String, Arc<Entry>>>,
}
impl PluginLifecycleManager {
    pub fn new(
        registry: ConnectorRegistry,
        binding: ConnectorScopeBinding,
        plugins: Vec<PluginConfig>,
    ) -> Result<Self, Error> {
        let mut entries = BTreeMap::new();
        for plugin in plugins {
            let target = &plugin.host.target;
            let registration = registry
                .admitted(&binding, target)
                .map_err(|_| Error::Configuration)?;
            if registration.manifest != plugin.host.manifest
                || entries.contains_key(target.connector.as_str())
            {
                return Err(Error::Configuration);
            }
            let admission = registry
                .bind(&binding, target, plugin.policy)
                .map_err(|_| Error::Configuration)?;
            let id = target.connector.as_str().to_owned();
            let host = McpConnectorHost::new(
                plugin.host,
                plugin.transport,
                admission.clone(),
                plugin.mapping,
                plugin.observer,
            )?;
            entries.insert(
                id,
                Arc::new(Entry {
                    host,
                    admission,
                    lifecycle: Mutex::new(()),
                }),
            );
        }
        if registry
            .registrations()
            .filter(|r| r.binding == binding && r.enabled)
            .count()
            != entries.len()
        {
            return Err(Error::Configuration);
        }
        Ok(Self {
            registry,
            binding,
            entries: RwLock::new(entries),
        })
    }
    /// Canonical registration snapshot in stable identity order. Never returns
    /// another project's/session's registrations or discovered provider data.
    pub fn index(&self) -> Vec<ConnectorRegistration> {
        self.registry
            .registrations()
            .filter(|r| r.binding == self.binding)
            .cloned()
            .collect()
    }
    /// Validate/build before revocation. Invalid replacement leaves the running
    /// candidate intact; valid replacement cannot reuse old sessions/inventories.
    pub fn rebuild(
        &mut self,
        registry: ConnectorRegistry,
        plugins: Vec<PluginConfig>,
    ) -> Result<(), Error> {
        let replacement = Self::new(registry, self.binding.clone(), plugins)?;
        let previous = std::mem::replace(self, replacement);
        drop(previous);
        Ok(())
    }
    /// Irreversible for this generation, including previously cloned admissions.
    /// Re-add requires an explicit canonical rebuild.
    pub fn remove(&self, target: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        self.check_scope(target)?;
        let entry = self
            .entries
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(target.connector.as_str())
            .ok_or_else(|| self.failure(target, ConnectorFailureCode::NotFound))?;
        entry.admission.revoke();
        entry
            .host
            .lifecycle(target, ConnectorLifecycleCommand::Disable)?;
        Ok(())
    }
    /// Runtime enable is permitted only for a canonical enabled registration.
    /// Shutdown clears the old inventory before opening the admission gate.
    pub fn enable(&self, target: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        let entry = self.entry(target)?;
        let _serial = entry.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        entry.admission.disable();
        entry
            .host
            .lifecycle(target, ConnectorLifecycleCommand::Shutdown)?;
        entry
            .admission
            .enable()
            .map_err(|_| self.failure(target, ConnectorFailureCode::PermissionDenied))
    }
    fn failure(&self, target: &ConnectorTarget, code: ConnectorFailureCode) -> ConnectorFailure {
        ConnectorFailure {
            code,
            effect: ConnectorEffect::NotDispatched,
            correlation: target.connector.clone(),
        }
    }
    fn check_scope(&self, target: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        if target.scope != self.binding.scope {
            return Err(self.failure(target, ConnectorFailureCode::ScopeMismatch));
        }
        Ok(())
    }
    fn entry(&self, target: &ConnectorTarget) -> Result<Arc<Entry>, ConnectorFailure> {
        self.check_scope(target)?;
        self.entries
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(target.connector.as_str())
            .cloned()
            .ok_or_else(|| self.failure(target, ConnectorFailureCode::NotFound))
    }
}
impl ConnectorApplicationPort for PluginLifecycleManager {
    fn discover(&self, target: &ConnectorTarget) -> Result<ConnectorInventory, ConnectorFailure> {
        self.entry(target)?.host.discover(target)
    }
    fn invoke(&self, request: &ConnectorRequest) -> Result<ConnectorObservation, ConnectorFailure> {
        self.entry(&request.target)?.host.invoke(request)
    }
    fn read_resource(
        &self,
        request: &ConnectorRequest,
        source: &SourceId,
    ) -> Result<ConnectorObservation, ConnectorFailure> {
        self.entry(&request.target)?
            .host
            .read_resource(request, source)
    }
    fn lifecycle(
        &self,
        target: &ConnectorTarget,
        command: ConnectorLifecycleCommand,
    ) -> Result<ConnectorLifecycleState, ConnectorFailure> {
        let entry = self.entry(target)?;
        let _serial = entry.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        if matches!(
            command,
            ConnectorLifecycleCommand::Disable | ConnectorLifecycleCommand::Shutdown
        ) {
            entry.admission.disable();
        }
        entry.host.lifecycle(target, command)
    }
}
impl Drop for PluginLifecycleManager {
    fn drop(&mut self) {
        let entries = self.entries.get_mut().unwrap_or_else(|e| e.into_inner());
        // Revoke all targets before any potentially slow transport cleanup.
        for entry in entries.values() {
            entry.admission.revoke();
        }
        for entry in entries.values() {
            let _ = entry.host.lifecycle(
                entry.host.configured_target(),
                ConnectorLifecycleCommand::Disable,
            );
        }
    }
}
