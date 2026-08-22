//! Shared validation primitives for the provider-independent domain model.

use std::{error::Error, fmt};

/// The maximum length of a domain identifier, in Unicode scalar values.
pub const MAX_IDENTIFIER_LENGTH: usize = 128;

/// The maximum length of required textual domain values, in Unicode scalar values.
pub const MAX_TEXT_LENGTH: usize = 16_384;

/// The reason a domain value was rejected during validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    /// A required textual value was empty or contained only whitespace.
    EmptyText { field: &'static str },
    /// A required textual value exceeded [`MAX_TEXT_LENGTH`].
    TextTooLong {
        field: &'static str,
        max_length: usize,
    },
    /// A required textual value contained a disallowed control character.
    ControlCharacter { field: &'static str },
    /// An identifier was empty or contained only whitespace.
    EmptyIdentifier,
    /// An identifier exceeded [`MAX_IDENTIFIER_LENGTH`].
    IdentifierTooLong { max_length: usize },
    /// An identifier contained a character outside the identifier alphabet.
    InvalidIdentifierCharacter { character: char },
    /// An identifier did not begin or end with an ASCII alphanumeric character.
    InvalidIdentifierBoundary,
    /// A schema version used the reserved zero major version.
    InvalidSchemaVersion,
    /// A schema version string was not in the supported `MAJOR.MINOR` form.
    InvalidSchemaVersionFormat,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyText { field } => write!(formatter, "{field} must not be empty"),
            Self::TextTooLong { field, max_length } => {
                write!(formatter, "{field} must not exceed {max_length} characters")
            }
            Self::ControlCharacter { field } => {
                write!(formatter, "{field} contains a disallowed control character")
            }
            Self::EmptyIdentifier => write!(formatter, "identifier must not be empty"),
            Self::IdentifierTooLong { max_length } => {
                write!(
                    formatter,
                    "identifier must not exceed {max_length} characters"
                )
            }
            Self::InvalidIdentifierCharacter { character } => {
                write!(
                    formatter,
                    "identifier contains invalid character {character:?}"
                )
            }
            Self::InvalidIdentifierBoundary => write!(
                formatter,
                "identifier must begin and end with an ASCII alphanumeric character"
            ),
            Self::InvalidSchemaVersion => {
                write!(
                    formatter,
                    "schema version major component must be greater than zero"
                )
            }
            Self::InvalidSchemaVersionFormat => {
                write!(formatter, "schema version must use MAJOR.MINOR format")
            }
        }
    }
}

impl Error for ValidationError {}

/// A required human- or machine-readable text value.
///
/// Values must contain at least one non-whitespace character, may not exceed
/// [`MAX_TEXT_LENGTH`], and may not contain control characters other than
/// horizontal tab, line feed, or carriage return.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct NonEmptyText(String);

impl NonEmptyText {
    /// Creates a validated text value without changing its contents.
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();

        if value.trim().is_empty() {
            return Err(ValidationError::EmptyText { field: "text" });
        }
        if value.chars().count() > MAX_TEXT_LENGTH {
            return Err(ValidationError::TextTooLong {
                field: "text",
                max_length: MAX_TEXT_LENGTH,
            });
        }
        if value
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\t' | '\n' | '\r'))
        {
            return Err(ValidationError::ControlCharacter { field: "text" });
        }

        Ok(Self(value))
    }

    /// Returns the validated text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the owned text.
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl AsRef<str> for NonEmptyText {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl From<NonEmptyText> for String {
    fn from(value: NonEmptyText) -> Self {
        value.into_inner()
    }
}

impl TryFrom<String> for NonEmptyText {
    type Error = ValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<&str> for NonEmptyText {
    type Error = ValidationError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

pub(crate) fn validate_identifier(value: impl Into<String>) -> Result<String, ValidationError> {
    let value = value.into();

    if value.trim().is_empty() {
        return Err(ValidationError::EmptyIdentifier);
    }
    if value.chars().count() > MAX_IDENTIFIER_LENGTH {
        return Err(ValidationError::IdentifierTooLong {
            max_length: MAX_IDENTIFIER_LENGTH,
        });
    }

    let mut characters = value.chars();
    let first = characters.next().ok_or(ValidationError::EmptyIdentifier)?;
    let last = value
        .chars()
        .next_back()
        .ok_or(ValidationError::EmptyIdentifier)?;
    if !first.is_ascii_alphanumeric() || !last.is_ascii_alphanumeric() {
        return Err(ValidationError::InvalidIdentifierBoundary);
    }
    for character in value.chars() {
        if !(character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')) {
            return Err(ValidationError::InvalidIdentifierCharacter { character });
        }
    }

    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_IDENTIFIER_LENGTH, MAX_TEXT_LENGTH, NonEmptyText, ValidationError, validate_identifier,
    };

    #[test]
    fn accepts_multiline_required_text() {
        let text = NonEmptyText::new("intent:\n\tinspect the repository").unwrap();

        assert_eq!(text.as_str(), "intent:\n\tinspect the repository");
    }

    #[test]
    fn rejects_empty_and_control_text() {
        assert!(matches!(
            NonEmptyText::new("  \n\t"),
            Err(ValidationError::EmptyText { .. })
        ));
        assert!(matches!(
            NonEmptyText::new("valid\0text"),
            Err(ValidationError::ControlCharacter { .. })
        ));
    }

    #[test]
    fn enforces_text_boundary() {
        assert!(NonEmptyText::new("x".repeat(MAX_TEXT_LENGTH)).is_ok());
        assert!(matches!(
            NonEmptyText::new("x".repeat(MAX_TEXT_LENGTH + 1)),
            Err(ValidationError::TextTooLong { .. })
        ));
    }

    #[test]
    fn validates_identifier_alphabet_and_boundaries() {
        assert!(validate_identifier("task-01.example".to_owned()).is_ok());
        assert!(matches!(
            validate_identifier("task name".to_owned()),
            Err(ValidationError::InvalidIdentifierCharacter { character: ' ' })
        ));
        assert!(matches!(
            validate_identifier("_task".to_owned()),
            Err(ValidationError::InvalidIdentifierBoundary)
        ));
        assert!(matches!(
            validate_identifier("x".repeat(MAX_IDENTIFIER_LENGTH + 1)),
            Err(ValidationError::IdentifierTooLong { .. })
        ));
    }
}
