use super::*;
use std::rc::Rc;

pub(super) fn binding() -> ScopeBinding {
    ScopeBinding {
        scope: fixture("situation.inspect.request")["scope"].clone(),
        canonical_scope: ContextScopeId::new("project-a").unwrap(),
        mapping_revision: "1".into(),
        session: SessionContext {
            principal: "operator".into(),
            session_id: "client-session".into(),
            connection_id: "binding-example".into(),
        },
    }
}
struct SecurityHost {
    payload: Value,
    calls: Rc<Cell<usize>>,
    reference_document: Option<Value>,
}
impl CodexHost for SecurityHost {
    fn operation_policy(&self, call: &Call) -> Result<OperationPolicy, FacadeError> {
        Ok(test_policy(call))
    }

    fn authorize(&self, _: &Call) -> Result<(), FacadeError> {
        self.calls.set(self.calls.get() + 1);
        Ok(())
    }
    fn project(&self, _: &Call, _: &str, _: Value) -> Result<Projection, FacadeError> {
        Ok(Projection {
            source: self.payload["source"].clone(),
            explainability: self.payload["explainability"].as_array().unwrap().clone(),
            evidence: vec![],
            provenance: self.payload["provenance"].as_array().unwrap().clone(),
        })
    }
    fn reference(&self, call: &Call, reference: &Value) -> Result<ReferenceRecord, FacadeError> {
        Ok(ReferenceRecord {
            scope: call.scope.clone(),
            session: call.binding.session.clone(),
            reference: reference.clone(),
            document: self.reference_document.as_ref().unwrap().to_string(),
            provenance: vec![
                json!({"reference":reference,"source_id":"source","source_revision":"1","freshness":"current","sensitivity":"NORMAL","lineage":[]}),
            ],
        })
    }
}
fn projection() -> Value {
    let document =
        fixture("inline.situation.inspect.request")["input"]["situation"]["document"].clone();
    json!({"source":{"kind":"document","contract":"cg.situation","contract_version":"1.0","document":document},
        "explainability":[],"provenance":[]})
}

#[test]
fn injected_credentials_never_reach_authorization_or_correlated_errors() {
    let calls = Rc::new(Cell::new(0));
    let app = CodexFacade::with_binding(
        binding(),
        SecurityHost {
            payload: projection(),
            calls: calls.clone(),
            reference_document: None,
        },
    )
    .unwrap();
    for pointer in [
        "/correlation/request_id",
        "/scope/project_id",
        "/input/situation/document/description",
    ] {
        let mut request = fixture("inline.situation.inspect.request");
        // Some injections are also schema-invalid; none may be echoed or dispatched.
        if let Some(value) = request.pointer_mut(pointer) {
            *value = json!("sk-proj-FAKE_CREDENTIAL_0123456789");
        } else {
            request["input"]["situation"]["document"]["api_key"] = json!("OPAQUE_FAKE_CREDENTIAL");
        }
        let response = app.execute("situation.inspect", &request);
        assert_eq!(code(&response), "CG_SENSITIVITY_DENIED");
        assert!(response["scope"].is_null());
        assert!(response["correlation"].is_null());
        assert!(!response.to_string().contains("FAKE_CREDENTIAL"));
    }
    assert_eq!(calls.get(), 0);
}

#[test]
fn host_projection_credentials_and_classified_payloads_are_refused() {
    let request = fixture("inline.situation.inspect.request");
    for location in [
        "document",
        "explanation",
        "provenance",
        "secret",
        "reference-only",
    ] {
        let mut payload = projection();
        match location {
            "document" => {
                payload["source"]["document"]["api_key"] = json!("OPAQUE_FAKE_CREDENTIAL")
            }
            "explanation" => {
                payload["explainability"] = json!([pinned("cg.resolution-trace", &json!({}))])
            }
            "provenance" => {
                payload["provenance"] = json!([{"source_id":"sk-proj-FAKE_CREDENTIAL_0123456789"}])
            }
            "secret" => payload["source"]["document"]["sensitivity"] = json!("SECRET"),
            _ => payload["source"]["document"]["reference_only"] = json!(true),
        }
        if location == "explanation" {
            payload["explainability"][0]["id"] = json!("sk-proj-FAKE_CREDENTIAL_0123456789");
        }
        let app = CodexFacade::with_binding(
            binding(),
            SecurityHost {
                payload,
                calls: Rc::new(Cell::new(0)),
                reference_document: None,
            },
        )
        .unwrap();
        let response = app.execute("situation.inspect", &request);
        assert_eq!(code(&response), "CG_SENSITIVITY_DENIED");
        assert!(response["result"].is_null());
        assert!(!response.to_string().contains("FAKE_CREDENTIAL"));
        assert_eq!(response["correlation"], request["correlation"]);
    }
    // Only metadata is disclosed for a secret reference, never its document.
    let mut payload = projection();
    let reference = pinned("cg.situation", &payload["source"]["document"]);
    payload["source"] = json!({"kind":"reference","reference":reference});
    payload["provenance"] = json!([{"reference":reference,"source_id":"source","source_revision":"1","freshness":"current","sensitivity":"SECRET","lineage":[]}]);
    let app = CodexFacade::with_binding(
        binding(),
        SecurityHost {
            payload,
            calls: Rc::new(Cell::new(0)),
            reference_document: None,
        },
    )
    .unwrap();
    let response = app.execute("situation.inspect", &request);
    assert_eq!(response["status"], "ok");
    assert_eq!(response["result"]["canonical_result"]["kind"], "reference");
    assert!(
        response["result"]["canonical_result"]
            .get("document")
            .is_none()
    );
}

#[test]
fn reference_credentials_are_refused_before_canonical_parsing() {
    for document in [
        json!({"api_key":"OPAQUE_FAKE_CREDENTIAL"}),
        json!({"sensitivity":"SECRET","text":"OPAQUE"}),
        json!({"reference_only":true,"text":"OPAQUE"}),
    ] {
        let reference = pinned("cg.situation", &document);
        let mut request = fixture("situation.inspect.request");
        request["input"]["situation"]["reference"] = reference;
        let app = CodexFacade::with_binding(
            binding(),
            SecurityHost {
                payload: projection(),
                calls: Rc::new(Cell::new(0)),
                reference_document: Some(document),
            },
        )
        .unwrap();
        assert_eq!(
            code(&app.execute("situation.inspect", &request)),
            "CG_SENSITIVITY_DENIED"
        );
    }
    let mut poisoned = binding();
    poisoned.session.principal = "sk-proj-FAKE_CREDENTIAL_0123456789".into();
    assert!(CodexFacade::with_binding(poisoned, UnavailableHost).is_err());
    assert_eq!(
        binding().cache_key("situation.inspect", &[json!({"api_key":"opaque"})], &[]),
        Err(FacadeError::SensitivityDenied)
    );
    let app = CodexFacade::with_binding(binding(), UnavailableHost).unwrap();
    assert_eq!(
        app.read_resource(
            &binding().scope,
            "sk-proj-FAKE_CREDENTIAL_0123456789",
            "1",
            "sha256:bad"
        ),
        Err(FacadeError::SensitivityDenied)
    );
}
