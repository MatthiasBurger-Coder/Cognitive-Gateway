#[path = "../../../tests/support/procedure_promotion.rs"]
mod support;
use gateway_domain::{ContentDigest, procedure_promotion::*};
use gateway_registry::learned_procedures::LearnedProcedureRegistry;
use support::*;

#[test]
fn versioned_supersession_and_rollback_preserve_all_evidence() {
    let mut h = History::default();
    let v1 = h.discover(1);
    let v2 = h.discover(2);
    h.canary(&v1, 20);
    h.success(&v1, 20, "canary-1");
    h.push(
        20,
        PromotionCommand::Activate {
            procedure: v1.clone(),
        },
    );
    h.push(
        20,
        PromotionCommand::ReserveExecution {
            procedure: v1.clone(),
            execution: execution("active-1", ExecutionMode::Active),
        },
    );
    h.canary(&v2, 20);
    h.success(&v2, 20, "canary-2");
    h.rejects(
        20,
        PromotionCommand::Activate {
            procedure: v2.clone(),
        },
    );
    h.push(
        20,
        PromotionCommand::Supersede {
            procedure: v2.clone(),
            previous: v1.clone(),
        },
    );
    let r = h.registry();
    assert_eq!(r.active(&v1.id), Some(&v2));
    assert_eq!(r.get(&v1).unwrap().state(), PromotionState::Superseded);
    assert_eq!(r.get(&v2).unwrap().predecessor(), Some(&v1));
    assert_eq!(
        r.get(&v1).unwrap().history().last(),
        r.get(&v2).unwrap().history().last()
    );
    h.rejects(
        21,
        PromotionCommand::Rollback {
            procedure: v2.clone(),
            restore: None,
        },
    );
    h.rejects(
        21,
        PromotionCommand::Rollback {
            procedure: v2.clone(),
            restore: Some(v2.clone()),
        },
    );
    h.push(
        21,
        PromotionCommand::Rollback {
            procedure: v2.clone(),
            restore: Some(v1.clone()),
        },
    );
    h.push(
        21,
        PromotionCommand::RecordOutcome {
            procedure: v1.clone(),
            execution_id: id("active-1"),
            outcome: RuntimeOutcome::ExecutionFailed,
            evidence: id("late-result"),
        },
    );
    let r = h.registry();
    assert_eq!(r.active(&v1.id), Some(&v1));
    assert_eq!(r.get(&v2).unwrap().state(), PromotionState::RolledBack);
    assert!(r.get(&v1).unwrap().evaluation().is_some());
    assert!(r.get(&v2).unwrap().canary().is_some());
    assert_eq!(
        r.get(&v1).unwrap().executions()[&id("active-1")].outcome_evidence,
        Some(id("late-result"))
    );
    let json = serde_json::to_string(&h.journal).unwrap();
    let replay: PromotionJournal = serde_json::from_str(&json).unwrap();
    assert_eq!(r, LearnedProcedureRegistry::from_journal(&replay).unwrap());
    if let Ok(path) = std::env::var("CG24_PROMOTION_OUTPUT") {
        std::fs::write(path, serde_json::to_vec_pretty(&h.journal).unwrap()).unwrap();
    }
    h.rejects(
        22,
        PromotionCommand::Activate {
            procedure: v2.clone(),
        },
    );
    h.push(
        22,
        PromotionCommand::Rollback {
            procedure: v1.clone(),
            restore: None,
        },
    );
    assert!(h.registry().active(&v1.id).is_none());
}

#[test]
fn illegal_edges_evidence_tampering_identity_and_audit_rejections() {
    let mut h = History::default();
    let v = h.discover(1);
    h.rejects(
        10,
        PromotionCommand::Discover {
            procedure: Box::new(procedure(1)),
            discovery_evidence: id("duplicate"),
        },
    );
    let mut altered = procedure(1);
    let mut json = serde_json::to_value(&altered).unwrap();
    json["digest"] = "a".repeat(64).into();
    altered = serde_json::from_value(json).unwrap();
    let empty = History::default();
    empty.rejects(
        10,
        PromotionCommand::Discover {
            procedure: Box::new(altered),
            discovery_evidence: id("tampered"),
        },
    );
    for from in [PromotionState::Discovered, PromotionState::Candidate] {
        h.rejects(
            10,
            PromotionCommand::Advance {
                procedure: v.clone(),
                from,
                to: PromotionState::Active,
                evidence: id("invalid"),
            },
        );
    }
    h.rejects(
        10,
        PromotionCommand::Evaluate {
            procedure: v.clone(),
            bundle: Box::new(bundle(1)),
        },
    );
    h.rejects(
        10,
        PromotionCommand::Approve {
            procedure: v.clone(),
            evaluation_digest: bundle(1).digest,
        },
    );
    h.rejects(
        10,
        PromotionCommand::StartCanary {
            procedure: v.clone(),
            boundary: boundary(),
        },
    );
    h.rejects(
        10,
        PromotionCommand::Disable {
            procedure: v.clone(),
            reason: id("invalid"),
        },
    );
    h.rejects(
        10,
        PromotionCommand::Rollback {
            procedure: v.clone(),
            restore: None,
        },
    );
    h.rejects(
        10,
        PromotionCommand::Activate {
            procedure: v.clone(),
        },
    );
    let mut wrong = v.clone();
    wrong.digest = ContentDigest::new("a".repeat(64)).unwrap();
    assert!(h.registry().get(&wrong).is_none());
    h.rejects(
        10,
        PromotionCommand::Advance {
            procedure: wrong,
            from: PromotionState::Discovered,
            to: PromotionState::Candidate,
            evidence: id("wrong-digest"),
        },
    );
    h.push(
        10,
        PromotionCommand::Advance {
            procedure: v.clone(),
            from: PromotionState::Discovered,
            to: PromotionState::Candidate,
            evidence: id("candidate"),
        },
    );
    h.rejects(
        10,
        PromotionCommand::Activate {
            procedure: v.clone(),
        },
    );
    h.push(
        10,
        PromotionCommand::Advance {
            procedure: v.clone(),
            from: PromotionState::Candidate,
            to: PromotionState::Validated,
            evidence: id("validated"),
        },
    );
    let mut b = bundle(1);
    b.report.passed = false;
    h.rejects(
        10,
        PromotionCommand::Evaluate {
            procedure: v.clone(),
            bundle: Box::new(b),
        },
    );
    h.rejects(
        10,
        PromotionCommand::Evaluate {
            procedure: v.clone(),
            bundle: Box::new(bundle(2)),
        },
    );
    h.push(
        10,
        PromotionCommand::Evaluate {
            procedure: v.clone(),
            bundle: Box::new(bundle(1)),
        },
    );
    h.rejects(
        10,
        PromotionCommand::Approve {
            procedure: v.clone(),
            evaluation_digest: bundle(2).digest,
        },
    );
    let mut reversed = h.journal.clone();
    reversed.events.last_mut().unwrap().metadata.at = 9;
    assert!(LearnedProcedureRegistry::from_journal(&reversed).is_err());
    let mut repeated = h.journal.clone();
    repeated.events[1].metadata.id = repeated.events[0].metadata.id.clone();
    assert!(LearnedProcedureRegistry::from_journal(&repeated).is_err());
    let mut wrong_version = h.journal.clone();
    wrong_version.schema_version = 2;
    assert!(LearnedProcedureRegistry::from_journal(&wrong_version).is_err());
    h.push(
        10,
        PromotionCommand::Advance {
            procedure: v.clone(),
            from: PromotionState::Evaluated,
            to: PromotionState::Rejected,
            evidence: id("reject"),
        },
    );
    h.rejects(
        10,
        PromotionCommand::Advance {
            procedure: v.clone(),
            from: PromotionState::Rejected,
            to: PromotionState::Candidate,
            evidence: id("restart"),
        },
    );
    assert_eq!(
        h.registry().get(&v).unwrap().state(),
        PromotionState::Rejected
    );
}

#[test]
fn canary_bounds_pending_failed_and_late_outcomes_are_enforced() {
    let mut h = History::default();
    let v = h.discover(1);
    h.canary(&v, 20);
    let r = h.registry();
    let scope = boundary().scope;
    for (at, cohort, mode) in [
        (19, id("pilot"), ExecutionMode::Canary),
        (100, id("pilot"), ExecutionMode::Canary),
        (20, id("public"), ExecutionMode::Canary),
        (20, id("pilot"), ExecutionMode::Active),
    ] {
        assert!(r.eligible(&v, &scope, &cohort, mode, at).is_err());
    }
    assert!(
        r.eligible(
            &v,
            &gateway_domain::ContextScopeId::new("other-project").unwrap(),
            &id("pilot"),
            ExecutionMode::Canary,
            20
        )
        .is_err()
    );
    assert!(
        r.eligible(&v, &scope, &id("pilot"), ExecutionMode::Canary, -1)
            .is_err()
    );
    h.rejects(
        20,
        PromotionCommand::Activate {
            procedure: v.clone(),
        },
    );
    h.rejects(
        20,
        PromotionCommand::RecordOutcome {
            procedure: v.clone(),
            execution_id: id("unreserved"),
            outcome: RuntimeOutcome::Success,
            evidence: id("false"),
        },
    );
    h.push(
        20,
        PromotionCommand::ReserveExecution {
            procedure: v.clone(),
            execution: execution("trial", ExecutionMode::Canary),
        },
    );
    h.rejects(
        20,
        PromotionCommand::ReserveExecution {
            procedure: v.clone(),
            execution: execution("another", ExecutionMode::Canary),
        },
    );
    h.rejects(
        20,
        PromotionCommand::Activate {
            procedure: v.clone(),
        },
    );
    h.push(
        20,
        PromotionCommand::RecordOutcome {
            procedure: v.clone(),
            execution_id: id("trial"),
            outcome: RuntimeOutcome::VerificationFailed,
            evidence: id("failure"),
        },
    );
    h.rejects(
        20,
        PromotionCommand::RecordOutcome {
            procedure: v.clone(),
            execution_id: id("trial"),
            outcome: RuntimeOutcome::Success,
            evidence: id("overwrite"),
        },
    );
    h.rejects(
        20,
        PromotionCommand::ReserveExecution {
            procedure: v.clone(),
            execution: execution("trial-2", ExecutionMode::Canary),
        },
    );
    h.rejects(
        20,
        PromotionCommand::Activate {
            procedure: v.clone(),
        },
    );
    h.rejects(
        20,
        PromotionCommand::Rollback {
            procedure: v.clone(),
            restore: Some(v.clone()),
        },
    );
    h.push(
        20,
        PromotionCommand::Rollback {
            procedure: v.clone(),
            restore: None,
        },
    );
    assert!(h.registry().active(&v.id).is_none());
    h.rejects(
        20,
        PromotionCommand::Disable {
            procedure: v.clone(),
            reason: id("again"),
        },
    );
}

#[test]
fn execution_budget_disable_and_supersession_guards() {
    let mut h = History::default();
    let v1 = h.discover(1);
    let v2 = h.discover(2);
    h.canary(&v1, 20);
    h.success(&v1, 20, "trial1");
    h.success(&v1, 20, "trial2");
    h.rejects(
        20,
        PromotionCommand::ReserveExecution {
            procedure: v1.clone(),
            execution: execution("trial3", ExecutionMode::Canary),
        },
    );
    h.push(
        20,
        PromotionCommand::Activate {
            procedure: v1.clone(),
        },
    );
    h.rejects(
        20,
        PromotionCommand::ReserveExecution {
            procedure: v1.clone(),
            execution: execution("trial1", ExecutionMode::Active),
        },
    );
    h.canary(&v2, 20);
    h.success(&v2, 20, "newtrial");
    h.rejects(
        20,
        PromotionCommand::Supersede {
            procedure: v2.clone(),
            previous: v2.clone(),
        },
    );
    h.push(
        20,
        PromotionCommand::Supersede {
            procedure: v2.clone(),
            previous: v1.clone(),
        },
    );
    h.push(
        20,
        PromotionCommand::Disable {
            procedure: v1.clone(),
            reason: id("revoked-safe-version"),
        },
    );
    h.rejects(
        20,
        PromotionCommand::Rollback {
            procedure: v2.clone(),
            restore: Some(v1.clone()),
        },
    );
    h.push(
        20,
        PromotionCommand::ReserveExecution {
            procedure: v2.clone(),
            execution: execution("late", ExecutionMode::Active),
        },
    );
    h.push(
        20,
        PromotionCommand::Disable {
            procedure: v2.clone(),
            reason: id("disable-active"),
        },
    );
    h.push(
        21,
        PromotionCommand::RecordOutcome {
            procedure: v2.clone(),
            execution_id: id("late"),
            outcome: RuntimeOutcome::Refused,
            evidence: id("refusal"),
        },
    );
    assert_eq!(
        h.registry().get(&v2).unwrap().state(),
        PromotionState::Deprecated
    );
    assert!(h.registry().active(&v2.id).is_none());
    h.rejects(
        21,
        PromotionCommand::ReserveExecution {
            procedure: v2.clone(),
            execution: execution("disabled", ExecutionMode::Active),
        },
    );
}
