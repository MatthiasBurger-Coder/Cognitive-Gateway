//! Explicit pause, resume, retry, repair and recovery semantics.

use serde::{Deserialize, Serialize};

use crate::{ActivityId, InstanceError, ProcessDefinition, ProcessInstance, TransitionProjection};

/// Generic causes for a process waiting state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PauseReason {
    HumanReview,
    MissingEvidence,
    MissingAuthorization,
    ExternalAction,
    InfrastructureRecovery,
    ExternalSystemUnavailable,
}

/// The condition that must be revalidated before resume.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaitingCondition {
    reason: PauseReason,
    detail: String,
}

impl WaitingCondition {
    pub fn new(reason: PauseReason, detail: impl Into<String>) -> Result<Self, InstanceError> {
        let detail = detail.into();
        if detail.trim().is_empty() {
            return Err(InstanceError::new(
                "INVALID_WAITING_CONDITION",
                "waiting detail cannot be empty",
            ));
        }
        Ok(Self { reason, detail })
    }
    #[must_use]
    pub const fn reason(&self) -> PauseReason {
        self.reason
    }
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

/// Result of a bounded same-activity retry attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryOutcome {
    Retried { attempt: u32, max_attempts: u32 },
    Exhausted { attempts: u32, max_attempts: u32 },
}

/// Operations governing lifecycle recovery. No operation executes an Agent or
/// external action; all decisions remain explicit process inputs.
#[derive(Debug, Default, Clone, Copy)]
pub struct LifecycleController;

impl LifecycleController {
    pub fn pause(
        instance: &mut ProcessInstance,
        reason: PauseReason,
        detail: impl Into<String>,
    ) -> Result<(), InstanceError> {
        instance.pause(WaitingCondition::new(reason, detail)?)
    }

    pub fn resume(
        instance: &mut ProcessInstance,
        condition_revalidated: bool,
    ) -> Result<(), InstanceError> {
        instance.resume(condition_revalidated)
    }

    pub fn retry(
        instance: &mut ProcessInstance,
        activity: ActivityId,
        max_attempts: u32,
    ) -> Result<RetryOutcome, InstanceError> {
        if max_attempts == 0 {
            return Err(InstanceError::new(
                "UNBOUNDED_RETRY",
                "retry budget must be positive",
            ));
        }
        let current = instance
            .retry_attempts()
            .get(&activity)
            .copied()
            .unwrap_or(0);
        if current >= max_attempts {
            return Ok(RetryOutcome::Exhausted {
                attempts: current,
                max_attempts,
            });
        }
        let attempt = instance.increment_retry(activity)?;
        Ok(RetryOutcome::Retried {
            attempt,
            max_attempts,
        })
    }

    /// Applies a declared repair/rework transition through the same projection
    /// boundary used by normal evaluation.
    pub fn repair(
        instance: &mut ProcessInstance,
        definition: &ProcessDefinition,
        projection: TransitionProjection,
    ) -> Result<(), InstanceError> {
        instance.apply_projection(definition, projection)
    }

    pub fn hard_fail(
        instance: &mut ProcessInstance,
        reason: impl Into<String>,
    ) -> Result<(), InstanceError> {
        instance.mark_failed(reason)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        EventTypeDefinition, EventTypeId, GuardExpression, ProcessDefinitionBuilder,
        ProcessDefinitionId, ProcessDefinitionVersion, ProcessInstanceId, ProcessInstanceStatus,
        StateDefinition, StateId, TransitionId,
    };

    fn definition() -> ProcessDefinition {
        ProcessDefinitionBuilder::new(
            ProcessDefinitionId::new("lifecycle-example").unwrap(),
            ProcessDefinitionVersion::new(1).unwrap(),
        )
        .with_states([
            StateDefinition::new(StateId::new("start").unwrap(), true, false).unwrap(),
            StateDefinition::new(StateId::new("repair").unwrap(), false, false).unwrap(),
            StateDefinition::new(StateId::new("done").unwrap(), false, true).unwrap(),
        ])
        .with_events([
            EventTypeDefinition::new(EventTypeId::new("repair").unwrap()),
            EventTypeDefinition::new(EventTypeId::new("finish").unwrap()),
        ])
        .with_transitions([
            crate::TransitionDefinition::new(
                TransitionId::new("to-repair").unwrap(),
                StateId::new("start").unwrap(),
                EventTypeId::new("repair").unwrap(),
                StateId::new("repair").unwrap(),
                GuardExpression::Always,
            ),
            crate::TransitionDefinition::new(
                TransitionId::new("finish").unwrap(),
                StateId::new("repair").unwrap(),
                EventTypeId::new("finish").unwrap(),
                StateId::new("done").unwrap(),
                GuardExpression::Always,
            ),
        ])
        .build()
        .unwrap()
    }

    #[test]
    fn pause_resume_revalidates_condition_and_preserves_identity() {
        let definition = definition();
        let mut instance =
            ProcessInstance::start(&definition, ProcessInstanceId::new("run-1").unwrap()).unwrap();
        let identity = (instance.id().clone(), instance.definition_digest().clone());
        LifecycleController::pause(&mut instance, PauseReason::HumanReview, "review required")
            .unwrap();
        assert_eq!(instance.status(), ProcessInstanceStatus::Paused);
        assert_eq!(
            instance.waiting_condition().unwrap().reason(),
            PauseReason::HumanReview
        );
        assert_eq!(
            LifecycleController::resume(&mut instance, false)
                .unwrap_err()
                .code(),
            "WAITING_CONDITION_NOT_CLEARED"
        );
        LifecycleController::resume(&mut instance, true).unwrap();
        assert_eq!(instance.status(), ProcessInstanceStatus::Running);
        assert_eq!(
            (instance.id().clone(), instance.definition_digest().clone()),
            identity
        );
    }

    #[test]
    fn retry_is_bounded_and_repair_is_a_declared_transition() {
        let definition = definition();
        let mut instance =
            ProcessInstance::start(&definition, ProcessInstanceId::new("run-1").unwrap()).unwrap();
        let activity = ActivityId::new("verify").unwrap();
        assert_eq!(
            LifecycleController::retry(&mut instance, activity.clone(), 2).unwrap(),
            RetryOutcome::Retried {
                attempt: 1,
                max_attempts: 2
            }
        );
        assert_eq!(
            LifecycleController::retry(&mut instance, activity.clone(), 2).unwrap(),
            RetryOutcome::Retried {
                attempt: 2,
                max_attempts: 2
            }
        );
        assert_eq!(
            LifecycleController::retry(&mut instance, activity, 2).unwrap(),
            RetryOutcome::Exhausted {
                attempts: 2,
                max_attempts: 2
            }
        );
        assert_eq!(
            LifecycleController::retry(&mut instance, ActivityId::new("other").unwrap(), 0)
                .unwrap_err()
                .code(),
            "UNBOUNDED_RETRY"
        );
        let projection = TransitionProjection::new(
            instance.revision(),
            TransitionId::new("to-repair").unwrap(),
            StateId::new("repair").unwrap(),
            ProcessInstanceStatus::Running,
            "repair",
        )
        .unwrap();
        LifecycleController::repair(&mut instance, &definition, projection).unwrap();
        assert_eq!(instance.current_state().as_str(), "repair");
    }

    #[test]
    fn hard_failure_is_explicit() {
        let definition = definition();
        let mut instance =
            ProcessInstance::start(&definition, ProcessInstanceId::new("run-1").unwrap()).unwrap();
        LifecycleController::hard_fail(&mut instance, "retry budget exhausted").unwrap();
        assert_eq!(instance.status(), ProcessInstanceStatus::Failed);
    }
}
