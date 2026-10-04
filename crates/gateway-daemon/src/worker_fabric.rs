//! CG-29 local reference adapter. Dedicated scoped handlers have no process ports.
use gateway_application::ports::outbound::CognitiveWorkerPort;
use gateway_domain::worker_fabric::*;

pub struct WorkerProposal {
    pub bytes: Vec<u8>,
    pub compute_units: u64,
}

/// Only a frozen work item reaches the handler. Host composition must supply
/// read-only scoped dependencies; lifecycle credentials never enter the worker.
pub struct LocalCognitiveWorker<H> {
    advertisement: WorkerAdvertisement,
    handler: H,
}
impl<H> LocalCognitiveWorker<H> {
    pub fn new(advertisement: WorkerAdvertisement, handler: H) -> Self {
        Self {
            advertisement,
            handler,
        }
    }
}
impl<H> CognitiveWorkerPort for LocalCognitiveWorker<H>
where
    H: FnMut(&WorkItem) -> Result<WorkerProposal, FailureReason>,
{
    fn advertisement(&self) -> WorkerAdvertisement {
        self.advertisement.clone()
    }
    fn execute(&mut self, lease: &WorkLease) -> Result<WorkResult, FailureReason> {
        lease
            .item
            .validate()
            .map_err(|_| FailureReason::InvalidResult)?;
        let spec = lease.item.spec();
        let ad = &self.advertisement;
        if lease.worker != ad.worker
            || lease.node != ad.node
            || spec.scope != ad.scope
            || !ad.kinds.contains(&spec.kind)
            || !ad.runtimes.contains(&spec.runtime)
            || spec.model.as_ref().is_some_and(|m| !ad.models.contains(m))
            || lease.token == 0
            || lease.attempt == 0
            || lease.attempt > spec.budget.max_attempts
            || ad.slots == 0
            || spec.budget.memory_bytes > ad.memory_bytes
            || spec.budget.compute_units > ad.compute_units
        {
            return Err(FailureReason::InvalidResult);
        }
        let proposal = (self.handler)(&lease.item)?;
        if proposal.bytes.is_empty()
            || proposal.bytes.len() > spec.budget.max_result_bytes
            || proposal.compute_units > spec.budget.compute_units
        {
            return Err(FailureReason::InvalidResult);
        }
        Ok(WorkResult {
            work_id: lease.item.id().into(),
            scope: spec.scope.clone(),
            trace: spec.trace.clone(),
            token: lease.token,
            attempt: lease.attempt,
            proposal: proposal.bytes,
            compute_units: proposal.compute_units,
            provenance: WorkProvenance {
                worker: ad.worker.clone(),
                node: ad.node.clone(),
                runtime: spec.runtime.clone(),
                model: spec.model.clone(),
                sources: spec.sources.clone(),
                snapshot_digest: lease.item.snapshot_digest().into(),
            },
        })
    }
}

/// Bounded deterministic diagnostic handler: proves round-trip transport without
/// performing retrieval, training, model inference or authoritative mutations.
pub fn snapshot_probe(item: &WorkItem) -> Result<WorkerProposal, FailureReason> {
    Ok(WorkerProposal {
        bytes: item.snapshot_digest().as_bytes().to_vec(),
        compute_units: 1,
    })
}
