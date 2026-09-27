//! Filesystem/Git and in-memory vector adapters at the outer retrieval boundary.
use gateway_application::ports::outbound::{
    EmbeddingPort, KnowledgeRetrievalPort, RetrievalAttemptFailure,
};
use gateway_application::retrieval_pipeline::{
    ContextualLocation, FusionPolicy, HybridCandidate, RetrievalReranker, RetrievalSourceAdapter,
    ScoreOrigin, federate, fuse_candidates, rerank,
};
use gateway_domain::{
    BudgetUsage, Confidence, ContentDigest, ContextBudgetClass, ContextScopeId,
    EmbeddingIndexMetadata, EmbeddingModel, EmbeddingModelRequirement, EmbeddingRequest,
    EmbeddingResult, FreshnessStatus, NonEmptyText, Provenance, ProvenanceId, QualityMetadata,
    ReferenceId, RetrievalBatch, RetrievalBatchInput, RetrievalError, RetrievalExplanation,
    RetrievalExplanationTarget, RetrievalPlan, RetrievalReason, RetrievalRequest, RetrievalResult,
    RetrievalRound, RetrievalSourceId, RetrievalStatus, RetrievalStrategyId, RetrievalVersion,
    RetrievedFragment, SensitivityClass, SourceId, SourceKind, TrustClass, Uncertainty,
};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

const MAX_FILES: usize = 10_000;
const MAX_FILE_BYTES: u64 = 16 * 1024;

fn digest(bytes: &[u8]) -> ContentDigest {
    ContentDigest::new(format!("{:x}", Sha256::digest(bytes))).expect("SHA-256 is a digest")
}

fn text(value: String) -> Result<NonEmptyText, RetrievalError> {
    NonEmptyText::new(value).map_err(|_| RetrievalError::InvalidResult)
}

/// A repository root is pinned to one explicit scope. Symlinks are never
/// traversed, and each source snapshot is derived from bytes read at search time.
pub struct RepositoryLexicalAdapter {
    scope: ContextScopeId,
    source: RetrievalSourceId,
    strategy: RetrievalStrategyId,
    root: PathBuf,
    sensitivity: SensitivityClass,
}

impl RepositoryLexicalAdapter {
    pub fn new(
        scope: ContextScopeId,
        source: RetrievalSourceId,
        strategy: RetrievalStrategyId,
        root: impl AsRef<Path>,
        sensitivity: SensitivityClass,
    ) -> Result<Self, RetrievalError> {
        let root = root
            .as_ref()
            .canonicalize()
            .map_err(|_| RetrievalError::ServiceUnavailable)?;
        if !root.is_dir() {
            return Err(RetrievalError::ServiceUnavailable);
        }
        Ok(Self {
            scope,
            source,
            strategy,
            root,
            sensitivity,
        })
    }

    /// Collect all eligible files for index construction or refresh checking.
    pub fn collect_fragments(
        &self,
    ) -> Result<Vec<(RetrievedFragment, ContextualLocation)>, RetrievalError> {
        let repository_revision = self.git_revision();
        let source_kind = if repository_revision.is_some() {
            SourceKind::Git
        } else {
            SourceKind::Repository
        };
        let mut paths = Vec::new();
        self.walk(&self.root, &mut paths)?;
        let mut fragments = Vec::new();
        for path in paths {
            let bytes = fs::read(&path).map_err(|_| RetrievalError::ServiceUnavailable)?;
            let Ok(content) = String::from_utf8(bytes.clone()) else {
                continue;
            };
            if content.trim().is_empty() {
                continue;
            }
            let relative = path
                .strip_prefix(&self.root)
                .map_err(|_| RetrievalError::InvalidResult)?;
            let relative = relative
                .to_str()
                .ok_or(RetrievalError::InvalidResult)?
                .replace('\\', "/");
            let snapshot = digest(&bytes);
            let path_id = format!(
                "{}-{}",
                self.source.as_str(),
                digest(relative.as_bytes()).as_str()
            );
            let revision = repository_revision
                .clone()
                .unwrap_or_else(|| snapshot.as_str().to_owned());
            let provenance = Provenance::new(
                ProvenanceId::new(&path_id).map_err(|_| RetrievalError::InvalidResult)?,
                source_kind,
                SourceId::new(self.source.as_str()).map_err(|_| RetrievalError::InvalidResult)?,
                format!("{relative}@{revision}"),
            )
            .map_err(|_| RetrievalError::InvalidResult)?;
            let fragment = RetrievedFragment {
                id: ReferenceId::new(path_id).map_err(|_| RetrievalError::InvalidResult)?,
                scope: self.scope.clone(),
                content: text(content)?,
                provenance,
                snapshot,
                quality: QualityMetadata::new(
                    TrustClass::RetrievedContent,
                    self.sensitivity,
                    Confidence::Unknown,
                    FreshnessStatus::Fresh,
                    Uncertainty::None,
                ),
                evidence: BTreeSet::new(),
            };
            let location = ContextualLocation {
                document: text(relative)?,
                section: None,
                symbol: None,
                revision: text(revision)?,
            };
            fragments.push((fragment, location));
        }
        Ok(fragments)
    }

    fn git_revision(&self) -> Option<String> {
        let output = Command::new("git")
            .args(["-C", self.root.to_str()?, "rev-parse", "--verify", "HEAD"])
            .output()
            .ok()?;
        if output.status.success() {
            String::from_utf8(output.stdout)
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        } else {
            None
        }
    }

    fn walk(&self, directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), RetrievalError> {
        let mut entries = fs::read_dir(directory)
            .map_err(|_| RetrievalError::ServiceUnavailable)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| RetrievalError::ServiceUnavailable)?;
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            let path = entry.path();
            let kind = entry
                .file_type()
                .map_err(|_| RetrievalError::ServiceUnavailable)?;
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                if !matches!(
                    entry.file_name().to_str(),
                    Some(".git" | "target" | ".idea")
                ) {
                    self.walk(&path, files)?;
                }
            } else if kind.is_file() {
                let extension = path
                    .extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or("");
                if matches!(
                    extension,
                    "md" | "rs" | "toml" | "json" | "yaml" | "yml" | "txt" | "feature" | "py"
                ) && entry
                    .metadata()
                    .map_err(|_| RetrievalError::ServiceUnavailable)?
                    .len()
                    <= MAX_FILE_BYTES
                {
                    files.push(path);
                    if files.len() > MAX_FILES {
                        return Err(RetrievalError::BudgetExceeded);
                    }
                }
            }
        }
        Ok(())
    }
}

impl RetrievalSourceAdapter for RepositoryLexicalAdapter {
    fn source(&self) -> &RetrievalSourceId {
        &self.source
    }
    fn strategy(&self) -> &RetrievalStrategyId {
        &self.strategy
    }
    fn retrieve(&self, request: &RetrievalRequest) -> Result<Vec<HybridCandidate>, RetrievalError> {
        if request.input().scope != self.scope {
            return Err(RetrievalError::ScopeMismatch);
        }
        if self.sensitivity > request.input().required.maximum_sensitivity
            || !request
                .input()
                .required
                .accepted_trust
                .contains(&TrustClass::RetrievedContent)
        {
            return Ok(Vec::new());
        }
        let mut results = Vec::new();
        for (fragment, mut location) in self.collect_fragments()? {
            let content_lower = fragment.content.as_str().to_lowercase();
            let mut best = 0u32;
            let mut exact = false;
            for query in &request.input().queries {
                let query_text = query.0.as_str().trim().to_lowercase();
                if query_text.is_empty() {
                    continue;
                }
                let terms: Vec<_> = query_text.split_whitespace().collect();
                let matches = terms
                    .iter()
                    .filter(|term| content_lower.contains(**term))
                    .count();
                let is_exact = content_lower.contains(&query_text)
                    || location
                        .document
                        .as_str()
                        .to_lowercase()
                        .contains(&query_text);
                if matches == 0 && !is_exact {
                    continue;
                }
                let score = if is_exact {
                    1_000_000
                } else {
                    ((matches as u64 * 800_000) / terms.len() as u64) as u32
                };
                if score > best {
                    best = score;
                    exact = is_exact;
                    if let Some(position) = content_lower.find(terms[0]) {
                        let prefix = &fragment.content.as_str()[..position];
                        location.section = prefix
                            .lines()
                            .rev()
                            .find(|line| line.starts_with('#'))
                            .and_then(|line| NonEmptyText::new(line.trim().to_owned()).ok());
                        location.symbol = prefix
                            .lines()
                            .rev()
                            .find(|line| {
                                let line = line.trim_start();
                                line.starts_with("fn ")
                                    || line.starts_with("pub fn ")
                                    || line.starts_with("struct ")
                                    || line.starts_with("pub struct ")
                            })
                            .and_then(|line| NonEmptyText::new(line.trim().to_owned()).ok());
                    }
                }
            }
            if best > 0 {
                let result = RetrievalResult {
                    fragment,
                    source: self.source.clone(),
                    strategy: self.strategy.clone(),
                    score: i64::from(best),
                };
                results.push(HybridCandidate::new(
                    result,
                    Some(location),
                    exact,
                    Some(best),
                    None,
                )?);
            }
        }
        Ok(results)
    }
}

struct IndexedCorpus {
    embedding: EmbeddingResult,
    locations: Vec<ContextualLocation>,
}

/// Replaceable embedding model with a scoped in-memory vector partition.
/// Searches check current source snapshots before using the index.
pub struct VectorIndexAdapter<'a> {
    source: RetrievalSourceId,
    strategy: RetrievalStrategyId,
    repository: &'a RepositoryLexicalAdapter,
    embedding: &'a dyn EmbeddingPort,
    model: EmbeddingModel,
    index: RefCell<Option<IndexedCorpus>>,
}

impl<'a> VectorIndexAdapter<'a> {
    pub fn new(
        source: RetrievalSourceId,
        strategy: RetrievalStrategyId,
        repository: &'a RepositoryLexicalAdapter,
        embedding: &'a dyn EmbeddingPort,
        model: EmbeddingModel,
    ) -> Self {
        Self {
            source,
            strategy,
            repository,
            embedding,
            model,
            index: RefCell::new(None),
        }
    }

    pub fn build(&self) -> Result<(), RetrievalError> {
        let documents = self.repository.collect_fragments()?;
        if documents.is_empty() {
            return Err(RetrievalError::InvalidResult);
        }
        let request = EmbeddingRequest {
            scope: self.repository.scope.clone(),
            fragments: documents
                .iter()
                .map(|(fragment, _)| fragment.clone())
                .collect(),
            model: EmbeddingModelRequirement::Exact(self.model.clone()),
        };
        let embedding = self.embedding.embed(&request)?;
        let expected = EmbeddingIndexMetadata {
            scope: request.scope,
            model: self.model.clone(),
        };
        expected.ensure_compatible(embedding.metadata())?;
        let mut sources = request.fragments;
        sources.sort_by(|a, b| a.id.cmp(&b.id));
        if embedding.sources() != sources {
            return Err(RetrievalError::IncompatibleEmbedding);
        }
        let locations = documents
            .into_iter()
            .map(|(_, location)| location)
            .collect();
        *self.index.borrow_mut() = Some(IndexedCorpus {
            embedding,
            locations,
        });
        Ok(())
    }

    pub fn rebuild(&self) -> Result<(), RetrievalError> {
        self.build()
    }
    pub fn invalidate(&self) {
        *self.index.borrow_mut() = None;
    }
    pub fn metadata(&self) -> Option<EmbeddingIndexMetadata> {
        self.index
            .borrow()
            .as_ref()
            .map(|index| index.embedding.metadata().clone())
    }
}

impl RetrievalSourceAdapter for VectorIndexAdapter<'_> {
    fn source(&self) -> &RetrievalSourceId {
        &self.source
    }
    fn strategy(&self) -> &RetrievalStrategyId {
        &self.strategy
    }
    fn retrieve(&self, request: &RetrievalRequest) -> Result<Vec<HybridCandidate>, RetrievalError> {
        if request.input().scope != self.repository.scope {
            return Err(RetrievalError::ScopeMismatch);
        }
        if self.repository.sensitivity > request.input().required.maximum_sensitivity
            || !request
                .input()
                .required
                .accepted_trust
                .contains(&TrustClass::RetrievedContent)
        {
            return Ok(Vec::new());
        }
        let index = self.index.borrow();
        let index = index.as_ref().ok_or(RetrievalError::ServiceUnavailable)?;
        let current = self.repository.collect_fragments()?;
        let mut current_sources: Vec<_> =
            current.into_iter().map(|(fragment, _)| fragment).collect();
        current_sources.sort_by(|a, b| a.id.cmp(&b.id));
        if current_sources != index.embedding.sources() {
            return Err(RetrievalError::StaleIndex);
        }
        let expected = EmbeddingIndexMetadata {
            scope: self.repository.scope.clone(),
            model: self.model.clone(),
        };
        expected.ensure_compatible(index.embedding.metadata())?;
        let mut scores = std::collections::BTreeMap::new();
        for query in &request.input().queries {
            let content = query.0.as_str();
            let query_fragment = RetrievedFragment {
                id: ReferenceId::new(format!("query-{}", digest(content.as_bytes()).as_str()))
                    .map_err(|_| RetrievalError::InvalidResult)?,
                scope: self.repository.scope.clone(),
                content: text(content.to_owned())?,
                provenance: Provenance::new(
                    ProvenanceId::new("retrieval-query")
                        .map_err(|_| RetrievalError::InvalidResult)?,
                    SourceKind::Caller,
                    SourceId::new("query").map_err(|_| RetrievalError::InvalidResult)?,
                    "query",
                )
                .map_err(|_| RetrievalError::InvalidResult)?,
                snapshot: digest(content.as_bytes()),
                quality: QualityMetadata::new(
                    TrustClass::CallerInput,
                    SensitivityClass::Public,
                    Confidence::Unknown,
                    FreshnessStatus::Fresh,
                    Uncertainty::None,
                ),
                evidence: BTreeSet::new(),
            };
            let embedding_request = EmbeddingRequest {
                scope: self.repository.scope.clone(),
                fragments: vec![query_fragment.clone()],
                model: EmbeddingModelRequirement::Exact(self.model.clone()),
            };
            let query_embedding = self.embedding.embed(&embedding_request)?;
            expected.ensure_compatible(query_embedding.metadata())?;
            if query_embedding.sources() != [query_fragment] {
                return Err(RetrievalError::IncompatibleEmbedding);
            }
            let query_vector = &query_embedding.vectors()[0].values;
            for vector in index.embedding.vectors() {
                let score = cosine_score(query_vector, &vector.values);
                scores
                    .entry(vector.fragment.clone())
                    .and_modify(|old: &mut u32| *old = (*old).max(score))
                    .or_insert(score);
            }
        }
        let mut results = Vec::new();
        for (fragment, location) in index.embedding.sources().iter().zip(&index.locations) {
            let score = scores.get(&fragment.id).copied().unwrap_or(0);
            if score == 0 {
                continue;
            }
            results.push(HybridCandidate::new(
                RetrievalResult {
                    fragment: fragment.clone(),
                    source: self.source.clone(),
                    strategy: self.strategy.clone(),
                    score: i64::from(score),
                },
                Some(location.clone()),
                false,
                None,
                Some(score),
            )?);
        }
        Ok(results)
    }
}

fn cosine_score(left: &[f32], right: &[f32]) -> u32 {
    let (mut dot, mut left_norm, mut right_norm) = (0f64, 0f64, 0f64);
    for (a, b) in left.iter().zip(right) {
        let (a, b) = (f64::from(*a), f64::from(*b));
        dot += a * b;
        left_norm += a * a;
        right_norm += b * b;
    }
    if left_norm == 0.0 || right_norm == 0.0 {
        return 0;
    }
    ((dot / (left_norm.sqrt() * right_norm.sqrt())).clamp(0.0, 1.0) * 1_000_000.0).round() as u32
}

/// One bounded retrieval round over explicitly registered source adapters.
/// Cumulative usage enters and leaves every call; callers own result selection
/// and evidence sufficiency across rounds.
pub struct FederatedRetrievalPort<'a> {
    pub adapters: Vec<&'a dyn RetrievalSourceAdapter>,
    pub fusion: FusionPolicy,
    pub reranker: Option<&'a dyn RetrievalReranker>,
}

impl KnowledgeRetrievalPort for FederatedRetrievalPort<'_> {
    fn retrieve_measured(
        &self,
        plan: &RetrievalPlan,
        round: RetrievalRound,
        usage: &BudgetUsage,
    ) -> Result<RetrievalBatch, RetrievalAttemptFailure> {
        let started = Instant::now();
        self.retrieve(plan, round, usage).map_err(|error| {
            let mut consumed = usage.clone();
            consumed.rounds = round.0.get();
            consumed.elapsed_ms = consumed
                .elapsed_ms
                .saturating_add(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX));
            let query_bytes = plan
                .request()
                .input()
                .queries
                .iter()
                .fold(0u64, |sum, query| {
                    sum.saturating_add(query.0.as_str().len() as u64)
                });
            consumed.tokens = consumed.tokens.saturating_add(query_bytes);
            RetrievalAttemptFailure {
                error,
                usage: consumed,
            }
        })
    }
    fn retrieve(
        &self,
        plan: &RetrievalPlan,
        round: RetrievalRound,
        usage: &BudgetUsage,
    ) -> Result<RetrievalBatch, RetrievalError> {
        let input = plan.request().input();
        if input.version == RetrievalVersion::V1 && input.budget.rounds.0.get() != 1 {
            return Err(RetrievalError::InvalidPlan);
        }
        if round.0.get()
            != usage
                .rounds
                .checked_add(1)
                .ok_or(RetrievalError::ArithmeticOverflow)?
            || round.0.get() > input.budget.rounds.0.get()
        {
            return Err(RetrievalError::InvalidPlan);
        }
        usage.validate(&input.budget)?;
        if plan.should_stop(usage, &BTreeSet::new())? {
            return Err(RetrievalError::BudgetExceeded);
        }
        let query_bytes = input.queries.iter().try_fold(0u64, |sum, query| {
            sum.checked_add(query.0.as_str().len() as u64)
                .ok_or(RetrievalError::ArithmeticOverflow)
        })?;
        if usage
            .tokens
            .checked_add(query_bytes)
            .is_none_or(|total| total > input.budget.tokens.0)
        {
            return Err(RetrievalError::BudgetExceeded);
        }
        let reserved_context = input
            .budget
            .context
            .reservations()
            .get(&ContextBudgetClass::Knowledge)
            .map_or(0, |value| value.0);
        let used_context = usage
            .context
            .get(&ContextBudgetClass::Knowledge)
            .copied()
            .unwrap_or(0);
        if used_context >= reserved_context {
            return Err(RetrievalError::BudgetExceeded);
        }
        let started = Instant::now();
        let gathered = federate(plan, &self.adapters);
        let mut explanations = BTreeSet::new();
        for (source, strategy, reason) in &gathered.failures {
            let source_optional = input
                .sources
                .iter()
                .any(|item| &item.id == source && item.optional);
            let strategy_optional = input
                .strategies
                .iter()
                .any(|item| &item.id == strategy && item.optional);
            if !source_optional && !strategy_optional {
                return Err(*reason);
            }
            let target = if source_optional {
                RetrievalExplanationTarget::Source(source.clone())
            } else {
                RetrievalExplanationTarget::Strategy(strategy.clone())
            };
            explanations.insert(RetrievalExplanation {
                target,
                selected: false,
                reason: RetrievalReason::ServiceUnavailable,
                detail: text(format!("{reason:?}: {source:?}/{strategy:?}"))?,
            });
        }
        let fused = fuse_candidates(gathered.candidates, self.fusion)?;
        let ranked = match self.reranker {
            Some(reranker) => rerank(fused, reranker),
            None => gateway_application::retrieval_pipeline::RerankOutcome {
                ranked: fused
                    .into_iter()
                    .map(
                        |candidate| gateway_application::retrieval_pipeline::RankedCandidate {
                            score: candidate.result().score as u32,
                            candidate,
                            score_origin: ScoreOrigin::Fusion,
                        },
                    )
                    .collect(),
                failure: None,
            },
        };
        if let Some(failure) = ranked.failure {
            explanations.insert(RetrievalExplanation {
                target: RetrievalExplanationTarget::Reranker,
                selected: false,
                reason: RetrievalReason::ServiceUnavailable,
                detail: text(format!("optional reranker: {failure:?}"))?,
            });
        }
        let reranker_failed = ranked.failure.is_some();
        let mut accepted = Vec::new();
        let mut budget_filtered = false;
        let mut context = usage.context.clone();
        let mut tokens = usage.tokens;
        tokens = tokens
            .checked_add(query_bytes)
            .ok_or(RetrievalError::ArithmeticOverflow)?;
        let available_context = input
            .budget
            .context
            .reservations()
            .get(&ContextBudgetClass::Knowledge)
            .map_or(0, |value| value.0);
        let mut context_used = context
            .get(&ContextBudgetClass::Knowledge)
            .copied()
            .unwrap_or(0);
        for mut ranked in ranked.ranked {
            if usage
                .results
                .checked_add(accepted.len() as u64)
                .is_none_or(|count| count >= input.budget.results.0.get())
            {
                budget_filtered = true;
                break;
            }
            let bytes = ranked.candidate.result().fragment.content.as_str().len() as u64;
            let next_tokens = tokens
                .checked_add(bytes)
                .ok_or(RetrievalError::ArithmeticOverflow)?;
            let next_context = context_used
                .checked_add(bytes)
                .ok_or(RetrievalError::ArithmeticOverflow)?;
            if next_tokens > input.budget.tokens.0 || next_context > available_context {
                budget_filtered = true;
                continue;
            }
            tokens = next_tokens;
            context_used = next_context;
            let final_score = if ranked.candidate.exact_match {
                1_000_000
            } else {
                ranked.score.min(999_999)
            };
            ranked.candidate.set_final_score(final_score)?;
            for contributor in ranked.candidate.contributors() {
                if contributor.fragment.id == ranked.candidate.result().fragment.id {
                    continue;
                }
                explanations.insert(RetrievalExplanation {
                    target: RetrievalExplanationTarget::Result(contributor.fragment.id.clone()),
                    selected: false,
                    reason: RetrievalReason::Duplicate,
                    detail: text("identical normalized content".to_owned())?,
                });
            }
            let mut detail = match &ranked.score_origin {
                ScoreOrigin::Fusion => format!("hybrid fusion score {final_score}"),
                ScoreOrigin::Model { id, version } => format!(
                    "reranked by {}@{}: model score {}; final score {final_score}",
                    id.as_str(),
                    version.as_str(),
                    ranked.score
                ),
            };
            for contributor in ranked.candidate.contributors() {
                detail.push_str(&format!(
                    "; duplicate from {} via {}",
                    contributor.source.as_str(),
                    contributor.strategy.as_str()
                ));
            }
            for path in &ranked.candidate.graph_paths {
                detail.push_str(&format!("; graph root {}", path.root.id.as_str()));
                for step in &path.steps {
                    detail.push_str(&format!(
                        " -> {} [{:?}, {:?}, source {}, snapshot {}, uncertainty {}]",
                        step.node.id.as_str(),
                        step.edge.relation,
                        step.edge.basis,
                        step.edge.provenance.source_reference(),
                        step.edge.snapshot.as_str(),
                        step.edge.quality.uncertainty().as_str(),
                    ));
                }
            }
            explanations.insert(RetrievalExplanation {
                target: RetrievalExplanationTarget::Result(
                    ranked.candidate.result().fragment.id.clone(),
                ),
                selected: true,
                reason: RetrievalReason::Relevant,
                detail: text(detail)?,
            });
            accepted.push(ranked.candidate.result().clone());
        }
        context.insert(ContextBudgetClass::Knowledge, context_used);
        let elapsed_ms = started
            .elapsed()
            .as_millis()
            .try_into()
            .map_err(|_| RetrievalError::ArithmeticOverflow)?;
        let final_usage = BudgetUsage {
            results: usage
                .results
                .checked_add(accepted.len() as u64)
                .ok_or(RetrievalError::ArithmeticOverflow)?,
            rounds: round.0.get(),
            elapsed_ms: usage
                .elapsed_ms
                .checked_add(elapsed_ms)
                .ok_or(RetrievalError::ArithmeticOverflow)?,
            cost: usage.cost,
            cost_unit: usage.cost_unit.clone(),
            tokens,
            context,
        };
        final_usage.validate(&input.budget)?;
        let degraded = !gathered.failures.is_empty() || reranker_failed;
        let (status, reason) = if degraded {
            (
                RetrievalStatus::Degraded,
                RetrievalReason::ServiceUnavailable,
            )
        } else if round.0.get() < input.budget.rounds.0.get()
            && !plan.should_stop(&final_usage, &BTreeSet::new())?
        {
            (
                RetrievalStatus::Partial,
                RetrievalReason::MoreInformationNeeded,
            )
        } else if accepted.is_empty() && !budget_filtered {
            (RetrievalStatus::Complete, RetrievalReason::NoMatches)
        } else {
            (RetrievalStatus::Complete, RetrievalReason::BudgetReached)
        };
        RetrievalBatch::new(
            RetrievalBatchInput {
                version: plan.version(),
                plan: plan.id().clone(),
                scope: input.scope.clone(),
                round,
                status,
                reason,
                results: accepted,
                usage: final_usage,
                explanations,
            },
            plan,
        )
    }
}
