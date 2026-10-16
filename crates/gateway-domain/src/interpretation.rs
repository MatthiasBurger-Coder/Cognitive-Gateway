//! Request-scoped interpretation inputs. These records confer no execution authority.
use crate::*;
use serde::{Deserialize, Serialize};

impl Serialize for SubjectPath {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}
impl<'de> Deserialize<'de> for SubjectPath {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}
impl Serialize for NonEmptyText {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}
impl<'de> Deserialize<'de> for NonEmptyText {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

pub type InterpretationPin = ResolvedReference<ReferenceId>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InterpretationTier {
    CurrentInput,
    TaskState,
    Situation,
    ConversationFocus,
    VerifiedMemory,
    RetrievedEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "SCREAMING_SNAKE_CASE",
    deny_unknown_fields
)]
pub enum InterpretationValue {
    Target(SemanticTarget),
    Reference(InterpretationPin),
    Literal(TypedValue),
    Instant(UnixTimestamp),
    Interval {
        start: UnixTimestamp,
        end: UnixTimestamp,
    },
}

/// The host authenticates this complete binding before any contextual lookup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterpretationBinding {
    pub scope: ContextScopeId,
    pub request: ReferenceId,
    pub input: InterpretationPin,
    pub conversation: Option<InterpretationPin>,
    pub task: Option<ResolvedReference<TaskId>>,
    pub host_basis: InterpretationPin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterpretationFact {
    pub id: ReferenceId,
    pub field: SubjectPath,
    pub tier: InterpretationTier,
    pub value: InterpretationValue,
    pub source: InterpretationPin,
    pub provenance: Provenance,
    pub quality: QualityMetadata,
    pub valid_until: UnixTimestamp,
    pub relevance: NonEmptyText,
    pub span: Option<(usize, usize)>,
    pub subject: Option<SemanticTarget>,
    /// True for immutable task declarations and established current truth.
    pub protected: bool,
    /// Explicitly required revisions cannot be discarded as stale implicit context.
    pub required_pin: bool,
    /// Authenticated correction relationship, never array chronology.
    pub supersedes: Option<ReferenceId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterpretationSource {
    pub source: InterpretationPin,
    pub tier: InterpretationTier,
    pub sensitivity_ceiling: SensitivityClass,
    pub trust: TrustClass,
    pub max_results: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterpretationAlias {
    pub label: NonEmptyText,
    pub kind: TargetKind,
    pub fact: InterpretationFact,
}

/// Calendar boundaries are pinned host projections, including timezone/DST rules.
/// Missing civil boundaries never imply UTC or the machine's local timezone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemporalAnchor {
    pub name: ReferenceId,
    pub fact: InterpretationFact,
    pub timezone: Option<NonEmptyText>,
    pub calendar: Option<NonEmptyText>,
    pub yesterday: Option<(UnixTimestamp, UnixTimestamp)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterpretationContextData {
    pub version: SchemaVersion,
    pub id: ReferenceId,
    pub revision: ReferenceId,
    pub binding: InterpretationBinding,
    pub captured_at: UnixTimestamp,
    pub expires_at: UnixTimestamp,
    pub basis_refs: Vec<InterpretationPin>,
    pub permitted_sources: Vec<InterpretationSource>,
    pub relevant_facts: Vec<InterpretationFact>,
    pub aliases: Vec<InterpretationAlias>,
    pub temporal_anchors: Vec<TemporalAnchor>,
}
