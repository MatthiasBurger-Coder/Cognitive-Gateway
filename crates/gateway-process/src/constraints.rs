//! Typed gate, evidence, invariant and blocker evaluation values.

use crate::EvidenceTypeId;
use serde::Serialize;

/// Explicit evidence evaluation status. Missing evidence never passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum EvidenceStatus {
    Missing,
    Present,
    Invalid,
    Failed,
}

impl EvidenceStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "MISSING",
            Self::Present => "PRESENT",
            Self::Invalid => "INVALID",
            Self::Failed => "FAILED",
        }
    }
}

/// A typed evidence reference with provenance supplied by an external input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceReference {
    evidence_type: EvidenceTypeId,
    status: EvidenceStatus,
    provenance: String,
}

impl EvidenceReference {
    pub fn new(
        evidence_type: EvidenceTypeId,
        status: EvidenceStatus,
        provenance: impl Into<String>,
    ) -> Result<Self, crate::MutationError> {
        let provenance = provenance.into();
        if provenance.trim().is_empty() {
            return Err(crate::MutationError::new(
                "INVALID_EVIDENCE",
                "evidence provenance cannot be empty",
            ));
        }
        Ok(Self {
            evidence_type,
            status,
            provenance,
        })
    }
    #[must_use]
    pub fn evidence_type(&self) -> &EvidenceTypeId {
        &self.evidence_type
    }
    #[must_use]
    pub const fn status(&self) -> EvidenceStatus {
        self.status
    }
    #[must_use]
    pub fn provenance(&self) -> &str {
        &self.provenance
    }
}

/// One machine-readable constraint result in a transition trace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConstraintEvaluation {
    kind: String,
    reference: String,
    status: String,
    detail: String,
}

impl ConstraintEvaluation {
    pub(crate) fn new(
        kind: &str,
        reference: &str,
        status: &str,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            kind: kind.to_owned(),
            reference: reference.to_owned(),
            status: status.to_owned(),
            detail: detail.into(),
        }
    }
    #[must_use]
    pub fn kind(&self) -> &str {
        &self.kind
    }
    #[must_use]
    pub fn reference(&self) -> &str {
        &self.reference
    }
    #[must_use]
    pub fn status(&self) -> &str {
        &self.status
    }
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}
