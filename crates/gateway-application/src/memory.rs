//! Governed, compare-and-swap memory lifecycle and eligibility checks.
use gateway_context::{CompileError, ContextFragment, FragmentKind, FragmentMetadata};
use gateway_domain::{
    ContextScopeId, KnowledgeProvenance, NonEmptyText, PlanStepId, ReferenceId, UnixTimestamp,
    memory::{
        CurationState, ExperienceRecord, MEMORY_SCHEMA_VERSION, MemoryEligibilityReference,
        MemoryEntry, MemoryPayload, MemoryReason,
    },
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryAction {
    Admit,
    Validate,
    Reject,
    Refresh,
    Supersede,
    Invalidate,
    Forget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurationDecision {
    pub scope: ContextScopeId,
    pub id: ReferenceId,
    pub input_revision: Option<u64>,
    pub input_snapshot: ReferenceId,
    pub action: MemoryAction,
    pub reason: ReferenceId,
    pub decided_at: UnixTimestamp,
    pub output_revision: u64,
    pub output_id: ReferenceId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryError {
    Missing,
    Duplicate,
    Conflict,
    RevisionConflict,
    ScopeMismatch,
    InvalidTransition,
    InvalidRecord,
    Storage,
    Ineligible(Vec<MemoryReason>),
    Context(CompileError),
}

/// An implementation must atomically check the expected revision, write the
/// new projection and append the decision. Forgotten payload bytes must be purged.
pub trait MemoryStore {
    fn get(
        &self,
        scope: &ContextScopeId,
        id: &ReferenceId,
    ) -> Result<Option<MemoryEntry>, MemoryError>;
    fn list(&self, scope: &ContextScopeId) -> Result<Vec<MemoryEntry>, MemoryError>;
    fn commit(
        &mut self,
        expected: Option<u64>,
        entry: MemoryEntry,
        decision: CurationDecision,
    ) -> Result<(), MemoryError>;
    fn decisions(
        &self,
        scope: &ContextScopeId,
        id: &ReferenceId,
    ) -> Result<Vec<CurationDecision>, MemoryError>;
}

pub struct MemoryChange {
    pub action: MemoryAction,
    pub reason: ReferenceId,
    pub at: UnixTimestamp,
    pub replacement: Option<ExperienceRecord>,
    pub successor: Option<ReferenceId>,
}

pub struct MemoryApplication<S> {
    store: S,
}
impl<S: MemoryStore> MemoryApplication<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }
    pub fn store(&self) -> &S {
        &self.store
    }
    pub fn into_store(self) -> S {
        self.store
    }

    pub fn admit(
        &mut self,
        record: ExperienceRecord,
        reason: ReferenceId,
        at: UnixTimestamp,
    ) -> Result<MemoryEntry, MemoryError> {
        if record.created_at > at {
            return Err(MemoryError::InvalidRecord);
        }
        let entry = MemoryEntry::new(record).map_err(|_| MemoryError::InvalidRecord)?;
        if self
            .store
            .get(&entry.record.scope, &entry.record.id)?
            .is_some()
        {
            return Err(MemoryError::Duplicate);
        }
        for existing in self.store.list(&entry.record.scope)? {
            if existing.state != CurationState::Forgotten
                && existing.record.source_snapshot == entry.record.source_snapshot
            {
                return Err(
                    if existing.record.source_digest == entry.record.source_digest {
                        MemoryError::Duplicate
                    } else {
                        MemoryError::Conflict
                    },
                );
            }
        }
        let decision = decision(&entry, None, MemoryAction::Admit, reason, at);
        self.store.commit(None, entry.clone(), decision)?;
        Ok(entry)
    }

    pub fn curate(
        &mut self,
        scope: &ContextScopeId,
        id: &ReferenceId,
        expected: u64,
        change: MemoryChange,
    ) -> Result<MemoryEntry, MemoryError> {
        let MemoryChange {
            action,
            reason,
            at,
            replacement,
            successor,
        } = change;
        let mut entry = self.store.get(scope, id)?.ok_or(MemoryError::Missing)?;
        if entry.record.scope != *scope {
            return Err(MemoryError::ScopeMismatch);
        }
        if entry.revision != expected {
            return Err(MemoryError::RevisionConflict);
        }
        if entry.state == CurationState::Forgotten || at < entry.record.created_at {
            return Err(MemoryError::InvalidTransition);
        }
        let old_snapshot = entry.record.source_snapshot.clone();
        match action {
            MemoryAction::Admit => return Err(MemoryError::InvalidTransition),
            MemoryAction::Validate
                if matches!(
                    entry.state,
                    CurationState::Pending | CurationState::Rejected
                ) && replacement.is_none()
                    && successor.is_none() =>
            {
                entry.state = CurationState::Validated;
                if !entry.is_current(at) {
                    return Err(MemoryError::Ineligible(entry.reasons(at)));
                }
            }
            MemoryAction::Reject
                if entry.state == CurationState::Pending
                    && replacement.is_none()
                    && successor.is_none() =>
            {
                entry.state = CurationState::Rejected
            }
            MemoryAction::Refresh
                if matches!(
                    entry.state,
                    CurationState::Pending | CurationState::Validated | CurationState::Invalidated
                ) && successor.is_none() =>
            {
                let next = replacement.ok_or(MemoryError::InvalidRecord)?;
                next.validate().map_err(|_| MemoryError::InvalidRecord)?;
                if next.scope != *scope
                    || next.id != *id
                    || next.created_at < entry.record.created_at
                    || next.created_at > at
                    || next.source_snapshot == entry.record.source_snapshot
                {
                    return Err(MemoryError::InvalidRecord);
                }
                entry.record = next;
                entry.state = CurationState::Pending;
            }
            MemoryAction::Supersede
                if entry.state == CurationState::Validated && replacement.is_none() =>
            {
                let next_id = successor.ok_or(MemoryError::InvalidTransition)?;
                if next_id == *id
                    || self
                        .store
                        .get(scope, &next_id)?
                        .is_none_or(|next| next.state == CurationState::Forgotten)
                {
                    return Err(MemoryError::InvalidTransition);
                }
                entry.state = CurationState::Superseded;
                entry.superseded_by = Some(next_id);
            }
            MemoryAction::Invalidate
                if matches!(
                    entry.state,
                    CurationState::Pending | CurationState::Validated | CurationState::Rejected
                ) && replacement.is_none()
                    && successor.is_none() =>
            {
                entry.state = CurationState::Invalidated
            }
            MemoryAction::Forget if replacement.is_none() && successor.is_none() => {
                entry.state = CurationState::Forgotten;
                entry.record.payload = None;
                entry.payload_forgotten = true;
            }
            _ => return Err(MemoryError::InvalidTransition),
        }
        entry.revision = entry
            .revision
            .checked_add(1)
            .ok_or(MemoryError::InvalidTransition)?;
        entry.eligibility_version = entry
            .eligibility_version
            .checked_add(1)
            .ok_or(MemoryError::InvalidTransition)?;
        let decision = CurationDecision {
            scope: scope.clone(),
            id: id.clone(),
            input_revision: Some(expected),
            input_snapshot: old_snapshot,
            action,
            reason,
            decided_at: at,
            output_revision: entry.revision,
            output_id: id.clone(),
        };
        self.store.commit(Some(expected), entry.clone(), decision)?;
        Ok(entry)
    }

    pub fn inspect(
        &self,
        scope: &ContextScopeId,
        id: &ReferenceId,
        at: UnixTimestamp,
    ) -> Result<(MemoryEntry, Vec<MemoryReason>), MemoryError> {
        let entry = self.store.get(scope, id)?.ok_or(MemoryError::Missing)?;
        let reasons = entry.reasons(at);
        Ok((entry, reasons))
    }

    pub fn recall(
        &self,
        scope: &ContextScopeId,
        at: UnixTimestamp,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        let mut entries: Vec<_> = self
            .store
            .list(scope)?
            .into_iter()
            .filter(|entry| entry.is_current(at))
            .collect();
        entries.sort_by(|a, b| a.record.id.cmp(&b.record.id));
        Ok(entries)
    }

    /// Case-folded lexical recall over currently eligible inline content and
    /// source metadata. Reference-only payloads require an external index.
    pub fn search(
        &self,
        scope: &ContextScopeId,
        query: &NonEmptyText,
        at: UnixTimestamp,
        maximum_sensitivity: gateway_domain::SensitivityClass,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        let needle = query.as_str().to_lowercase();
        let mut entries = self.recall(scope, at)?;
        entries.retain(|entry| {
            entry.record.quality.sensitivity() <= maximum_sensitivity
                && (entry.record.source_snapshot.as_str().to_lowercase().contains(&needle)
                    || entry.record.outcome.as_ref().is_some_and(|value| value.as_str().to_lowercase().contains(&needle))
                    || matches!(&entry.record.payload, Some(MemoryPayload::Inline(value)) if value.as_str().to_lowercase().contains(&needle)))
        });
        entries.truncate(limit);
        Ok(entries)
    }

    pub fn eligibility_reference(
        &self,
        scope: &ContextScopeId,
        id: &ReferenceId,
        at: UnixTimestamp,
    ) -> Result<MemoryEligibilityReference, MemoryError> {
        let (entry, reasons) = self.inspect(scope, id, at)?;
        let learning = entry.learning_reasons(at);
        if learning != [MemoryReason::Eligible] {
            return Err(MemoryError::Ineligible(
                if reasons == [MemoryReason::Eligible] {
                    learning
                } else {
                    reasons
                },
            ));
        }
        Ok(MemoryEligibilityReference {
            schema_version: MEMORY_SCHEMA_VERSION,
            scope: scope.clone(),
            id: id.clone(),
            revision: entry.revision,
            eligibility_version: entry.eligibility_version,
            source_snapshot: entry.record.source_snapshot.clone(),
            source_digest: entry.record.source_digest.clone(),
        })
    }

    pub fn revalidate_reference(
        &self,
        reference: &MemoryEligibilityReference,
        at: UnixTimestamp,
    ) -> Result<bool, MemoryError> {
        let Some(entry) = self.store.get(&reference.scope, &reference.id)? else {
            return Ok(false);
        };
        Ok(reference.schema_version == MEMORY_SCHEMA_VERSION
            && entry.revision == reference.revision
            && entry.eligibility_version == reference.eligibility_version
            && entry.record.source_snapshot == reference.source_snapshot
            && entry.record.source_digest == reference.source_digest
            && entry.learning_reasons(at) == [MemoryReason::Eligible])
    }

    pub fn context_fragment(
        &self,
        scope: &ContextScopeId,
        id: &ReferenceId,
        step: PlanStepId,
        at: UnixTimestamp,
    ) -> Result<ContextFragment, MemoryError> {
        let (entry, reasons) = self.inspect(scope, id, at)?;
        if reasons != [MemoryReason::Eligible] {
            return Err(MemoryError::Ineligible(reasons));
        }
        let provenance = KnowledgeProvenance::new(
            entry.record.source_snapshot.as_str(),
            Some(entry.record.source_version.as_str()),
        )
        .map_err(|_| MemoryError::InvalidRecord)?;
        let metadata = FragmentMetadata {
            provenance,
            evidence: BTreeSet::new(),
            quality: entry.record.quality,
            rationale: NonEmptyText::new("validated governed memory recall")
                .map_err(|_| MemoryError::InvalidRecord)?,
            validation: entry.record.validation.clone(),
        };
        match entry
            .record
            .payload
            .as_ref()
            .ok_or(MemoryError::InvalidRecord)?
        {
            MemoryPayload::Inline(value) => ContextFragment::external(
                id.clone(),
                FragmentKind::Memory,
                value.as_str(),
                metadata,
                scope.clone(),
                step,
            ),
            MemoryPayload::Reference(reference) => ContextFragment::memory_reference(
                id.clone(),
                reference.clone(),
                metadata,
                scope.clone(),
                step,
            ),
        }
        .map_err(MemoryError::Context)
    }
}

fn decision(
    entry: &MemoryEntry,
    input_revision: Option<u64>,
    action: MemoryAction,
    reason: ReferenceId,
    at: UnixTimestamp,
) -> CurationDecision {
    CurationDecision {
        scope: entry.record.scope.clone(),
        id: entry.record.id.clone(),
        input_revision,
        input_snapshot: entry.record.source_snapshot.clone(),
        action,
        reason,
        decided_at: at,
        output_revision: entry.revision,
        output_id: entry.record.id.clone(),
    }
}
