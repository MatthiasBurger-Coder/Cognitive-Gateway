//! Governed promotion commands. Authority and durable history are trusted adapter ports;
//! models and workers receive inspection data, never these ports or the service.
use gateway_domain::{ProvenanceId, ReferenceId, ValidationError, procedure_promotion::*};
use gateway_registry::learned_procedures::LearnedProcedureRegistry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromotionRole {
    Governor,
    Runtime,
    Model,
    Worker,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedPromotion {
    pub actor: ProvenanceId,
    pub policy_decision: ReferenceId,
    pub role: PromotionRole,
}
/// Authenticate the caller out of band and authorize the exact command.
/// Returning None denies it. Request payloads never supply actor, role or policy authority.
pub trait PromotionAuthority {
    fn authorize(&self, command: &PromotionCommand) -> Option<AuthorizedPromotion>;
}
/// The adapter must atomically compare the revision and append, or leave history unchanged.
/// Its journal must be authenticated trusted storage, not model-supplied JSON.
pub trait PromotionStore {
    fn load(&self) -> Result<PromotionJournal, PromotionError>;
    fn append(
        &mut self,
        expected_revision: usize,
        event: PromotionEvent,
    ) -> Result<(), PromotionError>;
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromotionError {
    Unauthorized,
    Invalid(ValidationError),
    Conflict,
    Store(String),
}
impl From<ValidationError> for PromotionError {
    fn from(error: ValidationError) -> Self {
        Self::Invalid(error)
    }
}

pub struct PromotionApplication<A, S> {
    authority: A,
    store: S,
}
impl<A: PromotionAuthority, S: PromotionStore> PromotionApplication<A, S> {
    pub fn new(authority: A, store: S) -> Self {
        Self { authority, store }
    }
    pub fn inspect(&self) -> Result<(PromotionJournal, LearnedProcedureRegistry), PromotionError> {
        let journal = self.store.load()?;
        let registry = LearnedProcedureRegistry::from_journal(&journal)?;
        Ok((journal, registry))
    }
    /// Validate a complete speculative projection before one atomic append.
    pub fn execute(
        &mut self,
        id: ReferenceId,
        at: i64,
        command: PromotionCommand,
    ) -> Result<LearnedProcedureRegistry, PromotionError> {
        let authorized = self
            .authority
            .authorize(&command)
            .ok_or(PromotionError::Unauthorized)?;
        if authorized.role != PromotionRole::Governor
            && !(authorized.role == PromotionRole::Runtime && command.is_runtime())
        {
            return Err(PromotionError::Unauthorized);
        }
        let mut journal = self.store.load()?;
        let revision = journal.events.len();
        let event = PromotionEvent {
            metadata: DecisionMetadata {
                id,
                at,
                actor: authorized.actor,
                policy_decision: authorized.policy_decision,
            },
            command,
        };
        journal.events.push(event.clone());
        let registry = LearnedProcedureRegistry::from_journal(&journal)?;
        self.store.append(revision, event)?;
        Ok(registry)
    }
}

/// Local single-process store; adapters can implement durable transactional storage.
#[derive(Debug, Default)]
pub struct InMemoryPromotionStore {
    journal: PromotionJournal,
}
impl PromotionStore for InMemoryPromotionStore {
    fn load(&self) -> Result<PromotionJournal, PromotionError> {
        Ok(self.journal.clone())
    }
    fn append(
        &mut self,
        expected_revision: usize,
        event: PromotionEvent,
    ) -> Result<(), PromotionError> {
        if self.journal.events.len() != expected_revision {
            return Err(PromotionError::Conflict);
        }
        let mut next = self.journal.clone();
        next.events.push(event);
        LearnedProcedureRegistry::from_journal(&next)?;
        self.journal = next;
        Ok(())
    }
}
