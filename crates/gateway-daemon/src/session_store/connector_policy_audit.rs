//! Durable evidence only: this journal never grants consent or dispatch rights.
use super::*;
use gateway_application::{
    connector_policy::{ConnectorPolicyEvaluation, ConnectorPolicyOutcome, ConnectorPolicyStage},
    connectors::{ConnectorDispatch, ConnectorFailureCode},
    resolution::ResolutionBasis,
};
use gateway_domain::{ContentDigest, ReferenceId, ResolvedReference};
use serde_json::json;

const KIND: &str = "connector-policy-decisions-v1";
const VERSION: u32 = 1;
const MAX_EVENTS: usize = 4096;
const MAX_EVENT_BYTES: usize = 65_536;
const MAX_JOURNAL_BYTES: usize = 16_777_216;
const MAX_PRODUCER_PINS: usize = 128;
const MAX_FINDINGS: usize = 1024;

/// Captured by the concrete immutable Source factory, never reloaded by audit.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorPolicyAuditPins {
    pub canonical_policy: ResolvedReference<ReferenceId>,
    pub runtime: ResolvedReference<ReferenceId>,
    pub manifest: ResolvedReference<ReferenceId>,
    pub mapping: ResolvedReference<ReferenceId>,
    pub producer: Vec<ResolvedReference<ReferenceId>>,
}
impl ConnectorPolicyAuditPins {
    fn validate(&self, scope: &ContextScopeId) -> Result<(), SessionError> {
        if self.producer.is_empty() || self.producer.len() > MAX_PRODUCER_PINS {
            return Err(SessionError::LimitExceeded);
        }
        for pin in [
            &self.canonical_policy,
            &self.runtime,
            &self.manifest,
            &self.mapping,
        ]
        .into_iter()
        .chain(self.producer.iter())
        {
            if &pin.scope != scope {
                return Err(SessionError::ScopeDenied);
            }
        }
        if self
            .producer
            .iter()
            .enumerate()
            .any(|(i, pin)| self.producer[..i].contains(pin))
        {
            return Err(SessionError::InvalidInput);
        }
        Ok(())
    }
}

/// Immutable evidence context captured from an opaque coordinator allocation.
/// It cannot be deserialized into an allocation or used as a permission token.
pub struct ConnectorPolicyAuditContext {
    allocation: AllocationFacts,
    resolution: ResolutionBasis,
    pins: ConnectorPolicyAuditPins,
}
impl ConnectorPolicyAuditContext {
    pub fn from_allocation(
        allocation: &SessionResourceAllocation,
        resolution: ResolutionBasis,
        pins: ConnectorPolicyAuditPins,
    ) -> Result<Self, SessionError> {
        let result = Self {
            allocation: AllocationFacts {
                reservation: allocation.reservation_id().clone(),
                binding: allocation.binding().clone(),
                claim_revision: allocation.claim_revision(),
                fence: allocation.fence().value(),
                phase: allocation.phase(),
                amounts: *allocation.amounts(),
                cost_unit: allocation.cost_unit().clone(),
            },
            resolution,
            pins,
        };
        result.captured()?.validate()?;
        Ok(result)
    }
    pub fn resolution_basis(&self) -> &ResolutionBasis {
        &self.resolution
    }
    pub fn pins(&self) -> &ConnectorPolicyAuditPins {
        &self.pins
    }
    fn captured(&self) -> Result<CapturedContext, SessionError> {
        Ok(CapturedContext {
            allocation: self.allocation.clone(),
            resolution: ResolutionFacts::capture(&self.resolution)?,
            pins: self.pins.clone(),
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AllocationFacts {
    reservation: ContentDigest,
    binding: ResourceReservationBinding,
    claim_revision: Revision,
    fence: u64,
    phase: SessionResourcePhase,
    amounts: SessionResourceAmounts,
    cost_unit: ReferenceId,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResolutionFacts {
    #[serde(with = "plan_id")]
    plan: gateway_domain::PlanId,
    plan_fingerprint: ContentDigest,
    admission: Option<ReferenceId>,
    situation: gateway_domain::SituationId,
    scope: ContextScopeId,
    situation_fingerprint: ContentDigest,
    registry_fingerprint: ContentDigest,
    process_catalog_fingerprint: ContentDigest,
    process_state_fingerprint: ContentDigest,
    rule_version: gateway_domain::SchemaVersion,
}
mod plan_id {
    use gateway_domain::PlanId;
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(id: &PlanId, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(id.as_str())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<PlanId, D::Error> {
        PlanId::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl ResolutionFacts {
    fn capture(basis: &ResolutionBasis) -> Result<Self, SessionError> {
        let fingerprint = |value: &gateway_application::resolution::ContentFingerprint| {
            ContentDigest::new(value.as_str()).map_err(|_| SessionError::InvalidInput)
        };
        Ok(Self {
            plan: basis.plan.clone(),
            plan_fingerprint: fingerprint(&basis.plan_fingerprint)?,
            admission: basis.admission.clone(),
            situation: basis.situation.clone(),
            scope: basis.scope.clone(),
            situation_fingerprint: fingerprint(&basis.situation_fingerprint)?,
            registry_fingerprint: fingerprint(&basis.registry_fingerprint)?,
            process_catalog_fingerprint: fingerprint(&basis.process_catalog_fingerprint)?,
            process_state_fingerprint: fingerprint(&basis.process_state_fingerprint)?,
            rule_version: basis.rule_version,
        })
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CapturedContext {
    allocation: AllocationFacts,
    resolution: ResolutionFacts,
    pins: ConnectorPolicyAuditPins,
}
impl CapturedContext {
    fn validate(&self) -> Result<(), SessionError> {
        self.allocation.binding.validate()?;
        FenceToken::new(self.allocation.fence)?;
        if self.allocation.amounts.is_zero()
            || self.allocation.claim_revision.value() == 0
            || self.allocation.binding.revision.value() == 0
            || self.allocation.claim_revision < self.allocation.binding.revision
            || self.allocation.phase == SessionResourcePhase::GoalVerification
        {
            return Err(SessionError::InvalidInput);
        }
        if self.resolution.scope != self.allocation.binding.scope {
            return Err(SessionError::ScopeDenied);
        }
        self.pins.validate(&self.allocation.binding.scope)
    }
}

fn encoded(value: &impl Serialize) -> Result<Vec<u8>, SessionError> {
    serde_json::to_vec(value).map_err(|_| SessionError::InvalidInput)
}
fn digest(domain: &str, value: &(impl Serialize + ?Sized)) -> Result<ContentDigest, SessionError> {
    Ok(gateway_domain::context_content_digest(&encoded(&(
        domain, value,
    ))?))
}
fn code(value: &impl Serialize) -> Result<String, SessionError> {
    serde_json::to_value(value)
        .map_err(|_| SessionError::InvalidInput)?
        .as_str()
        .map(str::to_owned)
        .ok_or(SessionError::InvalidInput)
}
fn approval(value: gateway_policy::Approval) -> &'static str {
    match value {
        gateway_policy::Approval::Granted => "GRANTED",
        gateway_policy::Approval::Denied => "DENIED",
    }
}
fn process(value: gateway_policy::ProcessReadiness) -> &'static str {
    use gateway_policy::ProcessReadiness::*;
    match value {
        Eligible => "ELIGIBLE",
        Blocked => "BLOCKED",
        Unknown => "UNKNOWN",
        NotApplicable => "NOT_APPLICABLE",
    }
}
fn failure_code(value: ConnectorFailureCode) -> &'static str {
    use ConnectorFailureCode::*;
    match value {
        Unavailable => "UNAVAILABLE",
        AuthenticationFailed => "AUTHENTICATION_FAILED",
        CredentialUnavailable => "CREDENTIAL_UNAVAILABLE",
        CredentialScopeDenied => "CREDENTIAL_SCOPE_DENIED",
        SecretDisclosure => "SECRET_DISCLOSURE",
        PermissionDenied => "PERMISSION_DENIED",
        Timeout => "TIMEOUT",
        Cancelled => "CANCELLED",
        ProtocolError => "PROTOCOL_ERROR",
        IncompatibleVersion => "INCOMPATIBLE_VERSION",
        SchemaMismatch => "SCHEMA_MISMATCH",
        RateLimited => "RATE_LIMITED",
        NotFound => "NOT_FOUND",
        StaleResult => "STALE_RESULT",
        ConflictingResult => "CONFLICTING_RESULT",
        MutationUncertain => "MUTATION_UNCERTAIN",
        RetryUnsafe => "RETRY_UNSAFE",
        ScopeMismatch => "SCOPE_MISMATCH",
        InventoryChanged => "INVENTORY_CHANGED",
        BudgetExhausted => "BUDGET_EXHAUSTED",
        Backpressure => "BACKPRESSURE",
    }
}
const FAILURE_CODES: &[&str] = &[
    "UNAVAILABLE",
    "AUTHENTICATION_FAILED",
    "CREDENTIAL_UNAVAILABLE",
    "CREDENTIAL_SCOPE_DENIED",
    "SECRET_DISCLOSURE",
    "PERMISSION_DENIED",
    "TIMEOUT",
    "CANCELLED",
    "PROTOCOL_ERROR",
    "INCOMPATIBLE_VERSION",
    "SCHEMA_MISMATCH",
    "RATE_LIMITED",
    "NOT_FOUND",
    "STALE_RESULT",
    "CONFLICTING_RESULT",
    "MUTATION_UNCERTAIN",
    "RETRY_UNSAFE",
    "SCOPE_MISMATCH",
    "INVENTORY_CHANGED",
    "BUDGET_EXHAUSTED",
    "BACKPRESSURE",
];
const DECISIONS: &[&str] = &["ALLOW", "DENY", "REQUIRE_EVIDENCE", "REQUIRE_CONSENT"];
const REASONS: &[&str] = &[
    "ALLOWED",
    "EXPLICIT_DENY",
    "NOT_ALLOWLISTED",
    "UNKNOWN_CAPABILITY",
    "CONTRACT_MISMATCH",
    "AUTHORIZATION_MISSING",
    "AUTHORIZATION_DENIED",
    "CONSENT_MISSING",
    "CONSENT_DENIED",
    "EVIDENCE_MISSING",
    "CONSTRAINT_UNSATISFIED",
    "CONSTRAINT_VIOLATION",
    "FEATURE_FROZEN",
    "WORK_CLASS_UNKNOWN",
    "INVALID_EXECUTION_PROFILE",
    "PROCESS_BLOCKED",
    "PROCESS_UNKNOWN",
    "UNRESOLVED",
    "PREREQUISITE_MISSING",
];

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SafeDispatch {
    request: ConnectorRequestIdentity,
    policy: gateway_domain::PolicyId,
    policy_revision: ContentDigest,
    consent_digest: Option<ContentDigest>,
    reservation_digest: ContentDigest,
    deadline: gateway_domain::UnixTimestamp,
}
impl SafeDispatch {
    fn capture(dispatch: &ConnectorDispatch) -> Result<Self, SessionError> {
        Ok(Self {
            request: ConnectorRequestIdentity::from_request(&dispatch.request)?,
            policy: dispatch.policy.clone(),
            policy_revision: dispatch.policy_revision.clone(),
            consent_digest: dispatch
                .consent_receipt
                .as_ref()
                .map(|v| digest("consent/1", v.as_str()))
                .transpose()?,
            reservation_digest: digest("reservation/1", dispatch.budget_reservation.as_str())?,
            deadline: dispatch.deadline,
        })
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "stage",
    content = "dispatch",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum Stage {
    Authorize,
    PreEffect(Box<SafeDispatch>),
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "decision",
    content = "code",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum Outcome {
    Allowed,
    Denied(String),
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Finding {
    decision: String,
    reason: String,
    subject_digest: ContentDigest,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    schema_version: u16,
    step: String,
    full_digest: ContentDigest,
    decision: String,
    findings: Vec<Finding>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    step: String,
    resolved: bool,
    has_prerequisites: bool,
    operating_mode: gateway_domain::OperatingMode,
    execution_profile: gateway_domain::ExecutionProfile,
    process: String,
    consent_required: bool,
    policy: gateway_domain::PolicyId,
    policy_revision: ContentDigest,
    consent_digest: Option<ContentDigest>,
    reservation_digest: ContentDigest,
    deadline: gateway_domain::UnixTimestamp,
    constraints_digest: ContentDigest,
    preconditions_digest: ContentDigest,
    authority_digest: ContentDigest,
    facts_digest: ContentDigest,
    administrative_capabilities_digest: ContentDigest,
}
impl Snapshot {
    fn capture(
        s: &gateway_application::connector_policy::ConnectorPolicySnapshot,
    ) -> Result<Self, SessionError> {
        let a = &s.authority;
        // Only the digests leave this function; descriptions, evidence keys and
        // process constraint tuples are never persisted as audit strings.
        let authority = json!({
            "policies":a.policies.iter().map(|p| json!({"id":p.id(),"description":p.description(),"allow":p.allowed_capability_ids(),"deny":p.denied_capability_ids()})).collect::<Vec<_>>(),
            "capabilities":a.capabilities,
            "constraints":a.constraints.iter().map(|c| (c.id(),c.kind().as_str())).collect::<Vec<_>>(),
            "required_evidence":a.required_evidence,
        });
        let f = &s.facts;
        let facts = json!({
            "authorizations":f.authorizations.iter().map(|(id,a)| (id,approval(*a))).collect::<Vec<_>>(),
            "consents":f.consents.iter().map(|(id,a)| (id,approval(*a))).collect::<Vec<_>>(),
            "evidence":f.evidence,"satisfied_constraints":f.satisfied_constraints,
            "work_class":f.work_class.map(|v| match v {gateway_policy::WorkClass::Feature=>"FEATURE",gateway_policy::WorkClass::Maintenance=>"MAINTENANCE"}),
            "prerequisites_satisfied":f.prerequisites_satisfied,
        });
        Ok(Self {
            step: s.step.as_str().into(),
            resolved: s.resolved,
            has_prerequisites: s.has_prerequisites,
            operating_mode: s.operating_mode,
            execution_profile: s.execution_profile,
            process: process(s.process).into(),
            consent_required: s.consent_required,
            policy: s.policy.clone(),
            policy_revision: s.policy_revision.clone(),
            consent_digest: s
                .consent_receipt
                .as_ref()
                .map(|v| digest("consent/1", v.as_str()))
                .transpose()?,
            reservation_digest: digest("reservation/1", s.budget_reservation.as_str())?,
            deadline: s.deadline,
            constraints_digest: digest("step-constraints/1", &s.constraints)?,
            preconditions_digest: digest("step-preconditions/1", &s.preconditions)?,
            authority_digest: digest("policy-authority/1", &authority)?,
            facts_digest: digest("step-facts/1", &facts)?,
            administrative_capabilities_digest: digest(
                "administrative-capabilities/1",
                &s.administrative_capabilities,
            )?,
        })
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Decision {
    context: CapturedContext,
    request: ConnectorRequestIdentity,
    source: Option<SourceId>,
    snapshot: Snapshot,
    stage: Stage,
    outcome: Outcome,
    report: Option<Report>,
}
impl Decision {
    fn capture(
        context: &ConnectorPolicyAuditContext,
        evaluation: &ConnectorPolicyEvaluation<'_>,
    ) -> Result<Self, SessionError> {
        let report = evaluation
            .report()
            .map(|report| -> Result<Report, SessionError> {
                if report.findings.len() > MAX_FINDINGS {
                    return Err(SessionError::LimitExceeded);
                }
                Ok(Report {
                    schema_version: report.schema_version,
                    step: report.step.as_str().into(),
                    full_digest: digest("policy-report/1", report)?,
                    decision: code(&report.decision)?,
                    findings: report
                        .findings
                        .iter()
                        .map(|f| {
                            Ok(Finding {
                                decision: code(&f.decision)?,
                                reason: code(&f.reason)?,
                                subject_digest: digest("policy-subject/1", &f.subject)?,
                            })
                        })
                        .collect::<Result<_, SessionError>>()?,
                })
            })
            .transpose()?;
        let result = Self {
            context: context.captured()?,
            request: ConnectorRequestIdentity::from_request(evaluation.request())?,
            source: evaluation.source().cloned(),
            snapshot: Snapshot::capture(evaluation.snapshot())?,
            stage: match evaluation.stage() {
                ConnectorPolicyStage::Authorize => Stage::Authorize,
                ConnectorPolicyStage::PreEffect(dispatch) => {
                    Stage::PreEffect(Box::new(SafeDispatch::capture(dispatch)?))
                }
            },
            outcome: match evaluation.outcome() {
                ConnectorPolicyOutcome::Allowed => Outcome::Allowed,
                ConnectorPolicyOutcome::Denied(code) => Outcome::Denied(failure_code(code).into()),
            },
            report,
        };
        result.validate()?;
        Ok(result)
    }
    fn validate(&self) -> Result<(), SessionError> {
        self.context.validate()?;
        self.request.validate()?;
        let a = &self.context.allocation;
        if self.request.scope != a.binding.scope.as_str()
            || self.request.step != a.binding.step.as_str()
        {
            return Err(SessionError::ScopeDenied);
        }
        gateway_domain::PlanStepId::new(&self.snapshot.step)
            .map_err(|_| SessionError::InvalidInput)?;
        // A denied binding mismatch must retain both distinct observed pins.
        // Only an Allowed result claims the exact parent reservation.
        if matches!(self.outcome, Outcome::Allowed)
            && self.snapshot.reservation_digest != digest("reservation/1", a.reservation.as_str())?
        {
            return Err(SessionError::StaleRevision);
        }
        if !["ELIGIBLE", "BLOCKED", "UNKNOWN", "NOT_APPLICABLE"]
            .contains(&self.snapshot.process.as_str())
        {
            return Err(SessionError::InvalidInput);
        }
        if let Stage::PreEffect(dispatch) = &self.stage {
            dispatch.request.validate()?;
            if dispatch.request != self.request {
                return Err(SessionError::InvalidInput);
            }
            if matches!(self.outcome, Outcome::Allowed)
                && (dispatch.policy != self.snapshot.policy
                    || dispatch.policy_revision != self.snapshot.policy_revision
                    || dispatch.consent_digest != self.snapshot.consent_digest
                    || dispatch.reservation_digest != self.snapshot.reservation_digest
                    || dispatch.deadline != self.snapshot.deadline)
            {
                return Err(SessionError::InvalidInput);
            }
        }
        if let Outcome::Denied(code) = &self.outcome
            && !FAILURE_CODES.contains(&code.as_str())
        {
            return Err(SessionError::InvalidInput);
        }
        if let Some(report) = &self.report {
            if report.findings.len() > MAX_FINDINGS {
                return Err(SessionError::LimitExceeded);
            }
            if report.schema_version != 1
                || report.step != self.request.step
                || !DECISIONS.contains(&report.decision.as_str())
                || report.findings.iter().any(|f| {
                    !DECISIONS.contains(&f.decision.as_str())
                        || !REASONS.contains(&f.reason.as_str())
                })
            {
                return Err(SessionError::InvalidInput);
            }
        }
        if matches!(self.outcome, Outcome::Allowed)
            && (self.snapshot.step != self.request.step
                || self.report.as_ref().is_none_or(|r| r.decision != "ALLOW"))
        {
            return Err(SessionError::InvalidInput);
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    ordinal: u64,
    previous: Option<ContentDigest>,
    decision: Decision,
    digest: ContentDigest,
}
impl Entry {
    fn digest(&self) -> Result<ContentDigest, SessionError> {
        digest(
            "cg.connector-policy-audit-entry/1.0",
            &(self.ordinal, &self.previous, &self.decision),
        )
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuditJournal {
    schema_version: u32,
    scope: ContextScopeId,
    entries: Vec<Entry>,
}
impl AuditJournal {
    fn initial(scope: ContextScopeId) -> Self {
        Self {
            schema_version: VERSION,
            scope,
            entries: vec![],
        }
    }
    fn validate(&self, scope: &ContextScopeId) -> Result<(), SessionError> {
        if self.schema_version != VERSION || &self.scope != scope {
            return Err(SessionError::StorageUnavailable);
        }
        if self.entries.len() > MAX_EVENTS || encoded(self)?.len() > MAX_JOURNAL_BYTES {
            return Err(SessionError::LimitExceeded);
        }
        for (i, entry) in self.entries.iter().enumerate() {
            entry
                .decision
                .validate()
                .map_err(|_| SessionError::StorageUnavailable)?;
            if entry.ordinal != u64::try_from(i + 1).map_err(|_| SessionError::LimitExceeded)?
                || entry.previous.as_ref() != i.checked_sub(1).map(|p| &self.entries[p].digest)
                || entry.digest != entry.digest()?
                || entry.decision.context.allocation.binding.scope != self.scope
                || encoded(entry)?.len() > MAX_EVENT_BYTES
            {
                return Err(SessionError::StorageUnavailable);
            }
        }
        Ok(())
    }
}

/// Evidence receipt only. Its ordinal identifies an append, not a child mint.
pub struct ConnectorPolicyAuditReceipt {
    ordinal: u64,
    digest: ContentDigest,
}
impl ConnectorPolicyAuditReceipt {
    pub fn ordinal(&self) -> u64 {
        self.ordinal
    }
    pub fn digest(&self) -> &ContentDigest {
        &self.digest
    }
}
impl PostgresSessionStore {
    /// Append the captured Source evaluation without touching task-sessions-v2.
    /// The concrete Source authenticates current access and supplies its frozen
    /// basis; no audit read is allowed to replace the evaluated snapshot.
    pub fn record_connector_policy_decision(
        &self,
        owner: &OwnerBinding,
        context: &ConnectorPolicyAuditContext,
        evaluation: &ConnectorPolicyEvaluation<'_>,
    ) -> Result<ConnectorPolicyAuditReceipt, SessionError> {
        if &context.allocation.binding.owner != owner
            || &context.allocation.binding.scope != self.store.scope()
        {
            return Err(SessionError::ScopeDenied);
        }
        let decision = Decision::capture(context, evaluation)?;
        self.store.transact(
            KIND,
            &AuditJournal::initial(self.store.scope().clone()),
            |journal| {
                journal.validate(self.store.scope())?;
                if journal.entries.len() == MAX_EVENTS {
                    return Err(SessionError::LimitExceeded);
                }
                let ordinal = u64::try_from(journal.entries.len() + 1)
                    .map_err(|_| SessionError::LimitExceeded)?;
                let previous = journal.entries.last().map(|e| e.digest.clone());
                let entry_digest = digest(
                    "cg.connector-policy-audit-entry/1.0",
                    &(ordinal, &previous, &decision),
                )?;
                let entry = Entry {
                    ordinal,
                    previous,
                    decision,
                    digest: entry_digest,
                };
                if encoded(&entry)?.len() > MAX_EVENT_BYTES {
                    return Err(SessionError::LimitExceeded);
                }
                let receipt = ConnectorPolicyAuditReceipt {
                    ordinal,
                    digest: entry.digest.clone(),
                };
                journal.entries.push(entry);
                journal.validate(self.store.scope())?;
                Ok(receipt)
            },
        )
    }
}

#[cfg(test)]
mod tests;
