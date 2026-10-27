//! CG-09 admission for outbound connector requests.
//!
//! The supplied source is a trusted composition boundary: it authenticates and
//! loads current policy, request facts, consent and aggregate reservation data.
//! This module evaluates those facts with the existing CG-09 engine and binds
//! the resulting dispatch to the exact request. It performs no I/O itself.

use crate::connectors::{
    ConnectorAdmissionPort, ConnectorDispatch, ConnectorFailure, ConnectorFailureCode,
    ConnectorOperationClass, ConnectorRequest, ConnectorTarget,
};
use gateway_domain::{
    CapabilityClass, CapabilityId, ExecutionProfile, OperatingMode, PlanStepId, PolicyId, SourceId,
    UnixTimestamp,
};
use gateway_policy::{
    PolicyAuthority, PolicyDecision, PolicyEngine, PolicyFinding, PolicyReason, ProcessReadiness,
    StepFacts, StepPolicyInput, StepPolicyReport,
};
use std::collections::BTreeSet;

/// Facts captured by a trusted host for one exact connector request.
///
/// `administrative_capabilities` is operator-owned. ADMIN requests are denied
/// unless their canonical capability appears here and the canonical CG-09
/// capability is Mutate. `consent_required` may add a requirement; it cannot
/// grant consent. Mutate always retains CG-09's existing consent requirement.
#[derive(Clone)]
pub struct ConnectorPolicySnapshot {
    pub step: PlanStepId,
    pub resolved: bool,
    pub has_prerequisites: bool,
    pub constraints: BTreeSet<String>,
    pub preconditions: BTreeSet<String>,
    pub authority: PolicyAuthority,
    pub facts: StepFacts,
    pub operating_mode: OperatingMode,
    pub execution_profile: ExecutionProfile,
    pub process: ProcessReadiness,
    pub consent_required: bool,
    pub administrative_capabilities: BTreeSet<CapabilityId>,
    pub policy: PolicyId,
    pub policy_revision: gateway_domain::ContentDigest,
    pub consent_receipt: Option<gateway_domain::NonEmptyText>,
    pub budget_reservation: gateway_domain::NonEmptyText,
    pub deadline: UnixTimestamp,
}

/// The audited boundary; pre-effect evaluation retains its original dispatch.
/// Deliberately not Debug: the dispatch can contain sensitive action arguments.
#[derive(Clone, Copy)]
pub enum ConnectorPolicyStage<'a> {
    Authorize,
    PreEffect(&'a ConnectorDispatch),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorPolicyOutcome {
    Allowed,
    Denied(ConnectorFailureCode),
}

/// Immutable evaluated facts, captured before calling the audit sink. A policy
/// report may allow while the effective outcome denies a stale dispatch binding.
/// Contract failures before policy evaluation have no fabricated policy report.
pub struct ConnectorPolicyEvaluation<'a> {
    request: &'a ConnectorRequest,
    source: Option<&'a SourceId>,
    snapshot: &'a ConnectorPolicySnapshot,
    report: Option<&'a StepPolicyReport>,
    outcome: ConnectorPolicyOutcome,
    stage: ConnectorPolicyStage<'a>,
}
impl<'a> ConnectorPolicyEvaluation<'a> {
    pub fn request(&self) -> &'a ConnectorRequest {
        self.request
    }
    pub fn source(&self) -> Option<&'a SourceId> {
        self.source
    }
    pub fn snapshot(&self) -> &'a ConnectorPolicySnapshot {
        self.snapshot
    }
    pub fn report(&self) -> Option<&'a StepPolicyReport> {
        self.report
    }
    pub fn outcome(&self) -> ConnectorPolicyOutcome {
        self.outcome
    }
    pub fn stage(&self) -> ConnectorPolicyStage<'a> {
        self.stage
    }
}

/// Trusted policy and budget source. Implementations must authenticate the
/// caller out of band and return current facts bound to all request fields,
/// including scope, source, typed arguments and inventory revision.
pub trait ConnectorPolicySource: Send + Sync {
    /// Bounded, read-only current target lookup. Transport liveness polls this
    /// method; it must not dispatch effects, acquire grants or record decisions.
    fn admit_target(&self, target: &ConnectorTarget) -> Result<(), ConnectorFailure>;
    fn authorize(
        &self,
        request: &ConnectorRequest,
        source: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure>;
    /// Read current exact bound facts only. This method also serves transport
    /// liveness checks: it must not record decisions, acquire grants or budget
    /// reservations, or perform connector effects. Audit writes belong solely
    /// to `record_decision` at normal admission/pre-effect boundaries.
    fn revalidate(
        &self,
        dispatch: &ConnectorDispatch,
        source: Option<&SourceId>,
    ) -> Result<ConnectorPolicySnapshot, ConnectorFailure>;
    /// Persist or forward stable CG-09 findings for audit/explanation. This
    /// port must not treat the report as a grant or place raw provider data in it.
    fn record_decision(
        &self,
        evaluation: &ConnectorPolicyEvaluation<'_>,
    ) -> Result<(), ConnectorFailure>;
}

/// Existing CG-09 policy engine wired to the connector admission port.
pub struct Cg09ConnectorAdmission<S> {
    source: S,
}

impl<S> Cg09ConnectorAdmission<S> {
    pub fn new(source: S) -> Self {
        Self { source }
    }
}

impl<S: ConnectorPolicySource> ConnectorAdmissionPort for Cg09ConnectorAdmission<S> {
    fn admit_target(&self, target: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        self.source.admit_target(target)
    }

    fn authorize(
        &self,
        request: &ConnectorRequest,
        source: Option<&SourceId>,
    ) -> Result<ConnectorDispatch, ConnectorFailure> {
        self.source.admit_target(&request.target)?;
        let snapshot = self.source.authorize(request, source)?;
        let report = preview_connector_policy(request, &snapshot);
        let result = report
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|report| require_allow(request, report));
        self.source.record_decision(&ConnectorPolicyEvaluation {
            request,
            source,
            snapshot: &snapshot,
            report: report.as_ref().ok(),
            outcome: outcome(&result),
            stage: ConnectorPolicyStage::Authorize,
        })?;
        result?;
        Ok(ConnectorDispatch {
            request: request.clone(),
            policy: snapshot.policy,
            policy_revision: snapshot.policy_revision,
            consent_receipt: snapshot.consent_receipt,
            budget_reservation: snapshot.budget_reservation,
            deadline: snapshot.deadline,
        })
    }

    fn revalidate(
        &self,
        dispatch: &ConnectorDispatch,
        source: Option<&SourceId>,
    ) -> Result<(), ConnectorFailure> {
        self.source.admit_target(&dispatch.request.target)?;
        let snapshot = self.source.revalidate(dispatch, source)?;
        let report = preview_connector_policy(&dispatch.request, &snapshot);
        let result = report
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|report| validate_dispatch_report(dispatch, &snapshot, report));
        self.source.record_decision(&ConnectorPolicyEvaluation {
            request: &dispatch.request,
            source,
            snapshot: &snapshot,
            report: report.as_ref().ok(),
            outcome: outcome(&result),
            stage: ConnectorPolicyStage::PreEffect(dispatch),
        })?;
        result?;
        // Audit persistence can revoke current authority. The successful
        // pre-effect boundary must observe that without writing another audit.
        self.validate_current_dispatch(dispatch, source)
    }

    fn validate_current_dispatch(
        &self,
        dispatch: &ConnectorDispatch,
        source: Option<&SourceId>,
    ) -> Result<(), ConnectorFailure> {
        self.source.admit_target(&dispatch.request.target)?;
        let snapshot = self.source.revalidate(dispatch, source)?;
        let report = preview_connector_policy(&dispatch.request, &snapshot)?;
        validate_dispatch_report(dispatch, &snapshot, &report)
    }
}

fn outcome(result: &Result<(), ConnectorFailure>) -> ConnectorPolicyOutcome {
    match result {
        Ok(()) => ConnectorPolicyOutcome::Allowed,
        Err(error) => ConnectorPolicyOutcome::Denied(error.code),
    }
}

fn require_allow(
    request: &ConnectorRequest,
    report: &StepPolicyReport,
) -> Result<(), ConnectorFailure> {
    if report.decision != PolicyDecision::Allow {
        return Err(failure(request, ConnectorFailureCode::PermissionDenied));
    }
    Ok(())
}

/// Pure checks shared by audited pre-effect admission and validation-only
/// liveness. Polling must enforce the same current decision and exact grant
/// identity without recursively invoking the audit sink.
fn validate_dispatch_report(
    dispatch: &ConnectorDispatch,
    snapshot: &ConnectorPolicySnapshot,
    report: &StepPolicyReport,
) -> Result<(), ConnectorFailure> {
    require_allow(&dispatch.request, report)?;
    if dispatch.policy != snapshot.policy
        || dispatch.policy_revision != snapshot.policy_revision
        || dispatch.consent_receipt != snapshot.consent_receipt
        || dispatch.budget_reservation != snapshot.budget_reservation
        || dispatch.deadline != snapshot.deadline
    {
        return Err(failure(
            &dispatch.request,
            ConnectorFailureCode::PermissionDenied,
        ));
    }
    Ok(())
}

/// Inspect the exact native connector policy requirements without constructing
/// a dispatch, recording a decision, acquiring consent or performing effects.
/// Hosts use the same evaluation for an outstanding consent question; an Allow
/// report is not an execution grant or proof of a compiled action context.
/// The caller must independently authenticate the captured snapshot. Actual
/// dispatch still crosses the audited admission and current-context boundary.
pub fn preview_connector_policy(
    request: &ConnectorRequest,
    snapshot: &ConnectorPolicySnapshot,
) -> Result<StepPolicyReport, ConnectorFailure> {
    let failure = || failure(request, ConnectorFailureCode::PermissionDenied);
    if snapshot.step != request.step {
        return Err(failure());
    }
    if !snapshot
        .authority
        .policies
        .iter()
        .any(|policy| policy.id() == &snapshot.policy)
    {
        return Err(failure());
    }
    let expected = match request.class {
        ConnectorOperationClass::Read
        | ConnectorOperationClass::Search
        | ConnectorOperationClass::Inspect => CapabilityClass::Inspect,
        ConnectorOperationClass::Mutate | ConnectorOperationClass::Admin => CapabilityClass::Mutate,
    };
    let Some(capability) = snapshot.authority.capabilities.get(&request.capability) else {
        return Err(failure());
    };
    if capability.class() != expected
        || (request.class == ConnectorOperationClass::Admin
            && !snapshot
                .administrative_capabilities
                .contains(&request.capability))
    {
        return Err(failure());
    }
    let mut capabilities = std::collections::BTreeMap::new();
    capabilities.insert(request.capability.clone(), capability.clone());
    let mut report = PolicyEngine::evaluate(
        &snapshot.authority,
        &StepPolicyInput {
            step: &request.step,
            capabilities: &capabilities,
            operating_mode: snapshot.operating_mode,
            execution_profile: snapshot.execution_profile,
            process: snapshot.process,
            resolved: snapshot.resolved,
            has_prerequisites: snapshot.has_prerequisites,
            constraints: &snapshot.constraints,
            preconditions: &snapshot.preconditions,
            facts: &snapshot.facts,
        },
    );
    let consent_needed = snapshot.consent_required || expected == CapabilityClass::Mutate;
    if consent_needed
        && (snapshot.consent_receipt.is_none()
            || snapshot.facts.consents.get(&request.capability)
                != Some(&gateway_policy::Approval::Granted))
    {
        report.findings.insert(PolicyFinding {
            decision: PolicyDecision::RequireConsent,
            reason: PolicyReason::ConsentMissing,
            subject: request.capability.to_string(),
        });
        report.decision = report.decision.max(PolicyDecision::RequireConsent);
    }
    Ok(report)
}

fn failure(request: &ConnectorRequest, code: ConnectorFailureCode) -> ConnectorFailure {
    ConnectorFailure {
        code,
        effect: crate::connectors::ConnectorEffect::NotDispatched,
        correlation: request.correlation.clone(),
    }
}
