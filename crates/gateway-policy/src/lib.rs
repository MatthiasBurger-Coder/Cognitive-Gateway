//! Deterministic authorization over trusted governance and explicit step facts.
//! Callers authenticate governance/facts; plans and retrieved text cannot supply them.
#![forbid(unsafe_code)]

use gateway_domain::{
    CapabilityClass, CapabilityDefinition, CapabilityId, Constraint, ConstraintKind,
    ExecutionProfile, OperatingMode, PlanStepId, PolicyDefinition,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PolicyDecision {
    Allow,
    RequireEvidence,
    RequireConsent,
    Deny,
}

/// Legacy capability-only boundary; full authorization uses [`PolicyEngine`].
pub trait PolicyEvaluator {
    fn evaluate(&self, capability: &CapabilityId) -> PolicyDecision;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PolicyReason {
    Allowed,
    ExplicitDeny,
    NotAllowlisted,
    UnknownCapability,
    ContractMismatch,
    AuthorizationMissing,
    AuthorizationDenied,
    ConsentMissing,
    ConsentDenied,
    EvidenceMissing,
    ConstraintUnsatisfied,
    ConstraintViolation,
    FeatureFrozen,
    WorkClassUnknown,
    InvalidExecutionProfile,
    ProcessBlocked,
    ProcessUnknown,
    Unresolved,
    PrerequisiteMissing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approval {
    Granted,
    Denied,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkClass {
    Feature,
    Maintenance,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessReadiness {
    Eligible,
    Blocked,
    Unknown,
    NotApplicable,
}

/// Trusted, request-scoped facts. Absence never means approval. Evidence and
/// constraint keys are exact matches, validated by the calling application.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StepFacts {
    pub authorizations: BTreeMap<CapabilityId, Approval>,
    pub consents: BTreeMap<CapabilityId, Approval>,
    pub evidence: BTreeSet<String>,
    pub satisfied_constraints: BTreeSet<String>,
    pub work_class: Option<WorkClass>,
    pub prerequisites_satisfied: bool,
}

/// Authoritative inputs loaded independently of the planner/resolver.
#[derive(Debug, Clone, Default)]
pub struct PolicyAuthority {
    pub policies: Vec<PolicyDefinition>,
    pub capabilities: BTreeMap<CapabilityId, CapabilityDefinition>,
    pub constraints: Vec<Constraint>,
    pub required_evidence: BTreeMap<CapabilityId, BTreeSet<String>>,
}

pub struct StepPolicyInput<'a> {
    pub step: &'a PlanStepId,
    pub capabilities: &'a BTreeMap<CapabilityId, CapabilityDefinition>,
    pub operating_mode: OperatingMode,
    pub execution_profile: ExecutionProfile,
    pub process: ProcessReadiness,
    pub resolved: bool,
    pub has_prerequisites: bool,
    /// Restrictions from the plan and process; these can only narrow access.
    pub constraints: &'a BTreeSet<String>,
    pub preconditions: &'a BTreeSet<String>,
    pub facts: &'a StepFacts,
}

/// Stable machine-readable findings; `subject` names a policy, capability or
/// required evidence/constraint. No raw retrieved content is copied here.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct PolicyFinding {
    pub decision: PolicyDecision,
    pub reason: PolicyReason,
    pub subject: String,
}

/// Output only: deliberately not deserializable as an authorization token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StepPolicyReport {
    pub schema_version: u16,
    #[serde(serialize_with = "serialize_step")]
    pub step: PlanStepId,
    pub decision: PolicyDecision,
    pub policies: BTreeSet<String>,
    pub capability_classes: BTreeMap<CapabilityId, String>,
    pub findings: BTreeSet<PolicyFinding>,
}

#[derive(Debug, Default)]
pub struct PolicyEngine;
impl PolicyEngine {
    #[must_use]
    pub fn evaluate(authority: &PolicyAuthority, input: &StepPolicyInput<'_>) -> StepPolicyReport {
        let mut findings = BTreeSet::new();
        let mut add = |decision, reason, subject: &str| {
            findings.insert(PolicyFinding {
                decision,
                reason,
                subject: subject.to_owned(),
            });
        };
        if !input.resolved {
            add(
                PolicyDecision::Deny,
                PolicyReason::Unresolved,
                input.step.as_str(),
            );
        }
        match input.process {
            ProcessReadiness::Blocked => add(
                PolicyDecision::Deny,
                PolicyReason::ProcessBlocked,
                input.step.as_str(),
            ),
            ProcessReadiness::Unknown => add(
                PolicyDecision::RequireEvidence,
                PolicyReason::ProcessUnknown,
                input.step.as_str(),
            ),
            ProcessReadiness::Eligible | ProcessReadiness::NotApplicable => {}
        }
        if input.has_prerequisites && !input.facts.prerequisites_satisfied {
            add(
                PolicyDecision::RequireEvidence,
                PolicyReason::PrerequisiteMissing,
                input.step.as_str(),
            );
        }
        for constraint in &authority.constraints {
            if constraint
                .validate_for(input.operating_mode, input.execution_profile)
                .is_err()
            {
                add(
                    PolicyDecision::Deny,
                    PolicyReason::InvalidExecutionProfile,
                    constraint.id().as_str(),
                );
            }
            if constraint.kind() == ConstraintKind::FeatureFreeze {
                match input.facts.work_class {
                    Some(WorkClass::Feature) => add(
                        PolicyDecision::Deny,
                        PolicyReason::FeatureFrozen,
                        constraint.id().as_str(),
                    ),
                    None => add(
                        PolicyDecision::RequireEvidence,
                        PolicyReason::WorkClassUnknown,
                        constraint.id().as_str(),
                    ),
                    Some(WorkClass::Maintenance) => {}
                }
            }
        }
        let mut classes = BTreeMap::new();
        let mut required_evidence = input.preconditions.clone();
        let mut constraints = input.constraints.clone();
        for (id, proposed) in input.capabilities {
            let Some(capability) = authority.capabilities.get(id) else {
                add(
                    PolicyDecision::Deny,
                    PolicyReason::UnknownCapability,
                    id.as_str(),
                );
                continue;
            };
            classes.insert(id.clone(), capability.class().as_str().to_owned());
            if capability != proposed || capability.id() != id {
                add(
                    PolicyDecision::Deny,
                    PolicyReason::ContractMismatch,
                    id.as_str(),
                );
            }
            if authority.policies.is_empty() {
                add(
                    PolicyDecision::Deny,
                    PolicyReason::NotAllowlisted,
                    id.as_str(),
                );
            }
            for policy in &authority.policies {
                if policy.denied_capability_ids().contains(id) {
                    add(
                        PolicyDecision::Deny,
                        PolicyReason::ExplicitDeny,
                        policy.id().as_str(),
                    );
                } else if !policy.allowed_capability_ids().contains(id) {
                    add(
                        PolicyDecision::Deny,
                        PolicyReason::NotAllowlisted,
                        policy.id().as_str(),
                    );
                }
            }
            match input.facts.authorizations.get(id) {
                Some(Approval::Granted) => {}
                Some(Approval::Denied) => add(
                    PolicyDecision::Deny,
                    PolicyReason::AuthorizationDenied,
                    id.as_str(),
                ),
                None => add(
                    PolicyDecision::RequireConsent,
                    PolicyReason::AuthorizationMissing,
                    id.as_str(),
                ),
            }
            if capability.class() == CapabilityClass::Mutate {
                if capability
                    .constraints()
                    .iter()
                    .any(|c| c.as_str() == "read-only")
                {
                    add(
                        PolicyDecision::Deny,
                        PolicyReason::ConstraintViolation,
                        id.as_str(),
                    );
                }
                match input.facts.consents.get(id) {
                    Some(Approval::Granted) => {}
                    Some(Approval::Denied) => add(
                        PolicyDecision::Deny,
                        PolicyReason::ConsentDenied,
                        id.as_str(),
                    ),
                    None => add(
                        PolicyDecision::RequireConsent,
                        PolicyReason::ConsentMissing,
                        id.as_str(),
                    ),
                }
            }
            required_evidence.extend(capability.preconditions().iter().map(ToString::to_string));
            required_evidence.extend(
                authority
                    .required_evidence
                    .get(id)
                    .into_iter()
                    .flatten()
                    .cloned(),
            );
            constraints.extend(capability.constraints().iter().map(ToString::to_string));
        }
        for requirement in required_evidence.difference(&input.facts.evidence) {
            add(
                PolicyDecision::RequireEvidence,
                PolicyReason::EvidenceMissing,
                requirement,
            );
        }
        for constraint in constraints.difference(&input.facts.satisfied_constraints) {
            add(
                PolicyDecision::RequireEvidence,
                PolicyReason::ConstraintUnsatisfied,
                constraint,
            );
        }
        let decision = findings
            .iter()
            .map(|f| f.decision)
            .max()
            .unwrap_or(PolicyDecision::Allow);
        if findings.is_empty() {
            findings.insert(PolicyFinding {
                decision,
                reason: PolicyReason::Allowed,
                subject: input.step.to_string(),
            });
        }
        StepPolicyReport {
            schema_version: 1,
            step: input.step.clone(),
            decision,
            policies: authority
                .policies
                .iter()
                .map(|p| p.id().to_string())
                .collect(),
            capability_classes: classes,
            findings,
        }
    }
}

fn serialize_step<S: serde::Serializer>(
    step: &PlanStepId,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(step.as_str())
}
