//! Operation authorization uses CG policy and trusted, request-scoped host facts.
use super::{Call, FacadeError};
use gateway_domain::{CapabilityClass, CapabilityId, ExecutionProfile, OperatingMode, PlanStepId};
use gateway_policy::{
    PolicyAuthority, PolicyDecision, PolicyEngine, ProcessReadiness, StepFacts, StepPolicyInput,
    StepPolicyReport,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationClass {
    Read,
    Search,
    Inspect,
    Mutate,
    Admin,
}
impl OperationClass {
    pub fn capability_class(self) -> CapabilityClass {
        match self {
            Self::Read | Self::Search | Self::Inspect => CapabilityClass::Inspect,
            Self::Mutate | Self::Admin => CapabilityClass::Mutate,
        }
    }
}

/// Closed mapping owned by CG, never by tool annotations or caller input.
pub fn operation_class(operation: &str) -> Result<OperationClass, FacadeError> {
    match operation {
        "resource.read" => Ok(OperationClass::Read),
        "situation.inspect"
        | "situation.assess"
        | "capabilities.resolve"
        | "state.explain"
        | "context.compile"
        | "evidence.inspect"
        | "registry.inspect"
        | "session.inspect" => Ok(OperationClass::Inspect),
        "session.start" | "session.approve" | "session.cancel" | "session.clarify"
        | "session.continue" => Ok(OperationClass::Mutate),
        _ => Err(FacadeError::UnsupportedCapability),
    }
}
pub fn operation_capability(operation: &str) -> Result<CapabilityId, FacadeError> {
    operation_class(operation)?;
    CapabilityId::new(format!("cg.{operation}")).map_err(|_| FacadeError::Internal)
}

/// Output of the trusted governance/consent owner; deliberately not deserializable.
/// Consent records in tool input are references to validate, never grants.
pub struct OperationPolicy {
    pub authority: PolicyAuthority,
    pub facts: StepFacts,
    pub process: ProcessReadiness,
    pub operating_mode: OperatingMode,
    pub execution_profile: ExecutionProfile,
    /// Explicit runtime enablement is necessary in addition to policy for writes.
    pub mutations_enabled: bool,
}
impl OperationPolicy {
    pub fn evaluate(&self, call: &Call) -> Result<StepPolicyReport, FacadeError> {
        let class = operation_class(&call.operation)?;
        let id = operation_capability(&call.operation)?;
        let capability = self
            .authority
            .capabilities
            .get(&id)
            .cloned()
            .unwrap_or_else(|| {
                gateway_domain::CapabilityDefinition::new(id.clone(), class.capability_class())
            });
        let contract_matches =
            capability.id() == &id && capability.class() == class.capability_class();
        let capabilities = BTreeMap::from([(id, capability)]);
        let step = PlanStepId::new(call.operation.clone()).map_err(|_| FacadeError::Internal)?;
        let empty = BTreeSet::new();
        let mut report = PolicyEngine::evaluate(
            &self.authority,
            &StepPolicyInput {
                step: &step,
                capabilities: &capabilities,
                operating_mode: self.operating_mode,
                execution_profile: self.execution_profile,
                process: self.process,
                resolved: true,
                has_prerequisites: false,
                constraints: &empty,
                preconditions: &empty,
                facts: &self.facts,
            },
        );
        use gateway_policy::{PolicyFinding, PolicyReason};
        let mut deny = |reason| {
            report.findings.insert(PolicyFinding {
                decision: PolicyDecision::Deny,
                reason,
                subject: call.operation.clone(),
            });
            report.decision = PolicyDecision::Deny;
        };
        if call.operating_mode != self.operating_mode
            || call.execution_profile != self.execution_profile
        {
            deny(PolicyReason::InvalidExecutionProfile);
        }
        if class.capability_class() == CapabilityClass::Mutate && !self.mutations_enabled {
            deny(PolicyReason::AuthorizationDenied);
        }
        if !contract_matches {
            deny(PolicyReason::ContractMismatch);
        }
        if report.decision != PolicyDecision::Allow {
            report
                .findings
                .retain(|f| f.reason != PolicyReason::Allowed);
        }
        Ok(report)
    }
}
pub fn decision_result(report: &StepPolicyReport) -> Result<(), FacadeError> {
    match report.decision {
        PolicyDecision::Allow => Ok(()),
        PolicyDecision::Deny => Err(FacadeError::PolicyDenied),
        PolicyDecision::RequireConsent => Err(FacadeError::ConsentRequired),
        PolicyDecision::RequireEvidence => Err(FacadeError::EvidenceRequired),
    }
}
