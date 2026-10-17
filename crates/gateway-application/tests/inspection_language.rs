use gateway_application::{completeness::*, inspection_language::*, interpretation::*};
use gateway_domain::{interpretation::*, *};
use sha2::{Digest, Sha256};
use std::cell::Cell;

fn id(s: &str) -> ReferenceId {
    ReferenceId::new(s).unwrap()
}
fn pin(s: &str) -> InterpretationPin {
    InterpretationPin {
        id: id(s),
        scope: ContextScopeId::new("project-x").unwrap(),
        contract: id("captured-record"),
        contract_version: SchemaVersion::V1,
        revision: id("7"),
        digest: ContentDigest::new("a".repeat(64)).unwrap(),
    }
}
fn expected() -> SemanticTaskIR {
    SemanticTaskIR::from_json(include_str!(
        "../../../tests/fixtures/semantic-task-v1/minimal.canonical.json"
    ))
    .unwrap()
}
fn fact(name: &str, target: SemanticTarget) -> InterpretationFact {
    InterpretationFact {
        id: id(name),
        field: "target".parse().unwrap(),
        tier: InterpretationTier::ConversationFocus,
        value: InterpretationValue::Target(target),
        source: pin("focus"),
        provenance: Provenance::new(
            ProvenanceId::new(name).unwrap(),
            SourceKind::Caller,
            SourceId::new("focus").unwrap(),
            name,
        )
        .unwrap(),
        quality: QualityMetadata::new(
            TrustClass::CanonicalReference,
            SensitivityClass::Normal,
            Confidence::Unknown,
            FreshnessStatus::Fresh,
            Uncertainty::None,
        ),
        valid_until: UnixTimestamp::new(200),
        relevance: NonEmptyText::new("current artifact").unwrap(),
        span: None,
        subject: None,
        protected: false,
        required_pin: false,
        supersedes: None,
    }
}
struct Host {
    text: String,
    denied: Cell<bool>,
    reject_reference: bool,
    reject_manifest: bool,
    reject_output: bool,
    change_on_lookup: bool,
}
impl Host {
    fn new(text: &str) -> Self {
        Self {
            text: text.into(),
            denied: Cell::new(false),
            reject_reference: false,
            reject_manifest: false,
            reject_output: false,
            change_on_lookup: false,
        }
    }
    fn context(&self, facts: Vec<InterpretationFact>) -> InterpretationContext {
        let mut input = pin("input");
        input.digest =
            ContentDigest::new(format!("{:x}", Sha256::digest(self.text.as_bytes()))).unwrap();
        let mut task = pin("task-inspect");
        task.contract = id("cg.semantic-task");
        let binding = InterpretationBinding {
            scope: input.scope.clone(),
            request: id("request"),
            input: input.clone(),
            conversation: Some(pin("focus")),
            task: Some(ResolvedReference {
                id: TaskId::new("task-inspect").unwrap(),
                scope: task.scope.clone(),
                contract: task.contract.clone(),
                contract_version: task.contract_version,
                revision: task.revision.clone(),
                digest: task.digest.clone(),
            }),
            host_basis: pin("host"),
        };
        InterpretationContext::admit(
            InterpretationContextData {
                version: SchemaVersion::V1,
                id: id("context"),
                revision: id("capture"),
                binding,
                captured_at: UnixTimestamp::new(100),
                expires_at: UnixTimestamp::new(200),
                basis_refs: vec![input, task, pin("host"), pin("focus")],
                permitted_sources: vec![InterpretationSource {
                    source: pin("focus"),
                    tier: InterpretationTier::RetrievedEvidence,
                    sensitivity_ceiling: SensitivityClass::Normal,
                    trust: TrustClass::CanonicalReference,
                    max_results: 2,
                }],
                relevant_facts: facts,
                aliases: vec![],
                temporal_anchors: vec![],
            },
            self,
            UnixTimestamp::new(100),
        )
        .unwrap()
    }
}
impl InterpretationHost for Host {
    fn authenticate(&self, b: &InterpretationBinding) -> Result<(), InterpretationError> {
        if self.denied.get() || b.scope.as_str() != "project-x" {
            Err(InterpretationError::Admission)
        } else {
            Ok(())
        }
    }
    fn validate_basis(&self, _: &InterpretationPin, _: UnixTimestamp) -> bool {
        !self.denied.get()
    }
    fn validate_fact(
        &self,
        f: &InterpretationFact,
        _: UnixTimestamp,
    ) -> Result<(), SemanticGapReason> {
        if f.value == InterpretationValue::Target(expected().data().target.clone()) {
            Ok(())
        } else if let InterpretationValue::Target(t) = &f.value {
            if t.reference.id.as_str() == "artifact-2" {
                Ok(())
            } else {
                Err(SemanticGapReason::InvalidReference)
            }
        } else {
            Err(SemanticGapReason::InvalidReference)
        }
    }
    fn validate_projection(&self, _: &InterpretationContextData) -> bool {
        !self.denied.get()
    }
    fn validate_query(&self, _: &InterpretationBinding, q: &InterpretationQuery) -> bool {
        let (start, end) = q.span;
        self.text
            .get(start..end)
            .is_some_and(|s| s == "it" || s == "artifact-1" || s == "absent")
            && q.provenance.source_id().as_str() == "input"
    }
}
impl CompletenessHost for Host {
    fn validate_requirements(
        &self,
        _: &InterpretationBinding,
        _: &ContentDigest,
        f: &[SemanticField],
    ) -> bool {
        !self.reject_manifest
            && f.len() == 1
            && f[0].query.field.to_string() == "target"
            && f[0].requirement == SemanticFieldRequirement::Mandatory
            && f[0].depends_on.is_empty()
    }
}
impl SemanticReferenceValidator for Host {
    fn validate(
        &self,
        _: &SemanticTaskData,
        b: &SemanticReferenceBinding,
    ) -> Result<(), ValidationError> {
        let task = expected();
        let r = if b.id == "artifact-1" {
            &task.data().target.reference
        } else {
            &task.data().output_contract.schema
        };
        if !self.reject_reference
            && b.id == r.id.as_str()
            && b.scope == r.scope
            && b.digest == r.digest
            && b.revision == r.revision
            && b.contract == r.contract
        {
            Ok(())
        } else {
            Err(ValidationError::InvalidStateCombination {
                reason: "captured reference mismatch",
            })
        }
    }
}
impl InspectionLanguageHost for Host {
    fn validate_inspection_output(
        &self,
        _: &InterpretationBinding,
        o: &SemanticOutputContract,
    ) -> bool {
        !self.reject_output && *o == expected().data().output_contract
    }
}
impl InterpretationLookup for Host {
    fn lookup(
        &self,
        _: &ContextScopeId,
        _: &InterpretationSource,
        _: &EntityQuery,
    ) -> Vec<InterpretationFact> {
        vec![]
    }
}
// Captured registry lookup respects the explicitly permitted source bound.
struct Registry<'a>(&'a Host);
impl InterpretationLookup for Registry<'_> {
    fn lookup(
        &self,
        _: &ContextScopeId,
        source: &InterpretationSource,
        q: &EntityQuery,
    ) -> Vec<InterpretationFact> {
        if self.0.change_on_lookup {
            self.0.denied.set(true);
        }
        if *q
            != (EntityQuery::Canonical {
                id: id("artifact-1"),
                kind: TargetKind::Artifact,
            })
        {
            return vec![];
        }
        let mut f = fact("registry-artifact", expected().data().target.clone());
        f.tier = source.tier;
        f.source = source.source.clone();
        f.provenance = Provenance::new(
            ProvenanceId::new("registry-artifact").unwrap(),
            SourceKind::Repository,
            SourceId::new(source.source.id.as_str()).unwrap(),
            "artifact-1",
        )
        .unwrap();
        vec![f]
    }
}
fn compile(
    host: &Host,
    text: &str,
    facts: Vec<InterpretationFact>,
) -> Result<(CompletenessResult, Option<SemanticTaskIR>), InspectionLanguageError> {
    let c = host.context(facts);
    CompletenessValidator {
        resolver: DeterministicInterpretationResolver {
            host,
            registry: &Registry(host),
            project: host,
        },
    }
    .compile_inspection_request(
        &c,
        text,
        &expected().data().output_contract,
        UnixTimestamp::new(100),
    )
}

#[test]
fn original_sentences_compile_to_exact_formal_ir_without_models() {
    for text in [
        "Inspect artifact artifact-1.",
        " Inspect artifact artifact-1 ",
        "Inspect it.",
        "Inspect it",
    ] {
        let host = Host::new(text);
        let (r, t) = compile(
            &host,
            text,
            vec![fact("focus-artifact", expected().data().target.clone())],
        )
        .unwrap();
        assert!(r.is_complete());
        assert!(r.clarification.questions.is_empty());
        assert_eq!(t.unwrap(), expected());
        assert_eq!(
            r.decisions[0].query.provenance.source_id().as_str(),
            "input"
        );
        let formal = gateway_cgsl::compile(
            "formal",
            include_bytes!("../../../examples/cgsl-v1/minimal.cgsl"),
        )
        .unwrap();
        assert_eq!(formal.task_shape(), Some(&expected()));
    }
}

#[test]
fn missing_ambiguous_and_unknown_targets_never_become_executable() {
    let h = Host::new("Inspect it.");
    let (r, t) = compile(&h, &h.text, vec![]).unwrap();
    assert!(t.is_none());
    assert_eq!(r.clarification.questions.len(), 1);
    let mut second = expected().data().target.clone();
    second.reference.id = id("artifact-2");
    let (r, t) = compile(
        &h,
        &h.text,
        vec![
            fact("first", expected().data().target.clone()),
            fact("second", second),
        ],
    )
    .unwrap();
    assert!(t.is_none());
    assert_eq!(
        r.decisions[0].resolution.state(),
        SemanticResolutionState::Ambiguous
    );
    let h = Host::new("Inspect artifact absent.");
    let (r, t) = compile(
        &h,
        &h.text,
        vec![fact("focus-artifact", expected().data().target.clone())],
    )
    .unwrap();
    assert!(t.is_none());
    assert!(!r.is_complete());
}

#[test]
fn unsupported_instructions_and_changed_input_are_rejected() {
    for text in [
        "Do not inspect it.",
        "Inspect it. Delete it.",
        "Inspect artifact ../foreign.",
        "Inspect it in project-y.",
        "",
        "inspect it.",
        "Inspect artifact ",
    ] {
        assert_eq!(
            compile(&Host::new(text), text, vec![]),
            Err(InspectionLanguageError::UnsupportedRequest)
        );
    }
    assert_eq!(
        compile(
            &Host::new("Inspect it."),
            "Inspect artifact artifact-1.",
            vec![]
        ),
        Err(InspectionLanguageError::InputMismatch)
    );
    assert_eq!(
        compile(&Host::new("Inspect it."), &"x".repeat(4097), vec![]),
        Err(InspectionLanguageError::ResourceLimit)
    );
}

#[test]
fn reference_output_manifest_expiry_and_scope_gates_remain_mandatory() {
    for flag in 0..3 {
        let mut h = Host::new("Inspect it.");
        h.reject_reference = flag == 0;
        h.reject_manifest = flag == 1;
        h.reject_output = flag == 2;
        assert!(
            compile(
                &h,
                &h.text,
                vec![fact("focus-artifact", expected().data().target.clone())]
            )
            .is_err()
        );
    }
    let h = Host::new("Inspect it.");
    let c = h.context(vec![]);
    let registry = Registry(&h);
    let v = CompletenessValidator {
        resolver: DeterministicInterpretationResolver {
            host: &h,
            registry: &registry,
            project: &h,
        },
    };
    assert!(
        v.compile_inspection_request(
            &c,
            &h.text,
            &expected().data().output_contract,
            UnixTimestamp::new(200)
        )
        .is_err()
    );
    let mut output = expected().data().output_contract.clone();
    output.schema.scope = ContextScopeId::new("foreign").unwrap();
    assert_eq!(
        v.compile_inspection_request(&c, &h.text, &output, UnixTimestamp::new(100)),
        Err(InspectionLanguageError::InvalidContract)
    );
    assert_ne!(
        inspection_revision(&c, &h.text, &output),
        inspection_revision(&c, &h.text, &expected().data().output_contract)
    );
    let mut data: InterpretationContextData = serde_json::from_slice(c.canonical_bytes()).unwrap();
    data.binding.task = None;
    let missing_task = InterpretationContext::admit(data, &h, UnixTimestamp::new(100)).unwrap();
    assert_eq!(
        v.compile_inspection_request(
            &missing_task,
            &h.text,
            &expected().data().output_contract,
            UnixTimestamp::new(100)
        ),
        Err(InspectionLanguageError::InvalidContract)
    );
    let mut data: InterpretationContextData = serde_json::from_slice(c.canonical_bytes()).unwrap();
    let mut foreign = expected().data().target.clone();
    foreign.reference.scope = ContextScopeId::new("foreign").unwrap();
    data.relevant_facts.push(fact("foreign", foreign));
    assert!(InterpretationContext::admit(data, &h, UnixTimestamp::new(100)).is_err());
}

#[test]
fn clarification_reenters_the_text_path_and_changed_basis_is_refused() {
    let initial = Host::new("Inspect it.");
    let (pending, task) = compile(&initial, &initial.text, vec![]).unwrap();
    assert!(task.is_none());
    let reply = Host::new("Inspect artifact artifact-1.");
    let (resolved, task) = compile(&reply, &reply.text, vec![]).unwrap();
    assert_eq!(task.unwrap(), expected());
    assert_ne!(
        pending.clarification.semantic_revision,
        resolved.clarification.semantic_revision
    );
    assert_ne!(
        pending.clarification.capture_digest,
        resolved.clarification.capture_digest
    );
    let mut changing = Host::new("Inspect artifact artifact-1.");
    changing.change_on_lookup = true;
    assert!(compile(&changing, &changing.text, vec![]).is_err());
}
