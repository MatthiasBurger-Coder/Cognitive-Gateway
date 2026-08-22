use crate::{NonEmptyText, ValidationError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeQuery(NonEmptyText);

impl KnowledgeQuery {
    pub fn new(query: impl Into<String>) -> Result<Self, ValidationError> {
        Ok(Self(NonEmptyText::new(query)?))
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
    use super::KnowledgeQuery;
    use crate::ValidationError;

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
