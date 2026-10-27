//! Opaque actual CG09 evaluation reaches real PG under an actual coordinator child.
use super::*;
use crate::native_policy_test_support as native_fixture;
use gateway_application::{connector_policy::*, connectors::*, policy_application::*};
use gateway_domain::*;
use native_fixture::NativePolicy;
use std::sync::Arc;
fn native(scope: ContextScopeId) -> NativePolicy {
    native_fixture::native(observed::goal(scope), OperatingMode::Hardening)
}
fn text(v: &str) -> NonEmptyText {
    NonEmptyText::new(v).unwrap()
}
struct Source<'a> {
    repo: &'a PostgresSessionStore,
    allocation: &'a SessionResourceAllocation,
    cp: &'a SessionCheckpoint,
    native: &'a NativePolicy,
    fault: &'a str,
}
impl Source<'_> {
    fn snapshot(&self, r: &ConnectorRequest) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        let n = PolicyApplication
            .step_input(
                &self.native.resolved,
                &self.native.context,
                &self.allocation.binding().step,
            )
            .unwrap();
        let mut authority = self.native.authority.clone();
        if self.fault == "finding" {
            authority.required_evidence.insert(
                r.capability.clone(),
                ["CANARY-PRIVATE-FINDING".into()].into(),
            );
        }
        if self.fault == "oversized" {
            authority.required_evidence.insert(
                r.capability.clone(),
                (0..257).map(|n| format!("evidence-{n}")).collect(),
            );
        }
        Ok(ConnectorPolicySnapshot {
            step: n.step,
            resolved: n.resolved,
            has_prerequisites: n.has_prerequisites,
            constraints: n.constraints,
            preconditions: n.preconditions,
            authority,
            facts: n.facts,
            operating_mode: n.operating_mode,
            execution_profile: n.execution_profile,
            process: n.process,
            consent_required: false,
            administrative_capabilities: Default::default(),
            policy: PolicyId::new("audit-native-policy").unwrap(),
            policy_revision: context_content_digest(self.native.goal.plan().digest().as_bytes()),
            consent_receipt: None,
            budget_reservation: text(if self.fault == "reservation" {
                "foreign-reservation"
            } else {
                self.allocation.reservation_id().as_str()
            }),
            deadline: UnixTimestamp::new((self.cp.lease_until_ms.unwrap() / 1000) as i64),
        })
    }
}
impl ConnectorPolicySource for Source<'_> {
    fn admit_target(&self, t: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        if t.scope == *self.cp.goal.scope() && t.connector == text("fixture.observed") {
            Ok(())
        } else {
            Err(denied())
        }
    }
    fn authorize(
        &self,
        r: &ConnectorRequest,
        _: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        self.snapshot(r)
    }
    fn revalidate(
        &self,
        d: &ConnectorDispatch,
        _: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure> {
        self.snapshot(&d.request)
    }
    fn record_decision(&self, e: &ConnectorPolicyEvaluation<'_>) -> Result<(), ConnectorFailure> {
        if self.fault == "stale" {
            let current = self
                .repo
                .load(&self.cp.owner, &self.cp.snapshot.session)
                .unwrap();
            let mut tx = append(&current);
            tx.next.snapshot.state = SessionState::Failed;
            tx.next.snapshot.dispatch_knowledge = DispatchKnowledge::Stopped;
            tx.next.lease_until_ms = None;
            self.repo.append(&self.cp.owner, tx).unwrap();
        }
        if self.fault == "expired" {
            self.repo
                .run(|j| {
                    let i = j.index(&self.cp.owner, &self.cp.snapshot.session)?;
                    j.sessions[i].checkpoint.lease_until_ms = Some(1);
                    Ok(())
                })
                .unwrap();
        }
        let mut actual = self.cp.owner.clone();
        if self.fault == "owner" {
            actual.project = ProjectId::new("foreign-project").unwrap()
        }
        self.repo
            .record_observation_policy_decision(&actual, self.allocation, e)
            .map_err(|_| denied())
    }
}
fn denied() -> ConnectorFailure {
    ConnectorFailure {
        code: ConnectorFailureCode::PermissionDenied,
        effect: ConnectorEffect::NotDispatched,
        correlation: text("audit-denied"),
    }
}
fn request(a: &SessionResourceAllocation, native: &NativePolicy) -> ConnectorRequest {
    ConnectorRequest {
        target: ConnectorTarget {
            connector: text("fixture.observed"),
            scope: a.binding().scope.clone(),
        },
        capability: native.capability.clone(),
        class: ConnectorOperationClass::Inspect,
        inventory_revision: context_content_digest(b"actual-fixture-inventory"),
        arguments: [(
            text("selector"),
            TypedValue::string("CANARY-PRIVATE-ARGUMENT").unwrap(),
        )]
        .into(),
        task: TaskId::new("actual-audit-task").unwrap(),
        step: a.binding().step.clone(),
        execution: text("actual-audit-execution"),
        correlation: text("actual-audit-call"),
    }
}
fn with_claim(
    id: &str,
    probe: impl Fn(
        &PostgresSessionStore,
        &SessionCheckpoint,
        &mut SessionPhaseResources,
        &NativePolicy,
    ) -> Result<(), SessionError>
    + 'static,
) -> Option<SessionCoordinator<PostgresSessionStore, GoalHost>> {
    let repo = repository()?;
    let policy = Arc::new(native(repo.store.scope().clone()));
    let callback_repo = reopen(repo.store.scope());
    let mut host = GoalHost::new(repo.store.scope().clone());
    host.goal = policy.goal.clone();
    host.now.set(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64,
    );
    host.probe = Some(Box::new(move |cp, resources| {
        probe(&callback_repo, cp, resources, &policy)
    }));
    let coordinator = SessionCoordinator::new(repo, host);
    let first = coordinator
        .execute(
            &owner(),
            SessionCommand::Start {
                command: CommandId::new(id).unwrap(),
                intent: coordinator.host.goal.intent().clone(),
                execution: RequestedExecution {
                    mode: OperatingMode::Hardening,
                    profile: ExecutionProfile::FullPath,
                },
            },
        )
        .unwrap();
    assert!(
        coordinator
            .execute(
                &owner(),
                SessionCommand::Continue {
                    at: Mutation {
                        session: first.session,
                        command: CommandId::new(format!("continue-{id}")).unwrap(),
                        expected_revision: first.revision
                    }
                }
            )
            .is_err()
    );
    Some(coordinator)
}
#[test]
fn postgres_real_cg09_audits_are_immutable_redacted_and_do_not_invalidate_active_claim() {
    let Some(coordinator) = with_claim("audit-native", |repo, cp, resources, native| {
        let allocation = resources.allocate(amounts())?;
        let before = repo.load(&cp.owner, &cp.snapshot.session)?;
        let r = request(&allocation, native);
        let source = native.goal.observations()[0].source();
        let adapter = Cg09ConnectorAdmission::new(Source {
            repo,
            allocation: &allocation,
            cp,
            native,
            fault: "none",
        });
        let dispatch = adapter.authorize(&r, Some(source)).unwrap();
        adapter.revalidate(&dispatch, Some(source)).unwrap();
        let mut stale = dispatch.clone();
        stale.policy_revision = context_content_digest(b"stale-policy");
        assert!(adapter.revalidate(&stale, Some(source)).is_err());
        assert_eq!(repo.load(&cp.owner, &cp.snapshot.session)?, before);
        let denied = Cg09ConnectorAdmission::new(Source {
            repo,
            allocation: &allocation,
            cp,
            native,
            fault: "finding",
        });
        assert!(denied.authorize(&r, Some(source)).is_err());
        assert_eq!(repo.load(&cp.owner, &cp.snapshot.session)?, before);
        repo.run(|j| {
            let audits = serde_json::to_value(&j.observation_policy_audits).unwrap();
            assert_eq!(audits.as_array().unwrap().len(), 4);
            let bytes = audits
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|a| {
                    a["bytes"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|n| n.as_u64().unwrap() as u8)
                })
                .collect::<Vec<_>>();
            let text = String::from_utf8(bytes).unwrap();
            assert!(!text.contains("CANARY-PRIVATE-ARGUMENT"));
            assert!(!text.contains("CANARY-PRIVATE-FINDING"));
            assert!(!text.contains("CANARY"));
            assert!(text.contains("PRE_EFFECT"));
            assert!(!text.contains("EvidenceMissing"));
            assert!(text.contains("EVIDENCE_MISSING"));
            Ok(())
        })?;
        Ok(())
    }) else {
        return;
    };
    let reader = reopen(coordinator.repository.store.scope());
    reader
        .run(|j| {
            assert_eq!(j.observation_policy_audits.len(), 4);
            Ok(())
        })
        .unwrap();
}
#[test]
fn postgres_audit_rejects_foreign_owner_stale_claim_expiry_budget_and_oversized_report() {
    for fault in [
        "owner",
        "stale",
        "expired",
        "reservation",
        "step",
        "oversized",
        "arguments",
    ] {
        let fault = fault.to_owned();
        let id = format!("audit-{}", fault);
        let Some(coordinator) = with_claim(&id, move |repo, cp, resources, native| {
            let allocation = resources.allocate(amounts())?;
            let mut r = request(&allocation, native);
            if fault == "arguments" {
                r.arguments = (0..257)
                    .map(|i| (text(&format!("field{i}")), TypedValue::Boolean(true)))
                    .collect();
            }
            if fault == "step" {
                r.step = PlanStepId::new("foreign-step").unwrap()
            }
            let adapter = Cg09ConnectorAdmission::new(Source {
                repo,
                allocation: &allocation,
                cp,
                native,
                fault: &fault,
            });
            assert!(
                adapter
                    .authorize(&r, Some(native.goal.observations()[0].source()))
                    .is_err(),
                "{fault}"
            );
            repo.run(|j| {
                assert!(j.observation_policy_audits.is_empty(), "{fault}");
                Ok(())
            })?;
            Ok(())
        }) else {
            return;
        };
        assert!(
            coordinator
                .repository
                .run(|j| Ok(j.observation_policy_audits.is_empty()))
                .unwrap()
        );
    }
}
#[test]
fn empty_audit_collection_preserves_legacy_journal_wire() {
    let wire = serde_json::to_value(Journal::initial()).unwrap();
    assert!(wire.get("observation_policy_audits").is_none());
    let decoded: Journal = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), wire);
}
