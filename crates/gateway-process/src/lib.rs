#![forbid(unsafe_code)]
#![doc = "Canonical, deterministic process contracts for Cognitive Gateway."]

mod compiler;
mod error;
mod identifiers;
mod ir;
mod source;
mod validation;

pub use compiler::{
    CompilationDiagnostic, CompilationError, CompilationResult, CompilationTraceEntry,
    SemanticCompiler,
};
pub use error::{ProcessError, ValidationCode};
pub use identifiers::{
    ActivityId, BlockerId, EventOccurrenceId, EventTypeId, EvidenceTypeId, GateId,
    ProcessDefinitionDigest, ProcessDefinitionId, ProcessDefinitionVersion, ProcessInstanceId,
    ProcessInstanceRevision, StateId, TransitionId,
};
pub use ir::{
    ActivityConstraint, ActivityDefinition, BlockerDefinition, DefinitionIdentity,
    EventTypeDefinition, EvidenceRequirement, ExecutionGraphExtension, GateDefinition, GateStatus,
    GuardExpression, InvariantDefinition, ProcessDefinition, ProcessDefinitionBuilder,
    ProcessIrVersion, RecoveryPolicy, StateDefinition, TransitionDefinition,
};
pub use source::{
    FrontendError, SourceDocument, SourceLocation, SourceRule, SourceScenario, SourceStep,
    SourceStepKeyword, SourceTag, TableRow,
};
pub use validation::{ProcessValidator, ValidationDiagnostic, ValidationReport};
