//! Application-owned independent artifact verification and CG-14 goal evaluation.
use super::*;
use crate::{
    PlanningCapabilitySnapshot, ScopedObservationBatch, SourceSnapshot,
    closed_loop::{ClosedLoop, LoopDecision, LoopRules},
    codex::CompileCommand,
    context_application::CompiledStep,
};
use gateway_context::ContextDisclosurePolicy;
use gateway_domain::*;
use sha2::{Digest, Sha256};

pub fn canonical_artifact(compiled: &CompiledStep) -> Result<Vec<u8>, SessionError> {
    compiled
        .to_json_with_policy(ContextDisclosurePolicy {
            maximum_sensitivity: SensitivityClass::Public,
            include_caller_input: true,
            include_external_content: false,
        })
        .map(String::into_bytes)
        .map_err(|_| SessionError::InvalidInput)
}

pub fn assess_initial_goal(
    checkpoint: &SessionCheckpoint,
    inputs: &CompileCommand,
    authority: &RecordRef,
) -> Result<InitialAssessment, SessionError> {
    // A new, owner-command-unique task has no released artifact. Empty captured
    // observations preserve UNKNOWN; no synthetic successful fact is supplied.
    let scope = checkpoint.goal.basis().scope.clone();
    let records = ObservationEvidenceSet::new(vec![], vec![], vec![], vec![])
        .map_err(|_| SessionError::InvalidInput)?;
    let capture = serde_json::to_vec(&records).map_err(|_| SessionError::InvalidInput)?;
    let batch = ScopedObservationBatch::new(
        scope.clone(),
        SourceSnapshot::new(
            SourceId::new("context-artifact-verifier").map_err(|_| SessionError::InvalidInput)?,
            SourceKind::Tool,
            None,
            Some(
                ContentDigest::new(format!("{:x}", Sha256::digest(&capture)))
                    .map_err(|_| SessionError::InvalidInput)?,
            ),
        )
        .map_err(|_| SessionError::InvalidInput)?,
        records,
    )
    .map_err(|_| SessionError::InvalidInput)?;
    let rules = LoopRules {
        capabilities: PlanningCapabilitySnapshot::new(
            inputs.resolved.snapshot.input().index.clone(),
            "context-artifact-verifier",
            PlanningIrVersion::V1,
        )
        .map_err(|_| SessionError::InvalidInput)?,
        requirements: CapabilityRequirementRules::default(),
        planner: PlannerRules::default(),
        max_iterations: checkpoint.budget.max_actions,
        max_retries: checkpoint.budget.max_retries,
    };
    let run = ClosedLoop::start(
        ReferenceId::new(checkpoint.snapshot.run.as_str())
            .map_err(|_| SessionError::InvalidInput)?,
        scope,
        checkpoint.goal.intent().clone(),
        batch,
        rules,
    )
    .map_err(|_| SessionError::InvalidInput)?;
    if run.decision() == LoopDecision::Success {
        return Err(SessionError::InvalidState);
    }
    Ok(InitialAssessment {
        goal: content_reference(
            "supported-goal",
            &serde_json::to_vec(checkpoint.goal.intent())
                .map_err(|_| SessionError::InvalidInput)?,
        )?,
        authority: authority.clone(),
        outcome: run.assessment().comparison.outcome().as_str().into(),
    })
}

pub fn verify_artifact(
    repository: &impl SessionRepositoryPort,
    checkpoint: &SessionCheckpoint,
    expected: &CompiledStep,
    inputs: &CompileCommand,
) -> Result<RecordWrite, SessionError> {
    let artifact = checkpoint
        .artifact
        .as_ref()
        .ok_or(SessionError::Unavailable)?;
    if repository.record_kind(&checkpoint.owner, artifact)? != RecordKind::Artifact {
        return Err(SessionError::InvalidInput);
    }
    let bytes = repository.load_artifact(&checkpoint.owner, artifact)?;
    if bytes.len() > 1_048_576
        || artifact.digest() != format!("sha256:{:x}", Sha256::digest(&bytes))
        || bytes != canonical_artifact(expected)?
    {
        return Err(SessionError::InvalidInput);
    }
    let goal = checkpoint.goal.basis();
    if expected.basis().scope != goal.scope
        || inputs.projection.mapping.step != goal.step
        || expected.basis() != &inputs.resolved.report.basis
    {
        return Err(SessionError::ScopeDenied);
    }
    let ir = expected.context().execution_context();
    ir.validate_against(&inputs.catalog)
        .map_err(|_| SessionError::AuthorityDenied)?;
    // Parse the stored canonical IR through its existing strict domain contract.
    let stored: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| SessionError::InvalidInput)?;
    let loaded = ExecutionContextIR::from_json(&stored["execution_context"].to_string())
        .map_err(|_| SessionError::InvalidInput)?;
    if &loaded != ir {
        return Err(SessionError::InvalidInput);
    }
    let subject = SubjectPath::new(
        format!("cg.context.{}.verified", goal.projection.id().as_str()).split('.'),
    )
    .map_err(|_| SessionError::InvalidInput)?;
    let provenance = Provenance::new(
        ProvenanceId::new("context-artifact-verifier").map_err(|_| SessionError::InvalidInput)?,
        SourceKind::Tool,
        SourceId::new("context-artifact-verifier").map_err(|_| SessionError::InvalidInput)?,
        format!("cg://verified-artifacts/{}", artifact.id().as_str()),
    )
    .map_err(|_| SessionError::InvalidInput)?;
    let observation = Observation::new(
        ObservationId::new("artifact-validation").map_err(|_| SessionError::InvalidInput)?,
        subject.clone(),
        TypedValue::Boolean(true),
        provenance.id().clone(),
    )
    .map_err(|_| SessionError::InvalidInput)?;
    let fact = Fact::new(
        FactId::new("artifact-verified").map_err(|_| SessionError::InvalidInput)?,
        subject.clone(),
        TypedValue::Boolean(true),
        AssertionPolarity::Affirmed,
        vec![observation.id().clone()],
    )
    .map_err(|_| SessionError::InvalidInput)?;
    let receipt = serde_json::json!({"contract":"cg.artifact-verification","contract_version":"2.0",
        "owner":checkpoint.owner,"session":checkpoint.snapshot.session,"run":checkpoint.snapshot.run,
        "goal":content_reference("supported-goal",&serde_json::to_vec(checkpoint.goal.intent()).map_err(|_| SessionError::InvalidInput)?)?,
        "basis":goal,"artifact":artifact,"dispatch":checkpoint.snapshot.dispatch,
        "current_basis":crate::resolution_encoding::basis_json(expected.basis()),
        "policy":expected.policy(),"checks":["canonical_bytes","digest","domain_ir","catalog","scope","step","pinned_inputs","provenance","disclosure","current_policy"]});
    let evidence = Evidence::new(
        EvidenceId::new("artifact-verification").map_err(|_| SessionError::InvalidInput)?,
        EvidenceKind::Report,
        "Independent stored context artifact validation",
        EvidenceContent::inline(receipt.to_string()).map_err(|_| SessionError::InvalidInput)?,
        provenance.id().clone(),
        vec![EvidenceLink::new(
            fact.id().clone(),
            EvidenceRelation::Supports,
        )],
    )
    .map_err(|_| SessionError::InvalidInput)?;
    let records = ObservationEvidenceSet::new(
        vec![provenance],
        vec![observation],
        vec![fact],
        vec![evidence],
    )
    .map_err(|_| SessionError::InvalidInput)?;
    let batch = ScopedObservationBatch::new(
        goal.scope.clone(),
        SourceSnapshot::new(
            SourceId::new("context-artifact-verifier").map_err(|_| SessionError::InvalidInput)?,
            SourceKind::Tool,
            None,
            Some(
                ContentDigest::new(&artifact.digest()[7..])
                    .map_err(|_| SessionError::InvalidInput)?,
            ),
        )
        .map_err(|_| SessionError::InvalidInput)?,
        records,
    )
    .map_err(|_| SessionError::InvalidInput)?
    .with_quality_metadata(
        subject,
        vec![QualityMetadata::new(
            TrustClass::ObservedEvidence,
            SensitivityClass::Public,
            Confidence::Unknown,
            FreshnessStatus::Fresh,
            Uncertainty::None,
        )],
    );
    let rules = LoopRules {
        capabilities: PlanningCapabilitySnapshot::new(
            inputs.resolved.snapshot.input().index.clone(),
            "context-artifact-verifier",
            PlanningIrVersion::V1,
        )
        .map_err(|_| SessionError::InvalidInput)?,
        requirements: CapabilityRequirementRules::default(),
        planner: PlannerRules::default(),
        max_iterations: checkpoint.budget.max_actions,
        max_retries: checkpoint.budget.max_retries,
    };
    let run = ClosedLoop::start(
        ReferenceId::new(checkpoint.snapshot.run.as_str())
            .map_err(|_| SessionError::InvalidInput)?,
        goal.scope.clone(),
        checkpoint.goal.intent().clone(),
        batch,
        rules,
    )
    .map_err(|_| SessionError::InvalidInput)?;
    if run.decision() != LoopDecision::Success
        || run.assessment().comparison.trace().facts().is_empty()
        || run.assessment().comparison.trace().evidence().is_empty()
    {
        return Err(SessionError::InvalidInput);
    }
    let bytes = serde_json::to_vec(&serde_json::json!({"contract":"cg.evidence","contract_version":"2.0","verification":receipt,
        "goal_outcome":run.assessment().comparison.outcome().as_str(),"facts":run.assessment().comparison.trace().facts().iter().map(FactId::as_str).collect::<Vec<_>>(),
        "evidence":run.assessment().comparison.trace().evidence().iter().map(EvidenceId::as_str).collect::<Vec<_>>() })).map_err(|_| SessionError::InvalidInput)?;
    Ok(RecordWrite {
        reference: content_reference(
            &format!("evidence-{}", checkpoint.snapshot.run.as_str()),
            &bytes,
        )?,
        kind: RecordKind::Evidence,
        bytes,
    })
}
