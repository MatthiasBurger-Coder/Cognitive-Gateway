use gateway_domain::{capability::CapabilityId, retrieval::KnowledgeQuery};

pub trait KnowledgePort {
    type Error;

    fn retrieve(&self, query: &KnowledgeQuery) -> Result<Vec<String>, Self::Error>;
}

pub trait CapabilityPort {
    type Error;

    fn is_available(&self, capability: &CapabilityId) -> Result<bool, Self::Error>;
}

pub trait ExecutionRuntimePort {
    type Error;

    fn runtime_id(&self) -> Result<String, Self::Error>;
}

pub trait EvidencePort {
    type Error;

    fn record(&self, event: &str) -> Result<(), Self::Error>;
}
