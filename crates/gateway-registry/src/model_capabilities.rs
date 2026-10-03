//! Qualified model/cognitive routes are explicit configuration, never discovery.
use gateway_domain::cognitive_routing::{ModelCapabilitySnapshot, RoutingError};

/// Immutable inspectable snapshot. Refresh creates a new registry, never mutates
/// an in-flight decision or silently upgrades a model artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCapabilityRegistry {
    snapshot: ModelCapabilitySnapshot,
}

impl ModelCapabilityRegistry {
    pub fn new(snapshot: ModelCapabilitySnapshot) -> Result<Self, RoutingError> {
        snapshot.validate()?;
        Ok(Self { snapshot })
    }

    pub fn snapshot(&self) -> &ModelCapabilitySnapshot {
        &self.snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validated_snapshot_is_immutable_and_inspectable() {
        let snapshot = ModelCapabilitySnapshot {
            version: "1.0".into(),
            configuration_digest: "sha256:config".into(),
            cost_unit: "microcredits".into(),
            candidates: vec![],
        };
        let registry = ModelCapabilityRegistry::new(snapshot.clone()).unwrap();
        assert_eq!(registry.snapshot(), &snapshot);
        let mut invalid = snapshot;
        invalid.version = "2".into();
        assert_eq!(
            ModelCapabilityRegistry::new(invalid),
            Err(RoutingError::InvalidRegistry)
        );
    }
}
