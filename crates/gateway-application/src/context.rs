use gateway_domain::{NonEmptyText, ValidationError};

/// Opaque, request-scoped configuration supplied by a consuming project.
///
/// The application boundary carries the bytes and their declared media type;
/// it does not interpret them as Agent, Skill, policy or capability
/// definitions. Registry membership therefore remains owned by the
/// Gateway catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectConfiguration {
    media_type: NonEmptyText,
    content: NonEmptyText,
}

impl ProjectConfiguration {
    /// Creates a validated opaque project configuration snapshot.
    pub fn new(
        media_type: impl Into<String>,
        content: impl Into<String>,
    ) -> Result<Self, ValidationError> {
        Ok(Self {
            media_type: NonEmptyText::new(media_type)?,
            content: NonEmptyText::new(content)?,
        })
    }

    /// Returns the configuration media type without interpreting its content.
    #[must_use]
    pub fn media_type(&self) -> &str {
        self.media_type.as_str()
    }

    /// Returns the opaque configuration content.
    #[must_use]
    pub fn content(&self) -> &str {
        self.content.as_str()
    }
}

/// Request-scoped context from a consuming project.
///
/// This type is intentionally owned by the application boundary. It is not a
/// registry input and cannot add, replace or override catalog definitions.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectContext {
    configuration: Option<ProjectConfiguration>,
}

impl ProjectContext {
    /// Creates a project context without project configuration.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            configuration: None,
        }
    }

    /// Creates a project context with one opaque configuration snapshot.
    #[must_use]
    pub const fn with_configuration(configuration: ProjectConfiguration) -> Self {
        Self {
            configuration: Some(configuration),
        }
    }

    /// Returns the optional project configuration snapshot.
    #[must_use]
    pub const fn configuration(&self) -> Option<&ProjectConfiguration> {
        self.configuration.as_ref()
    }
}

/// A stable, explicit request boundary for project-scoped configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionRequest<'a> {
    task: &'a gateway_domain::TaskDescriptor,
    project_context: &'a ProjectContext,
}

impl<'a> ExecutionRequest<'a> {
    /// Creates an execution request from a task and external project context.
    #[must_use]
    pub const fn new(
        task: &'a gateway_domain::TaskDescriptor,
        project_context: &'a ProjectContext,
    ) -> Self {
        Self {
            task,
            project_context,
        }
    }

    /// Returns the task submitted by the driving adapter.
    #[must_use]
    pub const fn task(&self) -> &gateway_domain::TaskDescriptor {
        self.task
    }

    /// Returns external project context without converting it into authority.
    #[must_use]
    pub const fn project_context(&self) -> &ProjectContext {
        self.project_context
    }
}

#[cfg(test)]
mod tests {
    use gateway_domain::{TaskDescriptor, TaskId};

    use super::{ExecutionRequest, ProjectConfiguration, ProjectContext};

    fn task() -> TaskDescriptor {
        TaskDescriptor::new(TaskId::new("task-1").unwrap(), "inspect").unwrap()
    }

    #[test]
    fn keeps_project_configuration_outside_the_domain_request_values() {
        let configuration =
            ProjectConfiguration::new("application/json", "{\"mode\":\"test\"}").unwrap();
        let project_context = ProjectContext::with_configuration(configuration);
        let task = task();
        let request = ExecutionRequest::new(&task, &project_context);

        assert_eq!(request.task().id().as_str(), "task-1");
        assert_eq!(
            request
                .project_context()
                .configuration()
                .unwrap()
                .media_type(),
            "application/json"
        );
    }

    #[test]
    fn rejects_empty_opaque_configuration_fields() {
        assert!(ProjectConfiguration::new("", "content").is_err());
        assert!(ProjectConfiguration::new("application/json", "").is_err());
    }
}
