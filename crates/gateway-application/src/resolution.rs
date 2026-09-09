//! CG-08.01 resolution contracts. A binding is never an execution permission.
//!
//! Public records are proposals; only `ResolutionResult::new` validates their
//! structural relationship to a request. Canonical catalog revalidation is a
//! separate boundary, introduced with the immutable snapshot adapter.

use std::{collections::BTreeSet, str::FromStr};

use gateway_domain::{
    AgentId, CapabilityRequirementId, ContextScopeId, Plan, PlanId, PlanStepId, ReferenceId,
    RequirementCardinality, SchemaVersion, SituationId, SkillId,
};
use gateway_process::{DefinitionIdentity, ProcessInstanceId, ProcessInstanceRevision};
use gateway_registry::CapabilityProvider;
use sha2::{Digest, Sha256};

/// Stable structural errors; no raw context or evidence values are included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionError {
    UnsupportedVersion,
    InvalidFingerprint,
    InvalidPlan,
    InvalidReference,
    DuplicateReference,
    InvalidAlternatives,
    InconsistentOutcome,
}

/// SHA-256 content identity, not authentication or caller authorization.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ContentFingerprint(String);

impl ContentFingerprint {
    pub fn parse(value: &str) -> Result<Self, ResolutionError> {
        if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(ResolutionError::InvalidFingerprint);
        }
        Ok(Self(value.to_ascii_lowercase()))
    }

    #[must_use]
    pub fn of_bytes(bytes: &[u8]) -> Self {
        Self(format!("{:x}", Sha256::digest(bytes)))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

macro_rules! resolution_enum {
    ($name:ident { $($variant:ident => $wire:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub enum $name { $($variant),+ }
        impl $name {
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $wire),+ }
            }
        }
        impl FromStr for $name {
            type Err = ResolutionError;
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    $($wire => Ok(Self::$variant),)+
                    _ => Err(ResolutionError::InvalidReference),
                }
            }
        }
    };
}

resolution_enum!(ResolutionOutcome {
    Resolved => "RESOLVED", NoOp => "NO_OP", Missing => "MISSING",
    Ambiguous => "AMBIGUOUS", Conflicting => "CONFLICTING",
    Unsupported => "UNSUPPORTED", InvalidInput => "INVALID_INPUT",
    Partial => "PARTIAL", SearchLimit => "SEARCH_LIMIT",
});

resolution_enum!(LifecycleReadiness {
    Eligible => "ELIGIBLE", Blocked => "BLOCKED", Deferred => "DEFERRED",
    Unknown => "UNKNOWN", NotApplicable => "NOT_APPLICABLE",
});

resolution_enum!(ResolutionReason {
    ContractMatch => "CONTRACT_MATCH", UnknownCapability => "UNKNOWN_CAPABILITY",
    IncompatibleContract => "INCOMPATIBLE_CONTRACT", MissingProvider => "MISSING_PROVIDER",
    MissingDependency => "MISSING_DEPENDENCY", DependencyCycle => "DEPENDENCY_CYCLE",
    ConditionUnknown => "CONDITION_UNKNOWN", ConstraintConflict => "CONSTRAINT_CONFLICT",
    EqualAlternatives => "EQUAL_ALTERNATIVES", OptionalOmitted => "OPTIONAL_OMITTED",
    NoTemplateRequired => "NO_TEMPLATE_REQUIRED", PinnedProcess => "PINNED_PROCESS",
    LifecycleBlocked => "LIFECYCLE_BLOCKED", PredecessorPending => "PREDECESSOR_PENDING",
    UnsupportedContract => "UNSUPPORTED_CONTRACT", InvalidBasis => "INVALID_BASIS",
    SearchBudgetExhausted => "SEARCH_BUDGET_EXHAUSTED",
});

/// All semantic inputs must be pinned; CG-08.02 captures and checks content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionBasis {
    pub plan: PlanId,
    pub plan_fingerprint: ContentFingerprint,
    pub admission: Option<ReferenceId>,
    pub situation: SituationId,
    pub scope: ContextScopeId,
    pub situation_fingerprint: ContentFingerprint,
    pub registry_fingerprint: ContentFingerprint,
    pub process_catalog_fingerprint: ContentFingerprint,
    pub process_state_fingerprint: ContentFingerprint,
    pub rule_version: SchemaVersion,
}

/// An explicitly declared one-of group. Optional requirements alone do not
/// establish equivalence. Members remain the upstream typed requirements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequirementAlternatives {
    pub step: PlanStepId,
    pub members: BTreeSet<CapabilityRequirementId>,
    pub cardinality: RequirementCardinality,
}

/// Exact CG-04 definition pin and optional captured instance revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessBinding {
    pub definition: DefinitionIdentity,
    pub instance: Option<(ProcessInstanceId, ProcessInstanceRevision)>,
}

/// Candidate identity includes its exact canonical catalog content basis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderCandidate {
    pub provider: CapabilityProvider,
    pub definition_fingerprint: ContentFingerprint,
    pub reason: ResolutionReason,
}

/// Each candidate set belongs to exactly one requirement on its parent step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequirementResolution {
    pub requirement: CapabilityRequirementId,
    pub candidates: Vec<ProviderCandidate>,
    pub selected: Option<CapabilityProvider>,
    pub reason: ResolutionReason,
}

/// Agent responsibility remains explicit for each effective Skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepBinding {
    pub process: Option<ProcessBinding>,
    pub primary_agent: AgentId,
    pub participating_agents: BTreeSet<AgentId>,
    pub skills: std::collections::BTreeMap<SkillId, AgentId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepResolution {
    pub step: PlanStepId,
    pub outcome: ResolutionOutcome,
    pub readiness: LifecycleReadiness,
    pub binding: Option<StepBinding>,
    pub requirements: Vec<RequirementResolution>,
    pub reasons: BTreeSet<ResolutionReason>,
}

/// An owned immutable request. Admission metadata never grants permission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionRequest {
    plan: Plan,
    basis: ResolutionBasis,
    alternatives: Vec<RequirementAlternatives>,
}

impl ResolutionRequest {
    pub fn new(
        plan: Plan,
        basis: ResolutionBasis,
        alternatives: Vec<RequirementAlternatives>,
    ) -> Result<Self, ResolutionError> {
        if basis.rule_version != SchemaVersion::V1 {
            return Err(ResolutionError::UnsupportedVersion);
        }
        let canonical = plan.to_json().map_err(|_| ResolutionError::InvalidPlan)?;
        if basis.plan != *plan.id()
            || basis.plan_fingerprint != ContentFingerprint::of_bytes(canonical.as_bytes())
        {
            return Err(ResolutionError::InvalidPlan);
        }
        let mut grouped = BTreeSet::new();
        for group in &alternatives {
            let step = plan
                .steps()
                .iter()
                .find(|s| s.id() == &group.step)
                .ok_or(ResolutionError::InvalidReference)?;
            if group.members.len() < 2 {
                return Err(ResolutionError::InvalidAlternatives);
            }
            for member in &group.members {
                if !step.capability_requirements().contains(member) {
                    return Err(ResolutionError::InvalidReference);
                }
                if !grouped.insert((group.step.clone(), member.clone())) {
                    return Err(ResolutionError::DuplicateReference);
                }
            }
        }
        Ok(Self {
            plan,
            basis,
            alternatives,
        })
    }

    #[must_use]
    pub fn plan(&self) -> &Plan {
        &self.plan
    }
    #[must_use]
    pub fn basis(&self) -> &ResolutionBasis {
        &self.basis
    }
    #[must_use]
    pub fn alternatives(&self) -> &[RequirementAlternatives] {
        &self.alternatives
    }
}

/// Structurally validated resolution, never a policy or transition token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionResult {
    version: SchemaVersion,
    basis: ResolutionBasis,
    outcome: ResolutionOutcome,
    steps: Vec<StepResolution>,
    alternatives: Vec<RequirementAlternatives>,
}

impl ResolutionResult {
    pub fn new(
        version: SchemaVersion,
        request: &ResolutionRequest,
        outcome: ResolutionOutcome,
        mut steps: Vec<StepResolution>,
    ) -> Result<Self, ResolutionError> {
        if version != SchemaVersion::V1 {
            return Err(ResolutionError::UnsupportedVersion);
        }
        steps.sort_by(|a, b| a.step.cmp(&b.step));
        if steps.windows(2).any(|p| p[0].step == p[1].step) {
            return Err(ResolutionError::DuplicateReference);
        }
        if steps.len() != request.plan.steps().len() {
            return Err(ResolutionError::InvalidReference);
        }
        for (result, step) in steps.iter().zip(request.plan.steps()) {
            if &result.step != step.id() {
                return Err(ResolutionError::InvalidReference);
            }
            validate_step(request, result, step)?;
        }
        let all_resolved = steps.iter().all(|s| {
            matches!(
                s.outcome,
                ResolutionOutcome::Resolved | ResolutionOutcome::NoOp
            )
        });
        if (outcome == ResolutionOutcome::NoOp) != request.plan.is_noop()
            || (outcome == ResolutionOutcome::Resolved) != (!request.plan.is_noop() && all_resolved)
        {
            return Err(ResolutionError::InconsistentOutcome);
        }
        Ok(Self {
            version,
            basis: request.basis.clone(),
            outcome,
            steps,
            alternatives: request.alternatives.clone(),
        })
    }

    #[must_use]
    pub const fn version(&self) -> SchemaVersion {
        self.version
    }
    #[must_use]
    pub fn basis(&self) -> &ResolutionBasis {
        &self.basis
    }
    #[must_use]
    pub const fn outcome(&self) -> ResolutionOutcome {
        self.outcome
    }
    #[must_use]
    pub fn steps(&self) -> &[StepResolution] {
        &self.steps
    }

    /// Retains explicit equivalence semantics for downstream revalidation.
    #[must_use]
    pub fn alternatives(&self) -> &[RequirementAlternatives] {
        &self.alternatives
    }
}

fn validate_step(
    request: &ResolutionRequest,
    result: &StepResolution,
    step: &gateway_domain::PlanStep,
) -> Result<(), ResolutionError> {
    if step.kind() == gateway_domain::PlanStepKind::NoOp {
        return if result.outcome == ResolutionOutcome::NoOp
            && result.binding.is_none()
            && result.requirements.is_empty()
            && step.capability_requirements().is_empty()
            && result.readiness == LifecycleReadiness::NotApplicable
        {
            Ok(())
        } else {
            Err(ResolutionError::InconsistentOutcome)
        };
    }
    let mut seen = BTreeSet::new();
    for item in &result.requirements {
        if !step.capability_requirements().contains(&item.requirement) {
            return Err(ResolutionError::InvalidReference);
        }
        if !seen.insert(&item.requirement) {
            return Err(ResolutionError::DuplicateReference);
        }
        let mut providers = BTreeSet::new();
        for candidate in &item.candidates {
            if !providers.insert(&candidate.provider) {
                return Err(ResolutionError::DuplicateReference);
            }
        }
        if let Some(selected) = &item.selected {
            if !providers.contains(selected) {
                return Err(ResolutionError::InvalidReference);
            }
        }
    }
    if seen.len() != step.capability_requirements().len() {
        return Err(ResolutionError::InvalidReference);
    }
    let resolved = result.outcome == ResolutionOutcome::Resolved;
    if resolved != result.binding.is_some() || result.outcome == ResolutionOutcome::NoOp {
        return Err(ResolutionError::InconsistentOutcome);
    }
    if let Some(binding) = &result.binding {
        if binding
            .participating_agents
            .contains(&binding.primary_agent)
            || binding.skills.values().any(|agent| {
                agent != &binding.primary_agent && !binding.participating_agents.contains(agent)
            })
        {
            return Err(ResolutionError::InvalidReference);
        }
        for item in &result.requirements {
            if let Some(provider) = &item.selected {
                let bound = match provider {
                    CapabilityProvider::Agent { agent_id } => {
                        agent_id == &binding.primary_agent
                            || binding.participating_agents.contains(agent_id)
                    }
                    CapabilityProvider::Skill { skill_id } => binding.skills.contains_key(skill_id),
                };
                if !bound {
                    return Err(ResolutionError::InvalidReference);
                }
            }
        }
        for group in request
            .alternatives
            .iter()
            .filter(|g| g.step == result.step)
        {
            let count = result
                .requirements
                .iter()
                .filter(|r| group.members.contains(&r.requirement) && r.selected.is_some())
                .count();
            if count > 1 || (count == 0 && group.cardinality == RequirementCardinality::Mandatory) {
                return Err(ResolutionError::InconsistentOutcome);
            }
        }
        for item in &result.requirements {
            let requirement = request
                .plan
                .capability_requirements()
                .iter()
                .find(|r| r.id() == &item.requirement)
                .ok_or(ResolutionError::InvalidReference)?;
            let grouped = request
                .alternatives
                .iter()
                .any(|g| g.step == result.step && g.members.contains(&item.requirement));
            if item.selected.is_none()
                && ((!grouped && requirement.cardinality() == RequirementCardinality::Mandatory)
                    || item.reason != ResolutionReason::OptionalOmitted)
            {
                return Err(ResolutionError::InconsistentOutcome);
            }
        }
    }
    Ok(())
}
