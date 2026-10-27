//! Authenticated immutable Before records. Historical DATA revalidates at the
//! recorded acquisition validation time; it never claims current freshness.
use super::super::scoped_snapshots::{
    SnapshotError,
    codec::{canonical, strict_value},
};
use super::*;
use serde_json::Value;

const CONTRACT: &str = "cg.session-prepared-before";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    contract: String,
    contract_version: SchemaVersion,
    binding: SnapshotBinding,
    universe: SnapshotPin,
    profile: SnapshotPin,
    runtime: SnapshotPin,
    native_context_digest: ContentDigest,
    snapshot_digest: ContentDigest,
    committed_revision: Revision,
    validated_at_ms: u64,
    snapshot: Value,
}
fn encoding(_: serde_json::Error) -> SessionError {
    SessionError::InvalidInteraction
}
fn snapshot_error(error: SnapshotError) -> SessionError {
    match error {
        SnapshotError::LimitExceeded => SessionError::LimitExceeded,
        SnapshotError::Expired => SessionError::ExpiredInteraction,
        _ => SessionError::InvalidInteraction,
    }
}
fn check_runtime(pin: &SnapshotPin, binding: &SnapshotBinding) -> Result<(), SessionError> {
    if pin.scope != binding.capture.scope || pin.contract_version != SchemaVersion::V1 {
        return Err(SessionError::InvalidInteraction);
    }
    Ok(())
}
/// Only the coordinator may call this after obtaining the actual durable claim.
/// The reference digest covers the final owned envelope, not a caller's digest.
pub(crate) fn prepare_before_record(
    claim: &FencedSession,
    captured: &CapturedBefore,
    validated_at_ms: u64,
) -> Result<(PreparedBeforeRef, RecordWrite), SessionError> {
    let cp = &claim.checkpoint;
    let binding = captured.snapshot.binding();
    let capture = &binding.capture;
    let reservation = binding
        .reservation
        .as_ref()
        .ok_or(SessionError::InvalidInteraction)?;
    if cp.snapshot.state != SessionState::Dispatching
        || cp.prepared_before.is_some()
        || binding.phase != SnapshotPhase::Before
        || reservation.fence != claim.fence
        || capture.owner != cp.owner
        || capture.scope != *cp.goal.scope()
        || capture.session != cp.snapshot.session
        || capture.run != cp.snapshot.run
        || capture.revision != cp.snapshot.revision
        || cp.snapshot.dispatch.as_ref() != Some(&capture.dispatch)
        || !cp
            .goal
            .as_connector()
            .is_some_and(|g| g.steps().contains(&capture.step))
        || !cp.resource_ledger.as_ref().is_some_and(|l| {
            l.reservations()
                .iter()
                .any(|r| r.binding() == capture && r.id() == &reservation.reservation)
        })
        || validated_at_ms == 0
        || validated_at_ms >= cp.budget.deadline_ms
        || cp
            .lease_until_ms
            .is_none_or(|until| validated_at_ms >= until)
    {
        return Err(SessionError::InvalidInteraction);
    }
    check_runtime(&captured.runtime, binding)?;
    let encoded_snapshot = captured.snapshot.encode().map_err(snapshot_error)?;
    // Recheck actual NOW, even if the trusted producer built this sealed data
    // earlier. A successful historical validation is not initial freshness.
    CompleteScopedSnapshot::decode_bound(
        &encoded_snapshot,
        binding,
        &captured.snapshot.universe().pin,
        &captured.snapshot.profile().pin,
        validated_at_ms,
    )
    .map_err(snapshot_error)?;
    let committed_revision = cp.snapshot.revision.next()?;
    let envelope = Envelope {
        contract: CONTRACT.into(),
        contract_version: SchemaVersion::V1,
        binding: binding.clone(),
        universe: captured.snapshot.universe().pin.clone(),
        profile: captured.snapshot.profile().pin.clone(),
        runtime: captured.runtime.clone(),
        native_context_digest: captured.native_context_digest.clone(),
        snapshot_digest: captured.snapshot.digest().clone(),
        committed_revision,
        validated_at_ms,
        snapshot: serde_json::from_slice(&encoded_snapshot).map_err(encoding)?,
    };
    let bytes = canonical(&envelope).map_err(snapshot_error)?;
    if bytes.len() > MAX_BEFORE_RECORD_BYTES {
        return Err(SessionError::LimitExceeded);
    }
    let reference = content_reference(
        &format!(
            "prepared-before-{}",
            gateway_domain::context_content_digest(&bytes).as_str()
        ),
        &bytes,
    )?;
    let retained = PreparedBeforeRef {
        binding: envelope.binding,
        universe: envelope.universe,
        profile: envelope.profile,
        runtime: envelope.runtime,
        native_context_digest: envelope.native_context_digest,
        snapshot_digest: envelope.snapshot_digest,
        record: reference.clone(),
        committed_revision,
        validated_at_ms,
    };
    Ok((
        retained,
        RecordWrite {
            reference,
            kind: RecordKind::CompleteBefore,
            bytes,
        },
    ))
}

fn load(
    repository: &impl SessionRepositoryPort,
    checkpoint: &SessionCheckpoint,
    now_ms: Option<u64>,
) -> Result<CompleteScopedSnapshot, SessionError> {
    checkpoint.validate()?;
    let expected = checkpoint
        .prepared_before
        .as_ref()
        .ok_or(SessionError::InvalidState)?;
    expected.validate_for(checkpoint)?;
    if repository.record_kind(&checkpoint.owner, &expected.record)? != RecordKind::CompleteBefore {
        return Err(SessionError::InvalidInteraction);
    }
    let bytes = repository.load_artifact(&checkpoint.owner, &expected.record)?;
    decode_before_record(expected, &bytes, now_ms.unwrap_or(expected.validated_at_ms))
}

/// Pure immutable record validation, safe while a repository transaction holds
/// its lock. The caller authenticates/selects the expected reference separately;
/// no recursive repository load or current-authority claim occurs here.
pub fn decode_before_record(
    expected: &PreparedBeforeRef,
    bytes: &[u8],
    validation_at_ms: u64,
) -> Result<CompleteScopedSnapshot, SessionError> {
    expected.binding.capture.validate()?;
    if expected.binding.phase != SnapshotPhase::Before
        || expected.binding.reservation.is_none()
        || expected.validated_at_ms == 0
        || expected.committed_revision != expected.binding.capture.revision.next()?
        || [&expected.universe, &expected.profile, &expected.runtime]
            .iter()
            .any(|p| {
                p.scope != expected.binding.capture.scope || p.contract_version != SchemaVersion::V1
            })
    {
        return Err(SessionError::InvalidInteraction);
    }
    if bytes.len() > MAX_BEFORE_RECORD_BYTES {
        return Err(SessionError::LimitExceeded);
    }
    if content_reference(expected.record.id().as_str(), bytes)? != expected.record {
        return Err(SessionError::InvalidInteraction);
    }
    let envelope: Envelope =
        serde_json::from_value(strict_value(bytes).map_err(snapshot_error)?).map_err(encoding)?;
    if envelope.contract != CONTRACT
        || envelope.contract_version != SchemaVersion::V1
        || envelope.binding != expected.binding
        || envelope.universe != expected.universe
        || envelope.profile != expected.profile
        || envelope.runtime != expected.runtime
        || envelope.native_context_digest != expected.native_context_digest
        || envelope.snapshot_digest != expected.snapshot_digest
        || envelope.committed_revision != expected.committed_revision
        || envelope.validated_at_ms != expected.validated_at_ms
        || canonical(&envelope).map_err(snapshot_error)? != bytes
    {
        return Err(SessionError::InvalidInteraction);
    }
    check_runtime(&envelope.runtime, &expected.binding)?;
    let encoded_snapshot = canonical(&envelope.snapshot).map_err(snapshot_error)?;
    let historical = CompleteScopedSnapshot::decode_bound(
        &encoded_snapshot,
        &expected.binding,
        &expected.universe,
        &expected.profile,
        expected.validated_at_ms,
    )
    .map_err(snapshot_error)?;
    if historical.digest() != &expected.snapshot_digest {
        return Err(SessionError::InvalidInteraction);
    }
    if validation_at_ms != expected.validated_at_ms {
        let now = validation_at_ms;
        if now < expected.validated_at_ms {
            return Err(SessionError::InvalidInteraction);
        }
        return CompleteScopedSnapshot::decode_bound(
            &encoded_snapshot,
            &expected.binding,
            &expected.universe,
            &expected.profile,
            now,
        )
        .map_err(snapshot_error);
    }
    Ok(historical)
}

/// Initial completion of persistence rechecks freshness against actual current
/// time. This does not replace the coordinator's final current-fence check.
pub fn load_prepared_before_current(
    repository: &impl SessionRepositoryPort,
    checkpoint: &SessionCheckpoint,
    now_ms: u64,
) -> Result<CompleteScopedSnapshot, SessionError> {
    load(repository, checkpoint, Some(now_ms))
}
/// Historical inspection authenticates the journal reference and original full
/// pins, and validates at the retained trusted time. No current freshness claim,
/// provider call, mutation, model or execution authorization is performed.
pub fn load_prepared_before_historical(
    repository: &impl SessionRepositoryPort,
    checkpoint: &SessionCheckpoint,
) -> Result<CompleteScopedSnapshot, SessionError> {
    load(repository, checkpoint, None)
}
