//! CG-07.02 deterministic DesiredState-to-CurrentState comparison semantics.
//!
//! The comparison layer is deliberately pure.  It consumes the typed CG-06
//! desired-state and observed-state contracts, never reads ambient time,
//! performs no source selection and has no authority to mutate anything.

use std::{cmp::Ordering, fmt, str::FromStr};

use crate::{
    declarative_context::CurrentState,
    identifiers::{
        ConditionId, DesiredStateId, EvidenceId, FactId, ObservationId, ObservedStateId,
        ProvenanceId,
    },
    intent::{
        ComparisonOperator, ConditionExpression, DecimalValue, DesiredCondition, DesiredState,
        SubjectPath, TypedValue,
    },
    normalization::{NormalizedStateEntry, StateStatus},
    observation::AssertionPolarity,
    quality::{ConflictStatus, FreshnessStatus, Uncertainty},
    validation::ValidationError,
    version::SchemaVersion,
};

/// The currently supported deterministic comparison semantics version.
pub const COMPARISON_SEMANTICS_VERSION: ComparisonSemanticsVersion = ComparisonSemanticsVersion::V1;

/// A version of the DesiredState-to-CurrentState comparison algebra.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct ComparisonSemanticsVersion(SchemaVersion);

impl ComparisonSemanticsVersion {
    /// The first supported comparison semantics version.
    pub const V1: Self = Self(SchemaVersion::V1);

    /// Creates a syntactically valid comparison semantics version.
    pub fn new(major: u16, minor: u16) -> Result<Self, ValidationError> {
        SchemaVersion::new(major, minor).map(Self)
    }

    /// Returns the major version component.
    #[must_use]
    pub const fn major(self) -> u16 {
        self.0.major()
    }

    /// Returns the minor version component.
    #[must_use]
    pub const fn minor(self) -> u16 {
        self.0.minor()
    }

    /// Rejects semantics versions that this implementation does not know.
    pub fn ensure_supported(self) -> Result<(), ValidationError> {
        if self == Self::V1 {
            Ok(())
        } else {
            Err(ValidationError::UnsupportedSchemaVersion {
                expected: "1.0",
                actual: self.to_string(),
            })
        }
    }
}

impl fmt::Display for ComparisonSemanticsVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for ComparisonSemanticsVersion {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        SchemaVersion::from_str(value).map(Self)
    }
}

/// Explicit options that influence comparison without introducing ambient state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct ComparisonRules {
    version: ComparisonSemanticsVersion,
    require_fresh_evidence: bool,
}

impl ComparisonRules {
    /// Creates rules after validating the comparison semantics version.
    pub fn new(version: ComparisonSemanticsVersion) -> Result<Self, ValidationError> {
        version.ensure_supported()?;
        Ok(Self {
            version,
            require_fresh_evidence: false,
        })
    }

    /// Creates the supported v1 rules with no implicit freshness requirement.
    #[must_use]
    pub const fn v1() -> Self {
        Self {
            version: COMPARISON_SEMANTICS_VERSION,
            require_fresh_evidence: false,
        }
    }

    /// Requires explicit fresh quality metadata for known current values.
    #[must_use]
    pub const fn requiring_fresh_evidence(mut self, required: bool) -> Self {
        self.require_fresh_evidence = required;
        self
    }

    /// Returns the comparison semantics version.
    #[must_use]
    pub const fn version(self) -> ComparisonSemanticsVersion {
        self.version
    }

    /// Returns whether stale or unknown freshness is insufficient evidence.
    #[must_use]
    pub const fn requires_fresh_evidence(self) -> bool {
        self.require_fresh_evidence
    }
}

impl Default for ComparisonRules {
    fn default() -> Self {
        Self::v1()
    }
}

/// The semantic result of one condition or finite logical expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum ComparisonOutcome {
    /// The desired condition is definitely satisfied.
    Satisfied,
    /// The desired condition is definitely not satisfied.
    Unsatisfied,
    /// The current state does not establish a value either way.
    Unknown,
    /// Current assertions or quality metadata conflict.
    Conflicted,
    /// Required evidence or explicit freshness is absent.
    InsufficientEvidence,
    /// An explicit caller input is needed before comparison can proceed.
    UnresolvedInput,
    /// The typed values or operation cannot be compared under v1 rules.
    Incomparable,
}

impl ComparisonOutcome {
    /// Returns the stable machine-readable outcome name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Satisfied => "SATISFIED",
            Self::Unsatisfied => "UNSATISFIED",
            Self::Unknown => "UNKNOWN",
            Self::Conflicted => "CONFLICTED",
            Self::InsufficientEvidence => "INSUFFICIENT_EVIDENCE",
            Self::UnresolvedInput => "UNRESOLVED_INPUT",
            Self::Incomparable => "INCOMPARABLE",
        }
    }
}

impl fmt::Display for ComparisonOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ComparisonOutcome {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "SATISFIED" => Ok(Self::Satisfied),
            "UNSATISFIED" => Ok(Self::Unsatisfied),
            "UNKNOWN" => Ok(Self::Unknown),
            "CONFLICTED" => Ok(Self::Conflicted),
            "INSUFFICIENT_EVIDENCE" => Ok(Self::InsufficientEvidence),
            "UNRESOLVED_INPUT" => Ok(Self::UnresolvedInput),
            "INCOMPARABLE" => Ok(Self::Incomparable),
            value => Err(ValidationError::UnknownDomainValue {
                field: "comparison_outcome",
                value: value.to_owned(),
            }),
        }
    }
}

/// Stable reason codes accompanying comparison outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum ComparisonReasonCode {
    ValueMatches,
    ValueDoesNotMatch,
    SubjectNotObserved,
    StateUnknown,
    StateConflict,
    MissingEvidence,
    StaleEvidence,
    FreshnessUnknown,
    IncompleteInformation,
    IncompatibleTypes,
    UnsupportedOperation,
    NegatedAssertionNotComparable,
    ExpressionSatisfied,
    ExpressionUnsatisfied,
    ExpressionUnknown,
    ExpressionConflict,
    ExpressionInsufficientEvidence,
    ExpressionUnresolvedInput,
    ExpressionIncomparable,
}

impl ComparisonReasonCode {
    /// Returns the stable machine-readable reason code.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ValueMatches => "VALUE_MATCHES",
            Self::ValueDoesNotMatch => "VALUE_DOES_NOT_MATCH",
            Self::SubjectNotObserved => "SUBJECT_NOT_OBSERVED",
            Self::StateUnknown => "STATE_UNKNOWN",
            Self::StateConflict => "STATE_CONFLICT",
            Self::MissingEvidence => "MISSING_EVIDENCE",
            Self::StaleEvidence => "STALE_EVIDENCE",
            Self::FreshnessUnknown => "FRESHNESS_UNKNOWN",
            Self::IncompleteInformation => "INCOMPLETE_INFORMATION",
            Self::IncompatibleTypes => "INCOMPATIBLE_TYPES",
            Self::UnsupportedOperation => "UNSUPPORTED_OPERATION",
            Self::NegatedAssertionNotComparable => "NEGATED_ASSERTION_NOT_COMPARABLE",
            Self::ExpressionSatisfied => "EXPRESSION_SATISFIED",
            Self::ExpressionUnsatisfied => "EXPRESSION_UNSATISFIED",
            Self::ExpressionUnknown => "EXPRESSION_UNKNOWN",
            Self::ExpressionConflict => "EXPRESSION_CONFLICT",
            Self::ExpressionInsufficientEvidence => "EXPRESSION_INSUFFICIENT_EVIDENCE",
            Self::ExpressionUnresolvedInput => "EXPRESSION_UNRESOLVED_INPUT",
            Self::ExpressionIncomparable => "EXPRESSION_INCOMPARABLE",
        }
    }
}

impl fmt::Display for ComparisonReasonCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ComparisonReasonCode {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        const VALUES: [ComparisonReasonCode; 19] = [
            ComparisonReasonCode::ValueMatches,
            ComparisonReasonCode::ValueDoesNotMatch,
            ComparisonReasonCode::SubjectNotObserved,
            ComparisonReasonCode::StateUnknown,
            ComparisonReasonCode::StateConflict,
            ComparisonReasonCode::MissingEvidence,
            ComparisonReasonCode::StaleEvidence,
            ComparisonReasonCode::FreshnessUnknown,
            ComparisonReasonCode::IncompleteInformation,
            ComparisonReasonCode::IncompatibleTypes,
            ComparisonReasonCode::UnsupportedOperation,
            ComparisonReasonCode::NegatedAssertionNotComparable,
            ComparisonReasonCode::ExpressionSatisfied,
            ComparisonReasonCode::ExpressionUnsatisfied,
            ComparisonReasonCode::ExpressionUnknown,
            ComparisonReasonCode::ExpressionConflict,
            ComparisonReasonCode::ExpressionInsufficientEvidence,
            ComparisonReasonCode::ExpressionUnresolvedInput,
            ComparisonReasonCode::ExpressionIncomparable,
        ];
        VALUES
            .into_iter()
            .find(|candidate| candidate.as_str() == value)
            .ok_or_else(|| ValidationError::UnknownDomainValue {
                field: "comparison_reason_code",
                value: value.to_owned(),
            })
    }
}

/// The comparison target retained in a result tree.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum ComparisonTarget {
    Condition(ConditionId),
    Expression(ConditionExpression),
}

impl ComparisonTarget {
    /// Returns a condition target.
    #[must_use]
    pub fn condition(id: ConditionId) -> Self {
        Self::Condition(id)
    }

    /// Returns an expression target.
    #[must_use]
    pub fn expression(expression: ConditionExpression) -> Self {
        Self::Expression(expression)
    }
}

/// Explicit values and lineage retained for explainability and Delta derivation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct ComparisonTrace {
    observed_state: ObservedStateId,
    subjects: Vec<SubjectPath>,
    desired_values: Vec<TypedValue>,
    current_values: Vec<TypedValue>,
    statuses: Vec<StateStatus>,
    polarities: Vec<AssertionPolarity>,
    facts: Vec<FactId>,
    observations: Vec<ObservationId>,
    evidence: Vec<EvidenceId>,
    provenances: Vec<ProvenanceId>,
}

impl ComparisonTrace {
    fn for_condition(
        observed_state: &ObservedStateId,
        subject: SubjectPath,
        desired_value: Option<&TypedValue>,
        entry: Option<&NormalizedStateEntry>,
    ) -> Self {
        let mut trace = Self {
            observed_state: observed_state.clone(),
            subjects: vec![subject],
            desired_values: desired_value.into_iter().cloned().collect(),
            current_values: Vec::new(),
            statuses: Vec::new(),
            polarities: Vec::new(),
            facts: Vec::new(),
            observations: Vec::new(),
            evidence: Vec::new(),
            provenances: Vec::new(),
        };
        if let Some(entry) = entry {
            trace.statuses.push(entry.status());
            if let Some(value) = entry.value() {
                trace.current_values.push(value.clone());
            }
            if let Some(polarity) = entry.polarity() {
                trace.polarities.push(polarity);
            }
            trace.facts.extend(entry.lineage().facts().iter().cloned());
            trace
                .observations
                .extend(entry.lineage().observations().iter().cloned());
            trace
                .evidence
                .extend(entry.lineage().evidence().iter().cloned());
            trace
                .provenances
                .extend(entry.lineage().provenances().iter().cloned());
        }
        trace.canonicalize();
        trace
    }

    fn from_children(children: &[ComparisonResult]) -> Self {
        let observed_state = children
            .first()
            .map(|child| child.trace.observed_state.clone())
            .expect("logical comparison expressions must have children");
        let mut trace = Self {
            observed_state,
            subjects: Vec::new(),
            desired_values: Vec::new(),
            current_values: Vec::new(),
            statuses: Vec::new(),
            polarities: Vec::new(),
            facts: Vec::new(),
            observations: Vec::new(),
            evidence: Vec::new(),
            provenances: Vec::new(),
        };
        for child in children {
            let child_trace = &child.trace;
            trace.subjects.extend(child_trace.subjects.iter().cloned());
            trace
                .desired_values
                .extend(child_trace.desired_values.iter().cloned());
            trace
                .current_values
                .extend(child_trace.current_values.iter().cloned());
            trace.statuses.extend(child_trace.statuses.iter().copied());
            trace
                .polarities
                .extend(child_trace.polarities.iter().copied());
            trace.facts.extend(child_trace.facts.iter().cloned());
            trace
                .observations
                .extend(child_trace.observations.iter().cloned());
            trace.evidence.extend(child_trace.evidence.iter().cloned());
            trace
                .provenances
                .extend(child_trace.provenances.iter().cloned());
        }
        trace.canonicalize();
        trace
    }

    fn canonicalize(&mut self) {
        sort_deduplicate(&mut self.subjects);
        sort_deduplicate(&mut self.desired_values);
        sort_deduplicate(&mut self.current_values);
        sort_deduplicate(&mut self.statuses);
        sort_deduplicate(&mut self.polarities);
        sort_deduplicate(&mut self.facts);
        sort_deduplicate(&mut self.observations);
        sort_deduplicate(&mut self.evidence);
        sort_deduplicate(&mut self.provenances);
    }

    /// Returns the observed-state snapshot identity.
    #[must_use]
    pub fn observed_state(&self) -> &ObservedStateId {
        &self.observed_state
    }

    /// Returns all compared subjects in canonical order.
    #[must_use]
    pub fn subjects(&self) -> &[SubjectPath] {
        &self.subjects
    }

    /// Returns desired values retained by this trace.
    #[must_use]
    pub fn desired_values(&self) -> &[TypedValue] {
        &self.desired_values
    }

    /// Returns current values retained by this trace.
    #[must_use]
    pub fn current_values(&self) -> &[TypedValue] {
        &self.current_values
    }

    /// Returns observed state statuses retained by this trace.
    #[must_use]
    pub fn statuses(&self) -> &[StateStatus] {
        &self.statuses
    }

    /// Returns assertion polarities retained by this trace.
    #[must_use]
    pub fn polarities(&self) -> &[AssertionPolarity] {
        &self.polarities
    }

    /// Returns source facts retained by this trace.
    #[must_use]
    pub fn facts(&self) -> &[FactId] {
        &self.facts
    }

    /// Returns source observations retained by this trace.
    #[must_use]
    pub fn observations(&self) -> &[ObservationId] {
        &self.observations
    }

    /// Returns source evidence retained by this trace.
    #[must_use]
    pub fn evidence(&self) -> &[EvidenceId] {
        &self.evidence
    }

    /// Returns source provenance retained by this trace.
    #[must_use]
    pub fn provenances(&self) -> &[ProvenanceId] {
        &self.provenances
    }
}

/// A complete deterministic result, including nested logical branches.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct ComparisonResult {
    version: ComparisonSemanticsVersion,
    desired_state: DesiredStateId,
    target: ComparisonTarget,
    outcome: ComparisonOutcome,
    reason: ComparisonReasonCode,
    trace: ComparisonTrace,
    children: Vec<Self>,
}

impl ComparisonResult {
    fn new(
        version: ComparisonSemanticsVersion,
        desired_state: DesiredStateId,
        target: ComparisonTarget,
        outcome: ComparisonOutcome,
        reason: ComparisonReasonCode,
        trace: ComparisonTrace,
        children: Vec<Self>,
    ) -> Self {
        Self {
            version,
            desired_state,
            target,
            outcome,
            reason,
            trace,
            children,
        }
    }

    /// Returns the comparison semantics version.
    #[must_use]
    pub const fn version(&self) -> ComparisonSemanticsVersion {
        self.version
    }

    /// Returns the DesiredState identity being evaluated.
    #[must_use]
    pub fn desired_state(&self) -> &DesiredStateId {
        &self.desired_state
    }

    /// Returns the condition or expression compared by this result node.
    #[must_use]
    pub const fn target(&self) -> &ComparisonTarget {
        &self.target
    }

    /// Returns the deterministic semantic outcome.
    #[must_use]
    pub const fn outcome(&self) -> ComparisonOutcome {
        self.outcome
    }

    /// Returns the stable reason code for the outcome.
    #[must_use]
    pub const fn reason(&self) -> ComparisonReasonCode {
        self.reason
    }

    /// Returns explicit values and lineage for this result tree.
    #[must_use]
    pub const fn trace(&self) -> &ComparisonTrace {
        &self.trace
    }

    /// Returns nested branch results in expression order.
    #[must_use]
    pub fn children(&self) -> &[Self] {
        &self.children
    }
}

/// Compares one declared DesiredCondition against a normalized CurrentState.
pub fn compare_condition(
    desired_state: &DesiredState,
    condition_id: &ConditionId,
    current_state: &CurrentState,
    rules: &ComparisonRules,
) -> Result<ComparisonResult, ValidationError> {
    rules.version.ensure_supported()?;
    desired_state.version().ensure_supported()?;
    current_state.version().ensure_supported()?;
    let condition = desired_state
        .conditions()
        .iter()
        .find(|candidate| candidate.id() == condition_id)
        .ok_or_else(|| ValidationError::MissingDeclarativeIdentity {
            kind: "condition",
            id: condition_id.to_string(),
        })?;
    compare_declared_condition(desired_state, condition, current_state, rules)
}

/// Alias emphasizing that the condition originates in a DesiredState.
pub fn compare_desired_condition(
    desired_state: &DesiredState,
    condition_id: &ConditionId,
    current_state: &CurrentState,
    rules: &ComparisonRules,
) -> Result<ComparisonResult, ValidationError> {
    compare_condition(desired_state, condition_id, current_state, rules)
}

/// Compares the complete finite DesiredState expression against CurrentState.
pub fn compare_desired_state(
    desired_state: &DesiredState,
    current_state: &CurrentState,
    rules: &ComparisonRules,
) -> Result<ComparisonResult, ValidationError> {
    rules.version.ensure_supported()?;
    desired_state.version().ensure_supported()?;
    current_state.version().ensure_supported()?;
    compare_expression(
        desired_state,
        desired_state.expression(),
        current_state,
        rules,
    )
}

fn compare_expression(
    desired_state: &DesiredState,
    expression: &ConditionExpression,
    current_state: &CurrentState,
    rules: &ComparisonRules,
) -> Result<ComparisonResult, ValidationError> {
    match expression {
        ConditionExpression::Condition(condition_id) => {
            compare_condition(desired_state, condition_id, current_state, rules)
        }
        ConditionExpression::All(expressions) => {
            let children = expressions
                .iter()
                .map(|expression| {
                    compare_expression(desired_state, expression, current_state, rules)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let outcome = fold_all(children.iter().map(|child| child.outcome()));
            Ok(ComparisonResult::new(
                rules.version,
                desired_state.id().clone(),
                ComparisonTarget::expression(expression.clone()),
                outcome,
                expression_reason(outcome),
                ComparisonTrace::from_children(&children),
                children,
            ))
        }
        ConditionExpression::Any(expressions) => {
            let children = expressions
                .iter()
                .map(|expression| {
                    compare_expression(desired_state, expression, current_state, rules)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let outcome = fold_any(children.iter().map(|child| child.outcome()));
            Ok(ComparisonResult::new(
                rules.version,
                desired_state.id().clone(),
                ComparisonTarget::expression(expression.clone()),
                outcome,
                expression_reason(outcome),
                ComparisonTrace::from_children(&children),
                children,
            ))
        }
        ConditionExpression::Not(inner) => {
            let child = compare_expression(desired_state, inner, current_state, rules)?;
            let outcome = negate_outcome(child.outcome());
            Ok(ComparisonResult::new(
                rules.version,
                desired_state.id().clone(),
                ComparisonTarget::expression(expression.clone()),
                outcome,
                expression_reason(outcome),
                ComparisonTrace::from_children(std::slice::from_ref(&child)),
                vec![child],
            ))
        }
    }
}

fn compare_declared_condition(
    desired_state: &DesiredState,
    condition: &DesiredCondition,
    current_state: &CurrentState,
    rules: &ComparisonRules,
) -> Result<ComparisonResult, ValidationError> {
    let matching_entries = current_state
        .entries()
        .iter()
        .filter(|entry| entry.subject() == condition.subject())
        .collect::<Vec<_>>();
    if matching_entries.len() > 1 {
        return Err(ValidationError::DuplicateDeclarativeIdentity {
            kind: "normalized_state.subject",
            id: condition.subject().to_string(),
        });
    }
    let entry = matching_entries.first().copied();
    let trace = ComparisonTrace::for_condition(
        current_state.id(),
        condition.subject().clone(),
        condition.expected(),
        entry,
    );
    let (outcome, reason) = if let Some(entry) = entry {
        validate_entry(entry)?;
        compare_entry(condition, entry, rules)
    } else {
        (
            ComparisonOutcome::Unknown,
            ComparisonReasonCode::SubjectNotObserved,
        )
    };
    Ok(ComparisonResult::new(
        rules.version,
        desired_state.id().clone(),
        ComparisonTarget::condition(condition.id().clone()),
        outcome,
        reason,
        trace,
        Vec::new(),
    ))
}

fn validate_entry(entry: &NormalizedStateEntry) -> Result<(), ValidationError> {
    let is_known = entry.status() == StateStatus::Known;
    if is_known != (entry.value().is_some() && entry.polarity().is_some()) {
        return Err(ValidationError::InvalidStateCombination {
            reason: "known comparison state must have value and polarity while other states must not",
        });
    }
    if entry.status() == StateStatus::Unknown && !entry.claims().is_empty() {
        return Err(ValidationError::InvalidStateCombination {
            reason: "unknown comparison state must not contain claims",
        });
    }
    if let Some(value) = entry.value() {
        value.validate()?;
    }
    Ok(())
}

fn compare_entry(
    condition: &DesiredCondition,
    entry: &NormalizedStateEntry,
    rules: &ComparisonRules,
) -> (ComparisonOutcome, ComparisonReasonCode) {
    match entry.status() {
        StateStatus::Unknown => (
            ComparisonOutcome::Unknown,
            ComparisonReasonCode::StateUnknown,
        ),
        StateStatus::Conflicted => (
            ComparisonOutcome::Conflicted,
            ComparisonReasonCode::StateConflict,
        ),
        StateStatus::Unsupported => (
            ComparisonOutcome::InsufficientEvidence,
            ComparisonReasonCode::MissingEvidence,
        ),
        StateStatus::Known => {
            if entry
                .metadata()
                .is_some_and(|metadata| metadata.conflict() == ConflictStatus::Unresolved)
            {
                return (
                    ComparisonOutcome::Conflicted,
                    ComparisonReasonCode::StateConflict,
                );
            }
            if let Some(metadata) = entry.metadata() {
                if metadata.uncertainty() != Uncertainty::None {
                    return (
                        ComparisonOutcome::InsufficientEvidence,
                        ComparisonReasonCode::IncompleteInformation,
                    );
                }
                if rules.require_fresh_evidence {
                    match metadata.freshness() {
                        FreshnessStatus::Fresh => {}
                        FreshnessStatus::Stale => {
                            return (
                                ComparisonOutcome::InsufficientEvidence,
                                ComparisonReasonCode::StaleEvidence,
                            );
                        }
                        FreshnessStatus::Unknown => {
                            return (
                                ComparisonOutcome::InsufficientEvidence,
                                ComparisonReasonCode::FreshnessUnknown,
                            );
                        }
                    }
                }
            } else if rules.require_fresh_evidence {
                return (
                    ComparisonOutcome::InsufficientEvidence,
                    ComparisonReasonCode::FreshnessUnknown,
                );
            }
            let value = entry
                .value()
                .expect("validate_entry ensures known values are present");
            match compare_value(condition, value, entry.polarity().expect("known polarity")) {
                ValueMatch::Match => (
                    ComparisonOutcome::Satisfied,
                    ComparisonReasonCode::ValueMatches,
                ),
                ValueMatch::Mismatch => (
                    ComparisonOutcome::Unsatisfied,
                    ComparisonReasonCode::ValueDoesNotMatch,
                ),
                ValueMatch::Incomparable(reason) => (ComparisonOutcome::Incomparable, reason),
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueMatch {
    Match,
    Mismatch,
    Incomparable(ComparisonReasonCode),
}

fn compare_value(
    condition: &DesiredCondition,
    actual: &TypedValue,
    polarity: AssertionPolarity,
) -> ValueMatch {
    let operator = condition.operator();
    if polarity == AssertionPolarity::Negated
        && matches!(
            operator,
            ComparisonOperator::Equals | ComparisonOperator::NotEquals
        )
    {
        let expected = condition
            .expected()
            .expect("equality operators require an expected value");
        return match typed_values_equal(actual, expected) {
            Some(true) => match operator {
                ComparisonOperator::Equals => ValueMatch::Mismatch,
                ComparisonOperator::NotEquals => ValueMatch::Match,
                _ => unreachable!("operator was checked above"),
            },
            Some(false) => {
                ValueMatch::Incomparable(ComparisonReasonCode::NegatedAssertionNotComparable)
            }
            None => ValueMatch::Incomparable(ComparisonReasonCode::IncompatibleTypes),
        };
    }
    if polarity == AssertionPolarity::Negated
        && !matches!(
            operator,
            ComparisonOperator::Present | ComparisonOperator::Absent
        )
    {
        return ValueMatch::Incomparable(ComparisonReasonCode::NegatedAssertionNotComparable);
    }

    match operator {
        ComparisonOperator::Present => ValueMatch::Match,
        ComparisonOperator::Absent => ValueMatch::Mismatch,
        ComparisonOperator::Equals => {
            let expected = condition
                .expected()
                .expect("equality operators require an expected value");
            match typed_values_equal(actual, expected) {
                Some(true) => ValueMatch::Match,
                Some(false) => ValueMatch::Mismatch,
                None => ValueMatch::Incomparable(ComparisonReasonCode::IncompatibleTypes),
            }
        }
        ComparisonOperator::NotEquals => {
            let expected = condition
                .expected()
                .expect("equality operators require an expected value");
            match typed_values_equal(actual, expected) {
                Some(true) => ValueMatch::Mismatch,
                Some(false) => ValueMatch::Match,
                None => ValueMatch::Incomparable(ComparisonReasonCode::IncompatibleTypes),
            }
        }
        ComparisonOperator::GreaterThan
        | ComparisonOperator::GreaterOrEqual
        | ComparisonOperator::LessThan
        | ComparisonOperator::LessOrEqual => {
            let expected = condition
                .expected()
                .expect("numeric operators require an expected value");
            match numeric_cmp(actual, expected) {
                Some(ordering) => ValueMatch::from(order_matches(operator, ordering)),
                None => ValueMatch::Incomparable(ComparisonReasonCode::IncompatibleTypes),
            }
        }
        ComparisonOperator::In => {
            let expected = condition.expected().expect("IN requires an expected value");
            let TypedValue::Set(values) = expected else {
                unreachable!("DesiredCondition validates IN set semantics")
            };
            let mut saw_compatible = false;
            for expected in values {
                match typed_values_equal(actual, expected) {
                    Some(true) => return ValueMatch::Match,
                    Some(false) => saw_compatible = true,
                    None => {}
                }
            }
            if saw_compatible {
                ValueMatch::Mismatch
            } else {
                ValueMatch::Incomparable(ComparisonReasonCode::IncompatibleTypes)
            }
        }
        ComparisonOperator::Contains => {
            let TypedValue::Set(values) = actual else {
                return ValueMatch::Incomparable(ComparisonReasonCode::UnsupportedOperation);
            };
            let expected = condition
                .expected()
                .expect("CONTAINS requires an expected value");
            if expected.kind().is_none() {
                return ValueMatch::Incomparable(ComparisonReasonCode::UnsupportedOperation);
            }
            let mut saw_compatible = false;
            for actual_value in values {
                match typed_values_equal(actual_value, expected) {
                    Some(true) => return ValueMatch::Match,
                    Some(false) => saw_compatible = true,
                    None => {}
                }
            }
            if saw_compatible {
                ValueMatch::Mismatch
            } else {
                ValueMatch::Incomparable(ComparisonReasonCode::IncompatibleTypes)
            }
        }
    }
}

impl From<bool> for ValueMatch {
    fn from(value: bool) -> Self {
        if value { Self::Match } else { Self::Mismatch }
    }
}

fn typed_values_equal(left: &TypedValue, right: &TypedValue) -> Option<bool> {
    match (left, right) {
        (TypedValue::Set(left), TypedValue::Set(right)) => {
            if left
                .iter()
                .chain(right.iter())
                .any(|value| value.kind().is_none())
            {
                return None;
            }
            if left
                .first()
                .zip(right.first())
                .is_some_and(|(left, right)| left.kind() != right.kind())
            {
                return None;
            }
            let mut left = left.clone();
            let mut right = right.clone();
            sort_deduplicate(&mut left);
            sort_deduplicate(&mut right);
            Some(left == right)
        }
        (left, right) if left.is_numeric() && right.is_numeric() => {
            numeric_cmp(left, right).map(|ordering| ordering == Ordering::Equal)
        }
        (left, right) if left.kind() == right.kind() => Some(left == right),
        _ => None,
    }
}

fn numeric_cmp(left: &TypedValue, right: &TypedValue) -> Option<Ordering> {
    match (left, right) {
        (TypedValue::Integer(left), TypedValue::Integer(right)) => Some(left.cmp(right)),
        (TypedValue::Decimal(left), TypedValue::Decimal(right)) => left.numeric_cmp(*right),
        (TypedValue::Integer(left), TypedValue::Decimal(right)) => {
            DecimalValue::new(i128::from(*left), 0)
                .ok()?
                .numeric_cmp(*right)
        }
        (TypedValue::Decimal(left), TypedValue::Integer(right)) => {
            left.numeric_cmp(DecimalValue::new(i128::from(*right), 0).ok()?)
        }
        _ => None,
    }
}

fn order_matches(operator: ComparisonOperator, ordering: Ordering) -> bool {
    match operator {
        ComparisonOperator::GreaterThan => ordering == Ordering::Greater,
        ComparisonOperator::GreaterOrEqual => ordering != Ordering::Less,
        ComparisonOperator::LessThan => ordering == Ordering::Less,
        ComparisonOperator::LessOrEqual => ordering != Ordering::Greater,
        _ => false,
    }
}

fn fold_all<I>(outcomes: I) -> ComparisonOutcome
where
    I: IntoIterator<Item = ComparisonOutcome>,
{
    let outcomes = outcomes.into_iter().collect::<Vec<_>>();
    if outcomes.contains(&ComparisonOutcome::Unsatisfied) {
        ComparisonOutcome::Unsatisfied
    } else if outcomes.contains(&ComparisonOutcome::Incomparable) {
        ComparisonOutcome::Incomparable
    } else if outcomes.contains(&ComparisonOutcome::Conflicted) {
        ComparisonOutcome::Conflicted
    } else if outcomes.contains(&ComparisonOutcome::InsufficientEvidence) {
        ComparisonOutcome::InsufficientEvidence
    } else if outcomes.contains(&ComparisonOutcome::UnresolvedInput) {
        ComparisonOutcome::UnresolvedInput
    } else if outcomes.contains(&ComparisonOutcome::Unknown) {
        ComparisonOutcome::Unknown
    } else {
        ComparisonOutcome::Satisfied
    }
}

fn fold_any<I>(outcomes: I) -> ComparisonOutcome
where
    I: IntoIterator<Item = ComparisonOutcome>,
{
    let outcomes = outcomes.into_iter().collect::<Vec<_>>();
    if outcomes.contains(&ComparisonOutcome::Satisfied) {
        ComparisonOutcome::Satisfied
    } else if outcomes.contains(&ComparisonOutcome::Incomparable) {
        ComparisonOutcome::Incomparable
    } else if outcomes.contains(&ComparisonOutcome::Conflicted) {
        ComparisonOutcome::Conflicted
    } else if outcomes.contains(&ComparisonOutcome::InsufficientEvidence) {
        ComparisonOutcome::InsufficientEvidence
    } else if outcomes.contains(&ComparisonOutcome::UnresolvedInput) {
        ComparisonOutcome::UnresolvedInput
    } else if outcomes.contains(&ComparisonOutcome::Unknown) {
        ComparisonOutcome::Unknown
    } else {
        ComparisonOutcome::Unsatisfied
    }
}

fn negate_outcome(outcome: ComparisonOutcome) -> ComparisonOutcome {
    match outcome {
        ComparisonOutcome::Satisfied => ComparisonOutcome::Unsatisfied,
        ComparisonOutcome::Unsatisfied => ComparisonOutcome::Satisfied,
        other => other,
    }
}

fn expression_reason(outcome: ComparisonOutcome) -> ComparisonReasonCode {
    match outcome {
        ComparisonOutcome::Satisfied => ComparisonReasonCode::ExpressionSatisfied,
        ComparisonOutcome::Unsatisfied => ComparisonReasonCode::ExpressionUnsatisfied,
        ComparisonOutcome::Unknown => ComparisonReasonCode::ExpressionUnknown,
        ComparisonOutcome::Conflicted => ComparisonReasonCode::ExpressionConflict,
        ComparisonOutcome::InsufficientEvidence => {
            ComparisonReasonCode::ExpressionInsufficientEvidence
        }
        ComparisonOutcome::UnresolvedInput => ComparisonReasonCode::ExpressionUnresolvedInput,
        ComparisonOutcome::Incomparable => ComparisonReasonCode::ExpressionIncomparable,
    }
}

fn sort_deduplicate<T: Ord>(values: &mut Vec<T>) {
    values.sort();
    values.dedup();
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::{
        identifiers::{FactId, ObservationId, ObservedStateId, ProvenanceId, SourceId},
        intent::{ConditionExpression, DesiredCondition},
        normalization::{NormalizationInput, normalize_current_state},
        observation::{Fact, Observation, ObservationEvidenceSet, Provenance, SourceKind},
        quality::{Confidence, ConflictStatus, QualityMetadata, SensitivityClass, TrustClass},
    };

    fn condition(
        id: &str,
        subject: &str,
        operator: ComparisonOperator,
        expected: Option<TypedValue>,
    ) -> DesiredCondition {
        DesiredCondition::new(
            ConditionId::new(id).unwrap(),
            SubjectPath::from_str(subject).unwrap(),
            operator,
            expected,
        )
        .unwrap()
    }

    fn make_desired(
        conditions: Vec<DesiredCondition>,
        expression: ConditionExpression,
    ) -> DesiredState {
        DesiredState::new(
            DesiredStateId::new("desired-1").unwrap(),
            conditions,
            expression,
            Vec::new(),
            Vec::new(),
        )
        .unwrap()
    }

    fn current_state(values: Vec<(&str, TypedValue, AssertionPolarity)>) -> CurrentState {
        let provenance = Provenance::new(
            ProvenanceId::new("provenance-1").unwrap(),
            SourceKind::Synthetic,
            SourceId::new("fixture").unwrap(),
            "comparison fixture",
        )
        .unwrap();
        let mut observations = Vec::new();
        let mut facts = Vec::new();
        for (index, (subject, value, polarity)) in values.into_iter().enumerate() {
            let observation_id = ObservationId::new(format!("observation-{}", index + 1)).unwrap();
            let fact_id = FactId::new(format!("fact-{}", index + 1)).unwrap();
            let subject = SubjectPath::from_str(subject).unwrap();
            observations.push(
                Observation::new(
                    observation_id.clone(),
                    subject.clone(),
                    value.clone(),
                    ProvenanceId::new("provenance-1").unwrap(),
                )
                .unwrap(),
            );
            facts.push(Fact::new(fact_id, subject, value, polarity, vec![observation_id]).unwrap());
        }
        let records =
            ObservationEvidenceSet::new(vec![provenance], observations, facts, vec![]).unwrap();
        normalize_current_state(
            ObservedStateId::new("state-1").unwrap(),
            NormalizationInput::new(records),
        )
        .unwrap()
    }

    fn unknown_state(subject: &str) -> CurrentState {
        let records = ObservationEvidenceSet::new(vec![], vec![], vec![], vec![]).unwrap();
        let input = NormalizationInput::new(records)
            .with_unknown_subjects(vec![SubjectPath::from_str(subject).unwrap()])
            .unwrap();
        normalize_current_state(ObservedStateId::new("state-1").unwrap(), input).unwrap()
    }

    fn missing_evidence_state() -> CurrentState {
        let state = current_state(vec![(
            "coverage.percent",
            TypedValue::Integer(92),
            AssertionPolarity::Affirmed,
        )]);
        let entry = state.entries()[0].clone();
        let records = ObservationEvidenceSet::new(
            vec![
                Provenance::new(
                    ProvenanceId::new("provenance-1").unwrap(),
                    SourceKind::Synthetic,
                    SourceId::new("fixture").unwrap(),
                    "comparison fixture",
                )
                .unwrap(),
            ],
            vec![
                Observation::new(
                    ObservationId::new("observation-1").unwrap(),
                    SubjectPath::from_str("coverage.percent").unwrap(),
                    TypedValue::Integer(92),
                    ProvenanceId::new("provenance-1").unwrap(),
                )
                .unwrap(),
            ],
            vec![
                Fact::new(
                    FactId::new("fact-1").unwrap(),
                    SubjectPath::from_str("coverage.percent").unwrap(),
                    TypedValue::Integer(92),
                    AssertionPolarity::Affirmed,
                    vec![ObservationId::new("observation-1").unwrap()],
                )
                .unwrap(),
            ],
            vec![],
        )
        .unwrap();
        let normalized = normalize_current_state(
            ObservedStateId::new("state-unsupported").unwrap(),
            NormalizationInput::new(records).with_required_evidence(true),
        )
        .unwrap();
        assert_ne!(entry.status(), normalized.entries()[0].status());
        normalized
    }

    fn known_state_with_metadata(metadata: QualityMetadata) -> CurrentState {
        let entry = NormalizedStateEntry::from_parts(
            SubjectPath::from_str("coverage.percent").unwrap(),
            StateStatus::Known,
            Some(TypedValue::Integer(96)),
            Some(AssertionPolarity::Affirmed),
            Vec::new(),
            crate::normalization::StateLineage::from_parts(
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )
            .unwrap(),
            Vec::new(),
            Some(metadata),
        )
        .unwrap();
        crate::declarative_context::ObservedState::new_v1_with_entries(
            ObservedStateId::new("state-quality").unwrap(),
            vec![entry],
            Vec::new(),
        )
    }

    #[test]
    fn versions_outcomes_and_reasons_are_strict_and_stable() {
        assert_eq!(ComparisonSemanticsVersion::V1.to_string(), "1.0");
        assert_eq!(
            ComparisonSemanticsVersion::from_str("1.0").unwrap(),
            ComparisonSemanticsVersion::V1
        );
        assert!(ComparisonSemanticsVersion::from_str("1").is_err());
        assert!(
            ComparisonSemanticsVersion::new(1, 1)
                .unwrap()
                .ensure_supported()
                .is_err()
        );
        assert!(ComparisonRules::new(ComparisonSemanticsVersion::new(1, 1).unwrap()).is_err());

        for outcome in [
            ComparisonOutcome::Satisfied,
            ComparisonOutcome::Unsatisfied,
            ComparisonOutcome::Unknown,
            ComparisonOutcome::Conflicted,
            ComparisonOutcome::InsufficientEvidence,
            ComparisonOutcome::UnresolvedInput,
            ComparisonOutcome::Incomparable,
        ] {
            assert_eq!(
                ComparisonOutcome::from_str(outcome.as_str()).unwrap(),
                outcome
            );
            assert_eq!(outcome.to_string(), outcome.as_str());
        }
        assert!(ComparisonOutcome::from_str("NOT_A_RESULT").is_err());
        for reason in [
            ComparisonReasonCode::ValueMatches,
            ComparisonReasonCode::ValueDoesNotMatch,
            ComparisonReasonCode::SubjectNotObserved,
            ComparisonReasonCode::StateUnknown,
            ComparisonReasonCode::StateConflict,
            ComparisonReasonCode::MissingEvidence,
            ComparisonReasonCode::StaleEvidence,
            ComparisonReasonCode::FreshnessUnknown,
            ComparisonReasonCode::IncompleteInformation,
            ComparisonReasonCode::IncompatibleTypes,
            ComparisonReasonCode::UnsupportedOperation,
            ComparisonReasonCode::NegatedAssertionNotComparable,
            ComparisonReasonCode::ExpressionSatisfied,
            ComparisonReasonCode::ExpressionUnsatisfied,
            ComparisonReasonCode::ExpressionUnknown,
            ComparisonReasonCode::ExpressionConflict,
            ComparisonReasonCode::ExpressionInsufficientEvidence,
            ComparisonReasonCode::ExpressionUnresolvedInput,
            ComparisonReasonCode::ExpressionIncomparable,
        ] {
            assert_eq!(
                ComparisonReasonCode::from_str(reason.as_str()).unwrap(),
                reason
            );
            assert_eq!(reason.to_string(), reason.as_str());
        }
        assert!(ComparisonReasonCode::from_str("NOPE").is_err());
        assert!(!ComparisonRules::default().requires_fresh_evidence());
        assert!(
            ComparisonRules::v1()
                .requiring_fresh_evidence(true)
                .requires_fresh_evidence()
        );
        assert_eq!(ComparisonSemanticsVersion::V1.major(), 1);
        assert_eq!(ComparisonSemanticsVersion::V1.minor(), 0);
        let rules = ComparisonRules::new(ComparisonSemanticsVersion::V1).unwrap();
        assert_eq!(rules.version(), ComparisonSemanticsVersion::V1);
    }

    #[test]
    fn compares_scalar_and_numeric_boundaries_without_coercion() {
        let current = current_state(vec![
            (
                "coverage.percent",
                TypedValue::Integer(92),
                AssertionPolarity::Affirmed,
            ),
            (
                "release.ready",
                TypedValue::Boolean(true),
                AssertionPolarity::Affirmed,
            ),
        ]);
        let rules = ComparisonRules::v1();
        let desired = make_desired(
            vec![condition(
                "coverage",
                "coverage.percent",
                ComparisonOperator::GreaterOrEqual,
                Some(TypedValue::Integer(95)),
            )],
            ConditionExpression::condition(ConditionId::new("coverage").unwrap()),
        );
        let result = compare_condition(
            &desired,
            &ConditionId::new("coverage").unwrap(),
            &current,
            &rules,
        )
        .unwrap();
        assert_eq!(result.outcome(), ComparisonOutcome::Unsatisfied);
        assert_eq!(result.reason(), ComparisonReasonCode::ValueDoesNotMatch);
        assert_eq!(result.trace().current_values(), &[TypedValue::Integer(92)]);
        assert_eq!(result.trace().desired_values(), &[TypedValue::Integer(95)]);
        assert_eq!(result.trace().facts().len(), 1);
        assert_eq!(result.trace().observations().len(), 1);
        assert_eq!(result.trace().provenances().len(), 1);

        let bool_desired = make_desired(
            vec![condition(
                "ready",
                "release.ready",
                ComparisonOperator::Equals,
                Some(TypedValue::Boolean(true)),
            )],
            ConditionExpression::condition(ConditionId::new("ready").unwrap()),
        );
        assert_eq!(
            compare_desired_condition(
                &bool_desired,
                &ConditionId::new("ready").unwrap(),
                &current,
                &rules,
            )
            .unwrap()
            .outcome(),
            ComparisonOutcome::Satisfied
        );

        for (operator, value, expected, outcome) in [
            (
                ComparisonOperator::GreaterThan,
                TypedValue::Integer(95),
                TypedValue::Integer(95),
                ComparisonOutcome::Unsatisfied,
            ),
            (
                ComparisonOperator::GreaterOrEqual,
                TypedValue::Integer(95),
                TypedValue::Integer(95),
                ComparisonOutcome::Satisfied,
            ),
            (
                ComparisonOperator::LessThan,
                TypedValue::Integer(92),
                TypedValue::Integer(95),
                ComparisonOutcome::Satisfied,
            ),
            (
                ComparisonOperator::LessOrEqual,
                TypedValue::Integer(92),
                TypedValue::Integer(92),
                ComparisonOutcome::Satisfied,
            ),
        ] {
            let condition_id = ConditionId::new("boundary").unwrap();
            let desired = make_desired(
                vec![
                    DesiredCondition::new(
                        condition_id.clone(),
                        SubjectPath::from_str("coverage.percent").unwrap(),
                        operator,
                        Some(expected),
                    )
                    .unwrap(),
                ],
                ConditionExpression::condition(condition_id.clone()),
            );
            let result = compare_condition(
                &desired,
                &condition_id,
                &current_state(vec![(
                    "coverage.percent",
                    value,
                    AssertionPolarity::Affirmed,
                )]),
                &rules,
            )
            .unwrap();
            assert_eq!(result.outcome(), outcome);
        }

        let decimal_current = current_state(vec![(
            "coverage.percent",
            TypedValue::Decimal(DecimalValue::from_str("92.50").unwrap()),
            AssertionPolarity::Affirmed,
        )]);
        let decimal_condition = condition(
            "decimal",
            "coverage.percent",
            ComparisonOperator::GreaterThan,
            Some(TypedValue::Integer(92)),
        );
        let decimal_desired = make_desired(
            vec![decimal_condition],
            ConditionExpression::condition(ConditionId::new("decimal").unwrap()),
        );
        assert_eq!(
            compare_desired_state(&decimal_desired, &decimal_current, &rules)
                .unwrap()
                .outcome(),
            ComparisonOutcome::Satisfied
        );

        let decimal_exact = make_desired(
            vec![condition(
                "decimal-exact",
                "coverage.percent",
                ComparisonOperator::Equals,
                Some(TypedValue::Decimal(DecimalValue::from_str("92.5").unwrap())),
            )],
            ConditionExpression::condition(ConditionId::new("decimal-exact").unwrap()),
        );
        assert_eq!(
            compare_desired_state(&decimal_exact, &decimal_current, &rules)
                .unwrap()
                .outcome(),
            ComparisonOutcome::Satisfied
        );

        let mismatch = make_desired(
            vec![condition(
                "wrong-type",
                "coverage.percent",
                ComparisonOperator::Equals,
                Some(TypedValue::Boolean(true)),
            )],
            ConditionExpression::condition(ConditionId::new("wrong-type").unwrap()),
        );
        assert_eq!(
            compare_desired_state(&mismatch, &current, &rules)
                .unwrap()
                .outcome(),
            ComparisonOutcome::Incomparable
        );
    }

    #[test]
    fn compares_presence_membership_sets_and_polarity_explicitly() {
        let current = current_state(vec![
            (
                "feature.enabled",
                TypedValue::Boolean(true),
                AssertionPolarity::Affirmed,
            ),
            (
                "environment.name",
                TypedValue::string("prod").unwrap(),
                AssertionPolarity::Affirmed,
            ),
            (
                "tags",
                TypedValue::set(vec![TypedValue::symbol("rust").unwrap()]).unwrap(),
                AssertionPolarity::Affirmed,
            ),
        ]);
        let rules = ComparisonRules::v1();
        for (id, subject, operator, expected, outcome) in [
            (
                "present",
                "feature.enabled",
                ComparisonOperator::Present,
                None,
                ComparisonOutcome::Satisfied,
            ),
            (
                "absent",
                "feature.enabled",
                ComparisonOperator::Absent,
                None,
                ComparisonOutcome::Unsatisfied,
            ),
            (
                "missing-present",
                "feature.missing",
                ComparisonOperator::Present,
                None,
                ComparisonOutcome::Unknown,
            ),
            (
                "membership",
                "environment.name",
                ComparisonOperator::In,
                Some(TypedValue::set(vec![TypedValue::string("prod").unwrap()]).unwrap()),
                ComparisonOutcome::Satisfied,
            ),
            (
                "contains",
                "tags",
                ComparisonOperator::Contains,
                Some(TypedValue::symbol("rust").unwrap()),
                ComparisonOutcome::Satisfied,
            ),
        ] {
            let condition_id = ConditionId::new(id).unwrap();
            let desired = make_desired(
                vec![
                    DesiredCondition::new(
                        condition_id.clone(),
                        SubjectPath::from_str(subject).unwrap(),
                        operator,
                        expected,
                    )
                    .unwrap(),
                ],
                ConditionExpression::condition(condition_id.clone()),
            );
            assert_eq!(
                compare_condition(&desired, &condition_id, &current, &rules)
                    .unwrap()
                    .outcome(),
                outcome
            );
        }

        let wrong_contains = make_desired(
            vec![condition(
                "wrong-contains",
                "environment.name",
                ComparisonOperator::Contains,
                Some(TypedValue::symbol("rust").unwrap()),
            )],
            ConditionExpression::condition(ConditionId::new("wrong-contains").unwrap()),
        );
        assert_eq!(
            compare_desired_state(&wrong_contains, &current, &rules)
                .unwrap()
                .reason(),
            ComparisonReasonCode::UnsupportedOperation
        );

        for (id, subject, operator, expected, outcome) in [
            (
                "in-mismatch",
                "environment.name",
                ComparisonOperator::In,
                TypedValue::set(vec![TypedValue::string("dev").unwrap()]).unwrap(),
                ComparisonOutcome::Unsatisfied,
            ),
            (
                "in-incomparable",
                "feature.enabled",
                ComparisonOperator::In,
                TypedValue::set(vec![TypedValue::string("true").unwrap()]).unwrap(),
                ComparisonOutcome::Incomparable,
            ),
            (
                "contains-mismatch",
                "tags",
                ComparisonOperator::Contains,
                TypedValue::symbol("go").unwrap(),
                ComparisonOutcome::Unsatisfied,
            ),
            (
                "contains-incomparable",
                "tags",
                ComparisonOperator::Contains,
                TypedValue::string("rust").unwrap(),
                ComparisonOutcome::Incomparable,
            ),
        ] {
            let condition_id = ConditionId::new(id).unwrap();
            let desired = make_desired(
                vec![condition(id, subject, operator, Some(expected))],
                ConditionExpression::condition(condition_id.clone()),
            );
            assert_eq!(
                compare_condition(&desired, &condition_id, &current, &rules)
                    .unwrap()
                    .outcome(),
                outcome
            );
        }

        let negated_equal = make_desired(
            vec![condition(
                "negated",
                "feature.enabled",
                ComparisonOperator::Equals,
                Some(TypedValue::Boolean(true)),
            )],
            ConditionExpression::condition(ConditionId::new("negated").unwrap()),
        );
        let negated_current = current_state(vec![(
            "feature.enabled",
            TypedValue::Boolean(true),
            AssertionPolarity::Negated,
        )]);
        assert_eq!(
            compare_desired_state(&negated_equal, &negated_current, &rules)
                .unwrap()
                .outcome(),
            ComparisonOutcome::Unsatisfied
        );
        let negated_not_equal = make_desired(
            vec![condition(
                "negated-not",
                "feature.enabled",
                ComparisonOperator::NotEquals,
                Some(TypedValue::Boolean(true)),
            )],
            ConditionExpression::condition(ConditionId::new("negated-not").unwrap()),
        );
        assert_eq!(
            compare_desired_state(&negated_not_equal, &negated_current, &rules)
                .unwrap()
                .outcome(),
            ComparisonOutcome::Satisfied
        );
        let negated_threshold = make_desired(
            vec![condition(
                "negated-threshold",
                "feature.enabled",
                ComparisonOperator::GreaterThan,
                Some(TypedValue::Integer(1)),
            )],
            ConditionExpression::condition(ConditionId::new("negated-threshold").unwrap()),
        );
        assert_eq!(
            compare_desired_state(&negated_threshold, &negated_current, &rules)
                .unwrap()
                .outcome(),
            ComparisonOutcome::Incomparable
        );
    }

    #[test]
    fn preserves_unknown_conflict_missing_evidence_and_freshness_states() {
        let rules = ComparisonRules::v1();
        let desired = make_desired(
            vec![condition(
                "coverage",
                "coverage.percent",
                ComparisonOperator::GreaterOrEqual,
                Some(TypedValue::Integer(95)),
            )],
            ConditionExpression::condition(ConditionId::new("coverage").unwrap()),
        );
        let unknown = unknown_state("coverage.percent");
        let unknown_result = compare_desired_state(&desired, &unknown, &rules).unwrap();
        assert_eq!(unknown_result.outcome(), ComparisonOutcome::Unknown);
        assert_eq!(unknown_result.reason(), ComparisonReasonCode::StateUnknown);

        let conflict = current_state(vec![
            (
                "coverage.percent",
                TypedValue::Integer(92),
                AssertionPolarity::Affirmed,
            ),
            (
                "coverage.percent",
                TypedValue::Integer(96),
                AssertionPolarity::Affirmed,
            ),
        ]);
        assert_eq!(
            compare_desired_state(&desired, &conflict, &rules)
                .unwrap()
                .outcome(),
            ComparisonOutcome::Conflicted
        );

        let unsupported = missing_evidence_state();
        let unsupported_result = compare_desired_state(&desired, &unsupported, &rules).unwrap();
        assert_eq!(
            unsupported_result.outcome(),
            ComparisonOutcome::InsufficientEvidence
        );
        assert_eq!(
            unsupported_result.reason(),
            ComparisonReasonCode::MissingEvidence
        );

        let quality = QualityMetadata::new(
            TrustClass::ObservedEvidence,
            SensitivityClass::Normal,
            Confidence::Unknown,
            FreshnessStatus::Stale,
            Uncertainty::None,
        );
        let fresh_current = current_state(vec![(
            "coverage.percent",
            TypedValue::Integer(96),
            AssertionPolarity::Affirmed,
        )]);
        let fresh_entry = fresh_current.entries()[0].clone();
        let quality_records = ObservationEvidenceSet::new(
            vec![
                Provenance::new(
                    ProvenanceId::new("provenance-1").unwrap(),
                    SourceKind::Synthetic,
                    SourceId::new("fixture").unwrap(),
                    "comparison fixture",
                )
                .unwrap(),
            ],
            vec![
                Observation::new(
                    ObservationId::new("observation-1").unwrap(),
                    SubjectPath::from_str("coverage.percent").unwrap(),
                    TypedValue::Integer(96),
                    ProvenanceId::new("provenance-1").unwrap(),
                )
                .unwrap(),
            ],
            vec![
                Fact::new(
                    FactId::new("fact-1").unwrap(),
                    SubjectPath::from_str("coverage.percent").unwrap(),
                    TypedValue::Integer(96),
                    AssertionPolarity::Affirmed,
                    vec![ObservationId::new("observation-1").unwrap()],
                )
                .unwrap(),
            ],
            vec![],
        )
        .unwrap();
        let quality_state = normalize_current_state(
            ObservedStateId::new("state-quality").unwrap(),
            NormalizationInput::new(quality_records).with_quality_metadata(
                SubjectPath::from_str("coverage.percent").unwrap(),
                vec![quality],
            ),
        )
        .unwrap();
        assert_eq!(fresh_entry.status(), StateStatus::Known);
        assert_eq!(
            compare_desired_state(
                &desired,
                &quality_state,
                &rules.requiring_fresh_evidence(true),
            )
            .unwrap()
            .outcome(),
            ComparisonOutcome::InsufficientEvidence
        );
        assert_eq!(
            compare_desired_state(&desired, &quality_state, &rules)
                .unwrap()
                .outcome(),
            ComparisonOutcome::Satisfied
        );
    }

    #[test]
    fn logical_expression_propagation_is_explicit_and_traceable() {
        let first = condition(
            "satisfied",
            "a",
            ComparisonOperator::Equals,
            Some(TypedValue::Boolean(true)),
        );
        let second = condition(
            "unsatisfied",
            "b",
            ComparisonOperator::Equals,
            Some(TypedValue::Boolean(true)),
        );
        let unknown = condition(
            "unknown",
            "c",
            ComparisonOperator::Equals,
            Some(TypedValue::Boolean(true)),
        );
        let desired = make_desired(
            vec![first, second, unknown],
            ConditionExpression::all(vec![
                ConditionExpression::condition(ConditionId::new("satisfied").unwrap()),
                ConditionExpression::condition(ConditionId::new("unsatisfied").unwrap()),
            ])
            .unwrap(),
        );
        let current = current_state(vec![
            ("a", TypedValue::Boolean(true), AssertionPolarity::Affirmed),
            ("b", TypedValue::Boolean(false), AssertionPolarity::Affirmed),
        ]);
        let all = compare_desired_state(&desired, &current, &ComparisonRules::v1()).unwrap();
        assert_eq!(all.outcome(), ComparisonOutcome::Unsatisfied);
        assert_eq!(all.reason(), ComparisonReasonCode::ExpressionUnsatisfied);
        assert_eq!(all.children().len(), 2);
        assert_eq!(all.trace().subjects().len(), 2);

        let unknown_desired = make_desired(
            vec![
                condition(
                    "satisfied",
                    "a",
                    ComparisonOperator::Equals,
                    Some(TypedValue::Boolean(true)),
                ),
                condition(
                    "unknown",
                    "c",
                    ComparisonOperator::Equals,
                    Some(TypedValue::Boolean(true)),
                ),
            ],
            ConditionExpression::all(vec![
                ConditionExpression::condition(ConditionId::new("satisfied").unwrap()),
                ConditionExpression::condition(ConditionId::new("unknown").unwrap()),
            ])
            .unwrap(),
        );
        assert_eq!(
            compare_desired_state(&unknown_desired, &current, &ComparisonRules::v1())
                .unwrap()
                .outcome(),
            ComparisonOutcome::Unknown
        );

        let any = make_desired(
            vec![
                condition(
                    "satisfied",
                    "a",
                    ComparisonOperator::Equals,
                    Some(TypedValue::Boolean(true)),
                ),
                condition(
                    "unknown",
                    "c",
                    ComparisonOperator::Equals,
                    Some(TypedValue::Boolean(true)),
                ),
            ],
            ConditionExpression::any(vec![
                ConditionExpression::condition(ConditionId::new("satisfied").unwrap()),
                ConditionExpression::condition(ConditionId::new("unknown").unwrap()),
            ])
            .unwrap(),
        );
        let any_result = compare_desired_state(&any, &current, &ComparisonRules::v1()).unwrap();
        assert_eq!(any_result.outcome(), ComparisonOutcome::Satisfied);
        assert_eq!(any_result.children().len(), 2);
        assert_eq!(any_result.trace().subjects().len(), 2);

        let not = make_desired(
            vec![condition(
                "unknown",
                "c",
                ComparisonOperator::Equals,
                Some(TypedValue::Boolean(true)),
            )],
            ConditionExpression::negate(ConditionExpression::condition(
                ConditionId::new("unknown").unwrap(),
            )),
        );
        let not_result = compare_desired_state(&not, &current, &ComparisonRules::v1()).unwrap();
        assert_eq!(not_result.outcome(), ComparisonOutcome::Unknown);

        let false_any = make_desired(
            vec![
                condition(
                    "false-a",
                    "a",
                    ComparisonOperator::Equals,
                    Some(TypedValue::Boolean(false)),
                ),
                condition(
                    "false-b",
                    "b",
                    ComparisonOperator::Equals,
                    Some(TypedValue::Boolean(true)),
                ),
            ],
            ConditionExpression::any(vec![
                ConditionExpression::condition(ConditionId::new("false-a").unwrap()),
                ConditionExpression::condition(ConditionId::new("false-b").unwrap()),
            ])
            .unwrap(),
        );
        assert_eq!(
            compare_desired_state(&false_any, &current, &ComparisonRules::v1())
                .unwrap()
                .outcome(),
            ComparisonOutcome::Unsatisfied
        );
    }

    #[test]
    fn rejects_missing_conditions_duplicate_state_subjects_and_bad_versions() {
        let current = current_state(vec![(
            "coverage.percent",
            TypedValue::Integer(92),
            AssertionPolarity::Affirmed,
        )]);
        let desired = make_desired(
            vec![condition(
                "known",
                "coverage.percent",
                ComparisonOperator::Equals,
                Some(TypedValue::Integer(92)),
            )],
            ConditionExpression::condition(ConditionId::new("known").unwrap()),
        );
        assert!(matches!(
            compare_condition(
                &desired,
                &ConditionId::new("missing").unwrap(),
                &current,
                &ComparisonRules::v1(),
            ),
            Err(ValidationError::MissingDeclarativeIdentity {
                kind: "condition",
                ..
            })
        ));

        let desired_condition = desired.conditions()[0].clone();
        let duplicate_entries = vec![
            crate::normalization::NormalizedStateEntry::from_parts(
                desired_condition.subject().clone(),
                StateStatus::Known,
                Some(TypedValue::Integer(92)),
                Some(AssertionPolarity::Affirmed),
                Vec::new(),
                crate::normalization::StateLineage::from_parts(
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                )
                .unwrap(),
                Vec::new(),
                None,
            )
            .unwrap(),
            crate::normalization::NormalizedStateEntry::from_parts(
                desired_condition.subject().clone(),
                StateStatus::Known,
                Some(TypedValue::Integer(92)),
                Some(AssertionPolarity::Affirmed),
                Vec::new(),
                crate::normalization::StateLineage::from_parts(
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                )
                .unwrap(),
                Vec::new(),
                None,
            )
            .unwrap(),
        ];
        let duplicate_state = crate::declarative_context::ObservedState::new_v1_with_entries(
            ObservedStateId::new("duplicate-state").unwrap(),
            duplicate_entries,
            Vec::new(),
        );
        assert!(matches!(
            compare_desired_state(&desired, &duplicate_state, &ComparisonRules::v1()),
            Err(ValidationError::DuplicateDeclarativeIdentity {
                kind: "normalized_state.subject",
                ..
            })
        ));

        let unsupported = ComparisonRules::new(ComparisonSemanticsVersion::new(1, 1).unwrap());
        assert!(unsupported.is_err());
    }

    #[test]
    fn comparison_results_and_traces_expose_the_complete_public_surface() {
        let condition_id = ConditionId::new("condition-1").unwrap();
        let desired = make_desired(
            vec![condition(
                "condition-1",
                "coverage.percent",
                ComparisonOperator::Equals,
                Some(TypedValue::Integer(92)),
            )],
            ConditionExpression::condition(condition_id.clone()),
        );
        let current = current_state(vec![(
            "coverage.percent",
            TypedValue::Integer(92),
            AssertionPolarity::Affirmed,
        )]);
        let result = compare_desired_state(&desired, &current, &ComparisonRules::v1()).unwrap();
        assert_eq!(result.version(), COMPARISON_SEMANTICS_VERSION);
        assert_eq!(result.desired_state(), desired.id());
        assert!(matches!(
            result.target(),
            ComparisonTarget::Condition(id) if id == &condition_id
        ));
        assert_eq!(result.outcome(), ComparisonOutcome::Satisfied);
        assert_eq!(result.reason(), ComparisonReasonCode::ValueMatches);
        assert!(result.children().is_empty());
        assert_eq!(result.trace().observed_state().as_str(), "state-1");
        assert_eq!(result.trace().subjects()[0].to_string(), "coverage.percent");
        assert_eq!(result.trace().statuses(), &[StateStatus::Known]);
        assert_eq!(result.trace().polarities(), &[AssertionPolarity::Affirmed]);
        assert_eq!(result.trace().current_values(), &[TypedValue::Integer(92)]);
        assert!(result.trace().evidence().is_empty());
        assert_eq!(result.trace().provenances().len(), 1);
        assert!(matches!(
            ComparisonTarget::expression(ConditionExpression::condition(condition_id)),
            ComparisonTarget::Expression(_)
        ));
    }

    #[test]
    fn covers_quality_edges_set_comparison_and_algebra_helpers() {
        let desired = make_desired(
            vec![condition(
                "coverage",
                "coverage.percent",
                ComparisonOperator::GreaterOrEqual,
                Some(TypedValue::Integer(95)),
            )],
            ConditionExpression::condition(ConditionId::new("coverage").unwrap()),
        );
        let rules = ComparisonRules::v1().requiring_fresh_evidence(true);

        let conflict = known_state_with_metadata(
            QualityMetadata::new(
                TrustClass::ObservedEvidence,
                SensitivityClass::Normal,
                Confidence::Unknown,
                FreshnessStatus::Fresh,
                Uncertainty::None,
            )
            .with_conflict(ConflictStatus::Unresolved),
        );
        assert_eq!(
            compare_desired_state(&desired, &conflict, &rules)
                .unwrap()
                .outcome(),
            ComparisonOutcome::Conflicted
        );
        for uncertainty in [
            Uncertainty::Incomplete,
            Uncertainty::Probabilistic,
            Uncertainty::Unknown,
        ] {
            let state = known_state_with_metadata(QualityMetadata::new(
                TrustClass::ObservedEvidence,
                SensitivityClass::Normal,
                Confidence::Unknown,
                FreshnessStatus::Fresh,
                uncertainty,
            ));
            assert_eq!(
                compare_desired_state(&desired, &state, &rules)
                    .unwrap()
                    .reason(),
                ComparisonReasonCode::IncompleteInformation
            );
        }
        for (freshness, reason) in [
            (FreshnessStatus::Stale, ComparisonReasonCode::StaleEvidence),
            (
                FreshnessStatus::Unknown,
                ComparisonReasonCode::FreshnessUnknown,
            ),
            (FreshnessStatus::Fresh, ComparisonReasonCode::ValueMatches),
        ] {
            let state = known_state_with_metadata(QualityMetadata::new(
                TrustClass::ObservedEvidence,
                SensitivityClass::Normal,
                Confidence::Unknown,
                freshness,
                Uncertainty::None,
            ));
            assert_eq!(
                compare_desired_state(&desired, &state, &rules)
                    .unwrap()
                    .reason(),
                reason
            );
        }
        assert_eq!(
            compare_desired_state(
                &desired,
                &current_state(vec![(
                    "coverage.percent",
                    TypedValue::Integer(96),
                    AssertionPolarity::Affirmed,
                )]),
                &rules,
            )
            .unwrap()
            .reason(),
            ComparisonReasonCode::FreshnessUnknown
        );

        let ordered_left = TypedValue::Set(vec![TypedValue::Integer(1), TypedValue::Integer(2)]);
        let ordered_right = TypedValue::Set(vec![TypedValue::Integer(2), TypedValue::Integer(1)]);
        assert_eq!(
            typed_values_equal(&ordered_left, &ordered_right),
            Some(true)
        );
        assert_eq!(
            typed_values_equal(
                &TypedValue::Set(vec![TypedValue::Set(Vec::new())]),
                &ordered_left,
            ),
            None
        );
        assert_eq!(
            typed_values_equal(
                &TypedValue::Set(vec![TypedValue::Integer(1)]),
                &TypedValue::Set(vec![TypedValue::Boolean(true)]),
            ),
            None
        );
        assert_eq!(
            typed_values_equal(&TypedValue::Boolean(true), &TypedValue::Boolean(true)),
            Some(true)
        );
        assert_eq!(
            typed_values_equal(
                &TypedValue::Boolean(true),
                &TypedValue::string("true").unwrap()
            ),
            None
        );
        assert_eq!(
            numeric_cmp(&TypedValue::Integer(1), &TypedValue::Integer(2)),
            Some(Ordering::Less)
        );
        assert_eq!(
            numeric_cmp(
                &TypedValue::Decimal(DecimalValue::from_str("1.50").unwrap()),
                &TypedValue::Decimal(DecimalValue::from_str("1.5").unwrap()),
            ),
            Some(Ordering::Equal)
        );
        assert_eq!(
            numeric_cmp(
                &TypedValue::Decimal(DecimalValue::from_str("1.5").unwrap()),
                &TypedValue::Integer(1),
            ),
            Some(Ordering::Greater)
        );
        assert_eq!(
            numeric_cmp(
                &TypedValue::Integer(1),
                &TypedValue::Decimal(DecimalValue::from_str("1.5").unwrap()),
            ),
            Some(Ordering::Less)
        );
        assert_eq!(
            numeric_cmp(
                &TypedValue::Decimal(DecimalValue::new(i128::MAX, 0).unwrap()),
                &TypedValue::Decimal(DecimalValue::new(1, DecimalValue::MAX_SCALE).unwrap()),
            ),
            None
        );
        assert!(!order_matches(ComparisonOperator::Equals, Ordering::Equal));
        assert_eq!(ValueMatch::from(true), ValueMatch::Match);
        assert_eq!(ValueMatch::from(false), ValueMatch::Mismatch);

        assert_eq!(
            fold_all([ComparisonOutcome::Satisfied]),
            ComparisonOutcome::Satisfied
        );
        assert_eq!(
            fold_all([
                ComparisonOutcome::Incomparable,
                ComparisonOutcome::Conflicted
            ]),
            ComparisonOutcome::Incomparable
        );
        assert_eq!(
            fold_all([
                ComparisonOutcome::Conflicted,
                ComparisonOutcome::InsufficientEvidence
            ]),
            ComparisonOutcome::Conflicted
        );
        assert_eq!(
            fold_all([
                ComparisonOutcome::InsufficientEvidence,
                ComparisonOutcome::UnresolvedInput
            ]),
            ComparisonOutcome::InsufficientEvidence
        );
        assert_eq!(
            fold_all([
                ComparisonOutcome::UnresolvedInput,
                ComparisonOutcome::Unknown
            ]),
            ComparisonOutcome::UnresolvedInput
        );
        assert_eq!(
            fold_all([ComparisonOutcome::Unknown]),
            ComparisonOutcome::Unknown
        );
        assert_eq!(
            fold_any([ComparisonOutcome::Satisfied]),
            ComparisonOutcome::Satisfied
        );
        assert_eq!(
            fold_any([
                ComparisonOutcome::Incomparable,
                ComparisonOutcome::Conflicted
            ]),
            ComparisonOutcome::Incomparable
        );
        assert_eq!(
            fold_any([
                ComparisonOutcome::Conflicted,
                ComparisonOutcome::InsufficientEvidence
            ]),
            ComparisonOutcome::Conflicted
        );
        assert_eq!(
            fold_any([
                ComparisonOutcome::InsufficientEvidence,
                ComparisonOutcome::UnresolvedInput
            ]),
            ComparisonOutcome::InsufficientEvidence
        );
        assert_eq!(
            fold_any([
                ComparisonOutcome::UnresolvedInput,
                ComparisonOutcome::Unknown
            ]),
            ComparisonOutcome::UnresolvedInput
        );
        assert_eq!(
            fold_any([ComparisonOutcome::Unknown]),
            ComparisonOutcome::Unknown
        );
        assert_eq!(
            fold_any([ComparisonOutcome::Unsatisfied]),
            ComparisonOutcome::Unsatisfied
        );

        for outcome in [
            ComparisonOutcome::Satisfied,
            ComparisonOutcome::Unsatisfied,
            ComparisonOutcome::Unknown,
            ComparisonOutcome::Conflicted,
            ComparisonOutcome::InsufficientEvidence,
            ComparisonOutcome::UnresolvedInput,
            ComparisonOutcome::Incomparable,
        ] {
            assert_eq!(negate_outcome(negate_outcome(outcome)), outcome);
            assert!(
                expression_reason(outcome)
                    .as_str()
                    .starts_with("EXPRESSION_")
            );
        }
    }
}
