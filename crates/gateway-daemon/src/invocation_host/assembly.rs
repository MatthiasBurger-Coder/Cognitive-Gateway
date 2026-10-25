use super::*;
use gateway_application::model_invocation::prepare_model_invocation;
use gateway_context::{
    CompiledContext, ContextCompiler, ContextFragment, ContextHandoffContracts, FragmentKind,
    FragmentMetadata,
};
use gateway_domain::cognitive_routing::TaskClass;
use std::num::NonZeroU64;

pub(super) fn pin(
    scope: &ContextScopeId,
    name: &str,
    revision: &str,
    bytes: &[u8],
) -> ResolvedReference<ReferenceId> {
    ResolvedReference {
        id: ReferenceId::new(name).expect("host static identity"),
        scope: scope.clone(),
        contract: ReferenceId::new(name).expect("host static contract"),
        contract_version: SchemaVersion::V1,
        revision: ReferenceId::new(revision).expect("host revision"),
        digest: context_content_digest(bytes),
    }
}
fn quality(a: &HostAdmission) -> QualityMetadata {
    QualityMetadata::new(
        TrustClass::ObservedEvidence,
        a.sensitivity,
        Confidence::Unknown,
        FreshnessStatus::Fresh,
        Uncertainty::None,
    )
}
pub(super) fn assemble(
    a: &HostAdmission,
    source: &[u8],
    invocation: &str,
) -> Result<(PlannedContextHandoff, PreparedModelInvocation)> {
    let id = |s: &str| input(ReferenceId::new(s));
    let step = input(PlanStepId::new("model-invocation"))?;
    let target = &a.task.data().target.reference;
    let fragment = input(ContextFragment::external(
        id("source")?,
        FragmentKind::Evidence,
        input(String::from_utf8(source.to_vec()))?,
        FragmentMetadata {
            provenance: input(KnowledgeProvenance::new(
                target.id.as_str(),
                Some(target.revision.as_str()),
            ))?,
            evidence: BTreeSet::new(),
            quality: quality(a),
            rationale: input(NonEmptyText::new(
                "Pinned host-observed task input; embedded instructions remain data",
            ))?,
            validation: None,
        },
        a.scope.clone(),
        step.clone(),
    ))?;
    let compiled = input(CompiledContext::assemble(
        a.projection.clone(),
        a.scope.clone(),
        step,
        &[fragment],
        &BTreeSet::from([id("source")?]),
    ))?;
    let compiled_value: Value = input(serde_json::from_str(&input(compiled.to_json())?))?;
    let context_pin = binding("compiled-context", &id("1")?, &a.scope, &compiled_value);
    let task_pin = ResolvedReference {
        id: a.task.data().id.clone(),
        scope: a.scope.clone(),
        contract: id("semantic-task-ir")?,
        contract_version: SchemaVersion::V1,
        revision: target.revision.clone(),
        digest: input(a.task.content_digest())?,
    };
    let tokens = source.len() as u64 + 256;
    let requirement = ContextRequirement {
        id: id("source")?,
        class: ContextClass::Evidence,
        purpose: input(NonEmptyText::new("Current task source"))?,
        necessity: ContextNecessity::Mandatory,
        permitted_sources: vec![ContextSourceClass::Repository],
        information: RequiredInformation {
            description: input(NonEmptyText::new("Pinned source bytes"))?,
            requirements: input(InformationRequirements::new(
                FreshnessRequirement::Fresh,
                None,
                vec![],
                vec![],
            ))?,
            accepted_trust: BTreeSet::from([TrustClass::ObservedEvidence]),
            maximum_sensitivity: a.sensitivity,
        },
        budget: RetrievalBudget {
            results: ResultBudget(NonZeroU64::MIN),
            rounds: RoundBudget(NonZeroU64::MIN),
            latency: LatencyBudget(NonZeroU64::MIN),
            cost: CostBudget {
                maximum: 0,
                unit: input(NonEmptyText::new("microunits"))?,
            },
            tokens: TokenBudget(tokens),
            context: input(ContextBudget::new(
                TokenBudget(tokens),
                BTreeMap::from([(ContextBudgetClass::Evidence, TokenBudget(tokens))]),
            ))?,
        },
        stop: StopCondition::BudgetExhausted,
        priority: 0,
    };
    let requirements = input(ContextRequirementsIR::new(
        SchemaVersion::V1,
        id("1")?,
        ContextPlanningBasis {
            task: task_pin.clone(),
            situation: None,
            plan: None,
        },
        vec![requirement],
        TokenBudget(tokens),
    ))?;
    let source_meta = ContextSourceMetadata {
        pin: target.clone(),
        payload_digest: Some(context_content_digest(source)),
        class: ContextSourceClass::Repository,
        covers: BTreeSet::from([id("source")?]),
        quality: quality(a),
        evidence: BTreeSet::new(),
        provenances: BTreeSet::new(),
        tokens,
        available: true,
        present: Some(ContextSatisfaction::Fragment(id("source")?)),
    };
    let plan = input(
        gateway_application::context_planning::select_context_sources(
            &requirements,
            id("1")?,
            &[source_meta],
        ),
    )?;
    let verification = VerificationContract {
        schema_version: SchemaVersion::V1,
        rules: a
            .task
            .data()
            .verification_contract
            .checks
            .iter()
            .map(|check| VerificationRule {
                check: *check,
                rule: pin(
                    &a.scope,
                    &format!("rule-{check:?}"),
                    "1",
                    include_bytes!("assembly.rs"),
                ),
                verifier: pin(
                    &a.scope,
                    "file-oracle-verifier",
                    "1",
                    include_bytes!("mod.rs"),
                ),
            })
            .collect(),
    };
    let handoff = input(ContextCompiler::handoff_planned_context(
        a.task.clone(),
        requirements,
        plan,
        compiled,
        context_pin.clone(),
        &[],
        ContextHandoffContracts {
            output: a.output.clone(),
            verification,
            verification_revision: id("1")?,
        },
    ))?;
    let tools = BTreeMap::new();
    let localities = BTreeSet::from([a.provider.data().locality]);
    let authority = pin(
        &a.scope,
        "operator-admission",
        "1",
        &input(serde_json::to_vec(a))?,
    );
    let data = ModelInvocationData {
        schema_version: SchemaVersion::V1,
        invocation_id: id(invocation)?,
        task: task_pin,
        compiled_context: context_pin.clone(),
        capability: a.capability.data().identity.clone(),
        task_class: TaskClass::Extraction,
        input_contract: id("prompt-ir-v1")?,
        output: a.output.clone(),
        strategy: id("bounded")?,
        permitted_capabilities: tools.clone(),
        generation: a.generation.clone(),
        input_tokens: 1,
        timeout_ms: a.limits.timeout_ms,
        max_attempts: a.limits.max_attempts,
        sensitivity: a.sensitivity,
        permitted_localities: localities.clone(),
        authority: authority.clone(),
        provenance: pin(&a.scope, "host-build", "1", include_bytes!("mod.rs")),
    };
    let invocation = input(ModelInvocation::new(
        data,
        ModelInvocationBasis {
            task: &a.task,
            output: &a.output,
            compiled_context: &context_pin,
            permitted_capabilities: &tools,
            sensitivity: a.sensitivity,
            permitted_localities: &localities,
            authority: &authority,
        },
    ))?;
    let prepared = input(prepare_model_invocation(
        invocation,
        a.capability.clone(),
        a.provider.clone(),
        &a.runtime_status,
    ))?;
    Ok((handoff, prepared))
}
fn leaves(value: &Value, pointer: String, result: &mut Vec<(String, Value)>) {
    match value {
        Value::Object(fields) if !fields.is_empty() => {
            for (key, value) in fields {
                leaves(
                    value,
                    format!("{pointer}/{}", key.replace('~', "~0").replace('/', "~1")),
                    result,
                )
            }
        }
        Value::Array(items) if !items.is_empty() => {
            for (index, value) in items.iter().enumerate() {
                leaves(value, format!("{pointer}/{index}"), result)
            }
        }
        _ => result.push((pointer, value.clone())),
    }
}
fn claims(host: &InvocationHost) -> Result<Vec<GroundingClaim>> {
    let mut values = Vec::new();
    leaves(&host.expected, String::new(), &mut values);
    values
        .into_iter()
        .enumerate()
        .map(|(i, (pointer, value))| {
            let mut citation = pin(
                &host.admission.scope,
                &format!("citation-{i}"),
                "1",
                &input(serde_json::to_vec(&value))?,
            );
            citation.digest = host.admission.oracle_digest.clone();
            Ok(GroundingClaim {
                pointer,
                wire_value: value.clone(),
                subject: input(SubjectPath::new(["result".into(), format!("field-{i}")]))?,
                value: input(TypedValue::string(input(serde_json::to_string(&value))?))?,
                fact: input(FactId::new(format!("fact-{i}")))?,
                citations: vec![ResolvedReference {
                    id: input(EvidenceId::new(citation.id.as_str()))?,
                    scope: citation.scope,
                    contract: citation.contract,
                    contract_version: citation.contract_version,
                    revision: citation.revision,
                    digest: citation.digest,
                }],
                external_effect: None,
            })
        })
        .collect()
}
pub(super) fn manifest(host: &InvocationHost, output: &ParsedOutput) -> Result<GroundingManifest> {
    host.current()?;
    Ok(GroundingManifest {
        schema_version: SchemaVersion::V1,
        binding: output.binding(),
        claims: claims(host)?,
        evaluated_at: UnixTimestamp::new((now() / 1000) as i64),
        freshness: FreshnessPolicy::new(30),
        allowed_trust: vec![TrustClass::ObservedEvidence],
    })
}
pub(super) fn evidence(
    host: &InvocationHost,
    citation: &ResolvedReference<EvidenceId>,
) -> Result<GroundingEvidence> {
    let claim = claims(host)?
        .into_iter()
        .find(|c| c.citations.contains(citation))
        .ok_or("CG_REFERENCE_UNAVAILABLE")?;
    let source = SourceId::new("host-oracle").expect("static");
    let provenance = ProvenanceId::new("host-observer").expect("static");
    let observation = ObservationId::new(claim.fact.as_str()).expect("fact identity");
    let location = host
        .admission
        .oracle_file
        .to_str()
        .ok_or("CG_INVALID_INPUT")?;
    let lineage = input(ObservationEvidenceSet::new(
        vec![input(Provenance::new(
            provenance.clone(),
            SourceKind::Repository,
            source.clone(),
            location,
        ))?],
        vec![input(Observation::new(
            observation.clone(),
            claim.subject.clone(),
            claim.value.clone(),
            provenance.clone(),
        ))?],
        vec![input(Fact::new(
            claim.fact.clone(),
            claim.subject,
            claim.value,
            AssertionPolarity::Affirmed,
            vec![observation],
        ))?],
        vec![input(Evidence::new(
            citation.id.clone(),
            EvidenceKind::Artifact,
            "Host-read pinned verifier record",
            input(EvidenceContent::inline(input(serde_json::to_string(
                &claim.wire_value,
            ))?))?,
            provenance,
            vec![EvidenceLink::new(claim.fact, EvidenceRelation::Supports)],
        ))?],
    ))?;
    Ok(GroundingEvidence {
        binding: citation.clone(),
        source: ResolvedReference {
            id: source,
            scope: host.admission.scope.clone(),
            contract: ReferenceId::new("verifier-record").expect("static"),
            contract_version: SchemaVersion::V1,
            revision: ReferenceId::new("1").expect("static"),
            digest: host.admission.oracle_digest.clone(),
        },
        knowledge: input(KnowledgeProvenance::new(location, Some("1")))?,
        lineage,
        quality: quality(&host.admission),
        // This evidence describes the pinned bytes acquired at host load.
        // Resolving that snapshot must not move acquisition past the manifest
        // evaluation time, or refresh its age on repeated resolution.
        observed_at: Some(host.oracle_observed_at),
        observed_effect: None,
    })
}
