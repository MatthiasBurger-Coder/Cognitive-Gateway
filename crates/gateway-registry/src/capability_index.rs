//! Deterministic, derived indexing and querying of catalog capabilities.
//!
//! The index is a read model.  It owns a snapshot of capability declarations
//! and provider relationships so it can be rebuilt from the canonical Agent
//! and Skill registries at any time.  It does not add catalog membership,
//! project context, permissions or execution behavior.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use gateway_domain::{
    AgentId, CapabilityClass, CapabilityConstraint, CapabilityDefinition, CapabilityDomain,
    CapabilityId, CapabilityInputKind, CapabilityOutputKind, CapabilityPrecondition, CapabilityTag,
    SkillId,
};

use crate::{Registry, RegistryIntegrityError, SkillDependencyGraph};

/// The kind of catalog definition that provides a capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum CapabilityProviderKind {
    /// The capability is declared directly by an Agent.
    Agent,
    /// The capability is declared directly by a Skill.
    Skill,
}

impl CapabilityProviderKind {
    /// Returns the canonical lower-case source prefix.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Skill => "skill",
        }
    }
}

impl std::fmt::Display for CapabilityProviderKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A canonical Agent or Skill source for a capability declaration.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum CapabilityProvider {
    /// A capability declared by an Agent.
    Agent { agent_id: AgentId },
    /// A capability declared by a Skill.
    Skill { skill_id: SkillId },
}

/// Alias emphasizing that a provider is also an explainability source.
pub type CapabilitySource = CapabilityProvider;

impl CapabilityProvider {
    /// Returns the provider kind.
    #[must_use]
    pub const fn kind(&self) -> CapabilityProviderKind {
        match self {
            Self::Agent { .. } => CapabilityProviderKind::Agent,
            Self::Skill { .. } => CapabilityProviderKind::Skill,
        }
    }

    /// Returns the provider's canonical ID as text.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Agent { agent_id } => agent_id.as_str(),
            Self::Skill { skill_id } => skill_id.as_str(),
        }
    }

    /// Returns the typed Agent ID when this is an Agent provider.
    #[must_use]
    pub fn agent_id(&self) -> Option<&AgentId> {
        match self {
            Self::Agent { agent_id } => Some(agent_id),
            Self::Skill { .. } => None,
        }
    }

    /// Returns the typed Skill ID when this is a Skill provider.
    #[must_use]
    pub fn skill_id(&self) -> Option<&SkillId> {
        match self {
            Self::Agent { .. } => None,
            Self::Skill { skill_id } => Some(skill_id),
        }
    }

    /// Returns the stable human- and machine-readable source identity.
    #[must_use]
    pub fn canonical_source(&self) -> String {
        format!("{}:{}", self.kind(), self.id())
    }
}

impl std::fmt::Display for CapabilityProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}:{}", self.kind(), self.id())
    }
}

/// An explicit structured selector accepted by [`CapabilityQuery`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum CapabilitySelector {
    /// Match the exact canonical capability ID.
    ///
    /// This variant is emitted in explainability data.  Queries keep their
    /// ID in [`CapabilityQuery::capability_id`] so structured selectors remain
    /// metadata filters.
    CapabilityId(CapabilityId),
    /// Match the capability's safety class.
    Class(CapabilityClass),
    /// Match the capability's reusable domain.
    Domain(CapabilityDomain),
    /// Require an input/context kind.
    InputKind(CapabilityInputKind),
    /// Require an output/result kind.
    OutputKind(CapabilityOutputKind),
    /// Require an intrinsic precondition.
    Precondition(CapabilityPrecondition),
    /// Require an intrinsic constraint.
    Constraint(CapabilityConstraint),
    /// Require an applicability tag.
    ApplicabilityTag(CapabilityTag),
}

/// A canonical capability query.
///
/// The capability ID is optional so callers can query all capabilities using
/// only explicit structured selectors.  When present, it is an exact typed
/// ID match.  All other selectors are conjunctive; list selectors mean that
/// the requested value must be present in the declaration's corresponding
/// list.  No fuzzy, textual, vector or inferred matching is performed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CapabilityQuery {
    capability_id: Option<CapabilityId>,
    selectors: BTreeSet<CapabilitySelector>,
}

impl CapabilityQuery {
    /// Creates an exact query for one canonical capability ID.
    #[must_use]
    pub fn new(capability_id: CapabilityId) -> Self {
        Self::for_capability(capability_id)
    }

    /// Creates an exact query for one canonical capability ID.
    #[must_use]
    pub fn for_capability(capability_id: CapabilityId) -> Self {
        Self {
            capability_id: Some(capability_id),
            selectors: BTreeSet::new(),
        }
    }

    /// Creates a query over all indexed capability IDs.
    #[must_use]
    pub fn all() -> Self {
        Self::default()
    }

    /// Sets or replaces the exact capability ID selector.
    #[must_use]
    pub fn with_capability_id(mut self, capability_id: CapabilityId) -> Self {
        self.capability_id = Some(capability_id);
        self
    }

    /// Adds an exact class selector.
    #[must_use]
    pub fn with_class(mut self, class: CapabilityClass) -> Self {
        self.selectors.insert(CapabilitySelector::Class(class));
        self
    }

    /// Adds an exact domain selector.
    #[must_use]
    pub fn with_domain(mut self, domain: CapabilityDomain) -> Self {
        self.selectors.insert(CapabilitySelector::Domain(domain));
        self
    }

    /// Adds a required input/context kind selector.
    #[must_use]
    pub fn with_input_kind(mut self, kind: CapabilityInputKind) -> Self {
        self.selectors.insert(CapabilitySelector::InputKind(kind));
        self
    }

    /// Alias for [`Self::with_input_kind`].
    #[must_use]
    pub fn requiring_input_kind(self, kind: CapabilityInputKind) -> Self {
        self.with_input_kind(kind)
    }

    /// Adds required input/context kind selectors.
    #[must_use]
    pub fn with_input_kinds(
        mut self,
        kinds: impl IntoIterator<Item = CapabilityInputKind>,
    ) -> Self {
        self.selectors
            .extend(kinds.into_iter().map(CapabilitySelector::InputKind));
        self
    }

    /// Adds a required output/result kind selector.
    #[must_use]
    pub fn with_output_kind(mut self, kind: CapabilityOutputKind) -> Self {
        self.selectors.insert(CapabilitySelector::OutputKind(kind));
        self
    }

    /// Alias for [`Self::with_output_kind`].
    #[must_use]
    pub fn requiring_output_kind(self, kind: CapabilityOutputKind) -> Self {
        self.with_output_kind(kind)
    }

    /// Adds required output/result kind selectors.
    #[must_use]
    pub fn with_output_kinds(
        mut self,
        kinds: impl IntoIterator<Item = CapabilityOutputKind>,
    ) -> Self {
        self.selectors
            .extend(kinds.into_iter().map(CapabilitySelector::OutputKind));
        self
    }

    /// Adds a required intrinsic precondition selector.
    #[must_use]
    pub fn with_precondition(mut self, precondition: CapabilityPrecondition) -> Self {
        self.selectors
            .insert(CapabilitySelector::Precondition(precondition));
        self
    }

    /// Alias for [`Self::with_precondition`].
    #[must_use]
    pub fn requiring_precondition(self, precondition: CapabilityPrecondition) -> Self {
        self.with_precondition(precondition)
    }

    /// Adds required intrinsic precondition selectors.
    #[must_use]
    pub fn with_preconditions(
        mut self,
        preconditions: impl IntoIterator<Item = CapabilityPrecondition>,
    ) -> Self {
        self.selectors.extend(
            preconditions
                .into_iter()
                .map(CapabilitySelector::Precondition),
        );
        self
    }

    /// Adds a required intrinsic constraint selector.
    #[must_use]
    pub fn with_constraint(mut self, constraint: CapabilityConstraint) -> Self {
        self.selectors
            .insert(CapabilitySelector::Constraint(constraint));
        self
    }

    /// Alias for [`Self::with_constraint`].
    #[must_use]
    pub fn requiring_constraint(self, constraint: CapabilityConstraint) -> Self {
        self.with_constraint(constraint)
    }

    /// Adds required intrinsic constraint selectors.
    #[must_use]
    pub fn with_constraints(
        mut self,
        constraints: impl IntoIterator<Item = CapabilityConstraint>,
    ) -> Self {
        self.selectors
            .extend(constraints.into_iter().map(CapabilitySelector::Constraint));
        self
    }

    /// Adds a required applicability tag selector.
    #[must_use]
    pub fn with_applicability_tag(mut self, tag: CapabilityTag) -> Self {
        self.selectors
            .insert(CapabilitySelector::ApplicabilityTag(tag));
        self
    }

    /// Alias for [`Self::with_applicability_tag`].
    #[must_use]
    pub fn with_tag(self, tag: CapabilityTag) -> Self {
        self.with_applicability_tag(tag)
    }

    /// Adds required applicability tag selectors.
    #[must_use]
    pub fn with_applicability_tags(
        mut self,
        tags: impl IntoIterator<Item = CapabilityTag>,
    ) -> Self {
        self.selectors
            .extend(tags.into_iter().map(CapabilitySelector::ApplicabilityTag));
        self
    }

    /// Adds one explicit selector.
    #[must_use]
    pub fn with_selector(mut self, selector: CapabilitySelector) -> Self {
        self.selectors.insert(selector);
        self
    }

    /// Returns the optional exact capability ID selector.
    #[must_use]
    pub fn capability_id(&self) -> Option<&CapabilityId> {
        self.capability_id.as_ref()
    }

    /// Returns structured selectors in canonical enum/value order.
    pub fn selectors(&self) -> impl ExactSizeIterator<Item = &CapabilitySelector> {
        self.selectors.iter()
    }

    /// Returns whether the query has no ID or structured selectors.
    #[must_use]
    pub fn is_unconstrained(&self) -> bool {
        self.capability_id.is_none() && self.selectors.is_empty()
    }
}

/// A provider candidate returned by a capability query.
///
/// The candidate owns its declaration and relationship snapshot.  For an
/// Agent, `skill_ids` preserves its direct ordered Agent-to-Skill references;
/// for a Skill, it contains that Skill ID.  The optional owner-Agent
/// relationship is retained separately.  `dependency_closure` contains all
/// mandatory Skill dependencies, including those roots, in deterministic
/// dependency-first order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityCandidate {
    capability: CapabilityDefinition,
    provider: CapabilityProvider,
    owner_agent_id: Option<AgentId>,
    skill_ids: Vec<SkillId>,
    dependency_closure: Vec<SkillId>,
    matched_selectors: Vec<CapabilitySelector>,
}

impl CapabilityCandidate {
    /// Returns the matched capability declaration.
    #[must_use]
    pub fn capability(&self) -> &CapabilityDefinition {
        &self.capability
    }

    /// Alias for [`Self::capability`].
    #[must_use]
    pub fn declaration(&self) -> &CapabilityDefinition {
        self.capability()
    }

    /// Returns the canonical capability ID.
    #[must_use]
    pub fn capability_id(&self) -> &CapabilityId {
        self.capability.id()
    }

    /// Returns the Agent or Skill source that declared the capability.
    #[must_use]
    pub fn provider(&self) -> &CapabilityProvider {
        &self.provider
    }

    /// Alias for [`Self::provider`].
    #[must_use]
    pub fn source(&self) -> &CapabilitySource {
        self.provider()
    }

    /// Returns the owning Agent relationship when one is declared.
    ///
    /// An Agent capability is owned by that Agent itself.  A Skill capability
    /// preserves the Skill document's optional `owner_agent_id` relationship.
    #[must_use]
    pub fn owner_agent_id(&self) -> Option<&AgentId> {
        self.owner_agent_id.as_ref()
    }

    /// Returns the direct ordered Skill relationships carried by the source.
    #[must_use]
    pub fn skill_ids(&self) -> &[SkillId] {
        &self.skill_ids
    }

    /// Alias for [`Self::skill_ids`].
    #[must_use]
    pub fn required_skill_ids(&self) -> &[SkillId] {
        self.skill_ids()
    }

    /// Returns all mandatory Skills needed by this candidate in dependency-
    /// first order.
    #[must_use]
    pub fn dependency_closure(&self) -> &[SkillId] {
        &self.dependency_closure
    }

    /// Alias emphasizing that the closure contains Skill IDs.
    #[must_use]
    pub fn skill_dependency_closure(&self) -> &[SkillId] {
        self.dependency_closure()
    }

    /// Returns the selectors that matched this candidate.
    #[must_use]
    pub fn matched_selectors(&self) -> &[CapabilitySelector] {
        &self.matched_selectors
    }
}

/// A capability declaration indexed with all deterministic provider candidates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityIndexEntry {
    capability: CapabilityDefinition,
    candidates: Vec<CapabilityCandidate>,
}

impl CapabilityIndexEntry {
    /// Returns the canonical capability ID.
    #[must_use]
    pub fn id(&self) -> &CapabilityId {
        self.capability.id()
    }

    /// Returns the canonical capability declaration.
    #[must_use]
    pub fn capability(&self) -> &CapabilityDefinition {
        &self.capability
    }

    /// Alias for [`Self::capability`].
    #[must_use]
    pub fn declaration(&self) -> &CapabilityDefinition {
        self.capability()
    }

    /// Returns candidates in stable Agent/Skill and canonical-ID order.
    #[must_use]
    pub fn candidates(&self) -> &[CapabilityCandidate] {
        &self.candidates
    }

    /// Alias for [`Self::candidates`].
    #[must_use]
    pub fn providers(&self) -> &[CapabilityCandidate] {
        self.candidates()
    }

    /// Returns whether more than one provider candidate exists.
    #[must_use]
    pub fn is_ambiguous(&self) -> bool {
        self.candidates.len() > 1
    }
}

/// A deterministic derived index over all provided Agent and Skill capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CapabilityIndex {
    entries: BTreeMap<CapabilityId, CapabilityIndexEntry>,
}

impl CapabilityIndex {
    /// Builds an index from a validated Agent/Skill registry snapshot.
    ///
    /// Registry integrity is checked before any entries are materialized.
    /// In particular, conflicting metadata for one capability ID fails closed
    /// instead of making the index depend on traversal order.
    pub fn build(registry: &Registry) -> Result<Self, RegistryIntegrityError> {
        registry.validate_integrity()?;
        let graph = registry.skills().dependency_graph()?;
        let mut entries = BTreeMap::new();

        for agent in registry.agents().iter() {
            let skill_ids = agent.skill_ids().to_vec();
            let dependency_closure = ordered_skill_closure(&skill_ids, &graph);
            let provider = CapabilityProvider::Agent {
                agent_id: agent.id().clone(),
            };
            for capability in agent.provided_capabilities() {
                add_candidate(
                    &mut entries,
                    capability,
                    provider.clone(),
                    Some(agent.id().clone()),
                    skill_ids.clone(),
                    dependency_closure.clone(),
                );
            }
        }

        for skill in registry.skills().iter() {
            let skill_ids = vec![skill.id().clone()];
            let dependency_closure = ordered_skill_closure(&skill_ids, &graph);
            let provider = CapabilityProvider::Skill {
                skill_id: skill.id().clone(),
            };
            for capability in skill.provided_capabilities() {
                add_candidate(
                    &mut entries,
                    capability,
                    provider.clone(),
                    skill.owner_agent_id().cloned(),
                    skill_ids.clone(),
                    dependency_closure.clone(),
                );
            }
        }

        for entry in entries.values_mut() {
            entry
                .candidates
                .sort_by(|left, right| left.provider.cmp(&right.provider));
        }

        Ok(Self { entries })
    }

    /// Alias for [`Self::build`].
    pub fn from_registry(registry: &Registry) -> Result<Self, RegistryIntegrityError> {
        Self::build(registry)
    }

    /// Returns all indexed entries in canonical capability-ID order.
    #[must_use]
    pub fn entries(&self) -> impl ExactSizeIterator<Item = &CapabilityIndexEntry> {
        self.entries.values()
    }

    /// Returns all indexed capability IDs in canonical order.
    #[must_use]
    pub fn ids(&self) -> impl ExactSizeIterator<Item = &CapabilityId> {
        self.entries.keys()
    }

    /// Returns one indexed capability by canonical ID.
    #[must_use]
    pub fn get(&self, capability_id: &CapabilityId) -> Option<&CapabilityIndexEntry> {
        self.entries.get(capability_id)
    }

    /// Alias for [`Self::get`].
    #[must_use]
    pub fn capability(&self, capability_id: &CapabilityId) -> Option<&CapabilityIndexEntry> {
        self.get(capability_id)
    }

    /// Returns whether a capability ID has at least one provider.
    #[must_use]
    pub fn contains(&self, capability_id: &CapabilityId) -> bool {
        self.entries.contains_key(capability_id)
    }

    /// Returns the number of distinct indexed capability IDs.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns whether no provided capabilities were indexed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Evaluates an exact, structured capability query.
    ///
    /// Candidates are returned in canonical capability-ID order followed by
    /// stable provider order.  Candidates that were considered but rejected
    /// by a selector are retained as explainability data.  An unknown ID is a
    /// valid empty result, not an implicit fallback or fuzzy match.
    #[must_use]
    pub fn query(&self, query: &CapabilityQuery) -> CapabilityQueryResult {
        let entries: Box<dyn Iterator<Item = &CapabilityIndexEntry>> = match query.capability_id() {
            Some(capability_id) => match self.get(capability_id) {
                Some(entry) => Box::new(std::iter::once(entry)),
                None => Box::new(std::iter::empty()),
            },
            None => Box::new(self.entries.values()),
        };
        let mut matches = Vec::new();
        let mut rejections = Vec::new();

        for entry in entries {
            for candidate in entry.candidates() {
                let reasons = rejection_reasons(candidate, query);
                if reasons.is_empty() {
                    let mut matched = candidate.clone();
                    matched.matched_selectors = matched_selectors(query);
                    matches.push(matched);
                } else {
                    rejections.push(CapabilityRejection {
                        capability: candidate.capability.clone(),
                        provider: candidate.provider.clone(),
                        reasons,
                    });
                }
            }
        }

        CapabilityQueryResult {
            query: query.clone(),
            matches,
            rejections,
        }
    }

    /// Queries one canonical capability ID without constructing a query value.
    #[must_use]
    pub fn query_capability(&self, capability_id: &CapabilityId) -> CapabilityQueryResult {
        self.query(&CapabilityQuery::for_capability(capability_id.clone()))
    }

    /// Resolves a query only when exactly one provider matches.
    pub fn resolve_unique(
        &self,
        query: &CapabilityQuery,
    ) -> Result<&CapabilityCandidate, CapabilityResolutionError> {
        let result = self.query(query);
        match result.matches.as_slice() {
            [candidate] => {
                let capability_id = candidate.capability_id().clone();
                let provider = candidate.provider().clone();
                Ok(self
                    .get(&capability_id)
                    .and_then(|entry| {
                        entry
                            .candidates()
                            .iter()
                            .find(|indexed| indexed.provider() == &provider)
                    })
                    .expect("a matched index candidate must remain indexed"))
            }
            [] => Err(CapabilityResolutionError::NoMatch {
                query: query.clone(),
            }),
            candidates => Err(CapabilityResolutionError::Ambiguous {
                query: query.clone(),
                candidates: candidates
                    .iter()
                    .map(|candidate| candidate.provider().clone())
                    .collect(),
            }),
        }
    }

    /// Alias for [`Self::resolve_unique`].
    pub fn resolve(
        &self,
        query: &CapabilityQuery,
    ) -> Result<&CapabilityCandidate, CapabilityResolutionError> {
        self.resolve_unique(query)
    }
}

/// The result of evaluating a capability query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityQueryResult {
    query: CapabilityQuery,
    matches: Vec<CapabilityCandidate>,
    rejections: Vec<CapabilityRejection>,
}

impl CapabilityQueryResult {
    /// Returns the normalized query that produced this result.
    #[must_use]
    pub fn query(&self) -> &CapabilityQuery {
        &self.query
    }

    /// Returns matching provider candidates in deterministic order.
    #[must_use]
    pub fn matches(&self) -> &[CapabilityCandidate] {
        &self.matches
    }

    /// Alias for [`Self::matches`].
    #[must_use]
    pub fn candidates(&self) -> &[CapabilityCandidate] {
        self.matches()
    }

    /// Returns candidates rejected by explicit selectors, with reasons.
    #[must_use]
    pub fn rejections(&self) -> &[CapabilityRejection] {
        &self.rejections
    }

    /// Alias for [`Self::rejections`].
    #[must_use]
    pub fn rejected(&self) -> &[CapabilityRejection] {
        self.rejections()
    }

    /// Returns the explicit query outcome.
    #[must_use]
    pub fn outcome(&self) -> CapabilityQueryOutcome {
        match self.matches.len() {
            0 => CapabilityQueryOutcome::NoMatch,
            1 => CapabilityQueryOutcome::Unique,
            _ => CapabilityQueryOutcome::Ambiguous,
        }
    }

    /// Returns whether no provider matched.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.matches.is_empty()
    }

    /// Returns the number of matching providers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.matches.len()
    }

    /// Returns whether the query matched more than one provider.
    #[must_use]
    pub fn is_ambiguous(&self) -> bool {
        matches!(self.outcome(), CapabilityQueryOutcome::Ambiguous)
    }

    /// Returns whether exactly one provider matched.
    #[must_use]
    pub fn is_unique(&self) -> bool {
        matches!(self.outcome(), CapabilityQueryOutcome::Unique)
    }

    /// Resolves the result only when exactly one candidate matched.
    pub fn unique_candidate(&self) -> Result<&CapabilityCandidate, CapabilityResolutionError> {
        match self.matches.as_slice() {
            [candidate] => Ok(candidate),
            [] => Err(CapabilityResolutionError::NoMatch {
                query: self.query.clone(),
            }),
            candidates => Err(CapabilityResolutionError::Ambiguous {
                query: self.query.clone(),
                candidates: candidates
                    .iter()
                    .map(|candidate| candidate.provider().clone())
                    .collect(),
            }),
        }
    }
}

/// The explicit cardinality outcome of a capability query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityQueryOutcome {
    /// No indexed provider satisfied the query.
    NoMatch,
    /// Exactly one indexed provider satisfied the query.
    Unique,
    /// Multiple indexed providers satisfied the query.
    Ambiguous,
}

/// A rejected provider candidate and the selectors it failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityRejection {
    capability: CapabilityDefinition,
    provider: CapabilityProvider,
    reasons: Vec<CapabilityRejectionReason>,
}

impl CapabilityRejection {
    /// Returns the rejected capability declaration.
    #[must_use]
    pub fn capability(&self) -> &CapabilityDefinition {
        &self.capability
    }

    /// Returns the rejected provider source.
    #[must_use]
    pub fn provider(&self) -> &CapabilityProvider {
        &self.provider
    }

    /// Returns every explicit selector that was not satisfied.
    #[must_use]
    pub fn reasons(&self) -> &[CapabilityRejectionReason] {
        &self.reasons
    }

    /// Alias for [`Self::reasons`].
    #[must_use]
    pub fn failed_selectors(&self) -> impl ExactSizeIterator<Item = &CapabilitySelector> {
        self.reasons.iter().map(CapabilityRejectionReason::selector)
    }
}

/// A precise reason a candidate was rejected by a deterministic selector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityRejectionReason {
    /// The candidate did not satisfy this selector.
    SelectorNotSatisfied(CapabilitySelector),
}

impl CapabilityRejectionReason {
    /// Returns the failed selector.
    #[must_use]
    pub fn selector(&self) -> &CapabilitySelector {
        match self {
            Self::SelectorNotSatisfied(selector) => selector,
        }
    }
}

/// An error from fail-closed unique capability resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityResolutionError {
    /// No indexed provider satisfied the query.
    NoMatch { query: CapabilityQuery },
    /// More than one provider satisfied the query.
    Ambiguous {
        query: CapabilityQuery,
        candidates: Vec<CapabilityProvider>,
    },
}

impl std::fmt::Display for CapabilityResolutionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoMatch { query } => {
                write!(formatter, "capability query {:?} has no match", query)
            }
            Self::Ambiguous { query, candidates } => write!(
                formatter,
                "capability query {:?} is ambiguous across {} providers",
                query,
                candidates.len()
            ),
        }
    }
}

impl std::error::Error for CapabilityResolutionError {}

fn add_candidate(
    entries: &mut BTreeMap<CapabilityId, CapabilityIndexEntry>,
    capability: &CapabilityDefinition,
    provider: CapabilityProvider,
    owner_agent_id: Option<AgentId>,
    skill_ids: Vec<SkillId>,
    dependency_closure: Vec<SkillId>,
) {
    entries
        .entry(capability.id().clone())
        .or_insert_with(|| CapabilityIndexEntry {
            capability: capability.clone(),
            candidates: Vec::new(),
        })
        .candidates
        .push(CapabilityCandidate {
            capability: capability.clone(),
            provider,
            owner_agent_id,
            skill_ids,
            dependency_closure,
            matched_selectors: Vec::new(),
        });
}

fn ordered_skill_closure(roots: &[SkillId], graph: &SkillDependencyGraph) -> Vec<SkillId> {
    let mut closure = BTreeSet::new();
    let mut pending = roots.to_vec();
    while let Some(skill_id) = pending.pop() {
        if !closure.insert(skill_id.clone()) {
            continue;
        }
        if let Some(dependencies) = graph.dependencies(&skill_id) {
            pending.extend(dependencies.iter().cloned());
        }
    }
    graph
        .topological_order()
        .iter()
        .filter(|skill_id| closure.contains(*skill_id))
        .cloned()
        .collect()
}

fn matched_selectors(query: &CapabilityQuery) -> Vec<CapabilitySelector> {
    let mut selectors = query.selectors.iter().cloned().collect::<Vec<_>>();
    if let Some(capability_id) = query.capability_id.clone() {
        selectors.insert(0, CapabilitySelector::CapabilityId(capability_id));
    }
    selectors
}

fn rejection_reasons(
    candidate: &CapabilityCandidate,
    query: &CapabilityQuery,
) -> Vec<CapabilityRejectionReason> {
    let mut reasons = Vec::new();
    if let Some(capability_id) = query.capability_id()
        && candidate.capability_id() != capability_id
    {
        reasons.push(CapabilityRejectionReason::SelectorNotSatisfied(
            CapabilitySelector::CapabilityId(capability_id.clone()),
        ));
    }
    for selector in &query.selectors {
        let satisfied = match selector {
            CapabilitySelector::CapabilityId(capability_id) => {
                candidate.capability_id() == capability_id
            }
            CapabilitySelector::Class(class) => candidate.capability.class() == *class,
            CapabilitySelector::Domain(domain) => candidate.capability.domain() == domain,
            CapabilitySelector::InputKind(kind) => {
                candidate.capability.input_kinds().contains(kind)
            }
            CapabilitySelector::OutputKind(kind) => {
                candidate.capability.output_kinds().contains(kind)
            }
            CapabilitySelector::Precondition(precondition) => {
                candidate.capability.preconditions().contains(precondition)
            }
            CapabilitySelector::Constraint(constraint) => {
                candidate.capability.constraints().contains(constraint)
            }
            CapabilitySelector::ApplicabilityTag(tag) => {
                candidate.capability.applicability_tags().contains(tag)
            }
        };
        if !satisfied {
            reasons.push(CapabilityRejectionReason::SelectorNotSatisfied(
                selector.clone(),
            ));
        }
    }
    reasons
}

#[cfg(test)]
mod tests {
    use gateway_domain::{
        AgentDefinitionDocument, CapabilityClass, CapabilityDefinition, CapabilityId,
        SkillDefinitionDocument,
    };

    use super::{
        CapabilityIndex, CapabilityProvider, CapabilityQuery, CapabilityQueryOutcome,
        CapabilityRejectionReason,
    };
    use crate::Registry;

    fn capability(id: &str, domain: &str, tags: &[&str]) -> CapabilityDefinition {
        CapabilityDefinition::new_with_contract(
            CapabilityId::new(id).unwrap(),
            CapabilityClass::Inspect,
            domain,
            format!("Capability {id}"),
            ["repository.snapshot"],
            ["analysis.report"],
            ["repository.available"],
            ["read-only"],
            tags.iter().copied(),
        )
        .unwrap()
    }

    fn skill(
        id: &str,
        dependencies: &[&str],
        capabilities: impl IntoIterator<Item = CapabilityDefinition>,
    ) -> SkillDefinitionDocument {
        SkillDefinitionDocument::new(
            gateway_domain::SkillId::new(id).unwrap(),
            format!("Skill {id}"),
            format!("Skill {id}"),
            None,
            ["source"],
            ["rule"],
            ["verification"],
            dependencies
                .iter()
                .map(|dependency| gateway_domain::SkillId::new(*dependency).unwrap()),
            [],
            [],
            [],
        )
        .unwrap()
        .with_provided_capabilities(capabilities)
        .unwrap()
    }

    fn agent(
        id: &str,
        skill_ids: &[&str],
        capabilities: impl IntoIterator<Item = CapabilityDefinition>,
    ) -> AgentDefinitionDocument {
        AgentDefinitionDocument::new_with_provided_capabilities(
            gateway_domain::AgentId::new(id).unwrap(),
            format!("Agent {id}"),
            skill_ids
                .iter()
                .map(|skill_id| gateway_domain::SkillId::new(*skill_id).unwrap()),
            capabilities,
        )
        .unwrap()
    }

    #[test]
    fn builds_equivalent_indexes_with_stable_provider_and_closure_order() {
        let shared = capability("shared.analysis", "analysis", &["analysis"]);
        let other = capability("other.analysis", "other", &["other"]);
        let first = Registry::from_documents(
            [agent("zeta", &["root"], [shared.clone()])],
            [
                skill("root", &["leaf"], [shared.clone()]),
                skill("leaf", &[], []),
                skill("other", &[], [other]),
            ],
        )
        .unwrap();
        let second = Registry::from_documents(
            [agent("zeta", &["root"], [shared.clone()])],
            [
                skill(
                    "other",
                    &[],
                    [capability("other.analysis", "other", &["other"])],
                ),
                skill("leaf", &[], []),
                skill("root", &["leaf"], [shared]),
            ],
        )
        .unwrap();

        let first_index = CapabilityIndex::build(&first).unwrap();
        let second_index = CapabilityIndex::from_registry(&second).unwrap();
        assert_eq!(first_index, second_index);
        assert_eq!(
            first_index
                .ids()
                .map(CapabilityId::as_str)
                .collect::<Vec<_>>(),
            ["other.analysis", "shared.analysis"]
        );

        let entry = first_index
            .get(&CapabilityId::new("shared.analysis").unwrap())
            .unwrap();
        assert_eq!(entry.candidates().len(), 2);
        assert_eq!(
            entry
                .candidates()
                .iter()
                .map(|candidate| candidate.provider().canonical_source())
                .collect::<Vec<_>>(),
            ["agent:zeta", "skill:root"]
        );
        assert_eq!(
            entry.candidates()[0]
                .dependency_closure()
                .iter()
                .map(gateway_domain::SkillId::as_str)
                .collect::<Vec<_>>(),
            ["leaf", "root"]
        );
        assert_eq!(
            entry.candidates()[0]
                .owner_agent_id()
                .map(gateway_domain::AgentId::as_str),
            Some("zeta")
        );
        assert_eq!(entry.candidates()[1].skill_ids()[0].as_str(), "root");
        assert!(entry.candidates()[1].owner_agent_id().is_none());
    }

    #[test]
    fn returns_matches_rejections_and_explicit_cardinality() {
        let shared = capability("shared.analysis", "analysis", &["analysis"]);
        let other = capability("other.analysis", "other", &["other"]);
        let registry = Registry::from_documents(
            [agent("reviewer", &["review"], [shared.clone()])],
            [skill("review", &[], [shared]), skill("other", &[], [other])],
        )
        .unwrap();
        let index = registry.capability_index().unwrap();

        let query = CapabilityQuery::new(CapabilityId::new("shared.analysis").unwrap())
            .with_class(CapabilityClass::Inspect)
            .with_domain(gateway_domain::CapabilityDomain::new("analysis").unwrap())
            .with_input_kind(
                gateway_domain::CapabilityInputKind::new("repository.snapshot").unwrap(),
            );
        let result = index.query(&query);
        assert_eq!(result.outcome(), CapabilityQueryOutcome::Ambiguous);
        assert_eq!(result.candidates().len(), 2);
        assert!(result.rejections().is_empty());
        assert!(
            result.candidates()[0]
                .matched_selectors()
                .iter()
                .any(|selector| matches!(selector, super::CapabilitySelector::CapabilityId(_)))
        );
        assert!(matches!(
            index.resolve_unique(&query),
            Err(super::CapabilityResolutionError::Ambiguous { .. })
        ));

        let filtered = index.query(
            &CapabilityQuery::all()
                .with_domain(gateway_domain::CapabilityDomain::new("analysis").unwrap()),
        );
        assert_eq!(filtered.candidates().len(), 2);
        assert_eq!(filtered.rejections().len(), 1);
        assert_eq!(
            filtered.rejections()[0].provider(),
            &CapabilityProvider::Skill {
                skill_id: gateway_domain::SkillId::new("other").unwrap()
            }
        );
        assert!(matches!(
            filtered.rejections()[0].reasons()[0],
            CapabilityRejectionReason::SelectorNotSatisfied(super::CapabilitySelector::Domain(_))
        ));

        let missing = index.query_capability(&CapabilityId::new("missing").unwrap());
        assert_eq!(missing.outcome(), CapabilityQueryOutcome::NoMatch);
        assert!(missing.candidates().is_empty());
        assert!(missing.rejections().is_empty());
        assert!(matches!(
            missing.unique_candidate(),
            Err(super::CapabilityResolutionError::NoMatch { .. })
        ));
    }

    #[test]
    fn refuses_conflicting_declarations_before_index_materialization() {
        let registry = Registry::from_documents(
            [agent(
                "reviewer",
                &["review"],
                [capability("shared.analysis", "analysis", &["analysis"])],
            )],
            [skill(
                "review",
                &[],
                [capability("shared.analysis", "security", &["security"])],
            )],
        )
        .unwrap();

        assert!(matches!(
            CapabilityIndex::build(&registry),
            Err(crate::RegistryIntegrityError::ConflictingCapabilityDeclaration { .. })
        ));
    }

    #[test]
    fn exposes_the_complete_typed_query_and_explanation_surface() {
        let shared = capability("shared.analysis", "analysis", &["analysis"]);
        let registry = Registry::from_documents(
            [agent("reviewer", &["review"], [shared.clone()])],
            [skill("review", &[], [shared])],
        )
        .unwrap();
        let index = CapabilityIndex::build(&registry).unwrap();
        let capability_id = CapabilityId::new("shared.analysis").unwrap();
        let domain = gateway_domain::CapabilityDomain::new("analysis").unwrap();
        let input = gateway_domain::CapabilityInputKind::new("repository.snapshot").unwrap();
        let output = gateway_domain::CapabilityOutputKind::new("analysis.report").unwrap();
        let precondition =
            gateway_domain::CapabilityPrecondition::new("repository.available").unwrap();
        let constraint = gateway_domain::CapabilityConstraint::new("read-only").unwrap();
        let tag = gateway_domain::CapabilityTag::new("analysis").unwrap();

        let query = CapabilityQuery::for_capability(capability_id.clone())
            .with_capability_id(capability_id.clone())
            .with_class(CapabilityClass::Inspect)
            .with_domain(domain.clone())
            .with_input_kind(input.clone())
            .requiring_input_kind(input.clone())
            .with_input_kinds([input.clone()])
            .with_output_kind(output.clone())
            .requiring_output_kind(output.clone())
            .with_output_kinds([output.clone()])
            .with_precondition(precondition.clone())
            .requiring_precondition(precondition.clone())
            .with_preconditions([precondition.clone()])
            .with_constraint(constraint.clone())
            .requiring_constraint(constraint.clone())
            .with_constraints([constraint.clone()])
            .with_applicability_tag(tag.clone())
            .with_tag(tag.clone())
            .with_applicability_tags([tag.clone()])
            .with_selector(super::CapabilitySelector::Class(CapabilityClass::Inspect));
        assert_eq!(query.capability_id(), Some(&capability_id));
        assert!(!query.is_unconstrained());
        assert!(query.selectors().len() >= 7);

        let result = index.query(&query);
        assert_eq!(result.matches().len(), 2);
        assert_eq!(result.candidates(), result.matches());
        assert_eq!(result.query(), &query);
        assert!(!result.is_unique());
        assert!(!result.is_empty());
        assert_eq!(result.len(), 2);
        assert!(result.is_ambiguous());
        assert!(result.unique_candidate().is_err());

        let candidate = &result.candidates()[0];
        assert_eq!(candidate.capability(), candidate.declaration());
        assert_eq!(candidate.capability_id(), &capability_id);
        assert_eq!(candidate.provider(), candidate.source());
        assert!(candidate.provider().agent_id().is_some());
        assert!(candidate.provider().skill_id().is_none());
        assert_eq!(candidate.owner_agent_id(), candidate.provider().agent_id());
        assert_eq!(candidate.skill_ids(), candidate.required_skill_ids());
        assert_eq!(
            candidate.dependency_closure(),
            candidate.skill_dependency_closure()
        );
        assert!(!candidate.matched_selectors().is_empty());

        let agent_provider = candidate.provider();
        assert_eq!(agent_provider.kind().as_str(), "agent");
        assert_eq!(agent_provider.id(), "reviewer");
        assert_eq!(agent_provider.canonical_source(), "agent:reviewer");
        assert_eq!(agent_provider.to_string(), "agent:reviewer");
        let skill_provider = super::CapabilityProvider::Skill {
            skill_id: gateway_domain::SkillId::new("review").unwrap(),
        };
        assert_eq!(skill_provider.kind().as_str(), "skill");
        assert_eq!(skill_provider.id(), "review");
        assert!(skill_provider.agent_id().is_none());
        assert!(skill_provider.skill_id().is_some());
        assert_eq!(skill_provider.canonical_source(), "skill:review");

        let entry = index.capability(&capability_id).unwrap();
        assert_eq!(entry.id(), &capability_id);
        assert_eq!(entry.capability(), entry.declaration());
        assert_eq!(entry.candidates(), entry.providers());
        assert!(entry.is_ambiguous());
        assert_eq!(index.entries().len(), 1);
        assert!(index.contains(&capability_id));
        assert_eq!(index.len(), 1);
        assert!(!index.is_empty());
        assert!(CapabilityIndex::default().is_empty());

        let unique_query = CapabilityQuery::new(capability_id.clone())
            .with_selector(super::CapabilitySelector::Domain(domain));
        assert!(index.resolve(&unique_query).is_err());
        let unique_result = index.query(&CapabilityQuery::new(capability_id.clone()));
        assert!(unique_result.unique_candidate().is_err());

        let all = CapabilityQuery::all();
        assert!(all.is_unconstrained());
        assert!(all.selectors().next().is_none());
        let id_selector = CapabilityQuery::all().with_selector(
            super::CapabilitySelector::CapabilityId(capability_id.clone()),
        );
        let id_result = index.query(&id_selector);
        assert_eq!(id_result.candidates().len(), 2);
        assert!(id_result.rejected().is_empty());
        assert!(id_result.rejections().is_empty());

        let display_error = index
            .resolve_unique(&CapabilityQuery::new(capability_id))
            .unwrap_err();
        assert!(display_error.to_string().contains("ambiguous"));
        let no_match = CapabilityIndex::default().query(&CapabilityQuery::all());
        assert_eq!(no_match.outcome(), CapabilityQueryOutcome::NoMatch);
    }

    #[test]
    fn resolves_unique_candidates_and_exposes_rejection_diagnostics() {
        let capability = capability("unique.analysis", "analysis", &["analysis"]);
        let registry =
            Registry::from_documents([], [skill("only", &[], [capability.clone()])]).unwrap();
        let index = registry.capability_index().unwrap();
        let capability_id = CapabilityId::new("unique.analysis").unwrap();
        let query = CapabilityQuery::new(capability_id.clone());

        let result = index.query(&query);
        assert_eq!(result.outcome(), CapabilityQueryOutcome::Unique);
        assert!(result.is_unique());
        let candidate = result.unique_candidate().unwrap();
        assert_eq!(candidate.capability_id(), &capability_id);
        let resolved = index.resolve_unique(&query).unwrap();
        assert_eq!(resolved.capability(), candidate.capability());
        assert_eq!(resolved.provider(), candidate.provider());
        assert_eq!(
            index.resolve(&query).unwrap().provider(),
            candidate.provider()
        );

        let filtered = index.query(&CapabilityQuery::all().with_selector(
            super::CapabilitySelector::CapabilityId(CapabilityId::new("other.analysis").unwrap()),
        ));
        assert_eq!(filtered.outcome(), CapabilityQueryOutcome::NoMatch);
        assert!(filtered.is_empty());
        assert_eq!(filtered.len(), 0);
        assert_eq!(filtered.rejected(), filtered.rejections());
        assert_eq!(filtered.rejections().len(), 1);
        let rejection = &filtered.rejections()[0];
        assert_eq!(rejection.capability(), &capability);
        assert_eq!(rejection.provider().to_string(), "skill:only");
        let failed = rejection.failed_selectors().collect::<Vec<_>>();
        assert_eq!(
            failed,
            rejection
                .reasons()
                .iter()
                .map(|reason| reason.selector())
                .collect::<Vec<_>>()
        );
        assert_eq!(failed.len(), 1);
        assert!(matches!(
            failed[0],
            super::CapabilitySelector::CapabilityId(id) if id.as_str() == "other.analysis"
        ));

        let no_match_error = filtered.unique_candidate().unwrap_err();
        assert!(no_match_error.to_string().contains("no match"));
    }
}
