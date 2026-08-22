//! Validation helpers for immutable descriptor relationships.

use std::collections::HashSet;

use crate::{ValidationError, identifiers::SkillId};

/// Checks a relationship list for duplicates and returns an owned immutable
/// vector. Relationship order is preserved because it can be meaningful to a
/// deterministic resolver.
pub(crate) fn unique_relationships<T>(
    values: impl IntoIterator<Item = T>,
    field: &'static str,
) -> Result<Vec<T>, ValidationError>
where
    T: Clone + Eq + std::hash::Hash,
{
    let mut seen = HashSet::new();
    let mut relationships = Vec::new();

    for value in values {
        if seen.contains(&value) {
            return Err(ValidationError::DuplicateRelationship { field });
        }
        seen.insert(value.clone());
        relationships.push(value);
    }

    Ok(relationships)
}

/// Rejects a self-referential skill dependency.
pub(crate) fn reject_self_dependency(
    skill_id: &SkillId,
    dependencies: &[SkillId],
) -> Result<(), ValidationError> {
    if dependencies.iter().any(|dependency| dependency == skill_id) {
        Err(ValidationError::SelfReference {
            field: "dependencies",
        })
    } else {
        Ok(())
    }
}
