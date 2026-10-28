#![forbid(unsafe_code)]

#[cfg(test)]
extern crate self as gateway_application;

pub mod closed_loop;
pub mod codex;
pub mod cognitive_routing;
pub mod completeness;
pub mod connector_admission;
pub mod connector_manifest;
pub mod connector_policy;
pub mod connector_proposals;
pub mod connector_resilience;
pub mod connector_results;
pub mod connectors;
pub mod context;
pub mod context_application;
pub mod context_budgeting;
pub mod evaluation;
pub mod experience_patterns;
pub mod external_context;
pub mod graph_retrieval;
pub mod grounding;
pub mod inspection_language;
pub mod interpretation;
pub mod interpretation_adapters;
pub mod invocation_resilience;
pub mod memory;
pub mod model_releases;
pub mod offline_learning;
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
pub mod retained_observations;
pub mod retrieval_pipeline;
pub mod semantic_planning;
pub mod sessions;
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

pub mod worker_fabric;

pub mod context_planning;

pub mod example_selection;

pub mod model_invocation;

pub mod prompt_rendering;

pub mod model_routing;

pub mod prompt_ir;

pub mod output_parsing;
pub mod output_verification;

pub mod invocation_qualification;
