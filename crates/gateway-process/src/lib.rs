#![forbid(unsafe_code)]
#![doc = "Canonical, deterministic process contracts for Cognitive Gateway."]

mod error;
mod identifiers;
mod ir;

pub use error::{ProcessError, ValidationCode};
pub use identifiers::{
    ActivityId, BlockerId, EventOccurrenceId, EventTypeId, EvidenceTypeId, GateId,
    ProcessDefinitionDigest, ProcessDefinitionId, ProcessDefinitionVersion, ProcessInstanceId,
    ProcessInstanceRevision, StateId, TransitionId,
};
pub use ir::{
    ActivityConstraint, ActivityDefinition, BlockerDefinition, DefinitionIdentity,
    EvidenceRequirement, ExecutionGraphExtension, GateDefinition, GateStatus, GuardExpression,
    InvariantDefinition, ProcessDefinition, ProcessDefinitionBuilder, ProcessIrVersion,
    RecoveryPolicy, StateDefinition, TransitionDefinition,
};
