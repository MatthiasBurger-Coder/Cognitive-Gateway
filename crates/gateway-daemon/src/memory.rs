//! Replaceable in-memory proof adapter for governed memory storage.
use gateway_application::memory::{CurationDecision, MemoryError, MemoryStore};
use gateway_domain::{ContextScopeId, ReferenceId, memory::MemoryEntry};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct InMemoryMemoryStore {
    entries: BTreeMap<(ContextScopeId, ReferenceId), MemoryEntry>,
    decisions: BTreeMap<(ContextScopeId, ReferenceId), Vec<CurationDecision>>,
}

impl MemoryStore for InMemoryMemoryStore {
    fn get(
        &self,
        scope: &ContextScopeId,
        id: &ReferenceId,
    ) -> Result<Option<MemoryEntry>, MemoryError> {
        Ok(self.entries.get(&(scope.clone(), id.clone())).cloned())
    }
    fn list(&self, scope: &ContextScopeId) -> Result<Vec<MemoryEntry>, MemoryError> {
        Ok(self
            .entries
            .iter()
            .filter(|((s, _), _)| s == scope)
            .map(|(_, entry)| entry.clone())
            .collect())
    }
    fn commit(
        &mut self,
        expected: Option<u64>,
        entry: MemoryEntry,
        decision: CurationDecision,
    ) -> Result<(), MemoryError> {
        let key = (entry.record.scope.clone(), entry.record.id.clone());
        if self.entries.get(&key).map(|entry| entry.revision) != expected {
            return Err(MemoryError::RevisionConflict);
        }
        if decision.scope != key.0
            || decision.id != key.1
            || decision.output_revision != entry.revision
            || decision.output_id != key.1
            || decision.input_revision != expected
        {
            return Err(MemoryError::InvalidRecord);
        }
        self.entries.insert(key.clone(), entry);
        self.decisions.entry(key).or_default().push(decision);
        Ok(())
    }
    fn decisions(
        &self,
        scope: &ContextScopeId,
        id: &ReferenceId,
    ) -> Result<Vec<CurationDecision>, MemoryError> {
        Ok(self
            .decisions
            .get(&(scope.clone(), id.clone()))
            .cloned()
            .unwrap_or_default())
    }
}
