//! EPIC-04.04 provider-independent application facade. Protocol framing stays outside.
pub mod assessment;
pub mod contracts;
pub mod ports;
use crate::{
    DeclarativeSituationApplication,
    context_application::{CompileStepInput, ContextApplication, ContextApplicationError},
    resolution_application::{DeclarativeResolutionApplication, ResolutionApplicationError},
    resolution_snapshot::SnapshotError,
};
use gateway_domain::{
    ContextScopeId, DeclarativeContextSituationDocument, ExecutionProfile, OperatingMode,
};
pub use ports::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FacadeError {
    InvalidInput,
    UnsupportedVersion,
    UnsupportedCapability,
    ScopeDenied,
    ReferenceUnavailable,
    StaleRevision,
    PolicyDenied,
    SensitivityDenied,
    ConsentRequired,
    EvidenceRequired,
    ProcessBlocked,
    Internal,
    DuplicateCommand,
    InvalidSessionState,
    Cancelled,
    Timeout,
    LimitExceeded,
    OutcomeUnknown,
}
impl FacadeError {
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidInput => "CG_INVALID_INPUT",
            Self::UnsupportedVersion => "CG_UNSUPPORTED_VERSION",
            Self::UnsupportedCapability => "CG_UNSUPPORTED_CAPABILITY",
            Self::ScopeDenied => "CG_SCOPE_DENIED",
            Self::ReferenceUnavailable => "CG_REFERENCE_UNAVAILABLE",
            Self::StaleRevision => "CG_STALE_REVISION",
            Self::PolicyDenied => "CG_POLICY_DENIED",
            Self::SensitivityDenied => "CG_SENSITIVITY_DENIED",
            Self::ConsentRequired => "CG_CONSENT_REQUIRED",
            Self::EvidenceRequired => "CG_EVIDENCE_REQUIRED",
            Self::ProcessBlocked => "CG_PROCESS_BLOCKED",
            Self::Internal => "CG_INTERNAL_ERROR",
            Self::DuplicateCommand => "CG_DUPLICATE_COMMAND",
            Self::InvalidSessionState => "CG_INVALID_SESSION_STATE",
            Self::Cancelled => "CG_CANCELLED",
            Self::Timeout => "CG_TIMEOUT",
            Self::LimitExceeded => "CG_LIMIT_EXCEEDED",
            Self::OutcomeUnknown => "CG_OUTCOME_UNKNOWN",
        }
    }
}
impl From<ResolutionApplicationError> for FacadeError {
    fn from(error: ResolutionApplicationError) -> Self {
        match error {
            ResolutionApplicationError::Snapshot(SnapshotError::ScopeMismatch) => Self::ScopeDenied,
            ResolutionApplicationError::Snapshot(SnapshotError::UnsupportedVersion) => {
                Self::UnsupportedVersion
            }
            ResolutionApplicationError::Snapshot(
                SnapshotError::StaleRevision | SnapshotError::ProcessMismatch,
            ) => Self::StaleRevision,
            ResolutionApplicationError::Snapshot(
                SnapshotError::InputUnavailable | SnapshotError::MissingProcessDefinition,
            ) => Self::ReferenceUnavailable,
            ResolutionApplicationError::Artifact(
                crate::resolution_artifact::ArtifactError::StaleBasis,
            ) => Self::StaleRevision,
            _ => Self::InvalidInput,
        }
    }
}
impl From<ContextApplicationError> for FacadeError {
    fn from(error: ContextApplicationError) -> Self {
        use gateway_policy::PolicyDecision;
        match error {
            ContextApplicationError::Resolution(e) => e.into(),
            ContextApplicationError::NotAuthorized(PolicyDecision::Deny) => Self::PolicyDenied,
            ContextApplicationError::NotAuthorized(PolicyDecision::RequireConsent) => {
                Self::ConsentRequired
            }
            ContextApplicationError::NotAuthorized(PolicyDecision::RequireEvidence) => {
                Self::EvidenceRequired
            }
            ContextApplicationError::StaleMapping => Self::StaleRevision,
            ContextApplicationError::Policy(
                crate::policy_application::PolicyApplicationError::StaleContext,
            ) => Self::StaleRevision,
            ContextApplicationError::Policy(
                crate::policy_application::PolicyApplicationError::Resolution(e),
            ) => e.into(),
            ContextApplicationError::PolicyMismatch => Self::PolicyDenied,
            _ => Self::InvalidInput,
        }
    }
}

/// Validated external query with trusted canonical scope. Execution is requested
/// CG-02 depth; the host's authorize method evaluates current authority.
pub struct Call {
    pub operation: String,
    pub scope: Value,
    pub canonical_scope: ContextScopeId,
    pub operating_mode: OperatingMode,
    pub execution_profile: ExecutionProfile,
    pub input: Value,
    pub correlation: Value,
}
pub struct CodexFacade<H> {
    scope: Value,
    canonical_scope: ContextScopeId,
    host: H,
}
impl<H: CodexHost> CodexFacade<H> {
    pub fn new(
        scope: Value,
        canonical_scope: ContextScopeId,
        host: H,
    ) -> Result<Self, FacadeError> {
        let common = contracts::artifact("common.schema.json").unwrap();
        if !contracts::valid(&scope, &common["$defs"]["scope"], &common) {
            return Err(FacadeError::ScopeDenied);
        }
        Ok(Self {
            scope,
            canonical_scope,
            host,
        })
    }
    fn source(&self, call: &Call, source: &Value) -> Result<Value, FacadeError> {
        if source["kind"] == "document" {
            if source["contract_version"] != "1.0" {
                return Err(FacadeError::UnsupportedVersion);
            }
            return Ok(source["document"].clone());
        }
        self.reference(call, &source["reference"])
    }
    fn reference(&self, call: &Call, reference: &Value) -> Result<Value, FacadeError> {
        if reference["contract_version"] != "1.0" {
            return Err(FacadeError::UnsupportedVersion);
        }
        // Host checks scope and disclosure before returning existence or content.
        let record = self.host.reference(call, reference)?;
        if record.scope != call.scope {
            return Err(FacadeError::ScopeDenied);
        }
        if record.document.len() > 1_048_576 {
            return Err(FacadeError::InvalidInput);
        }
        if record.reference != *reference {
            return Err(FacadeError::StaleRevision);
        }
        let digest = format!("sha256:{:x}", Sha256::digest(record.document.as_bytes()));
        if reference["digest"] != digest {
            return Err(FacadeError::StaleRevision);
        }
        serde_json::from_str(&record.document).map_err(|_| FacadeError::InvalidInput)
    }
    fn dispatch(&self, call: &Call) -> Result<(&'static str, Value), FacadeError> {
        let app = DeclarativeSituationApplication::new();
        let resolver = DeclarativeResolutionApplication;
        let input = &call.input;
        match call.operation.as_str() {
            "situation.inspect" => {
                let wire = self.source(call, &input["situation"])?;
                let document = DeclarativeContextSituationDocument::from_json(&wire.to_string())
                    .map_err(|_| FacadeError::InvalidInput)?;
                let inspection = app
                    .inspect_situation(&document, None, None)
                    .map_err(|_| FacadeError::InvalidInput)?;
                let _explanation = app.explain_situation(&inspection);
                Ok((
                    "cg.situation",
                    parse(
                        app.serialize_situation(&document)
                            .map_err(|_| FacadeError::Internal)?,
                    )?,
                ))
            }
            "situation.assess" => {
                let wire = self.source(call, &input["situation"])?;
                let assembly: assessment::AssessmentInput =
                    serde_json::from_value(wire).map_err(|_| FacadeError::InvalidInput)?;
                if assembly.scope != call.canonical_scope {
                    return Err(FacadeError::ScopeDenied);
                }
                if assembly.operating_mode != call.operating_mode
                    || assembly.execution_profile != call.execution_profile
                {
                    return Err(FacadeError::InvalidInput);
                }
                Ok((
                    "cg.assessment",
                    serde_json::to_value(assembly.assess()?).map_err(|_| FacadeError::Internal)?,
                ))
            }
            "capabilities.resolve" => {
                let documents = ["plan", "rules", "process"]
                    .iter()
                    .map(|key| self.reference(call, &input[*key]))
                    .collect::<Result<Vec<_>, _>>()?;
                let (snapshot, rules) = self.host.resolution(call, &documents)?;
                if snapshot.scope != call.canonical_scope {
                    return Err(FacadeError::ScopeDenied);
                }
                if snapshot.operating_mode != call.operating_mode
                    || snapshot.execution_profile != call.execution_profile
                {
                    return Err(FacadeError::InvalidInput);
                }
                let resolved = resolver.resolve_plan(&snapshot, &rules)?;
                Ok((
                    "cg.resolution",
                    parse(resolver.serialize_resolution(&resolved, Default::default())?)?,
                ))
            }
            "state.explain" => {
                let document = self.reference(call, &input["resolution"])?;
                let resolved = self.host.resolved(call, &document)?;
                check_resolved(call, &resolved)?;
                validate_resolved_document(&resolved, &document)?;
                let trace = resolver.explain_resolution(
                    &resolved,
                    crate::resolution_explain::TraceLimits {
                        max_nodes: 100_000,
                        max_optional_details: 10_000,
                    },
                )?;
                Ok(("cg.resolution-trace", parse(trace.to_json())?))
            }
            "context.compile" => {
                let mut documents = vec![
                    self.reference(call, &input["resolution"])?,
                    self.reference(call, &input["projection"])?,
                ];
                for reference in input["candidates"]
                    .as_array()
                    .ok_or(FacadeError::InvalidInput)?
                {
                    documents.push(self.reference(call, reference)?);
                }
                let command = self.host.compile(call, &documents)?;
                check_resolved(call, &command.resolved)?;
                validate_resolved_document(&command.resolved, &documents[0])?;
                if command.projection.mapping.step.as_str()
                    != input["step_id"].as_str().ok_or(FacadeError::InvalidInput)?
                    || command.policy_context.operating_mode != call.operating_mode
                    || command.policy_context.execution_profile != call.execution_profile
                {
                    return Err(FacadeError::InvalidInput);
                }
                let compiled = ContextApplication.compile_step(CompileStepInput {
                    resolved: &command.resolved,
                    authority: &command.authority,
                    policy_context: &command.policy_context,
                    catalog: &command.catalog,
                    projection: &command.projection,
                    candidates: &command.candidates,
                    selected: &command.selected,
                })?;
                Ok((
                    "cg.execution-context",
                    parse(
                        compiled
                            .to_json_with_policy(command.disclosure)
                            .map_err(|_| FacadeError::Internal)?,
                    )?,
                ))
            }
            "registry.inspect" => {
                let (registry, processes) = self.host.registry(call)?;
                let index = registry
                    .capability_index()
                    .map_err(|_| FacadeError::InvalidInput)?;
                let mut entries: Vec<Value> = match input["kind"].as_str() {
                    Some("agent") => registry
                        .agents()
                        .agents()
                        .iter()
                        .map(serde_json::to_value)
                        .collect::<Result<_, _>>(),
                    Some("skill") => registry
                        .skills()
                        .skills()
                        .iter()
                        .map(serde_json::to_value)
                        .collect::<Result<_, _>>(),
                    Some("process") => processes
                        .definitions()
                        .map(serde_json::to_value)
                        .collect::<Result<_, _>>(),
                    Some("capability") => index
                        .entries()
                        .map(|e| serde_json::to_value(e.capability()))
                        .collect::<Result<_, _>>(),
                    _ => return Err(FacadeError::InvalidInput),
                }
                .map_err(|_| FacadeError::Internal)?;
                let ids = input["ids"].as_array().ok_or(FacadeError::InvalidInput)?;
                entries.retain(|e| {
                    ids.is_empty() || ids.contains(&e["id"]) || ids.contains(&e["identity"]["id"])
                });

                Ok((
                    "cg.registry",
                    json!({"kind":input["kind"], "entries":entries}),
                ))
            }
            "evidence.inspect" => {
                let documents = input["references"]
                    .as_array()
                    .ok_or(FacadeError::InvalidInput)?
                    .iter()
                    .map(|r| self.reference(call, r))
                    .collect::<Result<Vec<_>, _>>()?;
                let records = self.host.evidence(call, &documents)?;
                // Reparse through the existing validated records contract.
                let wire = records.to_json().map_err(|_| FacadeError::Internal)?;
                gateway_domain::ObservationEvidenceSet::from_json(&wire)
                    .map_err(|_| FacadeError::InvalidInput)?;
                Ok(("cg.evidence", parse(wire)?))
            }
            _ => Err(FacadeError::UnsupportedCapability),
        }
    }
    fn run(&self, operation: &str, request: &Value) -> Result<Value, &'static str> {
        if !bounded(request) {
            return Err("CG_LIMIT_EXCEEDED");
        }
        if request["schema_version"] != "1.0" {
            return Err("CG_UNSUPPORTED_VERSION");
        }
        let schema = contracts::artifact("request.schema.json").unwrap();
        let common = contracts::artifact("common.schema.json").unwrap();
        if !schema["properties"]["operation"]["enum"]
            .as_array()
            .unwrap()
            .contains(&request["operation"])
        {
            return Err("CG_UNKNOWN_OPERATION");
        }
        if request["operation"] != operation || !contracts::valid(request, &schema, &common) {
            return Err("CG_INVALID_REQUEST");
        }
        if request["scope"] != self.scope {
            return Err("CG_SCOPE_DENIED");
        }
        let call = Call {
            operation: operation.into(),
            scope: self.scope.clone(),
            canonical_scope: self.canonical_scope.clone(),
            operating_mode: request["execution"]["operating_mode"]
                .as_str()
                .unwrap()
                .parse()
                .map_err(|_| "CG_INVALID_REQUEST")?,
            execution_profile: request["execution"]["execution_profile"]
                .as_str()
                .unwrap()
                .parse()
                .map_err(|_| "CG_INVALID_REQUEST")?,
            input: request["input"].clone(),
            correlation: request["correlation"].clone(),
        };
        self.host.authorize(&call).map_err(FacadeError::code)?;
        let (result, explainability, evidence, provenance) = if operation.starts_with("session.") {
            (
                self.host.session(&call).map_err(FacadeError::code)?,
                vec![],
                vec![],
                vec![],
            )
        } else {
            let (contract, document) = self.dispatch(&call).map_err(FacadeError::code)?;
            let projection = self
                .host
                .project(&call, contract, document)
                .map_err(FacadeError::code)?;
            // The host must keep secrets reference-only. Invalid projection fails closed.
            if projection.source["kind"] == "document"
                && projection
                    .provenance
                    .iter()
                    .any(|p| p["sensitivity"] == "SECRET")
            {
                return Err("CG_SENSITIVITY_DENIED");
            }
            let actual_contract = if projection.source["kind"] == "document" {
                &projection.source["contract"]
            } else {
                &projection.source["reference"]["contract"]
            };
            if actual_contract != contract {
                return Err("CG_INTERNAL_ERROR");
            }
            (
                json!({"kind":"query", "canonical_result":projection.source}),
                projection.explainability,
                projection.evidence,
                projection.provenance,
            )
        };
        let response = json!({"schema_version":"1.0", "scope":self.scope, "operation":operation, "correlation":call.correlation,
            "status":"ok", "result":result, "explainability":explainability, "evidence":evidence, "provenance":provenance, "diagnostics":[]});
        if !contracts::valid(
            &response,
            &contracts::artifact("response.schema.json").unwrap(),
            &common,
        ) {
            return Err("CG_INTERNAL_ERROR");
        }
        Ok(response)
    }
}
impl<H: CodexHost> CodexApplicationPort for CodexFacade<H> {
    fn execute(&self, operation: &str, request: &Value) -> Value {
        self.run(operation, request)
            .unwrap_or_else(contracts::failure)
    }
}
fn parse(text: String) -> Result<Value, FacadeError> {
    serde_json::from_str(&text).map_err(|_| FacadeError::Internal)
}
fn check_resolved(
    call: &Call,
    resolved: &crate::resolution_application::ResolvedPlan,
) -> Result<(), FacadeError> {
    let input = resolved.snapshot.input();
    if input.scope != call.canonical_scope {
        return Err(FacadeError::ScopeDenied);
    }
    if input.operating_mode != call.operating_mode
        || input.execution_profile != call.execution_profile
    {
        return Err(FacadeError::InvalidInput);
    }
    Ok(())
}

impl CodexFacade<UnavailableHost> {
    pub fn unavailable(scope: Value) -> Self {
        let canonical_scope =
            ContextScopeId::new(scope["project_id"].as_str().expect("trusted scope"))
                .expect("trusted scope ID");
        Self::new(scope, canonical_scope, UnavailableHost).expect("trusted scope")
    }
}

fn validate_resolved_document(
    resolved: &crate::resolution_application::ResolvedPlan,
    document: &Value,
) -> Result<(), FacadeError> {
    let canonical = parse(
        DeclarativeResolutionApplication.serialize_resolution(resolved, Default::default())?,
    )?;
    if canonical != *document {
        return Err(FacadeError::InvalidInput);
    }
    Ok(())
}

fn bounded(value: &Value) -> bool {
    let mut pending = vec![(value, 0usize)];
    let mut nodes = 0;
    while let Some((value, depth)) = pending.pop() {
        nodes += 1;
        if depth > 64 || nodes > 200_000 {
            return false;
        }
        match value {
            Value::Array(values) => pending.extend(values.iter().map(|v| (v, depth + 1))),
            Value::Object(values) => pending.extend(values.values().map(|v| (v, depth + 1))),
            _ => {}
        }
    }
    value.to_string().len() < 1_048_576
}
