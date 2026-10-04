//! Deterministic bounded reference scheduler. Only advisory state lives here.
use crate::ports::outbound::{CognitiveSchedulerPort, CognitiveWorkerPort};
use gateway_domain::{ContextScopeId, worker_fabric::*};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FabricLimits {
    /// Includes terminal records: deduplication history cannot grow unbounded.
    pub retained_items: usize,
    pub per_scope_items: usize,
    pub concurrent_leases: usize,
    pub max_attempts: u16,
    pub max_lease_ms: u64,
    pub max_lifetime_ms: u64,
    pub max_memory_bytes: u64,
    pub max_compute_units: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FabricMetrics {
    pub submissions: u64,
    pub duplicates: u64,
    pub backpressure: u64,
    pub dispatches: u64,
    pub recoveries: u64,
    pub completions: u64,
    pub failures: u64,
    pub invalid_results: u64,
}
struct Entry {
    item: WorkItem,
    status: WorkStatus,
    lease: Option<WorkLease>,
}

/// Single coordinator reference, serialized by &mut self. Retains tombstones
/// until shutdown. Durable adapters must transact claims, fencing and completion.
pub struct ReferenceScheduler {
    limits: FabricLimits,
    entries: BTreeMap<String, Entry>,
    next_token: u64,
    now_ms: u64,
    metrics: FabricMetrics,
}
impl ReferenceScheduler {
    pub fn new(limits: FabricLimits) -> Result<Self, FabricError> {
        if limits.retained_items == 0
            || limits.per_scope_items == 0
            || limits.per_scope_items > limits.retained_items
            || limits.concurrent_leases == 0
            || limits.max_attempts == 0
            || limits.max_attempts > 64
            || limits.max_lease_ms == 0
            || limits.max_lifetime_ms == 0
            || limits.max_memory_bytes == 0
            || limits.max_compute_units == 0
        {
            return Err(FabricError::InvalidContract);
        }
        Ok(Self {
            limits,
            entries: BTreeMap::new(),
            next_token: 0,
            now_ms: 0,
            metrics: FabricMetrics::default(),
        })
    }
    pub fn metrics(&self) -> &FabricMetrics {
        &self.metrics
    }
    fn clock(&mut self, now: u64) -> Result<(), FabricError> {
        if now < self.now_ms {
            return Err(FabricError::ClockRegression);
        }
        self.now_ms = now;
        Ok(())
    }
    fn retry(entry: &mut Entry, reason: FailureReason, now: u64) {
        entry.lease = None;
        entry.status.last_failure = Some(reason);
        let budget = &entry.item.spec().budget;
        let ready = now.saturating_add(budget.retry_delay_ms);
        entry.status.state = if now >= budget.deadline_ms || ready >= budget.deadline_ms {
            WorkState::Failed {
                reason: FailureReason::Deadline,
            }
        } else if entry.status.attempts >= budget.max_attempts {
            WorkState::Failed {
                reason: FailureReason::AttemptsExhausted,
            }
        } else {
            WorkState::Queued { ready_ms: ready }
        };
    }
    fn scoped(&self, scope: &ContextScopeId, id: &str) -> Result<&Entry, FabricError> {
        let entry = self.entries.get(id).ok_or(FabricError::UnknownWork)?;
        if entry.item.spec().scope != *scope {
            return Err(FabricError::ScopeMismatch);
        }
        Ok(entry)
    }
}
impl CognitiveSchedulerPort for ReferenceScheduler {
    fn submit(&mut self, item: WorkItem, now: u64) -> Result<bool, FabricError> {
        item.validate()?;
        self.clock(now)?;
        if self.entries.contains_key(item.id()) {
            self.metrics.duplicates += 1;
            return Ok(false);
        }
        let b = &item.spec().budget;
        if b.deadline_ms <= now
            || b.deadline_ms - now > self.limits.max_lifetime_ms
            || b.max_attempts > self.limits.max_attempts
            || b.lease_ms > self.limits.max_lease_ms
            || b.memory_bytes > self.limits.max_memory_bytes
            || b.compute_units * u64::from(b.max_attempts) > self.limits.max_compute_units
        {
            return Err(FabricError::InvalidContract);
        }
        if self.entries.len() >= self.limits.retained_items
            || self
                .entries
                .values()
                .filter(|e| e.item.spec().scope == item.spec().scope)
                .count()
                >= self.limits.per_scope_items
        {
            self.metrics.backpressure += 1;
            return Err(FabricError::Backpressure);
        }
        let status = WorkStatus {
            work_id: item.id().into(),
            scope: item.spec().scope.clone(),
            trace: item.spec().trace.clone(),
            snapshot_digest: item.snapshot_digest().into(),
            state: WorkState::Queued { ready_ms: now },
            attempts: 0,
            reserved_compute_units: 0,
            last_failure: None,
            result: None,
        };
        self.entries.insert(
            item.id().into(),
            Entry {
                item,
                status,
                lease: None,
            },
        );
        self.metrics.submissions += 1;
        Ok(true)
    }
    fn claim(
        &mut self,
        worker: &WorkerAdvertisement,
        now: u64,
    ) -> Result<Option<WorkLease>, FabricError> {
        self.recover(now)?;
        if worker.slots == 0
            || worker.memory_bytes == 0
            || worker.compute_units == 0
            || worker.kinds.is_empty()
            || worker.runtimes.is_empty()
            || worker.kinds.len() > 4
            || worker.runtimes.len() > 64
            || worker.models.len() > 64
        {
            return Err(FabricError::InvalidWorker);
        }
        let active: Vec<_> = self
            .entries
            .values()
            .filter(|e| e.lease.is_some())
            .collect();
        let assigned: Vec<_> = active
            .iter()
            .filter(|e| e.lease.as_ref().is_some_and(|l| l.worker == worker.worker))
            .collect();
        if assigned.iter().any(|e| {
            e.item.spec().scope != worker.scope
                || e.lease.as_ref().is_some_and(|l| l.node != worker.node)
        }) {
            return Err(FabricError::InvalidWorker);
        }
        let used_memory = assigned.iter().fold(0u64, |a, e| {
            a.saturating_add(e.item.spec().budget.memory_bytes)
        });
        let used_compute = assigned.iter().fold(0u64, |a, e| {
            a.saturating_add(e.item.spec().budget.compute_units)
        });
        if active.len() >= self.limits.concurrent_leases || assigned.len() >= worker.slots {
            return Ok(None);
        }
        let candidate = self
            .entries
            .iter()
            .find(|(_, e)| {
                let s = e.item.spec();
                matches!(e.status.state, WorkState::Queued { ready_ms } if ready_ms <= now)
                    && s.scope == worker.scope
                    && worker.kinds.contains(&s.kind)
                    && worker.runtimes.contains(&s.runtime)
                    && s.model.as_ref().is_none_or(|m| worker.models.contains(m))
                    && s.budget.memory_bytes <= worker.memory_bytes.saturating_sub(used_memory)
                    && s.budget.compute_units <= worker.compute_units.saturating_sub(used_compute)
            })
            .map(|(id, _)| id.clone());
        let Some(id) = candidate else {
            return Ok(None);
        };
        self.next_token = self
            .next_token
            .checked_add(1)
            .ok_or(FabricError::CounterExhausted)?;
        let entry = self.entries.get_mut(&id).ok_or(FabricError::UnknownWork)?;
        entry.status.attempts += 1;
        entry.status.reserved_compute_units += entry.item.spec().budget.compute_units;
        let lease = WorkLease {
            item: entry.item.clone(),
            worker: worker.worker.clone(),
            node: worker.node.clone(),
            token: self.next_token,
            attempt: entry.status.attempts,
            expires_ms: now
                .saturating_add(entry.item.spec().budget.lease_ms)
                .min(entry.item.spec().budget.deadline_ms),
        };
        entry.status.state = WorkState::Leased {
            worker: worker.worker.clone(),
            token: lease.token,
            expires_ms: lease.expires_ms,
        };
        entry.lease = Some(lease.clone());
        self.metrics.dispatches += 1;
        Ok(Some(lease))
    }
    fn complete(
        &mut self,
        scope: &ContextScopeId,
        result: WorkResult,
        now: u64,
    ) -> Result<bool, FabricError> {
        self.scoped(scope, &result.work_id)?;
        self.recover(now)?;
        let entry = self
            .entries
            .get_mut(&result.work_id)
            .ok_or(FabricError::UnknownWork)?;
        if entry.status.result.as_ref() == Some(&result) {
            self.metrics.duplicates += 1;
            return Ok(false);
        }
        let lease = entry.lease.as_ref().ok_or(FabricError::StaleLease)?;
        if result.token != lease.token || result.attempt != lease.attempt {
            return Err(FabricError::StaleLease);
        }
        let spec = entry.item.spec();
        if result.scope != *scope
            || result.trace != spec.trace
            || result.provenance.worker != lease.worker
            || result.provenance.node != lease.node
            || result.provenance.runtime != spec.runtime
            || result.provenance.model != spec.model
            || result.provenance.sources != spec.sources
            || result.provenance.snapshot_digest != entry.item.snapshot_digest()
            || result.compute_units > spec.budget.compute_units
            || result.proposal.len() > spec.budget.max_result_bytes
            || result.proposal.is_empty()
        {
            self.metrics.invalid_results += 1;
            return Err(FabricError::InvalidResult);
        }
        entry.status.result = Some(result);
        entry.status.state = WorkState::Completed;
        entry.lease = None;
        self.metrics.completions += 1;
        Ok(true)
    }
    fn fail(
        &mut self,
        scope: &ContextScopeId,
        id: &str,
        token: u64,
        reason: FailureReason,
        now: u64,
    ) -> Result<(), FabricError> {
        self.scoped(scope, id)?;
        self.recover(now)?;
        let entry = self.entries.get_mut(id).ok_or(FabricError::UnknownWork)?;
        if !entry.lease.as_ref().is_some_and(|l| l.token == token) {
            return Err(FabricError::StaleLease);
        }
        Self::retry(entry, reason, now);
        self.metrics.failures += 1;
        Ok(())
    }
    fn inspect(&self, scope: &ContextScopeId, id: &str) -> Result<WorkStatus, FabricError> {
        Ok(self.scoped(scope, id)?.status.clone())
    }
    fn recover(&mut self, now: u64) -> Result<usize, FabricError> {
        self.clock(now)?;
        let mut count = 0;
        for entry in self.entries.values_mut() {
            if entry.lease.as_ref().is_some_and(|l| now >= l.expires_ms) {
                Self::retry(entry, FailureReason::Timeout, now);
                count += 1;
            } else if matches!(entry.status.state, WorkState::Queued { .. })
                && now >= entry.item.spec().budget.deadline_ms
            {
                Self::retry(entry, FailureReason::Deadline, now);
                count += 1;
            }
        }
        self.metrics.recoveries += count as u64;
        Ok(count)
    }
}

/// Transport-neutral dispatch. Completion time must be measured by the host,
/// not worker telemetry, so slow calls cannot extend their own leases.
pub fn dispatch_one<S: CognitiveSchedulerPort, W: CognitiveWorkerPort>(
    scheduler: &mut S,
    worker: &mut W,
    started_ms: u64,
    mut clock: impl FnMut() -> u64,
) -> Result<bool, FabricError> {
    let Some(lease) = scheduler.claim(&worker.advertisement(), started_ms)? else {
        return Ok(false);
    };
    match worker.execute(&lease) {
        Ok(result) => {
            // Bind the response to this authenticated transport invocation before
            // looking up any scheduler record supplied by a worker.
            if result.work_id != lease.item.id()
                || result.token != lease.token
                || result.attempt != lease.attempt
            {
                scheduler.fail(
                    &lease.item.spec().scope,
                    lease.item.id(),
                    lease.token,
                    FailureReason::InvalidResult,
                    clock(),
                )?;
                return Err(FabricError::InvalidResult);
            }
            scheduler.complete(&lease.item.spec().scope, result, clock())?;
        }
        Err(reason) => {
            scheduler.fail(
                &lease.item.spec().scope,
                lease.item.id(),
                lease.token,
                reason,
                clock(),
            )?;
        }
    }
    Ok(true)
}
