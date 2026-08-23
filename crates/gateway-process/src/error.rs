use std::{error::Error, fmt};

/// Stable categories used by process construction and canonicalization errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ValidationCode {
    InvalidIdentifier,
    InvalidVersion,
    InvalidDigest,
    UnsupportedIrVersion,
    DuplicateIdentifier,
    MissingInitialState,
    MultipleInitialStates,
    EmptyDefinition,
    InvalidDefinition,
    InvalidReference,
    NonCanonicalDefinition,
}

/// Error returned when a process contract cannot be constructed or verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessError {
    code: ValidationCode,
    message: String,
}

impl ProcessError {
    pub(crate) fn new(code: ValidationCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn code(&self) -> ValidationCode {
        self.code
    }
}

impl fmt::Display for ProcessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code.as_str(), self.message)
    }
}

impl Error for ProcessError {}

impl ValidationCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidIdentifier => "INVALID_IDENTIFIER",
            Self::InvalidVersion => "INVALID_VERSION",
            Self::InvalidDigest => "INVALID_DIGEST",
            Self::UnsupportedIrVersion => "UNSUPPORTED_IR_VERSION",
            Self::DuplicateIdentifier => "DUPLICATE_IDENTIFIER",
            Self::MissingInitialState => "MISSING_INITIAL_STATE",
            Self::MultipleInitialStates => "MULTIPLE_INITIAL_STATES",
            Self::EmptyDefinition => "EMPTY_DEFINITION",
            Self::InvalidDefinition => "INVALID_DEFINITION",
            Self::InvalidReference => "INVALID_REFERENCE",
            Self::NonCanonicalDefinition => "NON_CANONICAL_DEFINITION",
        }
    }
}

impl From<gateway_domain::ValidationError> for ProcessError {
    fn from(error: gateway_domain::ValidationError) -> Self {
        Self::new(ValidationCode::InvalidIdentifier, error.to_string())
    }
}
