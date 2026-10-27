use gateway_application::sessions::*;
use gateway_domain::{ContextScopeId, PlanStepId, ReferenceId};
use serde_json::{Value, json};

fn amounts(value: u64) -> SessionResourceAmounts {
    SessionResourceAmounts {
        model_attempts: value,
        input_tokens: value,
        output_tokens: value,
        output_bytes: value,
        cost_microunits: value,
        retrieval_rounds: value,
        retrieval_results: value,
        retrieval_tokens: value,
        repair_attempts: value,
        worker_attempts: value,
        worker_compute_units: value,
        connector_calls: value,
    }
}
fn limits(value: u64) -> SessionResourceLimits {
    SessionResourceLimits {
        cost_unit: ReferenceId::new("test-usd-microunits").unwrap(),
        amounts: amounts(value),
    }
}
fn binding(ordinal: u64) -> ResourceReservationBinding {
    ResourceReservationBinding {
        owner: OwnerBinding {
            principal: PrincipalId::new("operator").unwrap(),
            workspace: WorkspaceId::new("workspace").unwrap(),
            project: ProjectId::new("project-a").unwrap(),
            binding: BindingId::new("authenticated-binding").unwrap(),
            client_owner: ClientOwnerId::new("client").unwrap(),
        },
        scope: ContextScopeId::new("project-a").unwrap(),
        session: SessionId::new("session").unwrap(),
        run: RunId::new("run").unwrap(),
        revision: Revision::new(ordinal).unwrap(),
        step: PlanStepId::new(format!("step-{ordinal}")).unwrap(),
        dispatch: DispatchId::new(format!("dispatch-{ordinal}")).unwrap(),
    }
}
fn populated() -> SessionResourceLedger {
    let mut ledger = SessionResourceLedger::new(limits(100)).unwrap();
    ledger.reserve(binding(1), amounts(2)).unwrap();
    ledger.reserve(binding(3), amounts(3)).unwrap();
    ledger
}
fn resource_names() -> Vec<String> {
    serde_json::to_value(amounts(1))
        .unwrap()
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect()
}

#[test]
fn cumulative_reservations_roundtrip_without_refunds_or_admission_changes() {
    let mut ledger = SessionResourceLedger::new(limits(5)).unwrap();
    let empty = ledger.clone();
    let first = ledger.reserve(binding(1), amounts(2)).unwrap();
    assert_eq!(first.binding(), &binding(1));
    assert_eq!(first.amounts(), &amounts(2));
    first.validate().unwrap();
    assert_eq!(ledger.limits(), &limits(5));
    assert_eq!(ledger.used(), &amounts(2));
    ledger.validate_successor(&empty).unwrap();
    let after_first = ledger.clone();
    let second = ledger.reserve(binding(2), amounts(3)).unwrap();
    assert_ne!(first.id(), second.id());
    assert_eq!(ledger.used(), &amounts(5));
    assert_eq!(ledger.reservations(), &[first.clone(), second]);
    ledger.validate_successor(&after_first).unwrap();
    ledger.validate_successor(&ledger).unwrap();
    let retained: SessionResourceLedger =
        serde_json::from_slice(&serde_json::to_vec(&ledger).unwrap()).unwrap();
    assert_eq!(retained, ledger);
    let retained_reservation: SessionResourceReservation =
        serde_json::from_value(serde_json::to_value(first.clone()).unwrap()).unwrap();
    assert_eq!(retained_reservation, first);
    let unchanged = ledger.clone();
    assert_eq!(
        ledger.reserve(binding(4), amounts(1)),
        Err(SessionError::LimitExceeded)
    );
    assert_eq!(
        ledger, unchanged,
        "missing or smaller telemetry never restores capacity"
    );
}

#[test]
fn every_resource_has_an_independent_cap_and_checked_arithmetic() {
    for field in resource_names() {
        let mut requested = serde_json::to_value(SessionResourceAmounts::default()).unwrap();
        requested[&field] = json!(3);
        let requested: SessionResourceAmounts = serde_json::from_value(requested).unwrap();
        let mut ledger = SessionResourceLedger::new(limits(2)).unwrap();
        let old = ledger.clone();
        assert!(!requested.within(&limits(2).amounts), "{field}");
        assert_eq!(
            ledger.reserve(binding(1), requested),
            Err(SessionError::LimitExceeded),
            "{field}"
        );
        assert_eq!(ledger, old, "{field}");
        let mut maximum = serde_json::to_value(SessionResourceAmounts::default()).unwrap();
        maximum[&field] = json!(u64::MAX);
        let maximum: SessionResourceAmounts = serde_json::from_value(maximum).unwrap();
        assert_eq!(
            maximum.checked_add(&requested),
            Err(SessionError::LimitExceeded),
            "{field}"
        );
    }
    assert!(SessionResourceAmounts::default().is_zero());
    assert_eq!(amounts(1).checked_add(&amounts(2)).unwrap(), amounts(3));
    let mut ledger = SessionResourceLedger::new(limits(u64::MAX)).unwrap();
    ledger.reserve(binding(1), amounts(u64::MAX)).unwrap();
    let old = ledger.clone();
    assert_eq!(
        ledger.reserve(binding(2), amounts(1)),
        Err(SessionError::LimitExceeded)
    );
    assert_eq!(ledger, old);
}

#[test]
fn absent_allowance_zero_request_and_uncommitted_revision_are_denied() {
    assert_eq!(
        SessionResourceLedger::new(limits(0)),
        Err(SessionError::LimitExceeded)
    );
    let mut ledger = SessionResourceLedger::new(limits(10)).unwrap();
    let old = ledger.clone();
    assert_eq!(
        ledger.reserve(binding(1), SessionResourceAmounts::default()),
        Err(SessionError::InvalidInput)
    );
    assert_eq!(
        ledger.reserve(binding(0), amounts(1)),
        Err(SessionError::InvalidInput)
    );
    assert_eq!(ledger, old);
}

#[test]
fn owner_project_scope_run_and_dispatch_are_not_reusable_budget_handles() {
    let mut ledger = populated();
    let old = ledger.clone();
    assert_eq!(
        ledger.reserve(binding(3), amounts(1)),
        Err(SessionError::Duplicate)
    );
    let mut stale = binding(2);
    stale.dispatch = DispatchId::new("new-dispatch").unwrap();
    assert_eq!(
        ledger.reserve(stale, amounts(1)),
        Err(SessionError::StaleRevision)
    );
    let mut variants = vec![];
    let mut foreign = binding(4);
    foreign.owner.project = ProjectId::new("project-b").unwrap();
    variants.push(foreign);
    let mut foreign = binding(4);
    foreign.owner.principal = PrincipalId::new("other").unwrap();
    variants.push(foreign);
    let mut foreign = binding(4);
    foreign.owner.workspace = WorkspaceId::new("other").unwrap();
    variants.push(foreign);
    let mut foreign = binding(4);
    foreign.owner.binding = BindingId::new("other").unwrap();
    variants.push(foreign);
    let mut foreign = binding(4);
    foreign.owner.client_owner = ClientOwnerId::new("other").unwrap();
    variants.push(foreign);
    let mut foreign = binding(4);
    foreign.scope = ContextScopeId::new("project-b").unwrap();
    variants.push(foreign);
    let mut foreign = binding(4);
    foreign.session = SessionId::new("other").unwrap();
    variants.push(foreign);
    let mut foreign = binding(4);
    foreign.run = RunId::new("other").unwrap();
    variants.push(foreign);
    for foreign in variants {
        assert_eq!(
            ledger.reserve(foreign, amounts(1)),
            Err(SessionError::ScopeDenied)
        );
        assert_eq!(ledger, old);
    }
}

#[test]
fn persistence_rejects_tampered_reservations_and_inconsistent_sums() {
    let ledger = populated();
    let encoded = serde_json::to_value(&ledger).unwrap();
    for field in ["id", "admission_digest"] {
        let mut forged = encoded.clone();
        forged["reservations"][0][field] = json!("b".repeat(64));
        assert!(
            serde_json::from_value::<SessionResourceLedger>(forged).is_err(),
            "{field}"
        );
    }
    let mut mutations: Vec<Value> = vec![];
    let mut forged = encoded.clone();
    forged["reservations"][0]["amounts"]["input_tokens"] = json!(99);
    mutations.push(forged);
    let mut forged = encoded.clone();
    forged["reservations"][0]["binding"]["revision"] = json!(0);
    mutations.push(forged);
    let mut forged = encoded.clone();
    forged["used"]["input_tokens"] = json!(4);
    mutations.push(forged);
    let mut forged = encoded.clone();
    forged["used"]["input_tokens"] = json!(101);
    mutations.push(forged);
    let mut forged = encoded.clone();
    forged["limits"]["cost_unit"] = json!("different-currency");
    mutations.push(forged);
    let mut forged = encoded.clone();
    forged["limits"]["amounts"]["input_tokens"] = json!(101);
    mutations.push(forged);
    let mut forged = encoded.clone();
    forged["reservations"].as_array_mut().unwrap().reverse();
    mutations.push(forged);
    let mut forged = encoded.clone();
    forged["reservations"][1] = forged["reservations"][0].clone();
    forged["used"] = serde_json::to_value(amounts(4)).unwrap();
    mutations.push(forged);
    for forged in mutations {
        assert!(serde_json::from_value::<SessionResourceLedger>(forged).is_err());
    }
    let mut foreign = SessionResourceLedger::new(limits(100)).unwrap();
    let mut foreign_binding = binding(4);
    foreign_binding.owner.project = ProjectId::new("other").unwrap();
    let record = foreign.reserve(foreign_binding, amounts(1)).unwrap();
    let mut forged = encoded;
    forged["reservations"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::to_value(record).unwrap());
    forged["used"] = serde_json::to_value(amounts(6)).unwrap();
    assert!(serde_json::from_value::<SessionResourceLedger>(forged).is_err());
}

#[test]
fn reservation_identity_authenticates_admission_cost_unit_and_all_bound_facts() {
    let mut first = SessionResourceLedger::new(limits(10)).unwrap();
    let id = first.reserve(binding(1), amounts(1)).unwrap().id().clone();
    let mut different_limits = limits(10);
    different_limits.cost_unit = ReferenceId::new("other-currency").unwrap();
    let mut other = SessionResourceLedger::new(different_limits).unwrap();
    assert_ne!(other.reserve(binding(1), amounts(1)).unwrap().id(), &id);
    let mut other = SessionResourceLedger::new(limits(11)).unwrap();
    assert_ne!(other.reserve(binding(1), amounts(1)).unwrap().id(), &id);
    let mut other = SessionResourceLedger::new(limits(10)).unwrap();
    assert_ne!(other.reserve(binding(1), amounts(2)).unwrap().id(), &id);
    let mut other = SessionResourceLedger::new(limits(10)).unwrap();
    assert_ne!(other.reserve(binding(2), amounts(1)).unwrap().id(), &id);
}

#[test]
fn persisted_zero_reservations_and_overflowing_aggregate_charges_are_refused() {
    let ledger = populated();
    let mut zero = serde_json::to_value(&ledger.reservations()[0]).unwrap();
    zero["amounts"] = serde_json::to_value(SessionResourceAmounts::default()).unwrap();
    assert!(serde_json::from_value::<SessionResourceReservation>(zero).is_err());

    // Each record is valid on its own; combining them must still use checked
    // arithmetic when rebuilding the cumulative balance after restart.
    let mut first = SessionResourceLedger::new(limits(u64::MAX)).unwrap();
    first.reserve(binding(1), amounts(u64::MAX)).unwrap();
    let mut second = SessionResourceLedger::new(limits(u64::MAX)).unwrap();
    let extra = second.reserve(binding(2), amounts(1)).unwrap();
    let mut combined = serde_json::to_value(first).unwrap();
    combined["reservations"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::to_value(extra).unwrap());
    assert!(serde_json::from_value::<SessionResourceLedger>(combined).is_err());
}

#[test]
fn journal_successor_cannot_remove_replace_or_reprice_consumed_resources() {
    let ledger = populated();
    let mut next = ledger.clone();
    next.reserve(binding(4), amounts(1)).unwrap();
    next.validate_successor(&ledger).unwrap();
    assert_eq!(
        ledger.validate_successor(&next),
        Err(SessionError::InvalidState)
    );
    let empty = SessionResourceLedger::new(limits(100)).unwrap();
    assert_eq!(
        empty.validate_successor(&ledger),
        Err(SessionError::InvalidState)
    );
    let mut replaced = SessionResourceLedger::new(limits(100)).unwrap();
    replaced.reserve(binding(2), amounts(5)).unwrap();
    assert_eq!(replaced.used(), ledger.used());
    assert_eq!(
        replaced.validate_successor(&ledger),
        Err(SessionError::InvalidState)
    );
    let repriced = SessionResourceLedger::new(limits(101)).unwrap();
    assert_eq!(
        repriced.validate_successor(&empty),
        Err(SessionError::InvalidState)
    );
    let mut foreign_currency = limits(100);
    foreign_currency.cost_unit = ReferenceId::new("other").unwrap();
    assert_eq!(
        SessionResourceLedger::new(foreign_currency)
            .unwrap()
            .validate_successor(&empty),
        Err(SessionError::InvalidState)
    );
}

#[test]
fn unknown_fields_negative_units_missing_metrics_and_unbounded_history_are_rejected() {
    let ledger = populated();
    let encoded = serde_json::to_value(&ledger).unwrap();
    for pointer in [
        "",
        "/limits",
        "/limits/amounts",
        "/used",
        "/reservations/0",
        "/reservations/0/binding",
        "/reservations/0/amounts",
    ] {
        let mut forged = encoded.clone();
        forged.pointer_mut(pointer).unwrap()["unexpected"] = json!(true);
        assert!(
            serde_json::from_value::<SessionResourceLedger>(forged).is_err(),
            "{pointer}"
        );
    }
    for field in resource_names() {
        let mut missing = serde_json::to_value(amounts(1)).unwrap();
        missing.as_object_mut().unwrap().remove(&field);
        if field == "connector_calls" {
            assert_eq!(
                serde_json::from_value::<SessionResourceAmounts>(missing)
                    .unwrap()
                    .connector_calls,
                0
            );
        } else {
            assert!(serde_json::from_value::<SessionResourceAmounts>(missing).is_err());
        }
        let mut negative = serde_json::to_value(amounts(1)).unwrap();
        negative[&field] = json!(-1);
        assert!(serde_json::from_value::<SessionResourceAmounts>(negative).is_err());
    }
    let mut oversized = encoded;
    let reservation = oversized["reservations"][0].clone();
    oversized["reservations"] =
        Value::Array(vec![reservation; MAX_SESSION_RESOURCE_RESERVATIONS + 1]);
    assert!(serde_json::from_value::<SessionResourceLedger>(oversized).is_err());
}

fn phase_plan() -> SessionResourcePlan {
    SessionResourcePlan {
        preparation: SessionResourceAmounts {
            model_attempts: 2,
            input_tokens: 20,
            output_tokens: 10,
            output_bytes: 100,
            cost_microunits: 4,
            repair_attempts: 1,
            ..Default::default()
        },
        approved_resume: SessionResourceAmounts {
            cost_microunits: 3,
            retrieval_rounds: 1,
            retrieval_results: 2,
            retrieval_tokens: 10,
            ..Default::default()
        },
        execution: SessionResourceAmounts {
            cost_microunits: 5,
            retrieval_rounds: 2,
            retrieval_results: 3,
            retrieval_tokens: 20,
            worker_attempts: 1,
            worker_compute_units: 30,
            ..Default::default()
        },
        after_snapshot_recovery: SessionResourceAmounts {
            cost_microunits: 2,
            retrieval_rounds: 1,
            retrieval_results: 1,
            retrieval_tokens: 10,
            ..Default::default()
        },
        goal_verification: SessionResourceAmounts {
            cost_microunits: 1,
            retrieval_rounds: 1,
            retrieval_results: 1,
            retrieval_tokens: 10,
            ..Default::default()
        },
    }
}

#[test]
fn persisted_phase_plan_charges_exact_sum_and_identity_includes_the_partitions() {
    let plan = phase_plan();
    plan.validate().unwrap();
    let total = plan.total().unwrap();
    assert_eq!(total.model_attempts, 2);
    assert_eq!(total.cost_microunits, 15);
    assert_eq!(total.retrieval_rounds, 5);
    assert_eq!(total.worker_compute_units, 30);
    let mut ledger = SessionResourceLedger::new(limits(100)).unwrap();
    let reservation = ledger.reserve_plan(binding(1), plan.clone()).unwrap();
    assert_eq!(reservation.plan(), Some(&plan));
    assert_eq!(reservation.amounts(), &total);
    assert_eq!(ledger.used(), &total);
    let retained: SessionResourceLedger =
        serde_json::from_value(serde_json::to_value(&ledger).unwrap()).unwrap();
    assert_eq!(retained, ledger);
    let mut changed = plan.clone();
    changed.preparation.cost_microunits -= 1;
    changed.approved_resume.cost_microunits += 1;
    assert_eq!(changed.total().unwrap(), total);
    let mut different = SessionResourceLedger::new(limits(100)).unwrap();
    let replacement = different.reserve_plan(binding(1), changed).unwrap();
    assert_ne!(replacement.id(), reservation.id());
    assert_eq!(different.used(), ledger.used());
    assert_eq!(
        different.validate_successor(&ledger),
        Err(SessionError::InvalidState)
    );
    let mut unpartitioned = SessionResourceLedger::new(limits(100)).unwrap();
    assert_ne!(
        unpartitioned.reserve(binding(1), total).unwrap().id(),
        reservation.id()
    );
    let before = ledger.clone();
    assert_eq!(
        ledger.reserve_plan(binding(1), plan),
        Err(SessionError::Duplicate)
    );
    assert_eq!(ledger, before);
}

#[test]
fn phase_partitions_refuse_model_or_repair_allowance_outside_preparation() {
    for phase in [
        "approved_resume",
        "execution",
        "after_snapshot_recovery",
        "goal_verification",
    ] {
        for metric in [
            "model_attempts",
            "input_tokens",
            "output_tokens",
            "output_bytes",
            "repair_attempts",
        ] {
            let mut encoded = serde_json::to_value(phase_plan()).unwrap();
            encoded[phase][metric] = json!(1);
            assert!(
                serde_json::from_value::<SessionResourcePlan>(encoded).is_err(),
                "{phase}/{metric}"
            );
        }
    }
    assert_eq!(
        SessionResourcePlan::default().validate(),
        Err(SessionError::InvalidInput)
    );
    let mut missing = serde_json::to_value(phase_plan()).unwrap();
    missing.as_object_mut().unwrap().remove("execution");
    assert!(serde_json::from_value::<SessionResourcePlan>(missing).is_err());
    for pointer in [
        "",
        "/preparation",
        "/approved_resume",
        "/execution",
        "/after_snapshot_recovery",
        "/goal_verification",
    ] {
        let mut unknown = serde_json::to_value(phase_plan()).unwrap();
        unknown.pointer_mut(pointer).unwrap()["unexpected"] = json!(1);
        assert!(serde_json::from_value::<SessionResourcePlan>(unknown).is_err());
    }
}

#[test]
fn phase_sum_uses_checked_arithmetic_for_every_unit_and_denials_preserve_ledger() {
    for field in resource_names() {
        let mut first = serde_json::to_value(SessionResourceAmounts::default()).unwrap();
        first[&field] = json!(u64::MAX);
        let mut second = serde_json::to_value(SessionResourceAmounts::default()).unwrap();
        second[&field] = json!(1);
        let plan = SessionResourcePlan {
            preparation: serde_json::from_value(first).unwrap(),
            execution: serde_json::from_value(second).unwrap(),
            ..Default::default()
        };
        assert_eq!(plan.total(), Err(SessionError::LimitExceeded), "{field}");
        let mut ledger = SessionResourceLedger::new(limits(u64::MAX)).unwrap();
        let before = ledger.clone();
        assert!(ledger.reserve_plan(binding(1), plan).is_err());
        assert_eq!(ledger, before);
    }
    let mut ledger = SessionResourceLedger::new(limits(10)).unwrap();
    let before = ledger.clone();
    assert_eq!(
        ledger.reserve_plan(binding(1), phase_plan()),
        Err(SessionError::LimitExceeded)
    );
    assert_eq!(ledger, before);
}

#[test]
fn persisted_phase_plan_cannot_be_removed_repartitioned_or_detached_from_total() {
    let mut ledger = SessionResourceLedger::new(limits(100)).unwrap();
    ledger.reserve_plan(binding(1), phase_plan()).unwrap();
    let encoded = serde_json::to_value(&ledger).unwrap();
    for field in ["remove", "repartition", "total", "unknown", "invalid_phase"] {
        let mut forged = encoded.clone();
        match field {
            "remove" => {
                forged["reservations"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("plan");
            }
            "repartition" => {
                forged["reservations"][0]["plan"]["preparation"]["cost_microunits"] = json!(3);
                forged["reservations"][0]["plan"]["approved_resume"]["cost_microunits"] = json!(4);
            }
            "total" => {
                forged["reservations"][0]["amounts"]["cost_microunits"] = json!(16);
            }
            "unknown" => {
                forged["reservations"][0]["plan"]["new_phase"] = json!({});
            }
            _ => {
                forged["reservations"][0]["plan"]["approved_resume"]["model_attempts"] = json!(1);
            }
        }
        assert!(
            serde_json::from_value::<SessionResourceLedger>(forged).is_err(),
            "{field}"
        );
    }
}

#[test]
fn legacy_reservation_keeps_exact_v1_digest_and_cannot_gain_an_unsigned_phase_plan() {
    let mut ledger = SessionResourceLedger::new(limits(100)).unwrap();
    let reservation = ledger
        .reserve(binding(1), phase_plan().total().unwrap())
        .unwrap();
    assert!(reservation.plan().is_none());
    let encoded = serde_json::to_value(&reservation).unwrap();
    assert!(encoded.get("plan").is_none());
    let expected = gateway_domain::context_content_digest(
        &serde_json::to_vec(&(
            "cg.session-resource-reservation/1.0",
            &encoded["admission_digest"],
            reservation.binding(),
            reservation.amounts(),
        ))
        .unwrap(),
    );
    assert_eq!(reservation.id(), &expected);
    let retained: SessionResourceReservation = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(retained, reservation);
    let mut extended = encoded.clone();
    extended["plan"] = serde_json::to_value(phase_plan()).unwrap();
    assert!(serde_json::from_value::<SessionResourceReservation>(extended).is_err());
    let mut explicit_none = encoded;
    explicit_none["plan"] = Value::Null;
    assert_eq!(
        serde_json::from_value::<SessionResourceReservation>(explicit_none).unwrap(),
        reservation
    );
}

#[test]
fn zero_connector_calls_preserve_exact_pre_extension_legacy_bytes_and_digests() {
    // Golden bytes use only the original eleven resource fields. The hashes
    // were calculated from that original v1 wire contract, not the new serde.
    let golden = r#"{"limits":{"cost_unit":"legacy-unit","amounts":{"model_attempts":10,"input_tokens":10,"output_tokens":10,"output_bytes":10,"cost_microunits":10,"retrieval_rounds":10,"retrieval_results":10,"retrieval_tokens":10,"repair_attempts":10,"worker_attempts":10,"worker_compute_units":10}},"used":{"model_attempts":1,"input_tokens":1,"output_tokens":1,"output_bytes":1,"cost_microunits":1,"retrieval_rounds":1,"retrieval_results":1,"retrieval_tokens":1,"repair_attempts":1,"worker_attempts":1,"worker_compute_units":1},"reservations":[{"id":"6a047de0672df55d1245ff3f88ea534a87b5b2e3e43c9c6e1cbc7537b355954f","admission_digest":"e629b03010f3b2f7482e7effdc39831b99e841a2bf3f77aa91ef203cfd362620","binding":{"owner":{"principal":"operator","workspace":"workspace","project":"project-a","binding":"authenticated-binding","client_owner":"client"},"scope":"project-a","session":"session","run":"run","revision":1,"step":"step-1","dispatch":"dispatch-1"},"amounts":{"model_attempts":1,"input_tokens":1,"output_tokens":1,"output_bytes":1,"cost_microunits":1,"retrieval_rounds":1,"retrieval_results":1,"retrieval_tokens":1,"repair_attempts":1,"worker_attempts":1,"worker_compute_units":1}}]}"#;
    let retained: SessionResourceLedger = serde_json::from_str(golden).unwrap();
    assert_eq!(retained.used().connector_calls, 0);
    assert_eq!(retained.limits().amounts.connector_calls, 0);
    assert_eq!(
        retained.reservations()[0].id().as_str(),
        "6a047de0672df55d1245ff3f88ea534a87b5b2e3e43c9c6e1cbc7537b355954f"
    );
    assert_eq!(serde_json::to_string(&retained).unwrap(), golden);
    let mut explicit_zero = serde_json::to_value(&retained).unwrap();
    for pointer in ["/limits/amounts", "/used", "/reservations/0/amounts"] {
        explicit_zero.pointer_mut(pointer).unwrap()["connector_calls"] = json!(0);
    }
    assert_eq!(
        serde_json::from_value::<SessionResourceLedger>(explicit_zero).unwrap(),
        retained
    );
    let mut forged = serde_json::to_value(&retained).unwrap();
    forged["reservations"][0]["amounts"]["connector_calls"] = json!(1);
    assert!(serde_json::from_value::<SessionResourceLedger>(forged).is_err());
}

#[test]
fn connector_operation_ceiling_is_cumulative_disjoint_and_never_implicitly_unlimited() {
    let zero_legacy: SessionResourceAmounts = serde_json::from_value({
        let mut wire = serde_json::to_value(amounts(0)).unwrap();
        wire.as_object_mut().unwrap().remove("connector_calls");
        wire
    })
    .unwrap();
    let operation = SessionResourceAmounts {
        connector_calls: 1,
        ..Default::default()
    };
    assert!(!operation.within(&zero_legacy));
    assert_eq!(
        zero_legacy.checked_sub(&operation),
        Err(SessionError::LimitExceeded)
    );
    let mut ledger = SessionResourceLedger::new(SessionResourceLimits {
        cost_unit: ReferenceId::new("credits").unwrap(),
        amounts: SessionResourceAmounts {
            connector_calls: 3,
            ..Default::default()
        },
    })
    .unwrap();
    let plan = SessionResourcePlan {
        preparation: operation,
        approved_resume: operation,
        execution: operation,
        ..Default::default()
    };
    assert_eq!(plan.total().unwrap().connector_calls, 3);
    let reservation = ledger.reserve_plan(binding(1), plan.clone()).unwrap();
    assert_eq!(ledger.used().connector_calls, 3);
    let retained: SessionResourceLedger =
        serde_json::from_value(serde_json::to_value(&ledger).unwrap()).unwrap();
    assert_eq!(retained, ledger);
    assert_eq!(reservation.plan(), Some(&plan));
    let unchanged = ledger.clone();
    assert_eq!(
        ledger.reserve(binding(2), operation),
        Err(SessionError::LimitExceeded)
    );
    assert_eq!(ledger, unchanged);
    let mut independent = SessionResourceLedger::new(SessionResourceLimits {
        cost_unit: ReferenceId::new("credits").unwrap(),
        amounts: amounts(100),
    })
    .unwrap();
    independent.reserve_plan(binding(1), plan).unwrap();
    assert_eq!(independent.used().model_attempts, 0);
    assert_eq!(independent.used().retrieval_rounds, 0);
    assert_eq!(independent.used().connector_calls, 3);
}
