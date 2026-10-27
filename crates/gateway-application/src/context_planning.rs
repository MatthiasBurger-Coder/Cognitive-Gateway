//! Pure semantic analysis and deterministic source selection. No acquisition ports.
use gateway_domain::*;
use std::collections::{BTreeMap, BTreeSet};

/// Extends existing semantic admission: authenticate completeness and exact
/// Situation/Plan bindings plus additional typed needs. Never a policy decision.
pub trait ContextPlanningHost: SemanticReferenceValidator {
    fn validate_context_basis(
        &self,
        task: &SemanticTaskIR,
        input: &ContextAnalysisInput,
    ) -> Result<(), ValidationError>;
}

pub struct ContextAnalysisInput {
    pub revision: ReferenceId,
    pub basis: ContextPlanningBasis,
    /// Explicit needs derived by the host from current Situation/Plan semantics;
    /// never inferred from source availability or free-form task prose.
    pub additional: Vec<ContextRequirement>,
    /// Existing CG-07 information needs derived from the captured Situation/Plan.
    /// The host validates lineage against `basis`; these are not model proposals.
    pub planning_inputs: Vec<PlanningInput>,
    pub budget_per_requirement: RetrievalBudget,
    pub total_tokens: TokenBudget,
    pub maximum_sensitivity: SensitivityClass,
}

pub fn analyze_context_requirements(
    task: &SemanticTaskIR,
    input: ContextAnalysisInput,
    host: &impl ContextPlanningHost,
) -> Result<ContextRequirementsIR, ContextPlanningError> {
    task.validate_references(host)
        .map_err(|_| ContextPlanningError::SemanticAdmission)?;
    host.validate_context_basis(task, &input)
        .map_err(|_| ContextPlanningError::SemanticAdmission)?;
    let d = task.data();
    if input.basis.task.id != d.id
        || input.basis.task.contract_version != d.schema_version
        || input.basis.task.scope != d.target.reference.scope
        || input.basis.task.digest
            != task
                .content_digest()
                .map_err(|_| ContextPlanningError::BindingMismatch)?
    {
        return Err(ContextPlanningError::BindingMismatch);
    }
    let mut needs = Vec::new();
    let mut add = |id: &str, class, sources: Vec<_>, trust: Vec<_>, evidence: Vec<_>, necessity| {
        needs.push(ContextRequirement {
            id: ReferenceId::new(id).expect("static requirement identity"),
            class,
            purpose: NonEmptyText::new(format!("Supply {id} for the validated task"))
                .expect("static purpose"),
            necessity,
            permitted_sources: sources,
            information: RequiredInformation {
                description: NonEmptyText::new(format!(
                    "{id}; target {}; outcome {}",
                    d.target.reference.id.as_str(),
                    d.goal.outcome.as_str()
                ))
                .expect("validated target and outcome"),
                requirements: InformationRequirements::new(
                    FreshnessRequirement::Fresh,
                    None,
                    evidence,
                    vec![],
                )
                .expect("task evidence is canonical"),
                accepted_trust: trust.into_iter().collect(),
                maximum_sensitivity: input.maximum_sensitivity,
            },
            budget: input.budget_per_requirement.clone(),
            stop: StopCondition::BudgetExhausted,
            priority: needs_priority(class),
        });
    };
    use ContextClass as C;
    use ContextNecessity::{Mandatory, Optional};
    use ContextSourceClass as S;
    use TrustClass as T;
    add(
        "authority",
        C::Authority,
        vec![S::CatalogAuthority],
        vec![T::CanonicalReference],
        vec![],
        Mandatory,
    );
    add(
        "task-input",
        C::TaskInput,
        vec![S::CurrentInput, S::TaskState],
        vec![T::CallerInput],
        vec![],
        Mandatory,
    );
    add(
        "output-verification",
        C::OutputVerification,
        vec![S::TaskState],
        vec![T::CallerInput],
        vec![],
        Mandatory,
    );
    // An explicit current target is necessary for inspection/modification/verification.
    if !matches!(d.task_type, SemanticTaskType::Create) || d.current_state.is_some() {
        let (class, sources) = if d.target.kind == TargetKind::Entity {
            (
                C::StructuredData,
                vec![S::StructuredData, S::SituationState, S::ToolsMcp],
            )
        } else {
            (
                C::SystemState,
                vec![
                    S::Repository,
                    S::SituationState,
                    S::RuntimeState,
                    S::ToolsMcp,
                ],
            )
        };
        add(
            "target-state",
            class,
            sources,
            vec![T::ObservedEvidence, T::RetrievedContent],
            vec![],
            Mandatory,
        );
    }
    if matches!(d.target.kind, TargetKind::Artifact | TargetKind::Project)
        && matches!(
            d.task_type,
            SemanticTaskType::Create | SemanticTaskType::Modify
        )
    {
        add(
            "domain-knowledge",
            C::DomainKnowledge,
            vec![S::Repository, S::Rag],
            vec![T::RetrievedContent],
            vec![],
            Optional,
        );
    }
    if !d.evidence_refs.is_empty()
        || d.verification_contract
            .checks
            .contains(&SemanticVerificationCheck::EvidenceRequired)
    {
        add(
            "evidence",
            C::Evidence,
            vec![S::SituationState, S::Repository, S::Rag, S::ToolsMcp],
            vec![T::ObservedEvidence],
            d.evidence_refs.iter().map(|e| e.id.clone()).collect(),
            Mandatory,
        );
        needs.last_mut().expect("evidence added").stop =
            StopCondition::EvidenceSatisfied(std::num::NonZeroU64::MIN);
    }
    if !d.history.is_empty() {
        add_history(&mut needs, &input);
    }
    if !d.observations.is_empty() || d.current_state.is_some() {
        needs.push(runtime_need(&input));
    }
    if !input.planning_inputs.is_empty()
        && (input.basis.plan.is_none() || input.basis.situation.is_none())
    {
        return Err(ContextPlanningError::InvalidRequirements);
    }
    for p in &input.planning_inputs {
        let (class, sources, trust) = match p.kind() {
            PlanningInputKind::EvidenceAcquisition | PlanningInputKind::ConflictResolution => (
                C::Evidence,
                vec![S::SituationState, S::Repository, S::Rag],
                T::ObservedEvidence,
            ),
            PlanningInputKind::Observation => (
                C::RuntimeState,
                vec![S::SituationState, S::ToolsMcp],
                T::ObservedEvidence,
            ),
            PlanningInputKind::InputAcquisition => (
                C::TaskInput,
                vec![S::CurrentInput, S::Clarification],
                T::CallerInput,
            ),
            PlanningInputKind::Normalization => (
                C::StructuredData,
                vec![S::StructuredData, S::TaskState],
                T::DerivedAssessment,
            ),
        };
        let mut r = supplemental(&input, p.id().as_str(), class, sources, trust);
        r.id =
            ReferenceId::new(format!("planning-{}", p.id().as_str())).expect("planning identity");
        r.purpose =
            NonEmptyText::new(p.required_outcome().description()).expect("validated outcome");
        r.information.requirements = p.requirements().clone();
        if class == C::Evidence {
            r.stop = StopCondition::EvidenceSatisfied(std::num::NonZeroU64::MIN);
        }
        needs.push(r);
    }
    needs.extend(input.additional);
    ContextRequirementsIR::new(
        SchemaVersion::V1,
        input.revision,
        input.basis,
        needs,
        input.total_tokens,
    )
}
fn needs_priority(class: ContextClass) -> u32 {
    match class {
        ContextClass::Authority => 0,
        ContextClass::TaskInput => 1,
        ContextClass::OutputVerification => 2,
        _ => 10,
    }
}
fn supplemental(
    input: &ContextAnalysisInput,
    id: &str,
    class: ContextClass,
    sources: Vec<ContextSourceClass>,
    trust: TrustClass,
) -> ContextRequirement {
    ContextRequirement {
        id: ReferenceId::new(id).expect("static identity"),
        class,
        purpose: NonEmptyText::new(id).expect("static purpose"),
        necessity: ContextNecessity::Mandatory,
        permitted_sources: sources,
        information: RequiredInformation {
            description: NonEmptyText::new(id).expect("static description"),
            requirements: InformationRequirements::new(
                FreshnessRequirement::Fresh,
                None,
                vec![],
                vec![],
            )
            .expect("empty references"),
            accepted_trust: BTreeSet::from([trust]),
            maximum_sensitivity: input.maximum_sensitivity,
        },
        budget: input.budget_per_requirement.clone(),
        stop: StopCondition::BudgetExhausted,
        priority: 10,
    }
}
fn add_history(needs: &mut Vec<ContextRequirement>, input: &ContextAnalysisInput) {
    needs.push(supplemental(
        input,
        "governed-history",
        ContextClass::Memory,
        vec![ContextSourceClass::TaskState, ContextSourceClass::Memory],
        TrustClass::DerivedAssessment,
    ));
}
fn runtime_need(input: &ContextAnalysisInput) -> ContextRequirement {
    supplemental(
        input,
        "runtime-state",
        ContextClass::RuntimeState,
        vec![
            ContextSourceClass::SituationState,
            ContextSourceClass::RuntimeState,
            ContextSourceClass::ToolsMcp,
        ],
        TrustClass::ObservedEvidence,
    )
}

pub fn select_context_sources(
    requirements: &ContextRequirementsIR,
    revision: ReferenceId,
    sources: &[ContextSourceMetadata],
) -> Result<ContextPlanIR, ContextPlanningError> {
    let mut identities = BTreeSet::new();
    for s in sources {
        if !identities.insert(&s.pin.id) {
            return Err(ContextPlanningError::ConflictingRequirements);
        }
    }
    let mut decisions = Vec::new();
    let mut selected = BTreeMap::new();
    let mut missing = BTreeSet::new();
    let mut charged = BTreeSet::new();
    let mut tokens = 0u64;
    for r in requirements.requirements() {
        let mut candidates: Vec<_> = sources.iter().collect();
        // Existing eligible content wins, then explicit preference, then stable source ID.
        candidates.sort_by_key(|s| {
            (
                s.present.is_none(),
                r.permitted_sources.iter().position(|c| c == &s.class),
                &s.pin.id,
            )
        });
        for s in candidates {
            use ContextSelectionReason as R;
            let reason = if !r.necessity.active() {
                R::Inactive
            } else if let Some(reason) =
                context_source_rejection(r, s, &requirements.basis().task.scope)
            {
                reason
            } else if selected.contains_key(&r.id) {
                R::LowerPreference
            } else if !charged.contains(&s.pin.id)
                && tokens
                    .checked_add(s.tokens)
                    .is_none_or(|n| n > requirements.total_tokens().0)
            {
                R::BudgetExceeded
            } else {
                if charged.insert(s.pin.id.clone()) {
                    tokens += s.tokens;
                }
                selected.insert(r.id.clone(), s.clone());
                if s.present.is_some() {
                    R::SelectedPresent
                } else {
                    R::SelectedAcquisition
                }
            };
            decisions.push(ContextSourceDecision {
                requirement: Some(r.id.clone()),
                class: s.class,
                source: Some(s.pin.id.clone()),
                reason,
            });
        }
        if r.necessity.required() && !selected.contains_key(&r.id) {
            missing.insert(r.id.clone());
        }
    }
    for class in ContextSourceClass::ALL {
        if !requirements
            .requirements()
            .iter()
            .any(|r| r.necessity.active() && r.permitted_sources.contains(&class))
        {
            decisions.push(ContextSourceDecision {
                requirement: None,
                class,
                source: None,
                reason: ContextSelectionReason::NoneRequired,
            });
        }
    }
    ContextPlanIR::new(revision, requirements, selected, decisions, missing)
}

/// Adapt an explicit selected retrieval source to CG-15. Tools/structured-data
/// acquisition stays with its owner and cannot masquerade as a retrieval engine.
pub struct ContextRetrievalBinding<'a> {
    pub source: RetrievalSource,
    pub strategy: RetrievalStrategy,
    pub support: &'a RetrievalSupport,
    pub provenance: ProvenanceId,
    pub step: Option<PlanStepId>,
}

pub fn context_retrieval_plan(
    requirements: &ContextRequirementsIR,
    plan: &ContextPlanIR,
    requirement: &ReferenceId,
    binding: ContextRetrievalBinding<'_>,
) -> Result<RetrievalPlan, RetrievalError> {
    let ContextRetrievalBinding {
        source,
        strategy,
        support,
        provenance,
        step,
    } = binding;
    if plan.requirements_binding() != &requirements.binding() {
        return Err(RetrievalError::InvalidPlan);
    }
    let r = requirements
        .requirements()
        .iter()
        .find(|r| &r.id == requirement)
        .ok_or(RetrievalError::InvalidPlan)?;
    let s = plan
        .selected()
        .get(requirement)
        .ok_or(RetrievalError::InvalidPlan)?;
    if s.present.is_some() || source.id.as_str() != s.pin.id.as_str() {
        return Err(RetrievalError::InvalidPlan);
    }
    let purpose = match s.class {
        ContextSourceClass::Repository | ContextSourceClass::Rag => {
            if source.kind == RetrievalSourceKind::Memory {
                return Err(RetrievalError::UnsupportedSource);
            }
            RetrievalPurpose::TaskKnowledge
        }
        ContextSourceClass::Memory if source.kind == RetrievalSourceKind::Memory => {
            RetrievalPurpose::MemoryRecall
        }
        _ => return Err(RetrievalError::UnsupportedSource),
    };
    let request = RetrievalRequest::new(RetrievalRequestInput {
        version: RetrievalVersion::V1,
        scope: requirements.basis().task.scope.clone(),
        provenance,
        situation: requirements
            .basis()
            .situation
            .as_ref()
            .map(|s| s.id.clone()),
        step,
        purpose,
        required: r.information.clone(),
        queries: BTreeSet::from([RetrievalQuery(r.information.description.clone())]),
        sources: vec![source],
        strategies: vec![strategy],
        budget: r.budget.clone(),
        stop: Some(r.stop),
    })?;
    RetrievalPlan::new(
        RetrievalPlanId::new(format!(
            "{}:{}",
            plan.binding().digest.as_str(),
            r.id.as_str()
        ))
        .expect("nonempty plan identity"),
        request,
        support,
    )
}
