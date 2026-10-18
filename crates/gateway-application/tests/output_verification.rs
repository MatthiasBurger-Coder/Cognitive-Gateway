#[path = "../../../tests/fixtures/prompt-rendering-v1/support.rs"]
mod support;
use gateway_application::{output_verification::*, prompt_ir::*, prompt_rendering::ToolDefinition};
use gateway_domain::{output_verification::*, prompt_ir::*, semantic_output::*, *};
use serde_json::{Value, json};
use std::{cell::Cell, collections::BTreeMap};
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
    let output = parse(&value);
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
