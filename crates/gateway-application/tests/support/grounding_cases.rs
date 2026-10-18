use super::*;
use gateway_application::grounding::*;
use gateway_domain::grounding::*;
use std::cell::RefCell;

struct GroundedHost {
    base: Host,
    manifest: RefCell<GroundingManifest>,
    evidence: RefCell<GroundingEvidence>,
    fault: Cell<u8>,
}
impl SemanticReferenceValidator for GroundedHost {
    fn validate(
        &self,
        task: &SemanticTaskData,
        binding: &SemanticReferenceBinding,
    ) -> Result<(), ValidationError> {
        self.base.validate(task, binding)
    }
}
impl InvocationAdmissionPort for GroundedHost {
    fn validate_current(&self, ir: &InvocationIr) -> Result<(), InvocationIrError> {
        self.base.validate_current(ir)
    }
}
impl OutputVerificationPort for GroundedHost {
    fn grounding(&self) -> Option<&dyn GroundingEvidencePort> {
        Some(self)
    }
    fn resolve_reference(
        &self,
        ir: &InvocationIr,
        reference: &OutputReference,
    ) -> Result<(), VerificationError> {
        self.base.resolve_reference(ir, reference)
    }
    fn evaluate(
        &self,
        ir: &InvocationIr,
        output: &ParsedOutput,
    ) -> Result<ConstraintReport, VerificationError> {
        self.base.evaluate(ir, output)
    }
}
impl GroundingEvidencePort for GroundedHost {
    fn manifest(
        &self,
        _: &InvocationIr,
        _: &ParsedOutput,
    ) -> Result<GroundingManifest, GroundingError> {
        if self.fault.get() == 1 {
            return Err(GroundingError::Unavailable);
        }
        Ok(self.manifest.borrow().clone())
    }
    fn resolve(
        &self,
        _: &InvocationIr,
        _: &ResolvedReference<EvidenceId>,
    ) -> Result<GroundingEvidence, GroundingError> {
        if self.fault.get() == 2 {
            return Err(GroundingError::InvalidCitation);
        }
        if self.fault.get() == 3 {
            self.base.error.set(Some(InvocationIrError::StaleInput));
        }
        Ok(self.evidence.borrow().clone())
    }
}
fn text() -> TypedValue {
    TypedValue::string("entity").unwrap()
}
fn subject() -> SubjectPath {
    SubjectPath::new(["artifact", "status"]).unwrap()
}
fn fid() -> FactId {
    FactId::new("fact").unwrap()
}
fn pid() -> ProvenanceId {
    ProvenanceId::new("source").unwrap()
}
fn oid() -> ObservationId {
    ObservationId::new("observation").unwrap()
}
fn lineage(
    source: SourceKind,
    kind: EvidenceKind,
    challenge: bool,
    contradictory: bool,
    observed_value: TypedValue,
) -> ObservationEvidenceSet {
    let provenance = Provenance::new(
        pid(),
        source,
        SourceId::new("store").unwrap(),
        "artifact://source",
    )
    .unwrap();
    let observation = Observation::new(oid(), subject(), observed_value, pid()).unwrap();
    let fact = Fact::new(
        fid(),
        subject(),
        text(),
        AssertionPolarity::Affirmed,
        vec![oid()],
    )
    .unwrap();
    let mut facts = vec![fact];
    if contradictory {
        facts.push(
            Fact::new(
                FactId::new("other").unwrap(),
                subject(),
                TypedValue::string("opposite").unwrap(),
                AssertionPolarity::Affirmed,
                vec![oid()],
            )
            .unwrap(),
        );
    }
    let mut links = vec![EvidenceLink::new(fid(), EvidenceRelation::Supports)];
    if challenge {
        links.push(EvidenceLink::new(fid(), EvidenceRelation::Challenges));
    }
    let evidence = Evidence::new(
        EvidenceId::new("citation").unwrap(),
        kind,
        "Observed artifact",
        EvidenceContent::inline("entity").unwrap(),
        pid(),
        links,
    )
    .unwrap();
    ObservationEvidenceSet::new(vec![provenance], vec![observation], facts, vec![evidence]).unwrap()
}
fn quality(
    trust: TrustClass,
    sensitivity: SensitivityClass,
    freshness: FreshnessStatus,
    uncertainty: Uncertainty,
) -> QualityMetadata {
    QualityMetadata::new(
        trust,
        sensitivity,
        Confidence::score(1.0).unwrap(),
        freshness,
        uncertainty,
    )
}
fn grounded() -> (InvocationIr, ParsedOutput, GroundedHost) {
    let (ir, base) = fixture();
    let output = parse(&wire(&ir));
    let citation = support::examples::pin(EvidenceId::new("citation").unwrap());
    let manifest = GroundingManifest {
        schema_version: SchemaVersion::V1,
        binding: output.binding(),
        claims: vec![GroundingClaim {
            pointer: "".into(),
            wire_value: json!("entity"),
            subject: subject(),
            value: text(),
            fact: fid(),
            citations: vec![citation.clone()],
            external_effect: None,
        }],
        evaluated_at: UnixTimestamp::new(100),
        freshness: FreshnessPolicy::new(10),
        allowed_trust: vec![TrustClass::ObservedEvidence, TrustClass::RetrievedContent],
    };
    let evidence = GroundingEvidence {
        binding: citation,
        source: support::examples::pin(SourceId::new("store").unwrap()),
        knowledge: KnowledgeProvenance::new("artifact://source", Some("r1")).unwrap(),
        lineage: lineage(
            SourceKind::Tool,
            EvidenceKind::ToolOutput,
            false,
            false,
            text(),
        ),
        quality: quality(
            TrustClass::ObservedEvidence,
            SensitivityClass::Normal,
            FreshnessStatus::Fresh,
            Uncertainty::None,
        ),
        observed_at: Some(UnixTimestamp::new(90)),
        observed_effect: None,
    };
    (
        ir,
        output,
        GroundedHost {
            base,
            manifest: RefCell::new(manifest),
            evidence: RefCell::new(evidence),
            fault: Cell::new(0),
        },
    )
}
fn reject_manifest(change: impl FnOnce(&mut GroundingManifest), expected: GroundingError) {
    let (ir, output, host) = grounded();
    change(&mut host.manifest.borrow_mut());
    assert_eq!(verify_grounding(&ir, &output, &host), Err(expected));
    let rejection = verify_output(&ir, output, &host, vec![CheckOutcome::Passed]).unwrap_err();
    let diagnostic = match expected {
        GroundingError::CorrelationMismatch => VerificationError::CorrelationMismatch,
        _ => VerificationError::GroundingFailed(expected),
    };
    assert_eq!(rejection.diagnostic, Some(diagnostic));
}
fn reject_evidence(change: impl FnOnce(&mut GroundingEvidence), expected: GroundingError) {
    let (ir, output, host) = grounded();
    change(&mut host.evidence.borrow_mut());
    assert_eq!(verify_grounding(&ir, &output, &host), Err(expected));
    assert!(verify_output(&ir, output, &host, vec![CheckOutcome::Passed]).is_err());
}
#[test]
fn grounded_outputs_use_current_support_and_keep_exact_correlation() {
    let (ir, output, host) = grounded();
    assert_eq!(verify_grounding(&ir, &output, &host), Ok(()));
    let accepted = verify_output(&ir, output.clone(), &host, vec![CheckOutcome::Failed]).unwrap();
    assert_eq!(accepted.record().binding, output.binding());
    host.evidence.borrow_mut().quality = quality(
        TrustClass::RetrievedContent,
        SensitivityClass::Normal,
        FreshnessStatus::Fresh,
        Uncertainty::None,
    );
    host.evidence.borrow_mut().lineage = lineage(
        SourceKind::Retrieval,
        EvidenceKind::RetrievalResult,
        false,
        false,
        text(),
    );
    assert_eq!(verify_grounding(&ir, &output, &host), Ok(()));
    host.fault.set(3);
    assert_eq!(
        verify_output(&ir, output, &host, vec![])
            .unwrap_err()
            .diagnostic,
        Some(VerificationError::StaleContext)
    );
}
#[test]
fn invalid_manifests_and_fabricated_citations_fail() {
    use GroundingError::*;
    reject_manifest(
        |m| m.schema_version = SchemaVersion::new(2, 0).unwrap(),
        IncompatibleVersion,
    );
    reject_manifest(
        |m| m.binding.output_digest = context_content_digest(b"other"),
        CorrelationMismatch,
    );
    reject_manifest(|m| m.claims.clear(), UnsupportedClaim);
    reject_manifest(|m| m.claims.push(m.claims[0].clone()), UnsupportedClaim);
    reject_manifest(
        |m| m.claims[0].pointer = "/missing".into(),
        UnsupportedClaim,
    );
    reject_manifest(
        |m| m.claims[0].wire_value = json!("invented"),
        UnsupportedClaim,
    );
    reject_manifest(|m| m.claims[0].citations.clear(), UnsupportedClaim);
    reject_manifest(
        |m| {
            let duplicate = m.claims[0].citations[0].clone();
            m.claims[0].citations.push(duplicate);
        },
        InvalidCitation,
    );
    reject_manifest(
        |m| m.claims[0].citations[0].contract_version = SchemaVersion::new(2, 0).unwrap(),
        InvalidCitation,
    );
    reject_manifest(
        |m| m.claims[0].citations[0].scope = ContextScopeId::new("other-project").unwrap(),
        CrossScopeEvidence,
    );
    reject_manifest(
        |m| m.claims[0].fact = FactId::new("missing").unwrap(),
        UnsupportedClaim,
    );
    reject_manifest(
        |m| m.claims[0].subject = SubjectPath::new(["other", "subject"]).unwrap(),
        UnsupportedClaim,
    );
    reject_manifest(|m| m.allowed_trust.clear(), UntrustedEvidence);
    reject_evidence(
        |e| e.source.scope = ContextScopeId::new("other-project").unwrap(),
        CrossScopeEvidence,
    );
    reject_evidence(
        |e| e.source.id = SourceId::new("invented").unwrap(),
        InvalidCitation,
    );
    reject_evidence(
        |e| e.source.contract_version = SchemaVersion::new(2, 0).unwrap(),
        InvalidCitation,
    );
    reject_evidence(
        |e| e.source.revision = ReferenceId::new("new-source").unwrap(),
        InvalidCitation,
    );

    reject_evidence(
        |e| e.binding.revision = ReferenceId::new("r2").unwrap(),
        InvalidCitation,
    );
    reject_evidence(
        |e| e.knowledge = KnowledgeProvenance::new("artifact://source", None::<&str>).unwrap(),
        InvalidCitation,
    );
    reject_evidence(
        |e| e.knowledge = KnowledgeProvenance::new("fabricated", Some("r1")).unwrap(),
        InvalidCitation,
    );
    reject_evidence(
        |e| e.binding.id = EvidenceId::new("fabricated").unwrap(),
        InvalidCitation,
    );
}
#[test]
fn freshness_conflicts_trust_and_disclosure_are_enforced() {
    use GroundingError::*;
    for time in [
        None,
        Some(UnixTimestamp::new(89)),
        Some(UnixTimestamp::new(101)),
    ] {
        reject_evidence(|e| e.observed_at = time, StaleEvidence);
    }
    reject_evidence(
        |e| {
            e.quality = quality(
                TrustClass::ObservedEvidence,
                SensitivityClass::Normal,
                FreshnessStatus::Unknown,
                Uncertainty::None,
            )
        },
        StaleEvidence,
    );
    reject_evidence(
        |e| {
            e.quality = quality(
                TrustClass::ObservedEvidence,
                SensitivityClass::Secret,
                FreshnessStatus::Fresh,
                Uncertainty::None,
            )
        },
        SensitivityViolation,
    );
    reject_evidence(
        |e| e.quality = e.quality.with_conflict(ConflictStatus::Unresolved),
        ContradictoryEvidence,
    );
    reject_evidence(
        |e| {
            e.quality = quality(
                TrustClass::ObservedEvidence,
                SensitivityClass::Normal,
                FreshnessStatus::Fresh,
                Uncertainty::Unknown,
            )
        },
        ContradictoryEvidence,
    );
    reject_evidence(
        |e| {
            e.lineage = lineage(
                SourceKind::Tool,
                EvidenceKind::ToolOutput,
                true,
                false,
                text(),
            )
        },
        ContradictoryEvidence,
    );
    reject_evidence(
        |e| {
            e.lineage = lineage(
                SourceKind::Tool,
                EvidenceKind::ToolOutput,
                false,
                true,
                text(),
            )
        },
        ContradictoryEvidence,
    );
    for (source, kind) in [
        (SourceKind::Model, EvidenceKind::Report),
        (SourceKind::Synthetic, EvidenceKind::Report),
        (SourceKind::Tool, EvidenceKind::ModelOutput),
        (SourceKind::Tool, EvidenceKind::SyntheticFixture),
    ] {
        reject_evidence(
            |e| e.lineage = lineage(source, kind, false, false, text()),
            UntrustedEvidence,
        );
    }
}
#[test]
fn external_success_needs_independent_observation_of_exact_action() {
    use GroundingError::*;
    let (ir, output, host) = grounded();
    let action = pin("host-action-1");
    host.manifest.borrow_mut().claims[0].external_effect = Some(action.clone());
    assert_eq!(
        verify_grounding(&ir, &output, &host),
        Err(ExternalEffectUnproven)
    );
    host.evidence.borrow_mut().observed_effect = Some(action.clone());
    assert_eq!(verify_grounding(&ir, &output, &host), Ok(()));
    assert!(verify_output(&ir, output.clone(), &host, vec![]).is_ok());
    for field in ["revision", "scope", "digest", "id"] {
        let mut wrong = json!(action);
        wrong[field] = match field {
            "digest" => json!(context_content_digest(b"other")),
            _ => json!("other"),
        };
        host.evidence.borrow_mut().observed_effect = Some(serde_json::from_value(wrong).unwrap());
        assert_eq!(
            verify_grounding(&ir, &output, &host),
            Err(ExternalEffectUnproven)
        );
    }
    host.evidence.borrow_mut().observed_effect = Some(action);
    for (source, kind) in [
        (SourceKind::Retrieval, EvidenceKind::ToolOutput),
        (SourceKind::Tool, EvidenceKind::Report),
    ] {
        host.evidence.borrow_mut().lineage = lineage(source, kind, false, false, text());
        assert_eq!(
            verify_grounding(&ir, &output, &host),
            Err(ExternalEffectUnproven)
        );
    }
    host.evidence.borrow_mut().lineage = lineage(
        SourceKind::Tool,
        EvidenceKind::ToolOutput,
        false,
        false,
        TypedValue::string("not observed").unwrap(),
    );
    assert_eq!(
        verify_grounding(&ir, &output, &host),
        Err(ExternalEffectUnproven)
    );
    host.evidence.borrow_mut().lineage = lineage(
        SourceKind::Tool,
        EvidenceKind::ToolOutput,
        false,
        false,
        text(),
    );
    host.evidence.borrow_mut().quality = quality(
        TrustClass::RetrievedContent,
        SensitivityClass::Normal,
        FreshnessStatus::Fresh,
        Uncertainty::None,
    );
    assert_eq!(
        verify_grounding(&ir, &output, &host),
        Err(ExternalEffectUnproven)
    );
}
#[test]
fn unavailable_grounding_never_uses_confidence_or_critics() {
    let (ir, output, host) = grounded();
    host.fault.set(1);
    assert_eq!(
        verify_output(&ir, output.clone(), &host, vec![CheckOutcome::Passed])
            .unwrap_err()
            .diagnostic,
        Some(VerificationError::VerificationUnavailable)
    );
    host.fault.set(2);
    assert_eq!(
        verify_grounding(&ir, &output, &host),
        Err(GroundingError::InvalidCitation)
    );
}

#[test]
fn required_semantic_checks_cannot_accept_host_passes_without_grounding() {
    for all_claims in [false, true] {
        let mut task = serde_json::to_value(support::examples::task()).unwrap();
        let mut checks = vec![
            json!("OUTPUT_SCHEMA_VALID"),
            json!("NO_UNRESOLVED_REFERENCE"),
            json!("EVIDENCE_REQUIRED"),
        ];
        if all_claims {
            checks.push(json!("ALL_CLAIMS_SUPPORTED"));
        }
        task["verification_contract"]["checks"] = json!(checks);
        let task = SemanticTaskIR::from_json(&task.to_string()).unwrap();
        let handoff = support::handoff_for(task);
        let prepared = support::prepared(&handoff, 100_000, false);
        let ir = admit_invocation_ir(
            SchemaVersion::V1,
            handoff,
            prepared,
            BTreeMap::new(),
            &Capture,
        )
        .unwrap();
        let (_, mut base) = fixture();
        base.expected = json!(ir);
        let output = parse(&wire(&ir));
        assert_eq!(
            verify_output(&ir, output.clone(), &base, vec![CheckOutcome::Passed])
                .unwrap_err()
                .diagnostic,
            Some(VerificationError::VerificationUnavailable)
        );
        let (_, _, mut host) = grounded();
        host.base = base;
        host.manifest.borrow_mut().binding = output.binding();
        assert!(verify_output(&ir, output, &host, vec![]).is_ok());
    }
}

#[test]
fn exact_payload_and_cg06_support_edges_are_mandatory() {
    use GroundingError::*;
    reject_manifest(
        |m| m.claims[0].value = TypedValue::Set(vec![]),
        UnsupportedClaim,
    );
    reject_evidence(
        |e| e.lineage = ObservationEvidenceSet::new(vec![], vec![], vec![], vec![]).unwrap(),
        InvalidCitation,
    );
    reject_evidence(
        |e| {
            let fact = e.lineage.facts()[0].clone();
            let other = Fact::new(
                FactId::new("other").unwrap(),
                SubjectPath::new(["other"]).unwrap(),
                text(),
                AssertionPolarity::Affirmed,
                vec![oid()],
            )
            .unwrap();
            let evidence = Evidence::new(
                EvidenceId::new("citation").unwrap(),
                EvidenceKind::ToolOutput,
                "unrelated",
                EvidenceContent::inline("entity").unwrap(),
                pid(),
                vec![EvidenceLink::new(
                    other.id().clone(),
                    EvidenceRelation::Supports,
                )],
            )
            .unwrap();
            e.lineage = ObservationEvidenceSet::new(
                e.lineage.provenances().to_vec(),
                e.lineage.observations().to_vec(),
                vec![fact, other],
                vec![evidence],
            )
            .unwrap();
        },
        UnsupportedClaim,
    );
    let (ir, output, host) = grounded();
    let mut other = wire(&ir);
    other["invocation"]["digest"] = json!(context_content_digest(b"other invocation"));
    // Change a real identity field rather than accepting arbitrary model metadata.
    other["invocation"]
        .as_object_mut()
        .unwrap()
        .remove("digest");
    other["invocation"]["invocation_id"] = json!("other-invocation");
    assert_eq!(
        verify_grounding(&ir, &parse(&other), &host),
        Err(CorrelationMismatch)
    );
    host.manifest.borrow_mut().claims[0].external_effect = Some(pin("action"));
    host.evidence.borrow_mut().observed_effect = Some(pin("action"));
    host.manifest.borrow_mut().claims[0]
        .external_effect
        .as_mut()
        .unwrap()
        .scope = ContextScopeId::new("other").unwrap();
    assert_eq!(
        verify_grounding(&ir, &output, &host),
        Err(ExternalEffectUnproven)
    );
}

#[test]
fn versioned_grounding_results_and_diagnostics_are_stable() {
    use GroundingError::*;
    let expected: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/grounding-v1/diagnostics.json"
    ))
    .unwrap();
    let errors = [
        Unavailable,
        IncompatibleVersion,
        CorrelationMismatch,
        UnsupportedClaim,
        InvalidCitation,
        CrossScopeEvidence,
        StaleEvidence,
        ContradictoryEvidence,
        UntrustedEvidence,
        SensitivityViolation,
        ExternalEffectUnproven,
    ];
    assert_eq!(json!(errors), expected);
    assert_eq!(
        serde_json::from_value::<Vec<GroundingError>>(expected).unwrap(),
        errors
    );
    assert_eq!(
        json!(VerificationError::GroundingFailed(StaleEvidence)),
        json!({"GROUNDING_FAILED":"STALE_EVIDENCE"})
    );
    let cases: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/grounding-v1/results.json"
    ))
    .unwrap();
    for case in cases.as_array().unwrap() {
        let (ir, _, host) = grounded();
        let mut value = wire(&ir);
        value["result"] = case["result"].clone();
        let output = parse(&value);
        host.manifest.borrow_mut().binding = output.binding();
        let expected = if case["expected"].is_null() {
            Ok(())
        } else {
            Err(serde_json::from_value(case["expected"].clone()).unwrap())
        };
        assert_eq!(
            verify_grounding(&ir, &output, &host),
            expected,
            "{}",
            case["id"]
        );
    }
}

#[test]
fn source_revision_is_independent_of_evidence_record_revision() {
    let (ir, output, host) = grounded();
    host.evidence.borrow_mut().source.revision = ReferenceId::new("git-commit-7").unwrap();
    host.evidence.borrow_mut().knowledge =
        KnowledgeProvenance::new("artifact://source", Some("git-commit-7")).unwrap();
    assert_ne!(
        host.evidence.borrow().source.revision,
        host.evidence.borrow().binding.revision
    );
    assert!(verify_output(&ir, output, &host, vec![]).is_ok());
}
