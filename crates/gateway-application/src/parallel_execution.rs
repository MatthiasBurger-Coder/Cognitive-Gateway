//! CG-28A: scheduling of independently authorized, single-task contexts.
//! The scheduler owns ordering and resource admission, never process or policy authority.
use crate::context_application::CompiledStep;
use gateway_context::CompiledContext;
use gateway_domain::{CapabilityId, TaskId};
use gateway_policy::PolicyDecision;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleError {
    InvalidCapsule,
    Unauthorized,
    DuplicateTask,
    UnknownDependency,
    Cycle,
    InvalidGroup,
    InvalidGraph,
    InvalidClaim,
    InvalidJoin,
    InvalidResult,
    StaleResult,
    DuplicateResult,
    NotRunning,
    InvalidConcurrency,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Resource {
    File(String),
    Module(String),
    Contract(String),
}
impl Resource {
    fn valid(&self) -> bool {
        let value = match self {
            Self::File(v) | Self::Module(v) | Self::Contract(v) => v,
        };
        !value.trim().is_empty()
            && !value.starts_with('/')
            && !value.contains('\\')
            && !value.chars().any(char::is_control)
            && !value
                .split('/')
                .any(|part| part == ".." || part == "." || part.is_empty())
    }
    fn contains(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::File(a), Self::File(b)) => a == b || b.starts_with(&format!("{a}/")),
            (Self::Module(a), Self::Module(b)) | (Self::Contract(a), Self::Contract(b)) => a == b,
            _ => false,
        }
    }
    fn overlaps(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::File(a), Self::File(b)) => {
                a == b || a.starts_with(&format!("{b}/")) || b.starts_with(&format!("{a}/"))
            }
            (Self::Module(a), Self::Module(b)) | (Self::Contract(a), Self::Contract(b)) => a == b,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Access {
    Read,
    Write,
    Lock,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResourceClaim {
    pub resource: Resource,
    pub access: Access,
}
impl ResourceClaim {
    fn conflicts(&self, other: &Self) -> bool {
        self.resource.overlaps(&other.resource)
            && (self.access != Access::Read || other.access != Access::Read)
    }
    fn permits(&self, requested: &Self) -> bool {
        self.resource.contains(&requested.resource)
            && matches!(
                (self.access, requested.access),
                (Access::Lock, Access::Lock)
                    | (Access::Write, Access::Read | Access::Write)
                    | (Access::Read, Access::Read)
            )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum GroupMode {
    Sequential,
    Parallel,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutionGroup {
    pub id: String,
    pub mode: GroupMode,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum JoinPolicy {
    AllRequired,
    AnySuccess,
    Quorum(usize),
    FailFast,
    CollectAll,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JoinBarrier {
    pub id: String,
    pub upstream: BTreeSet<TaskId>,
    pub policy: JoinPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskSpec {
    pub id: TaskId,
    pub parent_plan: String,
    pub action: String,
    pub target: String,
    pub completion_condition: String,
    pub group: String,
    pub dependencies: BTreeSet<TaskId>,
    pub barrier_dependencies: BTreeSet<String>,
    pub claims: Vec<ResourceClaim>,
    pub capabilities: BTreeSet<String>,
    pub forbidden_capabilities: BTreeSet<String>,
    pub forbidden_resources: BTreeSet<Resource>,
    pub stop_conditions: BTreeSet<String>,
    pub knowledge_requirements: BTreeSet<String>,
    pub evidence_requirements: BTreeSet<String>,
    pub retry_budget: u32,
    pub timeout_ms: u64,
    pub context_byte_budget: usize,
    pub mutation_allowed: bool,
    pub delegation_allowed: bool,
    pub scope_expansion_allowed: bool,
}

/// Immutable task plus the already compiled, policy-checked context.
#[derive(Debug, Clone)]
pub struct TaskCapsule {
    spec: TaskSpec,
    context: CompiledContext,
    output_contract: serde_json::Value,
    digest: String,
    input_snapshot: String,
}
impl TaskCapsule {
    pub fn new(mut spec: TaskSpec, context: CompiledStep) -> Result<Self, ScheduleError> {
        let ir = context.context().execution_context();
        if context.policy().decision != PolicyDecision::Allow {
            return Err(ScheduleError::Unauthorized);
        }
        if spec.id != *ir.task().id()
            || spec.parent_plan != context.basis().plan.as_str()
            || spec.action.trim().is_empty()
            || spec.target.trim().is_empty()
            || spec.completion_condition.trim().is_empty()
            || spec.group.trim().is_empty()
            || spec.stop_conditions.is_empty()
            || spec.timeout_ms == 0
            || spec.retry_budget == u32::MAX
            || spec.context_byte_budget == 0
            || spec.delegation_allowed
            || spec.scope_expansion_allowed
            || spec.dependencies.contains(&spec.id)
            || spec
                .capabilities
                .iter()
                .any(|c| CapabilityId::new(c).is_err())
            || spec.capabilities.is_empty()
            || spec
                .capabilities
                .iter()
                .any(|c| spec.forbidden_capabilities.contains(c))
            || spec.claims.iter().any(|c| !c.resource.valid())
            || spec.forbidden_resources.iter().any(|r| !r.valid())
            || spec.claims.iter().any(|c| {
                spec.forbidden_resources
                    .iter()
                    .any(|r| r.overlaps(&c.resource))
            })
            || spec
                .forbidden_capabilities
                .iter()
                .any(|c| CapabilityId::new(c).is_err())
            || (!spec.mutation_allowed && spec.claims.iter().any(|c| c.access == Access::Write))
        {
            return Err(ScheduleError::InvalidCapsule);
        }
        let queries = ir
            .knowledge_queries()
            .iter()
            .map(|q| q.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        if spec.knowledge_requirements != queries {
            return Err(ScheduleError::InvalidCapsule);
        }
        let approved = ir
            .approved_capability_ids()
            .iter()
            .map(|c| c.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        if !spec.capabilities.is_subset(&approved) {
            return Err(ScheduleError::Unauthorized);
        }
        spec.claims.sort_by(|a, b| {
            a.resource
                .cmp(&b.resource)
                .then_with(|| (a.access as u8).cmp(&(b.access as u8)))
        });
        spec.claims.dedup();
        let minimal_context = context.context().clone();
        let output_contract = context.output_contract().clone();
        let context_json = minimal_context
            .to_json()
            .map_err(|_| ScheduleError::InvalidCapsule)?;
        if context_json.len() > spec.context_byte_budget {
            return Err(ScheduleError::InvalidCapsule);
        }
        let input_snapshot = context.basis().situation_fingerprint.as_str().to_owned();
        let encoded = serde_json::to_vec(&(
            &spec,
            &input_snapshot,
            &context_json,
            &output_contract,
            context.policy(),
        ))
        .map_err(|_| ScheduleError::InvalidCapsule)?;
        let digest = format!("{:x}", Sha256::digest(encoded));
        Ok(Self {
            spec,
            context: minimal_context,
            output_contract,
            digest,
            input_snapshot,
        })
    }
    pub fn spec(&self) -> &TaskSpec {
        &self.spec
    }
    pub fn context(&self) -> &CompiledContext {
        &self.context
    }
    pub fn output_contract(&self) -> &serde_json::Value {
        &self.output_contract
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
    pub fn input_snapshot(&self) -> &str {
        &self.input_snapshot
    }
    /// Provider-independent runtime contract; retrieved text is carried only in `context`.
    pub fn dispatch_contract(&self) -> serde_json::Value {
        serde_json::json!({
            "schema_version": 1, "task_id": self.spec.id, "digest": self.digest,
            "input_snapshot": self.input_snapshot,
            "mission": {"action": self.spec.action, "target": self.spec.target},
            "scope": {"resources": self.spec.claims, "forbidden_resources": self.spec.forbidden_resources},
            "allowed_actions": self.spec.capabilities,
            "forbidden_actions": self.spec.forbidden_capabilities,
            "authorized_capabilities": self.spec.capabilities,
            "required_knowledge": self.spec.knowledge_requirements,
            "required_evidence": self.spec.evidence_requirements,
            "output_contract": self.output_contract,
            "done_condition": self.spec.completion_condition,
            "stop_conditions": self.spec.stop_conditions,
            "out_of_scope_rule": "report_observation_only",
            "delegation_allowed": false, "scope_expansion_allowed": false,
            "timeout_ms": self.spec.timeout_ms,
            "context_byte_budget": self.spec.context_byte_budget,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TaskStatus {
    Completed,
    BlockedMissingContext,
    BlockedPolicy,
    BlockedProcess,
    OutOfScope,
    Failed,
    Cancelled,
    StaleInput,
    RetryExhausted,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskResult {
    pub task_id: TaskId,
    pub task_digest: String,
    pub input_snapshot: String,
    pub attempt: u32,
    pub status: TaskStatus,
    pub structured_output: serde_json::Value,
    pub evidence: BTreeSet<String>,
    pub resource_changes: BTreeSet<Resource>,
    pub out_of_scope_observations: Vec<String>,
    pub execution_trace_ref: String,
    pub runtime_provenance: String,
    pub model_provenance: Option<String>,
    pub verified: bool,
}
impl TaskResult {
    fn success(&self, capsule: &TaskCapsule) -> bool {
        self.status == TaskStatus::Completed
            && self.verified
            && capsule.spec.evidence_requirements.is_subset(&self.evidence)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JoinedResult {
    pub barrier: String,
    pub state: JoinState,
    pub satisfied: bool,
    pub results: Vec<Option<TaskResult>>,
    pub missing: Vec<TaskId>,
    pub failed: Vec<TaskId>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum JoinState {
    Pending,
    Satisfied,
    Failed,
}

/// A tool call is checked against exactly one capsule before reaching the host.
pub trait ToolPort: Sync {
    fn invoke(
        &self,
        capability: &str,
        claim: &ResourceClaim,
        input: &serde_json::Value,
    ) -> Result<serde_json::Value, String>;
}
pub struct GuardedTools<'a> {
    capsule: &'a TaskCapsule,
    host: &'a dyn ToolPort,
    writes: Mutex<BTreeSet<Resource>>,
}
impl<'a> GuardedTools<'a> {
    fn new(capsule: &'a TaskCapsule, host: &'a dyn ToolPort) -> Self {
        Self {
            capsule,
            host,
            writes: Mutex::new(BTreeSet::new()),
        }
    }
    pub fn invoke(
        &self,
        capability: &str,
        claim: &ResourceClaim,
        input: &serde_json::Value,
    ) -> Result<serde_json::Value, ScheduleError> {
        let spec = &self.capsule.spec;
        if !spec.capabilities.contains(capability)
            || spec.forbidden_capabilities.contains(capability)
            || !claim.resource.valid()
            || spec
                .forbidden_resources
                .iter()
                .any(|r| r.overlaps(&claim.resource))
            || (claim.access == Access::Write && !spec.mutation_allowed)
            || !spec.claims.iter().any(|allowed| allowed.permits(claim))
        {
            return Err(ScheduleError::Unauthorized);
        }
        let output = self
            .host
            .invoke(capability, claim, input)
            .map_err(|_| ScheduleError::InvalidResult)?;
        if claim.access == Access::Write {
            self.writes
                .lock()
                .expect("audit lock")
                .insert(claim.resource.clone());
        }
        Ok(output)
    }
}
/// Replaceable in-process baseline. Worker receives no process mutation port.
pub trait ResultVerifier: Sync {
    fn verify(&self, capsule: &TaskCapsule, result: &TaskResult) -> bool;
}
/// Reads the current authenticated input revision immediately around dispatch.
pub trait SnapshotPort: Sync {
    fn current_snapshot(&self, capsule: &TaskCapsule) -> Result<String, String>;
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchReadiness {
    Ready,
    BlockedPolicy,
    BlockedProcess,
    BlockedMissingContext,
}
/// Trusted application adapter rechecks current CG-04 and CG-09 authority.
pub trait DispatchAuthority: Sync {
    fn readiness(&self, capsule: &TaskCapsule) -> DispatchReadiness;
}
pub trait SubagentRuntime: Sync {
    fn execute(&self, capsule: &TaskCapsule, tools: &GuardedTools<'_>, attempt: u32) -> TaskResult;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskState {
    Pending,
    Running(u32),
    Done,
}
/// Deterministic decisions; completion may arrive in any order.
pub struct Scheduler {
    capsules: BTreeMap<TaskId, TaskCapsule>,
    groups: BTreeMap<String, GroupMode>,
    barriers: BTreeMap<String, JoinBarrier>,
    states: BTreeMap<TaskId, TaskState>,
    results: BTreeMap<TaskId, TaskResult>,
    concurrency: usize,
    stopped: bool,
}
impl Scheduler {
    pub fn new(
        capsules: Vec<TaskCapsule>,
        groups: Vec<ExecutionGroup>,
        barriers: Vec<JoinBarrier>,
        concurrency: usize,
    ) -> Result<Self, ScheduleError> {
        if concurrency == 0 {
            return Err(ScheduleError::InvalidConcurrency);
        }
        let mut by_id = BTreeMap::new();
        for capsule in capsules {
            if by_id.insert(capsule.spec.id.clone(), capsule).is_some() {
                return Err(ScheduleError::DuplicateTask);
            }
        }
        let mut group_map = BTreeMap::new();
        for group in groups {
            if group.id.trim().is_empty() || group_map.insert(group.id, group.mode).is_some() {
                return Err(ScheduleError::InvalidGroup);
            }
        }
        for capsule in by_id.values() {
            if !group_map.contains_key(&capsule.spec.group) {
                return Err(ScheduleError::InvalidGroup);
            }
            if capsule
                .spec
                .dependencies
                .iter()
                .any(|id| !by_id.contains_key(id))
            {
                return Err(ScheduleError::UnknownDependency);
            }
        }
        if let Some(first) = by_id.values().next()
            && by_id.values().any(|capsule| {
                capsule.spec.parent_plan != first.spec.parent_plan
                    || capsule.input_snapshot != first.input_snapshot
            })
        {
            return Err(ScheduleError::InvalidGraph);
        }
        let mut barrier_map = BTreeMap::new();
        for barrier in barriers {
            let valid_policy = match barrier.policy {
                JoinPolicy::Quorum(n) => n > 0 && n <= barrier.upstream.len(),
                _ => true,
            };
            if barrier.id.trim().is_empty()
                || barrier.upstream.is_empty()
                || !valid_policy
                || barrier.upstream.iter().any(|id| !by_id.contains_key(id))
                || barrier_map.insert(barrier.id.clone(), barrier).is_some()
            {
                return Err(ScheduleError::InvalidJoin);
            }
        }
        let mut effective = BTreeMap::new();
        for (id, capsule) in &by_id {
            let mut dependencies = capsule.spec.dependencies.clone();
            for barrier_id in &capsule.spec.barrier_dependencies {
                let barrier = barrier_map
                    .get(barrier_id)
                    .ok_or(ScheduleError::InvalidJoin)?;
                dependencies.extend(barrier.upstream.iter().cloned());
            }
            effective.insert(id.clone(), dependencies);
        }
        let mut degrees = by_id
            .keys()
            .map(|id| (id.clone(), effective[id].len()))
            .collect::<BTreeMap<_, _>>();
        let mut ready = degrees
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(id, _)| id.clone())
            .collect::<BTreeSet<_>>();
        let mut visited = 0;
        while let Some(id) = ready.pop_first() {
            visited += 1;
            for (next, dependencies) in &effective {
                if dependencies.contains(&id) {
                    let degree = degrees.get_mut(next).expect("indexed");
                    *degree -= 1;
                    if *degree == 0 {
                        ready.insert(next.clone());
                    }
                }
            }
        }
        if visited != by_id.len() {
            return Err(ScheduleError::Cycle);
        }
        let states = by_id
            .keys()
            .map(|id| (id.clone(), TaskState::Pending))
            .collect();
        Ok(Self {
            capsules: by_id,
            groups: group_map,
            barriers: barrier_map,
            states,
            results: BTreeMap::new(),
            concurrency,
            stopped: false,
        })
    }
    pub fn next_batch(&mut self, authority: &dyn DispatchAuthority) -> Vec<TaskCapsule> {
        if self.stopped {
            return Vec::new();
        }
        let running = self
            .states
            .values()
            .filter(|state| matches!(state, TaskState::Running(_)))
            .count();
        let mut selected: Vec<TaskCapsule> = Vec::new();
        let mut blocked = Vec::new();
        for (id, capsule) in &self.capsules {
            if selected.len() + running >= self.concurrency {
                break;
            }
            if self.states[id] != TaskState::Pending
                || !capsule.spec.dependencies.iter().all(|dep| {
                    self.results
                        .get(dep)
                        .is_some_and(|r| r.success(&self.capsules[dep]))
                })
                || !capsule
                    .spec
                    .barrier_dependencies
                    .iter()
                    .all(|barrier| self.join(barrier).is_ok_and(|result| result.satisfied))
            {
                continue;
            }
            let group = &capsule.spec.group;
            if self.groups[group] == GroupMode::Sequential
                && (self.states.iter().any(|(other, state)| {
                    other != id
                        && self.capsules[other].spec.group == *group
                        && matches!(state, TaskState::Running(_))
                }) || selected.iter().any(|c| c.spec.group == *group))
            {
                continue;
            }
            let conflicts = self.states.iter().any(|(other, state)| {
                matches!(state, TaskState::Running(_))
                    && claims_conflict(&capsule.spec.claims, &self.capsules[other].spec.claims)
            }) || selected
                .iter()
                .any(|c| claims_conflict(&capsule.spec.claims, &c.spec.claims));
            if conflicts {
                continue;
            }
            let status = match authority.readiness(capsule) {
                DispatchReadiness::Ready => None,
                DispatchReadiness::BlockedPolicy => Some(TaskStatus::BlockedPolicy),
                DispatchReadiness::BlockedProcess => Some(TaskStatus::BlockedProcess),
                DispatchReadiness::BlockedMissingContext => Some(TaskStatus::BlockedMissingContext),
            };
            if let Some(status) = status {
                blocked.push((id.clone(), status));
                continue;
            }
            selected.push(capsule.clone());
        }
        for (id, status) in blocked {
            self.states.insert(id.clone(), TaskState::Done);
            self.results
                .insert(id.clone(), scheduler_result(&self.capsules[&id], 0, status));
        }
        for capsule in &selected {
            let attempt = self
                .results
                .get(&capsule.spec.id)
                .map_or(1, |r| r.attempt + 1);
            self.states
                .insert(capsule.spec.id.clone(), TaskState::Running(attempt));
        }
        selected
    }
    pub fn submit(
        &mut self,
        mut result: TaskResult,
        verifier: &dyn ResultVerifier,
    ) -> Result<(), ScheduleError> {
        let Some(capsule) = self.capsules.get(&result.task_id) else {
            return Err(ScheduleError::InvalidResult);
        };
        let Some(TaskState::Running(attempt)) = self.states.get(&result.task_id) else {
            return Err(if self.results.contains_key(&result.task_id) {
                ScheduleError::DuplicateResult
            } else {
                ScheduleError::NotRunning
            });
        };
        if result.task_digest != capsule.digest || result.input_snapshot != capsule.input_snapshot {
            return Err(ScheduleError::StaleResult);
        }
        if result.attempt != *attempt
            || result.execution_trace_ref.trim().is_empty()
            || result.runtime_provenance.trim().is_empty()
            || result.resource_changes.iter().any(|change| {
                !capsule
                    .spec
                    .claims
                    .iter()
                    .any(|claim| claim.access == Access::Write && claim.resource.overlaps(change))
            })
            || (result.status == TaskStatus::Completed
                && !capsule
                    .spec
                    .evidence_requirements
                    .is_subset(&result.evidence))
        {
            return Err(ScheduleError::InvalidResult);
        }
        if self.stopped {
            result.status = TaskStatus::Cancelled;
            result.verified = false;
        }
        result.verified = result.verified && verifier.verify(capsule, &result);
        if result.status == TaskStatus::Completed && !result.success(capsule) {
            return Err(ScheduleError::InvalidResult);
        }
        let retry =
            result.status == TaskStatus::Failed && result.attempt <= capsule.spec.retry_budget;
        if result.status == TaskStatus::Failed && !retry {
            result.status = TaskStatus::RetryExhausted;
        }
        self.states.insert(
            result.task_id.clone(),
            if retry {
                TaskState::Pending
            } else {
                TaskState::Done
            },
        );
        let task_id = result.task_id.clone();
        self.results.insert(task_id.clone(), result);
        if !retry
            && self.barriers.values().any(|barrier| {
                barrier.policy == JoinPolicy::FailFast
                    && barrier.upstream.contains(&task_id)
                    && self.results[&task_id].status != TaskStatus::Completed
            })
        {
            self.cancel();
        }
        Ok(())
    }
    pub fn cancel(&mut self) {
        self.stopped = true;
        for (id, state) in &mut self.states {
            if *state == TaskState::Pending {
                *state = TaskState::Done;
                let capsule = &self.capsules[id];
                self.results.insert(
                    id.clone(),
                    scheduler_result(capsule, 0, TaskStatus::Cancelled),
                );
            }
        }
    }
    fn abort_running(&mut self, ids: &[TaskId]) {
        self.cancel();
        for id in ids {
            if let Some(TaskState::Running(attempt)) = self.states.get(id).copied() {
                self.states.insert(id.clone(), TaskState::Done);
                self.results.insert(
                    id.clone(),
                    scheduler_result(&self.capsules[id], attempt, TaskStatus::Failed),
                );
            }
        }
    }
    pub fn result(&self, id: &TaskId) -> Option<&TaskResult> {
        self.results.get(id)
    }
    pub fn join(&self, id: &str) -> Result<JoinedResult, ScheduleError> {
        let barrier = self.barriers.get(id).ok_or(ScheduleError::InvalidJoin)?;
        let results = barrier
            .upstream
            .iter()
            .map(|task| self.results.get(task).cloned())
            .collect::<Vec<_>>();
        let missing = barrier
            .upstream
            .iter()
            .filter(|id| self.states[*id] != TaskState::Done)
            .cloned()
            .collect::<Vec<_>>();
        let failed = barrier
            .upstream
            .iter()
            .filter(|id| {
                self.states[*id] == TaskState::Done
                    && self
                        .results
                        .get(*id)
                        .is_some_and(|r| !r.success(&self.capsules[*id]))
            })
            .cloned()
            .collect::<Vec<_>>();
        let successes = barrier.upstream.len() - missing.len() - failed.len();
        let state = match barrier.policy {
            JoinPolicy::AllRequired => {
                if !missing.is_empty() {
                    JoinState::Pending
                } else if failed.is_empty() {
                    JoinState::Satisfied
                } else {
                    JoinState::Failed
                }
            }
            JoinPolicy::AnySuccess => {
                if successes > 0 {
                    JoinState::Satisfied
                } else if missing.is_empty() {
                    JoinState::Failed
                } else {
                    JoinState::Pending
                }
            }
            JoinPolicy::Quorum(n) => {
                if successes >= n {
                    JoinState::Satisfied
                } else if successes + missing.len() < n {
                    JoinState::Failed
                } else {
                    JoinState::Pending
                }
            }
            JoinPolicy::FailFast => {
                if !failed.is_empty() {
                    JoinState::Failed
                } else if missing.is_empty() {
                    JoinState::Satisfied
                } else {
                    JoinState::Pending
                }
            }
            JoinPolicy::CollectAll => {
                if missing.is_empty() {
                    JoinState::Satisfied
                } else {
                    JoinState::Pending
                }
            }
        };
        let satisfied = state == JoinState::Satisfied;
        Ok(JoinedResult {
            barrier: id.to_owned(),
            state,
            satisfied,
            results,
            missing,
            failed,
        })
    }
    /// Dispatch one maximal conflict-free wave on scoped threads and fold in task-id order.
    pub fn run_wave(
        &mut self,
        runtime: &dyn SubagentRuntime,
        host: &dyn ToolPort,
        verifier: &dyn ResultVerifier,
        snapshots: &dyn SnapshotPort,
        authority: &dyn DispatchAuthority,
    ) -> Result<Vec<TaskResult>, ScheduleError> {
        let batch = self.next_batch(authority);
        let batch_ids = batch.iter().map(|c| c.spec.id.clone()).collect::<Vec<_>>();
        let produced = std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for capsule in batch {
                let attempt = match self.states[&capsule.spec.id] {
                    TaskState::Running(n) => n,
                    _ => unreachable!(),
                };
                handles.push(scope.spawn(move || {
                    if snapshots.current_snapshot(&capsule).as_deref()
                        != Ok(capsule.input_snapshot())
                    {
                        return scheduler_result(&capsule, attempt, TaskStatus::StaleInput);
                    }
                    let started = Instant::now();
                    let guard = GuardedTools::new(&capsule, host);
                    let mut result = runtime.execute(&capsule, &guard, attempt);
                    let writes = guard.writes.lock().expect("audit lock");
                    if !result.resource_changes.is_subset(&writes) {
                        result.verified = false;
                    }
                    if started.elapsed().as_millis() > u128::from(capsule.spec.timeout_ms) {
                        result.status = TaskStatus::RetryExhausted;
                        result.verified = false;
                    }
                    if snapshots.current_snapshot(&capsule).as_deref()
                        != Ok(capsule.input_snapshot())
                    {
                        result.status = TaskStatus::StaleInput;
                        result.verified = false;
                    }
                    result
                }));
            }
            handles
                .into_iter()
                .map(|handle| handle.join().map_err(|_| ScheduleError::InvalidResult))
                .collect::<Result<Vec<_>, _>>()
        });
        let produced = match produced {
            Ok(results) => results,
            Err(error) => {
                self.abort_running(&batch_ids);
                return Err(error);
            }
        };
        let mut produced = produced;
        produced.sort_by(|a, b| a.task_id.cmp(&b.task_id));
        let ids = produced
            .iter()
            .map(|r| r.task_id.clone())
            .collect::<Vec<_>>();
        for result in produced {
            if let Err(error) = self.submit(result, verifier) {
                self.abort_running(&batch_ids);
                return Err(error);
            }
        }
        Ok(ids.iter().map(|id| self.results[id].clone()).collect())
    }
}
fn scheduler_result(capsule: &TaskCapsule, attempt: u32, status: TaskStatus) -> TaskResult {
    TaskResult {
        task_id: capsule.spec.id.clone(),
        task_digest: capsule.digest.clone(),
        input_snapshot: capsule.input_snapshot.clone(),
        attempt,
        status,
        structured_output: serde_json::Value::Null,
        evidence: BTreeSet::new(),
        resource_changes: BTreeSet::new(),
        out_of_scope_observations: Vec::new(),
        execution_trace_ref: format!("scheduler-{}-{attempt}", capsule.spec.id),
        runtime_provenance: "scheduler".into(),
        model_provenance: None,
        verified: false,
    }
}
fn claims_conflict(a: &[ResourceClaim], b: &[ResourceClaim]) -> bool {
    a.iter()
        .any(|left| b.iter().any(|right| left.conflicts(right)))
}
