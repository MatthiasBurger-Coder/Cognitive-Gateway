//! Bounded natural-language inspection profile 1.0. No model or execution authority.
use crate::{completeness::*, interpretation::*};
use gateway_domain::{interpretation::*, *};
use sha2::{Digest, Sha256};

/// The host authenticates the output definition and the complete inspection
/// profile against the request/task. The profile never supplies a target pin.
/// Reject outputs/tasks needing additional mandatory evidence, state, policies
/// or verification checks; this bounded profile cannot silently omit them.
pub trait InspectionLanguageHost: CompletenessHost + SemanticReferenceValidator {
    fn validate_inspection_output(
        &self,
        binding: &InterpretationBinding,
        output: &SemanticOutputContract,
    ) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InspectionLanguageError {
    ResourceLimit,
    InputMismatch,
    UnsupportedRequest,
    InvalidContract,
    Validation(CompletenessError),
}

/// Exact supported sentences: `Inspect artifact <canonical-id>[.]` and
/// `Inspect it[.]`, with optional surrounding whitespace. The entire sentence
/// must match; extra instructions, negation and other operations are rejected.
/// Original UTF-8 bytes must hash to the admitted input pin. Hosts authenticate
/// that pin, the derived query/manifest and references using the existing ports.
impl<H: InspectionLanguageHost, R: InterpretationLookup, P: InterpretationLookup>
    CompletenessValidator<'_, H, R, P>
{
    pub fn compile_inspection_request(
        &self,
        context: &InterpretationContext,
        text: &str,
        output: &SemanticOutputContract,
        at: UnixTimestamp,
    ) -> Result<(CompletenessResult, Option<SemanticTaskIR>), InspectionLanguageError> {
        use InspectionLanguageError as E;
        if text.len() > 4096 {
            return Err(E::ResourceLimit);
        }
        context
            .revalidate(self.resolver.host, at)
            .map_err(|e| E::Validation(e.into()))?;
        let binding = context.binding();
        if format!("{:x}", Sha256::digest(text.as_bytes())) != binding.input.digest.as_str() {
            return Err(E::InputMismatch);
        }
        let sentence = text.trim().strip_suffix('.').unwrap_or(text.trim());
        let start = text.len() - text.trim_start().len();
        let (expression, span) = if sentence == "Inspect it" {
            (ReferenceExpression::Focus, (start + 8, start + 10))
        } else if let Some(id) = sentence.strip_prefix("Inspect artifact ") {
            let id = ReferenceId::new(id).map_err(|_| E::UnsupportedRequest)?;
            (
                ReferenceExpression::Entity(EntityQuery::Canonical {
                    id,
                    kind: TargetKind::Artifact,
                }),
                (start + 17, start + sentence.len()),
            )
        } else {
            return Err(E::UnsupportedRequest);
        };
        let task_id = binding.task.as_ref().ok_or(E::InvalidContract)?.id.clone();
        if output.schema.scope != binding.scope
            || !self
                .resolver
                .host
                .validate_inspection_output(binding, output)
        {
            return Err(E::InvalidContract);
        }
        // These are versioned profile semantics, not inferred permissions or
        // missing facts. The host validates the manifest against this revision.
        let revision = inspection_revision(context, text, output);
        let fields = [SemanticField {
            query: InterpretationQuery {
                field: "target".parse().expect("static field"),
                expected: ExpectedMeaning::Target(TargetKind::Artifact),
                expression,
                provenance: Provenance::new(
                    ProvenanceId::new(binding.request.as_str()).expect("validated identifier"),
                    SourceKind::Caller,
                    SourceId::new(binding.input.id.as_str()).expect("validated identifier"),
                    binding.input.id.as_str(),
                )
                .expect("validated input identity"),
                span,
                subject: None,
            },
            requirement: SemanticFieldRequirement::Mandatory,
            depends_on: vec![],
        }];
        self.compile(context, &revision, &fields, at, |decisions| {
            let InterpretationValue::Target(target) = decisions[0]
                .resolution
                .require_resolved()
                .map_err(|_| CompletenessError::Compilation)?
            else {
                return Err(CompletenessError::Compilation);
            };
            SemanticTaskIR::new(SemanticTaskData {
                schema_version: SchemaVersion::V1,
                id: task_id,
                task_type: SemanticTaskType::Inspect,
                target: target.clone(),
                goal: SemanticGoal {
                    outcome: ReferenceId::new("inspect-artifact").expect("static outcome"),
                    description: "Report the artifact properties".into(),
                },
                inputs: vec![],
                context_refs: vec![],
                current_state: None,
                desired_state: None,
                observations: vec![],
                history: vec![],
                capability_requirements: vec![],
                constraints: vec![],
                policy_refs: vec![],
                evidence_refs: vec![],
                assumptions: vec![],
                output_contract: output.clone(),
                verification_contract: SemanticVerificationContract {
                    checks: vec![
                        SemanticVerificationCheck::OutputSchemaValid,
                        SemanticVerificationCheck::NoUnresolvedReference,
                    ],
                },
                process: None,
            })
            .map_err(|_| CompletenessError::Compilation)
        })
        .map_err(E::Validation)
    }
}

/// Exact semantic revision of the supported profile, input and admitted capture.
/// Hosts can reproduce this binding when authenticating the derived manifest.
pub fn inspection_revision(
    context: &InterpretationContext,
    text: &str,
    output: &SemanticOutputContract,
) -> ContentDigest {
    let bytes = serde_json::to_vec(&serde_json::json!({
        "profile":"artifact-inspection/1.0", "input":text,
        "capture":context.digest(), "output":output
    }))
    .expect("profile JSON");
    ContentDigest::new(format!("{:x}", Sha256::digest(bytes))).expect("SHA-256")
}
