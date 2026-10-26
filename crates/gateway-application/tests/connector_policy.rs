use gateway_application::{connector_policy::*, connectors::*};
use gateway_domain::*;
use gateway_policy::*;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

fn text(value: &str) -> NonEmptyText {
    NonEmptyText::new(value).unwrap()
}
fn digest(char: char) -> ContentDigest {
    ContentDigest::new(char.to_string().repeat(64)).unwrap()
}
fn request(class: ConnectorOperationClass, capability: &str) -> ConnectorRequest {
    ConnectorRequest {
        target: ConnectorTarget {
            connector: text("example"),
            scope: ContextScopeId::new("project-a").unwrap(),
        },
        capability: CapabilityId::new(capability).unwrap(),
        class,
        inventory_revision: digest('a'),
        arguments: BTreeMap::new(),
        task: TaskId::new("task").unwrap(),
        step: PlanStepId::new("step").unwrap(),
        execution: text("execution"),
        correlation: text("correlation"),
    }
}
fn snapshot(id: &str, class: CapabilityClass) -> ConnectorPolicySnapshot {
    let capability = CapabilityId::new(id).unwrap();
    let definition = CapabilityDefinition::new(capability.clone(), class);
    ConnectorPolicySnapshot {
        step: PlanStepId::new("step").unwrap(),
        resolved: true,
        has_prerequisites: false,
        constraints: Default::default(),
        preconditions: Default::default(),
        authority: PolicyAuthority {
            policies: vec![
                PolicyDefinition::new(
                    PolicyId::new("policy").unwrap(),
                    "test policy",
                    [capability.clone()],
                )
                .unwrap(),
            ],
            capabilities: BTreeMap::from([(capability.clone(), definition)]),
            ..Default::default()
        },
        facts: StepFacts {
            authorizations: BTreeMap::from([(capability.clone(), Approval::Granted)]),
            consents: BTreeMap::from([(capability, Approval::Granted)]),
            ..Default::default()
        },
        operating_mode: OperatingMode::Development,
        execution_profile: ExecutionProfile::FullPath,
        process: ProcessReadiness::Eligible,
        consent_required: false,
        administrative_capabilities: Default::default(),
        policy: PolicyId::new("policy").unwrap(),
        policy_revision: digest('b'),
        consent_receipt: None,
        budget_reservation: text("reservation"),
        deadline: UnixTimestamp::new(100),
    }
}
struct Source(Arc<Mutex<ConnectorPolicySnapshot>>);
impl ConnectorPolicySource for Source {
    fn admit_target(&self, _: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        Ok(())
    }
    fn authorize(
        &self,
        _: &ConnectorRequest,
        _: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn revalidate(
        &self,
        _: &ConnectorDispatch,
        _: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn record_decision(&self, _: &ConnectorPolicyEvaluation<'_>) -> Result<(), ConnectorFailure> {
        Ok(())
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum FailureAt {
    Admit,
    Authorize,
    Revalidate,
    Record,
}
struct FailingSource {
    fail_at: FailureAt,
    snapshot: ConnectorPolicySnapshot,
}
impl FailingSource {
    fn error(&self) -> ConnectorFailure {
        ConnectorFailure {
            code: ConnectorFailureCode::PermissionDenied,
            effect: ConnectorEffect::NotDispatched,
            correlation: text("correlation"),
        }
    }
}
impl ConnectorPolicySource for FailingSource {
    fn admit_target(&self, _: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        if self.fail_at == FailureAt::Admit {
            Err(self.error())
        } else {
            Ok(())
        }
    }
    fn authorize(
        &self,
        _: &ConnectorRequest,
        _: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        if self.fail_at == FailureAt::Authorize {
            Err(self.error())
        } else {
            Ok(self.snapshot.clone())
        }
    }
    fn revalidate(
        &self,
        _: &ConnectorDispatch,
        _: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        if self.fail_at == FailureAt::Revalidate {
            Err(self.error())
        } else {
            Ok(self.snapshot.clone())
        }
    }
    fn record_decision(&self, _: &ConnectorPolicyEvaluation<'_>) -> Result<(), ConnectorFailure> {
        if self.fail_at == FailureAt::Record {
            Err(self.error())
        } else {
            Ok(())
        }
    }
}
fn denied(result: Result<ConnectorDispatch, ConnectorFailure>) {
    assert_eq!(
        result.unwrap_err().code,
        ConnectorFailureCode::PermissionDenied
    );
}

#[test]
fn inspect_requires_canonical_inspect_permission_and_mutate_requires_consent() {
    let inspect = request(ConnectorOperationClass::Read, "example.read");
    let admission = Cg09ConnectorAdmission::new(Source(Arc::new(Mutex::new(snapshot(
        "example.read",
        CapabilityClass::Inspect,
    )))));
    let dispatch = admission.authorize(&inspect, None).unwrap();
    admission.revalidate(&dispatch, None).unwrap();

    let mutate = request(ConnectorOperationClass::Mutate, "example.write");
    let mut mutation_policy = snapshot("example.write", CapabilityClass::Mutate);
    mutation_policy.facts.consents.clear();
    let admission = Cg09ConnectorAdmission::new(Source(Arc::new(Mutex::new(mutation_policy))));
    denied(admission.authorize(&mutate, None));
}

#[test]
fn required_consent_and_admin_are_explicit_and_fail_closed() {
    let inspect = request(ConnectorOperationClass::Inspect, "example.inspect");
    let mut required = snapshot("example.inspect", CapabilityClass::Inspect);
    required.consent_required = true;
    denied(
        Cg09ConnectorAdmission::new(Source(Arc::new(Mutex::new(required.clone()))))
            .authorize(&inspect, None),
    );
    required.consent_receipt = Some(text("consent-receipt"));
    let dispatch = Cg09ConnectorAdmission::new(Source(Arc::new(Mutex::new(required))))
        .authorize(&inspect, None)
        .unwrap();
    assert_eq!(
        dispatch.consent_receipt.as_ref().unwrap().as_str(),
        "consent-receipt"
    );

    let admin = request(ConnectorOperationClass::Admin, "example.admin");
    let mut admin_snapshot = snapshot("example.admin", CapabilityClass::Mutate);
    admin_snapshot.facts.consents.insert(
        CapabilityId::new("example.admin").unwrap(),
        Approval::Granted,
    );
    admin_snapshot.consent_receipt = Some(text("admin-consent"));
    denied(
        Cg09ConnectorAdmission::new(Source(Arc::new(Mutex::new(admin_snapshot.clone()))))
            .authorize(&admin, None),
    );
    admin_snapshot
        .administrative_capabilities
        .insert(CapabilityId::new("example.admin").unwrap());
    assert!(
        Cg09ConnectorAdmission::new(Source(Arc::new(Mutex::new(admin_snapshot))))
            .authorize(&admin, None)
            .is_ok()
    );
}

#[test]
fn evidence_denials_and_stale_consent_or_policy_fail_revalidation() {
    let req = request(ConnectorOperationClass::Read, "example.read");
    let mut needs_evidence = snapshot("example.read", CapabilityClass::Inspect);
    needs_evidence.authority.required_evidence.insert(
        CapabilityId::new("example.read").unwrap(),
        ["review.complete".to_owned()].into(),
    );
    denied(
        Cg09ConnectorAdmission::new(Source(Arc::new(Mutex::new(needs_evidence))))
            .authorize(&req, None),
    );

    let mut initial = snapshot("example.read", CapabilityClass::Inspect);
    initial.consent_required = true;
    initial.consent_receipt = Some(text("consent-one"));
    let current = Arc::new(Mutex::new(initial));
    let admission = Cg09ConnectorAdmission::new(Source(current.clone()));
    let dispatch = admission.authorize(&req, None).unwrap();
    current.lock().unwrap().consent_receipt = Some(text("consent-two"));
    assert_eq!(
        admission.revalidate(&dispatch, None).unwrap_err().code,
        ConnectorFailureCode::PermissionDenied
    );
}

#[test]
fn unknown_mismatched_and_noncurrent_policy_bindings_are_denied() {
    let unknown = request(ConnectorOperationClass::Read, "missing.capability");
    let admission = Cg09ConnectorAdmission::new(Source(Arc::new(Mutex::new(snapshot(
        "example.read",
        CapabilityClass::Inspect,
    )))));
    denied(admission.authorize(&unknown, None));

    let mismatched = request(ConnectorOperationClass::Mutate, "example.read");
    denied(admission.authorize(&mismatched, None));

    let valid = request(ConnectorOperationClass::Read, "example.read");
    let mut stale = snapshot("example.read", CapabilityClass::Inspect);
    stale.policy = PolicyId::new("not-in-authority").unwrap();
    denied(
        Cg09ConnectorAdmission::new(Source(Arc::new(Mutex::new(stale)))).authorize(&valid, None),
    );
}

#[test]
fn changed_policy_revision_cannot_reuse_an_admitted_dispatch() {
    let req = request(ConnectorOperationClass::Read, "example.read");
    let current = Arc::new(Mutex::new(snapshot(
        "example.read",
        CapabilityClass::Inspect,
    )));
    let admission = Cg09ConnectorAdmission::new(Source(current.clone()));
    let dispatch = admission.authorize(&req, None).unwrap();
    current.lock().unwrap().policy_revision = digest('c');
    assert_eq!(
        admission.revalidate(&dispatch, None).unwrap_err().code,
        ConnectorFailureCode::PermissionDenied
    );
}

#[test]
fn trusted_source_failures_at_each_admission_stage_fail_closed() {
    let req = request(ConnectorOperationClass::Read, "example.read");
    let base = snapshot("example.read", CapabilityClass::Inspect);
    let allowed = Cg09ConnectorAdmission::new(Source(Arc::new(Mutex::new(base.clone()))));
    let dispatch = allowed.authorize(&req, None).unwrap();
    assert!(allowed.admit_target(&req.target).is_ok());

    for fail_at in [FailureAt::Admit, FailureAt::Authorize, FailureAt::Record] {
        let admission = Cg09ConnectorAdmission::new(FailingSource {
            fail_at,
            snapshot: base.clone(),
        });
        denied(admission.authorize(&req, None));
    }
    for fail_at in [FailureAt::Admit, FailureAt::Revalidate, FailureAt::Record] {
        let admission = Cg09ConnectorAdmission::new(FailingSource {
            fail_at,
            snapshot: base.clone(),
        });
        assert_eq!(
            admission.revalidate(&dispatch, None).unwrap_err().code,
            ConnectorFailureCode::PermissionDenied
        );
    }
    for fail_at in [FailureAt::Admit, FailureAt::Revalidate] {
        let admission = Cg09ConnectorAdmission::new(FailingSource {
            fail_at,
            snapshot: base.clone(),
        });
        let failure = admission
            .validate_current_dispatch(&dispatch, None)
            .unwrap_err();
        assert_eq!(
            failure,
            FailingSource {
                fail_at,
                snapshot: base.clone()
            }
            .error()
        );
    }
    // Polling reads current grants without entering the audit sink, even when
    // the sink would reject an ordinary admission or pre-effect audit.
    Cg09ConnectorAdmission::new(FailingSource {
        fail_at: FailureAt::Record,
        snapshot: base,
    })
    .validate_current_dispatch(&dispatch, None)
    .unwrap();
}

struct AuditedCurrentSource {
    current: Arc<Mutex<ConnectorPolicySnapshot>>,
    audits: Arc<std::sync::atomic::AtomicUsize>,
    revoke_on_audit: Arc<std::sync::atomic::AtomicBool>,
    reads: Arc<Mutex<Vec<Option<SourceId>>>>,
}
impl ConnectorPolicySource for AuditedCurrentSource {
    fn admit_target(&self, _: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        Ok(())
    }
    fn authorize(
        &self,
        _: &ConnectorRequest,
        source: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        self.reads.lock().unwrap().push(source.cloned());
        Ok(self.current.lock().unwrap().clone())
    }
    fn revalidate(
        &self,
        _: &ConnectorDispatch,
        source: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        self.reads.lock().unwrap().push(source.cloned());
        Ok(self.current.lock().unwrap().clone())
    }
    fn record_decision(
        &self,
        evaluation: &ConnectorPolicyEvaluation<'_>,
    ) -> Result<(), ConnectorFailure> {
        let request = evaluation.request();
        self.audits
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self
            .revoke_on_audit
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            self.current
                .lock()
                .unwrap()
                .facts
                .authorizations
                .insert(request.capability.clone(), Approval::Denied);
        }
        Ok(())
    }
}

#[test]
fn pre_effect_audit_revocation_denies_without_another_audit_or_recursive_lookup() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    let req = request(ConnectorOperationClass::Read, "example.read");
    let audits = Arc::new(AtomicUsize::new(0));
    let revoke = Arc::new(AtomicBool::new(false));
    let reads = Arc::new(Mutex::new(Vec::new()));
    let admission = Cg09ConnectorAdmission::new(AuditedCurrentSource {
        current: Arc::new(Mutex::new(snapshot(
            "example.read",
            CapabilityClass::Inspect,
        ))),
        audits: audits.clone(),
        revoke_on_audit: revoke.clone(),
        reads: reads.clone(),
    });
    let source = SourceId::new("repository").unwrap();
    let dispatch = admission.authorize(&req, Some(&source)).unwrap();
    revoke.store(true, Ordering::SeqCst);
    let failure = admission.revalidate(&dispatch, Some(&source)).unwrap_err();
    assert_eq!(failure.code, ConnectorFailureCode::PermissionDenied);
    assert_eq!(failure.effect, ConnectorEffect::NotDispatched);
    assert_eq!(audits.load(Ordering::SeqCst), 2);
    assert_eq!(reads.lock().unwrap().len(), 3);
    assert_eq!(
        admission.revalidate(&dispatch, Some(&source)).unwrap_err(),
        failure
    );
    assert_eq!(audits.load(Ordering::SeqCst), 3);
    assert_eq!(reads.lock().unwrap().len(), 4);
    assert!(
        reads
            .lock()
            .unwrap()
            .iter()
            .all(|read| read.as_ref() == Some(&source))
    );
}

#[test]
fn repeated_current_dispatch_polls_read_live_facts_without_growing_the_audit_log() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    let req = request(ConnectorOperationClass::Read, "example.read");
    let current = Arc::new(Mutex::new(snapshot(
        "example.read",
        CapabilityClass::Inspect,
    )));
    let audits = Arc::new(AtomicUsize::new(0));
    let reads = Arc::new(Mutex::new(Vec::new()));
    let admission = Cg09ConnectorAdmission::new(AuditedCurrentSource {
        current: current.clone(),
        audits: audits.clone(),
        revoke_on_audit: Arc::new(AtomicBool::new(false)),
        reads: reads.clone(),
    });
    let source = SourceId::new("repository").unwrap();
    let dispatch = admission.authorize(&req, Some(&source)).unwrap();
    assert_eq!(audits.load(Ordering::SeqCst), 1);
    for _ in 0..100 {
        admission
            .validate_current_dispatch(&dispatch, Some(&source))
            .unwrap();
    }
    assert_eq!(
        audits.load(Ordering::SeqCst),
        1,
        "transport polling is validation-only"
    );
    assert_eq!(reads.lock().unwrap().len(), 101);
    assert!(
        reads
            .lock()
            .unwrap()
            .iter()
            .all(|read| read.as_ref() == Some(&source))
    );
    admission.revalidate(&dispatch, Some(&source)).unwrap();
    assert_eq!(
        audits.load(Ordering::SeqCst),
        2,
        "normal pre-effect admission still records its decision"
    );
    current.lock().unwrap().budget_reservation = text("replaced-reservation");
    assert_eq!(
        admission
            .validate_current_dispatch(&dispatch, Some(&source))
            .unwrap_err()
            .code,
        ConnectorFailureCode::PermissionDenied
    );
    assert_eq!(audits.load(Ordering::SeqCst), 2);
}

#[test]
fn validation_only_poll_detects_each_changed_grant_binding_and_current_policy_denial() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    let req = request(ConnectorOperationClass::Read, "example.read");
    for field in [
        "policy",
        "policy_revision",
        "consent_receipt",
        "budget",
        "deadline",
        "authorization",
        "consent_status",
    ] {
        let mut allowed = snapshot("example.read", CapabilityClass::Inspect);
        allowed.consent_required = true;
        allowed.consent_receipt = Some(text("consent-one"));
        let current = Arc::new(Mutex::new(allowed));
        let audits = Arc::new(AtomicUsize::new(0));
        let admission = Cg09ConnectorAdmission::new(AuditedCurrentSource {
            current: current.clone(),
            audits: audits.clone(),
            revoke_on_audit: Arc::new(AtomicBool::new(false)),
            reads: Arc::new(Mutex::new(Vec::new())),
        });
        let dispatch = admission.authorize(&req, None).unwrap();
        admission
            .validate_current_dispatch(&dispatch, None)
            .unwrap();
        {
            let mut next = current.lock().unwrap();
            match field {
                "policy" => {
                    next.policy = PolicyId::new("replacement-policy").unwrap();
                    next.authority.policies = vec![
                        PolicyDefinition::new(
                            next.policy.clone(),
                            "replacement current policy",
                            [req.capability.clone()],
                        )
                        .unwrap(),
                    ];
                }
                "policy_revision" => next.policy_revision = digest('c'),
                "consent_receipt" => next.consent_receipt = Some(text("consent-two")),
                "budget" => next.budget_reservation = text("new-reservation"),
                "deadline" => next.deadline = UnixTimestamp::new(101),
                "authorization" => {
                    next.facts
                        .authorizations
                        .insert(req.capability.clone(), Approval::Denied);
                }
                _ => {
                    next.facts
                        .consents
                        .insert(req.capability.clone(), Approval::Denied);
                }
            }
        }
        let failure = admission
            .validate_current_dispatch(&dispatch, None)
            .unwrap_err();
        assert_eq!(
            failure.code,
            ConnectorFailureCode::PermissionDenied,
            "{field}"
        );
        assert_eq!(failure.effect, ConnectorEffect::NotDispatched, "{field}");
        assert_eq!(failure.correlation, req.correlation, "{field}");
        assert_eq!(audits.load(Ordering::SeqCst), 1, "{field}");
        let audited_failure = admission.revalidate(&dispatch, None).unwrap_err();
        assert_eq!(
            audited_failure, failure,
            "pre-effect checks enforce {field}"
        );
        assert_eq!(audits.load(Ordering::SeqCst), 2, "{field}");
    }
}

#[test]
fn each_operation_class_uses_its_canonical_cg09_capability_and_exact_dispatch() {
    for class in [
        ConnectorOperationClass::Read,
        ConnectorOperationClass::Search,
        ConnectorOperationClass::Inspect,
        ConnectorOperationClass::Mutate,
        ConnectorOperationClass::Admin,
    ] {
        let req = request(class, "example.operation");
        let mutation = matches!(
            class,
            ConnectorOperationClass::Mutate | ConnectorOperationClass::Admin
        );
        let mut current = snapshot(
            "example.operation",
            if mutation {
                CapabilityClass::Mutate
            } else {
                CapabilityClass::Inspect
            },
        );
        if mutation {
            current.consent_receipt = Some(text("operator-consent"));
        }
        if class == ConnectorOperationClass::Admin {
            current
                .administrative_capabilities
                .insert(req.capability.clone());
        }
        let state = Arc::new(Mutex::new(current));
        let admission = Cg09ConnectorAdmission::new(Source(state.clone()));
        let source = SourceId::new("scoped-readback").unwrap();
        let dispatch = admission.authorize(&req, Some(&source)).unwrap();
        assert_eq!(dispatch.request, req);
        admission.revalidate(&dispatch, Some(&source)).unwrap();
        admission
            .validate_current_dispatch(&dispatch, Some(&source))
            .unwrap();
        // Removing the canonical capability invalidates both admission paths;
        // a previously admitted dispatch cannot carry its old permission.
        state.lock().unwrap().authority.capabilities.clear();
        assert_eq!(
            admission
                .revalidate(&dispatch, Some(&source))
                .unwrap_err()
                .effect,
            ConnectorEffect::NotDispatched
        );
        assert_eq!(
            admission
                .validate_current_dispatch(&dispatch, Some(&source))
                .unwrap_err()
                .code,
            ConnectorFailureCode::PermissionDenied
        );
    }
}

#[test]
fn audit_callback_revocation_is_observed_by_the_next_readonly_liveness_check() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    let req = request(ConnectorOperationClass::Read, "example.read");
    for revoke_during_authorize in [true, false] {
        let current = Arc::new(Mutex::new(snapshot(
            "example.read",
            CapabilityClass::Inspect,
        )));
        let audits = Arc::new(AtomicUsize::new(0));
        let revoke = Arc::new(AtomicBool::new(revoke_during_authorize));
        let admission = Cg09ConnectorAdmission::new(AuditedCurrentSource {
            current,
            audits: audits.clone(),
            revoke_on_audit: revoke.clone(),
            reads: Arc::new(Mutex::new(Vec::new())),
        });
        let dispatch = admission.authorize(&req, None).unwrap();
        if !revoke_during_authorize {
            admission
                .validate_current_dispatch(&dispatch, None)
                .unwrap();
            revoke.store(true, Ordering::SeqCst);
            assert_eq!(
                admission.revalidate(&dispatch, None).unwrap_err().code,
                ConnectorFailureCode::PermissionDenied
            );
        }
        let expected_audits = if revoke_during_authorize { 1 } else { 2 };
        for _ in 0..10 {
            assert_eq!(
                admission
                    .validate_current_dispatch(&dispatch, None)
                    .unwrap_err()
                    .code,
                ConnectorFailureCode::PermissionDenied
            );
        }
        assert_eq!(audits.load(Ordering::SeqCst), expected_audits);
    }
}

#[test]
fn readonly_dispatch_validation_ignores_audit_sink_but_denies_unavailable_current_sources() {
    let req = request(ConnectorOperationClass::Read, "example.read");
    let base = snapshot("example.read", CapabilityClass::Inspect);
    let dispatch = Cg09ConnectorAdmission::new(Source(Arc::new(Mutex::new(base.clone()))))
        .authorize(&req, None)
        .unwrap();
    let audit_unavailable = Cg09ConnectorAdmission::new(FailingSource {
        fail_at: FailureAt::Record,
        snapshot: base.clone(),
    });
    audit_unavailable
        .validate_current_dispatch(&dispatch, None)
        .unwrap();
    assert!(audit_unavailable.revalidate(&dispatch, None).is_err());
    for fail_at in [FailureAt::Admit, FailureAt::Revalidate] {
        let admission = Cg09ConnectorAdmission::new(FailingSource {
            fail_at,
            snapshot: base.clone(),
        });
        assert_eq!(
            admission
                .validate_current_dispatch(&dispatch, None)
                .unwrap_err()
                .code,
            ConnectorFailureCode::PermissionDenied
        );
    }
    struct LegacyAdmission(ConnectorDispatch);
    impl ConnectorAdmissionPort for LegacyAdmission {
        fn admit_target(&self, _: &ConnectorTarget) -> Result<(), ConnectorFailure> {
            Ok(())
        }
        fn authorize(
            &self,
            _: &ConnectorRequest,
            _: Option<&SourceId>,
        ) -> Result<ConnectorDispatch, ConnectorFailure> {
            Ok(self.0.clone())
        }
        fn revalidate(
            &self,
            _: &ConnectorDispatch,
            _: Option<&SourceId>,
        ) -> Result<(), ConnectorFailure> {
            Ok(())
        }
    }
    let legacy = LegacyAdmission(dispatch.clone());
    legacy.revalidate(&dispatch, None).unwrap();
    let failure = legacy
        .validate_current_dispatch(&dispatch, None)
        .unwrap_err();
    assert_eq!(failure.code, ConnectorFailureCode::PermissionDenied);
    assert_eq!(failure.effect, ConnectorEffect::NotDispatched);
    assert_eq!(failure.correlation, dispatch.request.correlation);
}

#[derive(Clone)]
struct AuditCapture {
    request: ConnectorRequest,
    source: Option<SourceId>,
    snapshot: ConnectorPolicySnapshot,
    report: Option<StepPolicyReport>,
    outcome: ConnectorPolicyOutcome,
    dispatch: Option<ConnectorDispatch>,
}
struct EnvelopeSource {
    current: Arc<Mutex<ConnectorPolicySnapshot>>,
    records: Arc<Mutex<Vec<AuditCapture>>>,
    drift: bool,
    fail_audit: bool,
}
impl ConnectorPolicySource for EnvelopeSource {
    fn admit_target(&self, _: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        Ok(())
    }
    fn authorize(
        &self,
        _: &ConnectorRequest,
        _: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        Ok(self.current.lock().unwrap().clone())
    }
    fn revalidate(
        &self,
        _: &ConnectorDispatch,
        _: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        Ok(self.current.lock().unwrap().clone())
    }
    fn record_decision(
        &self,
        evaluation: &ConnectorPolicyEvaluation<'_>,
    ) -> Result<(), ConnectorFailure> {
        self.records.lock().unwrap().push(AuditCapture {
            request: evaluation.request().clone(),
            source: evaluation.source().cloned(),
            snapshot: evaluation.snapshot().clone(),
            report: evaluation.report().cloned(),
            outcome: evaluation.outcome(),
            dispatch: match evaluation.stage() {
                ConnectorPolicyStage::Authorize => None,
                ConnectorPolicyStage::PreEffect(dispatch) => Some(dispatch.clone()),
            },
        });
        if self.drift {
            let mut current = self.current.lock().unwrap();
            current.resolved = false;
            current.policy_revision = digest('f');
        }
        if self.fail_audit {
            Err(ConnectorFailure {
                code: ConnectorFailureCode::Unavailable,
                effect: ConnectorEffect::NotDispatched,
                correlation: evaluation.request().correlation.clone(),
            })
        } else {
            Ok(())
        }
    }
}

#[test]
fn snapshot_step_and_real_step_requirements_deny_with_truthful_audit_reports() {
    let req = request(ConnectorOperationClass::Read, "example.read");
    for case in [
        "step",
        "resolved",
        "prerequisite",
        "constraint",
        "precondition",
        "policy",
    ] {
        let mut facts = snapshot("example.read", CapabilityClass::Inspect);
        let expected_reason = match case {
            "step" => {
                facts.step = PlanStepId::new("foreign-step").unwrap();
                None
            }
            "resolved" => {
                facts.resolved = false;
                Some(PolicyReason::Unresolved)
            }
            "prerequisite" => {
                facts.has_prerequisites = true;
                Some(PolicyReason::PrerequisiteMissing)
            }
            "constraint" => {
                facts.constraints.insert("safe.target".into());
                Some(PolicyReason::ConstraintUnsatisfied)
            }
            "precondition" => {
                facts.preconditions.insert("before.committed".into());
                Some(PolicyReason::EvidenceMissing)
            }
            _ => {
                facts.authority.policies.clear();
                None
            }
        };
        let records = Arc::new(Mutex::new(Vec::new()));
        let admission = Cg09ConnectorAdmission::new(EnvelopeSource {
            current: Arc::new(Mutex::new(facts)),
            records: records.clone(),
            drift: false,
            fail_audit: false,
        });
        let failure = admission.authorize(&req, None).unwrap_err();
        assert_eq!(
            failure.code,
            ConnectorFailureCode::PermissionDenied,
            "{case}"
        );
        let captured = records.lock().unwrap();
        assert_eq!(captured.len(), 1, "{case}");
        let audit = &captured[0];
        assert_eq!(audit.outcome, ConnectorPolicyOutcome::Denied(failure.code));
        assert_eq!(audit.request, req);
        assert!(audit.dispatch.is_none());
        match expected_reason {
            Some(reason) => assert!(
                audit
                    .report
                    .as_ref()
                    .unwrap()
                    .findings
                    .iter()
                    .any(|finding| finding.reason == reason),
                "{case}"
            ),
            None => assert!(
                audit.report.is_none(),
                "contract failure has no invented report: {case}"
            ),
        }
    }
}

#[test]
fn audited_stale_dispatch_is_denied_despite_policy_allow_and_keeps_original_grant() {
    let req = request(ConnectorOperationClass::Read, "example.read");
    let current = Arc::new(Mutex::new(snapshot(
        "example.read",
        CapabilityClass::Inspect,
    )));
    let records = Arc::new(Mutex::new(Vec::new()));
    let admission = Cg09ConnectorAdmission::new(EnvelopeSource {
        current: current.clone(),
        records: records.clone(),
        drift: false,
        fail_audit: false,
    });
    let source = SourceId::new("authenticated-source").unwrap();
    let dispatch = admission.authorize(&req, Some(&source)).unwrap();
    current.lock().unwrap().budget_reservation = text("replacement-reservation");
    let failure = admission.revalidate(&dispatch, Some(&source)).unwrap_err();
    let captured = records.lock().unwrap();
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0].outcome, ConnectorPolicyOutcome::Allowed);
    let audit = &captured[1];
    assert_eq!(
        audit.report.as_ref().unwrap().decision,
        PolicyDecision::Allow
    );
    assert_eq!(audit.outcome, ConnectorPolicyOutcome::Denied(failure.code));
    assert_eq!(audit.dispatch.as_ref(), Some(&dispatch));
    assert_eq!(audit.source.as_ref(), Some(&source));
    assert_eq!(
        audit.snapshot.budget_reservation,
        text("replacement-reservation")
    );
    drop(captured);
    assert_eq!(
        admission
            .validate_current_dispatch(&dispatch, Some(&source))
            .unwrap_err(),
        failure
    );
    assert_eq!(
        records.lock().unwrap().len(),
        2,
        "polling never emits an audit envelope"
    );
}

#[test]
fn evaluation_snapshot_is_immutable_across_audit_drift_and_audit_errors_block_grants() {
    let req = request(ConnectorOperationClass::Read, "example.read");
    let current = Arc::new(Mutex::new(snapshot(
        "example.read",
        CapabilityClass::Inspect,
    )));
    let records = Arc::new(Mutex::new(Vec::new()));
    let admission = Cg09ConnectorAdmission::new(EnvelopeSource {
        current: current.clone(),
        records: records.clone(),
        drift: true,
        fail_audit: false,
    });
    let dispatch = admission.authorize(&req, None).unwrap();
    assert_eq!(dispatch.policy_revision, digest('b'));
    assert_eq!(current.lock().unwrap().policy_revision, digest('f'));
    let audit = records.lock().unwrap()[0].clone();
    assert!(audit.snapshot.resolved);
    assert_eq!(audit.snapshot.policy_revision, digest('b'));
    assert_eq!(audit.report.unwrap().decision, PolicyDecision::Allow);
    assert_eq!(audit.outcome, ConnectorPolicyOutcome::Allowed);
    assert!(
        admission
            .validate_current_dispatch(&dispatch, None)
            .is_err()
    );
    assert_eq!(records.lock().unwrap().len(), 1);

    for contract_denied in [false, true] {
        let mut facts = snapshot("example.read", CapabilityClass::Inspect);
        if contract_denied {
            facts.step = PlanStepId::new("wrong-step").unwrap();
        }
        let records = Arc::new(Mutex::new(Vec::new()));
        let admission = Cg09ConnectorAdmission::new(EnvelopeSource {
            current: Arc::new(Mutex::new(facts)),
            records: records.clone(),
            drift: false,
            fail_audit: true,
        });
        assert_eq!(
            admission.authorize(&req, None).unwrap_err().code,
            ConnectorFailureCode::Unavailable
        );
        assert_eq!(
            admission.revalidate(&dispatch, None).unwrap_err().code,
            ConnectorFailureCode::Unavailable
        );
        let captures = records.lock().unwrap();
        assert_eq!(captures.len(), 2);
        assert_eq!(
            captures[0].outcome,
            if contract_denied {
                ConnectorPolicyOutcome::Denied(ConnectorFailureCode::PermissionDenied)
            } else {
                ConnectorPolicyOutcome::Allowed
            }
        );
        assert_eq!(captures[0].report.is_none(), contract_denied);
        assert_eq!(captures[1].dispatch.as_ref(), Some(&dispatch));
    }
}

#[test]
fn explicit_snapshot_requirements_allow_when_exact_trusted_facts_satisfy_them() {
    let req = request(ConnectorOperationClass::Search, "example.search");
    let mut current = snapshot("example.search", CapabilityClass::Inspect);
    current.has_prerequisites = true;
    current.facts.prerequisites_satisfied = true;
    current.constraints.insert("safe.target".into());
    current
        .facts
        .satisfied_constraints
        .insert("safe.target".into());
    current.preconditions.insert("before.committed".into());
    current.facts.evidence.insert("before.committed".into());
    let records = Arc::new(Mutex::new(Vec::new()));
    let admission = Cg09ConnectorAdmission::new(EnvelopeSource {
        current: Arc::new(Mutex::new(current)),
        records: records.clone(),
        drift: false,
        fail_audit: false,
    });
    let dispatch = admission.authorize(&req, None).unwrap();
    admission.revalidate(&dispatch, None).unwrap();
    admission
        .validate_current_dispatch(&dispatch, None)
        .unwrap();
    let audits = records.lock().unwrap();
    assert_eq!(audits.len(), 2);
    assert!(
        audits
            .iter()
            .all(|audit| audit.outcome == ConnectorPolicyOutcome::Allowed)
    );
    assert!(
        audits
            .iter()
            .all(|audit| audit.report.as_ref().unwrap().step == req.step)
    );
}

#[test]
fn consent_preview_reports_exact_requirements_without_dispatch_or_audit() {
    let req = request(ConnectorOperationClass::Mutate, "example.update");
    let mut current = snapshot("example.update", CapabilityClass::Mutate);
    current.facts.consents.clear();
    current.consent_receipt = None;
    let report = preview_connector_policy(&req, &current).unwrap();
    assert_eq!(report.decision, PolicyDecision::RequireConsent);
    assert_eq!(report.step, req.step);
    // The normal audited boundary still refuses this same outstanding request.
    let records = Arc::new(Mutex::new(Vec::new()));
    let admission = Cg09ConnectorAdmission::new(EnvelopeSource {
        current: Arc::new(Mutex::new(current.clone())),
        records: records.clone(),
        drift: false,
        fail_audit: false,
    });
    assert!(admission.authorize(&req, None).is_err());
    assert_eq!(records.lock().unwrap().len(), 1);
    current.step = PlanStepId::new("foreign-step").unwrap();
    assert!(preview_connector_policy(&req, &current).is_err());
    current.step = req.step.clone();
    current.facts.authorizations.clear();
    current
        .facts
        .authorizations
        .insert(req.capability.clone(), Approval::Denied);
    assert_eq!(
        preview_connector_policy(&req, &current).unwrap().decision,
        PolicyDecision::Deny
    );
}

#[test]
fn legacy_admission_cannot_reuse_authorization_as_a_current_liveness_grant() {
    struct Legacy;
    impl ConnectorAdmissionPort for Legacy {
        fn admit_target(&self, _: &ConnectorTarget) -> Result<(), ConnectorFailure> {
            Ok(())
        }
        fn authorize(
            &self,
            _: &ConnectorRequest,
            _: Option<&SourceId>,
        ) -> Result<ConnectorDispatch, ConnectorFailure> {
            panic!("the default current guard must never acquire authorization")
        }
        fn revalidate(
            &self,
            _: &ConnectorDispatch,
            _: Option<&SourceId>,
        ) -> Result<(), ConnectorFailure> {
            panic!(
                "the default current guard must not silently fall back to a legacy revalidation callback"
            )
        }
    }
    let request = request(ConnectorOperationClass::Read, "example.read");
    let admission = Cg09ConnectorAdmission::new(Source(Arc::new(Mutex::new(snapshot(
        "example.read",
        CapabilityClass::Inspect,
    )))));
    let dispatch = admission.authorize(&request, None).unwrap();
    let failure = Legacy
        .validate_current_dispatch(&dispatch, None)
        .unwrap_err();
    assert_eq!(failure.code, ConnectorFailureCode::PermissionDenied);
    assert_eq!(failure.effect, ConnectorEffect::NotDispatched);
    assert_eq!(failure.correlation, request.correlation);
}

#[test]
fn explicit_denied_required_consent_keeps_native_denial_reason_and_truthful_audit() {
    for (class, canonical_class) in [
        (ConnectorOperationClass::Inspect, CapabilityClass::Inspect),
        (ConnectorOperationClass::Mutate, CapabilityClass::Mutate),
    ] {
        for retain_receipt in [false, true] {
            let request = request(class, "example.requires-consent");
            let mut allowed = snapshot("example.requires-consent", canonical_class);
            allowed.consent_required = true;
            allowed.consent_receipt = Some(text("authenticated-consent-record"));
            let current = Arc::new(Mutex::new(allowed));
            let records = Arc::new(Mutex::new(Vec::new()));
            let admission = Cg09ConnectorAdmission::new(EnvelopeSource {
                current: current.clone(),
                records: records.clone(),
                drift: false,
                fail_audit: false,
            });
            let dispatch = admission.authorize(&request, None).unwrap();
            {
                let mut denied = current.lock().unwrap();
                denied
                    .facts
                    .consents
                    .insert(request.capability.clone(), Approval::Denied);
                if !retain_receipt {
                    denied.consent_receipt = None;
                }
                let report = preview_connector_policy(&request, &denied).unwrap();
                assert_eq!(report.decision, PolicyDecision::Deny);
                assert_eq!(
                    report
                        .findings
                        .iter()
                        .filter(|f| f.reason == PolicyReason::ConsentDenied)
                        .count(),
                    1,
                    "the canonical and added consent checks agree"
                );
                assert!(
                    report
                        .findings
                        .iter()
                        .all(|f| f.reason != PolicyReason::ConsentMissing),
                    "explicit denial is not an unanswered consent question"
                );
            }
            let failure = admission.revalidate(&dispatch, None).unwrap_err();
            assert_eq!(failure.code, ConnectorFailureCode::PermissionDenied);
            assert_eq!(failure.effect, ConnectorEffect::NotDispatched);
            let failure = admission.authorize(&request, None).unwrap_err();
            assert_eq!(failure.code, ConnectorFailureCode::PermissionDenied);
            assert_eq!(failure.effect, ConnectorEffect::NotDispatched);
            let captured = records.lock().unwrap();
            assert_eq!(captured.len(), 3);
            assert_eq!(captured[1].dispatch.as_ref(), Some(&dispatch));
            assert!(captured[2].dispatch.is_none());
            for audit in &captured[1..] {
                assert_eq!(
                    audit.outcome,
                    ConnectorPolicyOutcome::Denied(ConnectorFailureCode::PermissionDenied)
                );
                let report = audit.report.as_ref().unwrap();
                assert_eq!(report.decision, PolicyDecision::Deny);
                assert!(report.findings.iter().any(|f| {
                    f.reason == PolicyReason::ConsentDenied && f.decision == PolicyDecision::Deny
                }));
                assert!(
                    report
                        .findings
                        .iter()
                        .all(|f| f.reason != PolicyReason::ConsentMissing)
                );
            }
        }
    }
}
