//! Replaceable process-local adapter for verified execution references.
use gateway_application::experience_patterns::{
    ExperienceIngestionPort, PatternError, PatternLimits, VerifiedExecution,
};
use gateway_domain::ContextScopeId;
use std::collections::BTreeMap;

#[derive(Default)]
pub struct InMemoryVerifiedExecutionSource {
    entries: BTreeMap<ContextScopeId, Vec<VerifiedExecution>>,
}

impl InMemoryVerifiedExecutionSource {
    pub fn insert(&mut self, scope: ContextScopeId, execution: VerifiedExecution) {
        self.entries.entry(scope).or_default().push(execution);
    }
}

impl ExperienceIngestionPort for InMemoryVerifiedExecutionSource {
    fn list_verified(
        &self,
        scope: &ContextScopeId,
        limits: PatternLimits,
    ) -> Result<Vec<VerifiedExecution>, PatternError> {
        let entries = self.entries.get(scope).map_or(&[][..], Vec::as_slice);
        if entries.len() > limits.max_inputs {
            return Err(PatternError::TooManyInputs);
        }
        if entries.iter().any(|entry| {
            entry.signals.len() > limits.max_signals
                || entry.evidence.len() > limits.max_evidence
                || entry
                    .semantic_hint
                    .as_ref()
                    .is_some_and(|hint| hint.len() > 256)
        }) {
            return Err(PatternError::InvalidExecution);
        }
        Ok(entries.to_vec())
    }
}
