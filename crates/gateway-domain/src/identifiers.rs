//! Strongly typed identifiers used by domain aggregates and references.

use std::{fmt, str::FromStr};

use crate::validation::{ValidationError, validate_identifier};

macro_rules! typed_identifier {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
        pub struct $name(String);

        impl $name {
            /// Creates an identifier after applying the shared identifier rules.
            pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
                Ok(Self(validate_identifier(value)?))
            }

            /// Returns the validated identifier text.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Returns the owned identifier text.
            #[must_use]
            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = ValidationError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }

        impl TryFrom<String> for $name {
            type Error = ValidationError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = ValidationError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }
    };
}

typed_identifier! {
    /// Identifies a task aggregate.
    TaskId
}

typed_identifier! {
    /// Identifies an agent definition.
    AgentId
}

typed_identifier! {
    /// Identifies a skill definition.
    SkillId
}

typed_identifier! {
    /// Identifies a workflow definition.
    WorkflowId
}

typed_identifier! {
    /// Identifies a policy definition.
    PolicyId
}

typed_identifier! {
    /// Identifies an execution context.
    ExecutionContextId
}

typed_identifier! {
    /// Identifies a capability exposed through a domain capability port.
    CapabilityId
}

typed_identifier! {
    /// Identifies the reusable domain in which a capability operates.
    CapabilityDomain
}

typed_identifier! {
    /// Identifies an input or context kind accepted by a capability.
    CapabilityInputKind
}

typed_identifier! {
    /// Identifies a result or output kind produced by a capability.
    CapabilityOutputKind
}

typed_identifier! {
    /// Identifies an intrinsic precondition of a capability.
    CapabilityPrecondition
}

typed_identifier! {
    /// Identifies an intrinsic limitation or constraint of a capability.
    CapabilityConstraint
}

typed_identifier! {
    /// Identifies a deterministic applicability selector for a capability.
    CapabilityTag
}

typed_identifier! {
    /// Identifies a named execution constraint.
    ConstraintId
}

typed_identifier! {
    /// Identifies a versioned declarative context.
    DeclarativeContextId
}

typed_identifier! {
    /// Identifies an intent within a declarative context.
    IntentId
}

typed_identifier! {
    /// Identifies a desired state within an intent.
    DesiredStateId
}

typed_identifier! {
    /// Identifies one desired condition.
    ConditionId
}

typed_identifier! {
    /// Identifies an acceptance criterion within a desired state.
    AcceptanceCriterionId
}

typed_identifier! {
    /// Identifies one reported observation.
    ObservationId
}

typed_identifier! {
    /// Identifies one normalized fact derived from an observation.
    FactId
}

typed_identifier! {
    /// Identifies a supporting or challenging evidence record.
    EvidenceId
}

typed_identifier! {
    /// Identifies the source lineage of a declarative record.
    ProvenanceId
}

typed_identifier! {
    /// Identifies a normalized observed-state snapshot.
    ObservedStateId
}

/// Short name for the observed-state identity used by later current-state APIs.
pub type CurrentStateId = ObservedStateId;

typed_identifier! {
    /// Identifies a derived assessment.
    AssessmentId
}

typed_identifier! {
    /// Identifies the stable rule contract that produced an assessment or risk.
    AssessmentRuleId
}

typed_identifier! {
    /// Identifies a derived risk.
    RiskId
}

typed_identifier! {
    /// Identifies a complete operational situation snapshot.
    SituationId
}

typed_identifier! {
    /// Identifies a desired-vs-current delta snapshot.
    DeltaId
}

typed_identifier! {
    /// Identifies one item in a desired-vs-current delta.
    DeltaItemId
}

typed_identifier! {
    /// Identifies one information-resolution input derived from a Delta item.
    PlanningInputId
}

typed_identifier! {
    /// Identifies one abstract capability requirement in a declarative plan.
    CapabilityRequirementId
}

typed_identifier! {
    /// Identifies a declarative plan.
    PlanId
}

typed_identifier! {
    /// Identifies one step in a declarative plan graph.
    PlanStepId
}

typed_identifier! {
    /// Identifies an explicitly scoped external-context request boundary.
    ContextScopeId
}

typed_identifier! {
    /// Identifies one derived, scoped external-context cache entry.
    ContextCacheEntryId
}

typed_identifier! {
    /// Identifies a source used for evidence or provenance without naming a provider.
    SourceId
}

typed_identifier! {
    /// Identifies a referenced external artifact without embedding its contents.
    ReferenceId
}

typed_identifier! {
    /// Identifies an execution runtime without coupling the domain to a
    /// concrete runtime provider.
    ExecutionRuntimeId
}

/// Short name for an execution runtime identity used by integration ports.
pub type RuntimeId = ExecutionRuntimeId;

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{
        AcceptanceCriterionId, AgentId, AssessmentId, AssessmentRuleId, CapabilityConstraint,
        CapabilityDomain, CapabilityId, CapabilityInputKind, CapabilityOutputKind,
        CapabilityPrecondition, CapabilityTag, ConditionId, ConstraintId, ContextCacheEntryId,
        ContextScopeId, DeclarativeContextId, DesiredStateId, EvidenceId, ExecutionContextId,
        ExecutionRuntimeId, FactId, IntentId, ObservationId, ObservedStateId, PolicyId,
        ProvenanceId, ReferenceId, RiskId, SituationId, SkillId, SourceId, TaskId, WorkflowId,
    };

    #[test]
    fn typed_ids_are_not_interchangeable() {
        let task = TaskId::new("build-check").unwrap();
        let agent = AgentId::new("build-check").unwrap();

        assert_eq!(task.as_str(), agent.as_str());
        assert_ne!(task, TaskId::new("other-task").unwrap());
        assert_eq!(agent.to_string(), "build-check");
    }

    #[test]
    fn ids_support_safe_parsing() {
        assert_eq!(
            ExecutionContextId::from_str("context-1").unwrap().as_str(),
            "context-1"
        );
        assert!(TaskId::from_str("../context").is_err());
        assert!(TaskId::from_str("context/").is_err());
        assert!(TaskId::from_str("").is_err());
    }

    #[test]
    fn ids_support_typed_conversions_and_owned_values() {
        let agent = AgentId::try_from("agent").unwrap();
        assert_eq!(agent.as_ref(), "agent");
        assert_eq!(agent.clone().into_inner(), "agent");
        assert_eq!(
            SkillId::try_from("skill".to_owned()).unwrap().to_string(),
            "skill"
        );
        assert_eq!(WorkflowId::new("workflow").unwrap().as_str(), "workflow");
        assert_eq!(PolicyId::new("policy").unwrap().as_str(), "policy");
        assert_eq!(
            CapabilityId::new("capability").unwrap().as_str(),
            "capability"
        );
        assert_eq!(
            CapabilityDomain::new("architecture").unwrap().as_str(),
            "architecture"
        );
        assert_eq!(
            CapabilityInputKind::new("repository.snapshot")
                .unwrap()
                .as_str(),
            "repository.snapshot"
        );
        assert_eq!(
            CapabilityOutputKind::new("architecture.graph")
                .unwrap()
                .as_str(),
            "architecture.graph"
        );
        assert_eq!(
            CapabilityPrecondition::new("repository.available")
                .unwrap()
                .as_str(),
            "repository.available"
        );
        assert_eq!(
            CapabilityConstraint::new("read-only").unwrap().as_str(),
            "read-only"
        );
        assert_eq!(
            CapabilityTag::new("architecture").unwrap().as_str(),
            "architecture"
        );
        assert_eq!(
            ConstraintId::new("constraint").unwrap().as_str(),
            "constraint"
        );
        assert_eq!(
            ExecutionRuntimeId::new("runtime").unwrap().as_str(),
            "runtime"
        );
        assert_eq!(
            DeclarativeContextId::new("context").unwrap().as_str(),
            "context"
        );
        assert_eq!(IntentId::new("intent").unwrap().as_str(), "intent");
        assert_eq!(DesiredStateId::new("desired").unwrap().as_str(), "desired");
        assert_eq!(ConditionId::new("condition").unwrap().as_str(), "condition");
        assert_eq!(
            AcceptanceCriterionId::new("criterion").unwrap().as_str(),
            "criterion"
        );
        assert_eq!(
            ObservationId::new("observation").unwrap().as_str(),
            "observation"
        );
        assert_eq!(FactId::new("fact").unwrap().as_str(), "fact");
        assert_eq!(EvidenceId::new("evidence").unwrap().as_str(), "evidence");
        assert_eq!(
            ProvenanceId::new("provenance").unwrap().as_str(),
            "provenance"
        );
        assert_eq!(ObservedStateId::new("state").unwrap().as_str(), "state");
        assert_eq!(
            AssessmentId::new("assessment").unwrap().as_str(),
            "assessment"
        );
        assert_eq!(RiskId::new("risk").unwrap().as_str(), "risk");
        assert_eq!(SituationId::new("situation").unwrap().as_str(), "situation");
        assert_eq!(
            AssessmentRuleId::new("assessment-rule").unwrap().as_str(),
            "assessment-rule"
        );
        assert_eq!(ContextScopeId::new("scope").unwrap().as_str(), "scope");
        assert_eq!(
            ContextCacheEntryId::new("cache-entry").unwrap().as_str(),
            "cache-entry"
        );
        assert_eq!(SourceId::new("source").unwrap().as_str(), "source");
        assert_eq!(ReferenceId::new("reference").unwrap().as_str(), "reference");
    }
}
