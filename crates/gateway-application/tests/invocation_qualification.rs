#[path = "../../../tests/fixtures/prompt-rendering-v1/support.rs"]
mod support;
use gateway_application::invocation_qualification::*;
use gateway_application::{output_verification::*, prompt_ir::*, prompt_rendering::ToolDefinition};
use gateway_domain::invocation_qualification::EvidenceKind;
use gateway_domain::invocation_qualification::*;
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
                    connector_inventory: None,
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
#[test]
fn replay_rechecks_current_authority_and_returns_no_dispatchable_result() {
    let (ir, host) = fixture();
    let mut value = wire(&ir);
    value["tools"] = json!([{"capability":ir.prompt().tools().values().next().unwrap().binding,"arguments":"sensitive-tool-argument"}]);
    let raw = serde_json::to_vec(&value).unwrap();
    let binding = ReplayBinding {
        schema_version: SchemaVersion::V1,
        output: VerificationBinding {
            invocation: ir.identity(),
            output_digest: context_content_digest(&raw),
        },
    };
    for _ in 0..2 {
        let receipt = replay_output(&ir, &binding, &raw, &host);
        assert_eq!(receipt.diagnostic, None);
        let audit = serde_json::to_string(&receipt).unwrap();
        assert!(!audit.contains("sensitive-tool-argument"));
        assert!(!audit.contains("arguments"));
        assert!(!audit.contains("result\""));
    }
    host.error.set(Some(InvocationIrError::StaleInput));
    assert_eq!(
        replay_output(&ir, &binding, &raw, &host).diagnostic,
        Some(VerificationError::StaleContext)
    );
    host.error.set(None);
    host.outcome.set(CheckOutcome::Failed);
    assert_eq!(
        replay_output(&ir, &binding, &raw, &host).diagnostic,
        Some(VerificationError::ConstraintViolation)
    );
    host.outcome.set(CheckOutcome::Passed);
    let mut bad = binding.clone();
    bad.schema_version = serde_json::from_value(json!("2.0")).unwrap();
    assert_eq!(
        replay_output(&ir, &bad, &raw, &host).diagnostic,
        Some(VerificationError::IncompatibleVersion)
    );
    assert_eq!(
        replay_output(&ir, &binding, b"modified", &host).diagnostic,
        Some(VerificationError::CorrelationMismatch)
    );
    for (raw, expected) in [
        (b"{".as_slice(), VerificationError::OutputParseFailed),
        (b"{}".as_slice(), VerificationError::OutputParseFailed),
    ] {
        let mut b = binding.clone();
        b.output.output_digest = context_content_digest(raw);
        assert_eq!(
            replay_output(&ir, &b, raw, &host).diagnostic,
            Some(expected)
        );
    }
    let mut bad = wire(&ir);
    bad["result"] = json!(12);
    let raw = serde_json::to_vec(&bad).unwrap();
    let mut b = binding;
    b.output.output_digest = context_content_digest(&raw);
    assert_eq!(
        replay_output(&ir, &b, &raw, &host).diagnostic,
        Some(VerificationError::OutputSchemaInvalid)
    );
}

fn metrics() -> (QualificationProfile, QualificationMeasurements) {
    let profile: QualificationProfile = serde_json::from_str(include_str!(
        "../../../tests/fixtures/invocation-qualification-v1/profile.json"
    ))
    .unwrap();
    let measurements: QualificationMeasurements = serde_json::from_str(include_str!(
        "../../../tests/fixtures/invocation-qualification-v1/measurements.json"
    ))
    .unwrap();
    (profile, measurements)
}
#[test]
fn role_specific_metrics_deny_semantically_wrong_schema_valid_outputs() {
    use QualificationMetric as M;
    let (profile, measured) = metrics();
    let d = qualify(&profile, &measured);
    assert!(d.gates_passed);
    assert!(!d.eligible_for_promotion);
    assert!(!d.confidence_calibrated);
    assert_eq!(d.diagnostics, vec![QualificationDiagnostic::FixtureOnly]);
    let mut live = measured.clone();
    live.evidence = EvidenceKind::Live;
    assert!(qualify(&profile, &live).eligible_for_promotion);
    for metric in [
        M::FieldCorrectness,
        M::UnsupportedClaimRate,
        M::CitationValidity,
        M::ClarificationCorrectness,
    ] {
        let mut bad = live.clone();
        bad.metrics.get_mut(&metric).unwrap().numerator = if metric == M::UnsupportedClaimRate {
            1
        } else {
            0
        };
        let d = qualify(&profile, &bad);
        assert!(!d.gates_passed);
        assert!(!d.eligible_for_promotion);
        assert!(
            d.diagnostics
                .contains(&QualificationDiagnostic::ThresholdFailed)
        );
    }
    live.perplexity_diagnostic = Some(NonEmptyText::new("diagnostic only: 1.01").unwrap());
    live.metrics.clear();
    assert!(!qualify(&profile, &live).eligible_for_promotion);
    for role in [
        QualificationRole::ToolProposal,
        QualificationRole::SemanticExtraction,
    ] {
        let mut p = profile.clone();
        p.role = role;
        p.gpu_used = true;
        assert!(!qualify(&p, &measured).gates_passed);
    }
}
#[test]
fn qualification_fails_closed_on_missing_invalid_or_mismatched_evidence() {
    use QualificationDiagnostic as D;
    use QualificationMetric as M;
    let (profile, measured) = metrics();
    for mode in 0..9 {
        let mut p = profile.clone();
        let mut m = measured.clone();
        let expected = match mode {
            0 => {
                p.schema_version = serde_json::from_value(json!("2.0")).unwrap();
                D::InvalidProfile
            }
            1 => {
                m.provenance.model.revision = ReferenceId::new("changed").unwrap();
                D::ProvenanceMismatch
            }
            2 => {
                m.metrics.remove(&M::SchemaConformance);
                D::MissingMetric
            }
            3 => {
                m.metrics
                    .get_mut(&M::SchemaConformance)
                    .unwrap()
                    .denominator = 0;
                D::InvalidMeasurement
            }
            4 => {
                m.metrics.get_mut(&M::SchemaConformance).unwrap().numerator = u64::MAX;
                D::InvalidMeasurement
            }
            5 => {
                p.thresholds.get_mut(&M::SchemaConformance).unwrap().maximum = 10_001;
                D::InvalidProfile
            }
            6 => {
                p.thresholds
                    .get_mut(&M::SchemaConformance)
                    .unwrap()
                    .minimum_samples = 0;
                D::InvalidProfile
            }
            7 => {
                m.metrics.get_mut(&M::ColdLatencyMs).unwrap().denominator = 2;
                D::InvalidMeasurement
            }
            _ => {
                p.thresholds.clear();
                D::InvalidProfile
            }
        };
        let d = qualify(&p, &m);
        assert!(!d.eligible_for_promotion);
        assert!(
            d.diagnostics.contains(&expected),
            "{mode}: {:?}",
            d.diagnostics
        );
    }
    let mut p = profile.clone();
    p.thresholds.insert(
        M::CalibrationErrorBasisPoints,
        MetricThreshold {
            minimum: 0,
            maximum: 500,
            minimum_samples: 1,
        },
    );
    let mut m = measured.clone();
    m.metrics.insert(
        M::CalibrationErrorBasisPoints,
        MetricObservation {
            numerator: 100,
            denominator: 1,
        },
    );
    assert!(qualify(&p, &m).confidence_calibrated);
    m.metrics
        .get_mut(&M::CalibrationErrorBasisPoints)
        .unwrap()
        .numerator = 600;
    assert!(!qualify(&p, &m).confidence_calibrated);
    let mut p = profile.clone();
    p.confidence_emitted = false;
    assert!(!qualify(&p, &measured).confidence_calibrated);
    // Large counts compare exactly without overflow or rounded threshold passes.
    let mut m = measured;
    m.metrics.get_mut(&M::SchemaConformance).unwrap().numerator = u64::MAX;
    m.metrics
        .get_mut(&M::SchemaConformance)
        .unwrap()
        .denominator = u64::MAX;
    assert!(qualify(&profile, &m).gates_passed);
}

use gateway_application::prompt_rendering::*;
use gateway_domain::prompt_rendering::*;
use support::examples::id;
struct Renderer;
impl PromptRendererPort for Renderer {
    fn identity(&self) -> (ReferenceId, ReferenceId) {
        (id("fixture"), id("1"))
    }
    fn render(&self, input: &RenderInput) -> Result<Value, RenderDiagnostic> {
        Ok(json!({"system":input.handoff()["instructions"], "data":render_data(input)}))
    }
}
struct Counter;
impl TokenizerPort for Counter {
    fn count(&self, r: &TokenCountRequest<'_>) -> Result<TokenCountResult, RenderDiagnostic> {
        Ok(TokenCountResult {
            profile: r.profile.clone(),
            payload_digest: r.payload_digest.clone(),
            mode: CountMode::Estimated,
            provenance: pin("fixture-bound"),
            usage: TokenUsage {
                instruction_tokens: 0,
                task_tokens: 0,
                context_tokens: 0,
                evidence_tokens: 0,
                tool_schema_tokens: 0,
                framing_tokens: r.payload.len() as u64,
            },
        })
    }
}
fn profile(ir: &InvocationIr) -> TokenizerProfile {
    TokenizerProfile {
        schema_version: SchemaVersion::V1,
        model: ir.prepared().capability().data().model.clone(),
        generation_profile: ir
            .prepared()
            .invocation()
            .data()
            .generation
            .content_digest(),
        tokenizer: id("fixture-bound"),
        tokenizer_version: id("1"),
        renderer: id("fixture"),
        template_version: id("1"),
    }
}

#[test]
fn traces_bind_rendering_and_reject_other_invocations_without_copying_content() {
    let (ir, host) = fixture();
    let rendered = render_invocation_ir(&ir, &host, profile(&ir), &Renderer, &Counter).unwrap();
    let raw = serde_json::to_vec(&wire(&ir)).unwrap();
    let trace = trace_output(&ir, &rendered, &raw).unwrap();
    assert_eq!(
        trace.replay.output.output_digest,
        context_content_digest(&raw)
    );
    assert_eq!(trace.accounting.count.profile, profile(&ir));
    assert_eq!(
        replay_output(&ir, &trace.replay, &raw, &host).diagnostic,
        None
    );
    let audit = serde_json::to_string(&trace).unwrap();
    assert!(!audit.contains("\"payload\":"));
    assert!(!audit.contains("result\""));
    assert_eq!(
        audit,
        serde_json::to_string(&trace_output(&ir, &rendered, &raw).unwrap()).unwrap()
    );
    let mut tools = ir.prompt().tools().clone();
    tools.values_mut().next().unwrap().description =
        NonEmptyText::new("different description").unwrap();
    let other = admit_invocation_ir(
        SchemaVersion::V1,
        ir.prompt().handoff().clone(),
        ir.prepared().clone(),
        tools,
        &Capture,
    )
    .unwrap();
    assert!(matches!(
        trace_output(&other, &rendered, &raw),
        Err(VerificationError::CorrelationMismatch)
    ));
}

#[test]
fn recorded_schema_valid_wrong_answers_fail_independent_semantic_and_claim_gates() {
    use QualificationMetric as M;
    let (profile, mut measurements) = metrics();
    let (ir, host) = fixture();
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tests/fixtures/invocation-qualification-v1/replay-cases.json"
    ))
    .unwrap();
    let mut correct = 0;
    let mut unsupported = 0;
    for case in &cases {
        let mut output = wire(&ir);
        output["result"] = case["recorded_result"].clone();
        let raw = serde_json::to_vec(&output).unwrap();
        let parsed =
            gateway_application::output_parsing::parse_invocation_output(&ir, &raw).unwrap();
        // Exact field labels and an independent allowed-claim set are fixture
        // oracles. No confidence or model critic determines these measurements.
        correct += u64::from(parsed.data().result == case["expected_result"]);
        let supported = case["supported_claims"]
            .as_array()
            .unwrap()
            .contains(&parsed.data().result);
        unsupported += u64::from(!supported);
        host.fault.set(if supported { 0 } else { 10 });
        let binding = ReplayBinding {
            schema_version: SchemaVersion::V1,
            output: VerificationBinding {
                invocation: ir.identity(),
                output_digest: context_content_digest(&raw),
            },
        };
        assert_eq!(
            replay_output(&ir, &binding, &raw, &host)
                .diagnostic
                .is_none(),
            supported
        );
    }
    measurements.cases_digest = context_content_digest(include_bytes!(
        "../../../tests/fixtures/invocation-qualification-v1/replay-cases.json"
    ));
    measurements.metrics.insert(
        M::FieldCorrectness,
        MetricObservation {
            numerator: correct,
            denominator: cases.len() as u64,
        },
    );
    measurements.metrics.insert(
        M::UnsupportedClaimRate,
        MetricObservation {
            numerator: unsupported,
            denominator: cases.len() as u64,
        },
    );
    assert_eq!((correct, unsupported), (9, 1));
    let d = qualify(&profile, &measurements);
    assert!(!d.gates_passed);
    assert!(
        d.diagnostics
            .contains(&QualificationDiagnostic::ThresholdFailed)
    );
}

#[test]
fn tool_role_and_gpu_require_their_own_thresholds_and_matching_provenance() {
    use QualificationMetric as M;
    let (mut profile, mut measured) = metrics();
    profile.role = QualificationRole::ToolProposal;
    profile.gpu_used = true;
    for metric in [
        M::ToolConformance,
        M::ToolIdentifierValidity,
        M::ArgumentValidity,
        M::UnauthorizedToolRefusal,
        M::GpuMillis,
        M::VramBytes,
    ] {
        let rate = metric.is_rate();
        profile.thresholds.insert(
            metric,
            MetricThreshold {
                minimum: if rate { 10_000 } else { 0 },
                maximum: if rate { 10_000 } else { 100 },
                minimum_samples: if rate { 10 } else { 1 },
            },
        );
        measured.metrics.insert(
            metric,
            MetricObservation {
                numerator: if rate { 10 } else { 5 },
                denominator: if rate { 10 } else { 1 },
            },
        );
    }
    assert!(qualify(&profile, &measured).gates_passed);
    measured
        .metrics
        .get_mut(&M::UnauthorizedToolRefusal)
        .unwrap()
        .numerator = 9;
    let denied = qualify(&profile, &measured);
    assert_eq!(
        denied.failed_metrics.get(&M::UnauthorizedToolRefusal),
        Some(&QualificationDiagnostic::ThresholdFailed)
    );
    for field in ["tokenizer", "runtime", "renderer", "dataset", "model"] {
        let mut changed = json!(measured);
        changed["provenance"][field]["revision"] = json!("different");
        let changed = serde_json::from_value(changed).unwrap();
        assert!(
            qualify(&profile, &changed)
                .diagnostics
                .contains(&QualificationDiagnostic::ProvenanceMismatch)
        );
    }
    for field in ["quantization", "hardware", "family", "generation_profile"] {
        let mut changed = json!(measured);
        changed["provenance"][field] = if field == "generation_profile" {
            json!("b".repeat(64))
        } else {
            json!("different")
        };
        let changed = serde_json::from_value(changed).unwrap();
        assert!(
            qualify(&profile, &changed)
                .diagnostics
                .contains(&QualificationDiagnostic::ProvenanceMismatch)
        );
    }
}
