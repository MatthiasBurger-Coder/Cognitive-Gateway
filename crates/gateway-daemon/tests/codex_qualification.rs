//! Component bridge evidence. The host supplies shared session projections;
//! it deliberately implements no task lifecycle, connector or model runtime.
use gateway_application::codex::*;
use gateway_daemon::local_mcp::{LaunchBinding, Server};
use gateway_domain::{CapabilityDefinition, ContextScopeId, PolicyDefinition, PolicyId};
use gateway_policy::{Approval, PolicyAuthority, ProcessReadiness, StepFacts};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[allow(dead_code)]
#[path = "../../gateway-application/tests/support/composition.rs"]
mod composition;
#[allow(dead_code)]
#[path = "../../gateway-application/tests/support/context_fixture.rs"]
mod context_fixture;
#[allow(dead_code)]
#[path = "../../gateway-application/tests/support/mod.rs"]
mod support;

use gateway_application::{
    resolution_application::*, resolution_composition::CompositionRules,
    resolution_snapshot::ResolutionSnapshotInput,
};
use sha2::{Digest, Sha256};

fn pinned(contract: &str, document: &Value) -> Value {
    json!({"id":"example","contract":contract,"contract_version":"1.0","revision":"1",
        "digest":format!("sha256:{:x}", Sha256::digest(document.to_string().as_bytes()))})
}
fn resolution_document(compile: bool) -> Value {
    let resolved = if compile {
        context_fixture::Fixture::new().resolved
    } else {
        DeclarativeResolutionApplication
            .resolve_plan(
                &support::with_process(support::fixture()),
                &composition::rules(),
            )
            .unwrap()
    };
    serde_json::from_str(
        &DeclarativeResolutionApplication
            .serialize_resolution(&resolved, Default::default())
            .unwrap(),
    )
    .unwrap()
}

/// Reuses existing canonical fixtures; all computation remains in real CG services.
struct CanonicalHost;
impl CodexHost for CanonicalHost {
    fn authorize(&self, _: &Call) -> Result<(), FacadeError> {
        Ok(())
    }
    fn operation_policy(&self, call: &Call) -> Result<OperationPolicy, FacadeError> {
        ProjectionHost {
            projection: Value::Null,
            consent: true,
            calls: Arc::new(AtomicUsize::new(0)),
        }
        .operation_policy(call)
    }
    fn reference(&self, call: &Call, reference: &Value) -> Result<ReferenceRecord, FacadeError> {
        let contract = reference["contract"].as_str().unwrap();
        let document = if contract == "cg.resolution" {
            resolution_document(call.operation == "context.compile")
        } else {
            json!({})
        };
        let reference = pinned(contract, &document);
        Ok(ReferenceRecord {
            scope: call.scope.clone(),
            session: call.binding.session.clone(),
            reference: reference.clone(),
            document: document.to_string(),
            provenance: vec![
                json!({"reference":reference,"source_id":"canonical-fixture",
                "source_revision":"1","freshness":"current","sensitivity":"NORMAL","lineage":[]}),
            ],
        })
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
    fn compile(&self, _: &Call, _: &[Value]) -> Result<CompileCommand, FacadeError> {
        let fixture = context_fixture::Fixture::new();
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
    fn project(
        &self,
        _: &Call,
        contract: &str,
        document: Value,
    ) -> Result<Projection, FacadeError> {
        Ok(Projection {
            source: json!({"kind":"document","contract":contract,
            "contract_version":"1.0","document":document}),
            explainability: vec![],
            evidence: vec![],
            provenance: vec![
                json!({"reference":pinned(contract, &document),"source_id":"canonical-fixture",
                "source_revision":"1","freshness":"current","sensitivity":"NORMAL","lineage":[]}),
            ],
        })
    }
}

struct ProjectionHost {
    projection: Value,
    consent: bool,
    calls: Arc<AtomicUsize>,
}
impl CodexHost for ProjectionHost {
    fn authorize(&self, _: &Call) -> Result<(), FacadeError> {
        Ok(())
    }
    fn operation_policy(&self, call: &Call) -> Result<OperationPolicy, FacadeError> {
        let id = operation_capability(&call.operation)?;
        Ok(OperationPolicy {
            authority: PolicyAuthority {
                policies: vec![
                    PolicyDefinition::new(
                        PolicyId::new("qualification").unwrap(),
                        "Trusted synthetic fixture authority",
                        [id.clone()],
                    )
                    .unwrap(),
                ],
                capabilities: [(
                    id.clone(),
                    CapabilityDefinition::new(
                        id.clone(),
                        operation_class(&call.operation)?.capability_class(),
                    ),
                )]
                .into(),
                ..Default::default()
            },
            facts: StepFacts {
                authorizations: [(id.clone(), Approval::Granted)].into(),
                consents: if self.consent {
                    [(id, Approval::Granted)].into()
                } else {
                    Default::default()
                },
                ..Default::default()
            },
            process: ProcessReadiness::NotApplicable,
            operating_mode: call.operating_mode,
            execution_profile: call.execution_profile,
            mutations_enabled: true,
        })
    }
    fn session_owner(&self, call: &Call, _: &str) -> Result<ScopeBinding, FacadeError> {
        Ok(call.binding.clone())
    }
    fn session(&self, call: &Call) -> Result<Value, FacadeError> {
        if let Some(runtime) = &call.runtime {
            runtime.check()?;
        }
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.projection.clone())
    }
}

fn request(operation: &str) -> Value {
    serde_json::from_str(
        &std::fs::read_to_string(format!(
            "{}/../../tests/fixtures/codex-v1/{operation}.request.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap(),
    )
    .unwrap()
}
fn server(projection: Value, consent: bool, calls: Arc<AtomicUsize>) -> Server {
    let facade = CodexFacade::with_binding(
        ScopeBinding {
            scope: request("session.start")["scope"].clone(),
            canonical_scope: ContextScopeId::new("qualification").unwrap(),
            mapping_revision: "1".into(),
            session: SessionContext {
                principal: "operator".into(),
                session_id: "client-session".into(),
                connection_id: "binding-example".into(),
            },
        },
        ProjectionHost {
            projection,
            consent,
            calls,
        },
    )
    .unwrap();
    ready(Box::new(facade))
}
fn ready(application: Box<dyn CodexApplicationPort + Send>) -> Server {
    let mut server = Server::with_application(
        LaunchBinding::new(
            "codex",
            "1.0",
            "operator",
            "workspace-example",
            "project-example",
            "binding-example",
        )
        .unwrap(),
        application,
    );
    let initialized = server
        .handle(
            json!({"jsonrpc":"2.0","id":1,"method":"initialize",
        "params":{"protocolVersion":"2025-11-25","capabilities":{},
        "clientInfo":{"name":"codex","version":"1.0"}}})
            .to_string()
            .as_bytes(),
        )
        .unwrap();
    assert!(initialized.get("result").is_some());
    assert!(
        server
            .handle(br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .is_none()
    );
    server
}
fn invoke(server: &mut Server, operation: &str, id: usize) -> Value {
    invoke_request(server, request(operation), id)
}
fn invoke_request(server: &mut Server, request: Value, id: usize) -> Value {
    let operation = request["operation"].as_str().unwrap();
    server
        .handle(
            json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
        "params":{"name":format!("cg_{}_v1", operation.replace('.', "_")),
        "arguments":request}})
            .to_string()
            .as_bytes(),
        )
        .unwrap()["result"]
        .clone()
}

#[test]
fn canonical_inspect_resolve_explain_and_context_are_deterministic_through_mcp() {
    let facade = CodexFacade::with_binding(
        ScopeBinding {
            scope: request("session.start")["scope"].clone(),
            canonical_scope: ContextScopeId::new("project-a").unwrap(),
            mapping_revision: "1".into(),
            session: SessionContext {
                principal: "operator".into(),
                session_id: "client-session".into(),
                connection_id: "binding-example".into(),
            },
        },
        CanonicalHost,
    )
    .unwrap();
    let mut server = ready(Box::new(facade));
    for (index, operation) in [
        "situation.inspect",
        "capabilities.resolve",
        "state.explain",
        "context.compile",
    ]
    .iter()
    .enumerate()
    {
        let mut request = if *operation == "situation.inspect" {
            request("inline.situation.inspect")
        } else {
            request(operation)
        };
        if *operation != "situation.inspect" {
            request["execution"]["operating_mode"] = json!("HARDENING");
            for reference in request["input"].as_object_mut().unwrap().values_mut() {
                if reference.is_object() && reference.get("contract").is_some() {
                    let document = if reference["contract"] == "cg.resolution" {
                        resolution_document(*operation == "context.compile")
                    } else {
                        json!({})
                    };
                    *reference = pinned(reference["contract"].as_str().unwrap(), &document);
                }
            }
        }
        if *operation == "context.compile" {
            request["input"]["candidates"] = json!([]);
            request["input"]["step_id"] = json!(
                context_fixture::Fixture::new()
                    .projection
                    .mapping
                    .step
                    .as_str()
            );
        }
        let first = invoke_request(&mut server, request.clone(), 2 + index * 2);
        assert_eq!(
            first["structuredContent"]["status"], "ok",
            "{operation}: {first}"
        );
        assert_eq!(invoke_request(&mut server, request, 3 + index * 2), first);
    }
}

#[test]
fn shared_host_start_status_pause_and_cancellation_projections_cross_the_bridge() {
    let scenarios: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/codex-qualification/session-projections.json"
    ))
    .unwrap();
    for scenario in scenarios.as_array().unwrap() {
        let operation = scenario["operation"].as_str().unwrap();
        let projection = scenario["projection"].clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut server = server(projection.clone(), true, calls.clone());
        let result = invoke(&mut server, operation, 2);
        assert_eq!(result["structuredContent"]["status"], "ok", "{result}");
        assert_eq!(result["structuredContent"]["result"], projection);
        assert_eq!(result["isError"], false);
        assert_eq!(
            serde_json::from_str::<Value>(result["content"][0]["text"].as_str().unwrap()).unwrap(),
            result["structuredContent"]
        );
        assert_eq!(
            invoke(&mut server, "session.inspect", 3)["structuredContent"]["result"],
            projection
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}

#[test]
fn client_approval_cannot_replace_trusted_consent_or_dispatch_session_mutations() {
    let calls = Arc::new(AtomicUsize::new(0));
    let projection = json!({"kind":"session","session_id":"session-example","revision":1,
        "status":"running","pending":[],"verified_final_result":null});
    let mut server = server(projection, false, calls.clone());
    for (index, operation) in [
        "session.start",
        "session.approve",
        "session.clarify",
        "session.cancel",
    ]
    .iter()
    .enumerate()
    {
        let result = invoke(&mut server, operation, index + 2);
        assert_eq!(
            result["structuredContent"]["diagnostics"][0]["code"],
            "CG_CONSENT_REQUIRED"
        );
        assert_eq!(result["isError"], true);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
