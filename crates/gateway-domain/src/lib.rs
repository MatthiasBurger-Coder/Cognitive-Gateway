#![forbid(unsafe_code)]

pub mod agent;
pub mod capability;
pub mod constraint;
pub mod definitions;
pub mod execution_context;
pub mod execution_profile;
pub mod identifiers;
pub mod operating_mode;
pub mod policy;
pub mod relationships;
pub mod retrieval;
pub mod serialization;
pub mod skill;
pub mod state;
pub mod task;
pub mod validation;
pub mod version;
pub mod workflow;

pub use agent::AgentDefinition;
pub use capability::{Capability, CapabilityClass, CapabilityDefinition};
pub use constraint::{Constraint, ConstraintDefinition, ConstraintKind};
pub use definitions::DefinitionCatalog;
pub use policy::PolicyDefinition;
pub use retrieval::KnowledgeQuery;
pub use serialization::SerializationError;
pub use skill::SkillDefinition;
pub use workflow::WorkflowDefinition;

pub use execution_context::{ExecutionContext, ExecutionContextIR, ExecutionContextIr};
pub use execution_profile::ExecutionProfile;
pub use identifiers::{
    AgentId, CapabilityId, ConstraintId, ExecutionContextId, ExecutionRuntimeId, PolicyId,
    RuntimeId, SkillId, TaskId, WorkflowId,
};
pub use operating_mode::OperatingMode;
pub use state::{
    BlockerState, ExecutionState, GateState, WorkflowState, validate_mode_and_profile,
};
pub use task::{TaskClassification, TaskConfidence, TaskDescriptor};
pub use validation::{NonEmptyText, ValidationError};
pub use version::SchemaVersion;
