//! CG-29 durable replay scheduler. Dedicated scoped coordinator owns the database.
use crate::cognitive_store::{CognitiveStore, StoreError};
use gateway_application::{
    ports::outbound::CognitiveSchedulerPort,
    worker_fabric::{FabricLimits, ReferenceScheduler},
};
use gateway_domain::{ContextScopeId, worker_fabric::*};
use serde::{Deserialize, Serialize};

impl From<StoreError> for FabricError {
    fn from(_: StoreError) -> Self {
        Self::Storage
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    limits: FabricLimits,
    commands: Vec<Command>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
enum Command {
    Submit(WorkItem, u64),
    Claim(WorkerAdvertisement, u64),
    Complete(ContextScopeId, WorkResult, u64),
    Fail(ContextScopeId, String, u64, FailureReason, u64),
    Recover(u64),
}
impl Command {
    fn replay(&self, scheduler: &mut ReferenceScheduler) -> Result<(), FabricError> {
        match self {
            Self::Submit(item, now) => {
                scheduler.submit(item.clone(), *now)?;
            }
            Self::Claim(worker, now) => {
                scheduler.claim(worker, *now)?;
            }
            Self::Complete(scope, result, now) => {
                scheduler.complete(scope, result.clone(), *now)?;
            }
            Self::Fail(scope, id, token, reason, now) => {
                scheduler.fail(scope, id, *token, *reason, *now)?
            }
            Self::Recover(now) => {
                scheduler.recover(*now)?;
            }
        }
        Ok(())
    }
}
pub struct DurableScheduler {
    store: CognitiveStore,
    limits: FabricLimits,
}
impl DurableScheduler {
    pub fn new(store: CognitiveStore, limits: FabricLimits) -> Result<Self, FabricError> {
        ReferenceScheduler::new(limits.clone())?;
        Ok(Self { store, limits })
    }
    fn run<R>(
        &self,
        command: Option<Command>,
        operation: impl FnOnce(&mut ReferenceScheduler) -> Result<R, FabricError>,
    ) -> Result<R, FabricError> {
        let initial = Journal {
            limits: self.limits.clone(),
            commands: Vec::new(),
        };
        self.store
            .transact("worker-fabric-v1", &initial, |journal| {
                if journal.limits != self.limits || journal.commands.len() > 4096 {
                    return Err(FabricError::Storage);
                }
                let mut scheduler = ReferenceScheduler::new(journal.limits.clone())?;
                for previous in &journal.commands {
                    previous.replay(&mut scheduler)?;
                }
                let result = operation(&mut scheduler)?;
                if let Some(command) = command {
                    if journal.commands.len() == 4096 {
                        return Err(FabricError::Backpressure);
                    }
                    journal.commands.push(command);
                }
                Ok(result)
            })
    }
    fn scope(&self, scope: &ContextScopeId) -> Result<(), FabricError> {
        if scope != self.store.scope() {
            return Err(FabricError::ScopeMismatch);
        }
        Ok(())
    }
}
impl CognitiveSchedulerPort for DurableScheduler {
    fn submit(&mut self, item: WorkItem, now: u64) -> Result<bool, FabricError> {
        self.scope(&item.spec().scope)?;
        self.run(Some(Command::Submit(item.clone(), now)), |s| {
            s.submit(item, now)
        })
    }
    fn claim(
        &mut self,
        worker: &WorkerAdvertisement,
        now: u64,
    ) -> Result<Option<WorkLease>, FabricError> {
        self.scope(&worker.scope)?;
        self.run(Some(Command::Claim(worker.clone(), now)), |s| {
            s.claim(worker, now)
        })
    }
    fn complete(
        &mut self,
        scope: &ContextScopeId,
        result: WorkResult,
        now: u64,
    ) -> Result<bool, FabricError> {
        self.scope(scope)?;
        self.run(
            Some(Command::Complete(scope.clone(), result.clone(), now)),
            |s| s.complete(scope, result, now),
        )
    }
    fn fail(
        &mut self,
        scope: &ContextScopeId,
        id: &str,
        token: u64,
        reason: FailureReason,
        now: u64,
    ) -> Result<(), FabricError> {
        self.scope(scope)?;
        self.run(
            Some(Command::Fail(scope.clone(), id.into(), token, reason, now)),
            |s| s.fail(scope, id, token, reason, now),
        )
    }
    fn inspect(&self, scope: &ContextScopeId, id: &str) -> Result<WorkStatus, FabricError> {
        self.scope(scope)?;
        self.run(None, |s| s.inspect(scope, id))
    }
    fn recover(&mut self, now: u64) -> Result<usize, FabricError> {
        self.run(Some(Command::Recover(now)), |s| s.recover(now))
    }
}
