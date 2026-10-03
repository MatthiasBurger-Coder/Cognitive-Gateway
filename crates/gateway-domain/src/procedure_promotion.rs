//! CG-24 journal contracts. These describe decisions, never authenticate authority.
use crate::{ContentDigest, ContextScopeId, ProvenanceId, ReferenceId, ValidationError};
use crate::{learning::LearnedProcedure, procedure_evaluation::EvaluationBundle};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcedureVersion {
    pub id: ReferenceId,
    pub version: u32,
    pub digest: ContentDigest,
}
impl ProcedureVersion {
    pub fn of(procedure: &LearnedProcedure) -> Self {
        Self {
            id: procedure.id().clone(),
            version: procedure.version(),
            digest: procedure.digest().clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PromotionState {
    Discovered,
    Candidate,
    Validated,
    Evaluated,
    Approved,
    Canary,
    Active,
    Rejected,
    Deprecated,
    RolledBack,
    Superseded,
}
impl PromotionState {
    /// Special evidence/activation/rollback edges are handled by dedicated commands.
    pub fn allows_advance(self, to: Self) -> bool {
        use PromotionState::*;
        matches!(
            (self, to),
            (Discovered, Candidate | Rejected)
                | (Candidate, Validated | Rejected)
                | (Validated | Evaluated, Rejected)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionMetadata {
    pub id: ReferenceId,
    pub actor: ProvenanceId,
    pub policy_decision: ReferenceId,
    pub at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanaryBoundary {
    pub scope: ContextScopeId,
    pub cohorts: BTreeSet<ReferenceId>,
    pub starts_at: i64,
    pub ends_at: i64,
    pub max_executions: u32,
    pub max_failures: u32,
    pub required_successes: u32,
}
impl CanaryBoundary {
    pub fn validate(&self, procedure: &LearnedProcedure) -> Result<(), ValidationError> {
        if self.scope != *procedure.fingerprint().scope()
            || self.cohorts.is_empty()
            || self.starts_at < 0
            || self.ends_at <= self.starts_at
            || self.max_executions == 0
            || self.required_successes == 0
            || self.required_successes > self.max_executions
            || self.max_failures >= self.max_executions
        {
            return Err(ValidationError::InvalidDeclarativeValue {
                reason: "invalid canary boundary",
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecutionMode {
    Canary,
    Active,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RuntimeOutcome {
    Success,
    ExecutionFailed,
    VerificationFailed,
    Refused,
}

/// Reservations are recorded before runtime dispatch and consume the rollout budget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionRequest {
    pub id: ReferenceId,
    pub scope: ContextScopeId,
    pub cohort: ReferenceId,
    pub mode: ExecutionMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
pub enum PromotionCommand {
    Discover {
        procedure: Box<LearnedProcedure>,
        discovery_evidence: ReferenceId,
    },
    Advance {
        procedure: ProcedureVersion,
        from: PromotionState,
        to: PromotionState,
        evidence: ReferenceId,
    },
    Evaluate {
        procedure: ProcedureVersion,
        bundle: Box<EvaluationBundle>,
    },
    Approve {
        procedure: ProcedureVersion,
        evaluation_digest: ContentDigest,
    },
    StartCanary {
        procedure: ProcedureVersion,
        boundary: CanaryBoundary,
    },
    Activate {
        procedure: ProcedureVersion,
    },
    /// Atomically activates the successful canary and supersedes the current active version.
    Supersede {
        procedure: ProcedureVersion,
        previous: ProcedureVersion,
    },
    /// An exact previously active predecessor is required when an active version is rolled back.
    Rollback {
        procedure: ProcedureVersion,
        restore: Option<ProcedureVersion>,
    },
    Disable {
        procedure: ProcedureVersion,
        reason: ReferenceId,
    },
    ReserveExecution {
        procedure: ProcedureVersion,
        execution: ExecutionRequest,
    },
    RecordOutcome {
        procedure: ProcedureVersion,
        execution_id: ReferenceId,
        outcome: RuntimeOutcome,
        evidence: ReferenceId,
    },
}
impl PromotionCommand {
    pub fn is_runtime(&self) -> bool {
        matches!(
            self,
            Self::ReserveExecution { .. } | Self::RecordOutcome { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromotionEvent {
    pub metadata: DecisionMetadata,
    pub command: PromotionCommand,
}
/// Durable journals must come from a trusted store. JSON is not an authorization token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromotionJournal {
    pub schema_version: u16,
    pub events: Vec<PromotionEvent>,
}
impl Default for PromotionJournal {
    fn default() -> Self {
        Self {
            schema_version: 1,
            events: Vec::new(),
        }
    }
}
