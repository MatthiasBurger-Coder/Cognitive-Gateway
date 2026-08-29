//! CG-06.07 scoped external-context boundary and neutral proof adapters.
//!
//! The application layer owns the request/session and cache contracts.  It
//! does not register projects, interpret provider configuration or mutate the
//! Gateway's Agent/Skill/Capability/Process catalogs.

use std::{
    collections::BTreeMap,
    error::Error,
    fmt,
    sync::{Mutex, MutexGuard},
};

use gateway_domain::{
    ContentDigest, ContextCacheEntryId, ContextScopeId, DeclarativeContextVersion, Intent,
    ObservationEvidenceSet, QualityMetadata, SourceId, SourceKind, SourceTimestamp, SubjectPath,
    ValidationError,
};

use crate::ports::{
    inbound::{DeclarativeIntentInputPort, ObservationEvidenceInputPort, ScopeLifecyclePort},
    outbound::{CachePort, ScopedContextSource},
};

type CacheMap = BTreeMap<(ContextScopeId, ContextCacheEntryId), CacheEntry>;

/// Errors at the scoped application boundary never include raw input content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextBoundaryError {
    Domain(ValidationError),
    ScopeAlreadyExists { scope: String },
    ScopeNotFound { scope: String },
    ScopeNotOpen { scope: String },
    StaleScopeHandle { scope: String },
    ScopeMismatch { expected: String, actual: String },
    ConflictingRetry { scope: String },
    CacheConflict { scope: String, entry: String },
    CacheMiss { scope: String, entry: String },
    StorageUnavailable,
}

impl fmt::Display for ContextBoundaryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Domain(error) => write!(formatter, "context input rejected: {error}"),
            Self::ScopeAlreadyExists { scope } => {
                write!(formatter, "context scope {scope:?} already exists")
            }
            Self::ScopeNotFound { scope } => write!(formatter, "context scope {scope:?} not found"),
            Self::ScopeNotOpen { scope } => {
                write!(formatter, "context scope {scope:?} is not open")
            }
            Self::StaleScopeHandle { scope } => {
                write!(formatter, "context scope handle {scope:?} is stale")
            }
            Self::ScopeMismatch { expected, actual } => write!(
                formatter,
                "context scope mismatch: expected {expected:?}, received {actual:?}"
            ),
            Self::ConflictingRetry { scope } => write!(
                formatter,
                "context retry for scope {scope:?} has different explicit source data"
            ),
            Self::CacheConflict { scope, entry } => write!(
                formatter,
                "cache entry {entry:?} conflicts in context scope {scope:?}"
            ),
            Self::CacheMiss { scope, entry } => write!(
                formatter,
                "cache entry {entry:?} is not present in context scope {scope:?}"
            ),
            Self::StorageUnavailable => {
                formatter.write_str("scoped context storage is unavailable")
            }
        }
    }
}

impl Error for ContextBoundaryError {}

impl From<ValidationError> for ContextBoundaryError {
    fn from(error: ValidationError) -> Self {
        Self::Domain(error)
    }
}

/// Explicit lifecycle of one transient external-context scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum ScopeLifecycle {
    Open,
    Sealed,
    Closed,
}

impl ScopeLifecycle {
    /// Returns the stable lifecycle value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "OPEN",
            Self::Sealed => "SEALED",
            Self::Closed => "CLOSED",
        }
    }

    /// Returns whether new intent or observation input is accepted.
    #[must_use]
    pub const fn accepts_input(self) -> bool {
        matches!(self, Self::Open)
    }
}

impl fmt::Display for ScopeLifecycle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Opaque application request/session scope; it is not a project profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextScope {
    id: ContextScopeId,
    lifecycle: ScopeLifecycle,
}

impl ContextScope {
    /// Opens a new explicit scope without requiring project registration.
    #[must_use]
    pub const fn new(id: ContextScopeId) -> Self {
        Self {
            id,
            lifecycle: ScopeLifecycle::Open,
        }
    }

    /// Returns the opaque scope identity.
    #[must_use]
    pub fn id(&self) -> &ContextScopeId {
        &self.id
    }

    /// Returns the current lifecycle value.
    #[must_use]
    pub const fn lifecycle(&self) -> ScopeLifecycle {
        self.lifecycle
    }
}

/// Source snapshot metadata used for retry identity and cache invalidation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct SourceSnapshot {
    source_id: SourceId,
    source_kind: SourceKind,
    change_token: Option<SourceTimestamp>,
    digest: Option<ContentDigest>,
}

impl SourceSnapshot {
    /// Creates a snapshot with at least one explicit source change marker.
    pub fn new(
        source_id: SourceId,
        source_kind: SourceKind,
        change_token: Option<SourceTimestamp>,
        digest: Option<ContentDigest>,
    ) -> Result<Self, ValidationError> {
        if change_token.is_none() && digest.is_none() {
            return Err(ValidationError::InvalidDeclarativeValue {
                reason: "source snapshot requires a change token or content digest",
            });
        }
        Ok(Self {
            source_id,
            source_kind,
            change_token,
            digest,
        })
    }

    /// Returns the source identity.
    #[must_use]
    pub fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    /// Returns the provider-neutral source kind.
    #[must_use]
    pub const fn source_kind(&self) -> SourceKind {
        self.source_kind
    }

    /// Returns the opaque source change token.
    #[must_use]
    pub fn change_token(&self) -> Option<&SourceTimestamp> {
        self.change_token.as_ref()
    }

    /// Returns the optional content digest.
    #[must_use]
    pub fn digest(&self) -> Option<&ContentDigest> {
        self.digest.as_ref()
    }

    fn identity_key(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            self.source_kind,
            self.source_id,
            self.change_token
                .as_ref()
                .map_or("", SourceTimestamp::as_str),
            self.digest.as_ref().map_or("", ContentDigest::as_str)
        )
    }
}

/// A scope-bound, validated observation/evidence ingestion batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedObservationBatch {
    scope: ContextScopeId,
    snapshot: SourceSnapshot,
    records: ObservationEvidenceSet,
    quality_metadata: BTreeMap<SubjectPath, Vec<QualityMetadata>>,
}

impl ScopedObservationBatch {
    /// Creates a batch whose scope and source snapshot are explicit.
    pub fn new(
        scope: ContextScopeId,
        snapshot: SourceSnapshot,
        records: ObservationEvidenceSet,
    ) -> Result<Self, ContextBoundaryError> {
        records.validate()?;
        Ok(Self {
            scope,
            snapshot,
            records,
            quality_metadata: BTreeMap::new(),
        })
    }

    /// Adds quality metadata without copying it into cache keys or diagnostics.
    #[must_use]
    pub fn with_quality_metadata(
        mut self,
        subject: SubjectPath,
        mut metadata: Vec<QualityMetadata>,
    ) -> Self {
        metadata.sort();
        metadata.dedup();
        self.quality_metadata.insert(subject, metadata);
        self
    }

    /// Returns the owning scope.
    #[must_use]
    pub fn scope(&self) -> &ContextScopeId {
        &self.scope
    }

    /// Returns source snapshot metadata.
    #[must_use]
    pub const fn snapshot(&self) -> &SourceSnapshot {
        &self.snapshot
    }

    /// Returns the validated lineage records.
    #[must_use]
    pub const fn records(&self) -> &ObservationEvidenceSet {
        &self.records
    }

    /// Returns quality metadata grouped by subject.
    #[must_use]
    pub const fn quality_metadata(&self) -> &BTreeMap<SubjectPath, Vec<QualityMetadata>> {
        &self.quality_metadata
    }

    /// Returns a deterministic retry identity containing no raw content.
    #[must_use]
    pub fn ingestion_key(&self) -> IngestionKey {
        let mut key = self.snapshot.identity_key();
        for provenance in self.records.provenances() {
            key.push_str("|p:");
            key.push_str(provenance.id().as_str());
        }
        for observation in self.records.observations() {
            key.push_str("|o:");
            key.push_str(observation.id().as_str());
        }
        for fact in self.records.facts() {
            key.push_str("|f:");
            key.push_str(fact.id().as_str());
        }
        for evidence in self.records.evidence() {
            key.push_str("|e:");
            key.push_str(evidence.id().as_str());
        }
        IngestionKey(key)
    }
}

/// Opaque deterministic retry identity derived from source metadata and typed IDs.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct IngestionKey(String);

impl IngestionKey {
    /// Returns the non-sensitive canonical key text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Result of a deterministic idempotent ingestion attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum IngestionResult {
    Accepted,
    IdempotentReplay,
}

/// Receipt returned by the scoped observation/evidence input port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestionReceipt {
    key: IngestionKey,
    result: IngestionResult,
}

impl IngestionReceipt {
    /// Returns the retry identity.
    #[must_use]
    pub const fn key(&self) -> &IngestionKey {
        &self.key
    }

    /// Returns whether this attempt changed scoped state or replayed it.
    #[must_use]
    pub const fn result(&self) -> IngestionResult {
        self.result
    }
}

#[derive(Debug, Clone)]
struct ScopedSession {
    scope: ContextScope,
    intent: Option<Intent>,
    batches: BTreeMap<IngestionKey, ScopedObservationBatch>,
}

/// A read-only view of one scoped transient session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedContextSnapshot {
    scope: ContextScope,
    intent: Option<Intent>,
    batches: Vec<ScopedObservationBatch>,
}

impl ScopedContextSnapshot {
    /// Returns the scope handle at snapshot time.
    #[must_use]
    pub const fn scope(&self) -> &ContextScope {
        &self.scope
    }

    /// Returns the optional structured intent.
    #[must_use]
    pub const fn intent(&self) -> Option<&Intent> {
        self.intent.as_ref()
    }

    /// Returns canonical batches in ingestion-key order.
    #[must_use]
    pub fn batches(&self) -> &[ScopedObservationBatch] {
        &self.batches
    }
}

/// Neutral in-memory proof adapter for scope isolation and lifecycle semantics.
#[derive(Debug, Default)]
pub struct InMemoryContextStore {
    sessions: Mutex<BTreeMap<ContextScopeId, ScopedSession>>,
}

impl InMemoryContextStore {
    /// Creates an empty store; no global current-project state exists.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            sessions: Mutex::new(BTreeMap::new()),
        }
    }

    fn sessions(
        &self,
    ) -> Result<MutexGuard<'_, BTreeMap<ContextScopeId, ScopedSession>>, ContextBoundaryError> {
        self.sessions
            .lock()
            .map_err(|_| ContextBoundaryError::StorageUnavailable)
    }

    fn session_mut<'a>(
        sessions: &'a mut BTreeMap<ContextScopeId, ScopedSession>,
        scope: &ContextScope,
    ) -> Result<&'a mut ScopedSession, ContextBoundaryError> {
        let session =
            sessions
                .get_mut(scope.id())
                .ok_or_else(|| ContextBoundaryError::ScopeNotFound {
                    scope: scope.id().to_string(),
                })?;
        if session.scope.lifecycle() != scope.lifecycle() {
            return Err(ContextBoundaryError::StaleScopeHandle {
                scope: scope.id().to_string(),
            });
        }
        Ok(session)
    }

    fn open_session(session: &ScopedSession) -> Result<(), ContextBoundaryError> {
        if session.scope.lifecycle().accepts_input() {
            Ok(())
        } else {
            Err(ContextBoundaryError::ScopeNotOpen {
                scope: session.scope.id().to_string(),
            })
        }
    }

    /// Returns the currently registered scope handle without exposing content.
    pub fn scope(&self, id: &ContextScopeId) -> Result<Option<ContextScope>, ContextBoundaryError> {
        Ok(self
            .sessions()?
            .get(id)
            .map(|session| session.scope.clone()))
    }

    /// Reads one scope's transient data by explicit handle.
    pub fn snapshot(
        &self,
        scope: &ContextScope,
    ) -> Result<ScopedContextSnapshot, ContextBoundaryError> {
        let mut sessions = self.sessions()?;
        let session = Self::session_mut(&mut sessions, scope)?;
        Ok(ScopedContextSnapshot {
            scope: session.scope.clone(),
            intent: session.intent.clone(),
            batches: session.batches.values().cloned().collect(),
        })
    }

    /// Removes all transient data for a scope and returns a closed handle.
    pub fn close(&self, scope: &ContextScope) -> Result<ContextScope, ContextBoundaryError> {
        let mut sessions = self.sessions()?;
        let session =
            sessions
                .get(scope.id())
                .ok_or_else(|| ContextBoundaryError::ScopeNotFound {
                    scope: scope.id().to_string(),
                })?;
        if session.scope.lifecycle() != scope.lifecycle() {
            return Err(ContextBoundaryError::StaleScopeHandle {
                scope: scope.id().to_string(),
            });
        }
        sessions.remove(scope.id());
        Ok(ContextScope {
            id: scope.id().clone(),
            lifecycle: ScopeLifecycle::Closed,
        })
    }
}

impl DeclarativeIntentInputPort for InMemoryContextStore {
    type Error = ContextBoundaryError;

    fn submit_intent(
        &self,
        scope: &ContextScope,
        intent: Intent,
    ) -> Result<IngestionResult, Self::Error> {
        let mut sessions = self.sessions()?;
        let session = Self::session_mut(&mut sessions, scope)?;
        Self::open_session(session)?;
        match &session.intent {
            None => {
                session.intent = Some(intent);
                Ok(IngestionResult::Accepted)
            }
            Some(existing) if existing == &intent => Ok(IngestionResult::IdempotentReplay),
            Some(_) => Err(ContextBoundaryError::ConflictingRetry {
                scope: scope.id().to_string(),
            }),
        }
    }
}

impl ObservationEvidenceInputPort for InMemoryContextStore {
    type Error = ContextBoundaryError;

    fn ingest_observations(
        &self,
        scope: &ContextScope,
        batch: ScopedObservationBatch,
    ) -> Result<IngestionReceipt, Self::Error> {
        if batch.scope() != scope.id() {
            return Err(ContextBoundaryError::ScopeMismatch {
                expected: scope.id().to_string(),
                actual: batch.scope().to_string(),
            });
        }
        let mut sessions = self.sessions()?;
        let session = Self::session_mut(&mut sessions, scope)?;
        Self::open_session(session)?;
        let key = batch.ingestion_key();
        let result = match session.batches.get(&key) {
            None => {
                session.batches.insert(key.clone(), batch);
                IngestionResult::Accepted
            }
            Some(existing) if existing == &batch => IngestionResult::IdempotentReplay,
            Some(_) => {
                return Err(ContextBoundaryError::ConflictingRetry {
                    scope: scope.id().to_string(),
                });
            }
        };
        Ok(IngestionReceipt { key, result })
    }
}

impl ScopeLifecyclePort for InMemoryContextStore {
    type Error = ContextBoundaryError;

    fn open_scope(&self, id: ContextScopeId) -> Result<ContextScope, Self::Error> {
        let mut sessions = self.sessions()?;
        if sessions.contains_key(&id) {
            return Err(ContextBoundaryError::ScopeAlreadyExists {
                scope: id.to_string(),
            });
        }
        let scope = ContextScope::new(id.clone());
        sessions.insert(
            id,
            ScopedSession {
                scope: scope.clone(),
                intent: None,
                batches: BTreeMap::new(),
            },
        );
        Ok(scope)
    }

    fn seal_scope(&self, scope: &ContextScope) -> Result<ContextScope, Self::Error> {
        let mut sessions = self.sessions()?;
        let session = Self::session_mut(&mut sessions, scope)?;
        if session.scope.lifecycle() != ScopeLifecycle::Open {
            return Err(ContextBoundaryError::ScopeNotOpen {
                scope: scope.id().to_string(),
            });
        }
        session.scope.lifecycle = ScopeLifecycle::Sealed;
        Ok(session.scope.clone())
    }

    fn close_scope(&self, scope: &ContextScope) -> Result<ContextScope, Self::Error> {
        self.close(scope)
    }
}

/// Cache retention capability exposed to later policy/operations adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum CacheRetention {
    UntilInvalidated,
    Ephemeral,
}

/// Explicit cache handling capabilities; these are not authorization decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct CacheCapabilities {
    retains_sensitivity_metadata: bool,
    supports_invalidation: bool,
    stores_raw_content: bool,
    retention: CacheRetention,
}

impl CacheCapabilities {
    /// Creates explicit handling capabilities for a cache implementation.
    #[must_use]
    pub const fn new(
        retains_sensitivity_metadata: bool,
        supports_invalidation: bool,
        stores_raw_content: bool,
        retention: CacheRetention,
    ) -> Self {
        Self {
            retains_sensitivity_metadata,
            supports_invalidation,
            stores_raw_content,
            retention,
        }
    }

    /// Returns whether sensitivity metadata survives the cache boundary.
    #[must_use]
    pub const fn retains_sensitivity_metadata(self) -> bool {
        self.retains_sensitivity_metadata
    }

    /// Returns whether explicit invalidation is supported.
    #[must_use]
    pub const fn supports_invalidation(self) -> bool {
        self.supports_invalidation
    }

    /// Returns whether raw inline content may be retained by this cache.
    #[must_use]
    pub const fn stores_raw_content(self) -> bool {
        self.stores_raw_content
    }

    /// Returns the retention contract.
    #[must_use]
    pub const fn retention(self) -> CacheRetention {
        self.retention
    }
}

/// One derived and explicitly scoped cache entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheEntry {
    id: ContextCacheEntryId,
    scope: ContextScopeId,
    version: DeclarativeContextVersion,
    batch: ScopedObservationBatch,
}

impl CacheEntry {
    /// Creates a scoped cache entry from one validated ingestion batch.
    pub fn new(
        id: ContextCacheEntryId,
        version: DeclarativeContextVersion,
        batch: ScopedObservationBatch,
    ) -> Result<Self, ContextBoundaryError> {
        version.ensure_supported()?;
        Ok(Self {
            id,
            scope: batch.scope().clone(),
            version,
            batch,
        })
    }

    /// Returns the cache entry identity.
    #[must_use]
    pub fn id(&self) -> &ContextCacheEntryId {
        &self.id
    }

    /// Returns the explicit owning scope.
    #[must_use]
    pub fn scope(&self) -> &ContextScopeId {
        &self.scope
    }

    /// Returns the supported declarative contract version.
    #[must_use]
    pub const fn version(&self) -> DeclarativeContextVersion {
        self.version
    }

    /// Returns source snapshot/change metadata.
    #[must_use]
    pub const fn source_snapshot(&self) -> &SourceSnapshot {
        self.batch.snapshot()
    }

    /// Returns retained lineage records.
    #[must_use]
    pub const fn records(&self) -> &ObservationEvidenceSet {
        self.batch.records()
    }

    /// Returns retained quality metadata, including sensitivity.
    #[must_use]
    pub const fn quality_metadata(&self) -> &BTreeMap<SubjectPath, Vec<QualityMetadata>> {
        self.batch.quality_metadata()
    }
}

/// Neutral in-memory scoped cache proof adapter.
#[derive(Debug)]
pub struct InMemoryContextCache {
    entries: Mutex<CacheMap>,
    capabilities: CacheCapabilities,
}

impl Default for InMemoryContextCache {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryContextCache {
    /// Creates an invalidatable cache that explicitly retains sensitivity metadata.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Mutex::new(BTreeMap::new()),
            capabilities: CacheCapabilities::new(
                true,
                true,
                true,
                CacheRetention::UntilInvalidated,
            ),
        }
    }

    fn entries(&self) -> Result<MutexGuard<'_, CacheMap>, ContextBoundaryError> {
        self.entries
            .lock()
            .map_err(|_| ContextBoundaryError::StorageUnavailable)
    }
}

impl CachePort for InMemoryContextCache {
    type Error = ContextBoundaryError;

    fn capabilities(&self) -> CacheCapabilities {
        self.capabilities
    }

    fn put(&self, entry: CacheEntry) -> Result<IngestionResult, Self::Error> {
        let key = (entry.scope().clone(), entry.id().clone());
        let mut entries = self.entries()?;
        match entries.get(&key) {
            None => {
                entries.insert(key, entry);
                Ok(IngestionResult::Accepted)
            }
            Some(existing) if existing == &entry => Ok(IngestionResult::IdempotentReplay),
            Some(_) => Err(ContextBoundaryError::CacheConflict {
                scope: key.0.to_string(),
                entry: key.1.to_string(),
            }),
        }
    }

    fn get(
        &self,
        scope: &ContextScopeId,
        id: &ContextCacheEntryId,
    ) -> Result<CacheEntry, Self::Error> {
        self.entries()?
            .get(&(scope.clone(), id.clone()))
            .cloned()
            .ok_or_else(|| ContextBoundaryError::CacheMiss {
                scope: scope.to_string(),
                entry: id.to_string(),
            })
    }

    fn invalidate(
        &self,
        scope: &ContextScopeId,
        id: &ContextCacheEntryId,
    ) -> Result<bool, Self::Error> {
        Ok(self
            .entries()?
            .remove(&(scope.clone(), id.clone()))
            .is_some())
    }

    fn clear_scope(&self, scope: &ContextScopeId) -> Result<usize, Self::Error> {
        let mut entries = self.entries()?;
        let before = entries.len();
        entries.retain(|(entry_scope, _), _| entry_scope != scope);
        Ok(before - entries.len())
    }
}

/// Neutral source fixture proving the provider-independent source port.
#[derive(Debug, Clone)]
pub struct SyntheticContextSource {
    batch: ScopedObservationBatch,
}

impl SyntheticContextSource {
    /// Creates a source fixture for one explicit scope and source snapshot.
    #[must_use]
    pub const fn new(batch: ScopedObservationBatch) -> Self {
        Self { batch }
    }
}

impl ScopedContextSource for SyntheticContextSource {
    type Error = ContextBoundaryError;

    fn source_snapshot(&self, scope: &ContextScope) -> Result<SourceSnapshot, Self::Error> {
        if self.batch.scope() != scope.id() {
            return Err(ContextBoundaryError::ScopeMismatch {
                expected: scope.id().to_string(),
                actual: self.batch.scope().to_string(),
            });
        }
        Ok(self.batch.snapshot().clone())
    }

    fn collect(
        &self,
        scope: &ContextScope,
        snapshot: &SourceSnapshot,
    ) -> Result<ScopedObservationBatch, Self::Error> {
        if self.batch.scope() != scope.id() {
            return Err(ContextBoundaryError::ScopeMismatch {
                expected: scope.id().to_string(),
                actual: self.batch.scope().to_string(),
            });
        }
        if self.batch.snapshot() != snapshot {
            return Err(ContextBoundaryError::ConflictingRetry {
                scope: scope.id().to_string(),
            });
        }
        Ok(self.batch.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use gateway_domain::{
        AssertionPolarity, AssessmentRuleId, Confidence, ContentDigest, DeclarativeContextVersion,
        Evidence, EvidenceContent, EvidenceId, EvidenceKind, EvidenceLink, EvidenceRelation, Fact,
        FactId, FreshnessStatus, IntentId, NonEmptyText, Observation, ObservationId, Provenance,
        ProvenanceId, SourceId, TaskConfidence, TrustClass, Uncertainty, ValidationError,
    };

    use super::*;
    use crate::ports::{
        inbound::{DeclarativeIntentInputPort, ObservationEvidenceInputPort, ScopeLifecyclePort},
        outbound::{CachePort, ScopedContextSource},
    };

    fn empty_records() -> ObservationEvidenceSet {
        ObservationEvidenceSet::new(vec![], vec![], vec![], vec![]).unwrap()
    }

    fn non_empty_records() -> ObservationEvidenceSet {
        let provenance = Provenance::new(
            ProvenanceId::new("prov-1").unwrap(),
            SourceKind::Repository,
            SourceId::new("source-1").unwrap(),
            "repository://snapshot",
        )
        .unwrap();
        let observation = Observation::new(
            ObservationId::new("observation-1").unwrap(),
            SubjectPath::from_str("repository.state").unwrap(),
            gateway_domain::TypedValue::Boolean(true),
            provenance.id().clone(),
        )
        .unwrap();
        let fact = Fact::new(
            FactId::new("fact-1").unwrap(),
            SubjectPath::from_str("repository.state").unwrap(),
            gateway_domain::TypedValue::Boolean(true),
            AssertionPolarity::Affirmed,
            vec![observation.id().clone()],
        )
        .unwrap();
        let evidence = Evidence::new(
            EvidenceId::new("evidence-1").unwrap(),
            EvidenceKind::Report,
            "repository state report",
            EvidenceContent::inline("state is present").unwrap(),
            provenance.id().clone(),
            vec![EvidenceLink::new(
                fact.id().clone(),
                EvidenceRelation::Supports,
            )],
        )
        .unwrap();
        ObservationEvidenceSet::new(
            vec![provenance],
            vec![observation],
            vec![fact],
            vec![evidence],
        )
        .unwrap()
    }

    fn batch(scope: &ContextScopeId, token: &str) -> ScopedObservationBatch {
        let snapshot = SourceSnapshot::new(
            SourceId::new("source-1").unwrap(),
            SourceKind::Repository,
            Some(SourceTimestamp::new(token).unwrap()),
            None,
        )
        .unwrap();
        ScopedObservationBatch::new(scope.clone(), snapshot, empty_records()).unwrap()
    }

    fn quality() -> QualityMetadata {
        QualityMetadata::new(
            TrustClass::ObservedEvidence,
            gateway_domain::SensitivityClass::Secret,
            Confidence::score(0.8).unwrap(),
            FreshnessStatus::Fresh,
            Uncertainty::None,
        )
    }

    fn intent() -> Intent {
        let condition = gateway_domain::DesiredCondition::new(
            gateway_domain::ConditionId::new("condition-1").unwrap(),
            SubjectPath::from_str("repository.state").unwrap(),
            gateway_domain::ComparisonOperator::Present,
            None,
        )
        .unwrap();
        let desired = gateway_domain::DesiredState::new(
            gateway_domain::DesiredStateId::new("desired-1").unwrap(),
            vec![condition],
            gateway_domain::ConditionExpression::all(vec![
                gateway_domain::ConditionExpression::condition(
                    gateway_domain::ConditionId::new("condition-1").unwrap(),
                ),
            ])
            .unwrap(),
            vec![],
            vec![],
        )
        .unwrap();
        Intent::new(IntentId::new("intent-1").unwrap(), desired)
    }

    #[test]
    fn scope_lifecycle_is_explicit_and_no_profile_is_required() {
        assert_eq!(ScopeLifecycle::Open.as_str(), "OPEN");
        assert!(ScopeLifecycle::Open.accepts_input());
        assert!(!ScopeLifecycle::Sealed.accepts_input());
        assert_eq!(ScopeLifecycle::Closed.to_string(), "CLOSED");
        let scope = ContextScope::new(ContextScopeId::new("scope-a").unwrap());
        assert_eq!(scope.id().as_str(), "scope-a");
        assert_eq!(scope.lifecycle(), ScopeLifecycle::Open);
        assert!(
            SourceSnapshot::new(
                SourceId::new("source").unwrap(),
                SourceKind::Repository,
                None,
                None,
            )
            .is_err()
        );
    }

    #[test]
    fn scopes_are_isolated_and_retries_are_idempotent() {
        let store = InMemoryContextStore::new();
        let first = store
            .open_scope(ContextScopeId::new("scope-a").unwrap())
            .unwrap();
        let second = store
            .open_scope(ContextScopeId::new("scope-b").unwrap())
            .unwrap();
        assert!(
            store
                .open_scope(ContextScopeId::new("scope-a").unwrap())
                .is_err()
        );
        let first_batch = batch(first.id(), "revision-1").with_quality_metadata(
            SubjectPath::from_str("secret.value").unwrap(),
            vec![quality(), quality()],
        );
        assert_eq!(
            first_batch
                .quality_metadata()
                .values()
                .next()
                .unwrap()
                .len(),
            1
        );
        let receipt = store
            .ingest_observations(&first, first_batch.clone())
            .unwrap();
        assert_eq!(receipt.result(), IngestionResult::Accepted);
        assert_eq!(
            store
                .ingest_observations(&first, first_batch.clone())
                .unwrap()
                .result(),
            IngestionResult::IdempotentReplay
        );
        let isolated = batch(second.id(), "revision-1");
        assert!(store.ingest_observations(&first, isolated).is_err());
        let snapshot = store.snapshot(&first).unwrap();
        assert_eq!(snapshot.scope().id(), first.id());
        assert!(snapshot.intent().is_none());
        assert_eq!(snapshot.batches().len(), 1);
        assert_eq!(
            snapshot.batches()[0]
                .quality_metadata()
                .values()
                .next()
                .unwrap()[0]
                .sensitivity(),
            gateway_domain::SensitivityClass::Secret
        );
        assert!(receipt.key().as_str().contains("source-1"));
        assert!(!receipt.key().as_str().contains("secret-value"));
    }

    #[test]
    fn intent_and_lifecycle_boundaries_reject_conflicting_or_stale_input() {
        let store = InMemoryContextStore::new();
        let scope = store
            .open_scope(ContextScopeId::new("scope-lifecycle").unwrap())
            .unwrap();
        assert_eq!(
            store.submit_intent(&scope, intent()).unwrap(),
            IngestionResult::Accepted
        );
        assert_eq!(
            store.submit_intent(&scope, intent()).unwrap(),
            IngestionResult::IdempotentReplay
        );
        let sealed = store.seal_scope(&scope).unwrap();
        assert_eq!(sealed.lifecycle(), ScopeLifecycle::Sealed);
        assert!(store.seal_scope(&sealed).is_err());
        assert!(store.submit_intent(&sealed, intent()).is_err());
        assert!(
            store
                .ingest_observations(&sealed, batch(sealed.id(), "revision-1"))
                .is_err()
        );
        assert!(store.snapshot(&scope).is_err());
        let closed = store.close_scope(&sealed).unwrap();
        assert_eq!(closed.lifecycle(), ScopeLifecycle::Closed);
        assert!(store.scope(closed.id()).unwrap().is_none());
        assert!(store.snapshot(&sealed).is_err());
    }

    #[test]
    fn cache_is_scoped_invalidatable_and_preserves_metadata() {
        let scope_a = ContextScopeId::new("scope-a").unwrap();
        let scope_b = ContextScopeId::new("scope-b").unwrap();
        let entry = CacheEntry::new(
            ContextCacheEntryId::new("cache-a").unwrap(),
            DeclarativeContextVersion::V1,
            ScopedObservationBatch::new(
                scope_a.clone(),
                SourceSnapshot::new(
                    SourceId::new("source-1").unwrap(),
                    SourceKind::Repository,
                    Some(SourceTimestamp::new("digest-1").unwrap()),
                    None,
                )
                .unwrap(),
                non_empty_records(),
            )
            .unwrap()
            .with_quality_metadata(
                SubjectPath::from_str("secret.value").unwrap(),
                vec![quality()],
            ),
        )
        .unwrap();
        let cache = InMemoryContextCache::new();
        assert!(cache.capabilities().retains_sensitivity_metadata());
        assert!(cache.capabilities().supports_invalidation());
        assert!(cache.capabilities().stores_raw_content());
        assert_eq!(
            cache.capabilities().retention(),
            CacheRetention::UntilInvalidated
        );
        assert_eq!(cache.put(entry.clone()).unwrap(), IngestionResult::Accepted);
        assert_eq!(
            cache.put(entry.clone()).unwrap(),
            IngestionResult::IdempotentReplay
        );
        assert_eq!(cache.get(&scope_a, entry.id()).unwrap(), entry);
        assert_eq!(entry.version(), DeclarativeContextVersion::V1);
        assert_eq!(entry.source_snapshot().source_id().as_str(), "source-1");
        assert_eq!(entry.records().facts().len(), 1);
        assert_eq!(entry.quality_metadata().len(), 1);
        assert!(cache.get(&scope_b, entry.id()).is_err());
        assert_eq!(cache.clear_scope(&scope_b).unwrap(), 0);
        assert!(cache.invalidate(&scope_a, entry.id()).unwrap());
        assert!(!cache.invalidate(&scope_a, entry.id()).unwrap());
        assert!(cache.get(&scope_a, entry.id()).is_err());
    }

    #[test]
    fn cache_rejects_unsupported_versions_and_conflicts_without_raw_data() {
        let scope = ContextScopeId::new("scope-cache").unwrap();
        let source_batch = batch(&scope, "revision-1");
        let cache = InMemoryContextCache::new();
        let first = CacheEntry::new(
            ContextCacheEntryId::new("cache-1").unwrap(),
            DeclarativeContextVersion::V1,
            source_batch.clone(),
        )
        .unwrap();
        cache.put(first.clone()).unwrap();
        let conflicting_batch = ScopedObservationBatch::new(
            scope.clone(),
            SourceSnapshot::new(
                SourceId::new("source-1").unwrap(),
                SourceKind::Repository,
                Some(SourceTimestamp::new("revision-2").unwrap()),
                None,
            )
            .unwrap(),
            empty_records(),
        )
        .unwrap();
        let conflicting = CacheEntry::new(
            ContextCacheEntryId::new("cache-1").unwrap(),
            DeclarativeContextVersion::V1,
            conflicting_batch,
        )
        .unwrap();
        assert!(cache.put(conflicting).is_err());
        let future = DeclarativeContextVersion::new(2, 0).unwrap();
        assert!(
            CacheEntry::new(
                ContextCacheEntryId::new("future").unwrap(),
                future,
                source_batch,
            )
            .is_err()
        );
    }

    #[test]
    fn synthetic_source_preserves_scope_and_snapshot_contract() {
        let scope = ContextScope::new(ContextScopeId::new("scope-source").unwrap());
        let source = SyntheticContextSource::new(batch(scope.id(), "revision-1"));
        let snapshot = source.source_snapshot(&scope).unwrap();
        assert_eq!(snapshot.source_kind(), SourceKind::Repository);
        assert_eq!(snapshot.source_id().as_str(), "source-1");
        assert_eq!(snapshot.change_token().unwrap().as_str(), "revision-1");
        assert!(snapshot.digest().is_none());
        assert_eq!(
            source.collect(&scope, &snapshot).unwrap().scope(),
            scope.id()
        );
        let wrong_scope = ContextScope::new(ContextScopeId::new("scope-other").unwrap());
        assert!(source.source_snapshot(&wrong_scope).is_err());
        let wrong_snapshot = SourceSnapshot::new(
            SourceId::new("source-1").unwrap(),
            SourceKind::Repository,
            Some(SourceTimestamp::new("revision-2").unwrap()),
            None,
        )
        .unwrap();
        assert!(source.collect(&scope, &wrong_snapshot).is_err());
    }

    #[test]
    fn source_metadata_and_ids_are_explicit_without_provider_specific_types() {
        let digest = ContentDigest::new("b".repeat(64)).unwrap();
        let snapshot = SourceSnapshot::new(
            SourceId::new("git-source").unwrap(),
            SourceKind::Git,
            None,
            Some(digest.clone()),
        )
        .unwrap();
        assert_eq!(snapshot.digest(), Some(&digest));
        assert_eq!(snapshot.change_token(), None);
        let scope = ContextScopeId::new("scope-digest").unwrap();
        let scoped = ScopedObservationBatch::new(scope, snapshot, non_empty_records()).unwrap();
        assert!(scoped.ingestion_key().as_str().contains("git-source"));
        assert!(AssessmentRuleId::new("rule").is_ok());
        let _ = TaskConfidence::new(0.5).unwrap();
        let _ = NonEmptyText::new("opaque").unwrap();
    }

    #[test]
    fn error_projection_is_non_sensitive_and_covers_boundary_failures() {
        let errors = [
            ContextBoundaryError::Domain(ValidationError::EmptyText { field: "input" }),
            ContextBoundaryError::ScopeAlreadyExists {
                scope: "scope".to_owned(),
            },
            ContextBoundaryError::ScopeNotFound {
                scope: "scope".to_owned(),
            },
            ContextBoundaryError::ScopeNotOpen {
                scope: "scope".to_owned(),
            },
            ContextBoundaryError::StaleScopeHandle {
                scope: "scope".to_owned(),
            },
            ContextBoundaryError::ScopeMismatch {
                expected: "scope-a".to_owned(),
                actual: "scope-b".to_owned(),
            },
            ContextBoundaryError::ConflictingRetry {
                scope: "scope".to_owned(),
            },
            ContextBoundaryError::CacheConflict {
                scope: "scope".to_owned(),
                entry: "entry".to_owned(),
            },
            ContextBoundaryError::CacheMiss {
                scope: "scope".to_owned(),
                entry: "entry".to_owned(),
            },
            ContextBoundaryError::StorageUnavailable,
        ];
        for error in errors {
            let rendered = error.to_string();
            assert!(!rendered.contains("secret-value"));
            assert!(!rendered.is_empty());
        }
    }
}
