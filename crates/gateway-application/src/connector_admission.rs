//! Operator-owned connector allowlist. Discovery and consuming-project input
//! cannot modify this immutable, rebuildable authority snapshot.
use crate::{connector_manifest::*, connectors::*};
use gateway_domain::{ContextScopeId, NonEmptyText, SourceId};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// Authenticated host identity, captured independently of connector proposals.
/// None is an exact value, never a wildcard or inherited global permission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorScopeBinding {
    pub project: NonEmptyText,
    pub scope: ContextScopeId,
    pub session: Option<NonEmptyText>,
    pub request: Option<NonEmptyText>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorRegistration {
    pub binding: ConnectorScopeBinding,
    pub manifest: ConnectorManifest,
    pub enabled: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionError {
    DuplicateIdentity,
    ConflictingIdentity,
    Incompatible,
    ScopeMismatch,
    NotAdmitted,
}

/// Derived index only; no runtime connection, discovered payload or credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorRegistry {
    entries: BTreeMap<(ContextScopeId, String), ConnectorRegistration>,
}
impl ConnectorRegistry {
    /// Validate the entire canonical configuration before publishing any entry.
    /// Same server in different scopes requires separate explicit registrations.
    pub fn rebuild(
        registrations: Vec<ConnectorRegistration>,
        profile: &ConnectorCompatibilityProfile,
    ) -> Result<Self, AdmissionError> {
        let mut entries = BTreeMap::new();
        let mut scopes = BTreeMap::new();
        let mut servers = BTreeMap::new();
        for registration in registrations {
            let m = &registration.manifest;
            // Disabled canonical entries are retained, but still need compatible
            // protocol/gateway/transport metadata. Manifest Disabled is allowed
            // only when the canonical registration is also disabled.
            let compatible = m.compatibility(profile);
            if !matches!(
                compatible,
                CompatibilityDecision::Compatible | CompatibilityDecision::CompatibleDeprecated
            ) && !(compatible == CompatibilityDecision::Disabled && !registration.enabled)
            {
                return Err(AdmissionError::Incompatible);
            }
            let binding = &registration.binding;
            if scopes
                .insert(binding.scope.clone(), binding.clone())
                .is_some_and(|old| old != *binding)
            {
                return Err(AdmissionError::ScopeMismatch);
            }
            let key = (binding.scope.clone(), m.connector_id().to_owned());
            if let Some(old) = entries.get(&key) {
                let old: &ConnectorRegistration = old;
                return Err(if old == &registration {
                    AdmissionError::DuplicateIdentity
                } else {
                    AdmissionError::ConflictingIdentity
                });
            }
            if servers
                .insert(
                    (binding.scope.clone(), m.server_id().to_owned()),
                    m.connector_id().to_owned(),
                )
                .is_some()
            {
                return Err(AdmissionError::ConflictingIdentity);
            }
            entries.insert(key, registration);
        }
        Ok(Self { entries })
    }
    pub fn registrations(&self) -> impl Iterator<Item = &ConnectorRegistration> {
        self.entries.values()
    }
    pub fn admitted(
        &self,
        binding: &ConnectorScopeBinding,
        target: &ConnectorTarget,
    ) -> Result<&ConnectorRegistration, AdmissionError> {
        if target.scope != binding.scope {
            return Err(AdmissionError::ScopeMismatch);
        }
        let entry = self
            .entries
            .get(&(target.scope.clone(), target.connector.as_str().to_owned()))
            .ok_or(AdmissionError::NotAdmitted)?;
        if entry.binding != *binding {
            return Err(AdmissionError::ScopeMismatch);
        }
        if !entry.enabled {
            return Err(AdmissionError::NotAdmitted);
        }
        Ok(entry)
    }
    /// Bind existing CG-09 authority to one canonical registration. No policy
    /// method is replaced, and caller-authentication remains the host's owner.
    pub fn bind(
        &self,
        binding: &ConnectorScopeBinding,
        target: &ConnectorTarget,
        policy: Arc<dyn ConnectorAdmissionPort>,
    ) -> Result<Arc<ScopedConnectorAdmission>, AdmissionError> {
        self.admitted(binding, target)?;
        Ok(Arc::new(ScopedConnectorAdmission {
            target: target.clone(),
            policy,
            enabled: AtomicBool::new(true),
            revoked: AtomicBool::new(false),
        }))
    }
}

/// One runtime generation. Removal/rebuild revokes this generation permanently.
/// Runtime disable is reversible only while the registration remains admitted.
pub struct ScopedConnectorAdmission {
    target: ConnectorTarget,
    policy: Arc<dyn ConnectorAdmissionPort>,
    enabled: AtomicBool,
    revoked: AtomicBool,
}
impl ScopedConnectorAdmission {
    pub fn disable(&self) {
        self.enabled.store(false, Ordering::Release);
    }
    pub fn enable(&self) -> Result<(), AdmissionError> {
        if self.revoked.load(Ordering::Acquire) {
            return Err(AdmissionError::NotAdmitted);
        }
        self.enabled.store(true, Ordering::Release);
        Ok(())
    }
    pub fn revoke(&self) {
        self.revoked.store(true, Ordering::Release);
        self.disable();
    }
    fn check(
        &self,
        target: &ConnectorTarget,
        correlation: &NonEmptyText,
    ) -> Result<(), ConnectorFailure> {
        let code = if target != &self.target {
            Some(ConnectorFailureCode::ScopeMismatch)
        } else if self.revoked.load(Ordering::Acquire) || !self.enabled.load(Ordering::Acquire) {
            Some(ConnectorFailureCode::PermissionDenied)
        } else {
            None
        };
        match code {
            Some(code) => Err(ConnectorFailure {
                code,
                effect: ConnectorEffect::NotDispatched,
                correlation: correlation.clone(),
            }),
            None => Ok(()),
        }
    }
}
impl ConnectorAdmissionPort for ScopedConnectorAdmission {
    fn admit_target(&self, target: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        self.check(target, &target.connector)?;
        self.policy.admit_target(target)?;
        self.check(target, &target.connector)
    }
    fn authorize(
        &self,
        request: &ConnectorRequest,
        source: Option<&SourceId>,
    ) -> Result<ConnectorDispatch, ConnectorFailure> {
        self.check(&request.target, &request.correlation)?;
        let dispatch = self.policy.authorize(request, source)?;
        self.check(&request.target, &request.correlation)?;
        Ok(dispatch)
    }
    fn revalidate(
        &self,
        dispatch: &ConnectorDispatch,
        source: Option<&SourceId>,
    ) -> Result<(), ConnectorFailure> {
        self.check(&dispatch.request.target, &dispatch.request.correlation)?;
        self.policy.revalidate(dispatch, source)?;
        self.check(&dispatch.request.target, &dispatch.request.correlation)
    }
    fn validate_current_dispatch(
        &self,
        dispatch: &ConnectorDispatch,
        source: Option<&SourceId>,
    ) -> Result<(), ConnectorFailure> {
        self.check(&dispatch.request.target, &dispatch.request.correlation)?;
        self.policy.validate_current_dispatch(dispatch, source)?;
        self.check(&dispatch.request.target, &dispatch.request.correlation)
    }
}
