#![forbid(unsafe_code)]

use gateway_domain::capability::CapabilityId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow,
    Deny,
    RequireConsent,
}

pub trait PolicyEvaluator {
    fn evaluate(&self, capability: &CapabilityId) -> PolicyDecision;
}
