use super::*;
use crate::{ContentDigest, ContextScopeId, ReferenceId};
use std::{collections::BTreeSet, num::NonZeroU64};

/// Full identity of a vector space. A change in any field invalidates compatibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingModel {
    pub id: EmbeddingModelId,
    pub version: EmbeddingModelVersion,
    pub digest: Option<ContentDigest>,
    pub dimensions: NonZeroU64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbeddingModelRequirement {
    Exact(EmbeddingModel),
    /// Explicitly permits adapter selection; result must still name its full identity.
    AdapterSelected {
        dimensions: NonZeroU64,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingRequest {
    pub scope: ContextScopeId,
    pub fragments: Vec<RetrievedFragment>,
    pub model: EmbeddingModelRequirement,
}
impl EmbeddingRequest {
    pub fn validate(&self) -> Result<(), RetrievalError> {
        if self.fragments.is_empty() {
            return Err(RetrievalError::InvalidResult);
        }
        let mut ids = BTreeSet::new();
        for fragment in &self.fragments {
            if fragment.scope != self.scope {
                return Err(RetrievalError::ScopeMismatch);
            }
            if !ids.insert(&fragment.id) {
                return Err(RetrievalError::DuplicateIdentity);
            }
        }
        Ok(())
    }
}
/// No SDK vector type. Values must be finite and match the declared dimension.
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingVector {
    pub fragment: ReferenceId,
    pub values: Vec<f32>,
}

/// Derived index partition identity; scope and complete model identity must match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingIndexMetadata {
    pub scope: ContextScopeId,
    pub model: EmbeddingModel,
}
impl EmbeddingIndexMetadata {
    pub fn ensure_compatible(&self, other: &Self) -> Result<(), RetrievalError> {
        if self.scope != other.scope {
            return Err(RetrievalError::ScopeMismatch);
        }
        if self.model != other.model {
            return Err(RetrievalError::IncompatibleEmbedding);
        }
        Ok(())
    }
}
/// Immutable derived vectors retain the full source snapshot/provenance lineage.
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingResult {
    metadata: EmbeddingIndexMetadata,
    sources: Vec<RetrievedFragment>,
    vectors: Vec<EmbeddingVector>,
}
impl EmbeddingResult {
    pub fn new(
        request: &EmbeddingRequest,
        model: EmbeddingModel,
        mut vectors: Vec<EmbeddingVector>,
    ) -> Result<Self, RetrievalError> {
        request.validate()?;
        let compatible = match &request.model {
            EmbeddingModelRequirement::Exact(expected) => expected == &model,
            EmbeddingModelRequirement::AdapterSelected { dimensions } => {
                dimensions == &model.dimensions
            }
        };
        if !compatible || vectors.len() != request.fragments.len() {
            return Err(RetrievalError::IncompatibleEmbedding);
        }
        let mut ids = BTreeSet::new();
        for vector in &vectors {
            if !ids.insert(&vector.fragment)
                || !request.fragments.iter().any(|f| f.id == vector.fragment)
                || vector.values.len() as u64 != model.dimensions.get()
                || vector.values.iter().any(|v| !v.is_finite())
            {
                return Err(RetrievalError::IncompatibleEmbedding);
            }
        }
        let mut sources = request.fragments.clone();
        sources.sort_by(|a, b| a.id.cmp(&b.id));
        vectors.sort_by(|a, b| a.fragment.cmp(&b.fragment));
        Ok(Self {
            metadata: EmbeddingIndexMetadata {
                scope: request.scope.clone(),
                model,
            },
            sources,
            vectors,
        })
    }
    pub fn metadata(&self) -> &EmbeddingIndexMetadata {
        &self.metadata
    }
    pub fn sources(&self) -> &[RetrievedFragment] {
        &self.sources
    }
    pub fn vectors(&self) -> &[EmbeddingVector] {
        &self.vectors
    }
}
