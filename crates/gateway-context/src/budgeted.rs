//! Deterministic, token bounded selection of external context data.
use crate::{ContextFragment, FragmentKind};
use gateway_domain::{
    ContextBudget, ContextBudgetClass, NonEmptyText, ReferenceId, TokenCount, TokenEstimate,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionError {
    InvalidEstimate(ReferenceId),
    InvalidSectionEstimate(ContextBudgetClass),
    DuplicateId(ReferenceId),
    InvalidCompaction(ReferenceId),
    QuarantinedMandatory(ReferenceId),
    MissingMandatory(ReferenceId),
    MandatoryOverBudget(ContextBudgetClass),
    TotalOverBudget,
    ArithmeticOverflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionReason {
    Selected,
    Redundant,
    BudgetExceeded,
    ReplacedByCompaction,
    Quarantined,
}
impl SelectionReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Selected => "CONTEXT_SELECTED",
            Self::Redundant => "CONTEXT_REDUNDANT",
            Self::BudgetExceeded => "CONTEXT_BUDGET_EXCEEDED",
            Self::ReplacedByCompaction => "CONTEXT_REPLACED_BY_COMPACTION",
            Self::Quarantined => "CONTEXT_QUARANTINED",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionDecision {
    pub id: ReferenceId,
    pub reason: SelectionReason,
}

/// A derived alternative must retain every source ID in `sources`. Only knowledge
/// and memory with identical provenance and quality may be compacted together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactedCandidate {
    pub fragment: ContextFragment,
    pub sources: BTreeSet<ReferenceId>,
    pub estimate: TokenEstimate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankedFragment {
    pub fragment: ContextFragment,
    pub score: u32,
    pub mandatory: bool,
    pub estimate: TokenEstimate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetedSelection {
    pub fragments: Vec<ContextFragment>,
    pub selected: BTreeSet<ReferenceId>,
    pub decisions: Vec<SelectionDecision>,
    pub usage: BTreeMap<ContextBudgetClass, u64>,
    pub lineage: BTreeMap<ReferenceId, BTreeSet<ReferenceId>>,
    pub estimates: BTreeMap<ReferenceId, TokenEstimate>,
}

pub fn fragment_budget_class(kind: FragmentKind) -> ContextBudgetClass {
    match kind {
        FragmentKind::Knowledge => ContextBudgetClass::Knowledge,
        FragmentKind::Evidence => ContextBudgetClass::Evidence,
        FragmentKind::Memory => ContextBudgetClass::Memory,
        FragmentKind::UserInput => ContextBudgetClass::TaskReserved,
        _ => ContextBudgetClass::AuthorityReserved,
    }
}

fn bound(estimate: &TokenEstimate, target: &NonEmptyText) -> Option<u64> {
    match &estimate.count {
        TokenCount::Exact {
            tokens,
            target: actual,
        } if actual == target => Some(*tokens),
        TokenCount::Estimated {
            tokens,
            upper_bound: Some(upper),
            ..
        } if upper >= tokens => Some(*upper),
        _ => None,
    }
}

/// Section estimates cover fixed gateway content. Authority, task and output
/// sections are mandatory, and the full safety margin is held unused.
pub fn select_context(
    budget: &ContextBudget,
    target: &NonEmptyText,
    sections: &BTreeMap<ContextBudgetClass, TokenEstimate>,
    candidates: &[RankedFragment],
    compacted: &[CompactedCandidate],
    required: &BTreeSet<ReferenceId>,
) -> Result<BudgetedSelection, SelectionError> {
    select_context_with_exclusions(
        budget,
        target,
        sections,
        candidates,
        compacted,
        required,
        &BTreeSet::new(),
    )
}

/// The host supplies quarantined IDs from its authenticated contamination
/// decision. A compacted artifact cannot launder an excluded source back in.
pub fn select_context_with_exclusions(
    budget: &ContextBudget,
    target: &NonEmptyText,
    sections: &BTreeMap<ContextBudgetClass, TokenEstimate>,
    candidates: &[RankedFragment],
    compacted: &[CompactedCandidate],
    required: &BTreeSet<ReferenceId>,
    excluded: &BTreeSet<ReferenceId>,
) -> Result<BudgetedSelection, SelectionError> {
    use ContextBudgetClass::{
        AuthorityReserved, OutputContractReserved, SafetyMargin, TaskReserved,
    };
    let mut usage = BTreeMap::new();
    for section in [AuthorityReserved, TaskReserved, OutputContractReserved] {
        let estimate = sections
            .get(&section)
            .ok_or(SelectionError::InvalidSectionEstimate(section))?;
        let count =
            bound(estimate, target).ok_or(SelectionError::InvalidSectionEstimate(section))?;
        usage.insert(section, count);
    }
    for (&section, estimate) in sections {
        if matches!(
            section,
            AuthorityReserved | TaskReserved | OutputContractReserved | SafetyMargin
        ) {
            continue;
        }
        let count =
            bound(estimate, target).ok_or(SelectionError::InvalidSectionEstimate(section))?;
        usage.insert(section, count);
    }
    usage.insert(
        SafetyMargin,
        budget.reservations().get(&SafetyMargin).map_or(0, |v| v.0),
    );
    if budget.validate_usage(&usage).is_err() {
        return Err(SelectionError::MandatoryOverBudget(
            usage
                .iter()
                .find(|(c, n)| **n > budget.reservations().get(c).map_or(0, |v| v.0))
                .map_or(AuthorityReserved, |(c, _)| *c),
        ));
    }

    let mut by_id = BTreeMap::new();
    for item in candidates {
        if let Some(previous) = by_id.insert(item.fragment.id().clone(), item) {
            if previous != item {
                return Err(SelectionError::DuplicateId(item.fragment.id().clone()));
            }
        }
    }
    let mut compacted_ids = BTreeSet::new();
    for artifact in compacted {
        let id = artifact.fragment.id();
        if !compacted_ids.insert(id.clone()) {
            return Err(SelectionError::InvalidCompaction(id.clone()));
        }
        if bound(&artifact.estimate, target).is_none() {
            return Err(SelectionError::InvalidEstimate(id.clone()));
        }
        if by_id.contains_key(id)
            || excluded.contains(id)
            || artifact.sources.is_empty()
            || !artifact.sources.is_disjoint(excluded)
            || !matches!(
                artifact.fragment.kind(),
                FragmentKind::Knowledge | FragmentKind::Memory
            )
            || artifact.sources.iter().any(|source| {
                by_id.get(source).is_none_or(|original| {
                    original.mandatory
                        || original.fragment.kind() != artifact.fragment.kind()
                        || original.fragment.metadata() != artifact.fragment.metadata()
                        || original.fragment.scope() != artifact.fragment.scope()
                        || original.fragment.step() != artifact.fragment.step()
                        || original.fragment.is_reference() != artifact.fragment.is_reference()
                })
            })
        {
            return Err(SelectionError::InvalidCompaction(id.clone()));
        }
    }
    for id in required {
        if !by_id.contains_key(id) {
            return Err(SelectionError::MissingMandatory(id.clone()));
        }
        if excluded.contains(id) {
            return Err(SelectionError::QuarantinedMandatory(id.clone()));
        }
    }

    let mut order: Vec<_> = by_id.values().copied().collect();
    order.sort_by(|a, b| {
        (b.mandatory || required.contains(b.fragment.id()))
            .cmp(&(a.mandatory || required.contains(a.fragment.id())))
            .then(b.score.cmp(&a.score))
            .then(a.fragment.id().cmp(b.fragment.id()))
    });
    let mut chosen = Vec::new();
    let mut selected = BTreeSet::new();
    let mut decisions = Vec::new();
    let mut lineage = BTreeMap::new();
    let mut estimates = BTreeMap::new();
    let mut consumed = BTreeSet::new();
    for item in order {
        let id = item.fragment.id().clone();
        if excluded.contains(&id) {
            if item.mandatory {
                return Err(SelectionError::QuarantinedMandatory(id));
            }
            decisions.push(SelectionDecision {
                id,
                reason: SelectionReason::Quarantined,
            });
            continue;
        }
        if consumed.contains(&id) {
            continue;
        }
        let category = fragment_budget_class(item.fragment.kind());
        let amount = bound(&item.estimate, target)
            .ok_or_else(|| SelectionError::InvalidEstimate(id.clone()))?;
        let mandatory = item.mandatory || required.contains(&id);
        let redundant = !mandatory
            && chosen.iter().any(|f: &ContextFragment| {
                f.kind() == item.fragment.kind()
                    && f.content() == item.fragment.content()
                    && f.metadata() == item.fragment.metadata()
                    && f.is_reference() == item.fragment.is_reference()
            });
        if redundant {
            decisions.push(SelectionDecision {
                id,
                reason: SelectionReason::Redundant,
            });
            continue;
        }
        let current = usage.get(&category).copied().unwrap_or(0);
        let next = current
            .checked_add(amount)
            .ok_or(SelectionError::ArithmeticOverflow)?;
        if next > budget.reservations().get(&category).map_or(0, |v| v.0) {
            if mandatory {
                return Err(SelectionError::MandatoryOverBudget(category));
            }
            let alternative = compacted
                .iter()
                .filter(|c| c.sources.contains(&id) && c.sources.is_disjoint(&consumed))
                .filter_map(|c| bound(&c.estimate, target).map(|tokens| (c, tokens)))
                .filter(|(_, tokens)| {
                    current.checked_add(*tokens).is_some_and(|n| {
                        n <= budget.reservations().get(&category).map_or(0, |v| v.0)
                    })
                })
                .min_by(|(a, ta), (b, tb)| ta.cmp(tb).then(a.fragment.id().cmp(b.fragment.id())));
            if let Some((artifact, tokens)) = alternative {
                let compact_id = artifact.fragment.id().clone();
                usage.insert(category, current + tokens);
                selected.insert(compact_id.clone());
                chosen.push(artifact.fragment.clone());
                lineage.insert(compact_id.clone(), artifact.sources.clone());
                estimates.insert(compact_id.clone(), artifact.estimate.clone());
                for source in &artifact.sources {
                    consumed.insert(source.clone());
                    decisions.push(SelectionDecision {
                        id: source.clone(),
                        reason: SelectionReason::ReplacedByCompaction,
                    });
                }
                decisions.push(SelectionDecision {
                    id: compact_id,
                    reason: SelectionReason::Selected,
                });
            } else {
                decisions.push(SelectionDecision {
                    id,
                    reason: SelectionReason::BudgetExceeded,
                });
            }
            continue;
        }
        usage.insert(category, next);
        selected.insert(id.clone());
        consumed.insert(id.clone());
        lineage.insert(id.clone(), BTreeSet::from([id.clone()]));
        estimates.insert(id.clone(), item.estimate.clone());
        chosen.push(item.fragment.clone());
        decisions.push(SelectionDecision {
            id,
            reason: SelectionReason::Selected,
        });
    }
    chosen.sort_by(|a, b| (a.kind(), a.id()).cmp(&(b.kind(), b.id())));
    decisions.sort_by(|a, b| {
        a.id.cmp(&b.id)
            .then((a.reason as u8).cmp(&(b.reason as u8)))
    });
    Ok(BudgetedSelection {
        fragments: chosen,
        selected,
        decisions,
        usage,
        lineage,
        estimates,
    })
}
