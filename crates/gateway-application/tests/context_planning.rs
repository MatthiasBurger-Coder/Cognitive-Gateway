use gateway_application::context_planning::*;
use gateway_context::*;
use gateway_domain::*;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::num::NonZeroU64;
fn id(v: &str) -> ReferenceId {
    ReferenceId::new(v).unwrap()
}
fn scope() -> ContextScopeId {
    ContextScopeId::new("project-x").unwrap()
}
fn pin<I>(id: I, digest: ContentDigest) -> ResolvedReference<I> {
    ResolvedReference {
        id,
        scope: scope(),
        contract: self::id("fixture"),
        contract_version: SchemaVersion::V1,
        revision: self::id("r1"),
        digest,
    }
}
fn task() -> SemanticTaskIR {
    SemanticTaskIR::from_json(include_str!(
        "../../../tests/fixtures/semantic-task-v1/minimal.json"
    ))
    .unwrap()
}
fn budget() -> RetrievalBudget {
    RetrievalBudget {
        results: ResultBudget(NonZeroU64::MIN),
        rounds: RoundBudget(NonZeroU64::MIN),
        latency: LatencyBudget(NonZeroU64::new(100).unwrap()),
        cost: CostBudget {
            maximum: 0,
            unit: NonEmptyText::new("credits").unwrap(),
        },
        tokens: TokenBudget(1000),
        context: ContextBudget::new(
            TokenBudget(7000),
            [
                ContextBudgetClass::Knowledge,
                ContextBudgetClass::AuthorityReserved,
                ContextBudgetClass::TaskReserved,
                ContextBudgetClass::OutputContractReserved,
                ContextBudgetClass::Evidence,
                ContextBudgetClass::Memory,
                ContextBudgetClass::RuntimeState,
            ]
            .into_iter()
            .map(|c| (c, TokenBudget(1000)))
            .collect(),
        )
        .unwrap(),
    }
}
struct Host {
    deny_refs: bool,
    deny_basis: bool,
}
impl SemanticReferenceValidator for Host {
    fn validate(
        &self,
        _: &SemanticTaskData,
        _: &SemanticReferenceBinding,
    ) -> Result<(), ValidationError> {
        if self.deny_refs {
            Err(ValidationError::InvalidSchemaVersion)
        } else {
            Ok(())
        }
    }
}
impl ContextPlanningHost for Host {
    fn validate_context_basis(
        &self,
        _: &SemanticTaskIR,
        _: &ContextAnalysisInput,
    ) -> Result<(), ValidationError> {
        if self.deny_basis {
            Err(ValidationError::InvalidSchemaVersion)
        } else {
            Ok(())
        }
    }
}
fn input(t: &SemanticTaskIR) -> ContextAnalysisInput {
    ContextAnalysisInput {
        revision: id("r1"),
        basis: ContextPlanningBasis {
            task: pin(t.data().id.clone(), t.content_digest().unwrap()),
            situation: Some(pin(
                SituationId::new("s1").unwrap(),
                context_content_digest(b"situation"),
            )),
            plan: Some(pin(
                PlanId::new("p1").unwrap(),
                context_content_digest(b"plan"),
            )),
        },
        additional: vec![],
        planning_inputs: vec![],
        budget_per_requirement: budget(),
        total_tokens: TokenBudget(4000),
        maximum_sensitivity: SensitivityClass::Internal,
    }
}
fn analyze(t: &SemanticTaskIR) -> ContextRequirementsIR {
    analyze_context_requirements(
        t,
        input(t),
        &Host {
            deny_refs: false,
            deny_basis: false,
        },
    )
    .unwrap()
}
fn plan(r: &ContextRequirementsIR, s: &[ContextSourceMetadata]) -> ContextPlanIR {
    select_context_sources(r, id("r1"), s).unwrap()
}
fn quality(trust: TrustClass) -> QualityMetadata {
    QualityMetadata::new(
        trust,
        SensitivityClass::Normal,
        Confidence::Unknown,
        FreshnessStatus::Fresh,
        Uncertainty::None,
    )
}
fn source(
    name: &str,
    class: ContextSourceClass,
    covers: &str,
    trust: TrustClass,
) -> ContextSourceMetadata {
    ContextSourceMetadata {
        pin: pin(id(name), context_content_digest(b"properties")),
        payload_digest: Some(context_content_digest(b"properties")),
        class,
        covers: BTreeSet::from([id(covers)]),
        quality: quality(trust),
        evidence: BTreeSet::new(),
        provenances: BTreeSet::new(),
        tokens: 10,
        available: true,
        present: None,
    }
}
fn projection() -> ExecutionContextIR {
    ExecutionContextIR::new_v1(
        ExecutionContextId::new("context").unwrap(),
        TaskDescriptor::new(task().data().id.clone(), "Inspect artifact").unwrap(),
        WorkflowId::new("inspect").unwrap(),
        AgentId::new("inspector").unwrap(),
        [SkillId::new("inspect").unwrap()],
        OperatingMode::Development,
        ExecutionProfile::FullPath,
        ExecutionState::new(
            WorkflowState::Running,
            GateState::Pending,
            BlockerState::Clear,
        )
        .unwrap(),
        PolicyId::new("read-only").unwrap(),
        [],
        [],
        [],
        ExecutionRuntimeId::new("offline").unwrap(),
    )
    .unwrap()
}
fn compiled(fragments: &[ContextFragment]) -> CompiledContext {
    CompiledContext::assemble(
        projection(),
        scope(),
        PlanStepId::new("step").unwrap(),
        fragments,
        &fragments.iter().map(|f| f.id().clone()).collect(),
    )
    .unwrap()
}
fn context_pin(c: &CompiledContext) -> ResolvedReference<ReferenceId> {
    binding(
        "compiled-context",
        &id("r1"),
        &scope(),
        &serde_json::from_str::<Value>(&c.to_json().unwrap()).unwrap(),
    )
}
fn fragment(s: &ContextSourceMetadata) -> ContextFragment {
    ContextFragment::external(
        id("fragment"),
        FragmentKind::Knowledge,
        "properties",
        FragmentMetadata {
            provenance: KnowledgeProvenance::new(s.pin.id.as_str(), Some(s.pin.revision.as_str()))
                .unwrap(),
            evidence: s
                .evidence
                .iter()
                .map(|e| id(e.as_str()))
                .chain(s.provenances.iter().map(|p| id(p.as_str())))
                .collect(),
            quality: s.quality,
            rationale: NonEmptyText::new("selected by plan").unwrap(),
            validation: None,
        },
        scope(),
        PlanStepId::new("step").unwrap(),
    )
    .unwrap()
}
fn sources(t: &SemanticTaskIR, c: &CompiledContext) -> Vec<ContextSourceMetadata> {
    let mut a = source(
        "catalog",
        ContextSourceClass::CatalogAuthority,
        "authority",
        TrustClass::CanonicalReference,
    );
    let cv: Value = serde_json::from_str(&c.to_json().unwrap()).unwrap();
    a.pin.digest = context_content_digest(serde_json::to_string(&cv["stable"]).unwrap().as_bytes());
    a.payload_digest = Some(a.pin.digest.clone());
    a.present = Some(ContextSatisfaction::Catalog);
    let mut b = source(
        "input",
        ContextSourceClass::CurrentInput,
        "task-input",
        TrustClass::CallerInput,
    );
    b.pin.digest = t.content_digest().unwrap();
    b.payload_digest = Some(b.pin.digest.clone());
    b.present = Some(ContextSatisfaction::Task);
    let mut o = source(
        "output",
        ContextSourceClass::TaskState,
        "output-verification",
        TrustClass::CallerInput,
    );
    o.pin.digest = context_content_digest(
        serde_json::to_string(&json!({"output":contracts(t).output,
        "verification":contracts(t).verification}))
        .unwrap()
        .as_bytes(),
    );
    o.payload_digest = Some(o.pin.digest.clone());
    o.present = Some(ContextSatisfaction::OutputContract);
    vec![
        a,
        b,
        o,
        source(
            "repo",
            ContextSourceClass::Repository,
            "target-state",
            TrustClass::RetrievedContent,
        ),
    ]
}
fn requirement() -> ContextRequirement {
    analyze(&task())
        .requirements()
        .iter()
        .find(|r| r.id == id("target-state"))
        .unwrap()
        .clone()
}
fn single(r: ContextRequirement) -> ContextRequirementsIR {
    ContextRequirementsIR::new(
        SchemaVersion::V1,
        id("r1"),
        input(&task()).basis,
        vec![r],
        TokenBudget(1000),
    )
    .unwrap()
}
#[test]
fn deterministic_requirements_and_plans_do_not_rewrite_meaning() {
    let t = task();
    let r = analyze(&t);
    let before = t.to_canonical_json().unwrap();
    assert_eq!(r, analyze(&t));
    assert_eq!(r.to_value()["basis"]["plan"]["id"], "p1");
    assert_eq!(r.basis().task.digest, t.content_digest().unwrap());
    let mut s = sources(&t, &compiled(&[]));
    let p = plan(&r, &s);
    s.reverse();
    assert_eq!(p, plan(&r, &s));
    s.clear();
    let unavailable = plan(&r, &s);
    assert_eq!(unavailable.requirements_binding(), &r.binding());
    assert_eq!(before, t.to_canonical_json().unwrap());
    assert_eq!(unavailable.missing().len(), 4);
    assert_eq!(p.selected().len(), 4);
    assert!(p.missing().is_empty());
    for class in [
        ContextSourceClass::Rag,
        ContextSourceClass::Memory,
        ContextSourceClass::Examples,
        ContextSourceClass::StructuredData,
        ContextSourceClass::ToolsMcp,
    ] {
        assert!(
            p.decisions()
                .iter()
                .any(|d| d.class == class && d.reason == ContextSelectionReason::NoneRequired)
        );
    }
    for d in p.decisions() {
        assert!(!d.reason.explanation().is_empty());
    }
}
#[test]
fn derives_coding_investigation_and_structured_data_needs() {
    let mut d = task().data().clone();
    d.task_type = SemanticTaskType::Modify;
    let coding = analyze(&SemanticTaskIR::new(d.clone()).unwrap());
    assert!(
        coding
            .requirements()
            .iter()
            .any(|r| r.class == ContextClass::DomainKnowledge
                && r.necessity == ContextNecessity::Optional)
    );
    d.task_type = SemanticTaskType::AnalyzePerformance;
    d.evidence_refs.push(pin(
        EvidenceId::new("e1").unwrap(),
        context_content_digest(b"evidence"),
    ));
    d.verification_contract
        .checks
        .push(SemanticVerificationCheck::EvidenceRequired);
    d.current_state = Some(pin(
        ObservedStateId::new("state").unwrap(),
        context_content_digest(b"state"),
    ));
    d.history
        .push(pin(id("history"), context_content_digest(b"history")));
    let r = analyze(&SemanticTaskIR::new(d.clone()).unwrap());
    for class in [
        ContextClass::Evidence,
        ContextClass::Memory,
        ContextClass::RuntimeState,
    ] {
        assert!(
            r.requirements()
                .iter()
                .any(|r| r.class == class && r.necessity.required())
        );
    }
    d.target.kind = TargetKind::Entity;
    assert!(
        analyze(&SemanticTaskIR::new(d).unwrap())
            .requirements()
            .iter()
            .any(|r| r.class == ContextClass::StructuredData)
    );
    let mut d = task().data().clone();
    d.task_type = SemanticTaskType::Create;
    assert!(
        !analyze(&SemanticTaskIR::new(d).unwrap())
            .requirements()
            .iter()
            .any(|r| r.class == ContextClass::SystemState)
    );
}
#[test]
fn completeness_references_and_exact_basis_are_prerequisites() {
    let t = task();
    for (deny_refs, deny_basis) in [(true, false), (false, true)] {
        assert_eq!(
            analyze_context_requirements(
                &t,
                input(&t),
                &Host {
                    deny_refs,
                    deny_basis
                }
            ),
            Err(ContextPlanningError::SemanticAdmission)
        );
    }
    for mutation in 0..4 {
        let mut i = input(&t);
        match mutation {
            0 => i.basis.task.id = TaskId::new("other").unwrap(),
            1 => i.basis.task.digest = context_content_digest(b"stale"),
            2 => i.basis.task.scope = ContextScopeId::new("other").unwrap(),
            _ => i.basis.task.contract_version = SchemaVersion::V2,
        }
        assert_eq!(
            analyze_context_requirements(
                &t,
                i,
                &Host {
                    deny_refs: false,
                    deny_basis: false
                }
            ),
            Err(ContextPlanningError::BindingMismatch)
        );
    }
}
#[test]
fn conditional_and_optional_needs_are_explicit_and_do_not_force_acquisition() {
    let mut r = requirement();
    r.necessity = ContextNecessity::Conditional {
        condition: id("when-observing"),
        active: false,
    };
    let req = single(r.clone());
    let s = source(
        "repo",
        ContextSourceClass::Repository,
        "target-state",
        TrustClass::RetrievedContent,
    );
    let p = plan(&req, std::slice::from_ref(&s));
    assert!(p.selected().is_empty());
    assert!(p.missing().is_empty());
    assert!(
        p.decisions()
            .iter()
            .any(|d| d.reason == ContextSelectionReason::Inactive)
    );
    r.necessity = ContextNecessity::Conditional {
        condition: id("when-observing"),
        active: true,
    };
    assert_eq!(plan(&single(r.clone()), &[]).missing().len(), 1);
    r.necessity = ContextNecessity::Optional;
    assert!(plan(&single(r), &[]).missing().is_empty());
}
#[test]
fn requirement_validation_rejects_contradictions_and_authority_laundering() {
    let original = requirement();
    let t = task();
    let make =
        |v, rs| ContextRequirementsIR::new(v, id("r1"), input(&t).basis, rs, TokenBudget(1000));
    assert_eq!(
        make(SchemaVersion::V2, vec![original.clone()]),
        Err(ContextPlanningError::InvalidRequirements)
    );
    assert_eq!(
        make(SchemaVersion::V1, vec![]),
        Err(ContextPlanningError::InvalidRequirements)
    );
    assert_eq!(
        make(SchemaVersion::V1, vec![original.clone(), original.clone()]),
        Err(ContextPlanningError::ConflictingRequirements)
    );
    for mutation in 0..8 {
        let mut r = original.clone();
        match mutation {
            0 => r.permitted_sources.clear(),
            1 => r.permitted_sources.push(r.permitted_sources[0]),
            2 => r.information.accepted_trust.clear(),
            3 => r
                .information
                .accepted_trust
                .insert(TrustClass::CanonicalReference)
                .then_some(())
                .unwrap(),
            4 => r.class = ContextClass::Authority,
            5 => r
                .permitted_sources
                .push(ContextSourceClass::CatalogAuthority),
            6 => {
                r.information.requirements = InformationRequirements::new(
                    FreshnessRequirement::Fresh,
                    Some(SensitivityClass::Secret),
                    vec![],
                    vec![],
                )
                .unwrap()
            }
            _ => r.budget.tokens = TokenBudget(0),
        }
        assert!(
            make(SchemaVersion::V1, vec![r]).is_err(),
            "mutation {mutation}"
        );
    }
    for situation in [false, true] {
        let mut b = input(&t).basis;
        if situation {
            b.situation.as_mut().unwrap().scope = ContextScopeId::new("other").unwrap();
        } else {
            b.plan.as_mut().unwrap().scope = ContextScopeId::new("other").unwrap();
        }
        assert_eq!(
            ContextRequirementsIR::new(
                SchemaVersion::V1,
                id("r1"),
                b,
                vec![original.clone()],
                TokenBudget(1000)
            ),
            Err(ContextPlanningError::ScopeMismatch)
        );
    }
}
#[test]
fn negative_source_metadata_fails_closed_with_stable_reasons() {
    use ContextSelectionReason as R;
    let r = requirement();
    let s = source(
        "repo",
        ContextSourceClass::Repository,
        "target-state",
        TrustClass::RetrievedContent,
    );
    let cases = [
        R::CrossScope,
        R::NotPermitted,
        R::NotRelevant,
        R::Unavailable,
        R::Untrusted,
        R::Stale,
        R::Conflicting,
        R::SensitivityRejected,
        R::EvidenceMissing,
        R::BudgetExceeded,
    ];
    let fixtures: Vec<String> = serde_json::from_str(include_str!(
        "../../../tests/fixtures/context-planning-v1/source-denials.json"
    ))
    .unwrap();
    for (reason, expected_code) in cases.into_iter().zip(fixtures) {
        assert_eq!(serde_json::to_value(reason).unwrap(), expected_code);
        let mut s = s.clone();
        let mut r = r.clone();
        match reason {
            R::CrossScope => s.pin.scope = ContextScopeId::new("other").unwrap(),
            R::NotPermitted => s.class = ContextSourceClass::Memory,
            R::NotRelevant => s.covers.clear(),
            R::Unavailable => s.available = false,
            R::Untrusted => s.quality = quality(TrustClass::SyntheticData),
            R::Stale => {
                s.quality = QualityMetadata::new(
                    TrustClass::RetrievedContent,
                    SensitivityClass::Normal,
                    Confidence::Unknown,
                    FreshnessStatus::Unknown,
                    Uncertainty::None,
                )
            }
            R::Conflicting => s.quality = s.quality.with_conflict(ConflictStatus::Unresolved),
            R::SensitivityRejected => r.information.maximum_sensitivity = SensitivityClass::Public,
            R::EvidenceMissing => {
                s.present = Some(ContextSatisfaction::Fragment(id("fragment")));
                r.information.requirements = InformationRequirements::new(
                    FreshnessRequirement::Fresh,
                    None,
                    vec![EvidenceId::new("missing").unwrap()],
                    vec![],
                )
                .unwrap()
            }
            R::BudgetExceeded => s.tokens = 1001,
            _ => unreachable!(),
        }
        assert_eq!(context_source_rejection(&r, &s, &scope()), Some(reason));
        let p = plan(&single(r), &[s]);
        assert_eq!(p.missing().len(), 1);
        assert!(p.decisions().iter().any(|d| d.reason == reason));
        assert!(!reason.explanation().is_empty());
    }
    let mut r = r.clone();
    r.stop = StopCondition::EvidenceSatisfied(NonZeroU64::MIN);
    let mut s = s.clone();
    s.present = Some(ContextSatisfaction::Fragment(id("fragment")));
    assert_eq!(
        context_source_rejection(&r, &s, &scope()),
        Some(R::EvidenceMissing)
    );
    s.evidence.insert(EvidenceId::new("e1").unwrap());
    assert_eq!(context_source_rejection(&r, &s, &scope()), None);
}
#[test]
fn present_context_wins_and_budgets_are_global_and_checked() {
    let r = single(requirement());
    let mut a = source(
        "a",
        ContextSourceClass::Repository,
        "target-state",
        TrustClass::RetrievedContent,
    );
    let mut b = a.clone();
    b.pin.id = id("b");
    b.present = Some(ContextSatisfaction::Fragment(id("fragment")));
    let p = plan(&r, &[a.clone(), b.clone()]);
    assert_eq!(p.selected()[&id("target-state")].pin.id, id("b"));
    assert!(
        p.decisions()
            .iter()
            .any(|d| d.reason == ContextSelectionReason::LowerPreference)
    );
    let mut r2 = requirement();
    r2.id = id("second");
    a.covers.insert(id("second"));
    a.tokens = 700;
    let requirements = ContextRequirementsIR::new(
        SchemaVersion::V1,
        id("r1"),
        r.basis().clone(),
        vec![requirement(), r2.clone()],
        TokenBudget(1000),
    )
    .unwrap();
    assert_eq!(plan(&requirements, &[a.clone()]).selected().len(), 2); // same source charged once
    let mut c = a.clone();
    c.pin.id = id("c");
    c.covers = BTreeSet::from([id("second")]);
    a.covers.remove(&id("second"));
    let p = plan(&requirements, &[a.clone(), c]);
    assert_eq!(p.selected().len(), 1);
    assert_eq!(p.missing().len(), 1);
    assert!(
        p.decisions()
            .iter()
            .any(|d| d.reason == ContextSelectionReason::BudgetExceeded)
    );
    assert_eq!(
        select_context_sources(&r, id("r1"), &[b.clone(), b]),
        Err(ContextPlanningError::ConflictingRequirements)
    );
}
#[test]
fn cg15_mapping_preserves_constraints_and_never_retrieves_present_context() {
    let r = single(requirement());
    let mut s = source(
        "repo",
        ContextSourceClass::Repository,
        "target-state",
        TrustClass::RetrievedContent,
    );
    let p = plan(&r, std::slice::from_ref(&s));
    let source = RetrievalSource {
        priority: 0,
        id: RetrievalSourceId::new("repo").unwrap(),
        kind: RetrievalSourceKind::Document,
        optional: false,
    };
    let strategy = RetrievalStrategy {
        priority: 0,
        id: RetrievalStrategyId::new("lexical").unwrap(),
        kind: RetrievalStrategyKind::Lexical,
        optional: false,
    };
    let support = RetrievalSupport {
        sources: BTreeSet::from([(source.id.clone(), source.kind)]),
        strategies: BTreeSet::from([(strategy.id.clone(), strategy.kind)]),
    };
    let map = |r: &ContextRequirementsIR,
               p: &ContextPlanIR,
               id: &ReferenceId,
               source: RetrievalSource,
               support: &RetrievalSupport| {
        context_retrieval_plan(
            r,
            p,
            id,
            ContextRetrievalBinding {
                source,
                strategy: strategy.clone(),
                support,
                provenance: ProvenanceId::new("prov").unwrap(),
                step: Some(PlanStepId::new("step").unwrap()),
            },
        )
    };
    let rp = map(&r, &p, &id("target-state"), source.clone(), &support).unwrap();
    let i = rp.request().input();
    assert_eq!(i.required, r.requirements()[0].information);
    assert_eq!(i.budget, r.requirements()[0].budget);
    assert_eq!(i.stop, Some(r.requirements()[0].stop));
    assert_eq!(i.scope, scope());
    assert!(
        i.queries
            .iter()
            .all(|q| q.0.as_str().contains("artifact-1")
                && q.0.as_str().contains("inspect-artifact"))
    );
    assert_eq!(
        i.situation,
        r.basis().situation.as_ref().map(|s| s.id.clone())
    );
    assert!(map(&r, &p, &id("absent"), source.clone(), &support).is_err());
    assert!(
        map(
            &r,
            &plan(&r, &[]),
            &id("target-state"),
            source.clone(),
            &support
        )
        .is_err()
    );
    let mut wrong = source.clone();
    wrong.id = RetrievalSourceId::new("other").unwrap();
    assert!(map(&r, &p, &id("target-state"), wrong, &support).is_err());
    let mut wrong = source.clone();
    wrong.kind = RetrievalSourceKind::Memory;
    assert!(map(&r, &p, &id("target-state"), wrong, &support).is_err());
    s.present = Some(ContextSatisfaction::Fragment(id("fragment")));
    assert!(
        map(
            &r,
            &plan(&r, &[s]),
            &id("target-state"),
            source.clone(),
            &support
        )
        .is_err()
    );
    let mut altered = requirement();
    altered.purpose = NonEmptyText::new("changed").unwrap();
    assert!(
        map(
            &single(altered),
            &p,
            &id("target-state"),
            source.clone(),
            &support
        )
        .is_err()
    );
    assert!(
        map(
            &r,
            &p,
            &id("target-state"),
            source,
            &RetrievalSupport {
                sources: BTreeSet::new(),
                strategies: BTreeSet::new()
            }
        )
        .is_err()
    );
}
#[test]
fn actual_cg10_handoff_preserves_canonical_records_and_exact_bindings() {
    let t = task();
    let r = analyze(&t);
    let s = sources(&t, &compiled(&[]));
    let p = plan(&r, &s);
    let f = fragment(&s[3]);
    let c = compiled(&[f]);
    let mut acquired = s[3].clone();
    acquired.present = Some(ContextSatisfaction::Fragment(id("fragment")));
    let h = handoff(
        t.clone(),
        r.clone(),
        p.clone(),
        c.clone(),
        context_pin(&c),
        &[acquired],
    )
    .unwrap();
    assert_eq!(h.task(), &t);
    assert_eq!(h.requirements(), &r);
    assert_eq!(h.plan(), &p);
    assert_eq!(h.context(), &c);
    let v = h.to_value();
    assert_eq!(v["planning"]["requirements"], r.to_value());
    assert_eq!(v["planning"]["plan"], p.to_value());
    assert_eq!(
        v["bindings"]["requirements"],
        serde_json::to_value(r.binding()).unwrap()
    );
    assert_eq!(
        v["bindings"]["plan"],
        serde_json::to_value(p.binding()).unwrap()
    );
    assert_eq!(v["planning"]["satisfaction"].as_array().unwrap().len(), 4);
    assert_eq!(
        v["output"],
        serde_json::to_value(&contracts(&t).output).unwrap()
    );
    assert_eq!(
        v["verification"],
        serde_json::to_value(&contracts(&t).verification).unwrap()
    );
    assert_eq!(
        v["planning"]["satisfaction"][0]["source"]["quality"],
        serde_json::to_value(s[0].quality).unwrap()
    );
    // Two pure offline layouts preserve the canonical payload and authority separation.
    let mut data = v.clone();
    let instructions = data
        .as_object_mut()
        .unwrap()
        .remove("instructions")
        .unwrap();
    let messages = json!({"messages":[{"role":"system","content":instructions.to_string()},
        {"role":"user","content":serde_json::to_string(&data).unwrap()}]});
    let blocks = json!({"system":instructions,"input":data});
    let mut reconstructed =
        serde_json::from_str::<Value>(messages["messages"][1]["content"].as_str().unwrap())
            .unwrap();
    assert_eq!(reconstructed, blocks["input"]);
    let message_instructions: Value =
        serde_json::from_str(messages["messages"][0]["content"].as_str().unwrap()).unwrap();
    assert_eq!(message_instructions, blocks["system"]);
    reconstructed["instructions"] = message_instructions;
    assert_eq!(reconstructed, v);
    assert_eq!(h.to_value(), v);
}
#[test]
fn handoff_rejects_missing_stale_cross_scope_and_unplanned_acquisition() {
    let t = task();
    let r = analyze(&t);
    let s = sources(&t, &compiled(&[]));
    let p = plan(&r, &s);
    let c = compiled(&[fragment(&s[3])]);
    let mut a = s[3].clone();
    a.present = Some(ContextSatisfaction::Fragment(id("fragment")));
    let run = |t, r, p, c: CompiledContext, pin, a: Vec<_>| handoff(t, r, p, c, pin, &a);
    assert_eq!(
        run(
            t.clone(),
            r.clone(),
            p.clone(),
            c.clone(),
            context_pin(&c),
            vec![]
        ),
        Err(ContextPlanningError::MissingMandatory)
    );
    for mutation in 0..10 {
        let mut a = a.clone();
        let mut cp = context_pin(&c);
        let mut t = t.clone();
        match mutation {
            0 => a.pin.digest = context_content_digest(b"stale"),
            1 => a.pin.scope = ContextScopeId::new("other").unwrap(),
            2 => a.present = Some(ContextSatisfaction::Fragment(id("missing"))),
            3 => a.present = None,
            4 => a.quality = quality(TrustClass::SyntheticData),
            5 => cp.digest = context_content_digest(b"stale"),
            6 => {
                let mut d = t.data().clone();
                d.goal.description = "changed".into();
                t = SemanticTaskIR::new(d).unwrap();
            }
            7 => a.tokens = 1001,
            8 => {
                a.quality = QualityMetadata::new(
                    TrustClass::RetrievedContent,
                    SensitivityClass::Normal,
                    Confidence::Unknown,
                    FreshnessStatus::Stale,
                    Uncertainty::None,
                )
            }
            _ => a.pin.revision = id("stale-revision"),
        }
        assert!(
            run(t, r.clone(), p.clone(), c.clone(), cp, vec![a]).is_err(),
            "mutation {mutation}"
        );
    }
    assert!(
        run(
            t.clone(),
            r.clone(),
            p.clone(),
            c.clone(),
            context_pin(&c),
            vec![a.clone(), a.clone()]
        )
        .is_err()
    );
    let mut wrong = s.clone();
    wrong[0].payload_digest = Some(context_content_digest(b"fake authority"));
    assert!(
        run(
            t.clone(),
            r.clone(),
            plan(&r, &wrong),
            c.clone(),
            context_pin(&c),
            vec![a.clone()]
        )
        .is_err()
    );
    let mut cp = context_pin(&c);
    cp.scope = ContextScopeId::new("other").unwrap();
    let value: Value = serde_json::from_str(&c.to_json().unwrap()).unwrap();
    cp = binding("compiled-context", &cp.revision, &cp.scope, &value);
    assert_eq!(
        run(t, r, p, c, cp, vec![a]),
        Err(ContextPlanningError::ScopeMismatch)
    );
}

#[test]
fn reference_task_examples_are_analyzed_without_acquisition() {
    for (name, text, expected) in [
        (
            "coding",
            include_str!("../../../examples/context-planning-v1/coding.json"),
            ContextClass::DomainKnowledge,
        ),
        (
            "investigation",
            include_str!("../../../examples/context-planning-v1/investigation.json"),
            ContextClass::Evidence,
        ),
        (
            "structured-data",
            include_str!("../../../examples/context-planning-v1/structured-data.json"),
            ContextClass::StructuredData,
        ),
    ] {
        let task = SemanticTaskIR::from_json(text).unwrap();
        let r = analyze(&task);
        assert!(
            r.requirements().iter().any(|r| r.class == expected),
            "{name}"
        );
        assert_eq!(r.to_value()["schema_version"], "1.0");
        assert!(!plan(&r, &[]).missing().is_empty());
    }
}
#[test]
fn existing_cg07_planning_inputs_keep_constraints_and_lineage() {
    let kinds = [
        (
            PlanningInputKind::EvidenceAcquisition,
            RequiredOutcomeKind::EvidenceAcquisition,
        ),
        (
            PlanningInputKind::ConflictResolution,
            RequiredOutcomeKind::ConflictResolution,
        ),
        (
            PlanningInputKind::Observation,
            RequiredOutcomeKind::Observation,
        ),
        (
            PlanningInputKind::InputAcquisition,
            RequiredOutcomeKind::InputAcquisition,
        ),
        (
            PlanningInputKind::Normalization,
            RequiredOutcomeKind::Assessment,
        ),
    ];
    let t = task();
    let mut i = input(&t);
    for (n, (kind, outcome)) in kinds.into_iter().enumerate() {
        i.planning_inputs.push(
            PlanningInput::new(
                PlanningInputId::new(format!("input-{n}")).unwrap(),
                DesiredStateId::new("desired").unwrap(),
                DeltaId::new("delta").unwrap(),
                DeltaItemId::new(format!("item-{n}")).unwrap(),
                ConditionId::new(format!("condition-{n}")).unwrap(),
                kind,
                DeltaReasonCode::IncompleteInformation,
                RequiredOutcome::new(outcome, "Resolve the declared information gap").unwrap(),
                PlanningInputCompletion::EvidenceAvailable,
                PlanningInputVerification::EvidenceValidated,
                InformationRequirements::new(
                    FreshnessRequirement::Fresh,
                    Some(SensitivityClass::Internal),
                    vec![EvidenceId::new("e1").unwrap()],
                    vec![ProvenanceId::new("p1").unwrap()],
                )
                .unwrap(),
                "Delta-derived need",
            )
            .unwrap(),
        );
    }
    let r = analyze_context_requirements(
        &t,
        i,
        &Host {
            deny_refs: false,
            deny_basis: false,
        },
    )
    .unwrap();
    let r = r
        .requirements()
        .iter()
        .find(|r| r.id == id("planning-input-0"))
        .unwrap();
    assert_eq!(
        r.information.requirements.evidence(),
        &[EvidenceId::new("e1").unwrap()]
    );
    assert_eq!(
        r.information.requirements.provenances(),
        &[ProvenanceId::new("p1").unwrap()]
    );
    assert_eq!(
        r.information.requirements.minimum_sensitivity(),
        Some(SensitivityClass::Internal)
    );
    let mut i = input(&t);
    i.planning_inputs.push(
        PlanningInput::new(
            PlanningInputId::new("input").unwrap(),
            DesiredStateId::new("desired").unwrap(),
            DeltaId::new("delta").unwrap(),
            DeltaItemId::new("item").unwrap(),
            ConditionId::new("condition").unwrap(),
            PlanningInputKind::Observation,
            DeltaReasonCode::SubjectNotObserved,
            RequiredOutcome::new(RequiredOutcomeKind::Observation, "Observe target").unwrap(),
            PlanningInputCompletion::StateObserved,
            PlanningInputVerification::ObservationNormalized,
            InformationRequirements::new(FreshnessRequirement::Fresh, None, vec![], vec![])
                .unwrap(),
            "need state",
        )
        .unwrap(),
    );
    i.basis.plan = None;
    assert_eq!(
        analyze_context_requirements(
            &t,
            i,
            &Host {
                deny_refs: false,
                deny_basis: false
            }
        ),
        Err(ContextPlanningError::InvalidRequirements)
    );
}
#[test]
fn every_reason_is_explainable_and_information_bounds_are_enforced() {
    use ContextSelectionReason as R;
    for reason in [
        R::SelectedPresent,
        R::SelectedAcquisition,
        R::NoneRequired,
        R::NotPermitted,
        R::NotRelevant,
        R::Unavailable,
        R::CrossScope,
        R::Untrusted,
        R::Stale,
        R::Conflicting,
        R::EvidenceMissing,
        R::SensitivityRejected,
        R::BudgetExceeded,
        R::LowerPreference,
        R::Inactive,
    ] {
        assert!(!reason.explanation().is_empty());
    }
    let mut r = requirement();
    let mut s = source(
        "repo",
        ContextSourceClass::Repository,
        "target-state",
        TrustClass::RetrievedContent,
    );
    s.present = Some(ContextSatisfaction::Fragment(id("fragment")));
    r.information.requirements = InformationRequirements::new(
        FreshnessRequirement::NotSpecified,
        Some(SensitivityClass::Internal),
        vec![],
        vec![ProvenanceId::new("prov").unwrap()],
    )
    .unwrap();
    assert_eq!(
        context_source_rejection(&r, &s, &scope()),
        Some(R::SensitivityRejected)
    );
    s.quality = QualityMetadata::new(
        TrustClass::RetrievedContent,
        SensitivityClass::Internal,
        Confidence::Unknown,
        FreshnessStatus::Unknown,
        Uncertainty::None,
    );
    assert_eq!(
        context_source_rejection(&r, &s, &scope()),
        Some(R::EvidenceMissing)
    );
    s.provenances.insert(ProvenanceId::new("prov").unwrap());
    assert_eq!(context_source_rejection(&r, &s, &scope()), None);
    r.budget.context =
        ContextBudget::new(TokenBudget(1000), std::collections::BTreeMap::new()).unwrap();
    assert_eq!(
        context_source_rejection(&r, &s, &scope()),
        Some(R::BudgetExceeded)
    );
    r.budget.context =
        ContextBudget::new(TokenBudget(0), std::collections::BTreeMap::new()).unwrap();
    assert!(
        ContextRequirementsIR::new(
            SchemaVersion::V1,
            id("r1"),
            input(&task()).basis,
            vec![r],
            TokenBudget(1000)
        )
        .is_err()
    );
}
#[test]
fn plan_construction_rejects_forged_selections_and_missing_sets() {
    let r = single(requirement());
    let s = source(
        "repo",
        ContextSourceClass::Repository,
        "target-state",
        TrustClass::RetrievedContent,
    );
    let p = plan(&r, std::slice::from_ref(&s));
    let create = |selected, decisions, missing| {
        ContextPlanIR::new(id("r1"), &r, selected, decisions, missing)
    };
    assert!(create(p.selected().clone(), vec![], BTreeSet::new()).is_err());
    assert!(
        create(
            p.selected().clone(),
            p.decisions().to_vec(),
            BTreeSet::from([id("target-state")])
        )
        .is_err()
    );
    assert!(
        create(
            std::collections::BTreeMap::from([(id("unknown"), s.clone())]),
            p.decisions().to_vec(),
            BTreeSet::new()
        )
        .is_err()
    );
    let mut invalid = s.clone();
    invalid.available = false;
    assert!(
        create(
            std::collections::BTreeMap::from([(id("target-state"), invalid)]),
            p.decisions().to_vec(),
            BTreeSet::new()
        )
        .is_err()
    );
    let mut r2 = requirement();
    r2.id = id("r2");
    let rs = ContextRequirementsIR::new(
        SchemaVersion::V1,
        id("r1"),
        r.basis().clone(),
        vec![requirement(), r2],
        TokenBudget(5),
    )
    .unwrap();
    assert!(
        ContextPlanIR::new(
            id("r1"),
            &rs,
            p.selected().clone(),
            p.decisions().to_vec(),
            BTreeSet::from([id("r2")])
        )
        .is_err()
    );
    let mut original = s.clone();
    original.covers.insert(id("r2"));
    let mut changed = original.clone();
    changed.tokens = 11;
    let selections =
        std::collections::BTreeMap::from([(id("target-state"), original), (id("r2"), changed)]);
    assert_eq!(
        ContextPlanIR::new(
            id("r1"),
            &rs,
            selections,
            p.decisions().to_vec(),
            BTreeSet::new()
        ),
        Err(ContextPlanningError::InvalidRequirements)
    );
}
#[test]
fn memory_mapping_and_unsupported_tool_acquisition_stay_separate() {
    let mut req = requirement();
    req.class = ContextClass::Memory;
    req.permitted_sources = vec![ContextSourceClass::Memory];
    req.information.accepted_trust = BTreeSet::from([TrustClass::DerivedAssessment]);
    let r = single(req);
    let s = source(
        "memory",
        ContextSourceClass::Memory,
        "target-state",
        TrustClass::DerivedAssessment,
    );
    let p = plan(&r, &[s]);
    let source = RetrievalSource {
        priority: 0,
        id: RetrievalSourceId::new("memory").unwrap(),
        kind: RetrievalSourceKind::Memory,
        optional: false,
    };
    let strategy = RetrievalStrategy {
        priority: 0,
        id: RetrievalStrategyId::new("recall").unwrap(),
        kind: RetrievalStrategyKind::MemoryRecall,
        optional: false,
    };
    let support = RetrievalSupport {
        sources: BTreeSet::from([(source.id.clone(), source.kind)]),
        strategies: BTreeSet::from([(strategy.id.clone(), strategy.kind)]),
    };
    assert_eq!(
        context_retrieval_plan(
            &r,
            &p,
            &id("target-state"),
            ContextRetrievalBinding {
                source,
                strategy,
                support: &support,
                provenance: ProvenanceId::new("prov").unwrap(),
                step: None
            }
        )
        .unwrap()
        .request()
        .input()
        .purpose,
        RetrievalPurpose::MemoryRecall
    );
    let mut req = requirement();
    req.permitted_sources = vec![ContextSourceClass::ToolsMcp];
    let r = single(req);
    let p = plan(&r, &[source_metadata_for_tool()]);
    let source = RetrievalSource {
        priority: 0,
        id: RetrievalSourceId::new("tool").unwrap(),
        kind: RetrievalSourceKind::Document,
        optional: false,
    };
    let strategy = RetrievalStrategy {
        priority: 0,
        id: RetrievalStrategyId::new("lexical").unwrap(),
        kind: RetrievalStrategyKind::Lexical,
        optional: false,
    };
    assert_eq!(
        context_retrieval_plan(
            &r,
            &p,
            &id("target-state"),
            ContextRetrievalBinding {
                source,
                strategy,
                support: &support,
                provenance: ProvenanceId::new("prov").unwrap(),
                step: None
            }
        ),
        Err(RetrievalError::UnsupportedSource)
    );
}
fn source_metadata_for_tool() -> ContextSourceMetadata {
    source(
        "tool",
        ContextSourceClass::ToolsMcp,
        "target-state",
        TrustClass::RetrievedContent,
    )
}
#[test]
fn handoff_cannot_include_unplanned_context_or_promote_data_to_authority() {
    let t = task();
    let r = analyze(&t);
    let mut s = sources(&t, &compiled(&[]));
    let mut a = s[3].clone();
    a.present = Some(ContextSatisfaction::Fragment(id("fragment")));
    s[3] = a.clone();
    let p = plan(&r, &s);
    let mut extra_source = a.clone();
    extra_source.pin.id = id("unplanned");
    let extra = ContextFragment::external(
        id("extra"),
        FragmentKind::Knowledge,
        "extra",
        FragmentMetadata {
            provenance: KnowledgeProvenance::new("unplanned", Some("r1")).unwrap(),
            evidence: BTreeSet::new(),
            quality: quality(TrustClass::RetrievedContent),
            rationale: NonEmptyText::new("extra").unwrap(),
            validation: None,
        },
        scope(),
        PlanStepId::new("step").unwrap(),
    )
    .unwrap();
    let c = compiled(&[fragment(&a), extra]);
    assert_eq!(
        handoff(t.clone(), r.clone(), p, c.clone(), context_pin(&c), &[]),
        Err(ContextPlanningError::InvalidSatisfaction)
    );
    s[0].present = Some(ContextSatisfaction::Fragment(id("fragment")));
    let p = plan(&r, &s);
    let c = compiled(&[fragment(&a)]);
    assert_eq!(
        handoff(t, r, p, c.clone(), context_pin(&c), &[]),
        Err(ContextPlanningError::InvalidSatisfaction)
    );
}

#[test]
fn cg10_preserves_evidence_memory_and_optional_or_inactive_absence() {
    for class in [
        ContextClass::Evidence,
        ContextClass::Memory,
        ContextClass::RuntimeState,
        ContextClass::DomainKnowledge,
    ] {
        let mut r = requirement();
        r.class = class;
        let (source_class, trust, kind) = match class {
            ContextClass::Memory => (
                ContextSourceClass::Memory,
                TrustClass::DerivedAssessment,
                FragmentKind::Memory,
            ),
            ContextClass::DomainKnowledge => (
                ContextSourceClass::Rag,
                TrustClass::RetrievedContent,
                FragmentKind::Knowledge,
            ),
            _ => (
                ContextSourceClass::SituationState,
                TrustClass::ObservedEvidence,
                FragmentKind::Evidence,
            ),
        };
        r.permitted_sources = vec![source_class];
        r.information.accepted_trust = BTreeSet::from([trust]);
        r.information.requirements = InformationRequirements::new(
            FreshnessRequirement::Fresh,
            None,
            vec![EvidenceId::new("e1").unwrap()],
            vec![ProvenanceId::new("p1").unwrap()],
        )
        .unwrap();
        let r = single(r);
        let mut s = source("source", source_class, "target-state", trust);
        s.evidence.insert(EvidenceId::new("e1").unwrap());
        s.provenances.insert(ProvenanceId::new("p1").unwrap());
        s.present = Some(ContextSatisfaction::Fragment(id("fragment")));
        let f = ContextFragment::external(
            id("fragment"),
            kind,
            "properties",
            FragmentMetadata {
                provenance: KnowledgeProvenance::new("source", Some("r1")).unwrap(),
                evidence: BTreeSet::from([id("e1"), id("p1")]),
                quality: s.quality,
                rationale: NonEmptyText::new("planned context").unwrap(),
                validation: Some(id("validation")),
            },
            scope(),
            PlanStepId::new("step").unwrap(),
        )
        .unwrap();
        let c = compiled(&[f]);
        let p = plan(&r, &[s]);
        let h = handoff(task(), r, p, c.clone(), context_pin(&c), &[]).unwrap();
        assert_eq!(
            h.to_value()["planning"]["satisfaction"][0]["link"]["id"],
            "fragment"
        );
    }
    for necessity in [
        ContextNecessity::Optional,
        ContextNecessity::Conditional {
            condition: id("condition"),
            active: false,
        },
    ] {
        let mut r = requirement();
        r.necessity = necessity;
        let r = single(r);
        let p = plan(&r, &[]);
        let c = compiled(&[]);
        assert!(handoff(task(), r, p, c.clone(), context_pin(&c), &[]).is_ok());
    }
}

fn contracts(t: &SemanticTaskIR) -> ContextHandoffContracts {
    use gateway_domain::semantic_output::*;
    ContextHandoffContracts {
        output: OutputContract {
            schema_version: SchemaVersion::V1,
            schema: t.data().output_contract.schema.clone(),
            shape: OutputType::String {},
        },
        verification: VerificationContract {
            schema_version: SchemaVersion::V1,
            rules: t
                .data()
                .verification_contract
                .checks
                .iter()
                .map(|check| VerificationRule {
                    check: *check,
                    rule: pin(
                        id(&format!("rule-{check:?}")),
                        context_content_digest(b"rule"),
                    ),
                    verifier: pin(id("verifier"), context_content_digest(b"verifier")),
                })
                .collect(),
        },
        verification_revision: id("r1"),
    }
}
fn handoff(
    t: SemanticTaskIR,
    r: ContextRequirementsIR,
    p: ContextPlanIR,
    c: CompiledContext,
    cp: ResolvedReference<ReferenceId>,
    a: &[ContextSourceMetadata],
) -> Result<PlannedContextHandoff, ContextPlanningError> {
    let contracts = contracts(&t);
    ContextCompiler::handoff_planned_context(t, r, p, c, cp, a, contracts)
}

#[test]
fn resolved_output_or_verification_mismatch_blocks_the_producer_handoff() {
    let t = task();
    let r = analyze(&t);
    let s = sources(&t, &compiled(&[]));
    let p = plan(&r, &s);
    let c = compiled(&[fragment(&s[3])]);
    let mut a = s[3].clone();
    a.present = Some(ContextSatisfaction::Fragment(id("fragment")));
    let mut wrong = contracts(&t);
    wrong.verification.rules.clear();
    assert_eq!(
        ContextCompiler::handoff_planned_context(t, r, p, c.clone(), context_pin(&c), &[a], wrong),
        Err(ContextPlanningError::BindingMismatch)
    );
}

#[test]
fn optional_context_never_spends_a_mandatory_reservation_first() {
    let mut mandatory = requirement();
    mandatory.priority = 100;
    let mut optional = mandatory.clone();
    optional.id = id("optional");
    optional.necessity = ContextNecessity::Optional;
    optional.priority = 0;
    let r = ContextRequirementsIR::new(
        SchemaVersion::V1,
        id("r1"),
        input(&task()).basis,
        vec![optional, mandatory],
        TokenBudget(10),
    )
    .unwrap();
    let mut a = source(
        "optional",
        ContextSourceClass::Repository,
        "optional",
        TrustClass::RetrievedContent,
    );
    a.tokens = 10;
    let b = source(
        "mandatory",
        ContextSourceClass::Repository,
        "target-state",
        TrustClass::RetrievedContent,
    );
    let p = plan(&r, &[a, b]);
    assert!(p.selected().contains_key(&id("target-state")));
    assert!(!p.selected().contains_key(&id("optional")));
    assert!(p.missing().is_empty());
}

#[test]
fn discovery_planning_does_not_require_retrieved_payloads_or_evidence_in_advance() {
    let mut need = requirement();
    need.class = ContextClass::Evidence;
    need.permitted_sources = vec![ContextSourceClass::Rag];
    need.information.accepted_trust = BTreeSet::from([TrustClass::ObservedEvidence]);
    need.stop = StopCondition::EvidenceSatisfied(NonZeroU64::MIN);
    let r = single(need);
    let mut pending = source(
        "evidence-index",
        ContextSourceClass::Rag,
        "target-state",
        TrustClass::ObservedEvidence,
    );
    pending.pin.digest = context_content_digest(b"captured-index-snapshot");
    pending.payload_digest = None;
    let p = plan(&r, std::slice::from_ref(&pending));
    assert!(p.missing().is_empty());
    assert_eq!(p.selected()[&id("target-state")].present, None);
    let empty = compiled(&[]);
    assert_eq!(
        handoff(
            task(),
            r.clone(),
            p.clone(),
            empty.clone(),
            context_pin(&empty),
            &[]
        ),
        Err(ContextPlanningError::MissingMandatory)
    );
    let mut actual = pending.clone();
    actual.payload_digest = Some(context_content_digest(b"properties"));
    actual
        .evidence
        .insert(EvidenceId::new("new-evidence").unwrap());
    actual.present = Some(ContextSatisfaction::Fragment(id("discovered")));
    let f = ContextFragment::external(
        id("discovered"),
        FragmentKind::Evidence,
        "properties",
        FragmentMetadata {
            provenance: KnowledgeProvenance::new(actual.pin.id.as_str(), Some("r1")).unwrap(),
            evidence: BTreeSet::from([id("new-evidence")]),
            quality: actual.quality,
            rationale: NonEmptyText::new("bounded discovered evidence").unwrap(),
            validation: Some(id("verified")),
        },
        scope(),
        PlanStepId::new("step").unwrap(),
    )
    .unwrap();
    let c = compiled(&[f]);
    let mut unproven = actual.clone();
    unproven.evidence.clear();
    assert_eq!(
        handoff(
            task(),
            r.clone(),
            p.clone(),
            c.clone(),
            context_pin(&c),
            &[unproven]
        ),
        Err(ContextPlanningError::InvalidSatisfaction)
    );
    let h = handoff(task(), r, p, c.clone(), context_pin(&c), &[actual]).unwrap();
    assert_eq!(h.plan().selected()[&id("target-state")].pin, pending.pin);
    assert_eq!(
        h.to_value()["planning"]["satisfaction"][0]["source"]["evidence"],
        json!(["new-evidence"])
    );
}
