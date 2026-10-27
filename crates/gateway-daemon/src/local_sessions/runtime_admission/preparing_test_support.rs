//! Test host around the actual coordinator's preparing phase. No allocation,
//! consent, model output or completion receipt is manufactured by this adapter.
use super::*;
use std::cell::Cell;

pub(super) struct NativePreparingProbe<'a> {
    pub(super) local: LocalSessionHost,
    pub(super) plan: SessionResourcePlan,
    pub(super) completed: &'a Cell<bool>,
    pub(super) callback: &'a dyn Fn(
        &LocalSessionHost,
        &SessionCheckpoint,
        &mut SessionPhaseResources,
    ) -> Result<(), SessionError>,
}
pub(super) struct NativeConsentPreparingProbe<'a> {
    pub(super) local: LocalSessionHost,
    pub(super) plan: SessionResourcePlan,
    pub(super) completed: &'a Cell<bool>,
    pub(super) callback: &'a dyn Fn(
        &LocalSessionHost,
        &SessionCheckpoint,
        &mut SessionPhaseResources,
        ConnectorPreparationContext<'_>,
    ) -> Result<ConnectorPreparation, SessionError>,
}
macro_rules! delegate_preparing_host {
    ($probe:ident) => {
        impl InteractionAuthorityPort for $probe<'_> {
            fn load_consent(
                &self,
                owner: &OwnerBinding,
                record: &ConsentRecordRef,
            ) -> Result<ConsentStatus, SessionError> {
                self.local.load_consent(owner, record)
            }
            fn live_status(&self, grant: &VerifiedConsent) -> Result<ConsentStatus, SessionError> {
                self.local.live_status(grant)
            }
        }
        impl StructuredSessionHost for $probe<'_> {
            fn now_ms(&self) -> Result<u64, SessionError> {
                self.local.now_ms()
            }
            fn register(
                &self,
                owner: &OwnerBinding,
                intent: Intent,
                execution: RequestedExecution,
            ) -> Result<(SupportedSessionGoal, SessionBudget), SessionError> {
                self.local.register(owner, intent, execution)
            }
            fn prepare(&self, _: &SessionCheckpoint) -> Result<PreparedTask, SessionError> {
                Err(SessionError::UnsupportedCapability)
            }
            fn disclose(
                &self,
                owner: &OwnerBinding,
                session: &SessionId,
            ) -> Result<(), SessionError> {
                self.local.disclose(owner, session)
            }
            fn issuer(
                &self,
                owner: &OwnerBinding,
                issuer: &PrincipalId,
            ) -> Result<(), SessionError> {
                self.local.issuer(owner, issuer)
            }
            fn resource_limits(
                &self,
                owner: &OwnerBinding,
                goal: &SupportedSessionGoal,
            ) -> Result<Option<SessionResourceLimits>, SessionError> {
                self.local.resource_limits(owner, goal)
            }
            fn connector_runtime(&self) -> Result<&dyn ConnectorSessionRuntimePort, SessionError> {
                Ok(self)
            }
        }
    };
}
delegate_preparing_host!(NativePreparingProbe);
delegate_preparing_host!(NativeConsentPreparingProbe);
impl ConnectorSessionRuntimePort for NativePreparingProbe<'_> {
    fn resource_plan(
        &self,
        _: &SessionCheckpoint,
        _: &PlanStepId,
    ) -> Result<SessionResourcePlan, SessionError> {
        Ok(self.plan.clone())
    }
    fn initial_assessment(
        &self,
        checkpoint: &SessionCheckpoint,
    ) -> Result<InitialAssessment, SessionError> {
        self.local.connector_runtime.initial_assessment(checkpoint)
    }
    fn prepare_step(
        &self,
        _: &SessionCheckpoint,
        _: &PlanStepId,
    ) -> Result<PreparedConnectorTurn, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
    fn prepare_authorized_step(
        &self,
        checkpoint: &SessionCheckpoint,
        _: &PlanStepId,
        resources: &mut SessionPhaseResources,
        _: ConnectorPreparationContext<'_>,
    ) -> Result<ConnectorPreparation, SessionError> {
        (self.callback)(&self.local, checkpoint, resources)
            .expect("the complete native preparing callback must succeed before the intentional unsupported-turn boundary");
        self.completed.set(true);
        Err(SessionError::UnsupportedCapability)
    }
    fn execute_step(
        &self,
        _: &SessionCheckpoint,
        _: &PlanStepId,
        _: &mut SessionPhaseResources,
        _: &DispatchId,
        _: &PreparedConnectorTurn,
    ) -> Result<ConnectorSessionTurn, SessionError> {
        panic!("preparing-only qualification cannot execute a connector proposal")
    }
    fn verify_goal(
        &self,
        _: &SessionCheckpoint,
        _: &PlanStepId,
        _: &DispatchId,
        _: &[PersistedConnectorReadback],
    ) -> Result<Option<gateway_application::closed_loop::VerifiedOutcomeReceipt>, SessionError>
    {
        Err(SessionError::UnsupportedCapability)
    }
}

impl ConnectorSessionRuntimePort for NativeConsentPreparingProbe<'_> {
    fn verify_pre_execution_goal(
        &self,
        checkpoint: &SessionCheckpoint,
        data: &PreExecutionGoalData,
    ) -> Result<Option<gateway_application::closed_loop::VerifiedOutcomeReceipt>, SessionError> {
        self.local.connector_runtime.verify_pre_execution_goal(checkpoint, data)
    }
    fn resource_plan(
        &self,
        _: &SessionCheckpoint,
        _: &PlanStepId,
    ) -> Result<SessionResourcePlan, SessionError> {
        Ok(self.plan.clone())
    }
    fn initial_assessment(
        &self,
        checkpoint: &SessionCheckpoint,
    ) -> Result<InitialAssessment, SessionError> {
        self.local.connector_runtime.initial_assessment(checkpoint)
    }
    fn prepare_step(
        &self,
        _: &SessionCheckpoint,
        _: &PlanStepId,
    ) -> Result<PreparedConnectorTurn, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
    fn prepare_authorized_step(
        &self,
        checkpoint: &SessionCheckpoint,
        _: &PlanStepId,
        resources: &mut SessionPhaseResources,
        context: ConnectorPreparationContext<'_>,
    ) -> Result<ConnectorPreparation, SessionError> {
        let preparation = (self.callback)(&self.local, checkpoint, resources, context)
            .expect("the complete native consent preparation must succeed before returning it to the actual coordinator");
        self.completed.set(true);
        Ok(preparation)
    }
    fn execute_step(
        &self,
        _: &SessionCheckpoint,
        _: &PlanStepId,
        _: &mut SessionPhaseResources,
        _: &DispatchId,
        _: &PreparedConnectorTurn,
    ) -> Result<ConnectorSessionTurn, SessionError> {
        panic!("preparing-only qualification cannot execute a connector proposal")
    }
    fn verify_goal(
        &self,
        _: &SessionCheckpoint,
        _: &PlanStepId,
        _: &DispatchId,
        _: &[PersistedConnectorReadback],
    ) -> Result<Option<gateway_application::closed_loop::VerifiedOutcomeReceipt>, SessionError>
    {
        Err(SessionError::UnsupportedCapability)
    }
}
