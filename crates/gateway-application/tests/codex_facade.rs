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
    CodexFacade::new(
        fixture("situation.inspect.request")["scope"].clone(),
        ContextScopeId::new("project-a").unwrap(),
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
    assert_eq!(response["explainability"].as_array().unwrap().len(), 1);
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
