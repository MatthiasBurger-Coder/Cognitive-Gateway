use super::*;
use gateway_application::{
    connector_policy::{Cg09ConnectorAdmission, ConnectorPolicySnapshot, ConnectorPolicySource},
    connectors::{
        ConnectorAdmissionPort, ConnectorEffect, ConnectorFailure, ConnectorOperationClass,
        ConnectorRequest, ConnectorTarget,
    },
    resolution::ContentFingerprint,
};
use gateway_domain::*;
use gateway_policy::{Approval, PolicyAuthority, ProcessReadiness, StepFacts};
use std::{
    cell::Cell,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

fn owner() -> OwnerBinding {
    super::super::tests::owner()
}
fn plan() -> SessionResourcePlan {
    SessionResourcePlan {
        preparation: SessionResourceAmounts {
            connector_calls: 4,
            ..Default::default()
        },
        ..Default::default()
    }
}
fn goal(scope: ContextScopeId) -> SupportedConnectorGoal {
    let intent: Intent = serde_json::from_str(include_str!(
        "../../../../../tests/fixtures/declarative-cli/intent.json"
    ))
    .unwrap();
    SupportedConnectorGoal::from_host_registration(
        intent,
        scope,
        content_reference("audit-plan", b"audit-plan").unwrap(),
        vec![PlanStepId::new("audit-step").unwrap()],
        vec![
            ConditionObservationMapping::new(
                ConditionId::new("clean").unwrap(),
                SourceId::new("audit-source").unwrap(),
                CapabilityId::new("audit.read").unwrap(),
                ConnectorOperationClass::Read,
            )
            .unwrap(),
        ],
    )
    .unwrap()
}
struct Probe<F> {
    scope: ContextScopeId,
    callback: F,
    called: Cell<bool>,
}
impl<F> InteractionAuthorityPort for Probe<F> {
    fn load_consent(
        &self,
        _: &OwnerBinding,
        _: &ConsentRecordRef,
    ) -> Result<ConsentStatus, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
    fn live_status(&self, _: &VerifiedConsent) -> Result<ConsentStatus, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
}
impl<F: Fn(&SessionCheckpoint, &SessionResourceAllocation)> StructuredSessionHost for Probe<F> {
    fn now_ms(&self) -> Result<u64, SessionError> {
        Ok(100)
    }
    fn register(
        &self,
        owner: &OwnerBinding,
        intent: Intent,
        _: RequestedExecution,
    ) -> Result<(SupportedSessionGoal, SessionBudget), SessionError> {
        assert_eq!(owner, &self::owner());
        let goal = goal(self.scope.clone());
        assert_eq!(goal.intent(), &intent);
        Ok((
            SupportedSessionGoal::connector(goal),
            SessionBudget {
                actions: 0,
                retries: 0,
                max_actions: 2,
                max_retries: 1,
                deadline_ms: 10000,
            },
        ))
    }
    fn prepare(&self, _: &SessionCheckpoint) -> Result<PreparedTask, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
    fn disclose(&self, owner: &OwnerBinding, _: &SessionId) -> Result<(), SessionError> {
        if owner == &self::owner() {
            Ok(())
        } else {
            Err(SessionError::ScopeDenied)
        }
    }
    fn issuer(&self, _: &OwnerBinding, _: &PrincipalId) -> Result<(), SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
    fn resource_limits(
        &self,
        _: &OwnerBinding,
        _: &SupportedSessionGoal,
    ) -> Result<Option<SessionResourceLimits>, SessionError> {
        Ok(Some(SessionResourceLimits {
            cost_unit: ReferenceId::new("audit-units").unwrap(),
            amounts: plan().total()?,
        }))
    }
    fn connector_runtime(&self) -> Result<&dyn ConnectorSessionRuntimePort, SessionError> {
        Ok(self)
    }
}
impl<F: Fn(&SessionCheckpoint, &SessionResourceAllocation)> ConnectorSessionRuntimePort
    for Probe<F>
{
    fn resource_plan(
        &self,
        _: &SessionCheckpoint,
        _: &PlanStepId,
    ) -> Result<SessionResourcePlan, SessionError> {
        Ok(plan())
    }
    fn initial_assessment(
        &self,
        cp: &SessionCheckpoint,
    ) -> Result<InitialAssessment, SessionError> {
        Ok(InitialAssessment {
            goal: content_reference(
                "supported-goal",
                &serde_json::to_vec(cp.goal.intent()).unwrap(),
            )?,
            authority: cp.goal.as_connector().unwrap().plan().clone(),
            outcome: "UNKNOWN".into(),
        })
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
        cp: &SessionCheckpoint,
        _: &PlanStepId,
        resources: &mut SessionPhaseResources,
        _: ConnectorPreparationContext<'_>,
    ) -> Result<ConnectorPreparation, SessionError> {
        self.called.set(true);
        let allocation = resources.allocate(plan().preparation)?;
        (self.callback)(cp, &allocation);
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
        Err(SessionError::UnsupportedCapability)
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
fn with_pg_allocation(
    test: impl Fn(&PostgresSessionStore, &SessionCheckpoint, &SessionResourceAllocation),
) {
    let Ok(connection) = std::env::var("CG_COGNITIVE_TEST_DATABASE") else {
        return;
    };
    let scope = ContextScopeId::new(format!(
        "audit-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
    .unwrap();
    let probe_store =
        PostgresSessionStore::new(CognitiveStore::connect(&connection, scope.clone()).unwrap());
    let coordinator = SessionCoordinator::new(
        PostgresSessionStore::new(CognitiveStore::connect(&connection, scope.clone()).unwrap()),
        Probe {
            scope: scope.clone(),
            called: Cell::new(false),
            callback: |cp: &SessionCheckpoint, a: &SessionResourceAllocation| {
                test(&probe_store, cp, a)
            },
        },
    );
    let started = coordinator
        .execute(
            &owner(),
            SessionCommand::Start {
                command: CommandId::new("start-audit").unwrap(),
                intent: goal(scope).intent().clone(),
                execution: RequestedExecution {
                    mode: OperatingMode::Development,
                    profile: ExecutionProfile::FullPath,
                },
            },
        )
        .unwrap();
    assert!(
        coordinator
            .execute(
                &owner(),
                SessionCommand::Continue {
                    at: Mutation {
                        session: started.session,
                        expected_revision: started.revision,
                        command: CommandId::new("continue-audit").unwrap()
                    }
                }
            )
            .is_err()
    );
    assert!(coordinator.host.called.get());
}
fn basis(scope: &ContextScopeId) -> ResolutionBasis {
    let fingerprint = ContentFingerprint::of_bytes(b"captured-real-source-facts");
    ResolutionBasis {
        plan: PlanId::new("audit-plan").unwrap(),
        plan_fingerprint: fingerprint.clone(),
        admission: Some(ReferenceId::new("runtime-admission").unwrap()),
        situation: SituationId::new("audit-situation").unwrap(),
        scope: scope.clone(),
        situation_fingerprint: fingerprint.clone(),
        registry_fingerprint: fingerprint.clone(),
        process_catalog_fingerprint: fingerprint.clone(),
        process_state_fingerprint: fingerprint,
        rule_version: SchemaVersion::V1,
    }
}
fn pin(scope: &ContextScopeId, id: &str) -> ResolvedReference<ReferenceId> {
    ResolvedReference {
        id: ReferenceId::new(id).unwrap(),
        scope: scope.clone(),
        contract: ReferenceId::new(format!("cg.{id}")).unwrap(),
        contract_version: SchemaVersion::V1,
        revision: ReferenceId::new("1").unwrap(),
        digest: ContentDigest::new("a".repeat(64)).unwrap(),
    }
}
fn pins(scope: &ContextScopeId) -> ConnectorPolicyAuditPins {
    ConnectorPolicyAuditPins {
        canonical_policy: pin(scope, "policy"),
        runtime: pin(scope, "runtime"),
        manifest: pin(scope, "manifest"),
        mapping: pin(scope, "mapping"),
        producer: vec![pin(scope, "producer")],
    }
}
fn context(a: &SessionResourceAllocation) -> ConnectorPolicyAuditContext {
    ConnectorPolicyAuditContext::from_allocation(
        a,
        basis(&a.binding().scope),
        pins(&a.binding().scope),
    )
    .unwrap()
}
fn request(a: &SessionResourceAllocation) -> ConnectorRequest {
    ConnectorRequest {
        target: ConnectorTarget {
            connector: NonEmptyText::new("audit.connector").unwrap(),
            scope: a.binding().scope.clone(),
        },
        capability: CapabilityId::new("audit.read").unwrap(),
        class: ConnectorOperationClass::Read,
        inventory_revision: ContentDigest::new("b".repeat(64)).unwrap(),
        arguments: [(
            NonEmptyText::new("private-input").unwrap(),
            TypedValue::String(NonEmptyText::new("argument-canary").unwrap()),
        )]
        .into(),
        task: TaskId::new("audit-task").unwrap(),
        step: a.binding().step.clone(),
        execution: NonEmptyText::new("audit-execution").unwrap(),
        correlation: NonEmptyText::new("audit-correlation").unwrap(),
    }
}
fn snapshot(a: &SessionResourceAllocation) -> ConnectorPolicySnapshot {
    let capability = CapabilityId::new("audit.read").unwrap();
    ConnectorPolicySnapshot {
        step: a.binding().step.clone(),
        resolved: true,
        has_prerequisites: false,
        constraints: Default::default(),
        preconditions: Default::default(),
        authority: PolicyAuthority {
            policies: vec![
                PolicyDefinition::new(
                    PolicyId::new("audit-policy").unwrap(),
                    "private-policy-description-canary",
                    [capability.clone()],
                )
                .unwrap(),
            ],
            capabilities: [(
                capability.clone(),
                CapabilityDefinition::new(capability.clone(), CapabilityClass::Inspect),
            )]
            .into(),
            ..Default::default()
        },
        facts: StepFacts {
            authorizations: [(capability, Approval::Granted)].into(),
            evidence: ["private-evidence-canary".into()].into(),
            ..Default::default()
        },
        operating_mode: OperatingMode::Development,
        execution_profile: ExecutionProfile::FullPath,
        process: ProcessReadiness::NotApplicable,
        consent_required: false,
        administrative_capabilities: Default::default(),
        policy: PolicyId::new("audit-policy").unwrap(),
        policy_revision: ContentDigest::new("c".repeat(64)).unwrap(),
        consent_receipt: Some(NonEmptyText::new("consent-canary").unwrap()),
        budget_reservation: NonEmptyText::new(a.reservation_id().as_str()).unwrap(),
        deadline: UnixTimestamp::new(10000),
    }
}
fn denied(request: &ConnectorRequest) -> ConnectorFailure {
    ConnectorFailure {
        code: ConnectorFailureCode::Unavailable,
        effect: ConnectorEffect::NotDispatched,
        correlation: request.correlation.clone(),
    }
}
struct Source<'a> {
    change_facts_on_audit: bool,
    last_error: Option<&'a Mutex<Option<SessionError>>>,
    store: &'a PostgresSessionStore,
    context: &'a ConnectorPolicyAuditContext,
    snapshot: &'a Mutex<ConnectorPolicySnapshot>,
    owner: OwnerBinding,
}
impl ConnectorPolicySource for Source<'_> {
    fn admit_target(&self, _: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        Ok(())
    }
    fn authorize(
        &self,
        _: &ConnectorRequest,
        _: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        Ok(self.snapshot.lock().unwrap().clone())
    }
    fn revalidate(
        &self,
        _: &ConnectorDispatch,
        _: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        Ok(self.snapshot.lock().unwrap().clone())
    }
    fn record_decision(&self, e: &ConnectorPolicyEvaluation<'_>) -> Result<(), ConnectorFailure> {
        if self.change_facts_on_audit {
            for authorization in self
                .snapshot
                .lock()
                .unwrap()
                .facts
                .authorizations
                .values_mut()
            {
                *authorization = Approval::Denied;
            }
        }
        let result = self
            .store
            .record_connector_policy_decision(&self.owner, self.context, e);
        if let (Err(error), Some(last_error)) = (&result, self.last_error) {
            *last_error.lock().unwrap() = Some(*error);
        }
        result.map(|_| ()).map_err(|_| denied(e.request()))
    }
}
fn journal(store: &PostgresSessionStore) -> AuditJournal {
    store
        .store
        .transact(
            KIND,
            &AuditJournal::initial(store.store.scope().clone()),
            |journal| {
                journal.validate(store.store.scope())?;
                Ok::<_, SessionError>(AuditJournal {
                    schema_version: journal.schema_version,
                    scope: journal.scope.clone(),
                    entries: journal.entries.clone(),
                })
            },
        )
        .unwrap()
}
fn row(scope: &ContextScopeId, kind: &str) -> Option<(i64, String, String)> {
    let connection = std::env::var("CG_COGNITIVE_TEST_DATABASE").unwrap();
    let mut client = postgres::Client::connect(&connection, postgres::NoTls).unwrap();
    client
        .query_opt(
            "SELECT revision,payload,digest FROM cg_cognitive_journals WHERE scope=$1 AND kind=$2",
            &[&scope.as_str(), &kind],
        )
        .unwrap()
        .map(|r| (r.get(0), r.get(1), r.get(2)))
}

#[test]
fn postgres_audit_appends_exact_evaluations_without_revising_live_claim_or_exposing_payloads() {
    with_pg_allocation(|store, cp, allocation| {
        let before = store.load(&cp.owner, &cp.snapshot.session).unwrap();
        let task_before = row(store.store.scope(), "task-sessions-v2").unwrap();
        assert_eq!(&before.checkpoint, cp);
        let context = context(allocation);
        let snapshot = Mutex::new(snapshot(allocation));
        let request = request(allocation);
        let admission = Cg09ConnectorAdmission::new(Source {
            change_facts_on_audit: false,
            last_error: None,
            store,
            context: &context,
            snapshot: &snapshot,
            owner: cp.owner.clone(),
        });
        let source = SourceId::new("audit-source").unwrap();
        let dispatch = admission.authorize(&request, Some(&source)).unwrap();
        admission.revalidate(&dispatch, Some(&source)).unwrap();
        let first = journal(store);
        assert_eq!(first.entries.len(), 2);
        assert_eq!(first.entries[0].ordinal, 1);
        assert_eq!(first.entries[1].ordinal, 2);
        assert!(matches!(first.entries[0].decision.stage, Stage::Authorize));
        assert!(matches!(
            first.entries[1].decision.stage,
            Stage::PreEffect(_)
        ));
        assert_ne!(
            first.entries[0].decision.snapshot.policy_revision,
            first.entries[0]
                .decision
                .context
                .pins
                .canonical_policy
                .digest,
        );
        let captured = &first.entries[0].decision.context.allocation;
        assert_eq!(captured.binding, *allocation.binding());
        assert_eq!(captured.claim_revision, allocation.claim_revision());
        assert_eq!(captured.fence, allocation.fence().value());
        assert_eq!(captured.phase, allocation.phase());
        assert_eq!(captured.amounts, *allocation.amounts());
        assert_eq!(&captured.cost_unit, allocation.cost_unit());
        assert_eq!(first.entries[0].decision.source.as_ref(), Some(&source));
        snapshot.lock().unwrap().step = PlanStepId::new("wrong-step").unwrap();
        assert!(admission.authorize(&request, Some(&source)).is_err());
        snapshot.lock().unwrap().step = request.step.clone();
        let mut stale = dispatch.clone();
        stale.budget_reservation = NonEmptyText::new("foreign-reservation-canary").unwrap();
        assert!(admission.revalidate(&stale, Some(&source)).is_err());
        snapshot.lock().unwrap().budget_reservation =
            NonEmptyText::new("snapshot-reservation-canary").unwrap();
        assert!(admission.revalidate(&dispatch, Some(&source)).is_err());
        snapshot.lock().unwrap().budget_reservation = dispatch.budget_reservation.clone();
        snapshot
            .lock()
            .unwrap()
            .constraints
            .insert("constraint-payload-canary".into());
        assert!(admission.authorize(&request, Some(&source)).is_err());
        let stored = journal(store);
        assert_eq!(stored.entries.len(), 6);
        assert!(stored.entries[2].decision.report.is_none());
        assert!(matches!(
            stored.entries[2].decision.outcome,
            Outcome::Denied(_)
        ));
        assert_eq!(
            stored.entries[3].decision.report.as_ref().unwrap().decision,
            "ALLOW"
        );
        assert!(matches!(
            stored.entries[3].decision.stage,
            Stage::PreEffect(_)
        ));
        assert!(matches!(
            stored.entries[3].decision.outcome,
            Outcome::Denied(_)
        ));
        assert_eq!(
            stored.entries[4].decision.report.as_ref().unwrap().decision,
            "ALLOW"
        );
        assert!(matches!(
            stored.entries[4].decision.outcome,
            Outcome::Denied(_)
        ));
        let bytes = String::from_utf8(encoded(&stored).unwrap()).unwrap();
        for canary in [
            "argument-canary",
            "consent-canary",
            "foreign-reservation-canary",
            "snapshot-reservation-canary",
            "constraint-payload-canary",
            "private-policy-description-canary",
            "private-evidence-canary",
        ] {
            assert!(!bytes.contains(canary), "raw payload leaked into audit");
        }
        let after = store.load(&cp.owner, &cp.snapshot.session).unwrap();
        assert_eq!(after.checkpoint, before.checkpoint);
        assert_eq!(after.fence, before.fence);
        assert_eq!(
            row(store.store.scope(), "task-sessions-v2").unwrap(),
            task_before
        );
        assert!(row(store.store.scope(), KIND).unwrap().0 >= 5);
        let expected = row(store.store.scope(), KIND).unwrap();
        let mut foreign = cp.owner.clone();
        foreign.project = ProjectId::new("foreign-project").unwrap();
        let other = Cg09ConnectorAdmission::new(Source {
            change_facts_on_audit: false,
            last_error: None,
            store,
            context: &context,
            snapshot: &snapshot,
            owner: foreign,
        });
        assert!(other.authorize(&request, None).is_err());
        assert_eq!(row(store.store.scope(), KIND).unwrap(), expected);
        let foreign_scope = ContextScopeId::new(format!("foreign-{}", std::process::id())).unwrap();
        let connection = std::env::var("CG_COGNITIVE_TEST_DATABASE").unwrap();
        let foreign_store = PostgresSessionStore::new(
            CognitiveStore::connect(&connection, foreign_scope.clone()).unwrap(),
        );
        let other = Cg09ConnectorAdmission::new(Source {
            change_facts_on_audit: false,
            last_error: None,
            store: &foreign_store,
            context: &context,
            snapshot: &snapshot,
            owner: cp.owner.clone(),
        });
        assert!(other.authorize(&request, None).is_err());
        assert!(row(&foreign_scope, KIND).is_none());
    });
}

#[test]
fn postgres_audit_rejects_corrupt_metadata_entries_and_full_storage_without_overwriting() {
    with_pg_allocation(|store, cp, allocation| {
        let context = context(allocation);
        let snapshot = Mutex::new(snapshot(allocation));
        let request = request(allocation);
        let admission = Cg09ConnectorAdmission::new(Source {
            change_facts_on_audit: false,
            last_error: None,
            store,
            context: &context,
            snapshot: &snapshot,
            owner: cp.owner.clone(),
        });
        admission.authorize(&request, None).unwrap();
        let valid = journal(store);
        for defect in [
            "version", "scope", "digest", "ordinal", "reason", "capacity",
        ] {
            store
                .store
                .transact(
                    KIND,
                    &AuditJournal::initial(store.store.scope().clone()),
                    |j| {
                        *j = AuditJournal {
                            schema_version: valid.schema_version,
                            scope: valid.scope.clone(),
                            entries: valid.entries.clone(),
                        };
                        match defect {
                            "version" => j.schema_version = 2,
                            "scope" => j.scope = ContextScopeId::new("corrupt-scope").unwrap(),
                            "digest" => {
                                j.entries[0].digest = ContentDigest::new("0".repeat(64)).unwrap()
                            }
                            "ordinal" => {
                                j.entries[0].ordinal = 2;
                                j.entries[0].digest = j.entries[0].digest()?;
                            }
                            "reason" => {
                                j.entries[0].decision.report.as_mut().unwrap().findings[0].reason =
                                    "UNKNOWN_POLICY_REASON".into();
                                j.entries[0].digest = j.entries[0].digest()?;
                            }
                            "capacity" => {
                                let seed = j.entries[0].clone();
                                let count = (MAX_JOURNAL_BYTES / encoded(&seed)?.len())
                                    .min(MAX_EVENTS - 1)
                                    + 1;
                                for _ in 1..count {
                                    let mut e = seed.clone();
                                    e.ordinal = u64::try_from(j.entries.len() + 1).unwrap();
                                    e.previous = Some(j.entries.last().unwrap().digest.clone());
                                    e.digest = e.digest()?;
                                    j.entries.push(e);
                                }
                            }
                            _ => unreachable!(),
                        };
                        Ok::<_, SessionError>(())
                    },
                )
                .unwrap();
            let prior = row(store.store.scope(), KIND).unwrap();
            assert!(admission.authorize(&request, None).is_err(), "{defect}");
            assert_eq!(row(store.store.scope(), KIND).unwrap(), prior, "{defect}");
        }
    });
}

#[test]
fn context_requires_complete_scoped_pins_and_native_basis() {
    crate::session_resource_test_support::with_preparation_resources(
        ContextScopeId::new("audit-context-unit").unwrap(),
        plan(),
        |resources, _, _| {
            let allocation = resources.allocate(plan().preparation).unwrap();
            let scope = &allocation.binding().scope;
            let mut wrong = basis(scope);
            wrong.scope = ContextScopeId::new("foreign-basis").unwrap();
            assert!(matches!(
                ConnectorPolicyAuditContext::from_allocation(&allocation, wrong, pins(scope)),
                Err(SessionError::ScopeDenied)
            ));
            let mut incomplete = pins(scope);
            incomplete.producer.clear();
            assert!(matches!(
                ConnectorPolicyAuditContext::from_allocation(&allocation, basis(scope), incomplete),
                Err(SessionError::LimitExceeded)
            ));
            let mut duplicate = pins(scope);
            duplicate.producer.push(duplicate.producer[0].clone());
            assert!(matches!(
                ConnectorPolicyAuditContext::from_allocation(&allocation, basis(scope), duplicate),
                Err(SessionError::InvalidInput)
            ));
            let mut foreign = pins(scope);
            foreign.mapping.scope = ContextScopeId::new("foreign-pin").unwrap();
            assert!(matches!(
                ConnectorPolicyAuditContext::from_allocation(&allocation, basis(scope), foreign),
                Err(SessionError::ScopeDenied)
            ));
        },
    );
}

#[test]
fn postgres_audit_commit_uncertainty_and_checksum_corruption_deny_admission() {
    with_pg_allocation(|store, cp, allocation| {
        let context = context(allocation);
        let snapshot = Mutex::new(snapshot(allocation));
        let request = request(allocation);
        let error = Mutex::new(None);
        let admission = Cg09ConnectorAdmission::new(Source {
            change_facts_on_audit: false,
            last_error: Some(&error),
            store,
            context: &context,
            snapshot: &snapshot,
            owner: cp.owner.clone(),
        });
        let task_before = row(store.store.scope(), "task-sessions-v2").unwrap();
        let connection = std::env::var("CG_COGNITIVE_TEST_DATABASE").unwrap();
        let mut database = postgres::Client::connect(&connection, postgres::NoTls).unwrap();
        let name = format!(
            "cg_audit_failure_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let scope = store.store.scope().as_str();
        database.batch_execute(&format!(
            "CREATE FUNCTION {name}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.scope='{scope}' AND NEW.kind='{KIND}' THEN RAISE EXCEPTION 'qualification deferred audit commit failure'; END IF; RETURN NEW; END $$; CREATE CONSTRAINT TRIGGER {name} AFTER INSERT OR UPDATE ON cg_cognitive_journals DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION {name}();"
        )).unwrap();
        let result = admission.authorize(&request, None);
        database
            .batch_execute(&format!(
                "DROP TRIGGER {name} ON cg_cognitive_journals; DROP FUNCTION {name}();"
            ))
            .unwrap();
        assert!(result.is_err());
        assert_eq!(*error.lock().unwrap(), Some(SessionError::OutcomeUnknown));
        assert!(row(store.store.scope(), KIND).is_none());
        assert_eq!(
            row(store.store.scope(), "task-sessions-v2").unwrap(),
            task_before
        );
        admission.authorize(&request, None).unwrap();
        database
            .execute(
                "UPDATE cg_cognitive_journals SET digest='damaged' WHERE scope=$1 AND kind=$2",
                &[&scope, &KIND],
            )
            .unwrap();
        let corrupt = row(store.store.scope(), KIND).unwrap();
        assert!(admission.authorize(&request, None).is_err());
        assert_eq!(
            *error.lock().unwrap(),
            Some(SessionError::StorageUnavailable)
        );
        assert_eq!(row(store.store.scope(), KIND).unwrap(), corrupt);
        assert_eq!(
            row(store.store.scope(), "task-sessions-v2").unwrap(),
            task_before
        );
    });
}

#[test]
fn postgres_audit_projection_bounds_deny_without_creating_partial_journal() {
    with_pg_allocation(|store, cp, allocation| {
        let context = context(allocation);
        let snapshot = Mutex::new(snapshot(allocation));
        let request = request(allocation);
        let error = Mutex::new(None);
        snapshot.lock().unwrap().constraints = (0..=MAX_FINDINGS)
            .map(|i| format!("unsatisfied-constraint-{i}"))
            .collect();
        let admission = Cg09ConnectorAdmission::new(Source {
            change_facts_on_audit: false,
            last_error: Some(&error),
            store,
            context: &context,
            snapshot: &snapshot,
            owner: cp.owner.clone(),
        });
        assert!(admission.authorize(&request, None).is_err());
        assert_eq!(*error.lock().unwrap(), Some(SessionError::LimitExceeded));
        assert!(row(store.store.scope(), KIND).is_none());
        snapshot.lock().unwrap().constraints.clear();
        let mut large_pins = pins(&allocation.binding().scope);
        large_pins.producer = (0..MAX_PRODUCER_PINS)
            .map(|i| {
                let mut reference = pin(
                    &allocation.binding().scope,
                    &format!("p-{i}-{}", "x".repeat(110)),
                );
                reference.contract = ReferenceId::new(format!("c-{}", "y".repeat(110))).unwrap();
                reference.revision = ReferenceId::new(format!("r-{}", "z".repeat(110))).unwrap();
                reference
            })
            .collect();
        let large = ConnectorPolicyAuditContext::from_allocation(
            allocation,
            context.resolution_basis().clone(),
            large_pins,
        )
        .unwrap();
        assert!(encoded(&large.captured().unwrap()).unwrap().len() > MAX_EVENT_BYTES);
        let admission = Cg09ConnectorAdmission::new(Source {
            change_facts_on_audit: false,
            last_error: Some(&error),
            store,
            context: &large,
            snapshot: &snapshot,
            owner: cp.owner.clone(),
        });
        assert!(admission.authorize(&request, None).is_err());
        assert_eq!(*error.lock().unwrap(), Some(SessionError::LimitExceeded));
        assert!(row(store.store.scope(), KIND).is_none());
    });
}

#[test]
fn postgres_audit_retains_evaluated_snapshot_when_live_facts_change_during_audit() {
    with_pg_allocation(|store, cp, allocation| {
        let context = context(allocation);
        let evaluated = snapshot(allocation);
        let expected_digest = Snapshot::capture(&evaluated).unwrap().facts_digest;
        let snapshot = Mutex::new(evaluated);
        let request = request(allocation);
        let admission = Cg09ConnectorAdmission::new(Source {
            change_facts_on_audit: true,
            last_error: None,
            store,
            context: &context,
            snapshot: &snapshot,
            owner: cp.owner.clone(),
        });
        let dispatch = admission.authorize(&request, None).unwrap();
        let entries = journal(store).entries;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].decision.snapshot.facts_digest, expected_digest);
        assert_ne!(
            entries[0].decision.snapshot.facts_digest,
            Snapshot::capture(&snapshot.lock().unwrap())
                .unwrap()
                .facts_digest,
        );
        assert_eq!(
            admission
                .validate_current_dispatch(&dispatch, None)
                .unwrap_err()
                .code,
            ConnectorFailureCode::PermissionDenied,
        );
        assert_eq!(journal(store).entries.len(), 1, "liveness stays audit-free");
    });
}
