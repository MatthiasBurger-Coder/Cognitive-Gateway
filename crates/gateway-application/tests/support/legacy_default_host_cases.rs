//! Compatibility of hosts that implement only the original artifact boundary.
use super::*;

struct LegacyHost(Host);

impl InteractionAuthorityPort for LegacyHost {
    fn load_consent(
        &self,
        owner: &OwnerBinding,
        reference: &ConsentRecordRef,
    ) -> Result<ConsentStatus, SessionError> {
        self.0.load_consent(owner, reference)
    }

    fn live_status(&self, consent: &VerifiedConsent) -> Result<ConsentStatus, SessionError> {
        self.0.live_status(consent)
    }
}

// Deliberately omit both additive methods: registration must delegate to the
// original register operation, and no connector preparation facet is exposed.
impl StructuredSessionHost for LegacyHost {
    fn now_ms(&self) -> Result<u64, SessionError> {
        self.0.now_ms()
    }

    fn register(
        &self,
        owner: &OwnerBinding,
        intent: Intent,
        execution: RequestedExecution,
    ) -> Result<(SupportedArtifactGoal, SessionBudget), SessionError> {
        self.0.register(owner, intent, execution)
    }

    fn prepare(&self, checkpoint: &SessionCheckpoint) -> Result<PreparedTask, SessionError> {
        self.0.prepare(checkpoint)
    }

    fn disclose(&self, owner: &OwnerBinding, session: &SessionId) -> Result<(), SessionError> {
        self.0.disclose(owner, session)
    }

    fn issuer(&self, owner: &OwnerBinding, issuer: &PrincipalId) -> Result<(), SessionError> {
        self.0.issuer(owner, issuer)
    }
}

#[test]
fn original_host_default_registration_and_absent_connector_facet_still_complete_native_artifacts() {
    let coordinator = SessionCoordinator::new(Repository::default(), LegacyHost(Host::new(false)));
    assert!(coordinator.host.goal_observation_runtime().is_none());
    let started = coordinator
        .execute(
            &owner(),
            SessionCommand::Start {
                command: CommandId::new("legacy-default-start").unwrap(),
                intent: goal(false).intent().clone(),
                execution: selection(),
            },
        )
        .unwrap();
    let checkpoint = coordinator.details(&owner(), &started.session).unwrap();
    assert!(matches!(checkpoint.goal, SupportedSessionGoal::Artifact(_)));
    assert!(checkpoint.prepared_before.is_none());
    let completed = coordinator
        .execute(
            &owner(),
            SessionCommand::Continue {
                at: mutation(&started, "legacy-default-continue"),
            },
        )
        .unwrap();
    assert_eq!(completed.state, SessionState::Completed);
    assert!(completed.final_evidence.is_some());
    let retained = coordinator.details(&owner(), &started.session).unwrap();
    assert!(retained.artifact.is_some());
    assert!(retained.prepared_before.is_none());
    assert!(retained.pre_execution_goal.is_none());
    assert!(coordinator.host.0.prepares.load(Ordering::SeqCst) > 0);
}
