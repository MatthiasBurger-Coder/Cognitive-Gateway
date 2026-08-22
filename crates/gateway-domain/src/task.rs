#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDescriptor {
    pub intent: String,
}

impl TaskDescriptor {
    #[must_use]
    pub fn new(intent: impl Into<String>) -> Self {
        Self {
            intent: intent.into(),
        }
    }
}
