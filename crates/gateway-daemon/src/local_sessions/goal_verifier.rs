//! Native retained-data goal verification. No model, transport, fake prior
//! observation or ExecutionRuntimePort participates in this production path.
use super::*;
use gateway_application::{
    PlanningCapabilitySnapshot,
    closed_loop::{ClosedLoop, LoopRules, VerifiedOutcomeReceipt},
};
use gateway_domain::{CapabilityRequirementRules, PlannerRules, PlanningIrVersion, ReferenceId};

/// Read-only view of the same authenticated store. Every record lookup checks
/// the original revision/fence/state join, and never starts another lookup
/// after the native session deadline has elapsed.
struct RetainedReadGuard<'a> {
    access: &'a LocalSessionAccess,
    repository: &'a PostgresSessionStore,
    expected: &'a FencedSession,
}
impl RetainedReadGuard<'_> {
    fn deadline(&self) -> Result<(), SessionError> {
        self.access
            .check_deadline(Some(self.expected.checkpoint.budget.deadline_ms))
    }
    fn check(&self) -> Result<(), SessionError> {
        self.deadline()?;
        let current = self.repository.load(
            &self.expected.checkpoint.owner,
            &self.expected.checkpoint.snapshot.session,
        );
        self.deadline()?;
        if current? != *self.expected {
            return Err(SessionError::StaleRevision);
        }
        Ok(())
    }
    fn current_authority(&self) -> Result<LocalCodexHost, SessionError> {
        self.deadline()?;
        // The configured path can stay identical while its store locator
        // changes. Authenticate and load the claim from the currently selected
        // repository as well as checking the original read handle.
        let (host, _, current) = self.access.current_repository_claim(
            &self.expected.checkpoint.owner,
            &self.expected.checkpoint.snapshot.session,
            Some(self.expected.checkpoint.budget.deadline_ms),
        )?;
        if current != *self.expected {
            return Err(SessionError::StaleRevision);
        }
        self.deadline()?;
        Ok(host)
    }
    fn read<T>(
        &self,
        operation: impl FnOnce() -> Result<T, SessionError>,
    ) -> Result<T, SessionError> {
        self.check()?;
        let result = operation();
        self.deadline()?;
        result
    }
}
impl SessionJournalPort for RetainedReadGuard<'_> {
    fn load(
        &self,
        owner: &OwnerBinding,
        session: &SessionId,
    ) -> Result<FencedSession, SessionError> {
        self.read(|| self.repository.load(owner, session))
    }
    fn command_outcome(&self, key: &CommandKey) -> Result<CommandOutcome, SessionError> {
        self.read(|| self.repository.command_outcome(key))
    }
    fn create(&self, _: &CommandId, _: SessionCheckpoint) -> Result<FencedSession, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
    fn append(&self, _: &OwnerBinding, _: JournalAppend) -> Result<FencedSession, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
}
impl ArtifactStoragePort for RetainedReadGuard<'_> {
    fn load_artifact(
        &self,
        owner: &OwnerBinding,
        reference: &RecordRef,
    ) -> Result<Vec<u8>, SessionError> {
        self.read(|| self.repository.load_artifact(owner, reference))
    }
}
impl SessionRepositoryPort for RetainedReadGuard<'_> {
    fn record_kind(
        &self,
        owner: &OwnerBinding,
        reference: &RecordRef,
    ) -> Result<RecordKind, SessionError> {
        self.read(|| self.repository.record_kind(owner, reference))
    }
}

pub(super) fn verify(
    access: &LocalSessionAccess,
    checkpoint: &SessionCheckpoint,
    last_step: &PlanStepId,
    dispatch: &DispatchId,
    supplied: &[PersistedConnectorReadback],
) -> Result<Option<VerifiedOutcomeReceipt>, SessionError> {
    let (host, repository, current) = access.current_repository_claim(
        &checkpoint.owner,
        &checkpoint.snapshot.session,
        Some(checkpoint.budget.deadline_ms),
    )?;
    if current.checkpoint != *checkpoint {
        return Err(SessionError::StaleRevision);
    }
    let goal = checkpoint
        .goal
        .as_connector()
        .ok_or(SessionError::UnsupportedCapability)?;
    if goal.steps().last() != Some(last_step)
        || checkpoint.snapshot.dispatch.as_ref() != Some(dispatch)
        || checkpoint.snapshot.state != SessionState::Runnable
        || checkpoint.snapshot.dispatch_knowledge != DispatchKnowledge::Observed
        || checkpoint.lease_until_ms.is_some()
        || checkpoint.pending.is_some()
    {
        return Err(SessionError::InvalidState);
    }
    let config = host
        .sessions
        .as_ref()
        .ok_or(SessionError::UnsupportedCapability)?;
    let admission = config
        .connector_runtime
        .as_ref()
        .ok_or(SessionError::UnsupportedCapability)?;
    let registered = registered_connector_goal(config, &host.binding.canonical_scope)?;
    if checkpoint.execution != config.execution
        || goal != &registered
        || checkpoint.goal.intent() != &config.intent
        || checkpoint.budget.max_actions != config.max_actions
        || checkpoint.budget.max_retries != config.max_retries
        || checkpoint
            .resource_ledger
            .as_ref()
            .map(|ledger| ledger.limits())
            != config.resource_limits.as_ref()
    {
        return Err(SessionError::StaleRevision);
    }
    let limits = admission.verifier(config)?;
    let configuration = admission.reference()?;
    let guarded = RetainedReadGuard {
        access,
        repository: &repository,
        expected: &current,
    };
    let mut actual = Vec::new();
    for reference in checkpoint.connector_observations.iter().filter(|r| {
        r.role == ConnectorObservationRole::GoalVerificationReadback && r.binding.step == *last_step
    }) {
        actual.push(decode_goal_verification_readback(
            &guarded,
            &checkpoint.owner,
            reference,
            goal,
        )?);
    }
    if actual.len() != supplied.len() || actual.iter().any(|r| !supplied.contains(r)) {
        return Err(SessionError::InvalidInteraction);
    }
    let readset = build_goal_verification_readset(checkpoint, last_step, &actual)?;
    let canonical = host
        .canonical
        .as_ref()
        .ok_or(SessionError::UnsupportedCapability)?;
    let documents = canonical.documents(&host.resources).map_err(error)?;
    let (snapshot, rules) = host_mapping::capture(&canonical.catalog, &documents).map_err(error)?;
    if snapshot.scope != *goal.scope()
        || snapshot.operating_mode != checkpoint.execution.mode
        || snapshot.execution_profile != checkpoint.execution.profile
    {
        return Err(SessionError::ScopeDenied);
    }
    let resolved = DeclarativeResolutionApplication
        .resolve_plan(&snapshot, &rules)
        .map_err(|_| SessionError::InvalidInput)?;
    if canonical.pinned_basis.as_ref() != Some(&resolved.report.basis) {
        return Err(SessionError::StaleRevision);
    }
    let capabilities = PlanningCapabilitySnapshot::new(
        resolved.snapshot.input().index.clone(),
        resolved.report.basis.registry_fingerprint.as_str(),
        PlanningIrVersion::V1,
    )
    .map_err(|_| SessionError::InvalidInput)?;
    let execution = ReferenceId::new(format!("{}-execution-1", dispatch.as_str()))
        .map_err(|_| SessionError::InvalidInteraction)?;
    guarded.check()?;
    let outcome = ClosedLoop::verify_retained(
        execution,
        goal.scope().clone(),
        goal.intent().clone(),
        &readset,
        LoopRules {
            capabilities,
            requirements: CapabilityRequirementRules::default(),
            planner: PlannerRules::default(),
            max_iterations: limits.max_iterations,
            max_retries: limits.max_retries,
        },
    )
    .map_err(|_| SessionError::InvalidInteraction);
    guarded.check()?;
    let latest = guarded.current_authority()?;
    let latest_config = latest
        .sessions
        .as_ref()
        .ok_or(SessionError::UnsupportedCapability)?;
    let latest_admission = latest_config
        .connector_runtime
        .as_ref()
        .ok_or(SessionError::UnsupportedCapability)?;
    let latest_canonical = latest
        .canonical
        .as_ref()
        .ok_or(SessionError::UnsupportedCapability)?;
    if latest_admission.reference()? != configuration
        || latest_canonical.catalog != canonical.catalog
        || latest_canonical.plan != canonical.plan
        || latest_canonical.rules != canonical.rules
        || latest_canonical.process != canonical.process
        || latest_canonical.policy != canonical.policy
        || latest_canonical.pinned_basis != canonical.pinned_basis
        || latest_config.store_file != config.store_file
        || latest_config.max_actions != config.max_actions
        || latest_config.max_retries != config.max_retries
        || latest_config.resource_limits != config.resource_limits
        || latest_config.execution != config.execution
        || latest_config.ttl_ms != config.ttl_ms
        || latest_config.consent_required != config.consent_required
        || latest_config.issuers != config.issuers
        || registered_connector_goal(latest_config, &latest.binding.canonical_scope)? != *goal
    {
        return Err(SessionError::StaleRevision);
    }
    guarded.check()?;
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_sessions::{
        connector_admission_tests::prepared_connector_turn, runtime_admission::tests::Fixture,
    };
    use crate::mcp_client::{ProtocolResult, mapping::MappingDocument, normalization::*};
    use gateway_application::{
        connector_proposals::{DispatchedProposal, ProposalBatch},
        connectors::*,
    };
    use gateway_domain::*;
    use std::{
        cell::{Cell, RefCell},
        collections::BTreeMap,
    };

    struct Collector(Acquisition);
    impl AcquisitionPort for Collector {
        fn collect(&self, _: &ConnectorDispatch) -> Result<Acquisition, crate::mcp_client::Error> {
            Ok(self.0.clone())
        }
    }
    fn source_read(
        checkpoint: &SessionCheckpoint,
        step: &PlanStepId,
        label: &str,
        value: &str,
    ) -> (
        ConnectorRequest,
        ConnectorObservation,
        gateway_application::ScopedObservationBatch,
    ) {
        let text = |s: &str| NonEmptyText::new(s).unwrap();
        let request = ConnectorRequest {
            target: ConnectorTarget {
                connector: text("example.records"),
                scope: checkpoint.goal.scope().clone(),
            },
            capability: CapabilityId::new("records.resource.read").unwrap(),
            class: ConnectorOperationClass::Read,
            inventory_revision: ContentDigest::new("a".repeat(64)).unwrap(),
            arguments: BTreeMap::new(),
            task: TaskId::new("task-inspect").unwrap(),
            step: step.clone(),
            execution: text(label),
            correlation: text(&format!(
                "{}-{label}",
                checkpoint.snapshot.dispatch.as_ref().unwrap().as_str()
            )),
        };
        let dispatch = ConnectorDispatch {
            request: request.clone(),
            policy: PolicyId::new("fixture-read-policy").unwrap(),
            policy_revision: ContentDigest::new("b".repeat(64)).unwrap(),
            consent_receipt: None,
            budget_reservation: text("fixture-read-reservation"),
            deadline: UnixTimestamp::new(4_000_000_000),
        };
        let now = i64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        )
        .unwrap();
        let normalizer = CanonicalResultNormalizer::new(
            NormalizationProfile {
                source: SourceId::new("records.one").unwrap(),
                source_kind: SourceKind::Repository,
                object_field: "object".into(),
                revision_field: None,
                digest_field: None,
                fields: BTreeMap::from([(
                    "text".into(),
                    SubjectPath::new(["records", "content"]).unwrap(),
                )]),
            },
            Box::new(Collector(Acquisition {
                acquired_at: UnixTimestamp::new(now),
                observed_at: Some(UnixTimestamp::new(now)),
                evaluated_at: UnixTimestamp::new(now),
                max_age: FreshnessPolicy::new(60),
                authenticated_source: text("scoped-goal-verifier-fixture"),
                sensitivity: SensitivityClass::Normal,
                // Qualified fixture collector: an authenticated full text
                // export is observed directly, not probabilistically inferred.
                confidence: Confidence::NotApplicable,
                uncertainty: Uncertainty::None,
                conflict: ConflictStatus::None,
                partial: false,
                basis: ConnectorObservationBasis::SourceRead,
                effect: ConnectorEffect::NoMutation,
                verifies: None,
                non_authoritative_raw: None,
            })),
        )
        .unwrap();
        let mappings = MappingDocument::from_json(include_bytes!(
            "../../../../tests/fixtures/connector-mapping-v1/mapping.json"
        ))
        .unwrap();
        let mapping = mappings
            .records()
            .iter()
            .find(|m| m.capability == request.capability)
            .unwrap();
        let result = normalizer
            .canonicalize(
                &dispatch,
                Some(&SourceId::new("records.one").unwrap()),
                mapping,
                ProtocolResult::Resource {
                    contents: vec![
                        json!({"uri":"fixture://registered-complete-state","text":value}),
                    ],
                },
            )
            .unwrap();
        let batch = result.batch().unwrap();
        (request, result.into_observation(), batch)
    }
    fn write_record(kind: RecordKind, name: &str, bytes: Vec<u8>) -> RecordWrite {
        let identity = gateway_domain::context_content_digest(&bytes);
        RecordWrite {
            reference: content_reference(&format!("{name}-{}", &identity.as_str()[..24]), &bytes)
                .unwrap(),
            kind,
            bytes,
        }
    }
    // Stage typed retained records after a real coordinator preparation claim.
    // This is fixture ingestion only; it does not pretend to call an effect port.
    fn stage(
        repository: &PostgresSessionStore,
        checkpoint: &SessionCheckpoint,
        step: &PlanStepId,
        value: &str,
    ) -> (FencedSession, Vec<PersistedConnectorReadback>) {
        let current = repository
            .load(&checkpoint.owner, &checkpoint.snapshot.session)
            .unwrap();
        let dispatch = checkpoint.snapshot.dispatch.clone().unwrap();
        let invocation = prepared_connector_turn().invocation().identity();
        let mut next = checkpoint.clone();
        next.snapshot.revision = next.snapshot.revision.next().unwrap();
        next.snapshot.state = SessionState::Dispatching;
        next.snapshot.dispatch_knowledge = DispatchKnowledge::Reserved;
        let mut binding = ConnectorStepBinding {
            owner: checkpoint.owner.clone(),
            scope: checkpoint.goal.scope().clone(),
            session: checkpoint.snapshot.session.clone(),
            run: checkpoint.snapshot.run.clone(),
            revision: next.snapshot.revision,
            task: TaskId::new("task-inspect").unwrap(),
            step: step.clone(),
            dispatch: dispatch.clone(),
            invocation,
        };
        let payload = ScopedConnectorSnapshotPayload::new(
            checkpoint.goal.scope().clone(),
            br#"{"records.content":"not-ready"}"#.to_vec(),
        )
        .unwrap();
        let before = write_record(
            RecordKind::ConnectorSnapshot,
            "goal-before",
            before_snapshot_record(&binding, &payload).unwrap(),
        );
        next.connector_step_snapshots
            .push(ConnectorStepSnapshotRef {
                binding: binding.clone(),
                phase: ConnectorSnapshotPhase::Before,
                record: before.reference.clone(),
            });
        let reserved = repository
            .append(
                &checkpoint.owner,
                JournalAppend {
                    session: checkpoint.snapshot.session.clone(),
                    expected_revision: checkpoint.snapshot.revision,
                    fence: current.fence,
                    command: None,
                    next,
                    records: vec![before],
                },
            )
            .unwrap();
        let mut next = reserved.checkpoint.clone();
        next.snapshot.revision = next.snapshot.revision.next().unwrap();
        next.snapshot.state = SessionState::Runnable;
        next.snapshot.dispatch_knowledge = DispatchKnowledge::Observed;
        next.lease_until_ms = None;
        binding.revision = next.snapshot.revision;
        let (request, observation, _) = source_read(checkpoint, step, "phase-source-read", value);
        let batch = ProposalBatch {
            invocation: binding.invocation.invocation_digest.clone(),
            all_calls_returned: true,
            results: vec![DispatchedProposal {
                ordinal: 0,
                request,
                observation,
            }],
            failure: None,
        };
        let result = write_record(
            RecordKind::ConnectorResult,
            "goal-result",
            proposal_batch_record(&binding, &batch).unwrap(),
        );
        next.connector_results.push(ConnectorResultRef {
            binding: binding.clone(),
            record: result.reference.clone(),
            after_snapshot_state: ConnectorAfterSnapshotState::Captured,
        });
        let after = write_record(
            RecordKind::ConnectorSnapshot,
            "goal-after",
            after_snapshot_record(
                &binding,
                &ScopedConnectorSnapshotPayload::new(
                    checkpoint.goal.scope().clone(),
                    serde_json::to_vec(&json!({"records.content":value})).unwrap(),
                )
                .unwrap(),
            )
            .unwrap(),
        );
        next.connector_step_snapshots
            .push(ConnectorStepSnapshotRef {
                binding: binding.clone(),
                phase: ConnectorSnapshotPhase::After,
                record: after.reference.clone(),
            });
        let (request, observation, batch) =
            source_read(checkpoint, step, "independent-goal-source-read", value);
        let goal = checkpoint.goal.as_connector().unwrap();
        let condition = goal.intent().desired_state().conditions()[0].id().clone();
        let readback = write_record(
            RecordKind::ConnectorObservation,
            "goal-read",
            encode_goal_verification_readback(
                &binding,
                &condition,
                &request,
                &observation,
                &batch,
                goal,
            )
            .unwrap(),
        );
        next.connector_observations.push(ConnectorObservationRef {
            binding,
            proposal_ordinal: None,
            goal_condition: Some(condition),
            role: ConnectorObservationRole::GoalVerificationReadback,
            record: readback.reference.clone(),
        });
        let current = repository
            .append(
                &checkpoint.owner,
                JournalAppend {
                    session: checkpoint.snapshot.session.clone(),
                    expected_revision: reserved.checkpoint.snapshot.revision,
                    fence: reserved.fence,
                    command: None,
                    next,
                    records: vec![result, after, readback],
                },
            )
            .unwrap();
        let readbacks = current
            .checkpoint
            .connector_observations
            .iter()
            .map(|r| {
                decode_goal_verification_readback(repository, &checkpoint.owner, r, goal).unwrap()
            })
            .collect();
        (current, readbacks)
    }
    struct Probe<'a> {
        local: &'a LocalSessionHost,
        plan: SessionResourcePlan,
        value: &'a str,
        captured: &'a RefCell<Option<(FencedSession, Vec<PersistedConnectorReadback>)>>,
        called: &'a Cell<bool>,
    }
    impl InteractionAuthorityPort for Probe<'_> {
        fn load_consent(
            &self,
            owner: &OwnerBinding,
            reference: &ConsentRecordRef,
        ) -> Result<ConsentStatus, SessionError> {
            self.local.load_consent(owner, reference)
        }
        fn live_status(&self, grant: &VerifiedConsent) -> Result<ConsentStatus, SessionError> {
            self.local.live_status(grant)
        }
    }
    impl StructuredSessionHost for Probe<'_> {
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
        fn disclose(&self, owner: &OwnerBinding, session: &SessionId) -> Result<(), SessionError> {
            self.local.disclose(owner, session)
        }
        fn issuer(&self, owner: &OwnerBinding, issuer: &PrincipalId) -> Result<(), SessionError> {
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
    impl ConnectorSessionRuntimePort for Probe<'_> {
        fn resource_plan(
            &self,
            _: &SessionCheckpoint,
            _: &PlanStepId,
        ) -> Result<SessionResourcePlan, SessionError> {
            Ok(self.plan.clone())
        }
        fn initial_assessment(
            &self,
            c: &SessionCheckpoint,
        ) -> Result<InitialAssessment, SessionError> {
            self.local.connector_runtime.initial_assessment(c)
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
            c: &SessionCheckpoint,
            step: &PlanStepId,
            _: &mut SessionPhaseResources,
            _: ConnectorPreparationContext<'_>,
        ) -> Result<ConnectorPreparation, SessionError> {
            self.captured.replace(Some(stage(
                &store(&self.local.current()?)?,
                c,
                step,
                self.value,
            )));
            self.called.set(true);
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
        ) -> Result<Option<VerifiedOutcomeReceipt>, SessionError> {
            Err(SessionError::UnsupportedCapability)
        }
    }
    fn copy_tree(source: &std::path::Path, target: &std::path::Path) {
        std::fs::create_dir_all(target).unwrap();
        for entry in std::fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let path = target.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_tree(&entry.path(), &path)
            } else {
                std::fs::copy(entry.path(), path).unwrap();
            }
        }
    }
    fn install(f: &mut Fixture, connection: &str) -> LocalSessionHost {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
        copy_tree(
            &fixture.join("declarative-cli/catalog"),
            &f.root.join("catalog"),
        );
        let scope = f.wire["mappings"][0]["canonical_scope"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut plan: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/declarative-v0.1/plan.json"
        ))
        .unwrap();
        plan["assessment"]["scope"] = json!(scope);
        let rules: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/declarative-cli/rules.json"
        ))
        .unwrap();
        let process: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/declarative-cli/process.json"
        ))
        .unwrap();
        let wire_scope = f.wire["mappings"][0]["scope"].clone();
        let mut canonical = json!({"catalog":f.root.join("catalog"),"policy":{}});
        for (id, contract, document) in [
            ("connector-plan", "cg.plan", plan),
            ("rules", "cg.composition-rules", rules),
            ("process", "cg.process-snapshot", process),
        ] {
            let reference = json!({"id":id,"contract":contract,"contract_version":"1.0","revision":"1","digest":format!("sha256:{}",context_content_digest(&serde_json::to_vec(&document).unwrap()).as_str())});
            f.wire["mappings"][0]["resources"].as_array_mut().unwrap().push(json!({"schema_version":"1.0","scope":wire_scope,"reference":reference,"document":document,"provenance":[{"reference":reference,"source_id":id,"source_revision":"1","freshness":"current","sensitivity":"NORMAL","lineage":[]}]}));
            canonical[if id == "connector-plan" { "plan" } else { id }] = reference.clone();
            if id == "connector-plan" {
                let pin = json!({"id":id,"revision":"1","digest":reference["digest"]});
                f.wire["mappings"][0]["sessions"]["connector_goal"]["plan"] = pin.clone();
                f.runtime()["plan"] = pin;
            }
        }
        f.wire["mappings"][0]["canonical"] = canonical;
        f.wire["mappings"][0]["sessions"]["connector_goal"]["steps"] =
            json!(["step-condition.0.1"]);
        f.runtime()["steps"][0]["step"] = json!("step-condition.0.1");
        f.rewrite("action-projection",json!({"schema_version":1,"id":"action-execution-context","step":"step-condition.0.1","task":{"id":"task-inspect"},"basis":{"scope":scope}}));
        let database = f.root.join("database.txt");
        std::fs::write(&database, connection).unwrap();
        f.wire["mappings"][0]["sessions"]["store_file"] = json!(database);
        let admission = f.root.join("admission.json");
        std::fs::write(&admission, f.wire.to_string()).unwrap();
        let resolver = LocalWorkspaceResolver::from_json(&f.wire.to_string()).unwrap();
        let path = f.root.to_string_lossy().into_owned();
        let binding = resolver
            .resolve(&WorkspaceReference {
                working_directory: path.clone(),
                repository: path.clone(),
            })
            .unwrap();
        LocalSessionHost::new(Arc::new(LocalSessionAccess {
            binding,
            admission: admission.to_string_lossy().into_owned(),
            cwd: path.clone(),
            repository: path,
            store_connection: Default::default(),
        }))
    }
    #[test]
    fn postgres_goal_verifier_uses_original_goal_and_actual_retained_evidence_without_effect_calls()
    {
        let Ok(connection) = std::env::var("CG_COGNITIVE_TEST_DATABASE") else {
            return;
        };
        for (value, expire) in [("ready", false), ("not-ready", false), ("ready", true)] {
            let scope = prepared_connector_turn()
                .invocation()
                .prepared()
                .invocation()
                .data()
                .task
                .scope
                .clone();
            let mut f = Fixture::with_scope(scope.as_str());
            if expire {
                f.wire["mappings"][0]["sessions"]["ttl_ms"] = json!(5000);
            }
            let host = install(&mut f, &connection);
            let owner = host.owner().unwrap();
            assert!(Arc::ptr_eq(&host.access, &host.connector_runtime.access));
            let config = host.current().unwrap().sessions.unwrap();
            let plan: SessionResourcePlan =
                serde_json::from_value(f.runtime()["steps"][0]["resources"].clone()).unwrap();
            let captured = RefCell::new(None);
            let called = Cell::new(false);
            let probe = Probe {
                local: &host,
                plan,
                value,
                captured: &captured,
                called: &called,
            };
            let coordinator =
                SessionCoordinator::new(store(&host.current().unwrap()).unwrap(), probe);
            let suffix = f.root.file_name().unwrap().to_string_lossy();
            let started = coordinator
                .execute(
                    &owner,
                    SessionCommand::Start {
                        command: CommandId::new(format!("start-{suffix}")).unwrap(),
                        intent: config.intent,
                        execution: config.execution,
                    },
                )
                .unwrap();
            assert!(
                coordinator
                    .execute(
                        &owner,
                        SessionCommand::Continue {
                            at: Mutation {
                                session: started.session,
                                expected_revision: started.revision,
                                command: CommandId::new(format!("continue-{suffix}")).unwrap()
                            }
                        }
                    )
                    .is_err()
            );
            assert!(called.get());
            let (current, readbacks) = captured.take().unwrap();
            let checkpoint = &current.checkpoint;
            let step = checkpoint
                .goal
                .as_connector()
                .unwrap()
                .steps()
                .last()
                .unwrap();
            let dispatch = checkpoint.snapshot.dispatch.as_ref().unwrap();
            if expire {
                let remaining = checkpoint
                    .budget
                    .deadline_ms
                    .saturating_sub(host.access.now_ms().unwrap());
                std::thread::sleep(std::time::Duration::from_millis(remaining + 1));
                assert_eq!(
                    host.connector_runtime
                        .verify_goal(checkpoint, step, dispatch, &readbacks),
                    Err(SessionError::LimitExceeded)
                );
                continue;
            }
            let receipt = host
                .connector_runtime
                .verify_goal(checkpoint, step, dispatch, &readbacks)
                .unwrap();
            if value == "not-ready" {
                assert!(receipt.is_none());
                continue;
            }
            let receipt = receipt.unwrap();
            assert_eq!(receipt.scope(), checkpoint.goal.scope());
            assert_eq!(
                receipt.goal_digest(),
                &ClosedLoop::goal_digest(checkpoint.goal.intent())
            );
            assert_eq!(
                receipt.readset_digest(),
                Some(
                    build_goal_verification_readset(checkpoint, step, &readbacks)
                        .unwrap()
                        .digest()
                )
            );
            assert!(!receipt.evidence().is_empty());
            assert!(!receipt.facts().is_empty());
            assert!(
                host.connector_runtime
                    .verify_goal(checkpoint, step, dispatch, &[])
                    .is_err()
            );
            let mut altered = readbacks.clone();
            altered[0].covered_conditions.clear();
            assert!(
                host.connector_runtime
                    .verify_goal(checkpoint, step, dispatch, &altered)
                    .is_err()
            );
            let mut smaller = checkpoint.clone();
            let mut intent = serde_json::to_value(checkpoint.goal.intent()).unwrap();
            intent["desired_state"]["conditions"][0]["operator"] = json!("NOT_EQUALS");
            intent["desired_state"]["conditions"][0]["expected"] =
                json!({"kind":"STRING","value":"impossible-value"});
            let g = checkpoint.goal.as_connector().unwrap();
            smaller.goal = SupportedSessionGoal::connector(
                SupportedConnectorGoal::from_host_registration(
                    serde_json::from_value(intent).unwrap(),
                    g.scope().clone(),
                    g.plan().clone(),
                    g.steps().to_vec(),
                    g.observations().to_vec(),
                )
                .unwrap(),
            );
            assert_eq!(
                host.connector_runtime
                    .verify_goal(&smaller, step, dispatch, &readbacks),
                Err(SessionError::StaleRevision)
            );
            let mut changed = checkpoint.clone();
            changed.connector_observations[0].record =
                content_reference("altered", b"different").unwrap();
            assert_eq!(
                host.connector_runtime
                    .verify_goal(&changed, step, dispatch, &readbacks),
                Err(SessionError::StaleRevision)
            );
            let mut changed = checkpoint.clone();
            changed.owner.project = ProjectId::new("foreign-project").unwrap();
            assert_eq!(
                host.connector_runtime
                    .verify_goal(&changed, step, dispatch, &readbacks),
                Err(SessionError::ScopeDenied)
            );
            assert!(
                host.connector_runtime
                    .verify_goal(
                        checkpoint,
                        step,
                        &DispatchId::new("foreign-dispatch").unwrap(),
                        &readbacks
                    )
                    .is_err()
            );
            // The same runtime re-authenticates current operator configuration.
            let original = f.wire.clone();
            f.wire["mappings"][0]["sessions"]["enabled"] = json!(false);
            std::fs::write(&host.access.admission, f.wire.to_string()).unwrap();
            assert_eq!(
                host.connector_runtime
                    .verify_goal(checkpoint, step, dispatch, &readbacks),
                Err(SessionError::AuthorityDenied)
            );
            std::fs::write(&host.access.admission, original.to_string()).unwrap();
            let mut expanded = original.clone();
            expanded["mappings"][0]["sessions"]["resource_limits"]["amounts"]["connector_calls"] =
                json!(11);
            std::fs::write(&host.access.admission, expanded.to_string()).unwrap();
            assert_eq!(
                host.connector_runtime
                    .verify_goal(checkpoint, step, dispatch, &readbacks),
                Err(SessionError::StaleRevision)
            );
            let mut foreign = original.clone();
            foreign["mappings"][0]["canonical_scope"] = json!("foreign-scope");
            std::fs::write(&host.access.admission, foreign.to_string()).unwrap();
            assert_eq!(
                host.connector_runtime
                    .verify_goal(checkpoint, step, dispatch, &readbacks),
                Err(SessionError::ScopeDenied)
            );
            std::fs::write(&host.access.admission, original.to_string()).unwrap();
            // A real append between kind lookup and content lookup invalidates
            // the shared read guard before it performs the following read.
            let repository = store(&host.current().unwrap()).unwrap();
            let guarded = RetainedReadGuard {
                access: &host.access,
                repository: &repository,
                expected: &current,
            };
            let reference = &checkpoint.connector_observations[0].record;
            assert_eq!(
                guarded.record_kind(&owner, reference),
                Ok(RecordKind::ConnectorObservation)
            );
            assert!(guarded.current_authority().is_ok());
            // The authenticated admission and locator path are unchanged, but
            // that path no longer selects the initial repository. The old
            // handle remains readable and must not qualify current authority.
            let locator = config.store_file.clone();
            let original_locator = std::fs::read(&locator).unwrap();
            std::fs::write(&locator, b"invalid-test-store-configuration").unwrap();
            assert!(host.access.current().is_ok());
            assert_eq!(
                guarded.record_kind(&owner, reference),
                Ok(RecordKind::ConnectorObservation)
            );
            assert!(guarded.current_authority().is_err());
            std::fs::write(&locator, original_locator).unwrap();
            assert!(guarded.current_authority().is_ok());
            let mut cancelled = checkpoint.clone();
            cancelled.snapshot.revision = cancelled.snapshot.revision.next().unwrap();
            cancelled.snapshot.state = SessionState::Cancelled;
            cancelled.snapshot.dispatch_knowledge = DispatchKnowledge::Stopped;
            repository
                .append(
                    &owner,
                    JournalAppend {
                        session: checkpoint.snapshot.session.clone(),
                        expected_revision: checkpoint.snapshot.revision,
                        fence: current.fence,
                        command: None,
                        next: cancelled,
                        records: vec![],
                    },
                )
                .unwrap();
            assert_eq!(
                guarded.load_artifact(&owner, reference),
                Err(SessionError::StaleRevision)
            );
            assert!(matches!(
                guarded.current_authority(),
                Err(SessionError::StaleRevision)
            ));
            assert_eq!(
                host.connector_runtime
                    .verify_goal(checkpoint, step, dispatch, &readbacks),
                Err(SessionError::StaleRevision)
            );
        }
    }
}
