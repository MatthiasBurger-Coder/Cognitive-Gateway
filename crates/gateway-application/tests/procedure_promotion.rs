#[path = "../../../tests/support/procedure_promotion.rs"]
mod support;
use gateway_application::procedure_promotion::*;
use gateway_domain::{ProvenanceId, procedure_promotion::*};
use std::{cell::Cell, rc::Rc};
use support::*;
struct Authority {
    role: Rc<Cell<PromotionRole>>,
    allowed: Rc<Cell<bool>>,
}
impl PromotionAuthority for Authority {
    fn authorize(&self, _: &PromotionCommand) -> Option<AuthorizedPromotion> {
        self.allowed.get().then(|| AuthorizedPromotion {
            actor: ProvenanceId::new("authenticated-operator").unwrap(),
            policy_decision: id("trusted-policy-decision"),
            role: self.role.get(),
        })
    }
}
fn authority(role: Rc<Cell<PromotionRole>>, allowed: Rc<Cell<bool>>) -> Authority {
    Authority { role, allowed }
}
#[test]
fn authority_cannot_be_claimed_by_models_workers_or_runtime() {
    let role = Rc::new(Cell::new(PromotionRole::Model));
    let allowed = Rc::new(Cell::new(true));
    let mut app = PromotionApplication::new(
        authority(role.clone(), allowed.clone()),
        InMemoryPromotionStore::default(),
    );
    let command = PromotionCommand::Discover {
        procedure: Box::new(procedure(1)),
        discovery_evidence: id("source"),
    };
    for denied in [
        PromotionRole::Model,
        PromotionRole::Worker,
        PromotionRole::Runtime,
    ] {
        role.set(denied);
        assert_eq!(
            app.execute(id("try"), 10, command.clone()),
            Err(PromotionError::Unauthorized)
        );
        assert!(app.inspect().unwrap().0.events.is_empty());
    }
    role.set(PromotionRole::Governor);
    allowed.set(false);
    assert_eq!(
        app.execute(id("try"), 10, command.clone()),
        Err(PromotionError::Unauthorized)
    );
    allowed.set(true);
    app.execute(id("discover"), 10, command).unwrap();
    let journal = app.inspect().unwrap().0;
    assert_eq!(
        journal.events[0].metadata.actor.as_str(),
        "authenticated-operator"
    );
    assert_eq!(
        journal.events[0].metadata.policy_decision,
        id("trusted-policy-decision")
    );
    let before = journal;
    let v = ProcedureVersion::of(&procedure(1));
    assert!(
        app.execute(id("skip"), 10, PromotionCommand::Activate { procedure: v })
            .is_err()
    );
    assert_eq!(before, app.inspect().unwrap().0);
}
#[test]
fn governed_lifecycle_runtime_outcomes_and_transactional_conflicts() {
    let role = Rc::new(Cell::new(PromotionRole::Governor));
    let allowed = Rc::new(Cell::new(true));
    let mut app = PromotionApplication::new(
        authority(role.clone(), allowed),
        InMemoryPromotionStore::default(),
    );
    let mut h = History::default();
    let v = h.discover(1);
    h.canary(&v, 20);
    for event in &h.journal.events {
        app.execute(
            event.metadata.id.clone(),
            event.metadata.at,
            event.command.clone(),
        )
        .unwrap();
    }
    role.set(PromotionRole::Runtime);
    app.execute(
        id("reserve"),
        20,
        PromotionCommand::ReserveExecution {
            procedure: v.clone(),
            execution: execution("trial", ExecutionMode::Canary),
        },
    )
    .unwrap();
    app.execute(
        id("outcome"),
        20,
        PromotionCommand::RecordOutcome {
            procedure: v.clone(),
            execution_id: id("trial"),
            outcome: RuntimeOutcome::Success,
            evidence: id("verification"),
        },
    )
    .unwrap();
    assert_eq!(
        app.execute(
            id("self-activate"),
            20,
            PromotionCommand::Activate {
                procedure: v.clone()
            }
        ),
        Err(PromotionError::Unauthorized)
    );
    role.set(PromotionRole::Governor);
    let registry = app
        .execute(
            id("activate"),
            20,
            PromotionCommand::Activate {
                procedure: v.clone(),
            },
        )
        .unwrap();
    assert_eq!(registry.active(&v.id), Some(&v));
    let mut store = InMemoryPromotionStore::default();
    store.append(0, h.journal.events[0].clone()).unwrap();
    assert_eq!(
        store.append(0, h.journal.events[0].clone()),
        Err(PromotionError::Conflict)
    );
    assert!(store.append(1, h.journal.events[0].clone()).is_err());
    assert_eq!(store.load().unwrap().events.len(), 1);
}
struct BrokenStore {
    load_fails: bool,
}
impl PromotionStore for BrokenStore {
    fn load(&self) -> Result<PromotionJournal, PromotionError> {
        if self.load_fails {
            Err(PromotionError::Store("offline".into()))
        } else {
            Ok(PromotionJournal::default())
        }
    }
    fn append(&mut self, _: usize, _: PromotionEvent) -> Result<(), PromotionError> {
        Err(PromotionError::Conflict)
    }
}
#[test]
fn store_failure_and_compare_and_append_conflict_are_not_success() {
    for load_fails in [false, true] {
        let mut app = PromotionApplication::new(
            authority(
                Rc::new(Cell::new(PromotionRole::Governor)),
                Rc::new(Cell::new(true)),
            ),
            BrokenStore { load_fails },
        );
        let error = app
            .execute(
                id("write"),
                10,
                PromotionCommand::Discover {
                    procedure: Box::new(procedure(1)),
                    discovery_evidence: id("source"),
                },
            )
            .unwrap_err();
        assert_eq!(
            error,
            if load_fails {
                PromotionError::Store("offline".into())
            } else {
                PromotionError::Conflict
            }
        );
    }
}
