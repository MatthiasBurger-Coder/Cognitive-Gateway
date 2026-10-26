//! #401 controlled COMPONENT integration. No installed policy source, model
//! inference, live mutation or session completion is qualified by this fixture.
use gateway_application::{
    connector_manifest::*, connector_policy::*, connector_proposals::*, connectors::*,
    output_verification::*, policy_application::*, prompt_ir::*, prompt_rendering::*,
};
use gateway_daemon::mcp_client::{mapping::*, normalization::*, transport::*, *};
use gateway_domain::{
    model_invocation::*, output_verification::*, prompt_ir::*, semantic_output::*, *,
};
use gateway_policy::*;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
#[allow(dead_code)]
#[path = "../../gateway-application/tests/support/composition.rs"]
mod composition;
#[path = "support/connector_proposal_fixture.rs"]
mod fixture;
#[path = "../../../tests/fixtures/prompt-rendering-v1/support.rs"]
mod prompt;
#[path = "../../gateway-application/tests/support/mod.rs"]
mod support;
use fixture::*;

#[test]
fn admitted_and_verified_native_inspection_crosses_actual_mapper_cg09_and_stdio_once() {
    let f = Fixture::new(false, "normal");
    let verified = f.verified(1);
    let batch = f.dispatch(&f.ir, &verified).unwrap();
    assert!(batch.all_calls_returned, "{:?}", batch.failure);
    assert_eq!(batch.results.len(), 1);
    assert!(batch.failure.is_none());
    let r = &batch.results[0];
    assert_eq!(r.request.task, f.ir.prepared().invocation().data().task.id);
    assert_eq!(r.request.step, f.policy.lock().unwrap().native.step);
    assert_eq!(r.request.execution, f.coordinates.lock().unwrap().execution);
    assert_eq!(r.observation.target, r.request.target);
    assert_eq!(r.observation.correlation, r.request.correlation);
    assert_eq!(
        r.observation.inventory_revision,
        r.request.inventory_revision
    );
    assert_eq!(f.calls(), 1);
    assert_eq!(f.policy.lock().unwrap().audits, 2);
}
#[test]
fn current_native_policy_denial_missing_mutation_consent_and_pre_effect_change_send_zero() {
    for mode in ["authorization", "consent", "process", "stale"] {
        let f = Fixture::new(mode == "consent", "normal");
        let verified = f.verified(1);
        {
            let mut state = f.policy.lock().unwrap();
            let step = state.native.step.clone();
            let cap = state.native.capability.clone();
            match mode {
                "authorization" => {
                    state
                        .native
                        .context
                        .steps
                        .get_mut(&step)
                        .unwrap()
                        .authorizations
                        .insert(cap, Approval::Denied);
                }
                "consent" => {
                    state
                        .native
                        .context
                        .steps
                        .get_mut(&step)
                        .unwrap()
                        .consents
                        .clear();
                }
                "process" => {
                    let constraints = &mut state
                        .native
                        .context
                        .steps
                        .get_mut(&step)
                        .unwrap()
                        .satisfied_constraints;
                    assert!(constraints.remove("[\"primary-agent\",\"alpha\"]"));
                }
                _ => state.change_before_effect = true,
            }
        }
        let batch = f.dispatch(&f.ir, &verified).unwrap();
        assert!(batch.results.is_empty(), "{mode}");
        assert!(
            matches!(
                batch.failure,
                Some((
                    0,
                    ProposalError::Connector(ConnectorFailure {
                        effect: ConnectorEffect::NotDispatched,
                        ..
                    })
                ))
            ),
            "{mode}"
        );
        assert_eq!(f.calls(), 0, "{mode}");
    }
}
#[test]
fn verified_proposals_cannot_escape_current_inventory_scope_or_semantic_references() {
    for change in [
        "inventory",
        "schema",
        "scope",
        "reference",
        "step-during-discovery",
        "execution-during-discovery",
    ] {
        let f = Fixture::new(false, "normal");
        let verified = f.verified(1);
        match change {
            "inventory" => f.discovery_fault.store(1, Ordering::SeqCst),
            "schema" => f.discovery_fault.store(2, Ordering::SeqCst),
            "scope" => {
                f.coordinates.lock().unwrap().target.scope =
                    ContextScopeId::new("foreign-project").unwrap()
            }
            "reference" => {
                f.admission.references_revoked.store(true, Ordering::SeqCst);
                assert!(
                    f.admission.validate_current(&f.ir).is_ok(),
                    "IR equality alone cannot replace native reference admission"
                );
            }
            "step-during-discovery" => f.discovery_fault.store(3, Ordering::SeqCst),
            _ => f.discovery_fault.store(4, Ordering::SeqCst),
        }
        let batch = f.dispatch(&f.ir, &verified).unwrap();
        assert!(batch.results.is_empty(), "{change}");
        assert!(batch.failure.is_some(), "{change}");
        assert_eq!(f.calls(), 0, "{change}");
    }
    let unpinned = Fixture::without_pin();
    let verified = unpinned.verified(1);
    let batch = unpinned.dispatch(&unpinned.ir, &verified).unwrap();
    assert!(batch.results.is_empty());
    assert!(batch.failure.is_some());
    assert_eq!(unpinned.calls(), 0);
}
#[test]
fn unverified_foreign_capability_arguments_and_invocation_do_not_reach_the_peer() {
    let f = Fixture::new(false, "normal");
    for field in ["capability", "arguments", "scope"] {
        let mut raw = f.wire(1);
        match field {
            "capability" => raw["tools"][0]["capability"]["revision"] = json!("foreign-revision"),
            "scope" => raw["tools"][0]["capability"]["scope"] = json!("foreign-project"),
            _ => raw["tools"][0]["arguments"]["query"] = json!(123),
        }
        assert!(
            gateway_application::output_parsing::parse_invocation_output(
                &f.ir,
                &serde_json::to_vec(&raw).unwrap()
            )
            .is_err(),
            "{field}"
        );
        assert_eq!(f.calls(), 0);
    }
    let foreign = Fixture::new(false, "normal");
    let verified = foreign.verified(1);
    assert!(matches!(
        f.dispatch(&f.ir, &verified),
        Err(ProposalError::StaleInvocation)
    ));
    assert_eq!(f.calls(), 0);
}
#[test]
fn ordered_results_stop_at_policy_denial_and_identical_proposals_have_distinct_instances() {
    let f = Fixture::new(false, "normal");
    let verified = f.verified(3);
    f.policy.lock().unwrap().deny_after = Some(1);
    let batch = f.dispatch(&f.ir, &verified).unwrap();
    assert!(!batch.all_calls_returned);
    assert_eq!(batch.results.len(), 1);
    assert!(matches!(batch.failure, Some((1, _))));
    assert_eq!(f.calls(), 1);
    let all = Fixture::new(false, "normal");
    let verified = all.verified(2);
    let batch = all.dispatch(&all.ir, &verified).unwrap();
    assert!(batch.all_calls_returned, "{:?}", batch.failure);
    assert_eq!(all.calls(), 2);
    assert_ne!(
        batch.results[0].request.correlation,
        batch.results[1].request.correlation
    );
}
#[test]
fn ambiguous_mutation_send_keeps_prior_observation_and_never_sends_later_proposals() {
    let f = Fixture::new(true, "eof-second");
    let verified = f.verified(3);
    let batch = f.dispatch(&f.ir, &verified).unwrap();
    assert_eq!(batch.results.len(), 1);
    assert!(matches!(
        batch.failure,
        Some((
            1,
            ProposalError::Connector(ConnectorFailure {
                effect: ConnectorEffect::Unknown,
                ..
            })
        ))
    ));
    assert_eq!(f.calls(), 2);
}
