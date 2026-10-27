//! Readonly initial goal observation. All lifecycle and reservation minting remain in SessionCoordinator.
use super::*;
use crate::closed_loop::VerifiedOutcomeReceipt;
/// Implementations authenticate the current checkpoint/configuration, native
/// readonly Process/Policy/Context and exact scoped source before every read.
/// These hooks never grant mutation, model, repair or worker execution.
pub enum ReadonlyPreparation {
    GoalSatisfied(Box<PreExecutionGoalCandidate>),
    BeforeCaptured(Box<CapturedBefore>),
}
pub trait ReadonlyGoalObservationRuntimePort {
    /// Default preserves the existing goal-only runtime. No model/effect is admitted.
    fn prepare_readonly(
        &self,
        cp: &SessionCheckpoint,
        resources: &mut SessionPhaseResources,
    ) -> Result<Option<ReadonlyPreparation>, SessionError> {
        self.observe_goal(cp, resources).map(|candidate| {
            candidate.map(|candidate| ReadonlyPreparation::GoalSatisfied(Box::new(candidate)))
        })
    }

    /// Pure current trusted registration; no provider/model/retrieval I/O.
    fn resource_limits(
        &self,
        _cp: &SessionCheckpoint,
    ) -> Result<SessionResourceLimits, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
    /// Pure complete hard ceiling in the ledger's registered cost unit.
    fn preparation_budget(
        &self,
        _cp: &SessionCheckpoint,
    ) -> Result<SessionResourceAmounts, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
    /// Called once only after the actual durable Preparation claim. Candidate is
    /// native full-goal evidence, not a boolean, grant or completion receipt.
    fn observe_goal(
        &self,
        _cp: &SessionCheckpoint,
        _resources: &mut SessionPhaseResources,
    ) -> Result<Option<PreExecutionGoalCandidate>, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
    /// Pure replay verification. Recheck full current runtime/provider/mapping/
    /// native context/goal pins and real observed_at/max_age without provider I/O.
    fn verify_retained_goal(
        &self,
        _cp: &SessionCheckpoint,
        _data: &PreExecutionGoalData,
    ) -> Result<Option<VerifiedOutcomeReceipt>, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
}
