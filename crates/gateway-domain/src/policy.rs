//! Policy definitions and typed capability decisions.

use crate::{
    CapabilityId, NonEmptyText, PolicyId, ValidationError, relationships::unique_relationships,
};

/// An authoritative capability boundary for a workflow.
///
/// An empty allow-list is valid and represents a deny-by-default policy. A
/// capability may not occur in both lists. The policy only describes the
/// decision input; it does not call or identify an external tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyDefinition {
    id: PolicyId,
    description: NonEmptyText,
    allowed_capability_ids: Vec<CapabilityId>,
    denied_capability_ids: Vec<CapabilityId>,
}

impl PolicyDefinition {
    /// Creates a policy with an allow-list and an initially empty deny-list.
    pub fn new(
        id: PolicyId,
        description: impl Into<String>,
        allowed_capability_ids: impl IntoIterator<Item = CapabilityId>,
    ) -> Result<Self, ValidationError> {
        Self::with_denied_capabilities(id, description, allowed_capability_ids, Vec::new())
    }

    /// Creates a policy with explicit allow and deny lists.
    pub fn with_denied_capabilities(
        id: PolicyId,
        description: impl Into<String>,
        allowed_capability_ids: impl IntoIterator<Item = CapabilityId>,
        denied_capability_ids: impl IntoIterator<Item = CapabilityId>,
    ) -> Result<Self, ValidationError> {
        let allowed_capability_ids =
            unique_relationships(allowed_capability_ids, "allowed_capability_ids")?;
        let denied_capability_ids =
            unique_relationships(denied_capability_ids, "denied_capability_ids")?;
        if denied_capability_ids
            .iter()
            .any(|capability| allowed_capability_ids.contains(capability))
        {
            return Err(ValidationError::ConflictingRelationship {
                field: "capability_ids",
            });
        }

        Ok(Self {
            id,
            description: NonEmptyText::new_for_field(description, "description")?,
            allowed_capability_ids,
            denied_capability_ids,
        })
    }

    /// Alias for [`Self::new`] for callers at parsing boundaries.
    pub fn try_new(
        id: PolicyId,
        description: impl Into<String>,
        allowed_capability_ids: impl IntoIterator<Item = CapabilityId>,
    ) -> Result<Self, ValidationError> {
        Self::new(id, description, allowed_capability_ids)
    }

    /// Returns the policy identity.
    #[must_use]
    pub fn id(&self) -> &PolicyId {
        &self.id
    }

    /// Returns the validated policy description.
    #[must_use]
    pub fn description(&self) -> &str {
        self.description.as_str()
    }

    /// Returns capabilities explicitly allowed by this policy.
    #[must_use]
    pub fn allowed_capability_ids(&self) -> &[CapabilityId] {
        &self.allowed_capability_ids
    }

    /// Returns capabilities explicitly denied by this policy.
    #[must_use]
    pub fn denied_capability_ids(&self) -> &[CapabilityId] {
        &self.denied_capability_ids
    }
}

#[cfg(test)]
mod tests {
    use super::PolicyDefinition;
    use crate::{CapabilityId, PolicyId, ValidationError};

    #[test]
    fn creates_allow_and_deny_policy_relationships() {
        let policy = PolicyDefinition::with_denied_capabilities(
            PolicyId::new("safe-review").unwrap(),
            "Read-only review policy",
            [CapabilityId::new("repository.read").unwrap()],
            [CapabilityId::new("repository.write").unwrap()],
        )
        .unwrap();

        assert_eq!(policy.id().as_str(), "safe-review");
        assert_eq!(policy.description(), "Read-only review policy");
        assert_eq!(policy.allowed_capability_ids().len(), 1);
        assert_eq!(policy.denied_capability_ids().len(), 1);
    }

    #[test]
    fn supports_deny_by_default_and_rejects_conflicts() {
        let empty = PolicyDefinition::new(
            PolicyId::new("deny-all").unwrap(),
            "No capabilities",
            Vec::<CapabilityId>::new(),
        )
        .unwrap();
        assert!(empty.allowed_capability_ids().is_empty());

        let capability = CapabilityId::new("repository.write").unwrap();
        assert!(matches!(
            PolicyDefinition::with_denied_capabilities(
                PolicyId::new("conflict").unwrap(),
                "Conflicting policy",
                [capability.clone()],
                [capability],
            ),
            Err(ValidationError::ConflictingRelationship {
                field: "capability_ids"
            })
        ));
    }

    #[test]
    fn rejects_duplicate_capabilities_and_bad_text() {
        let capability = CapabilityId::new("repository.read").unwrap();
        assert!(matches!(
            PolicyDefinition::new(
                PolicyId::new("safe").unwrap(),
                "Safe",
                [capability.clone(), capability],
            ),
            Err(ValidationError::DuplicateRelationship {
                field: "allowed_capability_ids"
            })
        ));
        assert!(
            PolicyDefinition::try_new(
                PolicyId::new("safe").unwrap(),
                "\0",
                Vec::<CapabilityId>::new(),
            )
            .is_err()
        );
    }
}
