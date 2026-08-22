use crate::{NonEmptyText, TaskId, validation::ValidationError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDescriptor {
    id: TaskId,
    intent: NonEmptyText,
}

impl TaskDescriptor {
    /// Creates a task descriptor with a stable identity supplied by the caller.
    pub fn new(id: TaskId, intent: impl Into<String>) -> Result<Self, ValidationError> {
        Ok(Self {
            id,
            intent: NonEmptyText::new(intent)?,
        })
    }

    /// Fallible constructor with an explicit name for use at parsing boundaries.
    pub fn try_new(id: TaskId, intent: impl Into<String>) -> Result<Self, ValidationError> {
        Self::new(id, intent)
    }

    /// Returns the task identity.
    #[must_use]
    pub fn id(&self) -> &TaskId {
        &self.id
    }

    /// Returns the validated task intent.
    #[must_use]
    pub fn intent(&self) -> &str {
        self.intent.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::TaskDescriptor;
    use crate::{TaskId, ValidationError};

    #[test]
    fn creates_a_descriptor_with_validated_values() {
        let descriptor =
            TaskDescriptor::new(TaskId::new("task-1").unwrap(), "Inspect the repository").unwrap();

        assert_eq!(descriptor.id().as_str(), "task-1");
        assert_eq!(descriptor.intent(), "Inspect the repository");
    }

    #[test]
    fn rejects_invalid_intents() {
        let result = TaskDescriptor::try_new(TaskId::new("task-1").unwrap(), " \n\t");

        assert!(matches!(result, Err(ValidationError::EmptyText { .. })));
    }
}
