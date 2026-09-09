#![forbid(unsafe_code)]

#[cfg(test)]
extern crate self as gateway_application;

pub mod context;
pub mod external_context;
pub mod planning_application;
pub mod ports;
pub mod resolution;
pub mod resolution_agents;
pub mod resolution_applicability;
pub mod resolution_artifact;
pub mod resolution_candidates;
pub mod resolution_composition;
mod resolution_encoding;
pub mod resolution_explain;
pub mod resolution_process;
pub mod resolution_skills;
pub mod resolution_snapshot;
pub mod situation_application;

pub use external_context::{
    CacheCapabilities, CacheEntry, CacheRetention, ContextBoundaryError, ContextScope,
    InMemoryContextCache, InMemoryContextStore, IngestionKey, IngestionReceipt, IngestionResult,
    ScopeLifecycle, ScopedContextSnapshot, ScopedObservationBatch, SourceSnapshot,
    SyntheticContextSource,
};
pub use planning_application::{
    DeclarativePlanningApplication, PlanningApplicationError, PlanningCapabilitySnapshot,
    PlanningExplainability, PlanningRuleSnapshot,
};
pub use situation_application::{
    DeclarativeSituationApplication, ProcessSituationReference, ProcessSnapshotInput,
    SituationApplicationError, SituationExplainability, SituationInspection,
};
