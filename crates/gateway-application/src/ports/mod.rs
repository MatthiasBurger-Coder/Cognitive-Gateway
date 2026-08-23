pub mod inbound;
pub mod outbound;

#[cfg(test)]
mod tests {
    use gateway_domain::{
        ExecutionContext, KnowledgeProvenance, KnowledgeQuery, RetrievedKnowledge,
    };

    use crate::{
        context::{ExecutionRequest, ProjectConfiguration, ProjectContext},
        ports::outbound::{KnowledgePort, KnowledgeRequest},
    };

    struct FixtureKnowledge;

    impl KnowledgePort for FixtureKnowledge {
        type Error = ();

        fn retrieve(
            &self,
            request: &KnowledgeRequest<'_>,
        ) -> Result<Vec<RetrievedKnowledge>, Self::Error> {
            Ok(vec![
                RetrievedKnowledge::new(
                    request.query().as_str(),
                    KnowledgeProvenance::new("fixture", Some("revision-1")).unwrap(),
                )
                .unwrap(),
            ])
        }
    }

    #[test]
    fn keeps_project_scope_explicit_and_retrieval_results_provenanced() {
        let query = KnowledgeQuery::new("repository conventions").unwrap();
        let configuration =
            ProjectConfiguration::new("application/json", "{\"scope\":\"repo\"}").unwrap();
        let project_context = ProjectContext::with_configuration(configuration);
        let request = KnowledgeRequest::new(&query).with_project_context(&project_context);
        let result = RetrievedKnowledge::new(
            "repository evidence",
            KnowledgeProvenance::new("repository", Some("revision-1")).unwrap(),
        )
        .unwrap();

        assert_eq!(request.query().as_str(), "repository conventions");
        assert!(request.project_context().is_some());
        assert_eq!(result.provenance().source(), "repository");

        let retrieved = FixtureKnowledge.retrieve(&request).unwrap();
        assert_eq!(retrieved[0].content(), "repository conventions");
        assert_eq!(retrieved[0].provenance().revision(), Some("revision-1"));
    }

    #[test]
    fn execution_requests_carry_project_context_only_at_the_application_boundary() {
        let task = gateway_domain::TaskDescriptor::new(
            gateway_domain::TaskId::new("task-1").unwrap(),
            "inspect",
        )
        .unwrap();
        let context = ProjectContext::empty();
        let request = ExecutionRequest::new(&task, &context);

        assert_eq!(request.task().id().as_str(), "task-1");
        assert!(request.project_context().configuration().is_none());

        fn accepts_domain_output(_request: &ExecutionRequest<'_>) -> Result<ExecutionContext, ()> {
            Err(())
        }

        assert!(accepts_domain_output(&request).is_err());
    }
}
