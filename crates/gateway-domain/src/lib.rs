#![forbid(unsafe_code)]

pub mod agent;
pub mod capability;
mod cg06_serialization;
pub mod comparison;
pub mod constraint;
pub mod declarative_context;
pub mod definition_contract;
pub mod definitions;
pub mod delta;
pub mod execution_context;
pub mod execution_profile;
pub mod identifiers;
pub mod intent;
pub mod normalization;
pub mod observation;
pub mod operating_mode;
pub mod planning;
pub mod policy;
pub mod quality;
pub mod relationships;
pub mod retrieval;
pub mod serialization;
pub mod situation;
pub mod skill;
pub mod state;
pub mod task;
pub mod validation;
pub mod version;
pub mod workflow;

pub use agent::AgentDefinition;
pub use capability::{Capability, CapabilityClass, CapabilityContract, CapabilityDefinition};
pub use cg06_serialization::DeclarativeContextSituationDocument;
pub use comparison::{
    COMPARISON_SEMANTICS_VERSION, ComparisonOutcome, ComparisonReasonCode, ComparisonResult,
    ComparisonRules, ComparisonSemanticsVersion, ComparisonTarget, ComparisonTrace,
    compare_condition, compare_desired_condition, compare_desired_state,
};
pub use constraint::{Constraint, ConstraintDefinition, ConstraintKind};
pub use declarative_context::{
    CurrentState, DECLARATIVE_CONTEXT_IR_VERSION, DeclarativeContext, DeclarativeContextVersion,
    ObservedState, Situation,
};
pub use definition_contract::{
    AgentDefinitionDocument, DEFINITION_SCHEMA_VERSION, DefinitionKind, SkillDefinitionDocument,
    VersionedAgentDefinition, VersionedSkillDefinition,
};
pub use definitions::DefinitionCatalog;
pub use delta::{
    DELTA_DERIVATION_VERSION, DeltaDerivation, DeltaDerivationRules, derive_delta,
    derive_delta_with_comparison, derive_delta_with_rules,
};
pub use intent::{
    AcceptanceCriterion, ComparisonOperator, ConditionExpression, DecimalValue,
    DeclarativeConstraint, DesiredCondition, DesiredState, DesiredSubject, DesiredValue, Intent,
    OriginalInput, SubjectPath, SymbolValue, TypedValue, ValueKind,
};
pub use normalization::{
    NormalizationDiagnostic, NormalizationInput, NormalizationReasonCode, NormalizedClaim,
    NormalizedStateEntry, StateLineage, StateStatus, normalize_current_state,
};
pub use policy::PolicyDefinition;
pub use quality::{
    Confidence, ConflictStatus, FreshnessPolicy, FreshnessStatus, QualityMetadata,
    SensitivityClass, TrustClass, Uncertainty, UnixTimestamp, ValidityInterval, evaluate_freshness,
};
pub use retrieval::{KnowledgeProvenance, KnowledgeQuery, RetrievedKnowledge};
pub use serialization::SerializationError;
pub use situation::{
    ASSESSMENT_RULE_VERSION, Assessment, AssessmentBasis, AssessmentConclusion, AssessmentKind,
    AssessmentOrigin, AssessmentReasonCode, AssessmentRuleContract, AssessmentRuleVersion,
    AssessmentStatus, BasisReferences, ExplainabilityItem, ExplainabilityTrace,
    QualitativeLikelihood, ReasonCode, Risk, RiskBasis, RiskCategory, RiskLikelihood, RiskOrigin,
    RiskReasonCode, RiskSeverity, RiskStatus, SituationAssemblyInput, SituationDiagnostic,
    SituationDiagnosticCode, SituationReference, assemble_situation,
};
pub use skill::SkillDefinition;
pub use workflow::WorkflowDefinition;

pub use execution_context::{ExecutionContext, ExecutionContextIR, ExecutionContextIr};
pub use execution_profile::ExecutionProfile;
pub use identifiers::{
    AcceptanceCriterionId, AgentId, AssessmentId, AssessmentRuleId, CapabilityConstraint,
    CapabilityDomain, CapabilityId, CapabilityInputKind, CapabilityOutputKind,
    CapabilityPrecondition, CapabilityRequirementId, CapabilityTag, ConditionId, ConstraintId,
    ContextCacheEntryId, ContextScopeId, CurrentStateId, DeclarativeContextId, DeltaId,
    DeltaItemId, DesiredStateId, EvidenceId, ExecutionContextId, ExecutionRuntimeId, FactId,
    IntentId, ObservationId, ObservedStateId, PlanId, PlanStepId, PolicyId, ProvenanceId,
    ReferenceId, RiskId, RuntimeId, SituationId, SkillId, SourceId, TaskId, WorkflowId,
};
pub use observation::{
    AssertionPolarity, ContentDigest, Evidence, EvidenceContent, EvidenceKind, EvidenceLink,
    EvidenceRelation, Fact, Observation, ObservationEvidenceSet, Provenance, SourceKind,
    SourceTimestamp,
};
pub use operating_mode::OperatingMode;
pub use planning::{
    CapabilityRequirement, DECLARATIVE_PLANNING_IR_VERSION, Delta, DeltaBasis, DeltaItem,
    DeltaKind, DeltaReasonCode, LifecycleRequirement, LifecycleRequirementKind, Plan,
    PlanCondition, PlanStep, PlanStepKind, PlanningIrVersion, RequiredOutcome, RequiredOutcomeKind,
    RequirementCardinality,
};
pub use state::{
    BlockerState, ExecutionState, GateState, WorkflowState, validate_mode_and_profile,
};
pub use task::{TaskClassification, TaskConfidence, TaskDescriptor};
pub use validation::{NonEmptyText, ValidationError};
pub use version::SchemaVersion;
