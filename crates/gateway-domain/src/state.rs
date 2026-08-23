//! Explicit execution state and deterministic lifecycle validation.

use std::{fmt, str::FromStr};

use crate::{ExecutionProfile, OperatingMode, ValidationError};

macro_rules! state_enum {
    ($(#[$meta:meta])* $name:ident, $field:literal { $($(#[$variant_meta:meta])* $variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
        pub enum $name {
            $($(#[$variant_meta])* $variant),+
        }

        impl $name {
            /// Returns the canonical wire representation.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = ValidationError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    $($text => Ok(Self::$variant),)+
                    value => Err(ValidationError::UnknownDomainValue {
                        field: $field,
                        value: value.to_owned(),
                    }),
                }
            }
        }
    };
}

state_enum! {
    /// Lifecycle state of the complete workflow execution.
    WorkflowState, "workflow_state" {
        /// The workflow has not started.
        Pending => "PENDING",
        /// The workflow is actively executing.
        Running => "RUNNING",
        /// Execution is paused without an active blocker.
        Paused => "PAUSED",
        /// Execution cannot proceed until its blocker is resolved.
        Blocked => "BLOCKED",
        /// All workflow work completed successfully.
        Completed => "COMPLETED",
        /// Execution ended unsuccessfully.
        Failed => "FAILED",
        /// Execution was intentionally stopped before completion.
        Cancelled => "CANCELLED"
    }
}

impl WorkflowState {
    /// Returns whether moving to `next` is legal for the workflow lifecycle.
    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Pending, Self::Running | Self::Cancelled)
                | (
                    Self::Running,
                    Self::Paused | Self::Blocked | Self::Completed | Self::Failed | Self::Cancelled
                )
                | (
                    Self::Paused,
                    Self::Running | Self::Blocked | Self::Cancelled
                )
                | (Self::Blocked, Self::Running | Self::Cancelled)
        )
    }

    /// Applies a legal lifecycle transition.
    pub fn transition_to(self, next: Self) -> Result<Self, ValidationError> {
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(ValidationError::InvalidStateTransition {
                state: "workflow",
                from: self.to_string(),
                to: next.to_string(),
            })
        }
    }

    /// Returns whether this is a terminal lifecycle state.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

state_enum! {
    /// State of the currently evaluated quality or governance gate.
    GateState, "gate_state" {
        /// The gate has not started.
        Pending => "PENDING",
        /// The gate is being evaluated.
        InProgress => "IN_PROGRESS",
        /// The gate passed.
        Passed => "PASSED",
        /// The gate failed.
        Failed => "FAILED",
        /// The gate cannot be evaluated while a blocker is active.
        Blocked => "BLOCKED",
        /// The gate was intentionally not applicable.
        Skipped => "SKIPPED"
    }
}

impl GateState {
    /// Returns whether moving to `next` is legal for the gate lifecycle.
    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Pending, Self::InProgress | Self::Skipped)
                | (
                    Self::InProgress,
                    Self::Passed | Self::Failed | Self::Blocked
                )
                | (Self::Blocked, Self::InProgress)
        )
    }

    /// Applies a legal gate transition.
    pub fn transition_to(self, next: Self) -> Result<Self, ValidationError> {
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(ValidationError::InvalidStateTransition {
                state: "gate",
                from: self.to_string(),
                to: next.to_string(),
            })
        }
    }
}

state_enum! {
    /// State of the blocker collection associated with an execution.
    BlockerState, "blocker_state" {
        /// No blocker is currently preventing progress.
        Clear => "CLEAR",
        /// At least one blocker prevents progress.
        Active => "ACTIVE",
        /// A previously active blocker has been resolved.
        Resolved => "RESOLVED"
    }
}

impl BlockerState {
    /// Returns whether moving to `next` is legal for blocker tracking.
    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Clear, Self::Active)
                | (Self::Active, Self::Resolved | Self::Clear)
                | (Self::Resolved, Self::Active | Self::Clear)
        )
    }

    /// Applies a legal blocker transition.
    pub fn transition_to(self, next: Self) -> Result<Self, ValidationError> {
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(ValidationError::InvalidStateTransition {
                state: "blocker",
                from: self.to_string(),
                to: next.to_string(),
            })
        }
    }

    /// Returns whether at least one unresolved blocker exists.
    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Active)
    }
}

/// The coordinated workflow, gate and blocker state for one execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionState {
    workflow: WorkflowState,
    gate: GateState,
    blocker: BlockerState,
}

impl ExecutionState {
    /// Creates a state triple after checking cross-field invariants.
    pub const fn new(
        workflow: WorkflowState,
        gate: GateState,
        blocker: BlockerState,
    ) -> Result<Self, ValidationError> {
        let valid = match workflow {
            WorkflowState::Pending => {
                matches!(gate, GateState::Pending) && matches!(blocker, BlockerState::Clear)
            }
            WorkflowState::Running | WorkflowState::Paused => {
                matches!(gate, GateState::Pending | GateState::InProgress) && !blocker.is_active()
            }
            WorkflowState::Blocked => matches!(gate, GateState::Blocked) && blocker.is_active(),
            WorkflowState::Completed => {
                matches!(gate, GateState::Passed | GateState::Skipped) && !blocker.is_active()
            }
            WorkflowState::Failed => matches!(gate, GateState::Failed) && !blocker.is_active(),
            WorkflowState::Cancelled => !blocker.is_active(),
        };

        if valid {
            Ok(Self {
                workflow,
                gate,
                blocker,
            })
        } else {
            Err(ValidationError::InvalidStateCombination {
                reason: "workflow, gate and blocker states are inconsistent",
            })
        }
    }

    /// Fallible constructor alias for callers at a parsing boundary.
    pub const fn try_new(
        workflow: WorkflowState,
        gate: GateState,
        blocker: BlockerState,
    ) -> Result<Self, ValidationError> {
        Self::new(workflow, gate, blocker)
    }

    /// Returns the workflow lifecycle state.
    #[must_use]
    pub const fn workflow(self) -> WorkflowState {
        self.workflow
    }

    /// Returns the gate state.
    #[must_use]
    pub const fn gate(self) -> GateState {
        self.gate
    }

    /// Returns the blocker state.
    #[must_use]
    pub const fn blocker(self) -> BlockerState {
        self.blocker
    }
}

/// Validates a mode/profile pair and keeps the dimensions independent.
pub const fn validate_mode_and_profile(
    _operating_mode: OperatingMode,
    _execution_profile: ExecutionProfile,
) -> Result<(), ValidationError> {
    // No pair is rejected here. Policies may impose a stricter project rule,
    // but lifecycle mode and execution depth are separate domain dimensions.
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{
        BlockerState, ExecutionState, GateState, WorkflowState, validate_mode_and_profile,
    };
    use crate::{ExecutionProfile, OperatingMode, ValidationError};

    #[test]
    fn all_state_values_have_safe_canonical_parsing() {
        for value in [
            WorkflowState::Pending,
            WorkflowState::Running,
            WorkflowState::Paused,
            WorkflowState::Blocked,
            WorkflowState::Completed,
            WorkflowState::Failed,
            WorkflowState::Cancelled,
        ] {
            assert_eq!(WorkflowState::from_str(value.as_str()).unwrap(), value);
        }
        for value in [
            GateState::Pending,
            GateState::InProgress,
            GateState::Passed,
            GateState::Failed,
            GateState::Blocked,
            GateState::Skipped,
        ] {
            assert_eq!(GateState::from_str(value.as_str()).unwrap(), value);
        }
        for value in [
            BlockerState::Clear,
            BlockerState::Active,
            BlockerState::Resolved,
        ] {
            assert_eq!(BlockerState::from_str(value.as_str()).unwrap(), value);
        }
    }

    #[test]
    fn unknown_state_values_fail_closed() {
        assert!(matches!(
            GateState::from_str("UNKNOWN"),
            Err(ValidationError::UnknownDomainValue {
                field: "gate_state",
                ..
            })
        ));
        assert!(WorkflowState::from_str("running ").is_err());
    }

    #[test]
    fn lifecycle_transitions_accept_progress_and_reject_terminal_changes() {
        assert_eq!(
            WorkflowState::Pending
                .transition_to(WorkflowState::Running)
                .unwrap(),
            WorkflowState::Running
        );
        assert!(
            WorkflowState::Completed
                .transition_to(WorkflowState::Running)
                .is_err()
        );
        assert!(WorkflowState::Completed.is_terminal());
        assert!(
            GateState::Pending
                .transition_to(GateState::InProgress)
                .is_ok()
        );
        assert!(GateState::Passed.transition_to(GateState::Failed).is_err());
        assert!(
            BlockerState::Clear
                .transition_to(BlockerState::Active)
                .is_ok()
        );
        assert!(
            BlockerState::Clear
                .transition_to(BlockerState::Resolved)
                .is_err()
        );
    }

    #[test]
    fn execution_state_enforces_cross_field_invariants() {
        let state = ExecutionState::new(
            WorkflowState::Blocked,
            GateState::Blocked,
            BlockerState::Active,
        )
        .unwrap();
        assert_eq!(state.workflow(), WorkflowState::Blocked);
        assert_eq!(state.gate(), GateState::Blocked);
        assert_eq!(state.blocker(), BlockerState::Active);
        assert!(
            ExecutionState::try_new(
                WorkflowState::Pending,
                GateState::Pending,
                BlockerState::Clear,
            )
            .is_ok()
        );

        assert!(
            ExecutionState::new(
                WorkflowState::Completed,
                GateState::Failed,
                BlockerState::Clear,
            )
            .is_err()
        );
        assert!(
            ExecutionState::new(
                WorkflowState::Running,
                GateState::InProgress,
                BlockerState::Active,
            )
            .is_err()
        );
    }

    #[test]
    fn every_mode_and_profile_pair_is_valid_without_policy_overrides() {
        for mode in [
            OperatingMode::Development,
            OperatingMode::Hardening,
            OperatingMode::ReleaseQualification,
        ] {
            for profile in [
                ExecutionProfile::FastPath,
                ExecutionProfile::NormalPath,
                ExecutionProfile::FullPath,
            ] {
                assert!(validate_mode_and_profile(mode, profile).is_ok());
            }
        }
    }
}
