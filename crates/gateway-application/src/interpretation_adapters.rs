//! Optional interpretation assistance. Scores rank proposals; they confer no authority.
use crate::{completeness::*, interpretation::*, local_inference::*};
use gateway_domain::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalSource {
    Classifier,
    Embedding,
    Slm,
}

/// All scores are uncalibrated ranking signals, never factual probabilities.
/// Embedding space identity includes model, version and artifact digest below.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticProposal {
    pub source: SignalSource,
    pub score: f64,
    pub embedding_space: Option<String>,
    pub task: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignalThreshold {
    pub source: SignalSource,
    pub model_id: String,
    pub model_version: String,
    pub artifact_digest: String,
    pub embedding_space: Option<String>,
    pub minimum_rank: f64,
    pub fixture_dataset: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterpretationPolicy {
    pub version: String,
    pub thresholds: Vec<SignalThreshold>,
}

impl InterpretationPolicy {
    pub fn validate(&self) -> bool {
        self.version == "1.0"
            && !self.thresholds.is_empty()
            && self.thresholds.iter().all(|t| {
                !t.model_id.is_empty()
                    && !t.model_version.is_empty()
                    && digest(&t.artifact_digest)
                    && !t.fixture_dataset.is_empty()
                    && t.minimum_rank.is_finite()
                    && (0.0..=1.0).contains(&t.minimum_rank)
                    && space_valid(t.source, &t.embedding_space)
            })
            && self.thresholds.iter().enumerate().all(|(i, t)| {
                !self.thresholds[..i].iter().any(|p| {
                    p.source == t.source
                        && p.model_id == t.model_id
                        && p.model_version == t.model_version
                        && p.artifact_digest == t.artifact_digest
                        && p.embedding_space == t.embedding_space
                })
            })
    }
}

fn digest(s: &str) -> bool {
    s.len() == 71 && s.starts_with("sha256:") && s[7..].bytes().all(|b| b.is_ascii_hexdigit())
}
fn space_valid(source: SignalSource, space: &Option<String>) -> bool {
    match source {
        SignalSource::Embedding => space.as_ref().is_some_and(|s| !s.is_empty()),
        _ => space.is_none(),
    }
}

/// Separate configured slots share this provider-neutral proposal contract.
pub trait InterpretationAdapter {
    fn propose(
        &self,
        request: &LocalInferenceRequest,
    ) -> Result<LocalInferenceProposal, LocalInferenceError>;
}

/// Reuses CG-27's port, including its HTTP service, qualification and lifecycle.
pub struct LocalSemanticAdapter<'a, P>(pub &'a P);
impl<P: LocalInferencePort> InterpretationAdapter for LocalSemanticAdapter<'_, P> {
    fn propose(
        &self,
        request: &LocalInferenceRequest,
    ) -> Result<LocalInferenceProposal, LocalInferenceError> {
        self.0.infer(request)
    }
}

#[derive(Default)]
pub struct InterpretationAdapters<'a> {
    pub classifier: Option<&'a dyn InterpretationAdapter>,
    pub embedding: Option<&'a dyn InterpretationAdapter>,
    pub slm: Option<&'a dyn InterpretationAdapter>,
}

/// Trusted host must compare ALL proposed meaning with the original request and
/// deterministic decisions, reject invented facts and independently evaluate
/// current permissions. Passing these checks is not consent or execution authority.
pub trait SemanticProposalHost: CompletenessHost + SemanticReferenceValidator {
    fn validate_meaning(&self, task: &SemanticTaskIR, basis: &CompletenessResult) -> bool;
    fn validate_permissions(&self, task: &SemanticTaskIR, basis: &CompletenessResult) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptStatus {
    Unavailable,
    Invalid,
    BelowThreshold,
    Rejected,
    Validated,
}

#[derive(Debug)]
pub struct AssistedInterpretation {
    /// Preserved even on success: assistance never resolves gaps by itself.
    pub deterministic: CompletenessResult,
    pub policy: InterpretationPolicy,
    pub attempts: Vec<(SignalSource, AttemptStatus)>,
    /// Inspection-only candidate, not executable admission. Consumers must re-enter
    /// normal compilation with trusted evidence satisfying the outstanding gaps.
    pub proposal: Option<LocalInferenceProposal>,
}

impl<H: SemanticProposalHost, R: InterpretationLookup, P: InterpretationLookup>
    CompletenessValidator<'_, H, R, P>
{
    pub fn assist(
        &self,
        context: &InterpretationContext,
        revision: &ContentDigest,
        fields: &[SemanticField],
        at: UnixTimestamp,
        policy: &InterpretationPolicy,
        adapters: &InterpretationAdapters<'_>,
    ) -> Result<AssistedInterpretation, CompletenessError> {
        let deterministic = self.validate(context, revision, fields, at)?;
        let mut result = AssistedInterpretation {
            deterministic,
            policy: policy.clone(),
            attempts: vec![],
            proposal: None,
        };
        // Models and configuration cannot break the no-model path.
        if result.deterministic.is_complete() {
            return Ok(result);
        }
        if !policy.validate() {
            return Err(CompletenessError::InvalidRequirements);
        }
        for (source, adapter) in [
            (SignalSource::Classifier, adapters.classifier),
            (SignalSource::Embedding, adapters.embedding),
            (SignalSource::Slm, adapters.slm),
        ] {
            let Some(adapter) = adapter else { continue };
            let request = LocalInferenceRequest {
                schema_version: "1.0".into(),
                role: match source {
                    SignalSource::Classifier => "semantic-classifier",
                    SignalSource::Embedding => "semantic-embedding",
                    SignalSource::Slm => "semantic-interpreter",
                }
                .into(),
                input_contract: "interpretation-gaps/1.0".into(),
                output_contract: "semantic-proposal/1.0".into(),
                prompt: String::from_utf8(result.deterministic.canonical_bytes())
                    .expect("JSON UTF-8"),
                output_schema: serde_json::json!({"type":"object", "additionalProperties":false,
                    "required":["source","score","embedding_space","task"], "properties":{
                        "source":{"const":source}, "score":{"type":"number","minimum":0,"maximum":1},
                        "embedding_space":{"type":["string","null"]},
                        "task":serde_json::from_str::<serde_json::Value>(include_str!("../../../schemas/semantic-task.schema.json")).expect("bundled schema")}}),
            };
            let response = adapter.propose(&request);
            // Failed calls may also race a changed host basis.
            context.revalidate(self.resolver.host, at)?;
            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    result.attempts.push((
                        source,
                        if error == LocalInferenceError::Unavailable {
                            AttemptStatus::Unavailable
                        } else {
                            AttemptStatus::Invalid
                        },
                    ));
                    continue;
                }
            };
            let status = validate_proposal(
                &response,
                source,
                &request,
                policy,
                &result.deterministic,
                self.resolver.host,
            );
            result.attempts.push((source, status.clone()));
            // Revalidate captured basis after every call, including accepted output.
            context.revalidate(self.resolver.host, at)?;
            if status == AttemptStatus::Validated {
                result.proposal = Some(response);
                break;
            }
        }
        Ok(result)
    }
}

fn validate_proposal(
    response: &LocalInferenceProposal,
    source: SignalSource,
    request: &LocalInferenceRequest,
    policy: &InterpretationPolicy,
    basis: &CompletenessResult,
    host: &impl SemanticProposalHost,
) -> AttemptStatus {
    let p = &response.provenance;
    if response.schema_version != "1.0"
        || response.kind != "proposal"
        || !digest(&response.artifact_digest)
        || !digest(&p.template_digest)
        || !digest(&p.system_digest)
        || p.runtime.is_empty()
        || p.runtime_version.is_empty()
        || p.prompt_version.is_empty()
        || !p.runtime_configuration.is_object()
        || p.input_contract != request.input_contract
        || p.output_contract != request.output_contract
    {
        return AttemptStatus::Invalid;
    }
    let Ok(proposal) = serde_json::from_value::<SemanticProposal>(response.proposal.clone()) else {
        return AttemptStatus::Invalid;
    };
    if proposal.source != source
        || !proposal.score.is_finite()
        || !(0.0..=1.0).contains(&proposal.score)
        || !space_valid(source, &proposal.embedding_space)
    {
        return AttemptStatus::Invalid;
    }
    let Some(threshold) = policy.thresholds.iter().find(|t| {
        t.source == source
            && t.model_id == response.model_id
            && t.model_version == p.model_version
            && t.artifact_digest == response.artifact_digest
            && t.embedding_space == proposal.embedding_space
    }) else {
        return AttemptStatus::Invalid;
    };
    let Ok(task) = SemanticTaskIR::from_json(&proposal.task.to_string()) else {
        return AttemptStatus::Invalid;
    };
    // Reference and permission validation is independent of confidence, including low scores.
    let references = task.validate_references(host).is_ok();
    let permissions = host.validate_permissions(&task, basis);
    let meaning = host.validate_meaning(&task, basis);
    if !references
        || !permissions
        || !meaning
        || basis
            .clarification
            .binding
            .task
            .as_ref()
            .is_none_or(|t| t.id != task.data().id)
        || basis.clarification.binding.scope != task.data().target.reference.scope
    {
        return AttemptStatus::Rejected;
    }
    if proposal.score < threshold.minimum_rank {
        AttemptStatus::BelowThreshold
    } else {
        AttemptStatus::Validated
    }
}
