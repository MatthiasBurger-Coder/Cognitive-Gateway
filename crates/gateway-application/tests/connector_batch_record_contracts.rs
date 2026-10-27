use gateway_application::sessions::*;
use gateway_domain::{
    ContentDigest, ContextScopeId, PlanStepId, ReferenceId, SchemaVersion, TaskId,
    prompt_ir::InvocationIrIdentity,
};
use serde_json::{Value, json};

fn binding() -> ConnectorStepBinding {
    ConnectorStepBinding {
        owner: OwnerBinding {
            principal: PrincipalId::new("operator").unwrap(),
            workspace: WorkspaceId::new("workspace").unwrap(),
            project: ProjectId::new("project").unwrap(),
            binding: BindingId::new("binding").unwrap(),
            client_owner: ClientOwnerId::new("client-owner").unwrap(),
        },
        scope: ContextScopeId::new("project").unwrap(),
        session: SessionId::new("session").unwrap(),
        run: RunId::new("run").unwrap(),
        revision: Revision::new(5).unwrap(),
        task: TaskId::new("task").unwrap(),
        step: PlanStepId::new("step").unwrap(),
        dispatch: DispatchId::new("dispatch").unwrap(),
        invocation: InvocationIrIdentity {
            schema_version: SchemaVersion::V1,
            invocation_id: ReferenceId::new("invocation").unwrap(),
            invocation_digest: ContentDigest::new("a".repeat(64)).unwrap(),
            handoff_digest: ContentDigest::new("b".repeat(64)).unwrap(),
            tools_digest: ContentDigest::new("c".repeat(64)).unwrap(),
            ir_digest: ContentDigest::new("d".repeat(64)).unwrap(),
        },
    }
}

fn valid_batch(binding: &ConnectorStepBinding) -> Value {
    json!({
        "contract": "cg.connector-proposal-batch-result",
        "contract_version": "1.0",
        "binding": binding,
        "payload": {
            "invocation": binding.invocation.invocation_digest.as_str(),
            "all_calls_returned": true,
            "effect": "acknowledged",
            "results": [{
                "ordinal": 0,
                "request_digest": "e".repeat(64),
                "capability": "connector.mutate",
                "operation_class": "mutate",
                "inventory_revision": "f".repeat(64),
                "execution": "execution-1",
                "correlation": "correlation-1",
                "observation": {"effect": "acknowledged"}
            }],
            "failure": null
        }
    })
}

fn encode(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}

#[test]
fn persisted_batch_identity_parser_accepts_only_consistent_bounded_records() {
    let expected = binding();
    let valid = encode(&valid_batch(&expected));
    let identity = proposal_request_identity_from_batch_record(&valid, &expected, 0).unwrap();
    assert_eq!(identity.digest.as_str(), "e".repeat(64));
    assert_eq!(identity.execution, "execution-1");
    assert_eq!(identity.correlation, "correlation-1");
    assert_eq!(
        mutation_request_digest_from_batch_record(&valid, &expected, 0).unwrap(),
        identity.digest
    );

    assert_eq!(
        proposal_request_identity_from_batch_record(&[], &expected, 0),
        Err(SessionError::LimitExceeded)
    );
    assert_eq!(
        proposal_request_identity_from_batch_record(
            &vec![b'x'; MAX_CONNECTOR_RECORD_BYTES + 1],
            &expected,
            0
        ),
        Err(SessionError::LimitExceeded)
    );
    assert_eq!(
        proposal_request_identity_from_batch_record(b"not-json", &expected, 0),
        Err(SessionError::InvalidInteraction)
    );

    let mut invalid = vec![];
    let mut value = valid_batch(&expected);
    value["contract"] = json!("other-contract");
    invalid.push(value);
    let mut value = valid_batch(&expected);
    value["contract_version"] = json!("2.0");
    invalid.push(value);
    let mut value = valid_batch(&expected);
    value["binding"]["dispatch"] = json!("different-dispatch");
    invalid.push(value);
    let mut value = valid_batch(&expected);
    value["payload"]["invocation"] = json!("f".repeat(64));
    invalid.push(value);
    let mut value = valid_batch(&expected);
    value["payload"]["effect"] = json!("impossible");
    invalid.push(value);
    let mut value = valid_batch(&expected);
    value["payload"]["all_calls_returned"] = json!(false);
    invalid.push(value);
    let mut value = valid_batch(&expected);
    value["payload"]["failure"] = json!({"code":"denied"});
    invalid.push(value);
    let mut value = valid_batch(&expected);
    value["payload"]["results"][0]["ordinal"] = json!(1);
    invalid.push(value);
    let mut value = valid_batch(&expected);
    value["payload"]["results"][0]["operation_class"] = json!("execute");
    invalid.push(value);
    let mut value = valid_batch(&expected);
    value["payload"]["results"][0]["capability"] = json!("");
    invalid.push(value);
    let mut value = valid_batch(&expected);
    value["payload"]["results"][0]["inventory_revision"] = json!("");
    invalid.push(value);
    let mut value = valid_batch(&expected);
    value["payload"]["results"][0]["execution"] = json!("");
    invalid.push(value);
    let mut value = valid_batch(&expected);
    value["payload"]["results"][0]["correlation"] = json!("");
    invalid.push(value);
    let mut value = valid_batch(&expected);
    value["payload"]["results"][0]["request_digest"] = json!("bad-digest");
    invalid.push(value);
    let mut value = valid_batch(&expected);
    value["payload"]["results"][0]["observation"] = Value::Null;
    invalid.push(value);
    for value in invalid {
        assert_eq!(
            proposal_request_identity_from_batch_record(&encode(&value), &expected, 0),
            Err(SessionError::InvalidInteraction)
        );
    }

    let mut too_many = valid_batch(&expected);
    let first = too_many["payload"]["results"][0].clone();
    too_many["payload"]["results"] = Value::Array(
        (0..=MAX_CONNECTOR_PROPOSALS_PER_STEP)
            .map(|ordinal| {
                let mut result = first.clone();
                result["ordinal"] = json!(ordinal);
                result
            })
            .collect(),
    );
    assert_eq!(
        proposal_request_identity_from_batch_record(&encode(&too_many), &expected, 0),
        Err(SessionError::InvalidInteraction)
    );

    let mut unknown = valid_batch(&expected);
    unknown["unexpected"] = json!(true);
    assert_eq!(
        proposal_request_identity_from_batch_record(&encode(&unknown), &expected, 0),
        Err(SessionError::InvalidInteraction)
    );

    assert_eq!(
        proposal_request_identity_from_batch_record(&valid, &expected, 1),
        Err(SessionError::InvalidInteraction)
    );
}
