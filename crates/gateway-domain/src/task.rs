use crate::{NonEmptyText, TaskId, validation::ValidationError};

/// A normalized semantic task classification with an explicit confidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskConfidence(u16);

impl TaskConfidence {
    /// Creates a confidence from a finite fraction in the inclusive 0..=1 range.
    pub fn new(value: f64) -> Result<Self, ValidationError> {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(ValidationError::InvalidConfidence {
                field: "confidence",
            });
        }
        Ok(Self((value * 10_000.0).round() as u16))
    }

    /// Returns this confidence as a fraction in the inclusive 0..=1 range.
    #[must_use]
    pub fn as_fraction(self) -> f64 {
        f64::from(self.0) / 10_000.0
    }

    /// Alias for consumers that use numeric-value terminology.
    #[must_use]
    pub fn as_f64(self) -> f64 {
        self.as_fraction()
    }
}

/// Optional semantic classification attached to a task descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskClassification {
    task_type: NonEmptyText,
    confidence: TaskConfidence,
}

impl TaskClassification {
    /// Creates a classification without normalizing its type text.
    pub fn new(task_type: impl Into<String>, confidence: f64) -> Result<Self, ValidationError> {
        Ok(Self {
            task_type: NonEmptyText::new_for_field(task_type, "task_type")?,
            confidence: TaskConfidence::new(confidence)?,
        })
    }

    /// Returns the semantic task type.
    #[must_use]
    pub fn task_type(&self) -> &str {
        self.task_type.as_str()
    }

    /// Returns the semantic confidence.
    #[must_use]
    pub const fn confidence(&self) -> TaskConfidence {
        self.confidence
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDescriptor {
    id: TaskId,
    intent: NonEmptyText,
    classification: Option<TaskClassification>,
}

impl TaskDescriptor {
    /// Creates a task descriptor with a stable identity supplied by the caller.
    pub fn new(id: TaskId, intent: impl Into<String>) -> Result<Self, ValidationError> {
        Ok(Self {
            id,
            intent: NonEmptyText::new_for_field(intent, "intent")?,
            classification: None,
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

    /// Adds optional semantic task type and confidence as one atomic value.
    pub fn with_classification(
        mut self,
        task_type: impl Into<String>,
        confidence: f64,
    ) -> Result<Self, ValidationError> {
        self.classification = Some(TaskClassification::new(task_type, confidence)?);
        Ok(self)
    }

    /// Returns the optional semantic classification. It is either fully
    /// present or absent; type and confidence are never independently null.
    #[must_use]
    pub fn classification(&self) -> Option<&TaskClassification> {
        self.classification.as_ref()
    }

    /// Returns the optional semantic task type.
    #[must_use]
    pub fn task_type(&self) -> Option<&str> {
        self.classification
            .as_ref()
            .map(TaskClassification::task_type)
    }

    /// Returns the optional semantic confidence.
    #[must_use]
    pub fn confidence(&self) -> Option<TaskConfidence> {
        self.classification
            .as_ref()
            .map(TaskClassification::confidence)
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
        assert!(descriptor.classification().is_none());
    }

    #[test]
    fn rejects_invalid_intents() {
        let result = TaskDescriptor::try_new(TaskId::new("task-1").unwrap(), " \n\t");

        assert!(matches!(result, Err(ValidationError::EmptyText { .. })));
    }

    #[test]
    fn keeps_task_classification_atomic_and_validated() {
        let descriptor = TaskDescriptor::new(TaskId::new("task-1").unwrap(), "Repair")
            .unwrap()
            .with_classification("runtime_bugfix", 0.94)
            .unwrap();

        assert_eq!(descriptor.task_type(), Some("runtime_bugfix"));
        assert_eq!(descriptor.confidence().unwrap().as_fraction(), 0.94);
        assert_eq!(descriptor.confidence().unwrap().as_f64(), 0.94);
        assert_eq!(
            descriptor.classification().unwrap().task_type(),
            "runtime_bugfix"
        );
    }

    #[test]
    fn rejects_invalid_classification_confidence_and_type() {
        let task = TaskDescriptor::new(TaskId::new("task-1").unwrap(), "Repair").unwrap();
        assert!(matches!(
            task.clone().with_classification("runtime_bugfix", 1.1),
            Err(ValidationError::InvalidConfidence {
                field: "confidence"
            })
        ));
        assert!(matches!(
            task.with_classification(" ", 0.5),
            Err(ValidationError::EmptyText { field: "task_type" })
        ));
    }
}
