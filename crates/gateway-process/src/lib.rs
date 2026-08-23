#![forbid(unsafe_code)]
#![doc = "Canonical, deterministic process contracts for Cognitive Gateway."]

mod authorization;
mod compiler;
mod constraints;
mod error;
mod evaluator;
mod identifiers;
mod instance;
mod ir;
mod lifecycle;
mod mutation;
mod registry;
mod source;
mod validation;

pub use authorization::{
    AuthorizationStatus, AuthorizedActivity, PolicyDecisionStatus, PolicyInput,
};
pub use compiler::{
    CompilationDiagnostic, CompilationError, CompilationResult, CompilationTraceEntry,
    SemanticCompiler,
};
pub use constraints::{ConstraintEvaluation, EvidenceReference, EvidenceStatus};
pub use error::{ProcessError, ValidationCode};
pub use evaluator::{
    EvaluationInputs, GuardEvaluation, TransitionDecision, TransitionDecisionCode,
    TransitionEvaluator,
};
pub use identifiers::{
    ActivityId, AuthorizationId, BlockerId, CausationId, CorrelationId, EventOccurrenceId,
    EventTypeId, EvidenceTypeId, GateId, PolicyDecisionId, ProcessDefinitionDigest,
    ProcessDefinitionId, ProcessDefinitionVersion, ProcessInstanceId, ProcessInstanceRevision,
    StateId, TransitionId,
};
pub use instance::{
    BlockerRuntimeState, InstanceError, ProcessInstance, ProcessInstanceStatus,
    TransitionHistoryEntry, TransitionProjection,
};
pub use ir::{
    ActivityConstraint, ActivityDefinition, BlockerDefinition, DefinitionIdentity,
    EventTypeDefinition, EvidenceRequirement, ExecutionGraphExtension, GateDefinition, GateStatus,
    GuardExpression, InvariantDefinition, ProcessDefinition, ProcessDefinitionBuilder,
    ProcessIrVersion, RecoveryPolicy, StateDefinition, TransitionDefinition,
};
pub use lifecycle::{LifecycleController, PauseReason, RetryOutcome, WaitingCondition};
pub use mutation::{
    AtomicProcessMutation, CommitOutcome, EventOccurrence, InMemoryProcessStore, MutationError,
};
pub use registry::{ProcessRegistry, ProcessRegistryError, ProcessSource, RegisteredProcess};
pub use source::{
    FrontendError, SourceDocument, SourceLocation, SourceRule, SourceScenario, SourceStep,
    SourceStepKeyword, SourceTag, TableRow,
};
pub use validation::{ProcessValidator, ValidationDiagnostic, ValidationReport};
