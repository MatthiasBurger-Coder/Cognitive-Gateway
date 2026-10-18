//! Pure trace/replay and role-specific qualification; no dispatch port exists here.
use crate::{
    output_parsing::parse_invocation_output,
    output_verification::{OutputVerificationPort, verify_output},
    prompt_ir::InvocationIr,
};
use gateway_domain::{
    SchemaVersion, context_content_digest,
    invocation_qualification::*,
    model_invocation::validate_model_binding,
    output_verification::{VerificationBinding, VerificationError},
    prompt_ir::InvocationIrIdentity,
};
use serde::{Deserialize, Serialize};

/// Output-only audit record. Digests remain scoped sensitive metadata; the host
/// controls retention/access. Raw prompts, outputs, credentials and thought text
/// are never copied into this receipt. Exact IR binds all upstream provenance.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayBinding {
    pub schema_version: SchemaVersion,
    pub output: VerificationBinding,
}
#[derive(Debug, Clone, Serialize)]
pub struct ReplayReceipt {
    pub schema_version: SchemaVersion,
    pub admitted_invocation: InvocationIrIdentity,
    pub output: VerificationBinding,
    pub diagnostic: Option<VerificationError>,
}
/// Content-free correlation envelope; store it only under the invocation's scope.
#[derive(Debug, Clone, Serialize)]
pub struct InvocationTrace {
    pub schema_version: SchemaVersion,
    pub replay: ReplayBinding,
    pub bindings: gateway_domain::prompt_ir::PromptBindings,
    pub model: gateway_domain::ResolvedReference<gateway_domain::ReferenceId>,
    pub provider: gateway_domain::ResolvedReference<gateway_domain::ReferenceId>,
    pub accounting: gateway_domain::prompt_rendering::TokenBudgetSnapshot,
    pub effective_generation_digest: gateway_domain::ContentDigest,
}
/// Capture exact rendered profile and output correlation, without copying data.
/// Run attempt measurements remain in InvocationRun's correlated usage records.
pub fn trace_output(
    ir: &InvocationIr,
    rendered: &crate::prompt_ir::RenderedInvocationIr,
    raw: &[u8],
) -> Result<InvocationTrace, VerificationError> {
    if rendered.identity() != &ir.identity() {
        return Err(VerificationError::CorrelationMismatch);
    }
    Ok(InvocationTrace {
        schema_version: SchemaVersion::V1,
        replay: ReplayBinding {
            schema_version: SchemaVersion::V1,
            output: VerificationBinding {
                invocation: ir.identity(),
                output_digest: context_content_digest(raw),
            },
        },
        bindings: ir.prompt().bindings().clone(),
        model: ir.prepared().capability().data().model.clone(),
        provider: ir.prepared().provider().data().identity.clone(),
        accounting: rendered.rendered().accounting().clone(),
        effective_generation_digest: context_content_digest(
            &serde_json::to_vec(ir.prepared().effective_generation()).expect("generation JSON"),
        ),
    })
}
/// Caller obtains exact bytes from a policy-permitted, scoped store. Always
/// revalidates against current authority; a prior success is never reusable
/// authority. Return only an audit receipt, never VerifiedOutput or tool values.
pub fn replay_output(
    ir: &InvocationIr,
    binding: &ReplayBinding,
    raw: &[u8],
    host: &dyn OutputVerificationPort,
) -> ReplayReceipt {
    let actual = VerificationBinding {
        invocation: ir.identity(),
        output_digest: context_content_digest(raw),
    };
    let diagnostic = if binding.schema_version != SchemaVersion::V1 {
        Some(VerificationError::IncompatibleVersion)
    } else if binding.output != actual {
        Some(VerificationError::CorrelationMismatch)
    } else {
        match parse_invocation_output(ir, raw) {
            Err(error) => Some(error),
            Ok(parsed) => verify_output(ir, parsed, host, vec![])
                .err()
                .and_then(|record| record.diagnostic),
        }
    };
    ReplayReceipt {
        schema_version: SchemaVersion::V1,
        admitted_invocation: ir.identity(),
        output: actual,
        diagnostic,
    }
}

/// Trusted evaluator owns dataset labels, complete case capture and independent
/// deterministic graders. This service evaluates their measurements; it never
/// treats confidence, a model critic or an uploaded PASS field as evidence.
pub fn qualify(
    profile: &QualificationProfile,
    measured: &QualificationMeasurements,
) -> QualificationDecision {
    use QualificationDiagnostic as D;
    use QualificationMetric as M;
    let mut diagnostics = Vec::new();
    let mut failed_metrics = std::collections::BTreeMap::new();
    let p = &profile.provenance;
    let pins = [
        &p.model,
        &p.tokenizer,
        &p.runtime,
        &p.renderer,
        &p.dataset,
        &profile.id,
    ];
    if profile.schema_version != SchemaVersion::V1
        || p.schema_version != SchemaVersion::V1
        || measured.schema_version != SchemaVersion::V1
        || pins.iter().any(|pin| validate_model_binding(pin).is_err())
    {
        diagnostics.push(D::InvalidProfile);
    }
    if p != &measured.provenance {
        diagnostics.push(D::ProvenanceMismatch);
    }
    let mut required = vec![
        M::SchemaConformance,
        M::ReferenceAccuracy,
        M::UnsupportedClaimRate,
        M::CitationValidity,
        M::BoundedRecovery,
        M::ContextLimitBehavior,
        M::FailureFallbackBehavior,
        M::ColdLatencyMs,
        M::WarmLatencyMs,
        M::ThroughputMilliPerSecond,
        M::CpuMillis,
        M::RamBytes,
        M::InputTokens,
        M::OutputTokens,
    ];
    match profile.role {
        QualificationRole::SemanticExtraction => required.extend([
            M::FieldCorrectness,
            M::RequiredCompleteness,
            M::ClarificationCorrectness,
        ]),
        QualificationRole::ToolProposal => required.extend([
            M::ToolConformance,
            M::ToolIdentifierValidity,
            M::ArgumentValidity,
            M::UnauthorizedToolRefusal,
        ]),
    }
    if profile.gpu_used {
        required.extend([M::GpuMillis, M::VramBytes]);
    }
    // Confidence can instead remain explicitly uncalibrated. If a calibration
    // gate is declared it must be measured and pass like every other gate.
    if required.iter().any(|m| !profile.thresholds.contains_key(m)) || profile.thresholds.is_empty()
    {
        diagnostics.push(D::InvalidProfile);
    }
    for (metric, threshold) in &profile.thresholds {
        if threshold.minimum > threshold.maximum
            || threshold.minimum_samples == 0
            || (metric.is_rate() && threshold.maximum > 10_000)
        {
            diagnostics.push(D::InvalidProfile);
        }
        let Some(value) = measured.metrics.get(metric) else {
            diagnostics.push(D::MissingMetric);
            failed_metrics.insert(*metric, D::MissingMetric);
            continue;
        };
        let valid = value.denominator > 0
            && value.denominator >= threshold.minimum_samples
            && if metric.is_rate() {
                value.numerator <= value.denominator
            } else {
                value.denominator == 1
            };
        if !valid {
            diagnostics.push(D::InvalidMeasurement);
            failed_metrics.insert(*metric, D::InvalidMeasurement);
            continue;
        }
        let scale = if metric.is_rate() { 10_000u128 } else { 1 };
        let numerator = u128::from(value.numerator) * scale;
        if numerator < u128::from(threshold.minimum) * u128::from(value.denominator)
            || numerator > u128::from(threshold.maximum) * u128::from(value.denominator)
        {
            diagnostics.push(D::ThresholdFailed);
            failed_metrics.insert(*metric, D::ThresholdFailed);
        }
    }
    let gates_passed = diagnostics.is_empty();
    let confidence_calibrated = profile.confidence_emitted
        && gates_passed
        && profile
            .thresholds
            .contains_key(&M::CalibrationErrorBasisPoints);
    if measured.evidence == EvidenceKind::Fixture {
        diagnostics.push(D::FixtureOnly);
    }
    QualificationDecision {
        schema_version: SchemaVersion::V1,
        profile: profile.id.clone(),
        provenance: p.clone(),
        cases_digest: measured.cases_digest.clone(),
        profile_digest: context_content_digest(&serde_json::to_vec(profile).expect("profile JSON")),
        measurements_digest: context_content_digest(
            &serde_json::to_vec(measured).expect("measurements JSON"),
        ),
        failed_metrics,
        evidence: measured.evidence,
        gates_passed,
        eligible_for_promotion: gates_passed && measured.evidence == EvidenceKind::Live,
        confidence_calibrated,
        diagnostics,
    }
}
