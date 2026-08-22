//! Strongly typed identifiers used by domain aggregates and references.

use std::{fmt, str::FromStr};

use crate::validation::{ValidationError, validate_identifier};

macro_rules! typed_identifier {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
        pub struct $name(String);

        impl $name {
            /// Creates an identifier after applying the shared identifier rules.
            pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
                Ok(Self(validate_identifier(value)?))
            }

            /// Returns the validated identifier text.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Returns the owned identifier text.
            #[must_use]
            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = ValidationError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }

        impl TryFrom<String> for $name {
            type Error = ValidationError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = ValidationError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }
    };
}

typed_identifier! {
    /// Identifies a task aggregate.
    TaskId
}

typed_identifier! {
    /// Identifies an agent definition.
    AgentId
}

typed_identifier! {
    /// Identifies a skill definition.
    SkillId
}

typed_identifier! {
    /// Identifies a workflow definition.
    WorkflowId
}

typed_identifier! {
    /// Identifies a policy definition.
    PolicyId
}

typed_identifier! {
    /// Identifies an execution context.
    ExecutionContextId
}

typed_identifier! {
    /// Identifies a capability exposed through a domain capability port.
    CapabilityId
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{AgentId, ExecutionContextId, TaskId};

    #[test]
    fn typed_ids_are_not_interchangeable() {
        let task = TaskId::new("build-check").unwrap();
        let agent = AgentId::new("build-check").unwrap();

        assert_eq!(task.as_str(), agent.as_str());
        assert_ne!(task, TaskId::new("other-task").unwrap());
        assert_eq!(agent.to_string(), "build-check");
    }

    #[test]
    fn ids_support_safe_parsing() {
        assert_eq!(
            ExecutionContextId::from_str("context-1").unwrap().as_str(),
            "context-1"
        );
        assert!(TaskId::from_str("../context").is_err());
        assert!(TaskId::from_str("context/").is_err());
        assert!(TaskId::from_str("").is_err());
    }
}
