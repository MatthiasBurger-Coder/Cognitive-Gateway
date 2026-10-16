//! Provider-independent completeness and clarification planning (#188).
//! This service produces semantic results, never consent or execution authority.
use crate::interpretation::*;
use gateway_domain::{interpretation::*, *};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// Host-derived requirements include output, evidence and verification dependencies.
/// Advisory fields must be omitted when unresolved, and cannot be dependencies of
/// mandatory fields. The complete manifest is authenticated before any lookup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SemanticField {
    pub query: InterpretationQuery,
    pub requirement: SemanticFieldRequirement,
    pub depends_on: Vec<SubjectPath>,
}

pub trait CompletenessHost: InterpretationHost {
    /// Verify that the manifest covers every required meaning of this exact input
    /// and semantic revision. Callers cannot remove requirements or downgrade them.
    fn validate_requirements(
        &self,
        binding: &InterpretationBinding,
        revision: &ContentDigest,
        fields: &[SemanticField],
    ) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletenessError {
    Interpretation(InterpretationError),
    InvalidRequirements,
    ResourceLimit,
    Compilation,
}

impl From<InterpretationError> for CompletenessError {
    fn from(value: InterpretationError) -> Self {
        Self::Interpretation(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClarificationQuestion {
    pub id: String,
    pub field: SubjectPath,
    pub expected: ExpectedMeaning,
    pub reason: SemanticGapReason,
    pub diagnostic: &'static str,
}

/// Immutable inspection contract. Persisted interaction, disclosure, expiry and
/// duplicate reply refusal belong to the host (#275). No reply is mutation consent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClarificationPlan {
    pub version: SchemaVersion,
    pub binding: InterpretationBinding,
    pub semantic_revision: ContentDigest,
    pub capture_digest: String,
    pub questions: Vec<ClarificationQuestion>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletenessResult {
    pub decisions: Vec<InterpretationDecision>,
    pub gaps: Vec<KnowledgeGap>,
    pub clarification: ClarificationPlan,
}

impl CompletenessResult {
    pub fn is_complete(&self) -> bool {
        !self.gaps.iter().any(KnowledgeGap::blocks_compilation)
    }

    /// Stable, versioned diagnostic sidecar; never executable task bytes.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let gaps: Vec<_> = self
            .gaps
            .iter()
            .map(|gap| {
                let subject = match gap.subject() {
                    KnowledgeGapSubject::Target(value) => serde_json::json!({"target": value}),
                    KnowledgeGapSubject::Path(value) => serde_json::json!({"path": value}),
                    KnowledgeGapSubject::Request(value) => serde_json::json!({"request": value}),
                };
                serde_json::json!({"subject": subject, "field": gap.missing_field(),
                "reason": gap.reason(), "diagnostic": gap.diagnostic_code(),
                "provenance": gap.provenance(), "requirement": gap.requirement(),
                "route": gap.required_resolution_class()})
            })
            .collect();
        let decisions: Vec<serde_json::Value> = self
            .decisions
            .iter()
            .map(|d| serde_json::from_slice(&d.canonical_bytes()).expect("decision JSON"))
            .collect();
        serde_json::to_vec(
            &serde_json::json!({"version": "1.0", "complete": self.is_complete(),
                "decisions": decisions, "gaps": gaps, "clarification": self.clarification
            }),
        )
        .expect("typed diagnostics serialize")
    }
}

/// Bounded work: at most 128 fields, each resolved once per validation using
/// #187's admitted source/result bounds. Compilation rechecks resolved decisions
/// once before returning. No retries or permission expansion are performed.
pub struct CompletenessValidator<'a, H, R, P> {
    pub resolver: DeterministicInterpretationResolver<'a, H, R, P>,
}

impl<H: CompletenessHost, R: InterpretationLookup, P: InterpretationLookup>
    CompletenessValidator<'_, H, R, P>
{
    pub fn validate(
        &self,
        context: &InterpretationContext,
        revision: &ContentDigest,
        fields: &[SemanticField],
        at: UnixTimestamp,
    ) -> Result<CompletenessResult, CompletenessError> {
        context.revalidate(self.resolver.host, at)?;
        if fields.len() > 128 {
            return Err(CompletenessError::ResourceLimit);
        }
        let mut fields = fields.to_vec();
        fields.sort_by_key(|f| f.query.field.to_string());
        for field in &mut fields {
            field.depends_on.sort_by_key(ToString::to_string);
            field.depends_on.dedup();
        }
        let index: BTreeMap<_, _> = fields
            .iter()
            .map(|f| (f.query.field.to_string(), f))
            .collect();
        if fields.is_empty()
            || context.binding().task.is_none()
            || index.len() != fields.len()
            || !self
                .resolver
                .host
                .validate_requirements(context.binding(), revision, &fields)
        {
            return Err(CompletenessError::InvalidRequirements);
        }
        for field in &fields {
            for dependency in &field.depends_on {
                let Some(parent) = index.get(&dependency.to_string()) else {
                    return Err(CompletenessError::InvalidRequirements);
                };
                if field.requirement == SemanticFieldRequirement::Mandatory
                    && parent.requirement != SemanticFieldRequirement::Mandatory
                {
                    return Err(CompletenessError::InvalidRequirements);
                }
            }
        }
        // Stable topological order. Cycles fail before any lookup or question.
        let mut ordered = Vec::new();
        let mut visited = BTreeSet::new();
        while ordered.len() < fields.len() {
            let Some(field) = fields.iter().find(|f| {
                !visited.contains(&f.query.field.to_string())
                    && f.depends_on
                        .iter()
                        .all(|d| visited.contains(&d.to_string()))
            }) else {
                return Err(CompletenessError::InvalidRequirements);
            };
            visited.insert(field.query.field.to_string());
            ordered.push(field);
        }
        let mut result = CompletenessResult {
            decisions: Vec::new(),
            gaps: Vec::new(),
            clarification: ClarificationPlan {
                version: SchemaVersion::V1,
                binding: context.binding().clone(),
                semantic_revision: revision.clone(),
                capture_digest: context.digest().into(),
                questions: Vec::new(),
            },
        };
        let mut unresolved = BTreeSet::new();
        for field in ordered {
            let decision = self.resolver.resolve(context, &field.query, at)?;
            let reason = match &decision.resolution {
                SemanticResolution::Resolved(_) => None,
                SemanticResolution::Unresolved(f) | SemanticResolution::Invalid(f) => {
                    Some(f.reason())
                }
                SemanticResolution::Ambiguous(_) => Some(SemanticGapReason::AmbiguousReference),
                SemanticResolution::Conflicting(_) => Some(SemanticGapReason::ConflictingContext),
            };
            let blocked = field
                .depends_on
                .iter()
                .any(|d| unresolved.contains(&d.to_string()));
            // A resolved child cannot satisfy a requirement whose prerequisite is absent.
            if let Some(reason) =
                reason.or(blocked.then_some(SemanticGapReason::MissingRequiredField))
            {
                unresolved.insert(field.query.field.to_string());
                let route = match reason {
                    SemanticGapReason::InvalidReference => KnowledgeResolutionClass::InputRepair,
                    SemanticGapReason::UnsupportedIntent => KnowledgeResolutionClass::Unsupported,
                    _ => KnowledgeResolutionClass::HumanClarification,
                };
                let gap = KnowledgeGap::new(
                    field.query.subject.clone().map_or_else(
                        || KnowledgeGapSubject::Request(context.binding().request.clone()),
                        KnowledgeGapSubject::Target,
                    ),
                    field.query.field.clone(),
                    reason,
                    vec![field.query.provenance.clone()],
                    route,
                    field.requirement,
                )
                .map_err(|_| CompletenessError::InvalidRequirements)?;
                // Only the unresolved dependency frontier is requested. Invalid input
                // requires repair; unsupported meaning is not a human knowledge question.
                if gap.blocks_compilation()
                    && !blocked
                    && route == KnowledgeResolutionClass::HumanClarification
                {
                    let identity = serde_json::to_vec(&(
                        "clarification/1.0",
                        context.binding(),
                        revision,
                        context.digest(),
                        &field.query.field,
                    ))
                    .expect("typed identity serializes");
                    result.clarification.questions.push(ClarificationQuestion {
                        id: format!("q-{:x}", Sha256::digest(identity)),
                        field: field.query.field.clone(),
                        expected: field.query.expected.clone(),
                        reason,
                        diagnostic: reason.diagnostic_code(),
                    });
                }
                result.gaps.push(gap);
            }
            result.decisions.push(decision);
        }
        Ok(result)
    }

    /// Initial requests and clarified requests use this same entrypoint. The host
    /// captures replies as fresh input, derives a fresh manifest, and supplies the
    /// normal semantic compiler. There is deliberately no patch-an-old-IR API.
    /// The compiler must bind resolved meanings to its typed operands; reference
    /// validation is always repeated here before returning the compiled task.
    pub fn compile(
        &self,
        context: &InterpretationContext,
        revision: &ContentDigest,
        fields: &[SemanticField],
        at: UnixTimestamp,
        compiler: impl FnOnce(&[InterpretationDecision]) -> Result<SemanticTaskIR, CompletenessError>,
    ) -> Result<(CompletenessResult, Option<SemanticTaskIR>), CompletenessError>
    where
        H: SemanticReferenceValidator,
    {
        let result = self.validate(context, revision, fields, at)?;
        if !result.is_complete() {
            return Ok((result, None));
        }
        let task = compiler(&result.decisions)?;
        context.revalidate(self.resolver.host, at)?;
        for decision in &result.decisions {
            if matches!(decision.resolution, SemanticResolution::Resolved(_)) {
                self.resolver.revalidate_decision(context, decision, at)?;
            }
        }
        task.validate_references(self.resolver.host)
            .map_err(|_| CompletenessError::Compilation)?;
        if context
            .binding()
            .task
            .as_ref()
            .is_some_and(|t| t.id != task.data().id)
            || task.data().target.reference.scope != context.binding().scope
        {
            return Err(CompletenessError::Compilation);
        }
        Ok((result, Some(task)))
    }
}
