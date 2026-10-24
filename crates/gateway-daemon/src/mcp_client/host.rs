use super::{protocol::Session, transport::TransportFactory, *};
pub use gateway_application::connectors::ConnectorAdmissionPort as ConnectorAdmission;
use gateway_application::{
    connector_manifest::{CompatibilityDecision, ConnectorCompatibilityProfile, ConnectorManifest},
    connectors::*,
};
use gateway_domain::{SourceId, TypedValue};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Mutex, MutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};

/// Adapter-owned translation only. Cannot grant authority or call the private session.
/// #227 owns canonical mapping, #228 owns evidence normalization. Both must fail
/// closed for unrecognized schemas and preserve the supplied request bindings.
pub trait ConnectorMapping: Send + Sync {
    fn inventory(
        &self,
        target: &ConnectorTarget,
        manifest: &ConnectorManifest,
        wire: &ProtocolInventory,
    ) -> Result<ConnectorInventory, Error>;
    fn operation(
        &self,
        dispatch: &ConnectorDispatch,
        source: Option<&SourceId>,
        wire: &ProtocolInventory,
    ) -> Result<WireOperation, Error>;
    fn normalize(
        &self,
        dispatch: &ConnectorDispatch,
        source: Option<&SourceId>,
        result: ProtocolResult,
    ) -> Result<ConnectorObservation, Error>;
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireOperation {
    Tool {
        name: String,
        arguments: BTreeMap<NonEmptyText, TypedValue>,
    },
    Resource {
        uri: String,
    },
}
/// Bounded untrusted result envelope; adapters validate payload schemas before
/// constructing candidate observations. Tool errors remain explicit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolResult {
    Tool {
        content: Vec<Value>,
        structured: Option<Value>,
        is_error: bool,
    },
    Resource {
        contents: Vec<Value>,
    },
}
struct Active<'a>(&'a Mutex<Option<CallContext>>);
impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).take();
    }
}
struct State {
    session: Session,
    canonical: Option<ConnectorInventory>,
}
struct Runtime {
    state: Mutex<State>,
    target: ConnectorTarget,
    manifest: ConnectorManifest,
    factory: Box<dyn TransportFactory>,
    admission: Arc<dyn ConnectorAdmission>,
    mapping: Box<dyn ConnectorMapping>,
    limits: Limits,
    disabled: AtomicBool,
    active: Mutex<Option<CallContext>>,
}
/// Trusted composition root. No public runtime/transport handle or raw dispatch.
/// Implements the versioned driving port. Construct only with operator-owned
/// configuration and application-owned authority, mapping and normalizer.
pub struct HostConfig {
    pub target: ConnectorTarget,
    pub manifest: ConnectorManifest,
    pub profile: ConnectorCompatibilityProfile,
    pub limits: Limits,
}
/// Effect methods on the session are inaccessible to application callers.
/// ```compile_fail
/// fn bypass(host: gateway_daemon::mcp_client::McpConnectorHost) {
///     let _ = host.runtime;
/// }
/// ```
pub struct McpConnectorHost {
    runtime: Runtime,
}
impl McpConnectorHost {
    pub fn new(
        config: HostConfig,
        factory: Box<dyn TransportFactory>,
        admission: Arc<dyn ConnectorAdmission>,
        mapping: Box<dyn ConnectorMapping>,
        observer: Arc<dyn Observer>,
    ) -> Result<Self, Error> {
        let HostConfig {
            target,
            manifest,
            profile,
            limits,
        } = config;
        limits.validate()?;
        if target.connector.as_str() != manifest.connector_id()
            || !matches!(
                manifest.compatibility(&profile),
                CompatibilityDecision::Compatible | CompatibilityDecision::CompatibleDeprecated
            )
        {
            return Err(Error::Configuration);
        }
        Ok(Self {
            runtime: Runtime {
                state: Mutex::new(State {
                    session: Session::new(limits.clone(), observer, {
                        let version = manifest.definition().server_version;
                        format!("{}.{}.{}", version.major, version.minor, version.patch)
                    }),
                    canonical: None,
                }),
                target,
                manifest,
                factory,
                admission,
                mapping,
                limits,
                disabled: AtomicBool::new(false),
                active: Mutex::new(None),
            },
        })
    }
    /// Host-provided cancellation handle; its deadline is narrowed by dispatch and
    /// configured bounds. Callers cannot extend an admitted budget.
    pub fn invoke_with_context(
        &self,
        request: &ConnectorRequest,
        source: Option<&SourceId>,
        context: &CallContext,
    ) -> Result<ConnectorObservation, ConnectorFailure> {
        context
            .check()
            .map_err(|e| e.failure(&request.correlation, ConnectorEffect::NotDispatched))?;
        self.runtime
            .check_target(&request.target, &context.correlation)?;
        if context.correlation != request.correlation {
            return Err(
                Error::Configuration.failure(&request.correlation, ConnectorEffect::NotDispatched)
            );
        }
        let dispatch = self.runtime.admission.authorize(request, source)?;
        if dispatch.request != *request {
            return Err(
                Error::Configuration.failure(&request.correlation, ConnectorEffect::NotDispatched)
            );
        }
        self.runtime.dispatch(&dispatch, source, context)
    }
    pub fn session_state(&self) -> Result<SessionState, Error> {
        self.runtime
            .state
            .try_lock()
            .map(|s| s.session.state)
            .map_err(|_| Error::InvalidState)
    }
}
impl Runtime {
    fn activate(&self, context: &CallContext) -> Active<'_> {
        *self.active.lock().unwrap_or_else(|e| e.into_inner()) = Some(context.clone());
        if self.disabled.load(Ordering::Acquire) {
            context.cancel();
        }
        Active(&self.active)
    }
    fn context(&self, correlation: NonEmptyText) -> CallContext {
        CallContext::new(correlation, Instant::now() + self.limits.call_timeout)
    }
    fn check_target(
        &self,
        target: &ConnectorTarget,
        correlation: &NonEmptyText,
    ) -> Result<(), ConnectorFailure> {
        if *target != self.target {
            return Err(ConnectorFailure {
                code: ConnectorFailureCode::ScopeMismatch,
                effect: ConnectorEffect::NotDispatched,
                correlation: correlation.clone(),
            });
        }
        self.admission.admit_target(target)
    }
    fn lock(&self, context: &CallContext) -> Result<MutexGuard<'_, State>, ConnectorFailure> {
        context
            .check()
            .map_err(|e| e.failure(&context.correlation, ConnectorEffect::NotDispatched))?;
        self.state.try_lock().map_err(|_| ConnectorFailure {
            code: ConnectorFailureCode::Backpressure,
            effect: ConnectorEffect::NotDispatched,
            correlation: context.correlation.clone(),
        })
    }
    fn dispatch(
        &self,
        dispatch: &ConnectorDispatch,
        source: Option<&SourceId>,
        supplied: &CallContext,
    ) -> Result<ConnectorObservation, ConnectorFailure> {
        let request = &dispatch.request;
        let mut context = supplied.clone();
        let remaining = u64::try_from(dispatch.deadline.seconds())
            .ok()
            .and_then(|s| UNIX_EPOCH.checked_add(Duration::from_secs(s)))
            .and_then(|d| d.duration_since(SystemTime::now()).ok());
        if remaining.is_none_or(|d| d.is_zero()) {
            return Err(
                Error::Timeout.failure(&request.correlation, ConnectorEffect::NotDispatched)
            );
        }
        context.deadline = context
            .deadline
            .min(Instant::now() + remaining.unwrap_or_default().min(self.limits.call_timeout));
        self.check_target(&request.target, &request.correlation)?;
        let mut state = self.lock(&context)?;
        state.session.sent = false;
        if self.disabled.load(Ordering::Acquire) {
            return Err(
                Error::InvalidState.failure(&request.correlation, ConnectorEffect::NotDispatched)
            );
        }
        let _active = self.activate(&context);
        let result = state.session.refresh(&context);
        if let Err(error) = result {
            state.canonical = None;
            if error != Error::InvalidState {
                state.session.failed(error, &context);
            }
            return Err(error.failure(&request.correlation, ConnectorEffect::NotDispatched));
        }
        let result = self.prepare(&state, dispatch, source);
        let operation =
            result.map_err(|e| e.failure(&request.correlation, ConnectorEffect::NotDispatched))?;
        self.admission.revalidate(dispatch, source)?;
        context
            .check()
            .map_err(|e| e.failure(&request.correlation, ConnectorEffect::NotDispatched))?;
        let result = self.execute(&mut state.session, operation, &context);
        let effect = if state.session.sent
            && matches!(
                request.class,
                ConnectorOperationClass::Mutate | ConnectorOperationClass::Admin
            ) {
            ConnectorEffect::Unknown
        } else if state.session.sent {
            ConnectorEffect::NoMutation
        } else {
            ConnectorEffect::NotDispatched
        };
        match result {
            Ok(result) => {
                let observation = self
                    .mapping
                    .normalize(dispatch, source, result)
                    .map_err(|e| e.failure(&request.correlation, effect))?;
                context
                    .check()
                    .map_err(|e| e.failure(&request.correlation, effect))?;
                if observation.records.validate().is_err()
                    || observation.target != request.target
                    || observation.inventory_revision != request.inventory_revision
                    || observation.correlation != request.correlation
                    || source.is_some_and(|s| *s != observation.source)
                {
                    return Err(Error::Schema.failure(&request.correlation, effect));
                }
                Ok(observation)
            }
            Err(error) => {
                state.canonical = None;
                state.session.failed(error, &context);
                Err(error.failure(&request.correlation, effect))
            }
        }
    }
    fn prepare(
        &self,
        state: &State,
        dispatch: &ConnectorDispatch,
        source: Option<&SourceId>,
    ) -> Result<WireOperation, Error> {
        state.session.ready()?;
        let inventory = state.canonical.as_ref().ok_or(Error::InventoryChanged)?;
        let request = &dispatch.request;
        if inventory.revision != request.inventory_revision {
            return Err(Error::InventoryChanged);
        }
        if !inventory
            .capabilities
            .iter()
            .any(|c| c.capability == request.capability && c.class == request.class)
            || source.is_some_and(|s| !inventory.resources.contains(s))
        {
            return Err(Error::Schema);
        }
        let wire = state
            .session
            .inventory
            .as_ref()
            .ok_or(Error::InventoryChanged)?;
        let operation = self.mapping.operation(dispatch, source, wire)?;
        match &operation {
            WireOperation::Tool { arguments, .. }
                if source.is_none() && *arguments == request.arguments => {}
            WireOperation::Resource { .. } if source.is_some() => (),
            _ => return Err(Error::Schema),
        }
        Ok(operation)
    }
    fn execute(
        &self,
        session: &mut Session,
        operation: WireOperation,
        context: &CallContext,
    ) -> Result<ProtocolResult, Error> {
        let inventory = session.inventory.as_ref().ok_or(Error::InventoryChanged)?;
        match operation {
            WireOperation::Tool { name, arguments } => {
                let tool = inventory
                    .tools
                    .iter()
                    .find(|t| t["name"].as_str() == Some(&name))
                    .ok_or(Error::Schema)?;
                let input_schema = tool["inputSchema"].clone();
                let output_schema = tool.get("outputSchema").cloned();
                let arguments: serde_json::Map<String, Value> = arguments
                    .iter()
                    .map(|(key, value)| Ok((key.as_str().to_owned(), typed(value)?)))
                    .collect::<Result<_, Error>>()?;
                super::schema::accepts(&input_schema, &Value::Object(arguments.clone()))?;
                let result = session.rpc(
                    "tools/call",
                    json!({"name":name,"arguments":arguments}),
                    context,
                )?;
                let content = result["content"].as_array().ok_or(Error::Schema)?.clone();
                let is_error = match result.get("isError") {
                    None => false,
                    Some(Value::Bool(b)) => *b,
                    _ => return Err(Error::Schema),
                };
                let structured = result.get("structuredContent").cloned();
                if structured.as_ref().is_some_and(|s| !s.is_object()) {
                    return Err(Error::Schema);
                }
                if let Some(schema) = output_schema
                    && !is_error
                {
                    super::schema::accepts(&schema, structured.as_ref().ok_or(Error::Schema)?)?;
                }
                Ok(ProtocolResult::Tool {
                    content,
                    structured,
                    is_error,
                })
            }
            WireOperation::Resource { uri } => {
                // Template expansion is deferred to explicit mapping qualification.
                if !inventory
                    .resources
                    .iter()
                    .any(|r| r["uri"].as_str() == Some(&uri))
                {
                    return Err(Error::Schema);
                }
                let result = session.rpc("resources/read", json!({"uri":uri}), context)?;
                let contents = result["contents"].as_array().ok_or(Error::Schema)?.clone();
                if contents.iter().any(|c| {
                    c["uri"].as_str() != Some(&uri)
                        || c.get("text").is_some() == c.get("blob").is_some()
                        || !c
                            .get("text")
                            .or_else(|| c.get("blob"))
                            .is_some_and(Value::is_string)
                }) {
                    return Err(Error::Schema);
                }
                Ok(ProtocolResult::Resource { contents })
            }
        }
    }
}
fn typed(value: &TypedValue) -> Result<Value, Error> {
    value.validate().map_err(|_| Error::Schema)?;
    match value {
        TypedValue::Boolean(v) => Ok(json!(v)),
        TypedValue::Integer(v) => Ok(json!(v)),
        TypedValue::String(v) => Ok(json!(v.as_str())),
        TypedValue::Symbol(v) => Ok(json!(v.as_str())),
        // Decimal/collection schemas require an explicitly qualified canonical mapping.
        TypedValue::Decimal(_) | TypedValue::Set(_) => Err(Error::Schema),
    }
}
impl ConnectorApplicationPort for McpConnectorHost {
    fn discover(&self, target: &ConnectorTarget) -> Result<ConnectorInventory, ConnectorFailure> {
        self.runtime.discover(target)
    }
    fn invoke(&self, request: &ConnectorRequest) -> Result<ConnectorObservation, ConnectorFailure> {
        self.invoke_with_context(
            request,
            None,
            &self.runtime.context(request.correlation.clone()),
        )
    }
    fn read_resource(
        &self,
        request: &ConnectorRequest,
        source: &SourceId,
    ) -> Result<ConnectorObservation, ConnectorFailure> {
        self.invoke_with_context(
            request,
            Some(source),
            &self.runtime.context(request.correlation.clone()),
        )
    }
    fn lifecycle(
        &self,
        target: &ConnectorTarget,
        command: ConnectorLifecycleCommand,
    ) -> Result<ConnectorLifecycleState, ConnectorFailure> {
        self.runtime.lifecycle(target, command)
    }
}
impl ConnectorRuntimePort for Runtime {
    fn invoke(
        &self,
        dispatch: &ConnectorDispatch,
    ) -> Result<ConnectorObservation, ConnectorFailure> {
        self.dispatch(
            dispatch,
            None,
            &self.context(dispatch.request.correlation.clone()),
        )
    }
    fn read_resource(
        &self,
        dispatch: &ConnectorDispatch,
        source: &SourceId,
    ) -> Result<ConnectorObservation, ConnectorFailure> {
        self.dispatch(
            dispatch,
            Some(source),
            &self.context(dispatch.request.correlation.clone()),
        )
    }
    fn discover(&self, target: &ConnectorTarget) -> Result<ConnectorInventory, ConnectorFailure> {
        let context = self.context(target.connector.clone());
        self.check_target(target, &context.correlation)?;
        let mut state = self.lock(&context)?;
        let _active = self.activate(&context);
        state.canonical = None;
        let result = state
            .session
            .discover(&context)
            .and_then(|wire| self.mapping.inventory(target, &self.manifest, &wire))
            .and_then(|inventory| {
                context.check()?;
                Ok(inventory)
            });
        match result {
            Ok(inventory)
                if inventory.target == *target
                    && inventory.capabilities.len() <= self.limits.items
                    && inventory.resources.len() <= self.limits.items
                    && inventory
                        .capabilities
                        .windows(2)
                        .all(|p| p[0].capability < p[1].capability)
                    && inventory.resources.windows(2).all(|p| p[0] < p[1]) =>
            {
                state.canonical = Some(inventory.clone());
                Ok(inventory)
            }
            Ok(_) => {
                Err(Error::Schema.failure(&context.correlation, ConnectorEffect::NotDispatched))
            }
            Err(error) => {
                state.session.failed(error, &context);
                Err(error.failure(&context.correlation, ConnectorEffect::NotDispatched))
            }
        }
    }
    fn lifecycle(
        &self,
        target: &ConnectorTarget,
        command: ConnectorLifecycleCommand,
    ) -> Result<ConnectorLifecycleState, ConnectorFailure> {
        let context = self.context(target.connector.clone());
        self.check_target(target, &context.correlation)?;
        if matches!(
            command,
            ConnectorLifecycleCommand::Disable | ConnectorLifecycleCommand::Shutdown
        ) {
            self.disabled.store(true, Ordering::Release);
            if let Some(active) = self
                .active
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
            {
                active.cancel();
            }
        }
        let draining = matches!(
            command,
            ConnectorLifecycleCommand::Disable | ConnectorLifecycleCommand::Shutdown
        );
        let until = Instant::now() + self.limits.shutdown * 4;
        let mut state = loop {
            match self.lock(&context) {
                Ok(state) => break state,
                Err(failure)
                    if draining
                        && failure.code == ConnectorFailureCode::Backpressure
                        && Instant::now() < until =>
                {
                    std::thread::sleep(Duration::from_millis(1))
                }
                Err(failure) => return Err(failure),
            }
        };
        let _active = if draining {
            None
        } else {
            Some(self.activate(&context))
        };
        let result = match command {
            ConnectorLifecycleCommand::Connect => {
                if self.disabled.load(Ordering::Acquire) {
                    return Err(Error::InvalidState
                        .failure(&context.correlation, ConnectorEffect::NotDispatched));
                }
                state
                    .session
                    .connect(self.factory.as_ref(), &context)
                    .map(|_| ConnectorLifecycleState::Ready)
            }
            ConnectorLifecycleCommand::Health => state
                .session
                .ready()
                .and_then(|_| state.session.rpc("ping", json!({}), &context))
                .map(|_| ConnectorLifecycleState::Ready),
            ConnectorLifecycleCommand::Disable | ConnectorLifecycleCommand::Shutdown => {
                state.canonical = None;
                let (inner, outer) = if command == ConnectorLifecycleCommand::Disable {
                    (SessionState::Disabled, ConnectorLifecycleState::Disabled)
                } else {
                    self.disabled.store(false, Ordering::Release);
                    (
                        SessionState::Disconnected,
                        ConnectorLifecycleState::Disconnected,
                    )
                };
                state.session.close(inner, &context);
                Ok(outer)
            }
        };
        result.map_err(|error| {
            state.canonical = None;
            if error != Error::InvalidState && state.session.state != SessionState::Degraded {
                state.session.failed(error, &context);
            }
            error.failure(&context.correlation, ConnectorEffect::NotDispatched)
        })
    }
}

#[cfg(test)]
#[path = "host_tests.rs"]
mod tests;
