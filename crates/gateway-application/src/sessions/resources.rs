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

/// The initial observation phase cannot dispatch mutations or models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionResourcePhase {
    Preparation,
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
        self.amounts.validate_readonly()?;
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
) -> Result<ContentDigest, SessionError> {
    digest(&(
        "cg.session-goal-observation-reservation/1.0",
        admission,
        binding,
        amounts,
    ))
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionResourceReservation {
    id: ContentDigest,
    admission_digest: ContentDigest,
    binding: ResourceReservationBinding,
    amounts: SessionResourceAmounts,
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
    pub fn validate(&self) -> Result<(), SessionError> {
        self.binding.validate()?;
        self.amounts.validate_readonly()?;
        if self.amounts.is_zero()
            || self.id != reservation_digest(&self.admission_digest, &self.binding, &self.amounts)?
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
        self.reserve_admitted(binding, amounts)
    }

    fn reserve_admitted(
        &mut self,
        binding: ResourceReservationBinding,
        amounts: SessionResourceAmounts,
    ) -> Result<SessionResourceReservation, SessionError> {
        self.validate()?;
        binding.validate()?;
        amounts.validate_readonly()?;
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
            id: reservation_digest(&admission_digest, &binding, &amounts)?,
            admission_digest,
            binding,
            amounts,
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
            remaining: *reservation.amounts(),
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

impl SessionResourceAmounts {
    pub fn validate_readonly(&self) -> Result<(), SessionError> {
        if self.model_attempts != 0
            || self.input_tokens != 0
            || self.output_tokens != 0
            || self.output_bytes != 0
            || self.repair_attempts != 0
            || self.worker_attempts != 0
            || self.worker_compute_units != 0
        {
            return Err(SessionError::UnsupportedCapability);
        }
        Ok(())
    }
}
/// Fresh actual resource/session authority remains separate from allocation data.
pub trait SessionResourceAuthorityPort: Send + Sync {
    fn validate_allocation(
        &self,
        allocation: &SessionResourceAllocation,
    ) -> Result<(), SessionError>;
    fn validate_connector_allocation(
        &self,
        _allocation: &SessionResourceAllocation,
        _request: &crate::connectors::ConnectorRequest,
        _source: Option<&gateway_domain::SourceId>,
    ) -> Result<(), SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
    fn validate_connector_discovery(
        &self,
        _allocation: &SessionResourceAllocation,
        _target: &crate::connectors::ConnectorTarget,
    ) -> Result<(), SessionError> {
        Err(SessionError::UnsupportedCapability)
    }
}
