//! Content-free retained CG09 decisions; never read as grants or spending cursors.
use super::*;
use gateway_application::{
    connector_policy::{ConnectorPolicyEvaluation, ConnectorPolicyOutcome, ConnectorPolicyStage},
    connectors::ConnectorOperationClass,
};
use gateway_domain::{ContentDigest, ReferenceId};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
const MAX_AUDITS: usize = 4096;
const MAX_ENTRY_BYTES: usize = 65_536;
const MAX_TOTAL_BYTES: usize = 4_194_304;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ObservationPolicyAudit {
    reference: RecordRef,
    bytes: Vec<u8>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuditBody {
    contract: String,
    contract_version: String,
    ordinal: u64,
    recorded_at_ms: u64,
    owner: OwnerBinding,
    binding: ResourceReservationBinding,
    claim_revision: Revision,
    fence: u64,
    reservation: ContentDigest,
    cost_unit: ReferenceId,
    amounts: SessionResourceAmounts,
    request_digest: ContentDigest,
    arguments_digest: String,
    source: Option<SourceId>,
    capability: CapabilityId,
    stage: String,
    outcome: Value,
    snapshot: Value,
    dispatch: Option<Value>,
    report: Option<AuditReport>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuditReport {
    schema_version: u16,
    step: String,
    decision: Value,
    policy_digests: Vec<ContentDigest>,
    capability_classes: BTreeMap<CapabilityId, String>,
    findings: Vec<AuditFinding>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuditFinding {
    decision: Value,
    reason: Value,
    subject_digest: ContentDigest,
}
fn digest<T: Serialize>(v: &T) -> Result<ContentDigest, SessionError> {
    Ok(gateway_domain::context_content_digest(
        &serde_json::to_vec(v).map_err(|_| SessionError::InvalidInput)?,
    ))
}
fn now_ms() -> Result<u64, SessionError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| SessionError::Unavailable)?
        .as_millis()
        .try_into()
        .map_err(|_| SessionError::LimitExceeded)
}
fn reference(bytes: &[u8]) -> Result<RecordRef, SessionError> {
    let hash = gateway_domain::context_content_digest(bytes);
    content_reference(&format!("observation-policy-{}", hash.as_str()), bytes)
}
pub(super) fn validate(journal: &Journal, scope: &ContextScopeId) -> Result<(), SessionError> {
    if journal.observation_policy_audits.len() > MAX_AUDITS
        || journal
            .observation_policy_audits
            .iter()
            .try_fold(0usize, |n, a| n.checked_add(a.bytes.len()))
            .is_none_or(|n| n > MAX_TOTAL_BYTES)
    {
        return Err(SessionError::StorageUnavailable);
    }
    for (i, audit) in journal.observation_policy_audits.iter().enumerate() {
        if audit.bytes.len() > MAX_ENTRY_BYTES || reference(&audit.bytes)? != audit.reference {
            return Err(SessionError::StorageUnavailable);
        }
        let body: AuditBody =
            serde_json::from_slice(&audit.bytes).map_err(|_| SessionError::StorageUnavailable)?;
        if body.contract != "cg.connector-observation-policy-audit"
            || body.contract_version != "1.0"
            || body.ordinal != (i as u64 + 1)
            || body.recorded_at_ms == 0
            || body.binding.scope != *scope
            || body.owner != body.binding.owner
            || body.claim_revision != body.binding.revision
            || body.fence == 0
            || body.amounts.validate_readonly().is_err()
            || body.amounts.is_zero()
            || !matches!(body.stage.as_str(), "AUTHORIZE" | "PRE_EFFECT")
            || (body.stage == "AUTHORIZE") != body.dispatch.is_none()
        {
            return Err(SessionError::StorageUnavailable);
        }
        let session = journal
            .sessions
            .iter()
            .find(|s| {
                s.checkpoint.owner == body.owner
                    && s.checkpoint.snapshot.session == body.binding.session
                    && s.checkpoint.snapshot.run == body.binding.run
            })
            .ok_or(SessionError::StorageUnavailable)?;
        let cp = session.typed()?.checkpoint;
        let ledger = cp
            .resource_ledger
            .as_ref()
            .ok_or(SessionError::StorageUnavailable)?;
        if body.claim_revision > cp.snapshot.revision
            || body.fence > session.fence
            || body.cost_unit != ledger.limits().cost_unit
            || !ledger.reservations().iter().any(|r| {
                r.id() == &body.reservation
                    && r.binding() == &body.binding
                    && body.amounts.within(r.amounts())
            })
        {
            return Err(SessionError::StorageUnavailable);
        }
    }
    Ok(())
}
impl PostgresSessionStore {
    /// Persist the actual CG09 evaluation under the still-live, opaque preparation
    /// allocation. This changes neither checkpoint revision nor its fencing token.
    /// Arguments, finding subjects and consent receipts are represented by digests.
    pub fn record_observation_policy_decision(
        &self,
        owner: &OwnerBinding,
        allocation: &SessionResourceAllocation,
        evaluation: &ConnectorPolicyEvaluation<'_>,
    ) -> Result<(), SessionError> {
        allocation.amounts().validate_readonly()?;
        if allocation.amounts().is_zero() || Instant::now() >= allocation.deadline() {
            return Err(SessionError::LimitExceeded);
        }
        if allocation.binding().owner != *owner || allocation.binding().scope != *self.store.scope()
        {
            return Err(SessionError::ScopeDenied);
        }
        let request = evaluation.request();
        if request.arguments.len() > 256
            || serde_json::to_vec(&request.arguments)
                .map_err(|_| SessionError::InvalidInput)?
                .len()
                > 262_144
        {
            return Err(SessionError::LimitExceeded);
        }
        if request.target.scope != allocation.binding().scope
            || request.step != allocation.binding().step
            || matches!(
                request.class,
                ConnectorOperationClass::Mutate | ConnectorOperationClass::Admin
            )
            || evaluation.snapshot().budget_reservation.as_str()
                != allocation.reservation_id().as_str()
        {
            return Err(SessionError::ScopeDenied);
        }
        let report = evaluation
            .report()
            .map(|r| -> Result<AuditReport, SessionError> {
                if r.findings.len() > 256
                    || r.policies.len() > 256
                    || r.capability_classes.len() > 256
                    || r.findings.iter().any(|f| f.subject.len() > 65_536)
                {
                    return Err(SessionError::LimitExceeded);
                }
                Ok(AuditReport {
                    schema_version: r.schema_version,
                    step: r.step.as_str().into(),
                    decision: json!(r.decision),
                    policy_digests: r.policies.iter().map(digest).collect::<Result<_, _>>()?,
                    capability_classes: r.capability_classes.clone(),
                    findings: r
                        .findings
                        .iter()
                        .map(|f| AuditFinding {
                            decision: json!(f.decision),
                            reason: json!(f.reason),
                            subject_digest: gateway_domain::context_content_digest(
                                f.subject.as_bytes(),
                            ),
                        })
                        .collect(),
                })
            })
            .transpose()?;
        let snapshot = evaluation.snapshot();
        let snapshot = json!({"step":snapshot.step.as_str(),"policy":snapshot.policy,"policy_revision":snapshot.policy_revision,"budget_reservation":allocation.reservation_id(),"deadline_seconds":snapshot.deadline.seconds(),"consent_receipt_digest":snapshot.consent_receipt.as_ref().map(|r|gateway_domain::context_content_digest(r.as_str().as_bytes()))});
        let (stage, dispatch) = match evaluation.stage() {
            ConnectorPolicyStage::Authorize => ("AUTHORIZE", None),
            ConnectorPolicyStage::PreEffect(d) => (
                "PRE_EFFECT",
                Some(
                    json!({"request_digest":connector_request_digest(&d.request)?,"policy":d.policy,"policy_revision":d.policy_revision,"reservation_digest":gateway_domain::context_content_digest(d.budget_reservation.as_str().as_bytes()),"consent_receipt_digest":d.consent_receipt.as_ref().map(|r|gateway_domain::context_content_digest(r.as_str().as_bytes())),"deadline_seconds":d.deadline.seconds()}),
                ),
            ),
        };
        let outcome = match evaluation.outcome() {
            ConnectorPolicyOutcome::Allowed => json!({"kind":"ALLOWED"}),
            ConnectorPolicyOutcome::Denied(code) => {
                json!({"kind":"DENIED","code":format!("{code:?}")})
            }
        };
        let identity = ConnectorRequestIdentity::from_request(request)?;
        let request_digest = connector_request_digest(request)?;
        self.run(|journal| {
            let current =
                journal.sessions[journal.index(owner, &allocation.binding().session)?].typed()?;
            let cp = &current.checkpoint;
            let now = now_ms()?;
            if current.fence != allocation.fence()
                || cp.snapshot.revision != allocation.claim_revision()
                || allocation.binding().revision != allocation.claim_revision()
            {
                return Err(SessionError::StaleRevision);
            }
            if cp.snapshot.state != SessionState::Dispatching
                || cp.lease_until_ms.is_none_or(|until| now >= until)
                || now >= cp.budget.deadline_ms
                || Instant::now() >= allocation.deadline()
                || cp.snapshot.dispatch.as_ref() != Some(&allocation.binding().dispatch)
            {
                return Err(SessionError::AuthorityDenied);
            }
            let goal = cp.goal.as_connector().ok_or(SessionError::InvalidState)?;
            if !goal.observations().iter().any(|m| {
                m.capability() == &request.capability
                    && m.operation_class() == request.class
                    && evaluation.source().is_none_or(|s| m.source() == s)
            }) {
                return Err(SessionError::ScopeDenied);
            }
            let ledger = cp
                .resource_ledger
                .as_ref()
                .ok_or(SessionError::InvalidState)?;
            if allocation.cost_unit() != &ledger.limits().cost_unit
                || !ledger.reservations().iter().any(|r| {
                    r.id() == allocation.reservation_id()
                        && r.binding() == allocation.binding()
                        && allocation.amounts().within(r.amounts())
                })
            {
                return Err(SessionError::AuthorityDenied);
            }
            if journal.observation_policy_audits.len() >= MAX_AUDITS {
                return Err(SessionError::LimitExceeded);
            }
            let body = AuditBody {
                contract: "cg.connector-observation-policy-audit".into(),
                contract_version: "1.0".into(),
                ordinal: journal.observation_policy_audits.len() as u64 + 1,
                recorded_at_ms: now,
                owner: owner.clone(),
                binding: allocation.binding().clone(),
                claim_revision: allocation.claim_revision(),
                fence: allocation.fence().value(),
                reservation: allocation.reservation_id().clone(),
                cost_unit: allocation.cost_unit().clone(),
                amounts: *allocation.amounts(),
                request_digest,
                arguments_digest: identity.arguments_digest,
                source: evaluation.source().cloned(),
                capability: request.capability.clone(),
                stage: stage.into(),
                outcome,
                snapshot,
                dispatch,
                report,
            };
            let bytes = serde_json::to_vec(&body).map_err(|_| SessionError::InvalidInput)?;
            if bytes.len() > MAX_ENTRY_BYTES
                || journal
                    .observation_policy_audits
                    .iter()
                    .map(|a| a.bytes.len())
                    .sum::<usize>()
                    .checked_add(bytes.len())
                    .is_none_or(|n| n > MAX_TOTAL_BYTES)
            {
                return Err(SessionError::LimitExceeded);
            }
            journal
                .observation_policy_audits
                .push(ObservationPolicyAudit {
                    reference: reference(&bytes)?,
                    bytes,
                });
            Ok(())
        })
    }
}
