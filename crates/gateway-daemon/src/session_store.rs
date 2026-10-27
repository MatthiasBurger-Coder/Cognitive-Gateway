//! #276 PostgreSQL journal: scoped row locks, immutable records and global owner ledger.
use crate::cognitive_store::{CognitiveStore, StoreError};
use gateway_application::sessions::*;
use gateway_domain::{CapabilityId, ConditionId, ContextScopeId, Intent, PlanStepId, SourceId};
use serde::{Deserialize, Serialize};
#[path = "session_store_observation_audit.rs"]
mod observation_audit;
use observation_audit::ObservationPolicyAudit;

impl From<StoreError> for SessionError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::CommitUnknown => Self::OutcomeUnknown,
            StoreError::Limit => Self::LimitExceeded,
            _ => Self::StorageUnavailable,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Grant {
    reference: RecordRef,
    binding: ActionBinding,
    issuer: PrincipalId,
    status: GrantState,
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum GrantState {
    Granted,
    Denied,
    Withdrawn,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    owner: OwnerBinding,
    intent: Intent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    basis: Option<ArtifactGoalBasis>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    connector_goal: Option<ConnectorGoalWire>,
    execution: RequestedExecution,
    snapshot: SessionSnapshot,
    budget: SessionBudget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resource_ledger: Option<SessionResourceLedger>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pre_execution_goal: Option<PreExecutionGoalRef>,
    pending: Option<PendingWire>,
    accepted_consent: Option<Grant>,
    authority_events: Vec<EventWire>,
    selected_source: Option<RecordRef>,
    artifact: Option<RecordRef>,
    lease_until_ms: Option<u64>,
    initial_assessment: Option<InitialAssessment>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConnectorGoalWire {
    scope: ContextScopeId,
    plan: RecordRef,
    steps: Vec<String>,
    observations: Vec<ConditionObservationWire>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConditionObservationWire {
    condition: ConditionId,
    source: SourceId,
    capability: CapabilityId,
    operation_class: ReadOperationWire,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ReadOperationWire {
    Read,
    Search,
    Inspect,
}

impl From<ReadOperationWire> for gateway_application::connectors::ConnectorOperationClass {
    fn from(value: ReadOperationWire) -> Self {
        match value {
            ReadOperationWire::Read => Self::Read,
            ReadOperationWire::Search => Self::Search,
            ReadOperationWire::Inspect => Self::Inspect,
        }
    }
}
impl From<gateway_application::connectors::ConnectorOperationClass> for ReadOperationWire {
    fn from(value: gateway_application::connectors::ConnectorOperationClass) -> Self {
        match value {
            gateway_application::connectors::ConnectorOperationClass::Read => Self::Read,
            gateway_application::connectors::ConnectorOperationClass::Search => Self::Search,
            gateway_application::connectors::ConnectorOperationClass::Inspect => Self::Inspect,
            gateway_application::connectors::ConnectorOperationClass::Mutate
            | gateway_application::connectors::ConnectorOperationClass::Admin => {
                unreachable!("validated connector goals only contain read operations")
            }
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "payload",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum PendingWire {
    Source(SourceQuestion),
    Consent(Box<ActionBinding>),
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum EventWire {
    Denied {
        pending: PendingId,
        revision: Revision,
    },
    Withdrawn {
        record: RecordRef,
        revision: Revision,
    },
}
impl Checkpoint {
    fn from_typed(c: &SessionCheckpoint) -> Self {
        let (basis, connector_goal) = match c.goal.as_artifact() {
            Some(goal) => (Some(goal.basis().clone()), None),
            None => {
                let goal = c
                    .goal
                    .as_connector()
                    .expect("session goal has one supported variant");
                (
                    None,
                    Some(ConnectorGoalWire {
                        scope: goal.scope().clone(),
                        plan: goal.plan().clone(),
                        steps: goal
                            .steps()
                            .iter()
                            .map(|step| step.as_str().to_owned())
                            .collect(),
                        observations: goal
                            .observations()
                            .iter()
                            .map(|mapping| ConditionObservationWire {
                                condition: mapping.condition().clone(),
                                source: mapping.source().clone(),
                                capability: mapping.capability().clone(),
                                operation_class: mapping.operation_class().into(),
                            })
                            .collect(),
                    }),
                )
            }
        };
        Self {
            owner: c.owner.clone(),
            intent: c.goal.intent().clone(),
            basis,
            connector_goal,
            execution: c.execution,
            snapshot: c.snapshot.clone(),
            budget: c.budget.clone(),
            resource_ledger: c.resource_ledger.clone(),
            pre_execution_goal: c.pre_execution_goal.clone(),
            pending: c.pending.as_ref().map(|p| match p {
                PendingInteraction::Source(q) => PendingWire::Source(q.clone()),
                PendingInteraction::Consent(a) => PendingWire::Consent(a.clone()),
            }),
            accepted_consent: c.accepted_consent.as_ref().map(|g| Grant {
                reference: g.record().0.clone(),
                binding: g.binding().clone(),
                issuer: g.issuer().clone(),
                status: GrantState::Granted,
            }),
            authority_events: c
                .authority_events
                .iter()
                .map(|e| match e {
                    AuthorityEvent::Denied { pending, revision } => EventWire::Denied {
                        pending: pending.clone(),
                        revision: *revision,
                    },
                    AuthorityEvent::Withdrawn { record, revision } => EventWire::Withdrawn {
                        record: record.0.clone(),
                        revision: *revision,
                    },
                })
                .collect(),
            selected_source: c.selected_source.clone(),
            artifact: c.artifact.clone(),
            lease_until_ms: c.lease_until_ms,
            initial_assessment: c.initial_assessment.clone(),
        }
    }
    fn typed(&self) -> Result<SessionCheckpoint, SessionError> {
        let goal = match (&self.basis, &self.connector_goal) {
            (Some(basis), None) => SupportedSessionGoal::artifact(SupportedArtifactGoal::validate(
                self.intent.clone(),
                basis.clone(),
            )?),
            (None, Some(goal)) => {
                SupportedSessionGoal::connector(SupportedConnectorGoal::from_host_registration(
                    self.intent.clone(),
                    goal.scope.clone(),
                    goal.plan.clone(),
                    goal.steps
                        .iter()
                        .map(|step| {
                            PlanStepId::new(step.clone())
                                .map_err(|_| SessionError::StorageUnavailable)
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    goal.observations
                        .iter()
                        .map(|mapping| {
                            ConditionObservationMapping::new(
                                mapping.condition.clone(),
                                mapping.source.clone(),
                                mapping.capability.clone(),
                                mapping.operation_class.into(),
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                )?)
            }
            _ => return Err(SessionError::StorageUnavailable),
        };
        let result = SessionCheckpoint {
            owner: self.owner.clone(),
            goal,
            execution: self.execution,
            snapshot: self.snapshot.clone(),
            budget: self.budget.clone(),
            resource_ledger: self.resource_ledger.clone(),
            pre_execution_goal: self.pre_execution_goal.clone(),
            pending: self.pending.as_ref().map(|p| match p {
                PendingWire::Source(q) => PendingInteraction::Source(q.clone()),
                PendingWire::Consent(a) => PendingInteraction::Consent(a.clone()),
            }),
            accepted_consent: self.accepted_consent.as_ref().map(|g| {
                VerifiedConsent::from_trusted_record(
                    ConsentRecordRef(g.reference.clone()),
                    g.binding.clone(),
                    g.issuer.clone(),
                )
            }),
            authority_events: self
                .authority_events
                .iter()
                .map(|e| match e {
                    EventWire::Denied { pending, revision } => AuthorityEvent::Denied {
                        pending: pending.clone(),
                        revision: *revision,
                    },
                    EventWire::Withdrawn { record, revision } => AuthorityEvent::Withdrawn {
                        record: ConsentRecordRef(record.clone()),
                        revision: *revision,
                    },
                })
                .collect(),
            selected_source: self.selected_source.clone(),
            artifact: self.artifact.clone(),
            lease_until_ms: self.lease_until_ms,
            initial_assessment: self.initial_assessment.clone(),
        };
        result.validate()?;
        if result.selected_source.as_ref().is_some_and(|r| {
            result
                .goal
                .as_artifact()
                .is_none_or(|goal| !goal.basis().sources.contains(r))
        }) {
            return Err(SessionError::StorageUnavailable);
        }
        Ok(result)
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredSession {
    checkpoint: Checkpoint,
    fence: u64,
}
impl StoredSession {
    fn typed(&self) -> Result<FencedSession, SessionError> {
        Ok(FencedSession {
            checkpoint: self.checkpoint.typed()?,
            fence: FenceToken::new(self.fence)?,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    key: CommandKey,
    outcome: CommandOutcome,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    owner: OwnerBinding,
    reference: RecordRef,
    kind: RecordKind,
    bytes: Vec<u8>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema_version: u32,
    sessions: Vec<StoredSession>,
    commands: Vec<Ledger>,
    records: Vec<Record>,
    grants: Vec<Grant>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    observation_policy_audits: Vec<ObservationPolicyAudit>,
}
impl Journal {
    fn initial() -> Self {
        Self {
            schema_version: 2,
            sessions: vec![],
            commands: vec![],
            records: vec![],
            grants: vec![],
            observation_policy_audits: vec![],
        }
    }
    fn validate(&self, scope: &ContextScopeId) -> Result<(), SessionError> {
        if self.schema_version != 2
            || self.sessions.len() > 64
            || self.commands.len() > 4096
            || self.records.len() > 4096
            || self.grants.len() > 4096
        {
            return Err(SessionError::StorageUnavailable);
        }
        for (i, s) in self.sessions.iter().enumerate() {
            let typed = s.typed()?;
            if s.checkpoint.initial_assessment.is_none() {
                return Err(SessionError::StorageUnavailable);
            }
            if typed.checkpoint.goal.scope() != scope {
                return Err(SessionError::StorageUnavailable);
            }
            if s.checkpoint
                .snapshot
                .command_outcome
                .as_ref()
                .is_none_or(|outcome| {
                    !self.commands.iter().any(|entry| {
                        entry.key.owner == s.checkpoint.owner && entry.outcome == *outcome
                    })
                })
            {
                return Err(SessionError::StorageUnavailable);
            }
            if self.sessions[..i]
                .iter()
                .any(|p| p.checkpoint.snapshot.session == s.checkpoint.snapshot.session)
            {
                return Err(SessionError::StorageUnavailable);
            }
            if let Some(marker) = &s.checkpoint.pre_execution_goal
                && !self.records.iter().any(|record| {
                    record.owner == s.checkpoint.owner
                        && record.reference == marker.record
                        && record.kind == RecordKind::PreExecutionGoal
                })
            {
                return Err(SessionError::StorageUnavailable);
            }
            for (reference, kind) in [
                (&s.checkpoint.artifact, RecordKind::Artifact),
                (&s.checkpoint.snapshot.final_evidence, RecordKind::Evidence),
            ] {
                let Some(r) = reference else {
                    continue;
                };
                if !self.records.iter().any(|record| {
                    record.owner == s.checkpoint.owner
                        && record.reference == *r
                        && record.kind == kind
                }) {
                    return Err(SessionError::StorageUnavailable);
                }
            }
        }
        for (i, c) in self.commands.iter().enumerate() {
            if self.commands[..i].iter().any(|p| p.key == c.key)
                || c.key.command != c.outcome.command
                || !self.sessions.iter().any(|s| {
                    s.checkpoint.owner == c.key.owner
                        && s.checkpoint.snapshot.session == c.outcome.session
                        && c.outcome.revision.value() > 0
                        && c.outcome.revision <= s.checkpoint.snapshot.revision
                })
            {
                return Err(SessionError::StorageUnavailable);
            }
        }
        for (i, r) in self.records.iter().enumerate() {
            if r.bytes.len() > 1_048_576
                || content_reference(r.reference.id().as_str(), &r.bytes)? != r.reference
                || self.records[..i].iter().any(|p| {
                    p.owner == r.owner
                        && p.reference.id() == r.reference.id()
                        && p.reference.revision() == r.reference.revision()
                })
            {
                return Err(SessionError::StorageUnavailable);
            }
        }
        observation_audit::validate(self, scope)?;
        for (i, grant) in self.grants.iter().enumerate() {
            let bytes =
                serde_json::to_vec(&("cg.consent-record/2.0", &grant.binding, &grant.issuer))
                    .map_err(|_| SessionError::StorageUnavailable)?;
            if content_reference(&format!("grant-{}", grant.binding.pending.as_str()), &bytes)?
                != grant.reference
                || grant.binding.issued_revision.value() == 0
                || grant.binding.expires_at_ms == 0
                || grant.binding.expires_at_ms > MAX_REVISION
                || self.grants[..i].iter().any(|prior| {
                    prior.reference == grant.reference && prior.binding.owner == grant.binding.owner
                })
                || !self.sessions.iter().any(|session| {
                    session.checkpoint.owner == grant.binding.owner
                        && session.checkpoint.snapshot.session == grant.binding.session
                        && session.checkpoint.snapshot.run == grant.binding.run
                        && session.checkpoint.snapshot.revision >= grant.binding.issued_revision
                })
            {
                return Err(SessionError::StorageUnavailable);
            }
        }
        Ok(())
    }
    fn index(&self, owner: &OwnerBinding, id: &SessionId) -> Result<usize, SessionError> {
        self.sessions
            .iter()
            .position(|s| s.checkpoint.snapshot.session == *id && s.checkpoint.owner == *owner)
            .ok_or(SessionError::ScopeDenied)
    }
}
#[derive(Clone)]
pub struct PostgresSessionStore {
    store: CognitiveStore,
}
impl PostgresSessionStore {
    pub fn new(store: CognitiveStore) -> Self {
        Self { store }
    }
    fn run<R>(
        &self,
        operation: impl FnOnce(&mut Journal) -> Result<R, SessionError>,
    ) -> Result<R, SessionError> {
        self.store
            .transact("task-sessions-v2", &Journal::initial(), |journal| {
                journal.validate(self.store.scope())?;
                let result = operation(journal)?;
                journal.validate(self.store.scope())?;
                Ok(result)
            })
    }
    pub fn consent(
        &self,
        owner: &OwnerBinding,
        reference: &ConsentRecordRef,
    ) -> Result<ConsentStatus, SessionError> {
        self.run(|j| {
            let g = j
                .grants
                .iter()
                .find(|g| g.binding.owner == *owner && g.reference == reference.0)
                .ok_or(SessionError::Unavailable)?;
            Ok(match g.status {
                GrantState::Granted => {
                    ConsentStatus::Granted(Box::new(VerifiedConsent::from_trusted_record(
                        reference.clone(),
                        g.binding.clone(),
                        g.issuer.clone(),
                    )))
                }
                GrantState::Denied => ConsentStatus::Denied,
                GrantState::Withdrawn => ConsentStatus::Withdrawn,
            })
        })
    }
    /// The caller authenticates an allowed issuer through the trusted host. This
    /// operation is intentionally absent from the MCP command surface.
    pub fn issue(
        &self,
        owner: &OwnerBinding,
        session: &SessionId,
        issuer: &PrincipalId,
        decision: &str,
        now: u64,
    ) -> Result<Option<RecordRef>, SessionError> {
        self.run(|j| {
            let i = j.index(owner, session)?;
            let current = j.sessions[i].typed()?;
            if current.checkpoint.snapshot.state.terminal() {
                if decision != "withdraw" {
                    return Err(SessionError::InvalidState);
                }
                let grant = j
                    .grants
                    .iter_mut()
                    .filter(|grant| {
                        grant.binding.owner == *owner
                            && grant.binding.session == *session
                            && grant.binding.run == current.checkpoint.snapshot.run
                            && grant.status == GrantState::Granted
                    })
                    .max_by_key(|grant| grant.binding.issued_revision)
                    .ok_or(SessionError::Unavailable)?;
                // Authority audit remains live while the task's terminal snapshot,
                // revision and immutable verification receipt remain historical.
                grant.status = GrantState::Withdrawn;
                return Ok(None);
            }
            let binding = match (
                &current.checkpoint.pending,
                &current.checkpoint.accepted_consent,
            ) {
                (Some(PendingInteraction::Consent(a)), _) => a.as_ref().clone(),
                (_, Some(g)) if decision == "withdraw" => g.binding().clone(),
                _ => return Err(SessionError::InvalidInteraction),
            };
            if now >= binding.expires_at_ms && decision != "withdraw" {
                return Err(SessionError::ExpiredInteraction);
            }
            let status = match decision {
                "approve" => GrantState::Granted,
                "deny" => GrantState::Denied,
                "withdraw" => GrantState::Withdrawn,
                _ => return Err(SessionError::InvalidInput),
            };
            let mut withdrawn_reference = None;
            if decision == "withdraw" {
                let g = j
                    .grants
                    .iter_mut()
                    .find(|g| g.binding == binding && g.status == GrantState::Granted)
                    .ok_or(SessionError::Unavailable)?;
                g.status = status;
                withdrawn_reference = Some(ConsentRecordRef(g.reference.clone()));
            } else {
                if j.grants.iter().any(|g| g.binding == binding) {
                    return Err(SessionError::Duplicate);
                }
                if j.grants.len() == 4096 {
                    return Err(SessionError::LimitExceeded);
                }
                let bytes = serde_json::to_vec(&("cg.consent-record/2.0", &binding, issuer))
                    .map_err(|_| SessionError::InvalidInput)?;
                let reference =
                    content_reference(&format!("grant-{}", binding.pending.as_str()), &bytes)?;
                j.grants.push(Grant {
                    reference: reference.clone(),
                    binding: binding.clone(),
                    issuer: issuer.clone(),
                    status,
                });
                if status == GrantState::Granted {
                    return Ok(Some(reference));
                }
            }
            let mut next = current.checkpoint.clone();
            next.snapshot.revision = next.snapshot.revision.next()?;
            if status == GrantState::Denied {
                next.authority_events.push(AuthorityEvent::Denied {
                    pending: binding.pending,
                    revision: next.snapshot.revision,
                });
            } else {
                next.authority_events.push(AuthorityEvent::Withdrawn {
                    record: withdrawn_reference.ok_or(SessionError::InvalidInteraction)?,
                    revision: next.snapshot.revision,
                });
            }
            // Pure compilation has no external effect; replacing the fence stops
            // all later result commits from an outstanding compiler.
            next.snapshot.state = SessionState::Failed;
            next.snapshot.pending = None;
            next.pending = None;
            next.lease_until_ms = None;
            next.snapshot.dispatch_knowledge = DispatchKnowledge::Stopped;
            next.validate()?;
            j.sessions[i] = StoredSession {
                checkpoint: Checkpoint::from_typed(&next),
                fence: current
                    .fence
                    .value()
                    .checked_add(1)
                    .ok_or(SessionError::LimitExceeded)?,
            };
            Ok(None)
        })
    }
}
impl SessionJournalPort for PostgresSessionStore {
    fn load(
        &self,
        owner: &OwnerBinding,
        session: &SessionId,
    ) -> Result<FencedSession, SessionError> {
        self.run(|j| j.sessions[j.index(owner, session)?].typed())
    }
    fn command_outcome(&self, key: &CommandKey) -> Result<CommandOutcome, SessionError> {
        self.run(|j| {
            j.commands
                .iter()
                .find(|c| c.key == *key)
                .map(|c| c.outcome.clone())
                .ok_or(SessionError::Unavailable)
        })
    }
    fn create(
        &self,
        command: &CommandId,
        initial: SessionCheckpoint,
    ) -> Result<FencedSession, SessionError> {
        initial.validate()?;
        if initial.initial_assessment.is_none() {
            return Err(SessionError::InvalidState);
        }
        if initial.goal.scope() != self.store.scope() {
            return Err(SessionError::ScopeDenied);
        }
        if initial.snapshot.revision.value() != 1
            || initial.budget.actions != 0
            || initial.budget.retries != 0
            || initial.snapshot.command_outcome.as_ref().is_none_or(|o| {
                o.command != *command
                    || o.revision.value() != 1
                    || o.session != initial.snapshot.session
            })
        {
            return Err(SessionError::InvalidState);
        }
        self.run(|j| {
            let key = CommandKey {
                owner: initial.owner.clone(),
                command: command.clone(),
            };
            if j.commands.iter().any(|c| c.key == key) {
                return Err(SessionError::Duplicate);
            }
            if j.sessions
                .iter()
                .any(|s| s.checkpoint.snapshot.session == initial.snapshot.session)
            {
                return Err(SessionError::Duplicate);
            }
            if j.sessions.len() == 64 || j.commands.len() == 4096 {
                return Err(SessionError::LimitExceeded);
            }
            let outcome = initial
                .snapshot
                .command_outcome
                .clone()
                .ok_or(SessionError::InvalidState)?;
            j.commands.push(Ledger { key, outcome });
            j.sessions.push(StoredSession {
                checkpoint: Checkpoint::from_typed(&initial),
                fence: 1,
            });
            Ok(FencedSession {
                checkpoint: initial,
                fence: FenceToken::new(1)?,
            })
        })
    }
    fn append(
        &self,
        owner: &OwnerBinding,
        transition: JournalAppend,
    ) -> Result<FencedSession, SessionError> {
        self.run(|j| {
            let i = j.index(owner, &transition.session)?;
            let current = j.sessions[i].typed()?;
            if let Some(command) = &transition.command
                && j.commands
                    .iter()
                    .any(|c| c.key.owner == *owner && c.key.command == *command)
            {
                return Err(SessionError::Duplicate);
            }
            transition.validate_against(&current)?;
            if transition.records.len() > 2
                || j.records.len() + transition.records.len() > 4096
                || (transition.command.is_some() && j.commands.len() == 4096)
            {
                return Err(SessionError::LimitExceeded);
            }
            for record in transition.records {
                if content_reference(record.reference.id().as_str(), &record.bytes)?
                    != record.reference
                    || record.bytes.len() > 1_048_576
                {
                    return Err(SessionError::InvalidInput);
                }
                let target = match record.kind {
                    RecordKind::Artifact => transition.next.artifact.as_ref(),
                    RecordKind::Evidence => transition.next.snapshot.final_evidence.as_ref(),
                    RecordKind::PreExecutionGoal => transition
                        .next
                        .pre_execution_goal
                        .as_ref()
                        .map(|marker| &marker.record),
                };
                if target != Some(&record.reference) {
                    return Err(SessionError::InvalidState);
                }
                if j.records.iter().any(|r| {
                    r.owner == *owner
                        && r.reference.id() == record.reference.id()
                        && r.reference.revision() == record.reference.revision()
                }) {
                    return Err(SessionError::Duplicate);
                }
                j.records.push(Record {
                    owner: owner.clone(),
                    reference: record.reference,
                    kind: record.kind,
                    bytes: record.bytes,
                });
            }
            if let Some(command) = transition.command {
                j.commands.push(Ledger {
                    key: CommandKey {
                        owner: owner.clone(),
                        command,
                    },
                    outcome: transition
                        .next
                        .snapshot
                        .command_outcome
                        .clone()
                        .ok_or(SessionError::InvalidState)?,
                });
            }
            let fence = current
                .fence
                .value()
                .checked_add(1)
                .ok_or(SessionError::LimitExceeded)?;
            j.sessions[i] = StoredSession {
                checkpoint: Checkpoint::from_typed(&transition.next),
                fence,
            };
            Ok(FencedSession {
                checkpoint: transition.next,
                fence: FenceToken::new(fence)?,
            })
        })
    }
}
impl ArtifactStoragePort for PostgresSessionStore {
    fn load_artifact(
        &self,
        owner: &OwnerBinding,
        reference: &RecordRef,
    ) -> Result<Vec<u8>, SessionError> {
        self.run(|j| {
            j.records
                .iter()
                .find(|r| r.owner == *owner && r.reference == *reference)
                .map(|r| r.bytes.clone())
                .ok_or(SessionError::Unavailable)
        })
    }
}
impl SessionRepositoryPort for PostgresSessionStore {
    fn record_kind(
        &self,
        owner: &OwnerBinding,
        reference: &RecordRef,
    ) -> Result<RecordKind, SessionError> {
        self.run(|j| {
            j.records
                .iter()
                .find(|r| r.owner == *owner && r.reference == *reference)
                .map(|r| r.kind)
                .ok_or(SessionError::Unavailable)
        })
    }
}

#[cfg(test)]
#[path = "session_store_tests.rs"]
mod tests;
