//! CG-06.02 declarative intent and desired-state semantics.

use std::{cmp::Ordering, fmt, ops::Not, str::FromStr};

use crate::{
    declarative_context::{DECLARATIVE_CONTEXT_IR_VERSION, DeclarativeContextVersion},
    identifiers::{
        AcceptanceCriterionId, ConditionId, ConstraintId, DesiredStateId, IntentId, ReferenceId,
    },
    validation::{NonEmptyText, ValidationError, validate_identifier},
};

/// The maximum number of nested logical-expression levels accepted by v1.
pub const MAX_LOGICAL_EXPRESSION_DEPTH: usize = 64;

/// A bounded decimal represented exactly as an integer plus an explicit scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DecimalValue {
    unscaled: i128,
    scale: u8,
}

impl DecimalValue {
    /// The highest supported decimal scale, avoiding unbounded precision.
    pub const MAX_SCALE: u8 = 18;

    /// Creates an exact decimal value from its unscaled integer and scale.
    pub fn new(unscaled: i128, scale: u8) -> Result<Self, ValidationError> {
        if scale > Self::MAX_SCALE {
            return Err(ValidationError::InvalidDeclarativeValue {
                reason: "decimal scale exceeds the supported maximum",
            });
        }
        Ok(Self { unscaled, scale })
    }

    /// Returns the exact unscaled integer.
    #[must_use]
    pub const fn unscaled(self) -> i128 {
        self.unscaled
    }

    /// Returns the explicit decimal scale.
    #[must_use]
    pub const fn scale(self) -> u8 {
        self.scale
    }

    /// Compares two decimals after deterministic scale alignment.
    #[must_use]
    pub fn numeric_cmp(self, other: Self) -> Option<Ordering> {
        let scale = self.scale.max(other.scale);
        let left = scale_value(self.unscaled, self.scale, scale)?;
        let right = scale_value(other.unscaled, other.scale, scale)?;
        Some(left.cmp(&right))
    }
}

impl fmt::Display for DecimalValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let negative = self.unscaled.is_negative();
        let magnitude = self.unscaled.unsigned_abs().to_string();
        if self.scale == 0 {
            return if negative {
                write!(formatter, "-{magnitude}")
            } else {
                formatter.write_str(&magnitude)
            };
        }

        let scale = usize::from(self.scale);
        let digits = if magnitude.len() <= scale {
            format!("{}{}", "0".repeat(scale + 1 - magnitude.len()), magnitude)
        } else {
            magnitude
        };
        let split = digits.len() - scale;
        if negative {
            write!(formatter, "-{}.{}", &digits[..split], &digits[split..])
        } else {
            write!(formatter, "{}.{}", &digits[..split], &digits[split..])
        }
    }
}

impl FromStr for DecimalValue {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty() || value.starts_with('+') || value.contains('e') || value.contains('E')
        {
            return Err(ValidationError::InvalidDeclarativeValue {
                reason: "decimal must use an explicit base-10 form",
            });
        }

        let (negative, unsigned) = value
            .strip_prefix('-')
            .map_or((false, value), |rest| (true, rest));
        let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
        if whole.is_empty()
            || !whole.chars().all(|character| character.is_ascii_digit())
            || !fraction.chars().all(|character| character.is_ascii_digit())
            || fraction.len() > usize::from(Self::MAX_SCALE)
            || (fraction.is_empty() && value.ends_with('.'))
        {
            return Err(ValidationError::InvalidDeclarativeValue {
                reason: "decimal must contain only ASCII digits and one optional fraction",
            });
        }

        let scale =
            u8::try_from(fraction.len()).map_err(|_| ValidationError::InvalidDeclarativeValue {
                reason: "decimal scale exceeds the supported maximum",
            })?;
        let factor = power_of_ten(scale).ok_or(ValidationError::InvalidDeclarativeValue {
            reason: "decimal scale cannot be represented",
        })?;
        let whole =
            whole
                .parse::<i128>()
                .map_err(|_| ValidationError::InvalidDeclarativeValue {
                    reason: "decimal magnitude exceeds the supported range",
                })?;
        let fraction = if fraction.is_empty() {
            0
        } else {
            fraction
                .parse::<i128>()
                .map_err(|_| ValidationError::InvalidDeclarativeValue {
                    reason: "decimal fraction exceeds the supported range",
                })?
        };
        let magnitude = whole
            .checked_mul(factor)
            .and_then(|value| value.checked_add(fraction))
            .ok_or(ValidationError::InvalidDeclarativeValue {
                reason: "decimal magnitude exceeds the supported range",
            })?;
        let unscaled = if negative {
            magnitude
                .checked_neg()
                .ok_or(ValidationError::InvalidDeclarativeValue {
                    reason: "decimal magnitude exceeds the supported range",
                })?
        } else {
            magnitude
        };
        Self::new(unscaled, scale)
    }
}

fn power_of_ten(scale: u8) -> Option<i128> {
    (0..scale).try_fold(1_i128, |value, _| value.checked_mul(10))
}

fn scale_value(value: i128, from: u8, to: u8) -> Option<i128> {
    value.checked_mul(power_of_ten(to.checked_sub(from)?)?)
}

/// An identifier-like symbol value with no provider-specific enum registry.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct SymbolValue(String);

impl SymbolValue {
    /// Creates a validated symbol using the shared identifier alphabet.
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        Ok(Self(validate_identifier(value)?))
    }

    /// Returns the canonical symbol text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SymbolValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The finite scalar categories supported by declarative values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum ValueKind {
    Boolean,
    Integer,
    Decimal,
    String,
    Symbol,
}

impl ValueKind {
    /// Returns the stable machine-readable kind name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Boolean => "BOOLEAN",
            Self::Integer => "INTEGER",
            Self::Decimal => "DECIMAL",
            Self::String => "STRING",
            Self::Symbol => "SYMBOL",
        }
    }
}

/// A finite, explicitly typed declarative value.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum TypedValue {
    Boolean(bool),
    Integer(i64),
    Decimal(DecimalValue),
    String(NonEmptyText),
    Symbol(SymbolValue),
    Set(Vec<TypedValue>),
}

/// Alias emphasizing that the value is used by desired-state contracts.
pub type DesiredValue = TypedValue;

impl TypedValue {
    /// Creates a validated string value without applying coercion.
    pub fn string(value: impl Into<String>) -> Result<Self, ValidationError> {
        Ok(Self::String(NonEmptyText::new_for_field(
            value,
            "desired_value",
        )?))
    }

    /// Creates a validated symbol value.
    pub fn symbol(value: impl Into<String>) -> Result<Self, ValidationError> {
        Ok(Self::Symbol(SymbolValue::new(value)?))
    }

    /// Creates a non-empty homogeneous set of scalar values.
    pub fn set(values: Vec<Self>) -> Result<Self, ValidationError> {
        let first = values
            .first()
            .ok_or(ValidationError::InvalidDeclarativeValue {
                reason: "typed sets must not be empty",
            })?;
        let kind = first
            .kind()
            .ok_or(ValidationError::InvalidDeclarativeValue {
                reason: "set elements must be scalar values",
            })?;
        if values.iter().any(|value| value.kind() != Some(kind)) {
            return Err(ValidationError::InvalidDeclarativeValue {
                reason: "set elements must have one scalar type",
            });
        }
        Ok(Self::Set(values))
    }

    /// Returns the scalar kind, or `None` for a set value.
    #[must_use]
    pub const fn kind(&self) -> Option<ValueKind> {
        match self {
            Self::Boolean(_) => Some(ValueKind::Boolean),
            Self::Integer(_) => Some(ValueKind::Integer),
            Self::Decimal(_) => Some(ValueKind::Decimal),
            Self::String(_) => Some(ValueKind::String),
            Self::Symbol(_) => Some(ValueKind::Symbol),
            Self::Set(_) => None,
        }
    }

    /// Returns whether the value is a supported numeric category.
    #[must_use]
    pub const fn is_numeric(&self) -> bool {
        matches!(self, Self::Integer(_) | Self::Decimal(_))
    }

    /// Validates public enum construction, including values built without helpers.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if let Self::Set(values) = self {
            let first = values
                .first()
                .ok_or(ValidationError::InvalidDeclarativeValue {
                    reason: "typed sets must not be empty",
                })?;
            let kind = first
                .kind()
                .ok_or(ValidationError::InvalidDeclarativeValue {
                    reason: "set elements must be scalar values",
                })?;
            if values.iter().any(|value| value.kind() != Some(kind)) {
                return Err(ValidationError::InvalidDeclarativeValue {
                    reason: "set elements must have one scalar type",
                });
            }
        }
        Ok(())
    }
}

impl fmt::Display for TypedValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Boolean(value) => write!(formatter, "BOOLEAN:{value}"),
            Self::Integer(value) => write!(formatter, "INTEGER:{value}"),
            Self::Decimal(value) => write!(formatter, "DECIMAL:{value}"),
            Self::String(value) => write!(formatter, "STRING:{}", value.as_str()),
            Self::Symbol(value) => write!(formatter, "SYMBOL:{value}"),
            Self::Set(values) => {
                formatter.write_str("SET:[")?;
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(",")?;
                    }
                    write!(formatter, "{value}")?;
                }
                formatter.write_str("]")
            }
        }
    }
}

/// A project-agnostic typed subject/property path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct SubjectPath(Vec<NonEmptyText>);

/// Alias for consumers that use subject terminology.
pub type DesiredSubject = SubjectPath;

impl SubjectPath {
    /// Creates a path from identifier-like segments.
    pub fn new<I, S>(segments: I) -> Result<Self, ValidationError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let segments = segments
            .into_iter()
            .map(|segment| {
                let segment = validate_identifier(segment)?;
                NonEmptyText::new_for_field(segment, "subject_path")
            })
            .collect::<Result<Vec<_>, _>>()?;
        if segments.is_empty() {
            return Err(ValidationError::InvalidDeclarativeValue {
                reason: "subject path must contain at least one segment",
            });
        }
        Ok(Self(segments))
    }

    /// Returns the validated path segments.
    #[must_use]
    pub fn segments(&self) -> &[NonEmptyText] {
        &self.0
    }
}

impl FromStr for SubjectPath {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value.split('.'))
    }
}

impl fmt::Display for SubjectPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, segment) in self.0.iter().enumerate() {
            if index > 0 {
                formatter.write_str(".")?;
            }
            formatter.write_str(segment.as_str())?;
        }
        Ok(())
    }
}

/// The finite comparison operations available to a desired condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum ComparisonOperator {
    Equals,
    NotEquals,
    GreaterThan,
    GreaterOrEqual,
    LessThan,
    LessOrEqual,
    Present,
    Absent,
    In,
    Contains,
}

impl ComparisonOperator {
    /// Returns the stable machine-readable operator name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Equals => "EQUALS",
            Self::NotEquals => "NOT_EQUALS",
            Self::GreaterThan => "GREATER_THAN",
            Self::GreaterOrEqual => "GREATER_OR_EQUAL",
            Self::LessThan => "LESS_THAN",
            Self::LessOrEqual => "LESS_OR_EQUAL",
            Self::Present => "PRESENT",
            Self::Absent => "ABSENT",
            Self::In => "IN",
            Self::Contains => "CONTAINS",
        }
    }
}

impl fmt::Display for ComparisonOperator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ComparisonOperator {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "EQUALS" => Ok(Self::Equals),
            "NOT_EQUALS" => Ok(Self::NotEquals),
            "GREATER_THAN" => Ok(Self::GreaterThan),
            "GREATER_OR_EQUAL" => Ok(Self::GreaterOrEqual),
            "LESS_THAN" => Ok(Self::LessThan),
            "LESS_OR_EQUAL" => Ok(Self::LessOrEqual),
            "PRESENT" => Ok(Self::Present),
            "ABSENT" => Ok(Self::Absent),
            "IN" => Ok(Self::In),
            "CONTAINS" => Ok(Self::Contains),
            value => Err(ValidationError::UnknownDomainValue {
                field: "comparison_operator",
                value: value.to_owned(),
            }),
        }
    }
}

/// One explicitly typed desired condition.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct DesiredCondition {
    id: ConditionId,
    subject: SubjectPath,
    operator: ComparisonOperator,
    expected: Option<TypedValue>,
}

impl DesiredCondition {
    /// Creates and validates a desired condition.
    pub fn new(
        id: ConditionId,
        subject: SubjectPath,
        operator: ComparisonOperator,
        expected: Option<TypedValue>,
    ) -> Result<Self, ValidationError> {
        if let Some(value) = expected.as_ref() {
            value.validate()?;
        }
        let needs_value = !matches!(
            operator,
            ComparisonOperator::Present | ComparisonOperator::Absent
        );
        if needs_value != expected.is_some() {
            return Err(ValidationError::InvalidDeclarativeCondition {
                reason: if needs_value {
                    "operator requires a value"
                } else {
                    "presence operators must not carry a value"
                },
            });
        }
        if matches!(
            operator,
            ComparisonOperator::GreaterThan
                | ComparisonOperator::GreaterOrEqual
                | ComparisonOperator::LessThan
                | ComparisonOperator::LessOrEqual
        ) && !expected.as_ref().is_some_and(TypedValue::is_numeric)
        {
            return Err(ValidationError::InvalidDeclarativeCondition {
                reason: "numeric operators require an integer or decimal value",
            });
        }
        if matches!(operator, ComparisonOperator::In)
            && !expected
                .as_ref()
                .is_some_and(|value| matches!(value, TypedValue::Set(_)))
        {
            return Err(ValidationError::InvalidDeclarativeCondition {
                reason: "IN requires a typed set value",
            });
        }
        Ok(Self {
            id,
            subject,
            operator,
            expected,
        })
    }

    /// Returns the condition identity.
    #[must_use]
    pub fn id(&self) -> &ConditionId {
        &self.id
    }

    /// Returns the target subject/property path.
    #[must_use]
    pub fn subject(&self) -> &SubjectPath {
        &self.subject
    }

    /// Returns the comparison operator.
    #[must_use]
    pub const fn operator(&self) -> ComparisonOperator {
        self.operator
    }

    /// Returns the explicit expected value, if the operator requires one.
    #[must_use]
    pub fn expected(&self) -> Option<&TypedValue> {
        self.expected.as_ref()
    }
}

/// A finite logical expression over desired-condition identities.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum ConditionExpression {
    Condition(ConditionId),
    All(Vec<Self>),
    Any(Vec<Self>),
    Not(Box<Self>),
}

impl ConditionExpression {
    /// References one declared desired condition.
    #[must_use]
    pub fn condition(id: ConditionId) -> Self {
        Self::Condition(id)
    }

    /// Creates a non-empty conjunction.
    pub fn all(expressions: Vec<Self>) -> Result<Self, ValidationError> {
        if expressions.is_empty() {
            return Err(ValidationError::InvalidDeclarativeExpression {
                reason: "ALL expressions must not be empty",
            });
        }
        Ok(Self::All(expressions).canonicalize())
    }

    /// Creates a non-empty disjunction.
    pub fn any(expressions: Vec<Self>) -> Result<Self, ValidationError> {
        if expressions.is_empty() {
            return Err(ValidationError::InvalidDeclarativeExpression {
                reason: "ANY expressions must not be empty",
            });
        }
        Ok(Self::Any(expressions).canonicalize())
    }

    /// Negates one finite expression.
    #[must_use]
    pub fn negate(expression: Self) -> Self {
        Self::Not(Box::new(expression))
    }

    /// Returns a canonical order for commutative ALL/ANY child collections.
    #[must_use]
    pub fn canonicalize(self) -> Self {
        match self {
            Self::All(expressions) => Self::All(canonicalize_children(expressions)),
            Self::Any(expressions) => Self::Any(canonicalize_children(expressions)),
            Self::Not(expression) => Self::Not(Box::new(expression.canonicalize())),
            Self::Condition(_) => self,
        }
    }

    /// Validates references and finite nesting against declared conditions.
    pub fn validate_against(
        &self,
        known_conditions: &std::collections::BTreeSet<ConditionId>,
    ) -> Result<(), ValidationError> {
        self.validate_at_depth(known_conditions, 0)
    }

    fn validate_at_depth(
        &self,
        known_conditions: &std::collections::BTreeSet<ConditionId>,
        depth: usize,
    ) -> Result<(), ValidationError> {
        if depth > MAX_LOGICAL_EXPRESSION_DEPTH {
            return Err(ValidationError::InvalidDeclarativeExpression {
                reason: "logical expression exceeds the supported nesting depth",
            });
        }
        match self {
            Self::Condition(id) => {
                if known_conditions.contains(id) {
                    Ok(())
                } else {
                    Err(ValidationError::MissingDeclarativeIdentity {
                        kind: "condition",
                        id: id.to_string(),
                    })
                }
            }
            Self::All(expressions) | Self::Any(expressions) => {
                if expressions.is_empty() {
                    return Err(ValidationError::InvalidDeclarativeExpression {
                        reason: "logical expression must not be empty",
                    });
                }
                for expression in expressions {
                    expression.validate_at_depth(known_conditions, depth + 1)?;
                }
                Ok(())
            }
            Self::Not(expression) => expression.validate_at_depth(known_conditions, depth + 1),
        }
    }

    fn canonical_key(&self) -> String {
        match self {
            Self::Condition(id) => format!("CONDITION:{id}"),
            Self::All(expressions) => format!(
                "ALL:[{}]",
                expressions
                    .iter()
                    .map(Self::canonical_key)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Self::Any(expressions) => format!(
                "ANY:[{}]",
                expressions
                    .iter()
                    .map(Self::canonical_key)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Self::Not(expression) => format!("NOT:{}", expression.canonical_key()),
        }
    }
}

impl Not for ConditionExpression {
    type Output = Self;

    fn not(self) -> Self::Output {
        Self::Not(Box::new(self))
    }
}

fn canonicalize_children(mut expressions: Vec<ConditionExpression>) -> Vec<ConditionExpression> {
    expressions = expressions
        .into_iter()
        .map(ConditionExpression::canonicalize)
        .collect();
    expressions.sort_by_key(ConditionExpression::canonical_key);
    expressions
}

/// A goal-space restriction that does not grant authority.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct DeclarativeConstraint {
    id: ConstraintId,
    expression: ConditionExpression,
}

impl DeclarativeConstraint {
    /// Creates a declarative goal-space constraint.
    #[must_use]
    pub fn new(id: ConstraintId, expression: ConditionExpression) -> Self {
        Self {
            id,
            expression: expression.canonicalize(),
        }
    }

    /// Returns the constraint identity.
    #[must_use]
    pub fn id(&self) -> &ConstraintId {
        &self.id
    }

    /// Returns the constrained desired-state expression.
    #[must_use]
    pub fn expression(&self) -> &ConditionExpression {
        &self.expression
    }
}

/// An explicit criterion describing how desired-state satisfaction is checked.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct AcceptanceCriterion {
    id: AcceptanceCriterionId,
    description: NonEmptyText,
    expression: ConditionExpression,
}

impl AcceptanceCriterion {
    /// Creates an acceptance criterion linked to condition identities.
    pub fn new(
        id: AcceptanceCriterionId,
        description: impl Into<String>,
        expression: ConditionExpression,
    ) -> Result<Self, ValidationError> {
        Ok(Self {
            id,
            description: NonEmptyText::new_for_field(description, "acceptance_criterion")?,
            expression: expression.canonicalize(),
        })
    }

    /// Returns the criterion identity.
    #[must_use]
    pub fn id(&self) -> &AcceptanceCriterionId {
        &self.id
    }

    /// Returns the criterion description.
    #[must_use]
    pub fn description(&self) -> &str {
        self.description.as_str()
    }

    /// Returns the condition expression verified by this criterion.
    #[must_use]
    pub fn expression(&self) -> &ConditionExpression {
        &self.expression
    }
}

/// A versioned set of desired conditions, constraints and acceptance criteria.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesiredState {
    version: DeclarativeContextVersion,
    id: DesiredStateId,
    conditions: Vec<DesiredCondition>,
    expression: ConditionExpression,
    constraints: Vec<DeclarativeConstraint>,
    acceptance_criteria: Vec<AcceptanceCriterion>,
}

impl DesiredState {
    /// Creates a supported v1 desired state with canonical collection ordering.
    pub fn new(
        id: DesiredStateId,
        conditions: Vec<DesiredCondition>,
        expression: ConditionExpression,
        constraints: Vec<DeclarativeConstraint>,
        acceptance_criteria: Vec<AcceptanceCriterion>,
    ) -> Result<Self, ValidationError> {
        Self::new_with_version(
            DECLARATIVE_CONTEXT_IR_VERSION,
            id,
            conditions,
            expression,
            constraints,
            acceptance_criteria,
        )
    }

    /// Creates a desired state after validating its explicit IR version.
    pub fn new_with_version(
        version: DeclarativeContextVersion,
        id: DesiredStateId,
        mut conditions: Vec<DesiredCondition>,
        expression: ConditionExpression,
        mut constraints: Vec<DeclarativeConstraint>,
        mut acceptance_criteria: Vec<AcceptanceCriterion>,
    ) -> Result<Self, ValidationError> {
        version.ensure_supported()?;
        if conditions.is_empty() {
            return Err(ValidationError::EmptyRelationship {
                field: "desired_conditions",
            });
        }
        conditions.sort_by(|left, right| left.id.cmp(&right.id));
        ensure_unique_conditions(&conditions)?;
        constraints.sort_by(|left, right| left.id.cmp(&right.id));
        ensure_unique_constraints(&constraints)?;
        acceptance_criteria.sort_by(|left, right| left.id.cmp(&right.id));
        ensure_unique_criteria(&acceptance_criteria)?;
        let known_conditions = conditions
            .iter()
            .map(|condition| condition.id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let expression = expression.canonicalize();
        expression.validate_against(&known_conditions)?;
        for constraint in &constraints {
            constraint.expression.validate_against(&known_conditions)?;
        }
        for criterion in &acceptance_criteria {
            criterion.expression.validate_against(&known_conditions)?;
        }
        Ok(Self {
            version,
            id,
            conditions,
            expression,
            constraints,
            acceptance_criteria,
        })
    }

    /// Returns the IR version.
    #[must_use]
    pub const fn version(&self) -> DeclarativeContextVersion {
        self.version
    }

    /// Returns the desired-state identity.
    #[must_use]
    pub fn id(&self) -> &DesiredStateId {
        &self.id
    }

    /// Returns conditions in canonical identity order.
    #[must_use]
    pub fn conditions(&self) -> &[DesiredCondition] {
        &self.conditions
    }

    /// Returns the finite logical composition of conditions.
    #[must_use]
    pub fn expression(&self) -> &ConditionExpression {
        &self.expression
    }

    /// Returns declarative goal-space constraints in canonical order.
    #[must_use]
    pub fn constraints(&self) -> &[DeclarativeConstraint] {
        &self.constraints
    }

    /// Returns acceptance criteria in canonical order.
    #[must_use]
    pub fn acceptance_criteria(&self) -> &[AcceptanceCriterion] {
        &self.acceptance_criteria
    }
}

fn ensure_unique_conditions(conditions: &[DesiredCondition]) -> Result<(), ValidationError> {
    for pair in conditions.windows(2) {
        if pair[0].id == pair[1].id {
            return Err(ValidationError::DuplicateDeclarativeIdentity {
                kind: "condition",
                id: pair[0].id.to_string(),
            });
        }
    }
    Ok(())
}

fn ensure_unique_constraints(constraints: &[DeclarativeConstraint]) -> Result<(), ValidationError> {
    for pair in constraints.windows(2) {
        if pair[0].id == pair[1].id {
            return Err(ValidationError::DuplicateDeclarativeIdentity {
                kind: "constraint",
                id: pair[0].id.to_string(),
            });
        }
    }
    Ok(())
}

fn ensure_unique_criteria(criteria: &[AcceptanceCriterion]) -> Result<(), ValidationError> {
    for pair in criteria.windows(2) {
        if pair[0].id == pair[1].id {
            return Err(ValidationError::DuplicateDeclarativeIdentity {
                kind: "acceptance_criterion",
                id: pair[0].id.to_string(),
            });
        }
    }
    Ok(())
}

/// Original caller input retained separately from normalized desired structure.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum OriginalInput {
    Inline(NonEmptyText),
    Reference(ReferenceId),
}

impl OriginalInput {
    /// Retains the caller input as validated content.
    pub fn inline(value: impl Into<String>) -> Result<Self, ValidationError> {
        Ok(Self::Inline(NonEmptyText::new_for_field(
            value,
            "original_input",
        )?))
    }

    /// Retains only a typed reference to external caller input.
    #[must_use]
    pub fn reference(id: ReferenceId) -> Self {
        Self::Reference(id)
    }
}

/// A caller intent containing one explicit desired state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Intent {
    version: DeclarativeContextVersion,
    id: IntentId,
    desired_state: DesiredState,
    original_input: Option<OriginalInput>,
}

impl Intent {
    /// Creates a v1 intent without assuming an original-input representation.
    #[must_use]
    pub fn new(id: IntentId, desired_state: DesiredState) -> Self {
        Self {
            version: DECLARATIVE_CONTEXT_IR_VERSION,
            id,
            desired_state,
            original_input: None,
        }
    }

    /// Attaches caller input without changing the normalized desired state.
    #[must_use]
    pub fn with_original_input(mut self, original_input: OriginalInput) -> Self {
        self.original_input = Some(original_input);
        self
    }

    /// Returns the IR version.
    #[must_use]
    pub const fn version(&self) -> DeclarativeContextVersion {
        self.version
    }

    /// Returns the intent identity.
    #[must_use]
    pub fn id(&self) -> &IntentId {
        &self.id
    }

    /// Returns the normalized desired state.
    #[must_use]
    pub fn desired_state(&self) -> &DesiredState {
        &self.desired_state
    }

    /// Returns the optional original caller input/reference.
    #[must_use]
    pub fn original_input(&self) -> Option<&OriginalInput> {
        self.original_input.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identifiers::{AcceptanceCriterionId, ConstraintId};

    fn condition(id: &str, subject: &str, value: TypedValue) -> DesiredCondition {
        DesiredCondition::new(
            ConditionId::new(id).unwrap(),
            SubjectPath::from_str(subject).unwrap(),
            ComparisonOperator::Equals,
            Some(value),
        )
        .unwrap()
    }

    #[test]
    fn decimal_values_are_exact_and_locale_independent() {
        let value = DecimalValue::from_str("95.00").unwrap();
        assert_eq!(value.unscaled(), 9500);
        assert_eq!(value.scale(), 2);
        assert_eq!(value.to_string(), "95.00");
        assert_eq!(
            DecimalValue::from_str("95.0").unwrap().numeric_cmp(value),
            Some(Ordering::Equal)
        );
        assert!(DecimalValue::from_str("95,0").is_err());
        assert!(DecimalValue::from_str("1e2").is_err());
        assert!(DecimalValue::new(1, DecimalValue::MAX_SCALE + 1).is_err());
    }

    #[test]
    fn typed_values_require_explicit_categories_and_homogeneous_sets() {
        let string = TypedValue::string("coverage").unwrap();
        let symbol = TypedValue::symbol("percent").unwrap();
        assert_eq!(string.kind(), Some(ValueKind::String));
        assert_eq!(symbol.kind(), Some(ValueKind::Symbol));
        assert!(TypedValue::set(vec![]).is_err());
        assert!(TypedValue::set(vec![TypedValue::Integer(1), TypedValue::Boolean(true)]).is_err());
        assert!(TypedValue::set(vec![TypedValue::Set(vec![])]).is_err());
        assert!(TypedValue::set(vec![TypedValue::Integer(1), TypedValue::Integer(2)]).is_ok());
    }

    #[test]
    fn condition_validation_rejects_implicit_or_ambiguous_values() {
        let subject = SubjectPath::from_str("coverage.percent").unwrap();
        assert_eq!(subject.to_string(), "coverage.percent");
        assert!(SubjectPath::from_str("coverage..percent").is_err());
        assert!(
            DesiredCondition::new(
                ConditionId::new("present").unwrap(),
                subject.clone(),
                ComparisonOperator::Present,
                Some(TypedValue::Boolean(true)),
            )
            .is_err()
        );
        assert!(
            DesiredCondition::new(
                ConditionId::new("threshold").unwrap(),
                subject.clone(),
                ComparisonOperator::GreaterOrEqual,
                Some(TypedValue::string("95").unwrap()),
            )
            .is_err()
        );
        assert!(
            DesiredCondition::new(
                ConditionId::new("membership").unwrap(),
                subject,
                ComparisonOperator::In,
                Some(TypedValue::Integer(1)),
            )
            .is_err()
        );
        assert!(ComparisonOperator::from_str("equals").is_err());
    }

    #[test]
    fn desired_state_canonicalizes_collections_and_logical_composition() {
        let first = condition("b", "coverage.percent", TypedValue::Integer(95));
        let second = condition("a", "architecture.violation", TypedValue::Boolean(false));
        let expression = ConditionExpression::all(vec![
            ConditionExpression::condition(first.id().clone()),
            ConditionExpression::condition(second.id().clone()),
        ])
        .unwrap();
        let criterion = AcceptanceCriterion::new(
            AcceptanceCriterionId::new("criterion-1").unwrap(),
            "Both desired conditions hold",
            expression.clone(),
        )
        .unwrap();
        let state = DesiredState::new(
            DesiredStateId::new("desired-1").unwrap(),
            vec![first, second],
            expression,
            vec![DeclarativeConstraint::new(
                ConstraintId::new("constraint-1").unwrap(),
                ConditionExpression::condition(ConditionId::new("a").unwrap()),
            )],
            vec![criterion],
        )
        .unwrap();
        assert_eq!(state.conditions()[0].id().as_str(), "a");
        assert_eq!(state.conditions()[1].id().as_str(), "b");
        assert_eq!(
            state.acceptance_criteria()[0].description(),
            "Both desired conditions hold"
        );
        let intent = Intent::new(IntentId::new("intent-1").unwrap(), state).with_original_input(
            OriginalInput::inline("ensure architecture and coverage").unwrap(),
        );
        assert_eq!(intent.version(), DeclarativeContextVersion::V1);
        assert!(intent.original_input().is_some());
    }

    #[test]
    fn expressions_are_finite_explicit_and_fail_closed() {
        assert!(ConditionExpression::all(vec![]).is_err());
        assert!(ConditionExpression::any(vec![]).is_err());
        let condition_id = ConditionId::new("condition-1").unwrap();
        let expression =
            ConditionExpression::negate(ConditionExpression::condition(condition_id.clone()));
        let mut known = std::collections::BTreeSet::new();
        known.insert(condition_id);
        assert!(expression.validate_against(&known).is_ok());
        assert!(
            ConditionExpression::condition(ConditionId::new("missing").unwrap())
                .validate_against(&known)
                .is_err()
        );
    }

    #[test]
    fn desired_state_rejects_duplicate_and_missing_condition_references() {
        let duplicate_a = condition("same", "a", TypedValue::Boolean(true));
        let duplicate_b = condition("same", "b", TypedValue::Boolean(false));
        let duplicate = DesiredState::new(
            DesiredStateId::new("desired-duplicate").unwrap(),
            vec![duplicate_a, duplicate_b],
            ConditionExpression::condition(ConditionId::new("same").unwrap()),
            vec![],
            vec![],
        );
        assert!(matches!(
            duplicate,
            Err(ValidationError::DuplicateDeclarativeIdentity {
                kind: "condition",
                ..
            })
        ));

        let missing = DesiredState::new(
            DesiredStateId::new("desired-missing").unwrap(),
            vec![condition("known", "a", TypedValue::Boolean(true))],
            ConditionExpression::condition(ConditionId::new("missing").unwrap()),
            vec![],
            vec![],
        );
        assert!(matches!(
            missing,
            Err(ValidationError::MissingDeclarativeIdentity {
                kind: "condition",
                ..
            })
        ));
    }

    #[test]
    fn decimal_values_cover_strict_parsing_and_scale_alignment() {
        assert_eq!(DecimalValue::from_str("0").unwrap().to_string(), "0");
        assert_eq!(DecimalValue::from_str("-12").unwrap().to_string(), "-12");
        assert_eq!(DecimalValue::from_str("0.01").unwrap().to_string(), "0.01");
        assert_eq!(DecimalValue::from_str("-0.1").unwrap().to_string(), "-0.1");
        assert_eq!(
            DecimalValue::from_str("12.340").unwrap().to_string(),
            "12.340"
        );
        assert!(DecimalValue::from_str(".1").is_err());
        assert!(DecimalValue::from_str("-").is_err());
        assert!(DecimalValue::from_str("-.1").is_err());
        assert!(DecimalValue::from_str("1.").is_err());
        assert!(DecimalValue::from_str("1.2.3").is_err());
        assert!(DecimalValue::from_str("1_000").is_err());
        assert!(DecimalValue::from_str("1e2").is_err());
        assert!(DecimalValue::from_str("1E2").is_err());
        assert!(DecimalValue::from_str("+1").is_err());
        assert!(DecimalValue::from_str("1,2").is_err());
        assert!(DecimalValue::from_str("1.1234567890123456789").is_err());

        let max = i128::MAX.to_string();
        assert!(DecimalValue::from_str(&format!("{max}.0")).is_err());
        assert!(DecimalValue::from_str(&format!("-{}.0", i128::MAX)).is_err());
        assert!(DecimalValue::from_str(&i128::MIN.to_string()).is_err());
        assert_eq!(power_of_ten(0), Some(1));
        assert!(power_of_ten(39).is_none());
        assert!(scale_value(1, 2, 1).is_none());
        assert_eq!(scale_value(12, 1, 3), Some(1200));
        assert_eq!(
            DecimalValue::from_str("1.20")
                .unwrap()
                .numeric_cmp(DecimalValue::from_str("1.2").unwrap()),
            Some(Ordering::Equal)
        );
        assert_eq!(
            DecimalValue::from_str("-1.20")
                .unwrap()
                .numeric_cmp(DecimalValue::from_str("1.2").unwrap()),
            Some(Ordering::Less)
        );
    }

    #[test]
    fn typed_values_cover_all_kinds_display_and_public_enum_validation() {
        let values = [
            TypedValue::Boolean(true),
            TypedValue::Integer(-4),
            TypedValue::Decimal(DecimalValue::from_str("2.50").unwrap()),
            TypedValue::string("text").unwrap(),
            TypedValue::symbol("ready").unwrap(),
        ];
        let kinds = [
            ValueKind::Boolean,
            ValueKind::Integer,
            ValueKind::Decimal,
            ValueKind::String,
            ValueKind::Symbol,
        ];
        for (value, kind) in values.iter().zip(kinds) {
            assert_eq!(value.kind(), Some(kind));
            assert!(!kind.as_str().is_empty());
            assert!(value.validate().is_ok());
        }
        assert!(!values[0].is_numeric());
        assert!(values[1].is_numeric());
        assert!(values[2].is_numeric());
        assert_eq!(values[3].to_string(), "STRING:text");
        assert_eq!(values[4].to_string(), "SYMBOL:ready");
        assert_eq!(values[0].to_string(), "BOOLEAN:true");
        assert_eq!(values[1].to_string(), "INTEGER:-4");
        assert_eq!(values[2].to_string(), "DECIMAL:2.50");
        assert_eq!(SymbolValue::new("approved").unwrap().as_str(), "approved");
        assert!(SymbolValue::new("not approved").is_err());

        let set = TypedValue::set(vec![TypedValue::Integer(1), TypedValue::Integer(2)]).unwrap();
        assert_eq!(set.kind(), None);
        assert!(!set.is_numeric());
        assert_eq!(set.to_string(), "SET:[INTEGER:1,INTEGER:2]");
        assert!(set.validate().is_ok());
        assert!(TypedValue::Set(vec![]).validate().is_err());
        assert!(
            TypedValue::Set(vec![TypedValue::Set(vec![])])
                .validate()
                .is_err()
        );
        assert!(
            TypedValue::Set(vec![TypedValue::Integer(1), TypedValue::Boolean(true)])
                .validate()
                .is_err()
        );
        assert!(TypedValue::set(vec![TypedValue::Set(vec![])]).is_err());
    }

    #[test]
    fn subject_paths_and_comparison_operators_are_strict_and_round_trip() {
        let path = SubjectPath::new(["project", "status"]).unwrap();
        assert_eq!(path.segments().len(), 2);
        assert_eq!(path.to_string(), "project.status");
        assert_eq!(SubjectPath::from_str("project.status").unwrap(), path);
        assert!(SubjectPath::new(Vec::<String>::new()).is_err());
        assert!(SubjectPath::from_str("").is_err());
        assert!(SubjectPath::from_str("project..status").is_err());
        assert!(SubjectPath::from_str("project/").is_err());

        let operators = [
            ComparisonOperator::Equals,
            ComparisonOperator::NotEquals,
            ComparisonOperator::GreaterThan,
            ComparisonOperator::GreaterOrEqual,
            ComparisonOperator::LessThan,
            ComparisonOperator::LessOrEqual,
            ComparisonOperator::Present,
            ComparisonOperator::Absent,
            ComparisonOperator::In,
            ComparisonOperator::Contains,
        ];
        for operator in operators {
            assert_eq!(
                ComparisonOperator::from_str(operator.as_str()).unwrap(),
                operator
            );
            assert_eq!(operator.to_string(), operator.as_str());
        }
        assert!(ComparisonOperator::from_str("equals").is_err());
    }

    #[test]
    fn desired_conditions_cover_presence_numeric_membership_and_accessors() {
        let subject = SubjectPath::from_str("project.status").unwrap();
        let id = ConditionId::new("condition-1").unwrap();
        let condition = DesiredCondition::new(
            id.clone(),
            subject.clone(),
            ComparisonOperator::Contains,
            Some(TypedValue::string("ready").unwrap()),
        )
        .unwrap();
        assert_eq!(condition.id(), &id);
        assert_eq!(condition.subject(), &subject);
        assert_eq!(condition.operator(), ComparisonOperator::Contains);
        assert!(condition.expected().is_some());

        for operator in [ComparisonOperator::Equals, ComparisonOperator::NotEquals] {
            assert!(
                DesiredCondition::new(
                    ConditionId::new(format!("value-{}", operator.as_str())).unwrap(),
                    subject.clone(),
                    operator,
                    None,
                )
                .is_err()
            );
        }
        for operator in [ComparisonOperator::Present, ComparisonOperator::Absent] {
            let presence = DesiredCondition::new(
                ConditionId::new(format!("presence-{}", operator.as_str())).unwrap(),
                subject.clone(),
                operator,
                None,
            )
            .unwrap();
            assert!(presence.expected().is_none());
            assert!(
                DesiredCondition::new(
                    ConditionId::new(format!("invalid-{}", operator.as_str())).unwrap(),
                    subject.clone(),
                    operator,
                    Some(TypedValue::Boolean(true)),
                )
                .is_err()
            );
        }
        for operator in [
            ComparisonOperator::GreaterThan,
            ComparisonOperator::GreaterOrEqual,
            ComparisonOperator::LessThan,
            ComparisonOperator::LessOrEqual,
        ] {
            assert!(
                DesiredCondition::new(
                    ConditionId::new(format!("numeric-{}", operator.as_str())).unwrap(),
                    subject.clone(),
                    operator,
                    Some(TypedValue::Decimal(DecimalValue::from_str("1.5").unwrap())),
                )
                .is_ok()
            );
            assert!(
                DesiredCondition::new(
                    ConditionId::new(format!("bad-numeric-{}", operator.as_str())).unwrap(),
                    subject.clone(),
                    operator,
                    Some(TypedValue::string("1.5").unwrap()),
                )
                .is_err()
            );
        }
        assert!(
            DesiredCondition::new(
                ConditionId::new("in-set").unwrap(),
                subject.clone(),
                ComparisonOperator::In,
                Some(TypedValue::set(vec![TypedValue::symbol("ready").unwrap()]).unwrap()),
            )
            .is_ok()
        );
        assert!(
            DesiredCondition::new(
                ConditionId::new("in-scalar").unwrap(),
                subject,
                ComparisonOperator::In,
                Some(TypedValue::Integer(1)),
            )
            .is_err()
        );
    }

    #[test]
    fn expressions_validate_shape_references_depth_and_canonical_order() {
        let first = ConditionId::new("a").unwrap();
        let second = ConditionId::new("b").unwrap();
        let all = ConditionExpression::all(vec![
            ConditionExpression::condition(second.clone()),
            ConditionExpression::condition(first.clone()),
        ])
        .unwrap();
        assert!(matches!(all, ConditionExpression::All(_)));
        assert_eq!(all.clone().canonicalize(), all);
        let any = ConditionExpression::any(vec![
            all.clone(),
            ConditionExpression::condition(first.clone()),
        ])
        .unwrap();
        assert!(matches!(any, ConditionExpression::Any(_)));
        let negated = !ConditionExpression::condition(first.clone());
        assert!(matches!(negated, ConditionExpression::Not(_)));
        assert_eq!(
            ConditionExpression::negate(any.clone()).canonicalize(),
            ConditionExpression::negate(any)
        );

        let mut known = std::collections::BTreeSet::new();
        known.insert(first.clone());
        known.insert(second);
        assert!(all.validate_against(&known).is_ok());
        assert!(
            ConditionExpression::All(vec![])
                .validate_against(&known)
                .is_err()
        );
        assert!(
            ConditionExpression::Any(vec![])
                .validate_against(&known)
                .is_err()
        );
        assert!(
            ConditionExpression::condition(ConditionId::new("unknown").unwrap())
                .validate_against(&known)
                .is_err()
        );

        let mut deep = ConditionExpression::condition(first);
        for _ in 0..=MAX_LOGICAL_EXPRESSION_DEPTH {
            deep = ConditionExpression::Not(Box::new(deep));
        }
        assert!(deep.validate_against(&known).is_err());
    }

    #[test]
    fn constraints_criteria_and_original_input_expose_validated_data() {
        let id = ConstraintId::new("constraint-1").unwrap();
        let expression = ConditionExpression::condition(ConditionId::new("condition-1").unwrap());
        let constraint = DeclarativeConstraint::new(id.clone(), expression.clone());
        assert_eq!(constraint.id(), &id);
        assert_eq!(constraint.expression(), &expression);

        let criterion_id = AcceptanceCriterionId::new("criterion-1").unwrap();
        let criterion = AcceptanceCriterion::new(
            criterion_id.clone(),
            "The condition is satisfied",
            expression.clone(),
        )
        .unwrap();
        assert_eq!(criterion.id(), &criterion_id);
        assert_eq!(criterion.description(), "The condition is satisfied");
        assert_eq!(criterion.expression(), &expression);
        assert!(AcceptanceCriterion::new(criterion_id, " ", expression.clone()).is_err());

        let inline = OriginalInput::inline("user supplied intent").unwrap();
        assert!(matches!(inline, OriginalInput::Inline(_)));
        assert!(OriginalInput::inline(" ").is_err());
        let reference_id = ReferenceId::new("request-1").unwrap();
        let reference = OriginalInput::reference(reference_id.clone());
        assert!(matches!(reference, OriginalInput::Reference(id) if id == reference_id));
    }

    #[test]
    fn desired_state_rejects_invalid_collections_and_exposes_all_members() {
        let known = condition("known", "project.status", TypedValue::Boolean(true));
        let known_id = known.id().clone();
        let expression = ConditionExpression::condition(known_id);
        let base_id = DesiredStateId::new("desired-full").unwrap();
        assert!(matches!(
            DesiredState::new(base_id.clone(), vec![], expression.clone(), vec![], vec![],),
            Err(ValidationError::EmptyRelationship {
                field: "desired_conditions"
            })
        ));

        let constraint_id = ConstraintId::new("duplicate-constraint").unwrap();
        let duplicate_constraints = DesiredState::new(
            base_id.clone(),
            vec![known.clone()],
            expression.clone(),
            vec![
                DeclarativeConstraint::new(constraint_id.clone(), expression.clone()),
                DeclarativeConstraint::new(constraint_id, expression.clone()),
            ],
            vec![],
        );
        assert!(matches!(
            duplicate_constraints,
            Err(ValidationError::DuplicateDeclarativeIdentity {
                kind: "constraint",
                ..
            })
        ));

        let criterion_id = AcceptanceCriterionId::new("duplicate-criterion").unwrap();
        let criterion = |id| AcceptanceCriterion::new(id, "criterion", expression.clone()).unwrap();
        let duplicate_criteria = DesiredState::new(
            base_id.clone(),
            vec![known.clone()],
            expression.clone(),
            vec![],
            vec![criterion(criterion_id.clone()), criterion(criterion_id)],
        );
        assert!(matches!(
            duplicate_criteria,
            Err(ValidationError::DuplicateDeclarativeIdentity {
                kind: "acceptance_criterion",
                ..
            })
        ));

        let missing_expression =
            ConditionExpression::condition(ConditionId::new("missing").unwrap());
        assert!(matches!(
            DesiredState::new(
                base_id.clone(),
                vec![known.clone()],
                missing_expression.clone(),
                vec![],
                vec![],
            ),
            Err(ValidationError::MissingDeclarativeIdentity {
                kind: "condition",
                ..
            })
        ));
        assert!(matches!(
            DesiredState::new(
                base_id.clone(),
                vec![known.clone()],
                expression.clone(),
                vec![DeclarativeConstraint::new(
                    ConstraintId::new("bad-constraint").unwrap(),
                    missing_expression.clone(),
                )],
                vec![],
            ),
            Err(ValidationError::MissingDeclarativeIdentity {
                kind: "condition",
                ..
            })
        ));

        let criterion_with_missing = AcceptanceCriterion::new(
            AcceptanceCriterionId::new("bad-criterion").unwrap(),
            "criterion",
            missing_expression,
        )
        .unwrap();
        assert!(matches!(
            DesiredState::new(
                base_id.clone(),
                vec![known.clone()],
                expression.clone(),
                vec![],
                vec![criterion_with_missing],
            ),
            Err(ValidationError::MissingDeclarativeIdentity {
                kind: "condition",
                ..
            })
        ));

        let unsupported = DeclarativeContextVersion::new(2, 0).unwrap();
        assert!(matches!(
            DesiredState::new_with_version(
                unsupported,
                base_id.clone(),
                vec![known.clone()],
                expression.clone(),
                vec![],
                vec![],
            ),
            Err(ValidationError::UnsupportedSchemaVersion { .. })
        ));

        let state = DesiredState::new(
            base_id.clone(),
            vec![known],
            expression.clone(),
            vec![DeclarativeConstraint::new(
                ConstraintId::new("constraint-1").unwrap(),
                expression.clone(),
            )],
            vec![criterion(
                AcceptanceCriterionId::new("criterion-1").unwrap(),
            )],
        )
        .unwrap();
        assert_eq!(state.version(), DeclarativeContextVersion::V1);
        assert_eq!(state.id(), &base_id);
        assert_eq!(state.expression(), &expression);
        assert_eq!(state.constraints().len(), 1);
        assert_eq!(state.acceptance_criteria().len(), 1);
    }

    #[test]
    fn intent_preserves_desired_state_and_optional_input() {
        let desired = DesiredState::new(
            DesiredStateId::new("desired-intent").unwrap(),
            vec![condition(
                "ready",
                "project.status",
                TypedValue::Boolean(true),
            )],
            ConditionExpression::condition(ConditionId::new("ready").unwrap()),
            vec![],
            vec![],
        )
        .unwrap();
        let intent_id = IntentId::new("intent-1").unwrap();
        let intent = Intent::new(intent_id.clone(), desired.clone());
        assert_eq!(intent.id(), &intent_id);
        assert_eq!(intent.desired_state(), &desired);
        assert!(intent.original_input().is_none());
        let reference_id = ReferenceId::new("input-1").unwrap();
        let intent = intent.with_original_input(OriginalInput::reference(reference_id.clone()));
        assert_eq!(intent.version(), DeclarativeContextVersion::V1);
        assert!(matches!(
            intent.original_input(),
            Some(OriginalInput::Reference(id)) if id == &reference_id
        ));
    }
}
