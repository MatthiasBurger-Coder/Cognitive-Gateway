use gateway_application::codex::*;
use gateway_application::{
    resolution_application::*, resolution_composition::CompositionRules, resolution_snapshot::*,
};
use gateway_domain::ContextScopeId;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::cell::Cell;
#[allow(dead_code)]
#[path = "support/composition.rs"]
mod composition;
#[allow(dead_code)]
#[path = "support/context_fixture.rs"]
mod context_fixture;
#[path = "support/codex_security.rs"]
mod security_cases;
mod support;

fn fixture(name: &str) -> Value {
    serde_json::from_str(
        &std::fs::read_to_string(format!(
            "{}/../../tests/fixtures/codex-v1/{name}.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap(),
    )
    .unwrap()
}
fn pinned(contract: &str, document: &Value) -> Value {
    json!({"id":"example","contract":contract,"contract_version":"1.0","revision":"1","digest":format!("sha256:{:x}", Sha256::digest(document.to_string().as_bytes()))})
}
struct Host {
    reads: Cell<usize>,
    corrupt: bool,
    secret: bool,
}
impl CodexHost for Host {
    fn operation_policy(&self, call: &Call) -> Result<OperationPolicy, FacadeError> {
        Ok(test_policy(call))
    }
    fn authorize(&self, _: &Call) -> Result<(), FacadeError> {
        Ok(())
    }
    fn reference(&self, call: &Call, reference: &Value) -> Result<ReferenceRecord, FacadeError> {
        self.reads.set(self.reads.get() + 1);
        let document = if reference["contract"] == "cg.situation" {
            fixture("inline.situation.inspect.request")["input"]["situation"]["document"].clone()
        } else if reference["contract"] == "cg.resolution" {
            if call.operation == "context.compile" {
                compile_resolution_document()
            } else {
                resolution_document()
            }
        } else {
            json!({})
        };
        let mut actual = pinned(reference["contract"].as_str().unwrap(), &document);
        if self.corrupt && call.operation != "context.compile" {
            actual["revision"] = json!("2");
        }
        Ok(ReferenceRecord {
            scope: call.scope.clone(),
            provenance: vec![
                json!({"reference":actual,"source_id":"source","source_revision":"1","freshness":"current","sensitivity":"NORMAL","lineage":[pinned("cg.evidence", &json!({}))]}),
            ],
            session: call.binding.session.clone(),
            reference: actual,
            document: document.to_string(),
        })
    }
    fn compile(&self, _: &Call, _: &[Value]) -> Result<CompileCommand, FacadeError> {
        let mut fixture = context_fixture::Fixture::new();
        if self.corrupt {
            fixture.authority.policies.clear();
        }
        Ok(CompileCommand {
            resolved: fixture.resolved,
            authority: fixture.authority,
            policy_context: fixture.policy,
            catalog: fixture.catalog,
            projection: fixture.projection,
            candidates: vec![],
            selected: Default::default(),
            disclosure: gateway_context::ContextDisclosurePolicy {
                maximum_sensitivity: gateway_domain::SensitivityClass::Normal,
                include_caller_input: false,
                include_external_content: false,
            },
        })
    }
    fn registry(
        &self,
        _: &Call,
    ) -> Result<(gateway_registry::Registry, gateway_process::ProcessRegistry), FacadeError> {
        let input = support::fixture();
        Ok((input.registry, input.processes))
    }
    fn resolution(
        &self,
        _: &Call,
        _: &[Value],
    ) -> Result<(ResolutionSnapshotInput, CompositionRules), FacadeError> {
        Ok((
            support::with_process(support::fixture()),
            composition::rules(),
        ))
    }
    fn resolved(&self, _: &Call, _: &Value) -> Result<ResolvedPlan, FacadeError> {
        DeclarativeResolutionApplication
            .resolve_plan(
                &support::with_process(support::fixture()),
                &composition::rules(),
            )
            .map_err(Into::into)
    }
    fn evidence(
        &self,
        _: &Call,
        _: &[Value],
    ) -> Result<gateway_domain::ObservationEvidenceSet, FacadeError> {
        gateway_domain::ObservationEvidenceSet::from_json(&fixture("inline.situation.inspect.request")["input"]["situation"]["document"]["records"].to_string()).map_err(|_| FacadeError::InvalidInput)
    }
    fn project(
        &self,
        _: &Call,
        contract: &str,
        document: Value,
    ) -> Result<Projection, FacadeError> {
        let reference = pinned(contract, &document);
        let link = pinned("cg.evidence", &json!({}));
        Ok(Projection {
            source: json!({"kind":"document","contract":contract,"contract_version":"1.0","document":document}),
            explainability: vec![pinned("cg.resolution-trace", &json!({}))],
            evidence: vec![link.clone()],
            provenance: vec![
                json!({"reference":reference,"source_id":"source","source_revision":"1","freshness":"current","sensitivity":if self.secret {"SECRET"} else {"NORMAL"},"lineage":[link]}),
            ],
        })
    }
}
fn facade(corrupt: bool, secret: bool) -> CodexFacade<Host> {
    CodexFacade::with_binding(
        ScopeBinding {
            scope: fixture("situation.inspect.request")["scope"].clone(),
            canonical_scope: ContextScopeId::new("project-a").unwrap(),
            mapping_revision: "1".into(),
            session: SessionContext {
                principal: "operator".into(),
                session_id: "test-session".into(),
                connection_id: "binding-example".into(),
            },
        },
        Host {
            reads: Cell::new(0),
            corrupt,
            secret,
        },
    )
    .unwrap()
}
fn code(response: &Value) -> &str {
    response["diagnostics"][0]["code"].as_str().unwrap()
}
#[test]
fn canonical_situation_validation_and_lineage_projection() {
    let app = facade(false, false);
    let request = fixture("inline.situation.inspect.request");
    let response = app.execute("situation.inspect", &request);
    assert_eq!(response["status"], "ok", "{response}");
    assert_eq!(
        response["result"]["canonical_result"]["document"],
        request["input"]["situation"]["document"]
    );
    assert_eq!(response["evidence"].as_array().unwrap().len(), 1);
    assert_eq!(response["explainability"].as_array().unwrap().len(), 2);
    assert_eq!(response["provenance"][0]["lineage"], response["evidence"]);
    let mut invalid = request.clone();
    invalid["input"]["situation"]["document"]["situation"]["observed_state_id"] =
        json!("wrong-state");
    assert_eq!(
        code(&app.execute("situation.inspect", &invalid)),
        "CG_INVALID_INPUT"
    );
    invalid = request.clone();
    invalid["input"]["situation"]["document"]["authorization"] = json!("ALLOW");
    assert_eq!(
        code(&app.execute("situation.inspect", &invalid)),
        "CG_INVALID_INPUT"
    );
    assert_eq!(
        code(&facade(false, true).execute("situation.inspect", &request)),
        "CG_SENSITIVITY_DENIED"
    );
}
#[test]
fn validation_precedence_scope_versions_and_pins_fail_closed() {
    let app = facade(false, false);
    let mut request = fixture("situation.inspect.request");
    let document =
        fixture("inline.situation.inspect.request")["input"]["situation"]["document"].clone();
    request["input"]["situation"]["reference"] = pinned("cg.situation", &document);
    assert_eq!(app.execute("situation.inspect", &request)["status"], "ok");
    assert_eq!(
        code(&facade(true, false).execute("situation.inspect", &request)),
        "CG_STALE_REVISION"
    );
    request["input"]["situation"]["reference"]["digest"] =
        json!(format!("sha256:{}", "0".repeat(64)));
    assert_eq!(
        code(&app.execute("situation.inspect", &request)),
        "CG_STALE_REVISION"
    );
    request["scope"]["project_id"] = json!("another-project");
    assert_eq!(
        code(&app.execute("situation.inspect", &request)),
        "CG_SCOPE_DENIED"
    );
    request["input"]["extra"] = json!("untrusted");
    assert_eq!(
        code(&app.execute("situation.inspect", &request)),
        "CG_INVALID_REQUEST"
    );
    request["schema_version"] = json!("2.0");
    assert_eq!(
        code(&app.execute("situation.inspect", &request)),
        "CG_UNSUPPORTED_VERSION"
    );
    let mut request = fixture("situation.inspect.request");
    request["operation"] = json!("unknown");
    assert_eq!(
        code(&app.execute("unknown", &request)),
        "CG_UNKNOWN_OPERATION"
    );
    assert_eq!(
        code(&app.execute("registry.inspect", &fixture("situation.inspect.request"))),
        "CG_INVALID_REQUEST"
    );
}
#[test]
fn assessment_uses_shared_normalization_and_cg02_semantics() {
    let app = facade(false, false);
    let records =
        fixture("inline.situation.inspect.request")["input"]["situation"]["document"]["records"]
            .clone();
    let mut request = fixture("situation.assess.request");
    request["input"]["situation"] = json!({"kind":"document","contract":"cg.situation-assembly","contract_version":"1.0","document":{
        "schema_version":1,"scope":"project-a","operating_mode":"DEVELOPMENT","execution_profile":"FULL_PATH",
        "context":{"id":"context","schema_version":"1.0"},"observed_state_id":"current","situation_id":"situation","records":records,"intent":null}});
    let response = app.execute("situation.assess", &request);
    assert_eq!(response["status"], "ok", "{response}");
    assert_eq!(
        response["result"]["canonical_result"]["document"]["document"]["observed_state"]["entries"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    request["execution"]["operating_mode"] = json!("HARDENING");
    assert_eq!(
        code(&app.execute("situation.assess", &request)),
        "CG_INVALID_INPUT"
    );
    request["input"]["situation"]["document"]["scope"] = json!("different-project");
    assert_eq!(
        code(&app.execute("situation.assess", &request)),
        "CG_SCOPE_DENIED"
    );
}
#[test]
fn registry_resolver_explain_and_evidence_use_existing_owners() {
    let app = facade(false, false);
    for kind in ["agent", "skill", "process", "capability"] {
        let mut request = fixture("registry.inspect.request");
        request["input"] = json!({"kind":kind,"ids":[]});
        let response = app.execute("registry.inspect", &request);
        assert_eq!(response["status"], "ok", "{response}");
        assert!(
            !response["result"]["canonical_result"]["document"]["entries"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        request["input"]["ids"] = json!(["missing"]);
        assert_eq!(
            app.execute("registry.inspect", &request)["result"]["canonical_result"]["document"]["entries"],
            json!([])
        );
    }
    for operation in ["capabilities.resolve", "state.explain", "evidence.inspect"] {
        let mut request = fixture(&format!("{operation}.request"));
        request["execution"]["operating_mode"] = json!("HARDENING");
        for value in request["input"].as_object_mut().unwrap().values_mut() {
            if value.is_array() {
                for r in value.as_array_mut().unwrap() {
                    r["digest"] = pinned("cg.evidence", &json!({}))["digest"].clone();
                }
            } else {
                let doc = if value["contract"] == "cg.resolution" {
                    resolution_document()
                } else {
                    json!({})
                };
                value["digest"] = pinned("cg.plan", &doc)["digest"].clone();
            }
        }
        let response = app.execute(operation, &request);
        assert_eq!(response["status"], "ok", "{operation}: {response}");
        assert_eq!(app.execute(operation, &request), response);
    }
}
#[test]
fn sessions_remain_unsupported_without_shared_service() {
    let app = facade(false, false);
    for operation in [
        "session.start",
        "session.inspect",
        "session.continue",
        "session.cancel",
        "session.approve",
        "session.clarify",
    ] {
        let response = app.execute(operation, &fixture(&format!("{operation}.request")));
        assert_eq!(code(&response), "CG_UNSUPPORTED_CAPABILITY");
        assert!(response["result"].is_null());
    }
}

fn resolution_document() -> Value {
    let resolved = DeclarativeResolutionApplication
        .resolve_plan(
            &support::with_process(support::fixture()),
            &composition::rules(),
        )
        .unwrap();
    serde_json::from_str(
        &DeclarativeResolutionApplication
            .serialize_resolution(&resolved, Default::default())
            .unwrap(),
    )
    .unwrap()
}

fn compile_resolution_document() -> Value {
    serde_json::from_str(
        &DeclarativeResolutionApplication
            .serialize_resolution(
                &context_fixture::Fixture::new().resolved,
                Default::default(),
            )
            .unwrap(),
    )
    .unwrap()
}
#[test]
fn context_compilation_rechecks_policy_and_rejects_stale_steps() {
    let mut request = fixture("context.compile.request");
    request["execution"]["operating_mode"] = json!("HARDENING");
    request["input"]["resolution"] = pinned("cg.resolution", &compile_resolution_document());
    request["input"]["projection"] = pinned("cg.context-projection", &json!({}));
    request["input"]["candidates"] = json!([]);
    request["input"]["step_id"] = json!(
        context_fixture::Fixture::new()
            .projection
            .mapping
            .step
            .as_str()
    );
    let response = facade(false, false).execute("context.compile", &request);
    assert_eq!(response["status"], "ok", "{response}");
    assert_eq!(
        response["result"]["canonical_result"]["contract"],
        "cg.execution-context"
    );
    let denied = facade(true, false).execute("context.compile", &request);
    assert_eq!(code(&denied), "CG_POLICY_DENIED", "{denied}");
    request["input"]["step_id"] = json!("unmapped-step");
    assert_eq!(
        code(&facade(false, false).execute("context.compile", &request)),
        "CG_INVALID_INPUT"
    );
}

#[test]
fn frozen_responses_and_sanitized_failures_match_complete_schema() {
    let common = contracts::artifact("common.schema.json").unwrap();
    let schema = contracts::artifact("response.schema.json").unwrap();
    for diagnostic in common["$defs"]["diagnostic"]["oneOf"].as_array().unwrap() {
        let code = diagnostic["properties"]["code"]["const"].as_str().unwrap();
        let response = contracts::failure(code);
        assert!(contracts::valid(&response, &schema, &common), "{code}");
        assert_eq!(response, fixture(&format!("{code}.response")));
    }
    for name in [
        "situation.inspect",
        "situation.assess",
        "context.compile",
        "capabilities.resolve",
        "state.explain",
        "evidence.inspect",
        "registry.inspect",
        "session.start",
        "session.inspect",
        "session.clarify",
        "session.approve",
        "session.continue",
        "session.cancel",
    ] {
        assert!(
            contracts::valid(&fixture(&format!("{name}.response")), &schema, &common),
            "{name}"
        );
    }
    let mut request = fixture("inline.situation.inspect.request");
    request["input"]["situation"]["contract_version"] = json!("2.0");
    assert_eq!(
        code(&facade(false, false).execute("situation.inspect", &request)),
        "CG_UNSUPPORTED_VERSION"
    );
    request["input"]["situation"]["document"] = json!({"oversized":"x".repeat(1_048_576)});
    assert_eq!(
        code(&facade(false, false).execute("situation.inspect", &request)),
        "CG_LIMIT_EXCEEDED"
    );
}

#[test]
fn shared_task_sessions_require_the_exact_immutable_client_owner() {
    struct SessionHost {
        foreign: bool,
        dispatches: std::rc::Rc<Cell<usize>>,
    }
    impl CodexHost for SessionHost {
        fn operation_policy(&self, call: &Call) -> Result<OperationPolicy, FacadeError> {
            Ok(test_policy(call))
        }
        fn authorize(&self, _: &Call) -> Result<(), FacadeError> {
            Ok(())
        }
        fn session_owner(&self, call: &Call, _: &str) -> Result<ScopeBinding, FacadeError> {
            let mut owner = call.binding.clone();
            if self.foreign {
                owner.session.session_id = "another-codex-session".into();
            }
            Ok(owner)
        }
        fn session(&self, call: &Call) -> Result<Value, FacadeError> {
            self.dispatches.set(self.dispatches.get() + 1);
            Ok(
                json!({"kind":"session","session_id":call.input["session_id"].as_str().unwrap_or("new-task"),
                "revision":0,"status":"running","pending":[],"verified_final_result":null}),
            )
        }
    }
    let binding = ScopeBinding {
        scope: fixture("session.inspect.request")["scope"].clone(),
        canonical_scope: ContextScopeId::new("canonical-project").unwrap(),
        mapping_revision: "1".into(),
        session: SessionContext {
            principal: "operator".into(),
            session_id: "client-session".into(),
            connection_id: "binding-example".into(),
        },
    };
    for operation in ["session.inspect", "session.start"] {
        let request = fixture(&format!("{operation}.request"));
        let dispatches = std::rc::Rc::new(Cell::new(0));
        let app = CodexFacade::with_binding(
            binding.clone(),
            SessionHost {
                foreign: true,
                dispatches: dispatches.clone(),
            },
        )
        .unwrap();
        let response = app.execute(operation, &request);
        assert_eq!(code(&response), "CG_SCOPE_DENIED");
        assert!(response["result"].is_null());
        assert_eq!(
            dispatches.get(),
            if operation == "session.start" { 1 } else { 0 }
        );
        let app = CodexFacade::with_binding(
            binding.clone(),
            SessionHost {
                foreign: false,
                dispatches: std::rc::Rc::new(Cell::new(0)),
            },
        )
        .unwrap();
        assert_eq!(app.execute(operation, &request)["status"], "ok");
    }
}

#[test]
fn reference_records_from_another_client_session_fail_before_projection() {
    struct ForeignHost;
    impl CodexHost for ForeignHost {
        fn operation_policy(&self, call: &Call) -> Result<OperationPolicy, FacadeError> {
            Ok(test_policy(call))
        }
        fn authorize(&self, _: &Call) -> Result<(), FacadeError> {
            Ok(())
        }
        fn reference(
            &self,
            call: &Call,
            reference: &Value,
        ) -> Result<ReferenceRecord, FacadeError> {
            let mut session = call.binding.session.clone();
            session.session_id = "foreign-session".into();
            Ok(ReferenceRecord {
                scope: call.scope.clone(),
                session,
                reference: reference.clone(),
                document: "PRIVATE_OTHER_SESSION".into(),
                provenance: vec![],
            })
        }
    }
    let request = fixture("situation.inspect.request");
    let app = CodexFacade::with_binding(
        ScopeBinding {
            scope: request["scope"].clone(),
            canonical_scope: ContextScopeId::new("canonical-project").unwrap(),
            mapping_revision: "1".into(),
            session: SessionContext {
                principal: "operator".into(),
                session_id: "test-session".into(),
                connection_id: "binding-example".into(),
            },
        },
        ForeignHost,
    )
    .unwrap();
    let response = app.execute("situation.inspect", &request);
    assert_eq!(code(&response), "CG_SCOPE_DENIED");
    assert!(!response.to_string().contains("PRIVATE_OTHER_SESSION"));
}

#[test]
fn resource_adapter_cannot_substitute_a_different_pinned_identity() {
    struct SubstitutionHost;
    impl CodexHost for SubstitutionHost {
        fn operation_policy(&self, call: &Call) -> Result<OperationPolicy, FacadeError> {
            Ok(test_policy(call))
        }
        fn authorize(&self, _: &Call) -> Result<(), FacadeError> {
            Ok(())
        }
        fn resource_reference(
            &self,
            _: &Call,
            _: &str,
            _: &str,
            _: &str,
        ) -> Result<Value, FacadeError> {
            Ok(pinned("cg.situation", &json!({})))
        }
        fn reference(&self, _: &Call, _: &Value) -> Result<ReferenceRecord, FacadeError> {
            panic!("substituted resource identity must fail before reading content")
        }
    }
    let scope = fixture("situation.inspect.request")["scope"].clone();
    let binding = ScopeBinding {
        scope: scope.clone(),
        canonical_scope: ContextScopeId::new("project-a").unwrap(),
        mapping_revision: "1".into(),
        session: SessionContext {
            principal: "operator".into(),
            session_id: "test-session".into(),
            connection_id: "binding-example".into(),
        },
    };
    let app = CodexFacade::with_binding(binding, SubstitutionHost).unwrap();
    assert_eq!(
        app.read_resource(
            &scope,
            "requested",
            "1",
            pinned("cg.situation", &json!({}))["digest"]
                .as_str()
                .unwrap()
        ),
        Err(FacadeError::StaleRevision)
    );
}

fn test_policy(call: &Call) -> OperationPolicy {
    use gateway_domain::{CapabilityDefinition, PolicyDefinition, PolicyId};
    use gateway_policy::{Approval, PolicyAuthority, ProcessReadiness, StepFacts};
    let id = operation_capability(&call.operation).unwrap();
    let capability = CapabilityDefinition::new(
        id.clone(),
        operation_class(&call.operation).unwrap().capability_class(),
    );
    OperationPolicy {
        authority: PolicyAuthority {
            policies: vec![
                PolicyDefinition::new(
                    PolicyId::new("test-policy").unwrap(),
                    "Explicit test admission",
                    [id.clone()],
                )
                .unwrap(),
            ],
            capabilities: [(id.clone(), capability)].into(),
            ..Default::default()
        },
        facts: StepFacts {
            authorizations: [(id.clone(), Approval::Granted)].into(),
            consents: [(id, Approval::Granted)].into(),
            ..Default::default()
        },
        process: ProcessReadiness::NotApplicable,
        operating_mode: call.operating_mode,
        execution_profile: call.execution_profile,
        mutations_enabled: true,
    }
}

mod policy_gates {
    use super::*;
    use gateway_policy::{Approval, PolicyDecision, PolicyReason, StepPolicyReport};
    use std::rc::Rc;

    #[derive(Clone, Copy)]
    enum Case {
        Allow,
        NoPolicy,
        UnknownCapability,
        NoAuthorization,
        NoConsent,
        ConsentDenied,
        Disabled,
        WrongClass,
        WrongMode,
        Evidence,
        Blocked,
        ReadOnly,
        ExplicitDeny,
    }
    struct PolicyHost {
        case: Case,
        dispatches: Rc<Cell<usize>>,
        reports: Rc<std::cell::RefCell<Vec<StepPolicyReport>>>,
    }
    impl CodexHost for PolicyHost {
        // A permissive availability/disclosure hook must never bypass policy.
        fn authorize(&self, _: &Call) -> Result<(), FacadeError> {
            Ok(())
        }
        fn operation_policy(&self, call: &Call) -> Result<OperationPolicy, FacadeError> {
            let mut policy = test_policy(call);
            let id = operation_capability(&call.operation)?;
            match self.case {
                Case::Allow => {}
                Case::NoPolicy => policy.authority.policies.clear(),
                Case::UnknownCapability => policy.authority.capabilities.clear(),
                Case::NoAuthorization => policy.facts.authorizations.clear(),
                Case::NoConsent => policy.facts.consents.clear(),
                Case::ConsentDenied => {
                    policy.facts.consents.insert(id, Approval::Denied);
                }
                Case::Disabled => policy.mutations_enabled = false,
                Case::WrongClass => {
                    policy.authority.capabilities.insert(
                        id.clone(),
                        gateway_domain::CapabilityDefinition::new(
                            id,
                            if operation_class(&call.operation)?.capability_class()
                                == gateway_domain::CapabilityClass::Inspect
                            {
                                gateway_domain::CapabilityClass::Mutate
                            } else {
                                gateway_domain::CapabilityClass::Inspect
                            },
                        ),
                    );
                }
                Case::WrongMode => policy.operating_mode = gateway_domain::OperatingMode::Hardening,
                Case::Evidence => {
                    policy
                        .authority
                        .required_evidence
                        .insert(id, ["review".into()].into());
                }
                Case::Blocked => policy.process = gateway_policy::ProcessReadiness::Blocked,
                Case::ReadOnly => {
                    let capability = policy.authority.capabilities[&id]
                        .clone()
                        .with_constraints(["read-only"])
                        .unwrap();
                    policy.authority.capabilities.insert(id, capability);
                    policy
                        .facts
                        .satisfied_constraints
                        .insert("read-only".into());
                }
                Case::ExplicitDeny => {
                    policy.authority.policies.push(
                        gateway_domain::PolicyDefinition::with_denied_capabilities(
                            gateway_domain::PolicyId::new("deny").unwrap(),
                            "Explicit deny",
                            [],
                            [id],
                        )
                        .unwrap(),
                    );
                }
            }
            Ok(policy)
        }
        fn policy_decision(&self, _: &Call, report: &StepPolicyReport) {
            self.reports.borrow_mut().push(report.clone());
        }
        fn session_owner(&self, call: &Call, _: &str) -> Result<ScopeBinding, FacadeError> {
            Ok(call.binding.clone())
        }
        fn session(&self, _: &Call) -> Result<Value, FacadeError> {
            self.dispatches.set(self.dispatches.get() + 1);
            Ok(
                json!({"kind":"session","session_id":"session-example","revision":1,"status":"running","pending":[],"verified_final_result":null}),
            )
        }
        fn resource_reference(
            &self,
            _: &Call,
            _: &str,
            _: &str,
            _: &str,
        ) -> Result<Value, FacadeError> {
            self.dispatches.set(self.dispatches.get() + 1);
            Err(FacadeError::ReferenceUnavailable)
        }
    }
    type PolicyFixture = (
        CodexFacade<PolicyHost>,
        Rc<Cell<usize>>,
        Rc<std::cell::RefCell<Vec<StepPolicyReport>>>,
    );
    fn policy_app(case: Case) -> PolicyFixture {
        let dispatches = Rc::new(Cell::new(0));
        let reports = Rc::new(std::cell::RefCell::new(vec![]));
        let binding = security_cases::binding();
        (
            CodexFacade::with_binding(
                binding,
                PolicyHost {
                    case,
                    dispatches: dispatches.clone(),
                    reports: reports.clone(),
                },
            )
            .unwrap(),
            dispatches,
            reports,
        )
    }
    #[test]
    fn mutation_requires_current_policy_enablement_consent_and_evidence() {
        for operation in [
            "session.start",
            "session.approve",
            "session.cancel",
            "session.clarify",
            "session.continue",
        ] {
            let request = fixture(&format!("{operation}.request"));
            for (case, expected, reason) in [
                (
                    Case::NoPolicy,
                    "CG_POLICY_DENIED",
                    PolicyReason::NotAllowlisted,
                ),
                (
                    Case::UnknownCapability,
                    "CG_POLICY_DENIED",
                    PolicyReason::UnknownCapability,
                ),
                (
                    Case::NoAuthorization,
                    "CG_CONSENT_REQUIRED",
                    PolicyReason::AuthorizationMissing,
                ),
                (
                    Case::NoConsent,
                    "CG_CONSENT_REQUIRED",
                    PolicyReason::ConsentMissing,
                ),
                (
                    Case::ConsentDenied,
                    "CG_POLICY_DENIED",
                    PolicyReason::ConsentDenied,
                ),
                (
                    Case::Disabled,
                    "CG_POLICY_DENIED",
                    PolicyReason::AuthorizationDenied,
                ),
                (
                    Case::WrongClass,
                    "CG_POLICY_DENIED",
                    PolicyReason::ContractMismatch,
                ),
                (
                    Case::WrongMode,
                    "CG_POLICY_DENIED",
                    PolicyReason::InvalidExecutionProfile,
                ),
                (
                    Case::Evidence,
                    "CG_EVIDENCE_REQUIRED",
                    PolicyReason::EvidenceMissing,
                ),
                (
                    Case::Blocked,
                    "CG_POLICY_DENIED",
                    PolicyReason::ProcessBlocked,
                ),
                (
                    Case::ReadOnly,
                    "CG_POLICY_DENIED",
                    PolicyReason::ConstraintViolation,
                ),
                (
                    Case::ExplicitDeny,
                    "CG_POLICY_DENIED",
                    PolicyReason::ExplicitDeny,
                ),
            ] {
                let (app, dispatches, reports) = policy_app(case);
                let first = app.execute(operation, &request);
                assert_eq!(code(&first), expected, "{operation}");
                assert_eq!(first, app.execute(operation, &request));
                assert_eq!(dispatches.get(), 0);
                let reports = reports.borrow();
                assert_eq!(reports[0], reports[1]);
                assert!(reports[0].findings.iter().any(|f| f.reason == reason));
                assert!(contracts::valid(
                    &first,
                    &contracts::artifact("response.schema.json").unwrap(),
                    &contracts::artifact("common.schema.json").unwrap()
                ));
            }
            let (app, dispatches, reports) = policy_app(Case::Allow);
            assert_eq!(app.execute(operation, &request)["status"], "ok");
            assert_eq!(dispatches.get(), 1);
            assert_eq!(reports.borrow()[0].decision, PolicyDecision::Allow);
        }
    }

    #[test]
    fn consent_is_rechecked_for_each_invocation() {
        struct RevokingHost {
            host: PolicyHost,
            evaluations: Cell<usize>,
        }
        impl CodexHost for RevokingHost {
            fn authorize(&self, _: &Call) -> Result<(), FacadeError> {
                Ok(())
            }
            fn operation_policy(&self, call: &Call) -> Result<OperationPolicy, FacadeError> {
                let mut policy = test_policy(call);
                if self.evaluations.replace(self.evaluations.get() + 1) > 0 {
                    policy.facts.consents.clear();
                }
                Ok(policy)
            }
            fn session_owner(&self, call: &Call, task: &str) -> Result<ScopeBinding, FacadeError> {
                self.host.session_owner(call, task)
            }
            fn session(&self, call: &Call) -> Result<Value, FacadeError> {
                self.host.session(call)
            }
        }
        let dispatches = Rc::new(Cell::new(0));
        let app = CodexFacade::with_binding(
            security_cases::binding(),
            RevokingHost {
                host: PolicyHost {
                    case: Case::Allow,
                    dispatches: dispatches.clone(),
                    reports: Rc::new(std::cell::RefCell::new(vec![])),
                },
                evaluations: Cell::new(0),
            },
        )
        .unwrap();
        let request = fixture("session.approve.request");
        assert_eq!(app.execute("session.approve", &request)["status"], "ok");
        assert_eq!(
            code(&app.execute("session.approve", &request)),
            "CG_CONSENT_REQUIRED"
        );
        assert_eq!(dispatches.get(), 1);
    }

    #[test]
    fn inspection_class_cannot_be_substituted_with_mutation() {
        let (app, dispatches, reports) = policy_app(Case::WrongClass);
        let request = fixture("session.inspect.request");
        assert_eq!(
            code(&app.execute("session.inspect", &request)),
            "CG_POLICY_DENIED"
        );
        assert_eq!(dispatches.get(), 0);
        assert!(
            reports.borrow()[0]
                .findings
                .iter()
                .any(|f| f.reason == PolicyReason::ContractMismatch)
        );
        assert_eq!(
            OperationClass::Read.capability_class(),
            gateway_domain::CapabilityClass::Inspect
        );
        assert_eq!(
            OperationClass::Search.capability_class(),
            gateway_domain::CapabilityClass::Inspect
        );
        assert_eq!(
            OperationClass::Admin.capability_class(),
            gateway_domain::CapabilityClass::Mutate
        );
    }
    #[test]
    fn discovery_and_permissive_host_do_not_supply_policy() {
        struct Permissive;
        impl CodexHost for Permissive {
            fn authorize(&self, _: &Call) -> Result<(), FacadeError> {
                Ok(())
            }
        }
        let app = CodexFacade::with_binding(security_cases::binding(), Permissive).unwrap();
        for tool in contracts::artifact("catalog").unwrap()["tools"]
            .as_array()
            .unwrap()
        {
            let operation = tool["operation"].as_str().unwrap();
            let request = fixture(&format!("{operation}.request"));
            assert_eq!(code(&app.execute(operation, &request)), "CG_POLICY_DENIED");
            assert_eq!(
                operation_class(operation)
                    .unwrap()
                    .capability_class()
                    .as_str()
                    .to_lowercase(),
                tool["classification"]
            );
        }
        assert_eq!(
            app.read_resource(
                &fixture("situation.inspect.request")["scope"],
                "example",
                "1",
                "sha256:0000"
            ),
            Err(FacadeError::PolicyDenied)
        );
        for operation in [
            "policy.alter",
            "capabilities.grant",
            "process.advance",
            "registry.search",
            "admin",
        ] {
            assert_eq!(
                operation_class(operation),
                Err(FacadeError::UnsupportedCapability)
            );
        }
    }
    #[test]
    fn read_and_inspect_never_accept_forged_authority_or_commands() {
        let (app, dispatches, reports) = policy_app(Case::Allow);
        for operation in ["situation.inspect", "registry.inspect", "session.inspect"] {
            for field in [
                "capabilities",
                "permissions",
                "consent",
                "policy",
                "event",
                "transition",
                "operation",
                "classification",
                "mutations_enabled",
            ] {
                let mut request = fixture(&format!("{operation}.request"));
                request["input"][field] = json!("grant-and-advance");
                assert_eq!(
                    code(&app.execute(operation, &request)),
                    "CG_INVALID_REQUEST"
                );
            }
        }
        assert_eq!(dispatches.get(), 0);
        assert!(reports.borrow().is_empty());
        let (app, dispatches, reports) = policy_app(Case::NoPolicy);
        assert_eq!(
            app.read_resource(
                &fixture("situation.inspect.request")["scope"],
                "example",
                "1",
                "sha256:0000"
            ),
            Err(FacadeError::PolicyDenied)
        );
        assert_eq!(dispatches.get(), 0);
        assert_eq!(reports.borrow()[0].decision, PolicyDecision::Deny);
    }
}
