//! Connector and artifact recoveries deliberately share the SAME existing
//! SessionCoordinator<Repository, Host> instantiation and actual native inputs.
use super::*;
use gateway_application::context_application::CompiledStep;
use gateway_application::sessions::scoped_snapshots::*;

pub(super) struct ConnectorRegistration {
    pub(super) goal: SupportedConnectorGoal,
    compiled: CompiledStep,
    capture: bool,
}
impl Host {
    fn connector_capture(capture: bool) -> Self {
        let native = fixture::Fixture::new();
        let compiled = native.compile().unwrap();
        let input = native.resolved.snapshot.input();
        let goal = SupportedConnectorGoal::from_host_registration(
            Intent::new(
                IntentId::new("mixed-native-before").unwrap(),
                input.desired.clone(),
            ),
            input.scope.clone(),
            content_reference(
                "mixed-native-plan",
                input.plan.to_json().unwrap().as_bytes(),
            )
            .unwrap(),
            vec![compiled.policy().step.clone()],
            input
                .desired
                .conditions()
                .iter()
                .map(|condition| {
                    ConditionObservationMapping::new(
                        condition.id().clone(),
                        SourceId::new("shared-state-provider").unwrap(),
                        CapabilityId::new("snapshot.read").unwrap(),
                        gateway_application::connectors::ConnectorOperationClass::Read,
                    )
                    .unwrap()
                })
                .collect(),
        )
        .unwrap();
        let mut result = Host::new(false);
        result.now.store(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            Ordering::SeqCst,
        );
        result.connector = Some(ConnectorRegistration {
            goal,
            compiled,
            capture,
        });
        result
    }
}
fn readonly_amounts() -> SessionResourceAmounts {
    SessionResourceAmounts {
        connector_calls: 2,
        retrieval_results: 16,
        retrieval_tokens: 10_000,
        ..Default::default()
    }
}
impl ReadonlyGoalObservationRuntimePort for Host {
    fn resource_limits(
        &self,
        _: &SessionCheckpoint,
    ) -> Result<SessionResourceLimits, SessionError> {
        self.connector
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        Ok(SessionResourceLimits {
            cost_unit: ReferenceId::new("LOCAL").unwrap(),
            amounts: readonly_amounts(),
        })
    }
    fn preparation_budget(
        &self,
        _: &SessionCheckpoint,
    ) -> Result<SessionResourceAmounts, SessionError> {
        self.connector
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        Ok(readonly_amounts())
    }
    fn prepare_readonly(
        &self,
        cp: &SessionCheckpoint,
        phase: &mut SessionPhaseResources,
    ) -> Result<Option<ReadonlyPreparation>, SessionError> {
        let connector = self
            .connector
            .as_ref()
            .ok_or(SessionError::UnsupportedCapability)?;
        self.prepares.fetch_add(1, Ordering::SeqCst);
        let allocation = phase.allocate(readonly_amounts())?;
        assert_eq!(allocation.binding().revision, cp.snapshot.revision);
        if !connector.capture {
            return Err(SessionError::Unavailable);
        }
        let mut data = crate::complete_snapshot_fixture::Fixture::atomic_in_scope(
            allocation.binding().scope.clone(),
        );
        let native: serde_json::Value =
            serde_json::from_str(&connector.compiled.to_json().unwrap()).unwrap();
        let runtime: SnapshotPin = serde_json::from_value(
            serde_json::json!({"id":"mixed-native-runtime","scope":allocation.binding().scope,
            "contract":"cg.connector-snapshot-runtime","contract_version":"1.0","revision":"1",
            "digest":context_content_digest(&serde_json::to_vec(&native).unwrap())}),
        )
        .unwrap();
        let captured = crate::prepared_before_fixture::capture(
            &mut data,
            &allocation,
            &connector.compiled,
            runtime,
            self.now.load(Ordering::SeqCst),
        );
        Ok(Some(ReadonlyPreparation::BeforeCaptured(Box::new(
            captured,
        ))))
    }
}
fn connector(capture: bool) -> SessionCoordinator<Repository, Host> {
    SessionCoordinator::new(Repository::default(), Host::connector_capture(capture))
}
#[test]
fn same_host_recovers_registered_connector_before_claim_and_expired_uncommitted_claim_without_retry()
 {
    let c = connector(false);
    let runnable = start(&c);
    assert_eq!(c.recover(&owner(), &runnable.session).unwrap(), runnable);
    assert_eq!(c.host.prepares.load(Ordering::SeqCst), 0);
    assert_eq!(continue_task(&c, &runnable), Err(SessionError::Unavailable));
    let claim = c.details(&owner(), &runnable.session).unwrap();
    assert_eq!(claim.snapshot.state, SessionState::Dispatching);
    assert!(claim.prepared_before.is_none());
    assert_eq!(
        claim.resource_ledger.as_ref().unwrap().reservations().len(),
        1
    );
    assert_eq!(
        c.recover(&owner(), &runnable.session).unwrap(),
        claim.snapshot
    );
    c.host
        .now
        .store(claim.lease_until_ms.unwrap(), Ordering::SeqCst);
    let failed = c.recover(&owner(), &runnable.session).unwrap();
    assert_eq!(failed.state, SessionState::Failed);
    assert_eq!(c.recover(&owner(), &runnable.session).unwrap(), failed);
    let stopped = c.details(&owner(), &runnable.session).unwrap();
    assert_eq!(stopped.budget, claim.budget);
    assert_eq!(stopped.resource_ledger, claim.resource_ledger);
    assert!(c.repository.rows.lock().unwrap().records.is_empty());
    assert_eq!(c.host.prepares.load(Ordering::SeqCst), 1);
}
#[test]
fn same_host_historical_before_recovery_denies_corruption_and_observes_real_concurrent_cancel() {
    let c = connector(true);
    let initial = start(&c);
    let prepared = continue_task(&c, &initial).unwrap();
    let original = c.details(&owner(), &prepared.session).unwrap();
    let marker = original.prepared_before.clone().unwrap();
    c.host.now.fetch_add(90_000, Ordering::SeqCst);
    assert_eq!(c.recover(&owner(), &prepared.session).unwrap(), prepared);
    for kind in [1, 2] {
        c.repository.corrupt.store(kind, Ordering::SeqCst);
        assert_eq!(
            c.recover(&owner(), &prepared.session),
            Err(SessionError::InvalidInteraction)
        );
    }
    c.repository.corrupt.store(0, Ordering::SeqCst);
    let peer = SessionCoordinator::new(
        Repository {
            rows: c.repository.rows.clone(),
            ..Default::default()
        },
        Host::new(false),
    );
    let at = mutation(&prepared, "cancel-during-historical-read");
    *c.repository.artifact_read_hook.lock().unwrap() = Some(Box::new(move || {
        let result = peer
            .execute(&owner(), SessionCommand::Cancel { at })
            .unwrap();
        assert_eq!(result.state, SessionState::Cancelled);
    }));
    assert_eq!(
        c.recover(&owner(), &prepared.session),
        Err(SessionError::StaleRevision)
    );
    let cancelled = c.details(&owner(), &prepared.session).unwrap();
    assert_eq!(cancelled.snapshot.state, SessionState::Cancelled);
    assert_eq!(cancelled.prepared_before, Some(marker));
    assert_eq!(cancelled.budget, original.budget);
    assert_eq!(cancelled.resource_ledger, original.resource_ledger);
    assert_eq!(
        c.recover(&owner(), &prepared.session).unwrap(),
        cancelled.snapshot
    );
    assert_eq!(c.host.prepares.load(Ordering::SeqCst), 1);
    assert!(cancelled.snapshot.final_evidence.is_none());
}
