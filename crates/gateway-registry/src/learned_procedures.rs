//! Read-only learned procedure projection. Only the application service owns live authority.
use gateway_domain::{
    ContentDigest, ContextScopeId, ReferenceId, ValidationError, learning::LearnedProcedure,
    procedure_promotion::*,
};
use std::collections::{BTreeMap, BTreeSet};

fn invalid(reason: &'static str) -> ValidationError {
    ValidationError::InvalidDeclarativeValue { reason }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionRecord {
    pub request: ExecutionRequest,
    pub reserved_at: i64,
    pub outcome: Option<RuntimeOutcome>,
    pub outcome_evidence: Option<ReferenceId>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcedureEntry {
    procedure: LearnedProcedure,
    state: PromotionState,
    evaluation: Option<ContentDigest>,
    canary: Option<CanaryBoundary>,
    predecessor: Option<ProcedureVersion>,
    executions: BTreeMap<ReferenceId, ExecutionRecord>,
    history: Vec<usize>,
}
impl ProcedureEntry {
    pub fn procedure(&self) -> &LearnedProcedure {
        &self.procedure
    }
    pub fn state(&self) -> PromotionState {
        self.state
    }
    pub fn evaluation(&self) -> Option<&ContentDigest> {
        self.evaluation.as_ref()
    }
    pub fn canary(&self) -> Option<&CanaryBoundary> {
        self.canary.as_ref()
    }
    pub fn predecessor(&self) -> Option<&ProcedureVersion> {
        self.predecessor.as_ref()
    }
    pub fn executions(&self) -> &BTreeMap<ReferenceId, ExecutionRecord> {
        &self.executions
    }
    /// Indexes into the journal, including both sides of supersession and rollback.
    pub fn history(&self) -> &[usize] {
        &self.history
    }
    fn canary_counts(&self) -> (usize, usize, usize, usize) {
        let records = self
            .executions
            .values()
            .filter(|e| e.request.mode == ExecutionMode::Canary)
            .collect::<Vec<_>>();
        (
            records.len(),
            records
                .iter()
                .filter(|e| e.outcome == Some(RuntimeOutcome::Success))
                .count(),
            records
                .iter()
                .filter(|e| e.outcome.is_some_and(|o| o != RuntimeOutcome::Success))
                .count(),
            records.iter().filter(|e| e.outcome.is_none()).count(),
        )
    }
    fn ready(&self, at: i64) -> Result<(), ValidationError> {
        if self.state != PromotionState::Canary {
            return Err(invalid("successful canary required"));
        }
        let boundary = self
            .canary
            .as_ref()
            .ok_or_else(|| invalid("missing canary boundary"))?;
        let (_, successes, failures, pending) = self.canary_counts();
        if at < boundary.starts_at
            || successes < boundary.required_successes as usize
            || failures > boundary.max_failures as usize
            || pending > 0
        {
            return Err(invalid("canary activation criteria not met"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LearnedProcedureRegistry {
    entries: BTreeMap<(ReferenceId, u32), ProcedureEntry>,
    active: BTreeMap<ReferenceId, ProcedureVersion>,
}
impl LearnedProcedureRegistry {
    /// Recomputes every state from trusted history; cached state is never accepted.
    pub fn from_journal(journal: &PromotionJournal) -> Result<Self, ValidationError> {
        if journal.schema_version != 1 {
            return Err(invalid("unsupported promotion journal version"));
        }
        let mut registry = Self::default();
        let mut decisions = BTreeSet::new();
        let mut last_at = 0;
        for (index, event) in journal.events.iter().enumerate() {
            if event.metadata.at < last_at || !decisions.insert(event.metadata.id.clone()) {
                return Err(invalid("duplicate decision or nonmonotonic decision time"));
            }
            registry.apply(event, index)?;
            last_at = event.metadata.at;
        }
        Ok(registry)
    }
    pub fn entries(&self) -> impl Iterator<Item = &ProcedureEntry> {
        self.entries.values()
    }
    pub fn get(&self, version: &ProcedureVersion) -> Option<&ProcedureEntry> {
        self.entries
            .get(&(version.id.clone(), version.version))
            .filter(|e| e.procedure.digest() == &version.digest)
    }
    pub fn active(&self, id: &ReferenceId) -> Option<&ProcedureVersion> {
        self.active.get(id)
    }
    fn entry(&self, version: &ProcedureVersion) -> Result<&ProcedureEntry, ValidationError> {
        self.get(version)
            .ok_or_else(|| invalid("unknown procedure identity/version/digest"))
    }
    fn entry_mut(
        &mut self,
        version: &ProcedureVersion,
    ) -> Result<&mut ProcedureEntry, ValidationError> {
        self.entry(version)?;
        Ok(self
            .entries
            .get_mut(&(version.id.clone(), version.version))
            .expect("checked entry"))
    }
    /// Eligibility only, never runtime permission. Current process/policy/facts still apply.
    pub fn eligible(
        &self,
        version: &ProcedureVersion,
        scope: &ContextScopeId,
        cohort: &ReferenceId,
        mode: ExecutionMode,
        at: i64,
    ) -> Result<(), ValidationError> {
        let entry = self.entry(version)?;
        if scope != entry.procedure.fingerprint().scope() || at < 0 {
            return Err(invalid("execution scope/time mismatch"));
        }
        match mode {
            ExecutionMode::Active
                if entry.state == PromotionState::Active
                    && self.active(&version.id) == Some(version) =>
            {
                Ok(())
            }
            ExecutionMode::Canary if entry.state == PromotionState::Canary => {
                let boundary = entry
                    .canary
                    .as_ref()
                    .ok_or_else(|| invalid("missing canary boundary"))?;
                let (used, _, failures, pending) = entry.canary_counts();
                // Bound in-flight trials by remaining failure tolerance plus one.
                if at < boundary.starts_at
                    || at >= boundary.ends_at
                    || !boundary.cohorts.contains(cohort)
                    || used >= boundary.max_executions as usize
                    || failures > boundary.max_failures as usize
                    || failures + pending > boundary.max_failures as usize
                {
                    return Err(invalid("execution outside canary boundary"));
                }
                Ok(())
            }
            _ => Err(invalid(
                "procedure is not eligible for requested execution mode",
            )),
        }
    }
    fn transition(
        &mut self,
        version: &ProcedureVersion,
        state: PromotionState,
        index: usize,
    ) -> Result<(), ValidationError> {
        let entry = self.entry_mut(version)?;
        entry.state = state;
        entry.history.push(index);
        Ok(())
    }
    fn apply(&mut self, event: &PromotionEvent, index: usize) -> Result<(), ValidationError> {
        use PromotionCommand::*;
        use PromotionState::*;
        let at = event.metadata.at;
        match &event.command {
            Discover { procedure, .. } => {
                let json = procedure
                    .to_json()
                    .map_err(|_| invalid("invalid procedure"))?;
                LearnedProcedure::from_json(&json).map_err(|_| invalid("invalid procedure"))?;
                let key = (procedure.id().clone(), procedure.version());
                if self.entries.contains_key(&key) {
                    return Err(invalid("procedure versions are immutable and unique"));
                }
                self.entries.insert(
                    key,
                    ProcedureEntry {
                        procedure: *procedure.clone(),
                        state: Discovered,
                        evaluation: None,
                        canary: None,
                        predecessor: None,
                        executions: BTreeMap::new(),
                        history: vec![index],
                    },
                );
            }
            Advance {
                procedure,
                from,
                to,
                ..
            } => {
                if self.entry(procedure)?.state != *from || !from.allows_advance(*to) {
                    return Err(invalid("illegal promotion transition"));
                }
                self.transition(procedure, *to, index)?;
            }
            Evaluate { procedure, bundle } => {
                let entry = self.entry(procedure)?;
                if entry.state != Validated {
                    return Err(invalid("validated procedure required"));
                }
                bundle.proves(&entry.procedure)?;
                self.entry_mut(procedure)?.evaluation = Some(bundle.digest.clone());
                self.transition(procedure, Evaluated, index)?;
            }
            Approve {
                procedure,
                evaluation_digest,
            } => {
                let entry = self.entry(procedure)?;
                if entry.state != Evaluated || entry.evaluation.as_ref() != Some(evaluation_digest)
                {
                    return Err(invalid(
                        "approval requires exact passing evaluation evidence",
                    ));
                }
                self.transition(procedure, Approved, index)?;
            }
            StartCanary {
                procedure,
                boundary,
            } => {
                let entry = self.entry(procedure)?;
                if entry.state != Approved || at > boundary.starts_at {
                    return Err(invalid(
                        "approved procedure and future canary boundary required",
                    ));
                }
                boundary.validate(&entry.procedure)?;
                self.entry_mut(procedure)?.canary = Some(boundary.clone());
                self.transition(procedure, Canary, index)?;
            }
            Activate { procedure } => {
                self.entry(procedure)?.ready(at)?;
                if self.active.contains_key(&procedure.id) {
                    return Err(invalid("explicit supersession of active version required"));
                }
                self.transition(procedure, Active, index)?;
                self.active.insert(procedure.id.clone(), procedure.clone());
            }
            Supersede {
                procedure,
                previous,
            } => {
                self.entry(procedure)?.ready(at)?;
                if procedure.id != previous.id
                    || procedure.version <= previous.version
                    || self.active(&previous.id) != Some(previous)
                    || self.entry(previous)?.state != Active
                {
                    return Err(invalid("supersession requires exact active older version"));
                }
                self.entry_mut(procedure)?.predecessor = Some(previous.clone());
                self.transition(previous, Superseded, index)?;
                self.transition(procedure, Active, index)?;
                self.active.insert(procedure.id.clone(), procedure.clone());
            }
            Rollback { procedure, restore } => {
                let entry = self.entry(procedure)?;
                if !matches!(entry.state, Canary | Active) {
                    return Err(invalid("only canary or active procedures can roll back"));
                }
                if entry.state == Canary {
                    if restore.is_some() {
                        return Err(invalid("canary rollback cannot change active version"));
                    }
                } else {
                    if entry.predecessor.as_ref() != restore.as_ref() {
                        return Err(invalid("rollback requires exact known safe predecessor"));
                    }
                    if self.active(&procedure.id) != Some(procedure) {
                        return Err(invalid("rollback target is not active"));
                    }
                    if let Some(safe) = restore {
                        if self.entry(safe)?.state != Superseded {
                            return Err(invalid("rollback predecessor is disabled or not safe"));
                        }
                        self.transition(safe, Active, index)?;
                        self.active.insert(procedure.id.clone(), safe.clone());
                    } else {
                        self.active.remove(&procedure.id);
                    }
                }
                self.transition(procedure, RolledBack, index)?;
            }
            Disable { procedure, .. } => {
                if !matches!(
                    self.entry(procedure)?.state,
                    Approved | Canary | Active | Superseded
                ) {
                    return Err(invalid("procedure cannot be disabled from this state"));
                }
                if self.active(&procedure.id) == Some(procedure) {
                    self.active.remove(&procedure.id);
                }
                self.transition(procedure, Deprecated, index)?;
            }
            ReserveExecution {
                procedure,
                execution,
            } => {
                self.eligible(
                    procedure,
                    &execution.scope,
                    &execution.cohort,
                    execution.mode,
                    at,
                )?;
                if self
                    .entries
                    .values()
                    .any(|e| e.executions.contains_key(&execution.id))
                {
                    return Err(invalid("execution identity already reserved"));
                }
                let entry = self.entry_mut(procedure)?;
                entry.executions.insert(
                    execution.id.clone(),
                    ExecutionRecord {
                        request: execution.clone(),
                        reserved_at: at,
                        outcome: None,
                        outcome_evidence: None,
                    },
                );
                entry.history.push(index);
            }
            RecordOutcome {
                procedure,
                execution_id,
                outcome,
                evidence,
            } => {
                let entry = self.entry_mut(procedure)?;
                let execution = entry.executions.get_mut(execution_id).ok_or_else(|| {
                    invalid("outcome requires a reserved execution for exact version")
                })?;
                if execution.outcome.is_some() || at < execution.reserved_at {
                    return Err(invalid("duplicate or reversed execution outcome"));
                }
                // Late outcomes remain attributable after disable/rollback/supersession.
                execution.outcome = Some(*outcome);
                execution.outcome_evidence = Some(evidence.clone());
                entry.history.push(index);
            }
        }
        Ok(())
    }
}
