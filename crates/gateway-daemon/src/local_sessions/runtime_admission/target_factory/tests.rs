use super::*;
use gateway_application::{
    PlanningCapabilitySnapshot, ScopedObservationBatch, SourceSnapshot,
    closed_loop::{ClosedLoop, LoopRules},
    retained_observations::{RetainedObservationMember, RetainedObservationReadset},
};
use gateway_domain::{
    CapabilityRequirementRules, ContentDigest, ObservationEvidenceSet, PlannerRules,
    PlanningIrVersion, QualityMetadata, ReferenceId, SourceKind, SourceTimestamp,
};
use serde::{Deserialize, Serialize};
use std::cell::Cell;

struct Timings {
    started: std::time::Instant,
    events: Mutex<Vec<(u128, mcp_client::EventKind)>>,
}
impl Observer for Timings {
    fn record(&self, event: mcp_client::Event) {
        if let Ok(mut events) = self.events.try_lock()
            && events.len() < 256
        {
            events.push((self.started.elapsed().as_millis(), event.kind));
        }
    }
}
const PEER: &str = r#"import json,sys
log=sys.argv[1]
for line in sys.stdin:
 q=json.loads(line);m=q.get('method')
 with open(log,'a') as out:out.write(m+'\n')
 if 'id' not in q:continue
 if m=='initialize':r={'protocolVersion':'2025-11-25','capabilities':{'tools':{}},'serverInfo':{'name':'example.records.local','version':'1.0.0'}}
 elif m=='tools/list':r={'tools':[{'name':'records.read','inputSchema':{'type':'object','properties':{'query':{'type':'string'}},'required':['query'],'additionalProperties':False}}]}
 elif m=='resources/list':r={'resources':[]}
 elif m=='resources/templates/list':r={'resourceTemplates':[]}
 elif m=='prompts/list':r={'prompts':[]}
 elif m=='tools/call':
  assert q['params']['arguments']=={'query':'native-fixture'}
  r={'content':[],'structuredContent':{'object':'native-fixture','id':'native-fixture','first':True,'second':True},'isError':False}
 elif m=='ping':r={}
 else:raise ValueError('unexpected method')
 print(json.dumps({'jsonrpc':'2.0','id':q['id'],'result':r}),flush=True)
"#;
struct Probe<'a> {
    local: LocalSessionHost,
    plan: SessionResourcePlan,
    called: &'a Cell<bool>,
    callback: &'a dyn Fn(
        &LocalSessionHost,
        &SessionCheckpoint,
        SessionResourceAllocation,
    ) -> Result<(), SessionError>,
}
impl InteractionAuthorityPort for Probe<'_> {
    fn load_consent(
        &self,
        o: &OwnerBinding,
        r: &ConsentRecordRef,
    ) -> Result<ConsentStatus, SessionError> {
        self.local.load_consent(o, r)
    }
    fn live_status(&self, g: &VerifiedConsent) -> Result<ConsentStatus, SessionError> {
        self.local.live_status(g)
    }
}
impl StructuredSessionHost for Probe<'_> {
    fn now_ms(&self) -> Result<u64, SessionError> {
        self.local.now_ms()
    }
    fn register(
        &self,
        o: &OwnerBinding,
        i: Intent,
        e: RequestedExecution,
    ) -> Result<(SupportedSessionGoal, SessionBudget), SessionError> {
        self.local.register(o, i, e)
    }
    fn prepare(&self, _: &SessionCheckpoint) -> Result<PreparedTask, SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
    fn disclose(&self, o: &OwnerBinding, s: &SessionId) -> Result<(), SessionError> {
        self.local.disclose(o, s)
    }
    fn issuer(&self, o: &OwnerBinding, i: &PrincipalId) -> Result<(), SessionError> {
        self.local.issuer(o, i)
    }
    fn resource_limits(
        &self,
        o: &OwnerBinding,
        g: &SupportedSessionGoal,
    ) -> Result<Option<SessionResourceLimits>, SessionError> {
        self.local.resource_limits(o, g)
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
    fn initial_assessment(&self, c: &SessionCheckpoint) -> Result<InitialAssessment, SessionError> {
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
        _: &PlanStepId,
        r: &mut SessionPhaseResources,
        _: ConnectorPreparationContext<'_>,
    ) -> Result<ConnectorPreparation, SessionError> {
        self.called.set(true);
        let allocation = r.allocate(SessionResourceAmounts {
            connector_calls: 3,
            retrieval_rounds: 3,
            retrieval_results: 12,
            retrieval_tokens: 900_000,
            ..Default::default()
        })?;
        (self.callback)(&self.local, c, allocation)
            .expect("the full source/normalizer/PG/original-goal callback must succeed before the deliberate unsupported-turn boundary");
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
        unreachable!("readonly target qualification has no model or effect")
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
// This fixture persists only the actual native CG06 document and measured
// metadata under the real scoped store. It is not the model-bound Session
// readback codec and fabricates no InvocationIR or session-completion claim.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectReadDocument {
    scope: gateway_domain::ContextScopeId,
    source: SourceId,
    source_kind: SourceKind,
    source_revision: Option<SourceTimestamp>,
    content_digest: ContentDigest,
    quality: QualityMetadata,
    records: String,
    request: ConnectorRequestIdentity,
}
// This envelope belongs to the scoped component journal below, not to the
// model-bound Session ConnectorObservation/Before/After record contracts.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectReadEnvelope {
    owner: OwnerBinding,
    scope: ContextScopeId,
    session: SessionId,
    run: RunId,
    claim_revision: Revision,
    step: NonEmptyText,
    dispatch: DispatchId,
    request: ConnectorRequestIdentity,
    document: RecordRef,
    bytes: Vec<u8>,
}
#[test]
fn real_stdio_target_source_normalizer_postgres_native_goal_without_fake_invocation() {
    let Ok(connection) = std::env::var("CG_COGNITIVE_TEST_DATABASE") else {
        return;
    };
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let scope = format!("target-peer-{nonce}");
    let mut f = crate::local_sessions::runtime_admission::tests::Fixture::with_scope(&scope);
    let native = f.install_actual_native_model();
    let peer = f.root.join("peer.py");
    let log = f.root.join("peer-methods.txt");
    std::fs::write(&peer, PEER).unwrap();
    f.runtime()["stdio"]["executable"] = json!("/usr/bin/python3");
    f.runtime()["stdio"]["arguments"] = json!([peer, log]);
    let profile = f.root.join("profiles.json");
    std::fs::write(&profile,json!({"schema_version":1,"profiles":{"records.local":{"connector":"example.records","server":"example.records.local","tools":["records.read"],"generation":1,"environment":{}}}}).to_string()).unwrap();
    f.runtime()["credential_store"] = json!(profile);
    let mut plan: SessionResourcePlan =
        serde_json::from_value(f.runtime()["steps"][0]["resources"].clone()).unwrap();
    plan.preparation.connector_calls = 3;
    plan.preparation.retrieval_rounds = 3;
    plan.preparation.retrieval_results = 12;
    plan.preparation.retrieval_tokens = 900_000;
    f.runtime()["steps"][0]["resources"] = json!(plan);
    f.wire["mappings"][0]["sessions"]["resource_limits"] = json!(SessionResourceLimits {
        cost_unit: ReferenceId::new("credits").unwrap(),
        amounts: plan.total().unwrap()
    });
    let database = f.root.join("database.txt");
    std::fs::write(&database, &connection).unwrap();
    f.wire["mappings"][0]["sessions"]["store_file"] = json!(database);
    let admission = f.root.join("admission.json");
    std::fs::write(&admission, f.wire.to_string()).unwrap();
    let host = f.host().unwrap();
    let config = host.sessions.as_ref().unwrap();
    let cwd = f.root.to_string_lossy().into_owned();
    let local = LocalSessionHost::new(Arc::new(LocalSessionAccess {
        binding: host.binding.clone(),
        admission: admission.to_string_lossy().into_owned(),
        cwd: cwd.clone(),
        repository: cwd,
        store_connection: Default::default(),
    }));
    let owner = local.owner().unwrap();
    let pipeline_completed = Cell::new(false);
    let callback = |local: &LocalSessionHost,
                    c: &SessionCheckpoint,
                    allocation: SessionResourceAllocation|
     -> Result<(), SessionError> {
        let setup_started = std::time::Instant::now();
        let until = allocation.deadline();
        let capture = PreparedTarget::capture(Arc::clone(&local.access), &allocation, None)?;
        let capture_ms = setup_started.elapsed().as_millis();
        let context = CallContext::new(NonEmptyText::new("target-peer-run").unwrap(), until);
        let timings = Arc::new(Timings {
            started: std::time::Instant::now(),
            events: Mutex::new(vec![]),
        });
        let mut target = capture.build(allocation, context, timings.clone())?;
        let build_ms = setup_started.elapsed().as_millis() - capture_ms;
        let before_discovery = target.source.diagnostic_timings();
        let started = std::time::Instant::now();
        let result = target.discover().map(|_| ());
        if result.is_err() {
            let methods = std::fs::read_to_string(&log).unwrap_or_default();
            let events = timings.events.lock().unwrap().clone();
            let source_timings = target.source.diagnostic_timings();
            eprintln!(
                "content-free target timings: capture_ms={capture_ms}; build_ms={build_ms}; source_before={before_discovery:?}; source_after={source_timings:?} (calls,total_ms,fresh_ms,native_ms,claims_ms); events={events:?}; peer methods={methods}; discover_ms={}",
                started.elapsed().as_millis()
            );
        }
        assert!(
            result.is_ok(),
            "real discovery failed after {:?}: {:?}; original lease unchanged",
            started.elapsed(),
            result.err()
        );
        assert!(
            target.discover().is_err(),
            "driver cannot perform a second unmetered discovery"
        );
        let read = target.source_read("mapping.records.read.v1").inspect_err(|error| {
            eprintln!("content-free source read failure: error={error:?}; elapsed_ms={}; source_timings={:?}; peer_methods={}", started.elapsed().as_millis(), target.source.diagnostic_timings(), std::fs::read_to_string(&log).unwrap_or_default());
        })?;
        assert_eq!(read.request.execution.as_str(), "native-producer-context");
        assert_eq!(
            read.observation.basis,
            ConnectorObservationBasis::SourceRead
        );
        assert_eq!(
            read.observation.metadata.quality.uncertainty(),
            Uncertainty::None
        );
        let batch = read.batch()?;
        let observation = &read.observation;
        let document = DirectReadDocument {
            scope: observation.target.scope.clone(),
            source: observation.source.clone(),
            source_kind: observation.metadata.source_kind,
            source_revision: observation.source_revision.clone(),
            content_digest: observation.content_digest.clone(),
            quality: observation.metadata.quality,
            records: observation.records.to_json().unwrap(),
            request: ConnectorRequestIdentity::from_request(&read.request)?,
        };
        let bytes = serde_json::to_vec(&document).unwrap();
        let reference = content_reference("target-direct-read", &bytes)?;
        target.context_source().validate_context_checkpoint(c)?;
        let binding = target.context_source().context_resource_binding();
        let envelope = DirectReadEnvelope {
            owner: binding.owner.clone(),
            scope: binding.scope.clone(),
            session: binding.session.clone(),
            run: binding.run.clone(),
            claim_revision: c.snapshot.revision,
            step: NonEmptyText::new(binding.step.as_str()).unwrap(),
            dispatch: binding.dispatch.clone(),
            request: ConnectorRequestIdentity::from_request(&read.request)?,
            document: reference.clone(),
            bytes,
        };
        // This is the real scoped PostgreSQL cognitive journal. The strict
        // Session record codec still requires a genuine verified model turn;
        // no orphan ConnectorObservation or invented turn is admitted here.
        let scoped =
            crate::cognitive_store::CognitiveStore::connect(&connection, binding.scope.clone())?;
        let journal_kind = "qualified-direct-source-read-v1";
        scoped.transact(journal_kind, &Vec::<DirectReadEnvelope>::new(), |records| {
            if !records.is_empty() {
                return Err(SessionError::Duplicate);
            }
            records.push(envelope);
            Ok(())
        })?;
        let mut stored: Vec<DirectReadEnvelope> = scoped
            .snapshot(journal_kind)?
            .ok_or(SessionError::Unavailable)?;
        assert_eq!(stored.len(), 1);
        let stored = stored.pop().unwrap();
        assert_eq!(stored.owner, c.owner);
        assert_eq!(stored.scope, *c.goal.scope());
        assert_eq!(stored.session, c.snapshot.session);
        assert_eq!(stored.run, c.snapshot.run);
        assert_eq!(stored.claim_revision, c.snapshot.revision);
        assert_eq!(stored.step.as_str(), binding.step.as_str());
        assert_eq!(stored.dispatch, binding.dispatch);
        assert_eq!(
            stored.request,
            ConnectorRequestIdentity::from_request(&read.request)?
        );
        assert_eq!(stored.document, reference);
        assert_eq!(
            content_reference(stored.document.id().as_str(), &stored.bytes)?,
            stored.document
        );
        let bytes = stored.bytes;
        let decoded: DirectReadDocument = serde_json::from_slice(&bytes).unwrap();
        let records = ObservationEvidenceSet::from_json(&decoded.records).unwrap();
        let snapshot = SourceSnapshot::new(
            decoded.source.clone(),
            decoded.source_kind,
            decoded.source_revision.clone(),
            Some(decoded.content_digest),
        )
        .unwrap();
        let mut retained =
            ScopedObservationBatch::new(decoded.scope.clone(), snapshot, records).unwrap();
        for fact in retained.records().facts().to_vec() {
            retained =
                retained.with_quality_metadata(fact.subject().clone(), vec![decoded.quality]);
        }
        assert_eq!(retained, batch);
        let readset = RetainedObservationReadset::new(
            decoded.scope.clone(),
            vec![RetainedObservationMember {
                binding: gateway_domain::context_content_digest(&bytes),
                version_domain: gateway_domain::context_content_digest(
                    observation.metadata.object_reference.as_str().as_bytes(),
                ),
                batch: retained,
            }],
        )
        .unwrap();
        let logical = ReferenceId::new("target-logical-verification-batch").unwrap();
        let receipt = ClosedLoop::verify_retained(
            logical.clone(),
            decoded.scope,
            c.goal.intent().clone(),
            &readset,
            LoopRules {
                capabilities: PlanningCapabilitySnapshot::new(
                    native.resolved.snapshot.input().index.clone(),
                    native.resolved.report.basis.registry_fingerprint.as_str(),
                    PlanningIrVersion::V1,
                )
                .unwrap(),
                requirements: CapabilityRequirementRules::default(),
                planner: PlannerRules::default(),
                max_iterations: 2,
                max_retries: 1,
            },
        )
        .unwrap()
        .expect("complete original goal must be proven from the actual peer read");
        assert_eq!(receipt.execution(), &logical);
        assert_ne!(
            receipt.execution().as_str(),
            read.request.execution.as_str()
        );
        assert_eq!(
            receipt.goal_digest(),
            &ClosedLoop::goal_digest(c.goal.intent())
        );
        assert_eq!(target.usage()?.connector_calls, 2);
        eprintln!(
            "content-free completed target profile: capture_ms={capture_ms}; build_ms={build_ms}; bounded_discovery_and_read_ms={}; source_timings={:?}",
            started.elapsed().as_millis(),
            target.source.diagnostic_timings()
        );
        let methods = std::fs::read_to_string(&log).unwrap();
        assert_eq!(
            methods
                .lines()
                .filter(|method| *method == "tools/call")
                .count(),
            1
        );
        pipeline_completed.set(true);
        Ok(())
    };
    let called = Cell::new(false);
    let probe = Probe {
        local,
        plan,
        called: &called,
        callback: &callback,
    };
    let repository = store(&probe.local.current().unwrap()).unwrap();
    let coordinator = SessionCoordinator::new(repository, probe);
    let started = coordinator
        .execute(
            &owner,
            SessionCommand::Start {
                command: CommandId::new(format!("target-start-{nonce}")).unwrap(),
                intent: config.intent.clone(),
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
                        command: CommandId::new(format!("target-continue-{nonce}")).unwrap()
                    }
                }
            )
            .is_err()
    );
    assert!(called.get());
    assert!(
        pipeline_completed.get(),
        "the actual source/normalizer/Postgres/decode/original-goal pipeline must complete; an expected coordinator rejection cannot hide an early callback error"
    );
}

#[test]
fn error_translation_preserves_effect_uncertainty_independently_of_error_code() {
    for code in [
        ConnectorFailureCode::SchemaMismatch,
        ConnectorFailureCode::PermissionDenied,
        ConnectorFailureCode::ScopeMismatch,
    ] {
        let failure = |effect| ConnectorFailure {
            code,
            effect,
            correlation: NonEmptyText::new("effect-translation-unit").unwrap(),
        };
        assert_eq!(
            session_error(failure(ConnectorEffect::Unknown)),
            SessionError::OutcomeUnknown
        );
        assert_eq!(
            session_error(failure(ConnectorEffect::Acknowledged)),
            SessionError::OutcomeUnknown
        );
        assert_ne!(
            session_error(failure(ConnectorEffect::NotDispatched)),
            SessionError::OutcomeUnknown
        );
    }
}

// Dispatch-shaped input is data in this metadata-only unit test: it creates
// no native context, authorization, transport, observation or goal receipt.
fn metadata_dispatch() -> ConnectorDispatch {
    let text = |value: &str| NonEmptyText::new(value).unwrap();
    ConnectorDispatch {
        request: ConnectorRequest {
            target: ConnectorTarget {
                connector: text("metadata-unit"),
                scope: gateway_domain::ContextScopeId::new("metadata-scope").unwrap(),
            },
            capability: CapabilityId::new("metadata.read").unwrap(),
            class: ConnectorOperationClass::Read,
            inventory_revision: ContentDigest::new("a".repeat(64)).unwrap(),
            arguments: BTreeMap::new(),
            task: gateway_domain::TaskId::new("metadata-task").unwrap(),
            step: PlanStepId::new("metadata-action-step").unwrap(),
            execution: text("metadata-real-read-id"),
            correlation: text("metadata-read-1"),
        },
        policy: PolicyId::new("metadata-policy").unwrap(),
        policy_revision: ContentDigest::new("b".repeat(64)).unwrap(),
        consent_receipt: None,
        budget_reservation: text("metadata-reservation"),
        deadline: UnixTimestamp::new(clock().unwrap().seconds() + 60),
    }
}
fn metadata_profile() -> AcquisitionDocument {
    serde_json::from_value(json!({"source_kind":"TOOL","sensitivity":"NORMAL","authenticated_profile":"metadata.profile","semantics":{"kind":"DIRECT_CURRENT_READ_PROFILE","max_age_seconds":60,"time_basis":"REQUEST_START_LOWER_BOUND"}})).unwrap()
}
#[test]
fn immutable_metadata_roles_refuse_unknown_requests_and_never_upgrade_acknowledgements() {
    let hints = Arc::new(RoleHints::default());
    let collector = DirectCollector {
        profile: metadata_profile(),
        wire_source: None,
        hints: Arc::clone(&hints),
    };
    let mut dispatch = metadata_dispatch();
    assert!(collector.collect(&dispatch).is_err());
    let started = clock().unwrap();
    hints
        .insert(RoleHint {
            request: dispatch.request.clone(),
            source: None,
            role: ReadRole::SourceRead,
            started_at: started,
        })
        .unwrap();
    let read = collector.collect(&dispatch).unwrap();
    assert_eq!(read.basis, ConnectorObservationBasis::SourceRead);
    assert_eq!(read.observed_at, Some(started));
    assert_eq!(read.confidence, Confidence::NotApplicable);
    assert_eq!(read.uncertainty, Uncertainty::None);
    assert_eq!(read.authenticated_source.as_str(), "metadata.profile");
    dispatch.request.correlation = NonEmptyText::new("metadata-ack-1").unwrap();
    dispatch.request.class = ConnectorOperationClass::Mutate;
    hints
        .insert(RoleHint {
            request: dispatch.request.clone(),
            source: None,
            role: ReadRole::ProposalReturn,
            started_at: clock().unwrap(),
        })
        .unwrap();
    let ack = collector.collect(&dispatch).unwrap();
    assert_eq!(
        ack.basis,
        ConnectorObservationBasis::ProviderAcknowledgement
    );
    assert_eq!(ack.effect, ConnectorEffect::Acknowledged);
    assert_eq!(ack.observed_at, None);
    assert_eq!(ack.confidence, Confidence::Unknown);
    assert_eq!(ack.uncertainty, Uncertainty::Unknown);
    assert!(ack.verifies.is_none());
    dispatch.request.arguments.insert(
        NonEmptyText::new("foreign").unwrap(),
        TypedValue::Boolean(true),
    );
    assert!(
        collector.collect(&dispatch).is_err(),
        "a known correlation is insufficient when exact arguments change"
    );
}
#[test]
fn independent_read_metadata_requires_distinct_read_context_and_exact_mutation_identity() {
    let hints = Arc::new(RoleHints::default());
    let collector = DirectCollector {
        profile: metadata_profile(),
        wire_source: None,
        hints: Arc::clone(&hints),
    };
    let dispatch = metadata_dispatch();
    let mut original = dispatch.request.clone();
    original.class = ConnectorOperationClass::Mutate;
    original.correlation = NonEmptyText::new("metadata-original-mutation").unwrap();
    original.execution = NonEmptyText::new("metadata-action-context").unwrap();
    hints
        .insert(RoleHint {
            request: dispatch.request.clone(),
            source: None,
            role: ReadRole::IndependentPostEffectRead(Box::new(original.clone())),
            started_at: clock().unwrap(),
        })
        .unwrap();
    let measured = collector.collect(&dispatch).unwrap();
    assert_eq!(
        measured.basis,
        ConnectorObservationBasis::IndependentPostEffectRead
    );
    assert_eq!(measured.verifies, Some(original));
    assert_eq!(measured.effect, ConnectorEffect::NoMutation);
    let mut wrong = dispatch.clone();
    wrong.request.correlation = NonEmptyText::new("metadata-same-context").unwrap();
    let mut original = dispatch.request.clone();
    original.class = ConnectorOperationClass::Mutate;
    hints
        .insert(RoleHint {
            request: wrong.request.clone(),
            source: None,
            role: ReadRole::IndependentPostEffectRead(Box::new(original)),
            started_at: clock().unwrap(),
        })
        .unwrap();
    assert!(collector.collect(&wrong).is_err());
}
