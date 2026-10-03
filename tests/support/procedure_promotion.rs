#![allow(dead_code)]
use gateway_domain::{
    ProvenanceId, ReferenceId,
    learning::{LearnedProcedure, PatternCandidate},
    procedure_evaluation::{EvaluationBundle, EvaluationDataset, counterfactuals},
    procedure_promotion::*,
};
use gateway_registry::learned_procedures::LearnedProcedureRegistry;

pub fn id(value: &str) -> ReferenceId {
    ReferenceId::new(value).unwrap()
}
pub fn procedure(version: u32) -> LearnedProcedure {
    let original = LearnedProcedure::from_json(include_str!(
        "../fixtures/procedure-evaluation-v1/procedure.json"
    ))
    .unwrap();
    let candidate = PatternCandidate::new(
        original.source_candidate().clone(),
        original.fingerprint().clone(),
        original.experience().to_vec(),
    )
    .unwrap();
    LearnedProcedure::new(
        original.id().clone(),
        version,
        &candidate,
        original.steps().to_vec(),
        original.required_observations().to_vec(),
        original.required_evidence().to_vec(),
        original.verification_evidence().to_vec(),
        original.fallback(),
    )
    .unwrap()
}
pub fn bundle(version: u32) -> EvaluationBundle {
    let p = procedure(version);
    let mut dataset: EvaluationDataset = serde_json::from_str(include_str!(
        "../fixtures/procedure-evaluation-v1/historical.json"
    ))
    .unwrap();
    dataset
        .cases
        .extend(counterfactuals(&p, &dataset.cases[0]).unwrap());
    EvaluationBundle::evaluate(&p, dataset, id("runtime-1")).unwrap()
}
pub fn boundary() -> CanaryBoundary {
    CanaryBoundary {
        scope: procedure(1).fingerprint().scope().clone(),
        cohorts: [id("pilot")].into(),
        starts_at: 20,
        ends_at: 100,
        max_executions: 2,
        max_failures: 0,
        required_successes: 1,
    }
}
pub fn execution(name: &str, mode: ExecutionMode) -> ExecutionRequest {
    ExecutionRequest {
        id: id(name),
        scope: boundary().scope,
        cohort: id("pilot"),
        mode,
    }
}
pub fn event(n: usize, at: i64, command: PromotionCommand) -> PromotionEvent {
    PromotionEvent {
        metadata: DecisionMetadata {
            id: id(&format!("decision-{n}")),
            actor: ProvenanceId::new("operator").unwrap(),
            policy_decision: id("policy-approval"),
            at,
        },
        command,
    }
}
#[derive(Default)]
pub struct History {
    pub journal: PromotionJournal,
}
impl History {
    pub fn registry(&self) -> LearnedProcedureRegistry {
        LearnedProcedureRegistry::from_journal(&self.journal).unwrap()
    }
    pub fn push(&mut self, at: i64, command: PromotionCommand) {
        self.journal
            .events
            .push(event(self.journal.events.len(), at, command));
        self.registry();
    }
    pub fn rejects(&self, at: i64, command: PromotionCommand) {
        let mut journal = self.journal.clone();
        journal
            .events
            .push(event(journal.events.len(), at, command));
        assert!(
            LearnedProcedureRegistry::from_journal(&journal).is_err(),
            "unexpected acceptance: {:?}",
            journal.events.last()
        );
    }
    pub fn discover(&mut self, version: u32) -> ProcedureVersion {
        let p = procedure(version);
        let v = ProcedureVersion::of(&p);
        self.push(
            10,
            PromotionCommand::Discover {
                procedure: Box::new(p),
                discovery_evidence: id("discovery"),
            },
        );
        v
    }
    pub fn canary(&mut self, v: &ProcedureVersion, at: i64) {
        for (from, to) in [
            (PromotionState::Discovered, PromotionState::Candidate),
            (PromotionState::Candidate, PromotionState::Validated),
        ] {
            self.push(
                at,
                PromotionCommand::Advance {
                    procedure: v.clone(),
                    from,
                    to,
                    evidence: id("validation"),
                },
            );
        }
        let b = bundle(v.version);
        let digest = b.digest.clone();
        self.push(
            at,
            PromotionCommand::Evaluate {
                procedure: v.clone(),
                bundle: Box::new(b),
            },
        );
        self.push(
            at,
            PromotionCommand::Approve {
                procedure: v.clone(),
                evaluation_digest: digest,
            },
        );
        let mut b = boundary();
        b.starts_at = at;
        self.push(
            at,
            PromotionCommand::StartCanary {
                procedure: v.clone(),
                boundary: b,
            },
        );
    }
    pub fn success(&mut self, v: &ProcedureVersion, at: i64, name: &str) {
        self.push(
            at,
            PromotionCommand::ReserveExecution {
                procedure: v.clone(),
                execution: execution(name, ExecutionMode::Canary),
            },
        );
        self.push(
            at,
            PromotionCommand::RecordOutcome {
                procedure: v.clone(),
                execution_id: id(name),
                outcome: RuntimeOutcome::Success,
                evidence: id("verified"),
            },
        );
    }
}
