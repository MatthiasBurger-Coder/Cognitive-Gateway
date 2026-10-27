//! Cumulative session resource reservations, retained conservatively across
//! retries, consent pauses and restart. These values are accounting records;
//! only the trusted session journal can make a reservation authoritative.
use super::{DispatchId, FenceToken, OwnerBinding, Revision, RunId, SessionError, SessionId};
use gateway_domain::{
    ContentDigest, ContextScopeId, PlanStepId, ReferenceId, context_content_digest,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, time::Instant};

pub const MAX_SESSION_RESOURCE_RESERVATIONS: usize = 4096;

fn zero_connector_calls(value: &u64) -> bool {
    *value == 0
}

macro_rules! resource_amounts {
    ($($field:ident),+ $(,)?) => {
        /// Integer upper bounds in the named units. Cost is interpreted only
        /// in the immutable `SessionResourceLimits::cost_unit` of this ledger.
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct SessionResourceAmounts {
            $(pub $field: u64),+,
            /// One top-level connector application operation, including explicit
            /// discovery. Wire pagination/framing retain their separate bounds.
            /// Omission at zero preserves existing v1 record bytes and digests.
            #[serde(default, skip_serializing_if = "zero_connector_calls")]
            pub connector_calls: u64,
        }
        impl SessionResourceAmounts {
            pub fn is_zero(&self) -> bool { self.connector_calls == 0 && $(self.$field == 0)&&+ }
            pub fn checked_add(&self, other: &Self) -> Result<Self, SessionError> {
                Ok(Self { $($field: self.$field.checked_add(other.$field)
                    .ok_or(SessionError::LimitExceeded)?),+,
                    connector_calls: self.connector_calls.checked_add(other.connector_calls)
                        .ok_or(SessionError::LimitExceeded)?, })
            }
            pub fn checked_sub(&self, other: &Self) -> Result<Self, SessionError> {
                Ok(Self { $($field: self.$field.checked_sub(other.$field)
                    .ok_or(SessionError::LimitExceeded)?),+,
                    connector_calls: self.connector_calls.checked_sub(other.connector_calls)
                        .ok_or(SessionError::LimitExceeded)?, })
            }
            pub fn within(&self, limits: &Self) -> bool {
                self.connector_calls <= limits.connector_calls && $(self.$field <= limits.$field)&&+
            }
        }
    };
}
resource_amounts!(
    model_attempts,
    input_tokens,
    output_tokens,
    output_bytes,
    cost_microunits,
    retrieval_rounds,
    retrieval_results,
    retrieval_tokens,
    repair_attempts,
    worker_attempts,
    worker_compute_units,
);

/// A lifecycle-owned allocation. Only preparation can invoke or repair a model;
/// other phases may acquire/validate observations using their own resource slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionResourcePhase {
    Preparation,
    ApprovedResume,
    Execution,
    AfterSnapshotRecovery,
    GoalVerification,
}

/// Immutable disjoint phase ceilings. Their exact sum is the conservative debit.
/// Retrieval, worker compute and cost may appear in any phase; zero denies that
/// resource. Model invocation/output repair belongs only to preparation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "PlanWire")]
pub struct SessionResourcePlan {
    pub preparation: SessionResourceAmounts,
    pub approved_resume: SessionResourceAmounts,
    pub execution: SessionResourceAmounts,
    pub after_snapshot_recovery: SessionResourceAmounts,
    pub goal_verification: SessionResourceAmounts,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanWire {
    preparation: SessionResourceAmounts,
    approved_resume: SessionResourceAmounts,
    execution: SessionResourceAmounts,
    after_snapshot_recovery: SessionResourceAmounts,
    goal_verification: SessionResourceAmounts,
}
impl TryFrom<PlanWire> for SessionResourcePlan {
    type Error = SessionError;
    fn try_from(value: PlanWire) -> Result<Self, Self::Error> {
        let result = Self {
            preparation: value.preparation,
            approved_resume: value.approved_resume,
            execution: value.execution,
            after_snapshot_recovery: value.after_snapshot_recovery,
            goal_verification: value.goal_verification,
        };
        result.validate()?;
        Ok(result)
    }
}
impl SessionResourcePlan {
    pub fn amounts(&self, phase: SessionResourcePhase) -> &SessionResourceAmounts {
        match phase {
            SessionResourcePhase::Preparation => &self.preparation,
            SessionResourcePhase::ApprovedResume => &self.approved_resume,
            SessionResourcePhase::Execution => &self.execution,
            SessionResourcePhase::AfterSnapshotRecovery => &self.after_snapshot_recovery,
            SessionResourcePhase::GoalVerification => &self.goal_verification,
        }
    }
    pub fn total(&self) -> Result<SessionResourceAmounts, SessionError> {
        self.preparation
            .checked_add(&self.approved_resume)?
            .checked_add(&self.execution)?
            .checked_add(&self.after_snapshot_recovery)?
            .checked_add(&self.goal_verification)
    }
    pub fn validate(&self) -> Result<(), SessionError> {
        for amounts in [
            &self.approved_resume,
            &self.execution,
            &self.after_snapshot_recovery,
            &self.goal_verification,
        ] {
            if amounts.model_attempts != 0
                || amounts.input_tokens != 0
                || amounts.output_tokens != 0
                || amounts.output_bytes != 0
                || amounts.repair_attempts != 0
            {
                return Err(SessionError::InvalidInput);
            }
        }
        if self.total()?.is_zero() {
            return Err(SessionError::InvalidInput);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionResourceLimits {
    /// Host-selected currency/accounting unit. Reservations in different units
    /// must never share a ledger or be compared by their numeric cost alone.
    pub cost_unit: ReferenceId,
    pub amounts: SessionResourceAmounts,
}
impl SessionResourceLimits {
    pub fn validate(&self) -> Result<(), SessionError> {
        if self.amounts.is_zero() {
            return Err(SessionError::LimitExceeded);
        }
        Ok(())
    }
    fn digest(&self) -> Result<ContentDigest, SessionError> {
        digest(&("cg.session-resource-admission/1.0", self))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceReservationBinding {
    pub owner: OwnerBinding,
    pub scope: ContextScopeId,
    pub session: SessionId,
    pub run: RunId,
    pub revision: Revision,
    #[serde(with = "resource_step_id")]
    pub step: PlanStepId,
    pub dispatch: DispatchId,
}

mod resource_step_id {
    use super::*;

    pub fn serialize<S: serde::Serializer>(
        value: &PlanStepId,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(value.as_str())
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<PlanStepId, D::Error> {
        PlanStepId::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}
impl ResourceReservationBinding {
    pub fn validate(&self) -> Result<(), SessionError> {
        if self.revision.value() == 0 {
            return Err(SessionError::InvalidInput);
        }
        Ok(())
    }
    fn same_run(&self, other: &Self) -> bool {
        self.owner == other.owner
            && self.scope == other.scope
            && self.session == other.session
            && self.run == other.run
    }
}

fn digest<T: Serialize>(value: &T) -> Result<ContentDigest, SessionError> {
    Ok(context_content_digest(
        &serde_json::to_vec(value).map_err(|_| SessionError::InvalidInput)?,
    ))
}

fn reservation_digest(
    admission: &ContentDigest,
    binding: &ResourceReservationBinding,
    amounts: &SessionResourceAmounts,
    plan: Option<&SessionResourcePlan>,
) -> Result<ContentDigest, SessionError> {
    match plan {
        Some(plan) => digest(&(
            "cg.session-resource-reservation/2.0",
            admission,
            binding,
            amounts,
            plan,
        )),
        None => digest(&(
            "cg.session-resource-reservation/1.0",
            admission,
            binding,
            amounts,
        )),
    }
}

/// Immutable conservative debit. Its digest is an identity, never a grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ReservationWire")]
pub struct SessionResourceReservation {
    id: ContentDigest,
    admission_digest: ContentDigest,
    binding: ResourceReservationBinding,
    amounts: SessionResourceAmounts,
    #[serde(skip_serializing_if = "Option::is_none")]
    plan: Option<SessionResourcePlan>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReservationWire {
    id: ContentDigest,
    admission_digest: ContentDigest,
    binding: ResourceReservationBinding,
    amounts: SessionResourceAmounts,
    #[serde(default)]
    plan: Option<SessionResourcePlan>,
}
impl TryFrom<ReservationWire> for SessionResourceReservation {
    type Error = SessionError;
    fn try_from(value: ReservationWire) -> Result<Self, Self::Error> {
        let result = Self {
            id: value.id,
            admission_digest: value.admission_digest,
            binding: value.binding,
            amounts: value.amounts,
            plan: value.plan,
        };
        result.validate()?;
        Ok(result)
    }
}
impl SessionResourceReservation {
    pub fn id(&self) -> &ContentDigest {
        &self.id
    }
    pub fn binding(&self) -> &ResourceReservationBinding {
        &self.binding
    }
    pub fn amounts(&self) -> &SessionResourceAmounts {
        &self.amounts
    }
    pub fn plan(&self) -> Option<&SessionResourcePlan> {
        self.plan.as_ref()
    }
    pub fn validate(&self) -> Result<(), SessionError> {
        self.binding.validate()?;
        if let Some(plan) = &self.plan {
            plan.validate()?;
            if plan.total()? != self.amounts {
                return Err(SessionError::InvalidState);
            }
        }
        if self.amounts.is_zero()
            || self.id
                != reservation_digest(
                    &self.admission_digest,
                    &self.binding,
                    &self.amounts,
                    self.plan.as_ref(),
                )?
        {
            return Err(SessionError::InvalidState);
        }
        Ok(())
    }
}

/// Append-only reservations use their admitted upper bounds even when provider
/// telemetry is absent or smaller. There is intentionally no refund operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "LedgerWire")]
pub struct SessionResourceLedger {
    limits: SessionResourceLimits,
    used: SessionResourceAmounts,
    reservations: Vec<SessionResourceReservation>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LedgerWire {
    limits: SessionResourceLimits,
    used: SessionResourceAmounts,
    reservations: Vec<SessionResourceReservation>,
}
impl TryFrom<LedgerWire> for SessionResourceLedger {
    type Error = SessionError;
    fn try_from(value: LedgerWire) -> Result<Self, Self::Error> {
        let result = Self {
            limits: value.limits,
            used: value.used,
            reservations: value.reservations,
        };
        result.validate()?;
        Ok(result)
    }
}
impl SessionResourceLedger {
    pub fn new(limits: SessionResourceLimits) -> Result<Self, SessionError> {
        limits.validate()?;
        Ok(Self {
            limits,
            used: SessionResourceAmounts::default(),
            reservations: vec![],
        })
    }
    pub fn limits(&self) -> &SessionResourceLimits {
        &self.limits
    }
    pub fn used(&self) -> &SessionResourceAmounts {
        &self.used
    }
    pub fn reservations(&self) -> &[SessionResourceReservation] {
        &self.reservations
    }

    pub fn validate(&self) -> Result<(), SessionError> {
        self.limits.validate()?;
        if self.reservations.len() > MAX_SESSION_RESOURCE_RESERVATIONS
            || !self.used.within(&self.limits.amounts)
        {
            return Err(SessionError::LimitExceeded);
        }
        let admission = self.limits.digest()?;
        let mut sum = SessionResourceAmounts::default();
        let mut dispatches = BTreeSet::new();
        let mut prior: Option<&ResourceReservationBinding> = None;
        for reservation in &self.reservations {
            reservation.validate()?;
            let binding = reservation.binding();
            if reservation.admission_digest != admission
                || !dispatches.insert(&binding.dispatch)
                || prior
                    .is_some_and(|old| !old.same_run(binding) || old.revision >= binding.revision)
            {
                return Err(SessionError::InvalidState);
            }
            sum = sum.checked_add(reservation.amounts())?;
            prior = Some(binding);
        }
        if sum != self.used {
            return Err(SessionError::InvalidState);
        }
        Ok(())
    }

    /// Call only with trusted complete upper bounds, and persist the successor
    /// before performing any resource-consuming preparation or nested call.
    /// Rejected reservations do not change this ledger.
    pub fn reserve(
        &mut self,
        binding: ResourceReservationBinding,
        amounts: SessionResourceAmounts,
    ) -> Result<SessionResourceReservation, SessionError> {
        self.reserve_admitted(binding, amounts, None)
    }

    pub fn reserve_plan(
        &mut self,
        binding: ResourceReservationBinding,
        plan: SessionResourcePlan,
    ) -> Result<SessionResourceReservation, SessionError> {
        plan.validate()?;
        let amounts = plan.total()?;
        self.reserve_admitted(binding, amounts, Some(plan))
    }

    fn reserve_admitted(
        &mut self,
        binding: ResourceReservationBinding,
        amounts: SessionResourceAmounts,
        plan: Option<SessionResourcePlan>,
    ) -> Result<SessionResourceReservation, SessionError> {
        self.validate()?;
        binding.validate()?;
        if amounts.is_zero() {
            return Err(SessionError::InvalidInput);
        }
        if self.reservations.len() == MAX_SESSION_RESOURCE_RESERVATIONS {
            return Err(SessionError::LimitExceeded);
        }
        if self
            .reservations
            .iter()
            .any(|r| r.binding.dispatch == binding.dispatch)
        {
            return Err(SessionError::Duplicate);
        }
        if let Some(prior) = self.reservations.last() {
            if !prior.binding.same_run(&binding) {
                return Err(SessionError::ScopeDenied);
            }
            if prior.binding.revision >= binding.revision {
                return Err(SessionError::StaleRevision);
            }
        }
        let used = self.used.checked_add(&amounts)?;
        if !used.within(&self.limits.amounts) {
            return Err(SessionError::LimitExceeded);
        }
        let admission_digest = self.limits.digest()?;
        let reservation = SessionResourceReservation {
            id: reservation_digest(&admission_digest, &binding, &amounts, plan.as_ref())?,
            admission_digest,
            binding,
            amounts,
            plan,
        };
        self.used = used;
        self.reservations.push(reservation.clone());
        Ok(reservation)
    }

    /// A journal append must retain the complete old prefix and exact admission.
    /// Recomputed sums additionally prevent silently decreasing usage or replacing
    /// reservations with new identities, even when numeric totals match.
    pub fn validate_successor(&self, old: &Self) -> Result<(), SessionError> {
        old.validate()?;
        self.validate()?;
        if self.limits != old.limits || !self.reservations.starts_with(&old.reservations) {
            return Err(SessionError::InvalidState);
        }
        Ok(())
    }
}

/// Ephemeral phase ownership minted only by the coordinator after a durable
/// claim. It cannot be cloned or deserialized to refill a phase allowance.
#[derive(Debug)]
pub struct SessionPhaseResources {
    reservation_id: ContentDigest,
    binding: ResourceReservationBinding,
    phase: SessionResourcePhase,
    cost_unit: ReferenceId,
    remaining: SessionResourceAmounts,
    claim_revision: Revision,
    fence: FenceToken,
    deadline: Instant,
}
impl SessionPhaseResources {
    pub(crate) fn mint(
        reservation: &SessionResourceReservation,
        phase: SessionResourcePhase,
        cost_unit: ReferenceId,
        claim_revision: Revision,
        fence: FenceToken,
        deadline: Instant,
    ) -> Result<Self, SessionError> {
        reservation.validate()?;
        let plan = reservation
            .plan()
            .ok_or(SessionError::UnsupportedCapability)?;
        if claim_revision < reservation.binding().revision {
            return Err(SessionError::StaleRevision);
        }
        if Instant::now() >= deadline {
            return Err(SessionError::LimitExceeded);
        }
        Ok(Self {
            reservation_id: reservation.id().clone(),
            binding: reservation.binding().clone(),
            phase,
            cost_unit,
            remaining: *plan.amounts(phase),
            claim_revision,
            fence,
            deadline,
        })
    }
    pub fn reservation_id(&self) -> &ContentDigest {
        &self.reservation_id
    }
    pub fn binding(&self) -> &ResourceReservationBinding {
        &self.binding
    }
    pub fn phase(&self) -> SessionResourcePhase {
        self.phase
    }
    pub fn cost_unit(&self) -> &ReferenceId {
        &self.cost_unit
    }
    pub fn remaining(&self) -> &SessionResourceAmounts {
        &self.remaining
    }
    pub fn claim_revision(&self) -> Revision {
        self.claim_revision
    }
    pub fn fence(&self) -> FenceToken {
        self.fence
    }
    pub fn deadline(&self) -> Instant {
        self.deadline
    }

    /// Permanently consumes a disjoint slice. Failure does not change the
    /// remaining allowance; dropping an allocation never refunds its parent.
    pub fn allocate(
        &mut self,
        amounts: SessionResourceAmounts,
    ) -> Result<SessionResourceAllocation, SessionError> {
        if amounts.is_zero() {
            return Err(SessionError::InvalidInput);
        }
        if Instant::now() >= self.deadline {
            return Err(SessionError::LimitExceeded);
        }
        let remaining = self.remaining.checked_sub(&amounts)?;
        let allocation = SessionResourceAllocation {
            reservation_id: self.reservation_id.clone(),
            binding: self.binding.clone(),
            phase: self.phase,
            cost_unit: self.cost_unit.clone(),
            amounts,
            claim_revision: self.claim_revision,
            fence: self.fence,
            deadline: self.deadline,
        };
        self.remaining = remaining;
        Ok(allocation)
    }
}

/// A disjoint child allowance. Its identity must be checked against current
/// trusted session state before every nested dispatch. It is neither a wire
/// grant nor a substitute for current policy/consent or a fencing check.
#[derive(Debug)]
pub struct SessionResourceAllocation {
    reservation_id: ContentDigest,
    binding: ResourceReservationBinding,
    phase: SessionResourcePhase,
    cost_unit: ReferenceId,
    amounts: SessionResourceAmounts,
    claim_revision: Revision,
    fence: FenceToken,
    deadline: Instant,
}
impl SessionResourceAllocation {
    pub fn reservation_id(&self) -> &ContentDigest {
        &self.reservation_id
    }
    pub fn binding(&self) -> &ResourceReservationBinding {
        &self.binding
    }
    pub fn phase(&self) -> SessionResourcePhase {
        self.phase
    }
    pub fn cost_unit(&self) -> &ReferenceId {
        &self.cost_unit
    }
    pub fn amounts(&self) -> &SessionResourceAmounts {
        &self.amounts
    }
    pub fn claim_revision(&self) -> Revision {
        self.claim_revision
    }
    pub fn fence(&self) -> FenceToken {
        self.fence
    }
    pub fn deadline(&self) -> Instant {
        self.deadline
    }
}

/// Trusted current session lookup, not an authority inferred from allocation
/// metadata. Implementations revalidate exact owner/project/scope/session/run/
/// step/dispatch, reservation, phase, claim revision and fence, cancellation,
/// deadline and that the phase has not completed. Every nested adapter attempt
/// must call this source; capability possession alone grants no dispatch.
pub trait SessionResourceAuthorityPort: Send + Sync {
    fn validate_allocation(
        &self,
        allocation: &SessionResourceAllocation,
    ) -> Result<(), SessionError>;

    /// Joined current admission for this exact connector operation and child
    /// allocation. The trusted source validates the owner/project/session/run,
    /// step/dispatch, claim revision/fence/deadline and reservation together
    /// with task/execution, target, current inventory, typed arguments and the
    /// exact optional source. Neither request data nor possession of an
    /// allocation grants a transport call or substitutes for current CG-09.
    fn validate_connector_allocation(
        &self,
        _allocation: &SessionResourceAllocation,
        _request: &crate::connectors::ConnectorRequest,
        _source: Option<&gateway_domain::SourceId>,
    ) -> Result<(), SessionError> {
        Err(SessionError::UnsupportedCapability)
    }

    /// Current scoped discovery admission for the exact target and phase
    /// allocation. Discovery consumes its own top-level operation allowance;
    /// it never grants permission for a later tool or resource operation.
    fn validate_connector_discovery(
        &self,
        _allocation: &SessionResourceAllocation,
        _target: &crate::connectors::ConnectorTarget,
    ) -> Result<(), SessionError> {
        Err(SessionError::UnsupportedCapability)
    }

    /// Trusted joined admission for this exact invocation and resource claim.
    /// The source binds current owner/project/session/run/step/dispatch and
    /// allocation to the registered task, compiled context and authority
    /// producer pins. Independently valid same-scope objects are insufficient.
    /// Fallback may vary only the route already admitted by the existing
    /// invocation engine; it cannot select a different task or authority basis.
    /// A source without this explicit joined lookup grants no model dispatch.
    fn validate_invocation_allocation(
        &self,
        _allocation: &SessionResourceAllocation,
        _ir: &crate::prompt_ir::InvocationIr,
    ) -> Result<(), SessionError> {
        Err(SessionError::UnsupportedCapability)
    }

    /// Additional qualification for an estimated complete-payload token ceiling
    /// using current trusted model/tokenizer/profile and count provenance. The
    /// host verifies the exact documented upper-bound formula and its binding
    /// to the payload; arbitrary estimates or unknown methods never grant
    /// capacity. Exact tokenizer counts remain subject to the existing admitted
    /// exact-profile/render-accounting gate. This optional bound source denies
    /// by default; possession of an allocation never qualifies an estimate.
    fn validate_token_bound(
        &self,
        _allocation: &SessionResourceAllocation,
        _count: &gateway_domain::prompt_rendering::TokenCountResult,
        _payload: &str,
    ) -> Result<(), SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
}

#[cfg(test)]
mod phase_tests {
    use super::*;
    use crate::sessions::{BindingId, ClientOwnerId, PrincipalId, ProjectId, WorkspaceId};
    use std::time::Duration;

    fn amount(value: u64) -> SessionResourceAmounts {
        SessionResourceAmounts {
            model_attempts: value,
            input_tokens: value,
            output_tokens: value,
            output_bytes: value,
            cost_microunits: value,
            retrieval_rounds: value,
            retrieval_results: value,
            retrieval_tokens: value,
            repair_attempts: value,
            worker_attempts: value,
            worker_compute_units: value,
            connector_calls: value,
        }
    }
    fn binding() -> ResourceReservationBinding {
        ResourceReservationBinding {
            owner: OwnerBinding {
                principal: PrincipalId::new("operator").unwrap(),
                workspace: WorkspaceId::new("workspace").unwrap(),
                project: ProjectId::new("project").unwrap(),
                binding: BindingId::new("binding").unwrap(),
                client_owner: ClientOwnerId::new("client").unwrap(),
            },
            scope: ContextScopeId::new("scope").unwrap(),
            session: SessionId::new("session").unwrap(),
            run: RunId::new("run").unwrap(),
            revision: Revision::new(1).unwrap(),
            step: PlanStepId::new("step").unwrap(),
            dispatch: DispatchId::new("dispatch").unwrap(),
        }
    }
    fn plan() -> SessionResourcePlan {
        let read = SessionResourceAmounts {
            cost_microunits: 3,
            retrieval_rounds: 2,
            retrieval_results: 4,
            retrieval_tokens: 10,
            worker_attempts: 1,
            worker_compute_units: 2,
            ..Default::default()
        };
        SessionResourcePlan {
            preparation: amount(10),
            approved_resume: read,
            execution: read,
            after_snapshot_recovery: read,
            goal_verification: read,
        }
    }
    fn ledger() -> SessionResourceLedger {
        SessionResourceLedger::new(SessionResourceLimits {
            cost_unit: ReferenceId::new("operator-credit").unwrap(),
            amounts: amount(100),
        })
        .unwrap()
    }
    fn mint(
        reservation: &SessionResourceReservation,
        phase: SessionResourcePhase,
    ) -> SessionPhaseResources {
        SessionPhaseResources::mint(
            reservation,
            phase,
            ReferenceId::new("operator-credit").unwrap(),
            Revision::new(2).unwrap(),
            FenceToken::new(3).unwrap(),
            Instant::now() + Duration::from_secs(30),
        )
        .unwrap()
    }

    #[test]
    fn phase_allocations_keep_claim_identity_and_cannot_refund_or_reuse_parent_allowance() {
        let mut ledger = ledger();
        let reservation = ledger.reserve_plan(binding(), plan()).unwrap();
        for phase in [
            SessionResourcePhase::Preparation,
            SessionResourcePhase::ApprovedResume,
            SessionResourcePhase::Execution,
            SessionResourcePhase::AfterSnapshotRecovery,
            SessionResourcePhase::GoalVerification,
        ] {
            let mut cursor = mint(&reservation, phase);
            assert_eq!(cursor.reservation_id(), reservation.id());
            assert_eq!(cursor.binding(), reservation.binding());
            assert_eq!(cursor.phase(), phase);
            assert_eq!(cursor.cost_unit().as_str(), "operator-credit");
            assert_eq!(cursor.claim_revision().value(), 2);
            assert_eq!(cursor.fence().value(), 3);
            let deadline = cursor.deadline();
            let expected = *reservation.plan().unwrap().amounts(phase);
            assert_eq!(*cursor.remaining(), expected);
            let allocation = cursor.allocate(expected).unwrap();
            assert_eq!(allocation.reservation_id(), reservation.id());
            assert_eq!(allocation.binding(), reservation.binding());
            assert_eq!(allocation.phase(), phase);
            assert_eq!(allocation.cost_unit(), cursor.cost_unit());
            assert_eq!(allocation.amounts(), &expected);
            assert_eq!(allocation.claim_revision(), cursor.claim_revision());
            assert_eq!(allocation.fence(), cursor.fence());
            assert_eq!(allocation.deadline(), deadline);
            assert!(cursor.remaining().is_zero());
            drop(allocation);
            assert!(matches!(
                cursor.allocate(expected),
                Err(SessionError::LimitExceeded)
            ));
            assert!(cursor.remaining().is_zero());
        }
        let mut resume = mint(&reservation, SessionResourcePhase::ApprovedResume);
        let before = *resume.remaining();
        assert!(matches!(
            resume.allocate(plan().preparation),
            Err(SessionError::LimitExceeded)
        ));
        assert_eq!(*resume.remaining(), before);
        assert_eq!(*ledger.used(), plan().total().unwrap());
    }

    #[test]
    fn allocation_subtraction_is_checked_in_every_unit_and_does_not_change_on_denial() {
        let mut ledger = ledger();
        let reservation = ledger.reserve_plan(binding(), plan()).unwrap();
        let fields: Vec<String> = serde_json::to_value(amount(1))
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        for field in fields {
            let mut cursor = mint(&reservation, SessionResourcePhase::Preparation);
            let mut encoded = serde_json::to_value(SessionResourceAmounts::default()).unwrap();
            encoded[&field] = serde_json::json!(11);
            let request = serde_json::from_value(encoded.clone()).unwrap();
            assert!(
                matches!(cursor.allocate(request), Err(SessionError::LimitExceeded)),
                "{field}"
            );
            assert_eq!(*cursor.remaining(), amount(10));
            encoded[&field] = serde_json::json!(6);
            let request: SessionResourceAmounts = serde_json::from_value(encoded).unwrap();
            let first = cursor.allocate(request).unwrap();
            assert!(
                matches!(cursor.allocate(request), Err(SessionError::LimitExceeded)),
                "{field}"
            );
            assert_eq!(
                *cursor.remaining(),
                amount(10).checked_sub(&request).unwrap()
            );
            drop(first);
            assert!(
                matches!(cursor.allocate(request), Err(SessionError::LimitExceeded)),
                "{field}"
            );
        }
    }

    #[test]
    fn expired_stale_or_legacy_records_cannot_mint_or_spend_a_phase() {
        let mut ledger = ledger();
        let legacy = ledger.reserve(binding(), amount(1)).unwrap();
        assert!(matches!(
            SessionPhaseResources::mint(
                &legacy,
                SessionResourcePhase::Preparation,
                ReferenceId::new("operator-credit").unwrap(),
                Revision::new(2).unwrap(),
                FenceToken::new(1).unwrap(),
                Instant::now() + Duration::from_secs(30)
            ),
            Err(SessionError::UnsupportedCapability)
        ));
        let mut ledger = self::ledger();
        let reservation = ledger.reserve_plan(binding(), plan()).unwrap();
        assert!(matches!(
            SessionPhaseResources::mint(
                &reservation,
                SessionResourcePhase::Preparation,
                ReferenceId::new("operator-credit").unwrap(),
                Revision::new(0).unwrap(),
                FenceToken::new(1).unwrap(),
                Instant::now() + Duration::from_secs(30)
            ),
            Err(SessionError::StaleRevision)
        ));
        assert!(matches!(
            SessionPhaseResources::mint(
                &reservation,
                SessionResourcePhase::Preparation,
                ReferenceId::new("operator-credit").unwrap(),
                Revision::new(2).unwrap(),
                FenceToken::new(1).unwrap(),
                Instant::now()
            ),
            Err(SessionError::LimitExceeded)
        ));
        let mut cursor = mint(&reservation, SessionResourcePhase::Preparation);
        assert!(matches!(
            cursor.allocate(SessionResourceAmounts::default()),
            Err(SessionError::InvalidInput)
        ));
        cursor.deadline = Instant::now();
        assert!(matches!(
            cursor.allocate(amount(1)),
            Err(SessionError::LimitExceeded)
        ));
        assert_eq!(*cursor.remaining(), amount(10));
    }

    #[test]
    fn current_allocation_without_explicit_token_bound_qualification_denies_all_count_modes() {
        use gateway_domain::{ResolvedReference, SchemaVersion, prompt_rendering::*};
        struct AllocationOnly;
        impl SessionResourceAuthorityPort for AllocationOnly {
            fn validate_allocation(
                &self,
                _: &SessionResourceAllocation,
            ) -> Result<(), SessionError> {
                Ok(())
            }
        }
        let mut ledger = ledger();
        let reservation = ledger.reserve_plan(binding(), plan()).unwrap();
        let mut cursor = mint(&reservation, SessionResourcePhase::Preparation);
        let allocation = cursor.allocate(amount(1)).unwrap();
        AllocationOnly.validate_allocation(&allocation).unwrap();
        let payload = "trusted allocation is not tokenizer qualification";
        let reference = ResolvedReference {
            id: ReferenceId::new("unqualified-tokenizer").unwrap(),
            scope: binding().scope,
            contract: ReferenceId::new("tokenizer").unwrap(),
            contract_version: SchemaVersion::V1,
            revision: ReferenceId::new("1").unwrap(),
            digest: context_content_digest(b"unqualified"),
        };
        for mode in [CountMode::Exact, CountMode::Estimated] {
            let count = TokenCountResult {
                profile: TokenizerProfile {
                    schema_version: SchemaVersion::V1,
                    model: reference.clone(),
                    generation_profile: context_content_digest(b"generation"),
                    tokenizer: ReferenceId::new("unqualified-tokenizer").unwrap(),
                    tokenizer_version: ReferenceId::new("1").unwrap(),
                    renderer: ReferenceId::new("renderer").unwrap(),
                    template_version: ReferenceId::new("1").unwrap(),
                },
                payload_digest: context_content_digest(payload.as_bytes()),
                mode,
                provenance: reference.clone(),
                usage: TokenUsage {
                    instruction_tokens: 0,
                    task_tokens: 0,
                    context_tokens: 0,
                    evidence_tokens: 0,
                    tool_schema_tokens: 0,
                    framing_tokens: 1,
                },
            };
            assert_eq!(
                AllocationOnly.validate_token_bound(&allocation, &count, payload),
                Err(SessionError::UnsupportedCapability)
            );
        }
    }

    #[test]
    fn generic_allocation_without_joined_connector_authority_never_grants_calls_or_discovery() {
        use crate::connectors::{ConnectorOperationClass, ConnectorRequest, ConnectorTarget};
        use gateway_domain::{CapabilityId, NonEmptyText, SourceId, TaskId};
        struct AllocationOnly;
        impl SessionResourceAuthorityPort for AllocationOnly {
            fn validate_allocation(
                &self,
                _: &SessionResourceAllocation,
            ) -> Result<(), SessionError> {
                Ok(())
            }
        }
        let mut ledger = ledger();
        let reservation = ledger.reserve_plan(binding(), plan()).unwrap();
        let mut cursor = mint(&reservation, SessionResourcePhase::Preparation);
        let allocation = cursor
            .allocate(SessionResourceAmounts {
                connector_calls: 1,
                ..Default::default()
            })
            .unwrap();
        AllocationOnly.validate_allocation(&allocation).unwrap();
        let target = ConnectorTarget {
            connector: NonEmptyText::new("registered-connector").unwrap(),
            scope: binding().scope,
        };
        let request = ConnectorRequest {
            target: target.clone(),
            capability: CapabilityId::new("repository.read").unwrap(),
            class: ConnectorOperationClass::Read,
            inventory_revision: context_content_digest(b"inventory"),
            arguments: Default::default(),
            task: TaskId::new("task").unwrap(),
            step: binding().step,
            execution: NonEmptyText::new("execution").unwrap(),
            correlation: NonEmptyText::new("correlation").unwrap(),
        };
        assert_eq!(
            AllocationOnly.validate_connector_discovery(&allocation, &target),
            Err(SessionError::UnsupportedCapability)
        );
        assert_eq!(
            AllocationOnly.validate_connector_allocation(&allocation, &request, None),
            Err(SessionError::UnsupportedCapability)
        );
        assert_eq!(
            AllocationOnly.validate_connector_allocation(
                &allocation,
                &request,
                Some(&SourceId::new("repository").unwrap())
            ),
            Err(SessionError::UnsupportedCapability)
        );
    }
}
