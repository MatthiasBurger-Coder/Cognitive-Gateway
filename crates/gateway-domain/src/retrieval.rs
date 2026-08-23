use crate::{NonEmptyText, ValidationError};

/// Provenance attached to knowledge returned by a retrieval adapter.
///
/// Retrieval is advisory, so provenance describes where the material came
/// from without turning that source into an authority or permission.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KnowledgeProvenance {
    source: NonEmptyText,
    revision: Option<NonEmptyText>,
}

impl KnowledgeProvenance {
    /// Creates provenance for one retrieved knowledge item.
    pub fn new(
        source: impl Into<String>,
        revision: Option<impl Into<String>>,
    ) -> Result<Self, ValidationError> {
        let revision = revision
            .map(Into::into)
            .map(NonEmptyText::new)
            .transpose()?;

        Ok(Self {
            source: NonEmptyText::new(source)?,
            revision,
        })
    }

    /// Returns the source reference supplied by the retrieval adapter.
    #[must_use]
    pub fn source(&self) -> &str {
        self.source.as_str()
    }

    /// Returns the optional source revision, commit or snapshot identity.
    #[must_use]
    pub fn revision(&self) -> Option<&str> {
        self.revision.as_ref().map(NonEmptyText::as_str)
    }
}

/// Knowledge returned by a retrieval adapter.
///
/// The value deliberately contains only material and provenance. It does not
/// contain capabilities, policy decisions, Agent/Skill membership or other
/// authority-bearing fields.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RetrievedKnowledge {
    content: NonEmptyText,
    provenance: KnowledgeProvenance,
}

impl RetrievedKnowledge {
    /// Creates one validated retrieval result.
    pub fn new(
        content: impl Into<String>,
        provenance: KnowledgeProvenance,
    ) -> Result<Self, ValidationError> {
        Ok(Self {
            content: NonEmptyText::new(content)?,
            provenance,
        })
    }

    /// Returns the retrieved material.
    #[must_use]
    pub fn content(&self) -> &str {
        self.content.as_str()
    }

    /// Returns the material's provenance.
    #[must_use]
    pub const fn provenance(&self) -> &KnowledgeProvenance {
        &self.provenance
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeQuery(NonEmptyText);

impl KnowledgeQuery {
    pub fn new(query: impl Into<String>) -> Result<Self, ValidationError> {
        Ok(Self(NonEmptyText::new_for_field(query, "query")?))
    }

    /// Fallible constructor with an explicit name for use at parsing boundaries.
    pub fn try_new(query: impl Into<String>) -> Result<Self, ValidationError> {
        Self::new(query)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::{KnowledgeProvenance, KnowledgeQuery, RetrievedKnowledge};
    use crate::ValidationError;

    #[test]
    fn preserves_retrieval_content_and_provenance() {
        let provenance = KnowledgeProvenance::new("git://project", Some("abc123")).unwrap();
        let result = RetrievedKnowledge::new("retrieved fact", provenance).unwrap();

        assert_eq!(result.content(), "retrieved fact");
        assert_eq!(result.provenance().source(), "git://project");
        assert_eq!(result.provenance().revision(), Some("abc123"));
    }

    #[test]
    fn rejects_empty_retrieval_content_and_provenance() {
        assert!(KnowledgeProvenance::new("  ", None::<&str>).is_err());
        assert!(KnowledgeProvenance::new("source", Some("\t")).is_err());

        let provenance = KnowledgeProvenance::new("source", None::<&str>).unwrap();
        assert!(RetrievedKnowledge::new("\n\t", provenance).is_err());
    }

    #[test]
    fn creates_a_query_without_normalizing_it() {
        let query = KnowledgeQuery::new("  architecture decision  ").unwrap();

        assert_eq!(query.as_str(), "  architecture decision  ");
    }

    #[test]
    fn rejects_empty_queries() {
        let result = KnowledgeQuery::try_new("\t\n");

        assert!(matches!(result, Err(ValidationError::EmptyText { .. })));
    }
}
