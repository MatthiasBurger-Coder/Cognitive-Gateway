#[path = "../../../tests/fixtures/prompt-rendering-v1/support.rs"]
mod support;
use gateway_application::{
    connector_policy::{
        Cg09ConnectorAdmission, ConnectorPolicyEvaluation, ConnectorPolicySnapshot,
        ConnectorPolicySource,
    },
    connector_proposals::*,
    connectors::*,
    output_verification::*,
    prompt_ir::*,
    prompt_rendering::ToolDefinition,
};
use gateway_domain::{output_verification::*, prompt_ir::*, semantic_output::*, *};
use gateway_policy::{Approval, PolicyAuthority, ProcessReadiness, StepFacts};
use serde_json::{Value, json};
use std::sync::atomic::Ordering;
use std::{cell::Cell, collections::BTreeMap, sync::Mutex};
use support::{model::pin, *};
// Synthetic trusted store: authenticates the entire expected IR, including the
// complete producer handoff. State changes simulate independent host revocation.
struct Host {
    expected: Value,
    error: Cell<Option<InvocationIrError>>,
    calls: Cell<usize>,
    outcome: Cell<CheckOutcome>,
    fault: Cell<u8>,
}
impl SemanticReferenceValidator for Host {
    fn validate(
        &self,
        _: &SemanticTaskData,
        _: &SemanticReferenceBinding,
    ) -> Result<(), ValidationError> {
        if self.error.get() == Some(InvocationIrError::InvalidBinding) {
            Err(ValidationError::InvalidStateCombination {
                reason: "stale reference",
            })
        } else {
            Ok(())
        }
    }
}
impl InvocationAdmissionPort for Host {
    fn validate_current(&self, ir: &InvocationIr) -> Result<(), InvocationIrError> {
        self.calls.set(self.calls.get() + 1);
        if let Some(error) = self.error.get() {
            return Err(error);
        }
        if self.expected != json!(ir) {
            return Err(InvocationIrError::StaleInput);
        }
        Ok(())
    }
}
// First capture is explicitly a fixture-only authority, never production admission.
struct Capture;
impl SemanticReferenceValidator for Capture {
    fn validate(
        &self,
        _: &SemanticTaskData,
        _: &SemanticReferenceBinding,
    ) -> Result<(), ValidationError> {
        Ok(())
    }
}
impl InvocationAdmissionPort for Capture {
    fn validate_current(&self, _: &InvocationIr) -> Result<(), InvocationIrError> {
        Ok(())
    }
}
fn fixture() -> (InvocationIr, Host) {
    let h = handoff();
    let p = prepared(&h, 100_000, true);
    let tools = p
        .invocation()
        .data()
        .permitted_capabilities
        .iter()
        .map(|(id, pin)| {
            (
                id.clone(),
                ToolDefinition {
                    binding: pin.clone(),
                    description: NonEmptyText::new("Inspect").unwrap(),
                    input: gateway_domain::semantic_output::OutputType::String {},
                    connector_inventory: None,
                },
            )
        })
        .collect();
    let ir = admit_invocation_ir(SchemaVersion::V1, h, p, tools, &Capture).unwrap();
    let host = Host {
        expected: json!(ir),
        error: Cell::new(None),
        calls: Cell::new(0),
        outcome: Cell::new(CheckOutcome::Passed),
        fault: Cell::new(0),
    };
    (ir, host)
}
impl OutputVerificationPort for Host {
    fn resolve_reference(
        &self,
        _: &InvocationIr,
        reference: &OutputReference,
    ) -> Result<(), VerificationError> {
        if reference.binding != pin("entity") {
            return Err(VerificationError::UnsupportedReference);
        }
        Ok(())
    }
    fn evaluate(
        &self,
        ir: &InvocationIr,
        output: &ParsedOutput,
    ) -> Result<ConstraintReport, VerificationError> {
        if self.fault.get() == 1 {
            return Err(VerificationError::VerificationUnavailable);
        }
        let mut binding = output.binding();
        if self.fault.get() == 2 {
            binding.output_digest = context_content_digest(b"old output");
        }
        if self.fault.get() == 3 {
            self.error.set(Some(InvocationIrError::StaleInput));
        }
        let mut checks: BTreeMap<_, _> = ir
            .prompt()
            .handoff()
            .task()
            .data()
            .verification_contract
            .checks
            .iter()
            .filter(|c| **c != SemanticVerificationCheck::OutputSchemaValid)
            .map(|c| (*c, CheckOutcome::Passed))
            .collect();
        match self.fault.get() {
            4 => checks.clear(),
            5 => {
                checks.insert(
                    SemanticVerificationCheck::OutputSchemaValid,
                    CheckOutcome::Passed,
                );
            }
            6 => {
                for v in checks.values_mut() {
                    *v = CheckOutcome::Failed;
                }
            }
            7 => {
                for v in checks.values_mut() {
                    *v = CheckOutcome::Indeterminate;
                }
            }
            _ => (),
        }
        Ok(ConstraintReport {
            binding,
            policy: self.outcome.get(),
            process: if self.fault.get() == 9 {
                CheckOutcome::Failed
            } else {
                CheckOutcome::Passed
            },
            task_completeness: if self.fault.get() == 8 {
                CheckOutcome::Failed
            } else {
                CheckOutcome::Passed
            },
            semantic_output: if self.fault.get() == 10 {
                CheckOutcome::Failed
            } else {
                CheckOutcome::Passed
            },
            checks,
        })
    }
}
fn wire(ir: &InvocationIr) -> Value {
    json!({"schema_version":"1.0", "invocation":ir.identity(), "result":"entity", "references":[], "tools":[]})
}
fn parse(value: &Value) -> ParsedOutput {
    parse_output(&serde_json::to_vec(value).unwrap()).unwrap()
}

#[test]
fn retained_output_preserves_exact_owned_bytes_instead_of_reencoding_fields() {
    let (ir, _) = fixture();
    let mut raw = serde_json::to_vec_pretty(&wire(&ir)).unwrap();
    raw.extend_from_slice(b"\n");
    let parsed = parse_output(&raw).unwrap();
    assert_eq!(parsed.raw_bytes(), raw);
    assert_eq!(parsed.binding().output_digest, context_content_digest(&raw));
    let reencoded = serde_json::to_vec(&serde_json::to_value(parsed.data()).unwrap()).unwrap();
    assert_ne!(parsed.raw_bytes(), reencoded);
    let cloned = parsed.clone();
    assert!(std::ptr::eq(parsed.raw_bytes(), cloned.raw_bytes()));
    raw[0] = b'!';
    assert_ne!(parsed.raw_bytes(), raw);
    assert!(parse_output(&raw).is_err());
    let reloaded = parse_output(parsed.raw_bytes()).unwrap();
    assert_eq!(reloaded.binding(), parsed.binding());
    assert_eq!(
        serde_json::to_value(reloaded.data()).unwrap(),
        serde_json::to_value(parsed.data()).unwrap()
    );
}

fn denied(ir: &InvocationIr, host: &Host, value: &Value, error: VerificationError) {
    let parsed = parse(value);
    let binding = parsed.binding();
    let record = verify_output(ir, parsed, host, vec![CheckOutcome::Passed]).unwrap_err();
    assert_eq!(record.diagnostic, Some(error));
    assert_eq!(record.binding, binding);
    assert_eq!(record.admitted_invocation, ir.identity());
    assert_eq!(record.validity, ResultValidity::VerificationFailed);
}
#[test]
fn accepted_output_is_exactly_correlated_and_critics_are_advisory() {
    let (ir, host) = fixture();
    let mut value = wire(&ir);
    value["references"] = json!([{"pointer":"", "binding":pin("entity")}]);
    value["tools"] = json!([{"capability":ir.prompt().tools().values().next().unwrap().binding,"arguments":"inspect"}]);
    let output = gateway_application::output_parsing::parse_invocation_output(
        &ir,
        &serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    let binding = output.binding();
    assert_eq!(
        binding.output_digest,
        context_content_digest(&serde_json::to_vec(&value).unwrap())
    );
    for critics in [
        vec![],
        vec![CheckOutcome::Failed],
        vec![CheckOutcome::Indeterminate],
    ] {
        let verified = verify_output(&ir, output.clone(), &host, critics.clone()).unwrap();
        assert_eq!(verified.output().data().result, "entity");
        assert_eq!(verified.record().binding, binding);
        assert_eq!(verified.record().critics, critics);
        assert_eq!(verified.record().validity, ResultValidity::Valid);
        assert_eq!(verified.record().diagnostic, None);
        assert_eq!(json!(verified.record())["schema_version"], "1.0");
    }
    host.outcome.set(CheckOutcome::Failed);
    denied(&ir, &host, &value, VerificationError::ConstraintViolation);
    host.outcome.set(CheckOutcome::Indeterminate);
    denied(
        &ir,
        &host,
        &value,
        VerificationError::VerificationUnavailable,
    );
}
#[test]
fn stale_and_unavailable_authority_and_constraints_fail_closed() {
    let (ir, host) = fixture();
    for (error, expected) in [
        (
            InvocationIrError::StaleInput,
            VerificationError::StaleContext,
        ),
        (
            InvocationIrError::AuthorityDenied,
            VerificationError::AuthorityDenied,
        ),
        (
            InvocationIrError::AuthenticationUnavailable,
            VerificationError::VerificationUnavailable,
        ),
        (
            InvocationIrError::InvalidBinding,
            VerificationError::AuthorityDenied,
        ),
    ] {
        host.error.set(Some(error));
        denied(&ir, &host, &wire(&ir), expected);
    }
    host.error.set(None);
    for (fault, expected) in [
        (1, VerificationError::VerificationUnavailable),
        (2, VerificationError::CorrelationMismatch),
        (3, VerificationError::StaleContext),
        (4, VerificationError::SemanticVerificationFailed),
        (5, VerificationError::SemanticVerificationFailed),
        (6, VerificationError::SemanticVerificationFailed),
        (7, VerificationError::SemanticVerificationFailed),
        (8, VerificationError::ConstraintViolation),
        (9, VerificationError::ConstraintViolation),
        (10, VerificationError::ConstraintViolation),
    ] {
        host.fault.set(fault);
        denied(&ir, &host, &wire(&ir), expected);
        host.error.set(None);
    }
}
#[test]
fn schema_and_every_identity_axis_must_match() {
    let (ir, host) = fixture();
    for field in [
        "invocation_id",
        "invocation_digest",
        "handoff_digest",
        "tools_digest",
        "ir_digest",
    ] {
        let mut v = wire(&ir);
        v["invocation"][field] = if field.ends_with("digest") {
            json!(context_content_digest(b"changed"))
        } else {
            json!("changed")
        };
        denied(&ir, &host, &v, VerificationError::CorrelationMismatch);
    }
    for result in [json!(null), json!(2), json!({"result":"entity"})] {
        let mut v = wire(&ir);
        v["result"] = result;
        denied(&ir, &host, &v, VerificationError::OutputSchemaInvalid);
    }
    // Task revision, provider provenance and scope are included in the IR digest.
    let mut changed = ir.prompt().tools().clone();
    changed.values_mut().next().unwrap().description =
        NonEmptyText::new("changed inventory").unwrap();
    let other = admit_invocation_ir(
        SchemaVersion::V1,
        ir.prompt().handoff().clone(),
        ir.prepared().clone(),
        changed,
        &Capture,
    )
    .unwrap();
    denied(
        &other,
        &host,
        &wire(&ir),
        VerificationError::CorrelationMismatch,
    );
}
#[test]
fn unsupported_references_and_tool_proposals_are_rejected() {
    let (ir, host) = fixture();
    let valid = json!({"pointer":"", "binding":pin("entity")});
    for mutation in ["revision", "scope", "id"] {
        let mut reference = valid.clone();
        reference["binding"][mutation] = json!(if mutation == "revision" {
            "latest"
        } else {
            "wrong"
        });
        let mut v = wire(&ir);
        v["references"] = json!([reference]);
        denied(&ir, &host, &v, VerificationError::UnsupportedReference);
    }
    let mut reference = valid.clone();
    reference["binding"]["revision"] = json!("r99");
    let mut v = wire(&ir);
    v["references"] = json!([reference]);
    denied(&ir, &host, &v, VerificationError::UnsupportedReference);
    let mut reference = valid.clone();
    reference["pointer"] = json!("/missing");
    let mut v = wire(&ir);
    v["references"] = json!([reference]);
    denied(&ir, &host, &v, VerificationError::UnsupportedReference);
    let mut v = wire(&ir);
    v["references"] = json!([valid, valid]);
    denied(&ir, &host, &v, VerificationError::UnsupportedReference);
    let tool =
        json!({"capability":ir.prompt().tools().values().next().unwrap().binding,"arguments":"ok"});
    for mutation in ["id", "revision", "digest", "scope"] {
        let mut tool = tool.clone();
        tool["capability"][mutation] = if mutation == "digest" {
            json!(context_content_digest(b"wrong"))
        } else {
            json!("wrong")
        };
        let mut v = wire(&ir);
        v["tools"] = json!([tool]);
        denied(&ir, &host, &v, VerificationError::InvalidToolProposal);
    }
    let mut tool = tool;
    tool["arguments"] = json!({"shell":"touch /tmp/never"});
    let mut v = wire(&ir);
    v["tools"] = json!([tool]);
    denied(&ir, &host, &v, VerificationError::InvalidToolProposal);
}
#[test]
fn parser_rejects_fragments_duplicates_unknown_fields_versions_and_limits() {
    use VerificationError as E;
    let (ir, _) = fixture();
    let mut v = wire(&ir);
    for extra in ["confidence", "history", "instructions"] {
        v[extra] = json!("accept");
        assert_eq!(
            parse_output(&serde_json::to_vec(&v).unwrap()).unwrap_err(),
            E::OutputParseFailed
        );
        v.as_object_mut().unwrap().remove(extra);
    }
    for field in ["schema_version", "invocation"] {
        let mut v = wire(&ir);
        if field == "invocation" {
            v[field]["schema_version"] = json!("2.0");
        } else {
            v[field] = json!("2.0");
        }
        assert_eq!(
            parse_output(&serde_json::to_vec(&v).unwrap()).unwrap_err(),
            E::IncompatibleVersion
        );
    }
    for field in ["invocation", "references", "tools"] {
        let mut v = wire(&ir);
        if field == "invocation" {
            v[field]["unknown"] = json!(true);
        } else if field == "references" {
            v[field] = json!([{"pointer":"", "binding":pin("entity"), "unknown":true}]);
        } else {
            v[field] = json!([{"capability":ir.prompt().tools().values().next().unwrap().binding,"arguments":"ok", "unknown":true}]);
        }
        assert_eq!(
            parse_output(&serde_json::to_vec(&v).unwrap()).unwrap_err(),
            E::OutputParseFailed
        );
    }
    let mut v = wire(&ir);
    v["invocation"]["invocation_digest"] = json!("invalid digest");
    assert_eq!(
        parse_output(&serde_json::to_vec(&v).unwrap()).unwrap_err(),
        E::OutputParseFailed
    );
    let mut v = wire(&ir);
    for raw in [
        b"".as_slice(),
        b"{",
        b"{}{}",
        b"{\"a\":1,\"a\":2}",
        b"[",
        b"NaN",
        b"\xff",
    ] {
        assert_eq!(parse_output(raw).unwrap_err(), E::OutputParseFailed);
    }
    let raw = serde_json::to_vec(&wire(&ir)).unwrap();
    assert_eq!(
        parse_output(&raw[..raw.len() - 1]).unwrap_err(),
        E::OutputParseFailed
    );
    assert_eq!(
        parse_output(&vec![b' '; MAX_OUTPUT_BYTES + 1]).unwrap_err(),
        E::OutputLimitExceeded
    );
    let deep = format!("{}0{}", "[".repeat(65), "]".repeat(65));
    assert_eq!(
        parse_output(deep.as_bytes()).unwrap_err(),
        E::OutputLimitExceeded
    );
    // Exercise every JSON primitive and nested duplicate-key protection.
    v["result"] = json!([true,false,1,-2,1.25,null,[],{}, {"a":["text"]}]);
    parse(&v);
    let raw = serde_json::to_string(&wire(&ir))
        .unwrap()
        .replace("\"entity\"", "{\"a\":1,\"a\":2}");
    assert_eq!(
        parse_output(raw.as_bytes()).unwrap_err(),
        E::OutputParseFailed
    );
    let valid = serde_json::to_vec(&wire(&ir)).unwrap();
    assert_ne!(
        parse_output(&valid).unwrap().binding().output_digest,
        parse_output(&[valid.as_slice(), b" "].concat())
            .unwrap()
            .binding()
            .output_digest
    );
}
#[test]
fn diagnostics_and_wire_contracts_are_stable() {
    let errors = [
        VerificationError::OutputParseFailed,
        VerificationError::OutputLimitExceeded,
        VerificationError::IncompatibleVersion,
        VerificationError::CorrelationMismatch,
        VerificationError::OutputSchemaInvalid,
        VerificationError::UnsupportedReference,
        VerificationError::InvalidToolProposal,
        VerificationError::StaleContext,
        VerificationError::AuthorityDenied,
        VerificationError::VerificationUnavailable,
        VerificationError::ConstraintViolation,
        VerificationError::SemanticVerificationFailed,
    ];
    let expected: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/output-verification-v1/diagnostics.json"
    ))
    .unwrap();
    assert_eq!(json!(errors), expected);
    assert_eq!(
        serde_json::from_value::<Vec<VerificationError>>(expected).unwrap(),
        errors
    );
}

#[test]
fn invocation_parser_uses_complete_schema_inventory_and_fixture_results() {
    use gateway_application::output_parsing::*;
    let (ir, _) = fixture();
    let cases: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/output-parsing-v1/results.json"
    ))
    .unwrap();
    let template = serde_json::to_string(&wire(&ir)).unwrap();
    for case in cases.as_array().unwrap() {
        let raw = template.replace("\"entity\"", case["raw"].as_str().unwrap());
        let result = parse_invocation_output(&ir, raw.as_bytes());
        match case["error"].as_str() {
            None => assert_eq!(result.unwrap().data().result, "entity"),
            Some(error) => assert_eq!(json!(result.unwrap_err()), error),
        }
    }
    let mut value = wire(&ir);
    let tool = json!({"capability":ir.prompt().tools().values().next().unwrap().binding,"arguments":"inspect"});
    value["tools"] = json!([tool]);
    parse_invocation_output(&ir, &serde_json::to_vec(&value).unwrap()).unwrap();
    for field in ["id", "revision", "scope", "digest"] {
        let mut value = value.clone();
        value["tools"][0]["capability"][field] = if field == "digest" {
            json!(context_content_digest(b"changed"))
        } else {
            json!("changed")
        };
        assert_eq!(
            parse_invocation_output(&ir, &serde_json::to_vec(&value).unwrap()).unwrap_err(),
            VerificationError::InvalidToolProposal
        );
    }
    value["tools"][0]["arguments"] = json!({"command":"touch /tmp/never"});
    assert_eq!(
        parse_invocation_output(&ir, &serde_json::to_vec(&value).unwrap()).unwrap_err(),
        VerificationError::InvalidToolProposal
    );
}

#[test]
fn streams_require_completion_and_overflow_is_terminal() {
    use gateway_application::output_parsing::*;
    use gateway_domain::output_parsing::*;
    let (ir, _) = fixture();
    let mut value = wire(&ir);
    value["result"] = json!("é");
    let raw = serde_json::to_vec(&value).unwrap();
    // Single-byte chunks deliberately split the UTF-8 character.
    let mut stream = OutputStream::new(&ir);
    for byte in &raw {
        stream.push(&[*byte]).unwrap();
    }
    assert_eq!(
        stream
            .finish(OutputCompletion::Complete)
            .unwrap()
            .data()
            .result,
        "é"
    );
    let mut stream = OutputStream::new(&ir);
    stream.push(&raw).unwrap();
    assert_eq!(
        stream.finish(OutputCompletion::Interrupted).unwrap_err(),
        VerificationError::OutputParseFailed
    );
    let mut stream = OutputStream::new(&ir);
    stream.push(&raw[..raw.len() - 1]).unwrap();
    assert_eq!(
        stream.finish(OutputCompletion::Complete).unwrap_err(),
        VerificationError::OutputParseFailed
    );
    let mut padded = raw;
    padded.resize(MAX_OUTPUT_BYTES, b' ');
    let mut stream = OutputStream::new(&ir);
    stream.push(&padded).unwrap();
    stream.finish(OutputCompletion::Complete).unwrap();
    let mut stream = OutputStream::new(&ir);
    stream.push(&padded).unwrap();
    assert_eq!(
        stream.push(b" "),
        Err(VerificationError::OutputLimitExceeded)
    );
    assert_eq!(
        stream.push(b""),
        Err(VerificationError::OutputLimitExceeded)
    );
    assert_eq!(
        stream.finish(OutputCompletion::Complete).unwrap_err(),
        VerificationError::OutputLimitExceeded
    );
    assert_eq!(json!(OutputCompletion::Interrupted), "INTERRUPTED");
    assert_eq!(
        serde_json::from_value::<OutputCompletion>(json!("COMPLETE")).unwrap(),
        OutputCompletion::Complete
    );
}

#[test]
fn repairs_are_correlated_bounded_and_require_aggregate_admission() {
    use gateway_application::output_parsing::*;
    use gateway_domain::output_parsing::*;
    struct Budget {
        remaining: u8,
        calls: usize,
        expected: gateway_domain::prompt_ir::InvocationIrIdentity,
    }
    impl OutputRepairBudgetPort for Budget {
        fn reserve(
            &mut self,
            ir: &InvocationIr,
            attempt: u8,
            bytes: usize,
        ) -> Result<(), RepairError> {
            assert_eq!(ir.identity(), self.expected);
            assert!((1..=3).contains(&attempt));
            assert_eq!(bytes, 1024);
            self.calls += 1;
            if self.remaining == 0 {
                return Err(RepairError::BudgetExhausted);
            }
            self.remaining -= 1;
            Ok(())
        }
    }
    let (ir, _) = fixture();
    for (attempts, bytes) in [(0, 1024), (4, 1024), (1, 0), (1, MAX_OUTPUT_BYTES + 1)] {
        assert!(matches!(
            OutputRepairSession::new(&ir, attempts, bytes),
            Err(RepairError::InvalidBudget)
        ));
    }
    let mut budget = Budget {
        remaining: 3,
        calls: 0,
        expected: ir.identity(),
    };
    let mut session = OutputRepairSession::new(&ir, 3, 1024).unwrap();
    let mut schema = wire(&ir);
    schema["result"] = json!(null);
    let mut tool = wire(&ir);
    tool["tools"] = json!([{"capability":support::examples::pin(CapabilityId::new("absent").unwrap()),"arguments":{}}]);
    for (index, raw) in [
        b"{".to_vec(),
        serde_json::to_vec(&schema).unwrap(),
        serde_json::to_vec(&tool).unwrap(),
    ]
    .iter()
    .enumerate()
    {
        let proposal = session.propose(raw, &mut budget).unwrap();
        assert_eq!(proposal.attempt, index as u8 + 1);
        assert_eq!(proposal.failed_output.invocation, ir.identity());
        assert_eq!(
            proposal.failed_output.output_digest,
            context_content_digest(raw)
        );
        assert_eq!(json!(proposal)["schema_version"], "1.0");
        assert_eq!(proposal.max_output_bytes, 1024);
    }
    assert!(matches!(
        session.propose(b"{", &mut budget),
        Err(RepairError::BudgetExhausted)
    ));
    // Reconstructing a local controller cannot reset the host's aggregate budget.
    let mut session = OutputRepairSession::new(&ir, 1, 1024).unwrap();
    assert!(matches!(
        session.propose(b"{", &mut budget),
        Err(RepairError::BudgetExhausted)
    ));
    assert!(matches!(
        session.propose(b"{", &mut budget),
        Err(RepairError::BudgetExhausted)
    ));
    assert_eq!(budget.calls, 4);
    let mut session = OutputRepairSession::new(&ir, 1, 1024).unwrap();
    let mut wrong_identity = wire(&ir);
    wrong_identity["invocation"]["invocation_id"] = json!("wrong");
    let mut wrong_version = wire(&ir);
    wrong_version["schema_version"] = json!("2.0");
    for raw in [
        serde_json::to_vec(&wire(&ir)).unwrap(),
        serde_json::to_vec(&wrong_identity).unwrap(),
        serde_json::to_vec(&wrong_version).unwrap(),
        vec![b' '; MAX_OUTPUT_BYTES + 1],
    ] {
        assert!(matches!(
            session.propose(&raw, &mut budget),
            Err(RepairError::RepairNotPermitted)
        ));
    }
    assert_eq!(budget.calls, 4);
    for error in [
        RepairError::InvalidBudget,
        RepairError::BudgetExhausted,
        RepairError::RepairNotPermitted,
    ] {
        assert_eq!(
            serde_json::from_value::<RepairError>(json!(error)).unwrap(),
            error
        );
    }
}

#[test]
fn nested_typed_arguments_reject_unknown_null_and_coerced_fields() {
    use gateway_application::output_parsing::parse_invocation_output;
    let (original, _) = fixture();
    let mut tools = original.prompt().tools().clone();
    tools.values_mut().next().unwrap().input = OutputType::Object {
        fields: BTreeMap::from([(
            "target".into(),
            OutputField {
                required: true,
                value_type: OutputType::Object {
                    fields: BTreeMap::from([(
                        "id".into(),
                        OutputField {
                            required: true,
                            value_type: OutputType::String {},
                        },
                    )]),
                },
            },
        )]),
    };
    let ir = admit_invocation_ir(
        SchemaVersion::V1,
        original.prompt().handoff().clone(),
        original.prepared().clone(),
        tools,
        &Capture,
    )
    .unwrap();
    let mut value = wire(&ir);
    for arguments in [
        json!({"target":{"id":"entity"}}),
        json!({"target":{"id":"entity","shell":"echo injected"}}),
        json!({"target":{"id":12}}),
        json!({"target":null}),
        json!({}),
    ] {
        value["tools"] = json!([{"capability":ir.prompt().tools().values().next().unwrap().binding,"arguments":arguments}]);
        let parsed = parse_invocation_output(&ir, &serde_json::to_vec(&value).unwrap());
        if arguments == json!({"target":{"id":"entity"}}) {
            assert_eq!(parsed.unwrap().data().tools[0].arguments, arguments);
        } else {
            assert_eq!(parsed.unwrap_err(), VerificationError::InvalidToolProposal);
        }
    }
}

fn connector_fixture() -> (InvocationIr, Host) {
    connector_fixture_with_description("Connector read")
}

fn connector_fixture_with_description(description: &str) -> (InvocationIr, Host) {
    let h = handoff();
    let p = prepared(&h, 100_000, true);
    let tools = p
        .invocation()
        .data()
        .permitted_capabilities
        .iter()
        .map(|(id, pin)| {
            let input = OutputType::Object {
                fields: BTreeMap::from([
                    (
                        "query".to_owned(),
                        OutputField {
                            required: true,
                            value_type: OutputType::String {},
                        },
                    ),
                    (
                        "enabled".to_owned(),
                        OutputField {
                            required: true,
                            value_type: OutputType::Boolean {},
                        },
                    ),
                    (
                        "count".to_owned(),
                        OutputField {
                            required: true,
                            value_type: OutputType::Integer {},
                        },
                    ),
                    (
                        "mode".to_owned(),
                        OutputField {
                            required: true,
                            value_type: OutputType::String {},
                        },
                    ),
                ]),
            };
            let argument_schema_digest = gateway_domain::context_content_digest(
                &serde_json::to_vec(&input.json_schema()).unwrap(),
            );
            (
                id.clone(),
                ToolDefinition {
                    binding: pin.clone(),
                    description: NonEmptyText::new(description).unwrap(),
                    input,
                    connector_inventory: Some(
                        gateway_application::prompt_rendering::ConnectorToolInventoryBinding {
                            connector: NonEmptyText::new("fixture.connector").unwrap(),
                            scope: p.invocation().data().task.scope.clone(),
                            revision: ContentDigest::new("a".repeat(64)).unwrap(),
                            argument_schema_digest,
                        },
                    ),
                },
            )
        })
        .collect();
    let ir = admit_invocation_ir(SchemaVersion::V1, h, p, tools, &Capture).unwrap();
    let host = Host {
        expected: json!(ir),
        error: Cell::new(None),
        calls: Cell::new(0),
        outcome: Cell::new(CheckOutcome::Passed),
        fault: Cell::new(0),
    };
    (ir, host)
}
fn connector_text(value: impl Into<String>) -> NonEmptyText {
    NonEmptyText::new(value).unwrap()
}

struct ProposalBinder {
    bad_ordinal: Option<usize>,
    bind_error_ordinal: Option<usize>,
    unsupported_ordinal: Option<(usize, bool)>,
    stale: bool,
}
impl ConnectorProposalBindingPort for ProposalBinder {
    fn validate_current(&self, _: &InvocationIr) -> Result<(), ProposalError> {
        if self.stale {
            Err(ProposalError::StaleInvocation)
        } else {
            Ok(())
        }
    }
    fn bind(
        &self,
        invocation: &InvocationIr,
        proposal: &ToolCallProposal,
        ordinal: usize,
    ) -> Result<BoundConnectorProposal, ProposalError> {
        if self.bind_error_ordinal == Some(ordinal) {
            return Err(ProposalError::PolicyDenied);
        }
        let scope = invocation.prepared().invocation().data().task.scope.clone();
        let mut args = std::collections::BTreeMap::from([
            (
                connector_text("query"),
                TypedValue::string(proposal.arguments["query"].as_str().unwrap()).unwrap(),
            ),
            (
                connector_text("enabled"),
                TypedValue::Boolean(proposal.arguments["enabled"].as_bool().unwrap()),
            ),
            (
                connector_text("count"),
                TypedValue::Integer(proposal.arguments["count"].as_i64().unwrap()),
            ),
            (
                connector_text("mode"),
                TypedValue::Symbol(
                    SymbolValue::new(proposal.arguments["mode"].as_str().unwrap()).unwrap(),
                ),
            ),
        ]);
        if self.bad_ordinal == Some(ordinal) {
            args.insert(
                connector_text("query"),
                TypedValue::string("tampered").unwrap(),
            );
        }
        if let Some((bad_ordinal, is_set)) = self
            .unsupported_ordinal
            .filter(|(index, _)| *index == ordinal)
        {
            args.insert(
                connector_text("query"),
                if is_set {
                    TypedValue::Set(vec![TypedValue::Integer(1)])
                } else {
                    TypedValue::Decimal(DecimalValue::new(1, 1).unwrap())
                },
            );
            let _ = bad_ordinal;
        }
        Ok(BoundConnectorProposal {
            request: ConnectorRequest {
                target: ConnectorTarget {
                    connector: connector_text("fixture.connector"),
                    scope,
                },
                capability: proposal.capability.id.clone(),
                class: ConnectorOperationClass::Read,
                inventory_revision: ContentDigest::new("a".repeat(64)).unwrap(),
                arguments: args,
                task: invocation.prepared().invocation().data().task.id.clone(),
                step: PlanStepId::new("current-step").unwrap(),
                execution: connector_text("current-execution"),
                correlation: connector_text(format!("proposal-{ordinal}")),
            },
        })
    }
}

struct ProposalConnector {
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    fail_at: Option<usize>,
    inventory_capability: CapabilityId,
    inventory_revision: ContentDigest,
    argument_schema_digest: ContentDigest,
    invalid_observation: bool,
    operation_class: ConnectorOperationClass,
    wrong_discovery_target: bool,
}
fn proposal_connector(ir: &InvocationIr, fail_at: Option<usize>) -> ProposalConnector {
    let tool = ir.prompt().tools().values().next().unwrap();
    let schema = tool.input.json_schema();
    ProposalConnector {
        calls: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        fail_at,
        inventory_capability: ir.prompt().tools().keys().next().unwrap().clone(),
        inventory_revision: ContentDigest::new("a".repeat(64)).unwrap(),
        argument_schema_digest: gateway_domain::context_content_digest(
            &serde_json::to_vec(&schema).unwrap(),
        ),
        invalid_observation: false,
        operation_class: ConnectorOperationClass::Read,
        wrong_discovery_target: false,
    }
}

struct TestConnectorPolicySource {
    authorize: ConnectorPolicySnapshot,
    revalidate: ConnectorPolicySnapshot,
}
impl ConnectorPolicySource for TestConnectorPolicySource {
    fn admit_target(&self, _: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        Ok(())
    }
    fn authorize(
        &self,
        _: &ConnectorRequest,
        _: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        Ok(self.authorize.clone())
    }
    fn revalidate(
        &self,
        _: &ConnectorDispatch,
        _: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        Ok(self.revalidate.clone())
    }
    fn record_decision(&self, _: &ConnectorPolicyEvaluation<'_>) -> Result<(), ConnectorFailure> {
        Ok(())
    }
}

fn connector_policy_snapshot(
    capability: &CapabilityId,
    authorization: Approval,
    consent_required: bool,
    consent_receipt: Option<NonEmptyText>,
) -> ConnectorPolicySnapshot {
    let definition = CapabilityDefinition::new(capability.clone(), CapabilityClass::Inspect);
    ConnectorPolicySnapshot {
        step: PlanStepId::new("current-step").unwrap(),
        resolved: true,
        has_prerequisites: false,
        constraints: Default::default(),
        preconditions: Default::default(),
        authority: PolicyAuthority {
            policies: vec![
                PolicyDefinition::new(
                    PolicyId::new("connector-test-policy").unwrap(),
                    "test connector policy",
                    [capability.clone()],
                )
                .unwrap(),
            ],
            capabilities: BTreeMap::from([(capability.clone(), definition)]),
            ..Default::default()
        },
        facts: StepFacts {
            authorizations: BTreeMap::from([(capability.clone(), authorization)]),
            consents: BTreeMap::from([(capability.clone(), Approval::Granted)]),
            ..Default::default()
        },
        operating_mode: OperatingMode::Development,
        execution_profile: ExecutionProfile::FullPath,
        process: ProcessReadiness::Eligible,
        consent_required,
        administrative_capabilities: Default::default(),
        policy: PolicyId::new("connector-test-policy").unwrap(),
        policy_revision: ContentDigest::new("d".repeat(64)).unwrap(),
        consent_receipt,
        budget_reservation: NonEmptyText::new("test-budget-reservation").unwrap(),
        deadline: UnixTimestamp::new(10_000),
    }
}

struct Cg09GatedProposalConnector {
    admission: Cg09ConnectorAdmission<TestConnectorPolicySource>,
    transport: ProposalConnector,
    authorized: std::sync::Arc<Mutex<Vec<ConnectorRequest>>>,
}
impl ConnectorApplicationPort for Cg09GatedProposalConnector {
    fn discover(&self, target: &ConnectorTarget) -> Result<ConnectorInventory, ConnectorFailure> {
        self.transport.discover(target)
    }
    fn invoke(&self, request: &ConnectorRequest) -> Result<ConnectorObservation, ConnectorFailure> {
        let dispatch = self.admission.authorize(request, None)?;
        self.admission.revalidate(&dispatch, None)?;
        self.authorized
            .lock()
            .unwrap()
            .push(dispatch.request.clone());
        self.transport.invoke(&dispatch.request)
    }
    fn read_resource(
        &self,
        request: &ConnectorRequest,
        source: &SourceId,
    ) -> Result<ConnectorObservation, ConnectorFailure> {
        self.transport.read_resource(request, source)
    }
    fn lifecycle(
        &self,
        target: &ConnectorTarget,
        command: ConnectorLifecycleCommand,
    ) -> Result<ConnectorLifecycleState, ConnectorFailure> {
        self.transport.lifecycle(target, command)
    }
}

fn cg09_gated_connector(
    ir: &InvocationIr,
    authorize: ConnectorPolicySnapshot,
    revalidate: ConnectorPolicySnapshot,
) -> Cg09GatedProposalConnector {
    Cg09GatedProposalConnector {
        admission: Cg09ConnectorAdmission::new(TestConnectorPolicySource {
            authorize,
            revalidate,
        }),
        transport: proposal_connector(ir, None),
        authorized: std::sync::Arc::new(Mutex::new(Vec::new())),
    }
}
impl ProposalConnector {
    fn failure(request: &ConnectorRequest, effect: ConnectorEffect) -> ConnectorFailure {
        ConnectorFailure {
            code: ConnectorFailureCode::MutationUncertain,
            effect,
            correlation: request.correlation.clone(),
        }
    }
    fn observation(request: &ConnectorRequest) -> ConnectorObservation {
        let source = SourceId::new("test-source").unwrap();
        let provenance = Provenance::new(
            ProvenanceId::new("proposal.provenance").unwrap(),
            SourceKind::Tool,
            source.clone(),
            "observation",
        )
        .unwrap()
        .with_producer("test-host")
        .unwrap()
        .with_acquired_at(SourceTimestamp::new("1").unwrap());
        let observation = Observation::new(
            ObservationId::new("proposal.observation").unwrap(),
            SubjectPath::new(["connector", "status"]).unwrap(),
            TypedValue::Boolean(true),
            provenance.id().clone(),
        )
        .unwrap();
        let fact = Fact::new(
            FactId::new("proposal.fact").unwrap(),
            observation.subject().clone(),
            observation.value().clone(),
            AssertionPolarity::Affirmed,
            vec![observation.id().clone()],
        )
        .unwrap();
        let evidence = Evidence::new(
            EvidenceId::new("proposal.evidence").unwrap(),
            EvidenceKind::ToolOutput,
            "connector observation",
            EvidenceContent::inline("observed").unwrap(),
            provenance.id().clone(),
            vec![EvidenceLink::new(
                fact.id().clone(),
                EvidenceRelation::Supports,
            )],
        )
        .unwrap();
        let records = ObservationEvidenceSet::new(
            vec![provenance],
            vec![observation],
            vec![fact],
            vec![evidence],
        )
        .unwrap();
        ConnectorObservation {
            metadata: gateway_application::connector_results::ConnectorResultMetadata {
                task: request.task.clone(),
                step: request.step.clone(),
                execution: request.execution.clone(),
                object_reference: connector_text("observation"),
                source_kind: SourceKind::Tool,
                source_digest: None,
                observed_at: None,
                authenticated_source: connector_text("test-host"),
                quality: QualityMetadata::new(
                    TrustClass::ObservedEvidence,
                    SensitivityClass::Public,
                    Confidence::Unknown,
                    FreshnessStatus::Unknown,
                    Uncertainty::None,
                ),
                effect: ConnectorEffect::Acknowledged,
                partial: false,
                verifies: None,
                non_authoritative_raw: None,
            },
            target: request.target.clone(),
            inventory_revision: request.inventory_revision.clone(),
            correlation: request.correlation.clone(),
            source: SourceId::new("test-source").unwrap(),
            source_revision: None,
            content_digest: ContentDigest::new("b".repeat(64)).unwrap(),
            acquired_at: UnixTimestamp::new(1),
            records,
            basis: ConnectorObservationBasis::ProviderAcknowledgement,
        }
    }
}
impl ConnectorApplicationPort for ProposalConnector {
    fn discover(&self, target: &ConnectorTarget) -> Result<ConnectorInventory, ConnectorFailure> {
        let mut discovered_target = target.clone();
        if self.wrong_discovery_target {
            discovered_target.scope = ContextScopeId::new("foreign-project").unwrap();
        }
        Ok(ConnectorInventory {
            target: discovered_target,
            revision: self.inventory_revision.clone(),
            capabilities: vec![ConnectorCapability {
                capability: self.inventory_capability.clone(),
                class: self.operation_class,
                argument_schema_digest: self.argument_schema_digest.clone(),
                result_schema_digest: ContentDigest::new("c".repeat(64)).unwrap(),
            }],
            resources: vec![],
        })
    }
    fn invoke(&self, request: &ConnectorRequest) -> Result<ConnectorObservation, ConnectorFailure> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if self.fail_at == Some(call) {
            Err(Self::failure(request, ConnectorEffect::Unknown))
        } else {
            let mut observation = Self::observation(request);
            if self.invalid_observation {
                observation.metadata.task = TaskId::new("foreign-task").unwrap();
            }
            Ok(observation)
        }
    }
    fn read_resource(
        &self,
        request: &ConnectorRequest,
        _: &SourceId,
    ) -> Result<ConnectorObservation, ConnectorFailure> {
        Err(Self::failure(request, ConnectorEffect::NotDispatched))
    }
    fn lifecycle(
        &self,
        _: &ConnectorTarget,
        _: ConnectorLifecycleCommand,
    ) -> Result<ConnectorLifecycleState, ConnectorFailure> {
        unreachable!()
    }
}

fn verified_tools(ir: &InvocationIr, host: &Host) -> VerifiedOutput {
    let mut value = wire(ir);
    let capability = ir.prompt().tools().values().next().unwrap().binding.clone();
    value["tools"] = json!([
        {"capability": capability, "arguments":{"query":"first","enabled":true,"count":1,"mode":"fast"}},
        {"capability": capability, "arguments":{"query":"second","enabled":false,"count":2,"mode":"safe"}}
    ]);
    let parsed = gateway_application::output_parsing::parse_invocation_output(
        ir,
        &serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    verify_output(ir, parsed, host, vec![]).unwrap()
}

struct ProposalSessionBinding;
impl ConnectorSessionBindingPort for ProposalSessionBinding {
    fn current_binding(
        &self,
        invocation: &InvocationIr,
    ) -> Result<ConnectorSessionBinding, ProposalError> {
        Ok(ConnectorSessionBinding {
            target: ConnectorTarget {
                connector: connector_text("fixture.connector"),
                scope: invocation.prepared().invocation().data().task.scope.clone(),
            },
            step: PlanStepId::new("current-step").unwrap(),
            execution: connector_text("current-execution"),
        })
    }
}

struct WrongScopeProposalSessionBinding;
impl ConnectorSessionBindingPort for WrongScopeProposalSessionBinding {
    fn current_binding(&self, _: &InvocationIr) -> Result<ConnectorSessionBinding, ProposalError> {
        Ok(ConnectorSessionBinding {
            target: ConnectorTarget {
                connector: connector_text("fixture.connector"),
                scope: ContextScopeId::new("foreign-project").unwrap(),
            },
            step: PlanStepId::new("current-step").unwrap(),
            execution: connector_text("current-execution"),
        })
    }
}

#[test]
fn canonical_binder_rejects_scope_drift_and_missing_inventory_capability_before_effect() {
    let (ir, host) = connector_fixture();
    let verified = verified_tools(&ir, &host);

    let connector = proposal_connector(&ir, None);
    let calls = connector.calls.clone();
    let dispatcher = ConnectorProposalDispatcher::new(
        CanonicalConnectorProposalBinder::new(
            Capture,
            WrongScopeProposalSessionBinding,
            proposal_connector(&ir, None),
        ),
        connector,
    );
    let batch = dispatcher.dispatch(&ir, &verified).unwrap();
    assert_eq!(batch.failure, Some((0, ProposalError::StaleInvocation)));
    assert!(batch.results.is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let connector = proposal_connector(&ir, None);
    let calls = connector.calls.clone();
    let mut inventory_without_capability = proposal_connector(&ir, None);
    inventory_without_capability.inventory_capability =
        CapabilityId::new("connector.not-in-invocation").unwrap();
    let dispatcher = ConnectorProposalDispatcher::new(
        CanonicalConnectorProposalBinder::new(
            Capture,
            ProposalSessionBinding,
            inventory_without_capability,
        ),
        connector,
    );
    let batch = dispatcher.dispatch(&ir, &verified).unwrap();
    assert_eq!(batch.failure, Some((0, ProposalError::InvalidProposal)));
    assert!(batch.results.is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn canonical_binder_rejects_discovery_for_a_different_target() {
    let (ir, host) = connector_fixture();
    let verified = verified_tools(&ir, &host);
    let mut inventory = proposal_connector(&ir, None);
    inventory.wrong_discovery_target = true;
    let connector = proposal_connector(&ir, None);
    let calls = connector.calls.clone();
    let binder = CanonicalConnectorProposalBinder::new(Capture, ProposalSessionBinding, inventory);
    let batch = ConnectorProposalDispatcher::new(binder, connector)
        .dispatch(&ir, &verified)
        .unwrap();
    assert_eq!(batch.failure, Some((0, ProposalError::StaleInvocation)));
    assert!(batch.results.is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn canonical_binder_builds_request_from_current_session_and_discovery() {
    let (ir, host) = connector_fixture();
    let verified = verified_tools(&ir, &host);
    let binder = CanonicalConnectorProposalBinder::new(
        Capture,
        ProposalSessionBinding,
        proposal_connector(&ir, None),
    );
    let dispatcher = ConnectorProposalDispatcher::new(binder, proposal_connector(&ir, None));
    let batch = dispatcher.dispatch(&ir, &verified).unwrap();
    assert!(batch.all_calls_returned);
    assert_eq!(batch.results.len(), 2);
    assert_eq!(
        batch.results[0].observation.metadata.task,
        ir.prepared().invocation().data().task.id
    );
    assert_eq!(
        batch.results[0].observation.metadata.step.as_str(),
        "current-step"
    );
    assert_eq!(
        batch.results[0].observation.metadata.execution.as_str(),
        "current-execution"
    );
    assert_eq!(
        batch.results[0].observation.target.scope,
        ir.prepared().invocation().data().task.scope
    );
}

#[test]
fn dispatcher_marks_malformed_mutation_result_unknown_and_rejects_read_result() {
    let (ir, host) = connector_fixture();
    let verified = verified_tools(&ir, &host);
    for (class, expected_effect) in [
        (ConnectorOperationClass::Read, ConnectorEffect::Acknowledged),
        (ConnectorOperationClass::Mutate, ConnectorEffect::Unknown),
    ] {
        let mut connector = proposal_connector(&ir, None);
        connector.invalid_observation = true;
        connector.operation_class = class;
        let mut binder_connector = proposal_connector(&ir, None);
        binder_connector.operation_class = class;
        let binder = CanonicalConnectorProposalBinder::new(
            Capture,
            ProposalSessionBinding,
            binder_connector,
        );
        let dispatcher = ConnectorProposalDispatcher::new(binder, connector);
        let batch = dispatcher.dispatch(&ir, &verified).unwrap();
        assert!(batch.results.is_empty());
        match batch.failure {
            Some((0, ProposalError::Connector(failure))) => {
                assert_eq!(failure.code, ConnectorFailureCode::StaleResult);
                assert_eq!(failure.effect, expected_effect);
            }
            other => panic!("expected stale connector result, got {other:?}"),
        }
    }
}

#[test]
fn canonical_dispatcher_cg09_denial_and_stale_consent_have_zero_transport_calls() {
    let (ir, host) = connector_fixture();
    let verified = verified_tools(&ir, &host);
    let capability = ir.prompt().tools().keys().next().unwrap().clone();

    let denied = connector_policy_snapshot(&capability, Approval::Denied, false, None);
    let connector = cg09_gated_connector(&ir, denied.clone(), denied);
    let transport_calls = connector.transport.calls.clone();
    let dispatcher = ConnectorProposalDispatcher::new(
        CanonicalConnectorProposalBinder::new(
            Capture,
            ProposalSessionBinding,
            proposal_connector(&ir, None),
        ),
        connector,
    );
    let batch = dispatcher.dispatch(&ir, &verified).unwrap();
    assert!(batch.results.is_empty());
    assert!(matches!(
        batch.failure,
        Some((0, ProposalError::Connector(_)))
    ));
    assert_eq!(transport_calls.load(Ordering::SeqCst), 0);

    let consent_one = NonEmptyText::new("consent-one").unwrap();
    let consent_two = NonEmptyText::new("consent-two").unwrap();
    let initial =
        connector_policy_snapshot(&capability, Approval::Granted, true, Some(consent_one));
    let changed =
        connector_policy_snapshot(&capability, Approval::Granted, true, Some(consent_two));
    let connector = cg09_gated_connector(&ir, initial, changed);
    let transport_calls = connector.transport.calls.clone();
    let dispatcher = ConnectorProposalDispatcher::new(
        CanonicalConnectorProposalBinder::new(
            Capture,
            ProposalSessionBinding,
            proposal_connector(&ir, None),
        ),
        connector,
    );
    let batch = dispatcher.dispatch(&ir, &verified).unwrap();
    assert!(batch.results.is_empty());
    assert!(matches!(
        batch.failure,
        Some((0, ProposalError::Connector(_)))
    ));
    assert_eq!(transport_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn canonical_dispatcher_cg09_allow_preserves_exact_current_request_identity() {
    let (ir, host) = connector_fixture();
    let verified = verified_tools(&ir, &host);
    let capability = ir.prompt().tools().keys().next().unwrap().clone();
    let allowed = connector_policy_snapshot(&capability, Approval::Granted, false, None);
    let connector = cg09_gated_connector(&ir, allowed.clone(), allowed);
    let transport_calls = connector.transport.calls.clone();
    let admitted_requests = connector.authorized.clone();
    let dispatcher = ConnectorProposalDispatcher::new(
        CanonicalConnectorProposalBinder::new(
            Capture,
            ProposalSessionBinding,
            proposal_connector(&ir, None),
        ),
        connector,
    );
    let batch = dispatcher.dispatch(&ir, &verified).unwrap();

    assert!(batch.all_calls_returned);
    assert_eq!(batch.results.len(), 2);
    assert_eq!(transport_calls.load(Ordering::SeqCst), 2);
    let admitted = admitted_requests.lock().unwrap();
    assert_eq!(admitted.len(), batch.results.len());
    for (request, result) in admitted.iter().zip(&batch.results) {
        assert_eq!(request, &result.request);
        assert_eq!(request.capability, capability);
        assert_eq!(
            request.target.scope,
            ir.prepared().invocation().data().task.scope
        );
        assert_eq!(request.task, ir.prepared().invocation().data().task.id);
        assert_eq!(request.step.as_str(), "current-step");
        assert_eq!(request.execution.as_str(), "current-execution");
        assert_eq!(request.inventory_revision.as_str(), "a".repeat(64));
    }
}

#[test]
fn canonical_dispatcher_rejects_changed_inventory_before_transport() {
    let (ir, host) = connector_fixture();
    let verified = verified_tools(&ir, &host);
    let capability = ir.prompt().tools().keys().next().unwrap().clone();
    let allowed = connector_policy_snapshot(&capability, Approval::Granted, false, None);
    let connector = cg09_gated_connector(&ir, allowed.clone(), allowed);
    let transport_calls = connector.transport.calls.clone();
    let mut changed_inventory = proposal_connector(&ir, None);
    // Same capability ID with a new revision must still be rejected before dispatch.
    changed_inventory.inventory_revision = ContentDigest::new("f".repeat(64)).unwrap();
    let dispatcher = ConnectorProposalDispatcher::new(
        CanonicalConnectorProposalBinder::new(Capture, ProposalSessionBinding, changed_inventory),
        connector,
    );
    let batch = dispatcher.dispatch(&ir, &verified).unwrap();

    assert!(batch.results.is_empty());
    assert!(matches!(
        batch.failure,
        Some((0, ProposalError::StaleInvocation))
    ));
    assert_eq!(transport_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn canonical_dispatcher_rejects_argument_schema_drift_before_transport() {
    let (ir, host) = connector_fixture();
    let verified = verified_tools(&ir, &host);
    let capability = ir.prompt().tools().keys().next().unwrap().clone();
    let allowed = connector_policy_snapshot(&capability, Approval::Granted, false, None);
    let connector = cg09_gated_connector(&ir, allowed.clone(), allowed);
    let transport_calls = connector.transport.calls.clone();
    let mut changed_schema = proposal_connector(&ir, None);
    changed_schema.argument_schema_digest = ContentDigest::new("f".repeat(64)).unwrap();
    let dispatcher = ConnectorProposalDispatcher::new(
        CanonicalConnectorProposalBinder::new(Capture, ProposalSessionBinding, changed_schema),
        connector,
    );
    let batch = dispatcher.dispatch(&ir, &verified).unwrap();

    assert!(batch.results.is_empty());
    assert!(matches!(
        batch.failure,
        Some((0, ProposalError::StaleInvocation))
    ));
    assert_eq!(transport_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn invocation_admission_rejects_a_forged_connector_schema_pin() {
    let (ir, _) = connector_fixture();
    let mut tools = ir.prompt().tools().clone();
    tools
        .values_mut()
        .next()
        .unwrap()
        .connector_inventory
        .as_mut()
        .unwrap()
        .argument_schema_digest = ContentDigest::new("f".repeat(64)).unwrap();
    assert_eq!(
        admit_invocation_ir(
            SchemaVersion::V1,
            ir.prompt().handoff().clone(),
            ir.prepared().clone(),
            tools,
            &Capture,
        )
        .unwrap_err(),
        InvocationIrError::InvalidBinding
    );
}

#[test]
fn verified_proposals_dispatch_in_order_and_do_not_claim_goal_completion() {
    let (ir, host) = connector_fixture();
    let verified = verified_tools(&ir, &host);
    let connector = proposal_connector(&ir, None);
    let dispatcher = ConnectorProposalDispatcher::new(
        ProposalBinder {
            bad_ordinal: None,
            bind_error_ordinal: None,
            unsupported_ordinal: None,
            stale: false,
        },
        connector,
    );
    let batch = dispatcher.dispatch(&ir, &verified).unwrap();
    assert!(batch.all_calls_returned);
    assert_eq!(batch.results.len(), 2);
    assert_eq!(batch.results[0].ordinal, 0);
    assert_eq!(batch.results[1].ordinal, 1);
    // Connector observations are inputs to a later verifier, not goal success.
    assert_eq!(batch.failure, None);
}

#[test]
fn proposal_binding_failure_after_an_effect_retains_partial_result() {
    let (ir, host) = connector_fixture();
    let verified = verified_tools(&ir, &host);
    let connector = proposal_connector(&ir, None);
    let dispatcher = ConnectorProposalDispatcher::new(
        ProposalBinder {
            bad_ordinal: Some(1),
            bind_error_ordinal: None,
            unsupported_ordinal: None,
            stale: false,
        },
        connector,
    );
    let batch = dispatcher.dispatch(&ir, &verified).unwrap();
    assert!(!batch.all_calls_returned);
    assert_eq!(batch.results.len(), 1);
    assert_eq!(batch.failure, Some((1, ProposalError::InvalidProposal)));
}

#[test]
fn uncertain_connector_failure_is_typed_and_stops_later_proposals() {
    let (ir, host) = connector_fixture();
    let verified = verified_tools(&ir, &host);
    let connector = proposal_connector(&ir, Some(2));
    let dispatcher = ConnectorProposalDispatcher::new(
        ProposalBinder {
            bad_ordinal: None,
            bind_error_ordinal: None,
            unsupported_ordinal: None,
            stale: false,
        },
        connector,
    );
    let batch = dispatcher.dispatch(&ir, &verified).unwrap();
    assert!(!batch.all_calls_returned);
    assert_eq!(batch.results.len(), 1);
    let Some((1, ProposalError::Connector(failure))) = batch.failure else {
        panic!("typed failure expected")
    };
    assert_eq!(failure.effect, ConnectorEffect::Unknown);
    assert_eq!(failure.code, ConnectorFailureCode::MutationUncertain);
}

#[test]
fn stale_invocation_stops_before_connector_dispatch() {
    let (ir, host) = connector_fixture();
    let verified = verified_tools(&ir, &host);
    let connector = proposal_connector(&ir, None);
    let dispatcher = ConnectorProposalDispatcher::new(
        ProposalBinder {
            bad_ordinal: None,
            bind_error_ordinal: None,
            unsupported_ordinal: None,
            stale: true,
        },
        connector,
    );
    let batch = dispatcher.dispatch(&ir, &verified).unwrap();
    assert_eq!(batch.failure, Some((0, ProposalError::StaleInvocation)));
    assert!(batch.results.is_empty());
}

#[test]
fn binder_denial_after_an_effect_preserves_prior_observation() {
    let (ir, host) = connector_fixture();
    let verified = verified_tools(&ir, &host);
    let connector = proposal_connector(&ir, None);
    let dispatcher = ConnectorProposalDispatcher::new(
        ProposalBinder {
            bad_ordinal: None,
            bind_error_ordinal: Some(1),
            unsupported_ordinal: None,
            stale: false,
        },
        connector,
    );
    let batch = dispatcher.dispatch(&ir, &verified).unwrap();
    assert_eq!(batch.results.len(), 1);
    assert_eq!(batch.failure, Some((1, ProposalError::PolicyDenied)));
}

#[test]
fn unsupported_decimal_and_collection_arguments_fail_before_effect() {
    for is_set in [false, true] {
        let (ir, host) = connector_fixture();
        let verified = verified_tools(&ir, &host);
        let connector = proposal_connector(&ir, None);
        let dispatcher = ConnectorProposalDispatcher::new(
            ProposalBinder {
                bad_ordinal: None,
                bind_error_ordinal: None,
                unsupported_ordinal: Some((0, is_set)),
                stale: false,
            },
            connector,
        );
        let batch = dispatcher.dispatch(&ir, &verified).unwrap();
        assert!(batch.results.is_empty());
        assert_eq!(batch.failure, Some((0, ProposalError::InvalidProposal)));
    }
}

#[test]
fn a_verified_output_cannot_be_replayed_against_another_invocation() {
    let (ir, host) = connector_fixture();
    let verified = verified_tools(&ir, &host);
    let (other, _) = connector_fixture_with_description("Different invocation");
    let connector = proposal_connector(&ir, None);
    let dispatcher = ConnectorProposalDispatcher::new(
        ProposalBinder {
            bad_ordinal: None,
            bind_error_ordinal: None,
            unsupported_ordinal: None,
            stale: false,
        },
        connector,
    );
    assert_eq!(
        dispatcher.dispatch(&other, &verified).unwrap_err(),
        ProposalError::StaleInvocation
    );
}

#[path = "support/grounding_cases.rs"]
mod grounding_cases;

fn verified_identical_connector_proposals(ir: &InvocationIr, host: &Host) -> VerifiedOutput {
    let mut value = wire(ir);
    let proposal = json!({
        "capability": ir.prompt().tools().values().next().unwrap().binding,
        "arguments": {"query":"increment","enabled":true,"count":1,"mode":"ordered"},
    });
    value["tools"] = json!([proposal.clone(), proposal]);
    let parsed = gateway_application::output_parsing::parse_invocation_output(
        ir,
        &serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    verify_output(ir, parsed, host, vec![]).unwrap()
}

#[test]
fn identical_mutating_proposals_have_distinct_ordered_requests_and_deterministic_instance_ids() {
    use gateway_application::sessions::connector_request_digest;
    let (ir, host) = connector_fixture();
    let verified = verified_identical_connector_proposals(&ir, &host);
    assert_eq!(
        verified.output().data().tools[0],
        verified.output().data().tools[1]
    );
    let mut inventory = proposal_connector(&ir, None);
    inventory.operation_class = ConnectorOperationClass::Mutate;
    let binder = CanonicalConnectorProposalBinder::new(Capture, ProposalSessionBinding, inventory);
    let proposal = &verified.output().data().tools[0];
    let first = binder.bind(&ir, proposal, 0).unwrap().request;
    assert_eq!(binder.bind(&ir, proposal, 0).unwrap().request, first);
    let second = binder.bind(&ir, proposal, 1).unwrap().request;
    assert_ne!(first.correlation, second.correlation);
    assert_eq!(first.arguments, second.arguments);
    assert_ne!(
        connector_request_digest(&first).unwrap(),
        connector_request_digest(&second).unwrap()
    );
    let mut transport = proposal_connector(&ir, None);
    transport.operation_class = ConnectorOperationClass::Mutate;
    let calls = transport.calls.clone();
    let dispatcher = ConnectorProposalDispatcher::new(binder, transport);
    let batch = dispatcher.dispatch(&ir, &verified).unwrap();
    assert!(batch.all_calls_returned);
    assert!(batch.failure.is_none());
    assert_eq!(batch.results.len(), 2);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "intentional duplicate instances are not silently dropped"
    );
    assert_eq!(batch.results[0].ordinal, 0);
    assert_eq!(batch.results[1].ordinal, 1);
    assert_eq!(batch.results[0].request, first);
    assert_eq!(batch.results[1].request, second);
    assert_ne!(
        batch.results[0].observation.correlation,
        batch.results[1].observation.correlation
    );
}

#[test]
fn retained_request_binding_is_exact_canonical_data_without_current_admission() {
    let (ir, host) = connector_fixture();
    let verified = verified_identical_connector_proposals(&ir, &host);
    let proposal = &verified.output().data().tools[0];
    let metadata = proposal_connector(&ir, None);
    let calls = metadata.calls.clone();
    let coordinates = || ProposalSessionBinding.current_binding(&ir).unwrap();
    let inventory = metadata.discover(&coordinates().target).unwrap();
    let live = CanonicalConnectorProposalBinder::new(Capture, ProposalSessionBinding, metadata)
        .bind(&ir, proposal, 0)
        .unwrap()
        .request;
    let retained = reconstruct_retained_invocation_ir(
        SchemaVersion::V1,
        ir.prompt().handoff().clone(),
        ir.prepared().clone(),
        ir.prompt().tools().clone(),
        &json!(ir),
        &ir.identity(),
    )
    .unwrap();
    host.error.set(Some(InvocationIrError::StaleInput));
    assert!(matches!(
        revalidate_retained_invocation_ir(&retained, &host),
        Err(InvocationIrError::StaleInput)
    ));
    let historical =
        bind_retained_connector_proposal(&retained, proposal, 0, coordinates(), &inventory)
            .unwrap()
            .request;
    assert_eq!(historical, live);
    let second =
        bind_retained_connector_proposal(&retained, proposal, 1, coordinates(), &inventory)
            .unwrap()
            .request;
    assert_ne!(historical.correlation, second.correlation);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "neither live metadata binding nor historical data binding invokes an effect"
    );
}

#[test]
fn retained_request_binding_rejects_wrong_full_pins_inventory_schema_and_ambiguous_entries() {
    let (ir, host) = connector_fixture();
    let verified = verified_tools(&ir, &host);
    let proposal = &verified.output().data().tools[0];
    let coordinates = || ProposalSessionBinding.current_binding(&ir).unwrap();
    let inventory = proposal_connector(&ir, None)
        .discover(&coordinates().target)
        .unwrap();
    let retained = reconstruct_retained_invocation_ir(
        SchemaVersion::V1,
        ir.prompt().handoff().clone(),
        ir.prepared().clone(),
        ir.prompt().tools().clone(),
        &json!(ir),
        &ir.identity(),
    )
    .unwrap();
    for field in ["revision", "scope", "digest", "contract"] {
        let mut changed = proposal.clone();
        match field {
            "revision" => changed.capability.revision = ReferenceId::new("other").unwrap(),
            "scope" => changed.capability.scope = ContextScopeId::new("other").unwrap(),
            "digest" => changed.capability.digest = context_content_digest(b"other"),
            "contract" => changed.capability.contract = ReferenceId::new("other").unwrap(),
            _ => unreachable!(),
        }
        assert!(
            bind_retained_connector_proposal(&retained, &changed, 0, coordinates(), &inventory)
                .is_err(),
            "{field}"
        );
    }
    for arguments in [
        json!({}),
        json!({"unknown":true}),
        json!({"query":{"nested":true}}),
        json!([]),
    ] {
        let mut changed = proposal.clone();
        changed.arguments = arguments;
        assert!(
            bind_retained_connector_proposal(&retained, &changed, 0, coordinates(), &inventory)
                .is_err()
        );
    }
    for field in [
        "revision",
        "schema",
        "connector",
        "scope",
        "duplicate",
        "missing",
    ] {
        let mut changed = inventory.clone();
        match field {
            "revision" => changed.revision = context_content_digest(b"other"),
            "schema" => {
                changed.capabilities[0].argument_schema_digest = context_content_digest(b"other")
            }
            "connector" => changed.target.connector = connector_text("other"),
            "scope" => changed.target.scope = ContextScopeId::new("other").unwrap(),
            "duplicate" => changed.capabilities.push(changed.capabilities[0].clone()),
            "missing" => changed.capabilities.clear(),
            _ => unreachable!(),
        }
        assert!(
            bind_retained_connector_proposal(&retained, proposal, 0, coordinates(), &changed)
                .is_err(),
            "{field}"
        );
    }
    let mut wrong_coordinates = coordinates();
    wrong_coordinates.target.scope = ContextScopeId::new("other").unwrap();
    assert!(
        bind_retained_connector_proposal(&retained, proposal, 0, wrong_coordinates, &inventory)
            .is_err()
    );
}

#[test]
fn first_identical_mutation_consent_cannot_authorize_the_next_ordered_instance() {
    use gateway_application::sessions::*;
    let (ir, host) = connector_fixture();
    let verified = verified_identical_connector_proposals(&ir, &host);
    let mut inventory = proposal_connector(&ir, None);
    inventory.operation_class = ConnectorOperationClass::Mutate;
    let binder = CanonicalConnectorProposalBinder::new(Capture, ProposalSessionBinding, inventory);
    let proposal = &verified.output().data().tools[0];
    let first = binder.bind(&ir, proposal, 0).unwrap().request;
    let second = binder.bind(&ir, proposal, 1).unwrap().request;
    let owner = OwnerBinding {
        principal: PrincipalId::new("operator").unwrap(),
        workspace: WorkspaceId::new("workspace").unwrap(),
        project: ProjectId::new("project").unwrap(),
        binding: BindingId::new("binding").unwrap(),
        client_owner: ClientOwnerId::new("client").unwrap(),
    };
    let scope = first.target.scope.clone();
    let session = SessionId::new("session").unwrap();
    let run = RunId::new("run").unwrap();
    let revision = Revision::new(3).unwrap();
    let dispatch = DispatchId::new("step-dispatch").unwrap();
    let policy = PolicyId::new("mutation-policy").unwrap();
    let policy_revision = context_content_digest(b"mutation-policy-revision");
    let record = content_reference("retained-output", b"retained-output").unwrap();
    let binding = ConnectorConsentBinding::new(
        owner.clone(),
        scope.clone(),
        session.clone(),
        run.clone(),
        revision,
        PendingId::new("exact-proposal-consent").unwrap(),
        first.step.clone(),
        dispatch.clone(),
        &first,
        ConnectorConsentResumeBinding {
            invocation: ir.identity(),
            proposal_ordinal: 0,
            output_record: record.clone(),
            output_digest: context_content_digest(b"retained-output"),
            producer_pins: vec![record.clone()],
        },
        policy.clone(),
        policy_revision.clone(),
        10_000,
        owner.principal.clone(),
    )
    .unwrap();
    let grant =
        VerifiedConnectorConsent::from_trusted_record(ConnectorConsentRecordRef(record), binding)
            .unwrap();
    grant
        .check_dispatch(
            &owner,
            &scope,
            &session,
            &run,
            revision,
            &first.step,
            &dispatch,
            &first,
            &policy,
            &policy_revision,
            1,
            false,
        )
        .unwrap();
    assert_eq!(
        grant.check_dispatch(
            &owner,
            &scope,
            &session,
            &run,
            revision,
            &second.step,
            &dispatch,
            &second,
            &policy,
            &policy_revision,
            1,
            false
        ),
        Err(SessionError::StaleRevision)
    );
}
