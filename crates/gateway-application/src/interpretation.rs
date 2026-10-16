//! Deterministic EPIC-05.09 resolver. No clock, model, retrieval or session owner.
use gateway_domain::{interpretation::*, *};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterpretationError {
    Admission,
    Scope,
    Expired,
    InvalidCapture,
    ChangedBasis,
    InvalidQuery,
}

/// Trusted host boundary. Implementations verify source lineage, origin tier,
/// session/task binding, corrections, memory eligibility and calendar projections.
/// Validation must inspect exact pins; replacing them with latest is forbidden.
pub trait InterpretationHost {
    fn authenticate(&self, binding: &InterpretationBinding) -> Result<(), InterpretationError>;
    fn validate_basis(&self, pin: &InterpretationPin, at: UnixTimestamp) -> bool;
    fn validate_fact(
        &self,
        fact: &InterpretationFact,
        at: UnixTimestamp,
    ) -> Result<(), SemanticGapReason>;
    /// Verify the captured projections and source permissions against host-owned
    /// records, including alias membership, corrections and civil-time boundaries.
    fn validate_projection(&self, context: &InterpretationContextData) -> bool;
    fn validate_query(&self, binding: &InterpretationBinding, query: &InterpretationQuery) -> bool;
}

/// Registry and project adapters implement the same bounded, exact lookup contract.
/// The resolver passes the admitted scope and source; adapters cannot widen either.
pub trait InterpretationLookup {
    fn lookup(
        &self,
        scope: &ContextScopeId,
        source: &InterpretationSource,
        query: &EntityQuery,
    ) -> Vec<InterpretationFact>;
}

/// Deterministic adapter for host-captured registry/project lookup results.
/// Failed/unavailable acquisition is represented by no entries, never a guess.
#[derive(Debug, Clone, Default)]
pub struct CapturedInterpretationLookup {
    entries: Vec<CapturedLookupEntry>,
}

#[derive(Debug, Clone)]
pub struct CapturedLookupEntry {
    pub scope: ContextScopeId,
    pub source: InterpretationSource,
    pub query: EntityQuery,
    pub facts: Vec<InterpretationFact>,
}

impl CapturedInterpretationLookup {
    pub fn new(entries: Vec<CapturedLookupEntry>) -> Self {
        Self { entries }
    }
}

impl InterpretationLookup for CapturedInterpretationLookup {
    fn lookup(
        &self,
        scope: &ContextScopeId,
        source: &InterpretationSource,
        query: &EntityQuery,
    ) -> Vec<InterpretationFact> {
        self.entries
            .iter()
            .filter(|e| &e.scope == scope && &e.source == source && &e.query == query)
            .flat_map(|e| e.facts.clone())
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum EntityQuery {
    Canonical {
        id: ReferenceId,
        kind: TargetKind,
    },
    Alias {
        label: NonEmptyText,
        kind: TargetKind,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ReferenceExpression {
    Entity(EntityQuery),
    /// A typed pronoun/discourse reference to a named field, not raw chat history.
    Focus,
    RelativeSeconds {
        anchor: ReferenceId,
        seconds: i64,
    },
    Yesterday {
        anchor: ReferenceId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ExpectedMeaning {
    Target(TargetKind),
    Reference(ReferenceId),
    Literal,
    Temporal,
}

impl ExpectedMeaning {
    fn accepts(&self, value: &InterpretationValue) -> bool {
        match (self, value) {
            (Self::Target(kind), InterpretationValue::Target(target)) => *kind == target.kind,
            (Self::Reference(contract), InterpretationValue::Reference(pin)) => {
                *contract == pin.contract
            }
            (Self::Literal, InterpretationValue::Literal(_)) => true,
            (
                Self::Temporal,
                InterpretationValue::Instant(_) | InterpretationValue::Interval { .. },
            ) => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InterpretationQuery {
    pub field: SubjectPath,
    pub expected: ExpectedMeaning,
    pub expression: ReferenceExpression,
    pub provenance: Provenance,
    pub span: (usize, usize),
    pub subject: Option<SemanticTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InterpretationTrace {
    pub fact: ReferenceId,
    pub tier: InterpretationTier,
    pub rule: &'static str,
    pub reason: Option<SemanticGapReason>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterpretationDecision {
    pub context: ReferenceId,
    pub revision: ReferenceId,
    pub request: ReferenceId,
    pub query: InterpretationQuery,
    pub field: SubjectPath,
    pub span: (usize, usize),
    pub capture_digest: String,
    pub evaluated_at: UnixTimestamp,
    pub selected_tier: Option<InterpretationTier>,
    pub resolution: SemanticResolution<InterpretationValue>,
    pub trace: Vec<InterpretationTrace>,
    /// Exact lookup results retained for replay, including excluded same-scope facts.
    pub lookup_facts: Vec<InterpretationFact>,
}

impl InterpretationDecision {
    /// Versioned inspection envelope, not executable SemanticTaskIR bytes.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let candidate = |c: &ResolutionCandidate<InterpretationValue>| serde_json::json!({"value": c.value(), "provenance": c.provenance()});
        let (candidates, reason, provenance) = match &self.resolution {
            SemanticResolution::Resolved(c) => (vec![candidate(c)], None, vec![]),
            SemanticResolution::Ambiguous(cs) | SemanticResolution::Conflicting(cs) => (
                cs.candidates().iter().map(candidate).collect(),
                None,
                vec![],
            ),
            SemanticResolution::Unresolved(f) | SemanticResolution::Invalid(f) => {
                (vec![], Some(f.reason()), f.provenance().to_vec())
            }
        };
        bytes(
            &serde_json::json!({"version": "1.0", "context": self.context, "revision": self.revision,
            "request": self.request, "query": self.query, "capture_digest": self.capture_digest,
            "evaluated_at": self.evaluated_at, "selected_tier": self.selected_tier,
            "state": self.resolution.state(), "candidates": candidates, "reason": reason,
            "provenance": provenance, "trace": self.trace, "lookup_facts": self.lookup_facts}),
        )
    }
}

/// Immutable admitted snapshot. Deserialization always goes through `admit_json`.
#[derive(Debug, Clone)]
pub struct InterpretationContext {
    data: InterpretationContextData,
    canonical: Vec<u8>,
    digest: String,
}

fn bytes(value: &impl Serialize) -> Vec<u8> {
    serde_json::to_vec(value).expect("interpretation records have JSON-compatible keys")
}

fn pin_valid(pin: &InterpretationPin, scope: &ContextScopeId) -> Result<(), InterpretationError> {
    if &pin.scope != scope {
        return Err(InterpretationError::Scope);
    }
    let digest = pin.digest.as_str();
    if pin.contract_version != SchemaVersion::V1
        || digest.len() != 64
        || !digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(InterpretationError::InvalidCapture);
    }
    Ok(())
}

fn value_valid(
    value: &InterpretationValue,
    scope: &ContextScopeId,
) -> Result<(), InterpretationError> {
    match value {
        InterpretationValue::Target(target) => pin_valid(&target.reference, scope),
        InterpretationValue::Reference(pin) => pin_valid(pin, scope),
        InterpretationValue::Literal(value) => value
            .validate()
            .map_err(|_| InterpretationError::InvalidCapture),
        InterpretationValue::Interval { start, end } if start >= end => {
            Err(InterpretationError::InvalidCapture)
        }
        _ => Ok(()),
    }
}

fn fact_valid(
    fact: &InterpretationFact,
    data: &InterpretationContextData,
) -> Result<(), InterpretationError> {
    pin_valid(&fact.source, &data.binding.scope)?;
    value_valid(&fact.value, &data.binding.scope)?;
    if let Some(subject) = &fact.subject {
        pin_valid(&subject.reference, &data.binding.scope)?;
    }
    if !data.basis_refs.contains(&fact.source)
        || fact.span.is_some_and(|(start, end)| start >= end)
        || (fact.tier == InterpretationTier::CurrentInput
            && (fact.source != data.binding.input || fact.span.is_none()))
        || (fact.tier == InterpretationTier::TaskState && data.binding.task.is_none())
        || (fact.tier == InterpretationTier::ConversationFocus
            && data.binding.conversation.is_none())
    {
        return Err(InterpretationError::InvalidCapture);
    }
    Ok(())
}

fn canonical_set<T: Serialize>(values: &mut Vec<T>) {
    values.sort_by_key(bytes);
    values.dedup_by(|a, b| bytes(a) == bytes(b));
}

fn canonical_fact(fact: &mut InterpretationFact) -> Result<(), InterpretationError> {
    if let InterpretationValue::Literal(TypedValue::Set(values)) = &mut fact.value {
        values.sort();
        if values.windows(2).any(|p| p[0] == p[1]) {
            return Err(InterpretationError::InvalidCapture);
        }
    }
    Ok(())
}

impl InterpretationContext {
    pub fn admit_json(
        json: &[u8],
        host: &impl InterpretationHost,
        at: UnixTimestamp,
    ) -> Result<Self, InterpretationError> {
        let data = serde_json::from_slice(json).map_err(|_| InterpretationError::InvalidCapture)?;
        Self::admit(data, host, at)
    }

    pub fn admit(
        mut data: InterpretationContextData,
        host: &impl InterpretationHost,
        at: UnixTimestamp,
    ) -> Result<Self, InterpretationError> {
        host.authenticate(&data.binding)?;
        if data.version != SchemaVersion::V1 || data.captured_at >= data.expires_at {
            return Err(InterpretationError::InvalidCapture);
        }
        if at < data.captured_at || at >= data.expires_at {
            return Err(InterpretationError::Expired);
        }
        let mut required = vec![data.binding.input.clone(), data.binding.host_basis.clone()];
        required.extend(data.binding.conversation.clone());
        if let Some(task) = &data.binding.task {
            required.push(InterpretationPin {
                id: ReferenceId::new(task.id.as_str())
                    .map_err(|_| InterpretationError::InvalidCapture)?,
                scope: task.scope.clone(),
                contract: task.contract.clone(),
                contract_version: task.contract_version,
                revision: task.revision.clone(),
                digest: task.digest.clone(),
            });
        }
        if required.iter().any(|pin| !data.basis_refs.contains(pin)) {
            return Err(InterpretationError::InvalidCapture);
        }
        let mut identities = BTreeMap::new();
        for pin in &data.basis_refs {
            pin_valid(pin, &data.binding.scope)?;
            let key = (&pin.id, &pin.contract, &pin.revision);
            if identities.insert(key, pin).is_some_and(|old| old != pin) {
                return Err(InterpretationError::InvalidCapture);
            }
        }
        let mut sources = BTreeMap::new();
        for source in &data.permitted_sources {
            pin_valid(&source.source, &data.binding.scope)?;
            if source.max_results == 0
                || !data.basis_refs.contains(&source.source)
                || sources
                    .insert((&source.source.id, source.tier), source)
                    .is_some_and(|old| old != source)
            {
                return Err(InterpretationError::InvalidCapture);
            }
        }
        for fact in data
            .relevant_facts
            .iter_mut()
            .chain(data.aliases.iter_mut().map(|a| &mut a.fact))
            .chain(data.temporal_anchors.iter_mut().map(|a| &mut a.fact))
        {
            canonical_fact(fact)?;
        }
        let facts: Vec<_> = data
            .relevant_facts
            .iter()
            .chain(data.aliases.iter().map(|a| &a.fact))
            .chain(data.temporal_anchors.iter().map(|a| &a.fact))
            .collect();
        let mut ids = BTreeMap::new();
        let mut provenance = BTreeMap::new();
        for fact in &facts {
            fact_valid(fact, &data)?;
            if ids.insert(&fact.id, *fact).is_some_and(|old| old != *fact)
                || provenance
                    .insert(fact.provenance.id(), &fact.provenance)
                    .is_some_and(|old| old != &fact.provenance)
            {
                return Err(InterpretationError::InvalidCapture);
            }
        }
        for fact in &facts {
            if let Some(previous) = &fact.supersedes {
                let old = ids
                    .get(previous)
                    .ok_or(InterpretationError::InvalidCapture)?;
                if old.field != fact.field
                    || old.tier != fact.tier
                    || old.id == fact.id
                    || old.protected
                {
                    return Err(InterpretationError::InvalidCapture);
                }
                let mut visited = BTreeSet::new();
                let mut current = *fact;
                while let Some(id) = &current.supersedes {
                    if !visited.insert(id) {
                        return Err(InterpretationError::InvalidCapture);
                    }
                    current = ids.get(id).ok_or(InterpretationError::InvalidCapture)?;
                }
            }
        }
        for alias in &data.aliases {
            if !matches!(&alias.fact.value, InterpretationValue::Target(t) if t.kind == alias.kind)
            {
                return Err(InterpretationError::InvalidCapture);
            }
        }
        for anchor in &data.temporal_anchors {
            if !matches!(anchor.fact.value, InterpretationValue::Instant(_))
                || anchor.yesterday.is_some_and(|(start, end)| start >= end)
            {
                return Err(InterpretationError::InvalidCapture);
            }
        }
        if !host.validate_projection(&data) {
            return Err(InterpretationError::Admission);
        }
        canonical_set(&mut data.basis_refs);
        canonical_set(&mut data.permitted_sources);
        canonical_set(&mut data.relevant_facts);
        canonical_set(&mut data.aliases);
        canonical_set(&mut data.temporal_anchors);
        let canonical = bytes(&data);
        let digest = format!("{:x}", Sha256::digest(&canonical));
        let context = Self {
            data,
            canonical,
            digest,
        };
        context.revalidate(host, at)?;
        Ok(context)
    }

    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }

    pub fn revalidate(
        &self,
        host: &impl InterpretationHost,
        at: UnixTimestamp,
    ) -> Result<(), InterpretationError> {
        host.authenticate(&self.data.binding)?;
        if at < self.data.captured_at || at >= self.data.expires_at {
            return Err(InterpretationError::Expired);
        }
        if self
            .data
            .basis_refs
            .iter()
            .any(|pin| !host.validate_basis(pin, at))
        {
            return Err(InterpretationError::ChangedBasis);
        }
        Ok(())
    }
}

fn failure(
    reason: SemanticGapReason,
    provenance: Vec<Provenance>,
) -> SemanticResolution<InterpretationValue> {
    let value = ResolutionFailure::new(reason, provenance)
        .expect("single failure reason and validated lineage");
    if reason.resolution_state() == SemanticResolutionState::Invalid {
        SemanticResolution::Invalid(value)
    } else {
        SemanticResolution::Unresolved(value)
    }
}

pub struct DeterministicInterpretationResolver<'a, H, R, P> {
    pub host: &'a H,
    pub registry: &'a R,
    pub project: &'a P,
}

impl<H: InterpretationHost, R: InterpretationLookup, P: InterpretationLookup>
    DeterministicInterpretationResolver<'_, H, R, P>
{
    pub fn resolve(
        &self,
        context: &InterpretationContext,
        query: &InterpretationQuery,
        at: UnixTimestamp,
    ) -> Result<InterpretationDecision, InterpretationError> {
        context.revalidate(self.host, at)?;
        if query.span.0 >= query.span.1 || !self.host.validate_query(&context.data.binding, query) {
            return Err(InterpretationError::InvalidQuery);
        }
        if let Some(subject) = &query.subject {
            pin_valid(&subject.reference, &context.data.binding.scope)?;
        }
        let data = &context.data;
        let mut facts = Vec::new();
        let mut lookup_facts = Vec::new();
        let mut temporal_missing = false;
        match &query.expression {
            ReferenceExpression::Focus => facts.extend(
                data.relevant_facts
                    .iter()
                    .filter(|f| f.field == query.field)
                    .cloned(),
            ),
            ReferenceExpression::Entity(entity) => {
                if let EntityQuery::Alias { label, kind } = entity {
                    facts.extend(
                        data.aliases
                            .iter()
                            .filter(|a| &a.label == label && &a.kind == kind)
                            .map(|a| a.fact.clone()),
                    );
                }
                for source in &data.permitted_sources {
                    let mut found = self.registry.lookup(&data.binding.scope, source, entity);
                    found.extend(self.project.lookup(&data.binding.scope, source, entity));
                    canonical_set(&mut found);
                    if found.len() > source.max_results {
                        return Err(InterpretationError::InvalidCapture);
                    }
                    for fact in &found {
                        fact_valid(fact, data)?;
                        if fact.source != source.source || fact.tier != source.tier {
                            return Err(InterpretationError::InvalidCapture);
                        }
                        let InterpretationValue::Target(target) = &fact.value else {
                            return Err(InterpretationError::InvalidCapture);
                        };
                        let matches = match entity {
                            EntityQuery::Canonical { id, kind } => {
                                &target.reference.id == id && &target.kind == kind
                            }
                            EntityQuery::Alias { kind, .. } => &target.kind == kind,
                        };
                        if !matches {
                            return Err(InterpretationError::InvalidCapture);
                        }
                    }
                    lookup_facts.extend(found.clone());
                    facts.extend(found);
                }
            }
            ReferenceExpression::RelativeSeconds { anchor, .. }
            | ReferenceExpression::Yesterday { anchor } => {
                for value in data.temporal_anchors.iter().filter(|a| &a.name == anchor) {
                    let mut fact = value.fact.clone();
                    let InterpretationValue::Instant(instant) = fact.value else {
                        unreachable!("admitted instant")
                    };
                    let resolved = match &query.expression {
                        ReferenceExpression::RelativeSeconds { seconds, .. } => instant
                            .seconds()
                            .checked_add(*seconds)
                            .map(|s| InterpretationValue::Instant(UnixTimestamp::new(s))),
                        _ => value
                            .yesterday
                            .filter(|_| {
                                value.timezone.is_some()
                                    && value
                                        .calendar
                                        .as_ref()
                                        .is_some_and(|c| c.as_str() == "Gregorian")
                            })
                            .map(|(start, end)| InterpretationValue::Interval { start, end }),
                    };
                    if let Some(resolved) = resolved {
                        fact.value = resolved;
                        facts.push(fact);
                    } else {
                        temporal_missing = true;
                    }
                }
            }
        }
        let explicit_ids: BTreeSet<_> = facts.iter().map(|f| f.id.clone()).collect();
        if !matches!(query.expression, ReferenceExpression::Focus) {
            facts.extend(
                data.relevant_facts
                    .iter()
                    .filter(|f| f.field == query.field && f.protected)
                    .cloned(),
            );
        }
        canonical_set(&mut facts);
        canonical_set(&mut lookup_facts);
        let mut trace = Vec::new();
        let mut eligible = Vec::new();
        let mut explicit_failure = None;
        let mut stale = false;
        // Validate duplicate adapter identities and lineage before merging candidates.
        let mut ids = BTreeMap::new();
        let mut lineage = BTreeMap::new();
        for fact in &facts {
            if ids.insert(&fact.id, fact).is_some_and(|old| old != fact)
                || lineage
                    .insert(fact.provenance.id(), &fact.provenance)
                    .is_some_and(|old| old != &fact.provenance)
                || (fact.provenance.id() == query.provenance.id()
                    && fact.provenance != query.provenance)
            {
                return Err(InterpretationError::InvalidCapture);
            }
            let permission = data.permitted_sources.iter().any(|s| {
                s.source == fact.source
                    && s.tier == fact.tier
                    && fact.quality.sensitivity() <= s.sensitivity_ceiling
                    && fact.quality.trust() == s.trust
            });
            let original = data
                .temporal_anchors
                .iter()
                .find(|a| a.fact.id == fact.id)
                .map_or(fact, |a| &a.fact);
            let wrong_subject = fact.subject.is_some() && fact.subject != query.subject;
            let reason = if !query.expected.accepts(&fact.value)
                || wrong_subject
                || fact.tier >= InterpretationTier::VerifiedMemory && !permission
            {
                Some(SemanticGapReason::InvalidReference)
            } else if at >= fact.valid_until || fact.quality.freshness() != FreshnessStatus::Fresh {
                Some(SemanticGapReason::StaleReference)
            } else if fact.quality.conflict() == ConflictStatus::Unresolved {
                Some(SemanticGapReason::ConflictingContext)
            } else {
                self.host.validate_fact(original, at).err()
            };
            if let Some(reason) = reason {
                stale |= reason == SemanticGapReason::StaleReference;
                if fact.required_pin
                    || fact.tier == InterpretationTier::CurrentInput
                    || (!wrong_subject
                        && (fact.protected || fact.tier == InterpretationTier::TaskState))
                {
                    explicit_failure = Some(SemanticGapReason::InvalidReference);
                }
                trace.push(InterpretationTrace {
                    fact: fact.id.clone(),
                    tier: fact.tier,
                    rule: if wrong_subject {
                        "target-invalidated"
                    } else {
                        "excluded"
                    },
                    reason: Some(reason),
                });
            } else {
                eligible.push(fact.clone());
            }
        }
        let superseded: BTreeSet<_> = eligible
            .iter()
            .filter_map(|f| f.supersedes.clone())
            .collect();
        eligible.retain(|fact| {
            if superseded.contains(&fact.id) {
                trace.push(InterpretationTrace {
                    fact: fact.id.clone(),
                    tier: fact.tier,
                    rule: "explicit-correction",
                    reason: None,
                });
                false
            } else {
                true
            }
        });
        let explicit = !matches!(query.expression, ReferenceExpression::Focus);
        let selected_tier = if explicit {
            Some(InterpretationTier::CurrentInput)
        } else {
            eligible.iter().map(|f| f.tier).min()
        };
        let tier = eligible.iter().map(|f| f.tier).min();
        // Explicit entity/temporal expressions consume all exact lookup candidates.
        let winners: Vec<_> = eligible
            .iter()
            .filter(|f| {
                if explicit {
                    explicit_ids.contains(&f.id)
                } else {
                    Some(f.tier) == tier
                }
            })
            .collect();
        let mut meanings: BTreeMap<
            Vec<u8>,
            (InterpretationValue, BTreeMap<ProvenanceId, Provenance>),
        > = BTreeMap::new();
        let mut conflict = false;
        for fact in &eligible {
            let winning = winners.contains(&fact);
            let protected_conflict =
                fact.protected && winners.iter().any(|w| w.value != fact.value);
            conflict |= protected_conflict;
            trace.push(InterpretationTrace {
                fact: fact.id.clone(),
                tier: fact.tier,
                rule: if protected_conflict {
                    "protected-conflict"
                } else if winning {
                    match query.expression {
                        ReferenceExpression::Entity(EntityQuery::Canonical { .. }) => {
                            "canonical-id"
                        }
                        ReferenceExpression::Entity(EntityQuery::Alias { .. }) => {
                            "registered-alias"
                        }
                        ReferenceExpression::RelativeSeconds { .. } => "anchored-duration",
                        ReferenceExpression::Yesterday { .. } => "anchored-civil-interval",
                        ReferenceExpression::Focus => "selected-tier",
                    }
                } else {
                    "superseded-tier"
                },
                reason: None,
            });
            if winning || protected_conflict {
                let entry = meanings
                    .entry(bytes(&fact.value))
                    .or_insert_with(|| (fact.value.clone(), BTreeMap::new()));
                entry
                    .1
                    .insert(fact.provenance.id().clone(), fact.provenance.clone());
                if explicit {
                    entry
                        .1
                        .insert(query.provenance.id().clone(), query.provenance.clone());
                }
            }
        }
        // Same entity at different exact revisions is a conflict, not two referents.
        for (index, a) in winners.iter().enumerate() {
            for b in winners.iter().skip(index + 1) {
                conflict |= a.value != b.value
                    && (a.tier <= InterpretationTier::Situation && !explicit
                        || matches!((&a.value, &b.value), (InterpretationValue::Target(x), InterpretationValue::Target(y)) if x.reference.id == y.reference.id)
                        || matches!((&a.value, &b.value), (InterpretationValue::Reference(x), InterpretationValue::Reference(y)) if x.id == y.id));
            }
        }
        let candidates: Vec<_> = meanings
            .into_values()
            .map(|(value, lineage)| {
                ResolutionCandidate::new(value, lineage.into_values().collect())
                    .expect("nonempty unique provenance")
            })
            .collect();
        let resolution = if let Some(reason) = explicit_failure {
            failure(reason, vec![query.provenance.clone()])
        } else if temporal_missing {
            failure(
                SemanticGapReason::MissingRequiredField,
                vec![query.provenance.clone()],
            )
        } else if candidates.is_empty() {
            let reason = match query.expression {
                ReferenceExpression::Entity(EntityQuery::Canonical { .. }) => {
                    SemanticGapReason::InvalidReference
                }
                ReferenceExpression::Entity(EntityQuery::Alias { .. }) => {
                    SemanticGapReason::UnknownEntity
                }
                _ if stale => SemanticGapReason::StaleReference,
                _ => SemanticGapReason::MissingRequiredField,
            };
            failure(reason, vec![query.provenance.clone()])
        } else if candidates.len() == 1 {
            SemanticResolution::Resolved(candidates.into_iter().next().expect("one candidate"))
        } else {
            let candidates =
                ResolutionCandidates::new(candidates).expect("distinct canonical values");
            if conflict {
                SemanticResolution::Conflicting(candidates)
            } else {
                SemanticResolution::Ambiguous(candidates)
            }
        };
        trace.sort_by(|a, b| (&a.fact, a.tier, a.rule).cmp(&(&b.fact, b.tier, b.rule)));
        Ok(InterpretationDecision {
            context: data.id.clone(),
            revision: data.revision.clone(),
            request: data.binding.request.clone(),
            query: query.clone(),
            field: query.field.clone(),
            span: query.span,
            capture_digest: context.digest.clone(),
            evaluated_at: at,
            selected_tier,
            resolution,
            trace,
            lookup_facts,
        })
    }

    /// Re-run exact lookup and eligibility checks before consuming a decision.
    /// Changed clocks require a new decision; changed basis requires recapture.
    /// This validates meaning only, never policy, consent or completeness.
    pub fn revalidate_decision(
        &self,
        context: &InterpretationContext,
        decision: &InterpretationDecision,
        at: UnixTimestamp,
    ) -> Result<(), InterpretationError> {
        if at != decision.evaluated_at || self.resolve(context, &decision.query, at)? != *decision {
            return Err(InterpretationError::ChangedBasis);
        }
        decision
            .resolution
            .require_resolved()
            .map(|_| ())
            .map_err(|_| InterpretationError::InvalidQuery)
    }
}
