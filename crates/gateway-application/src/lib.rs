#![forbid(unsafe_code)]

pub mod context;
pub mod external_context;
pub mod ports;
pub mod situation_application;

pub use external_context::{
    CacheCapabilities, CacheEntry, CacheRetention, ContextBoundaryError, ContextScope,
    InMemoryContextCache, InMemoryContextStore, IngestionKey, IngestionReceipt, IngestionResult,
    ScopeLifecycle, ScopedContextSnapshot, ScopedObservationBatch, SourceSnapshot,
    SyntheticContextSource,
};
pub use situation_application::{
    DeclarativeSituationApplication, ProcessSituationReference, ProcessSnapshotInput,
    SituationApplicationError, SituationExplainability, SituationInspection,
};
