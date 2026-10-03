#![forbid(unsafe_code)]

#[cfg(test)]
extern crate self as gateway_application;

pub mod closed_loop;
pub mod cognitive_routing;
pub mod context;
pub mod context_application;
pub mod context_budgeting;
pub mod evaluation;
pub mod experience_patterns;
pub mod external_context;
pub mod graph_retrieval;
pub mod memory;
pub mod parallel_execution;
pub mod planning_application;
pub mod policy_application;
pub mod ports;
pub mod procedure_evaluation;
pub mod procedure_promotion;
pub mod reasoning_strategy;
pub mod recursive_retrieval;
pub mod reflex;
pub mod resolution;
pub mod resolution_agents;
pub mod resolution_applicability;
pub mod resolution_application;
pub mod resolution_artifact;
pub mod resolution_candidates;
pub mod resolution_composition;
mod resolution_encoding;
pub mod resolution_explain;
pub mod resolution_process;
pub mod resolution_skills;
pub mod resolution_snapshot;
pub mod retrieval_pipeline;
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

pub mod local_inference;
