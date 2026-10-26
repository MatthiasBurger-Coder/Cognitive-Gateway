//! Trusted session goals for connector-backed work.
//!
//! These types are constructed from a host-registered CG-14 intent and a
//! canonical plan. They are application contracts, not client or model DTOs;
//! no serde construction path is provided. The existing artifact goal remains
//! a separate variant with its original context-artifact success semantics.

use std::collections::BTreeSet;

use gateway_domain::{CapabilityId, ConditionId, ContextScopeId, Intent, PlanStepId, SourceId};

use crate::{
    connectors::ConnectorOperationClass,
    sessions::{RecordRef, SessionError, SupportedArtifactGoal},
};

/// Maximum number of planned steps retained by one registered connector goal.
pub const MAX_CONNECTOR_GOAL_STEPS: usize = 64;

/// Maximum number of desired conditions that can be bound to read-only evidence.
pub const MAX_CONNECTOR_GOAL_CONDITIONS: usize = 256;

/// Trusted source and canonical read-only capability for one CG-14 condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConditionObservationMapping {
    condition: ConditionId,
    source: SourceId,
    capability: CapabilityId,
    operation_class: ConnectorOperationClass,
}

impl ConditionObservationMapping {
    /// Create one trusted observation mapping. Mutating/admin capabilities
    /// cannot be used as goal evidence sources.
    pub fn new(
        condition: ConditionId,
        source: SourceId,
        capability: CapabilityId,
        operation_class: ConnectorOperationClass,
    ) -> Result<Self, SessionError> {
        if !is_read_only(operation_class) {
            return Err(SessionError::UnsupportedCapability);
        }
        Ok(Self {
            condition,
            source,
            capability,
            operation_class,
        })
    }

    pub fn condition(&self) -> &ConditionId {
        &self.condition
    }

    pub fn source(&self) -> &SourceId {
        &self.source
    }

    pub fn capability(&self) -> &CapabilityId {
        &self.capability
    }

    pub const fn operation_class(&self) -> ConnectorOperationClass {
        self.operation_class
    }
}

/// A host-registered connector task over an existing CG-14 intent and plan.
///
/// The caller is the trusted registration/composition boundary. The model may
/// propose tool calls while executing these steps, but cannot construct this
/// value through a wire format or supply its goal, scope, plan, steps, or
/// observation bindings at dispatch time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportedConnectorGoal {
    intent: Intent,
    scope: ContextScopeId,
    plan: RecordRef,
    steps: Vec<PlanStepId>,
    observations: Vec<ConditionObservationMapping>,
}

impl SupportedConnectorGoal {
    /// Validate the host-registered goal and its exact condition evidence map.
    pub fn from_host_registration(
        intent: Intent,
        scope: ContextScopeId,
        plan: RecordRef,
        steps: Vec<PlanStepId>,
        mut observations: Vec<ConditionObservationMapping>,
    ) -> Result<Self, SessionError> {
        if steps.is_empty() {
            return Err(SessionError::InvalidInput);
        }
        if steps.len() > MAX_CONNECTOR_GOAL_STEPS {
            return Err(SessionError::LimitExceeded);
        }
        let mut unique_steps = BTreeSet::new();
        if steps.iter().any(|step| !unique_steps.insert(step.clone())) {
            return Err(SessionError::InvalidInput);
        }

        let conditions = intent.desired_state().conditions();
        if conditions.is_empty() {
            return Err(SessionError::UnsupportedCapability);
        }
        if conditions.len() > MAX_CONNECTOR_GOAL_CONDITIONS {
            return Err(SessionError::LimitExceeded);
        }
        if observations.len() != conditions.len() {
            return Err(SessionError::InvalidInput);
        }

        let expected = conditions
            .iter()
            .map(|condition| condition.id().clone())
            .collect::<BTreeSet<_>>();
        let mut actual = BTreeSet::new();
        for mapping in &observations {
            if !is_read_only(mapping.operation_class)
                || !expected.contains(&mapping.condition)
                || !actual.insert(mapping.condition.clone())
            {
                return Err(SessionError::InvalidInput);
            }
        }
        if actual != expected {
            return Err(SessionError::InvalidInput);
        }

        // Canonical mapping order makes equality and persisted projections
        // deterministic regardless of trusted registry iteration order.
        observations.sort_by(|left, right| left.condition.cmp(&right.condition));

        Ok(Self {
            intent,
            scope,
            plan,
            steps,
            observations,
        })
    }

    pub fn intent(&self) -> &Intent {
        &self.intent
    }

    pub fn scope(&self) -> &ContextScopeId {
        &self.scope
    }

    pub fn plan(&self) -> &RecordRef {
        &self.plan
    }

    pub fn steps(&self) -> &[PlanStepId] {
        &self.steps
    }

    pub fn observations(&self) -> &[ConditionObservationMapping] {
        &self.observations
    }
}

/// Session goal sum type; the artifact variant retains its original meaning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupportedSessionGoal {
    Artifact(SupportedArtifactGoal),
    Connector(SupportedConnectorGoal),
}

impl SupportedSessionGoal {
    pub fn artifact(goal: SupportedArtifactGoal) -> Self {
        Self::Artifact(goal)
    }

    pub fn connector(goal: SupportedConnectorGoal) -> Self {
        Self::Connector(goal)
    }

    pub fn intent(&self) -> &Intent {
        match self {
            Self::Artifact(goal) => goal.intent(),
            Self::Connector(goal) => goal.intent(),
        }
    }

    pub fn scope(&self) -> &ContextScopeId {
        match self {
            Self::Artifact(goal) => &goal.basis().scope,
            Self::Connector(goal) => goal.scope(),
        }
    }

    pub fn as_artifact(&self) -> Option<&SupportedArtifactGoal> {
        match self {
            Self::Artifact(goal) => Some(goal),
            Self::Connector(_) => None,
        }
    }

    pub fn as_connector(&self) -> Option<&SupportedConnectorGoal> {
        match self {
            Self::Artifact(_) => None,
            Self::Connector(goal) => Some(goal),
        }
    }
}

fn is_read_only(class: ConnectorOperationClass) -> bool {
    matches!(
        class,
        ConnectorOperationClass::Read
            | ConnectorOperationClass::Search
            | ConnectorOperationClass::Inspect
    )
}

impl From<SupportedArtifactGoal> for SupportedSessionGoal {
    fn from(value: SupportedArtifactGoal) -> Self {
        Self::Artifact(value)
    }
}

impl SupportedSessionGoal {
    pub fn artifact_basis(&self) -> Result<&super::ArtifactGoalBasis, SessionError> {
        self.as_artifact()
            .map(SupportedArtifactGoal::basis)
            .ok_or(SessionError::UnsupportedCapability)
    }
}
