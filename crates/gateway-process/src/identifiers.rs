use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use crate::error::{ProcessError, ValidationCode};

fn validate(value: String, kind: &'static str) -> Result<String, ProcessError> {
    if value.is_empty()
        || value.len() > 128
        || value.starts_with('.')
        || value.ends_with('.')
        || value.contains("..")
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_')
        })
    {
        return Err(ProcessError::new(
            ValidationCode::InvalidIdentifier,
            format!("{kind} must be a non-empty dotted identifier: {value:?}"),
        ));
    }
    Ok(value)
}

macro_rules! identifier {
    ($name:ident) => {
        #[doc = "Validated strongly typed process identifier."]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, ProcessError> {
                Ok(Self(validate(value.into(), stringify!($name))?))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

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
            type Err = ProcessError;
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
            }
        }
    };
}

identifier!(ProcessDefinitionId);
identifier!(ProcessInstanceId);
identifier!(StateId);
identifier!(EventTypeId);
identifier!(EventOccurrenceId);
identifier!(CorrelationId);
identifier!(CausationId);
identifier!(TransitionId);
identifier!(GateId);
identifier!(EvidenceTypeId);
identifier!(ActivityId);
identifier!(BlockerId);

/// Monotonic version of a process definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct ProcessDefinitionVersion(u32);

impl ProcessDefinitionVersion {
    pub fn new(value: u32) -> Result<Self, ProcessError> {
        if value == 0 {
            Err(ProcessError::new(
                ValidationCode::InvalidVersion,
                "version must be positive",
            ))
        } else {
            Ok(Self(value))
        }
    }

    #[must_use]
    pub const fn value(self) -> u32 {
        self.0
    }
}

impl TryFrom<u32> for ProcessDefinitionVersion {
    type Error = ProcessError;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ProcessDefinitionVersion> for u32 {
    fn from(value: ProcessDefinitionVersion) -> Self {
        value.0
    }
}

impl fmt::Display for ProcessDefinitionVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// SHA-256 digest of the canonical Process IR representation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct ProcessDefinitionDigest(String);

impl ProcessDefinitionDigest {
    pub fn new(value: impl Into<String>) -> Result<Self, ProcessError> {
        let value = value.into();
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(ProcessError::new(
                ValidationCode::InvalidDigest,
                "definition digest must be exactly 64 hexadecimal characters",
            ));
        }
        Ok(Self(value.to_ascii_lowercase()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProcessDefinitionDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for ProcessDefinitionDigest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ProcessDefinitionDigest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// Monotonic revision of a process instance.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize, Default,
)]
pub struct ProcessInstanceRevision(u64);

impl ProcessInstanceRevision {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn initial() -> Self {
        Self(0)
    }

    pub fn next(self) -> Result<Self, ProcessError> {
        self.0.checked_add(1).map(Self).ok_or_else(|| {
            ProcessError::new(
                ValidationCode::InvalidDefinition,
                "instance revision overflow",
            )
        })
    }

    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}
