use gateway_domain::{ReferenceId, learning::LearnedProcedure, procedure_promotion::*};
fn procedure() -> LearnedProcedure {
    LearnedProcedure::from_json(include_str!(
        "../../../tests/fixtures/procedure-evaluation-v1/procedure.json"
    ))
    .unwrap()
}
#[test]
fn finite_lifecycle_edges_and_strict_versioned_wire() {
    use PromotionState::*;
    let states = [
        Discovered, Candidate, Validated, Evaluated, Approved, Canary, Active, Rejected,
        Deprecated, RolledBack, Superseded,
    ];
    for from in states {
        for to in states {
            assert_eq!(
                from.allows_advance(to),
                matches!(
                    (from, to),
                    (Discovered, Candidate | Rejected)
                        | (Candidate, Validated | Rejected)
                        | (Validated | Evaluated, Rejected)
                )
            );
        }
    }
    let p = procedure();
    let version = ProcedureVersion::of(&p);
    assert_eq!(version.id, *p.id());
    assert_eq!(version.version, p.version());
    assert_eq!(version.digest, *p.digest());
    assert_eq!(PromotionJournal::default().schema_version, 1);
    assert!(
        serde_json::from_str::<PromotionJournal>(
            r#"{"schema_version":1,"events":[],"role":"GOVERNOR"}"#
        )
        .is_err()
    );
    let command = PromotionCommand::Discover {
        procedure: Box::new(p),
        discovery_evidence: ReferenceId::new("source").unwrap(),
    };
    assert!(!command.is_runtime());
    assert!(serde_json::from_str::<PromotionCommand>(r#"{"kind":"ACTIVATE","procedure":{"id":"p","version":1,"digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"actor":"approver"}"#).is_err());
    let runtime = PromotionCommand::RecordOutcome {
        procedure: version,
        execution_id: ReferenceId::new("exec").unwrap(),
        outcome: RuntimeOutcome::Success,
        evidence: ReferenceId::new("evidence").unwrap(),
    };
    assert!(runtime.is_runtime());
}
#[test]
fn explicit_canary_boundaries_fail_closed() {
    let p = procedure();
    let b = CanaryBoundary {
        scope: p.fingerprint().scope().clone(),
        cohorts: [ReferenceId::new("pilot").unwrap()].into(),
        starts_at: 10,
        ends_at: 20,
        max_executions: 2,
        max_failures: 0,
        required_successes: 1,
    };
    b.validate(&p).unwrap();
    for field in 0..9 {
        let mut changed = b.clone();
        match field {
            0 => changed.scope = gateway_domain::ContextScopeId::new("wrong-scope").unwrap(),
            1 => changed.cohorts.clear(),
            2 => changed.starts_at = -1,
            3 => changed.ends_at = 10,
            4 => changed.max_executions = 0,
            5 => changed.required_successes = 0,
            6 => changed.required_successes = 3,
            7 => changed.max_failures = 2,
            _ => changed.ends_at = 9,
        }
        assert!(changed.validate(&p).is_err());
    }
}
