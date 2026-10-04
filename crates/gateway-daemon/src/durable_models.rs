//! CG-28 durable release coordinator and exact-version inference selection.
use crate::cognitive_store::{CognitiveStore, StoreError};
use gateway_application::{model_releases::*, offline_learning::LearningError};
use gateway_domain::ContextScopeId;
use gateway_domain::offline_learning::ModelVersion;

impl From<StoreError> for LearningError {
    fn from(_: StoreError) -> Self {
        Self::Storage
    }
}

pub struct DurableModelReleases {
    store: CognitiveStore,
}
impl DurableModelReleases {
    pub fn new(store: CognitiveStore) -> Self {
        Self { store }
    }
    pub fn scope(&self) -> &ContextScopeId {
        self.store.scope()
    }
    /// Every mutation replays trusted evidence under the database lock and saves
    /// the journal before returning a routing change to the caller.
    pub fn apply<A: ModelRecoveryAuthority, R>(
        &self,
        authority: &A,
        operation: impl FnOnce(&mut ModelReleaseRegistry) -> Result<R, LearningError>,
    ) -> Result<R, LearningError> {
        let initial = ModelReleaseRegistry::new(self.store.scope().clone()).journal();
        self.store
            .transact("model-releases-v1", &initial, |journal| {
                if journal.scope != *self.store.scope() {
                    return Err(LearningError::ScopeMismatch);
                }
                let mut registry = ModelReleaseRegistry::recover(journal, authority)?;
                let result = operation(&mut registry)?;
                *journal = registry.journal();
                Ok(result)
            })
    }
    /// Inference resolves current durable authority on every call, including
    /// rollback/restart. No stale process-local active-model pointer survives.
    pub fn active<A: ModelRecoveryAuthority>(
        &self,
        authority: &A,
    ) -> Result<Option<ModelVersion>, LearningError> {
        self.apply(authority, |registry| Ok(registry.active().cloned()))
    }
}
