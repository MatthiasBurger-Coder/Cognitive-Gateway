//! Typed process-to-resolution and process-to-policy boundary contracts.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{ActivityDefinition, ActivityId, AuthorizationId, PolicyDecisionId};

/// Abstract authorization input consumed by the process engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub enum AuthorizationStatus {
    Allowed,
    Denied,
    Waiting,
}

impl AuthorizationStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Allowed => "ALLOWED",
            Self::Denied => "DENIED",
            Self::Waiting => "WAITING",
        }
    }
}

/// Abstract policy decision input; policy rule evaluation belongs to CG-09.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub enum PolicyDecisionStatus {
    Allow,
    Deny,
    Waiting,
}

impl PolicyDecisionStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "ALLOW",
            Self::Deny => "DENY",
            Self::Waiting => "WAITING",
        }
    }
}

/// The typed external inputs relevant to one process evaluation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyInput {
    authorizations: BTreeMap<AuthorizationId, AuthorizationStatus>,
    decisions: BTreeMap<PolicyDecisionId, PolicyDecisionStatus>,
}

impl PolicyInput {
    #[must_use]
    pub fn with_authorization(mut self, id: AuthorizationId, status: AuthorizationStatus) -> Self {
        self.authorizations.insert(id, status);
        self
    }
    #[must_use]
    pub fn with_policy_decision(
        mut self,
        id: PolicyDecisionId,
        status: PolicyDecisionStatus,
    ) -> Self {
        self.decisions.insert(id, status);
        self
    }
    #[must_use]
    pub fn authorizations(&self) -> &BTreeMap<AuthorizationId, AuthorizationStatus> {
        &self.authorizations
    }
    #[must_use]
    pub fn decisions(&self) -> &BTreeMap<PolicyDecisionId, PolicyDecisionStatus> {
        &self.decisions
    }
}

/// Capability-first authorized work projection. It does not select an Agent or
/// Skill and cannot mutate a process instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizedActivity {
    id: ActivityId,
    capabilities: Vec<gateway_domain::CapabilityId>,
    output_evidence: Vec<crate::EvidenceTypeId>,
    constraints: Vec<crate::ActivityConstraint>,
}

impl AuthorizedActivity {
    #[must_use]
    pub fn from_definition(definition: &ActivityDefinition) -> Self {
        Self {
            id: definition.id().clone(),
            capabilities: definition.capabilities().to_vec(),
            output_evidence: definition.output_evidence().to_vec(),
            constraints: definition.constraints().to_vec(),
        }
    }
    #[must_use]
    pub fn id(&self) -> &ActivityId {
        &self.id
    }
    #[must_use]
    pub fn capabilities(&self) -> &[gateway_domain::CapabilityId] {
        &self.capabilities
    }
    #[must_use]
    pub fn output_evidence(&self) -> &[crate::EvidenceTypeId] {
        &self.output_evidence
    }
    #[must_use]
    pub fn constraints(&self) -> &[crate::ActivityConstraint] {
        &self.constraints
    }
}
