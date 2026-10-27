//! Live verified proposals to coordinator-owned consent data. This module never
//! executes a connector, persists a journal transition or constructs a grant.
use super::{
    connector_source::ModelInputsCurrent, model_factory::NativeModelProducerRun,
    target_factory::ScopedTarget, *,
};
use gateway_application::{
    connector_proposals::{
        CanonicalConnectorProposalBinder, ConnectorProposalBindingPort, ConnectorSessionBinding,
        ConnectorSessionBindingPort, ProposalError,
    },
    connectors::*,
    prompt_ir::{InvocationAdmissionPort, InvocationIr},
    sessions::{before_snapshot_record, content_reference},
};
use gateway_domain::{prompt_ir::InvocationIrError, *};
use gateway_policy::{PolicyDecision, PolicyReason};

fn invalid() -> SessionError {
    SessionError::InvalidInteraction
}
fn canonical(value: impl serde::Serialize) -> Result<Vec<u8>, SessionError> {
    serde_json::to_vec(&serde_json::to_value(value).map_err(|_| invalid())?).map_err(|_| invalid())
}

/// Entry is restricted to a genuine live native run. The run checks its actual
/// opaque model child/context before and after this bounded, metadata-only work.
pub(super) fn prepare(
    run: &NativeModelProducerRun,
    target: &ScopedTarget,
    turn: &PreparedConnectorTurn,
    context: ConnectorPreparationContext<'_>,
) -> Result<ConnectorPreparation, SessionError> {
    prepare_bound(run, target, turn, context).map(|(preparation, _)| preparation)
}
/// Bound requests are immutable data for the same preparation result, never
/// dispatch grants. Keeping them avoids reconstructing a different correlation.
pub(super) fn prepare_bound(
    run: &NativeModelProducerRun,
    target: &ScopedTarget,
    turn: &PreparedConnectorTurn,
    context: ConnectorPreparationContext<'_>,
) -> Result<(ConnectorPreparation, Vec<ConnectorRequest>), SessionError> {
    run.with_current_preparation(target, turn, |view| {
        let checkpoint = view.checkpoint();
        if context.revision != checkpoint.snapshot.revision.next()?
            || checkpoint.snapshot.dispatch.as_ref() != Some(context.dispatch)
            || view.binding().dispatch != *context.dispatch
            || context.expires_at_ms == 0
            || context.expires_at_ms > checkpoint.budget.deadline_ms
            || turn.before_snapshot().scope() != &view.binding().scope
        {
            return Err(SessionError::StaleRevision);
        }
        let proposals = &turn.verified_output().output().data().tools;
        if proposals.is_empty() || proposals.len() > MAX_CONNECTOR_PROPOSALS_PER_STEP {
            return Err(invalid());
        }
        // Bind the whole ordered batch before any preview or pending record.
        let requests = bind_proposals(view, turn.invocation(), proposals)?;
        let mut needs_consent = Vec::new();
        for (ordinal, request) in requests.iter().enumerate() {
            let preview = target
                .context_source()
                .preview_in_view(view, request)
                .map_err(|_| SessionError::AuthorityDenied)?;
            match preview.report.decision {
                PolicyDecision::Allow => {}
                PolicyDecision::RequireConsent
                    if preview.report.findings.iter().all(|f| {
                        f.decision == PolicyDecision::Allow
                            || f.reason == PolicyReason::ConsentMissing
                    }) =>
                {
                    needs_consent.push((ordinal, preview))
                }
                _ => return Err(SessionError::AuthorityDenied),
            }
        }
        if needs_consent.is_empty() {
            return Ok((
                ConnectorPreparation::Ready(Box::new(turn.clone())),
                requests,
            ));
        }
        // BATCH-CONSENT-1: the app owns one accepted grant per step. No first
        // approval is represented as approval for other ordered effect instances.
        if needs_consent.len() != 1 {
            return Err(SessionError::UnsupportedCapability);
        }
        let (ordinal, preview) = needs_consent.pop().ok_or_else(invalid)?;
        let mut records = producer_records(view, turn)?;
        records.push(retained(
            &turn.invocation().identity(),
            "generation-evidence",
            run.generation_evidence()?,
            RecordKind::ConnectorProducerInput,
        )?);
        let producer_pins = records.iter().map(|r| r.reference.clone()).collect();
        let material = ConnectorConsentResumeMaterial::new(
            turn.invocation().identity(),
            u32::try_from(ordinal).map_err(|_| invalid())?,
            turn.verified_output().output().raw_bytes().to_vec(),
            producer_pins,
        )?;
        let bytes = canonical(&material)?;
        let output_record = content_reference(
            &format!("native-consent-output-{}", material.output_digest.as_str()),
            &bytes,
        )?;
        let resume = ConnectorConsentResumeBinding {
            invocation: material.invocation.clone(),
            proposal_ordinal: material.proposal_ordinal,
            output_record: output_record.clone(),
            output_digest: material.output_digest.clone(),
            producer_pins: material.producer_pins.clone(),
        };
        let binding = ConnectorConsentBinding::new(
            checkpoint.owner.clone(),
            view.binding().scope.clone(),
            checkpoint.snapshot.session.clone(),
            checkpoint.snapshot.run.clone(),
            context.revision,
            context.pending.clone(),
            view.binding().step.clone(),
            context.dispatch.clone(),
            &requests[ordinal],
            resume,
            preview.policy,
            preview.policy_revision,
            context.expires_at_ms,
            checkpoint.owner.principal.clone(),
        )?;
        records.push(RecordWrite {
            reference: output_record,
            kind: RecordKind::ConnectorProposal,
            bytes,
        });
        validate_records(&records)?;
        Ok((
            ConnectorPreparation::ConsentRequired {
                binding: Box::new(binding),
                records,
            },
            requests,
        ))
    })
}

/// Same canonical request mapping for verification, preparation and execution.
/// The view is native freshly checked data; this adapter has no transport.
pub(super) fn bind_proposals(
    view: &ModelInputsCurrent,
    ir: &InvocationIr,
    proposals: &[gateway_domain::output_verification::ToolCallProposal],
) -> Result<Vec<ConnectorRequest>, SessionError> {
    if proposals.is_empty() || proposals.len() > MAX_CONNECTOR_PROPOSALS_PER_STEP {
        return Err(invalid());
    }
    let binder = CanonicalConnectorProposalBinder::new(
        ExactInvocation { expected: ir, view },
        CurrentCoordinates { view },
        RetainedInventory { view },
    );
    proposals
        .iter()
        .enumerate()
        .map(|(ordinal, p)| {
            binder
                .bind(ir, p, ordinal)
                .map(|b| b.request)
                .map_err(|_| SessionError::AuthorityDenied)
        })
        .collect()
}

struct ExactInvocation<'a> {
    expected: &'a InvocationIr,
    view: &'a ModelInputsCurrent,
}
impl SemanticReferenceValidator for ExactInvocation<'_> {
    fn validate(
        &self,
        task: &SemanticTaskData,
        binding: &SemanticReferenceBinding,
    ) -> Result<(), ValidationError> {
        let validate = || -> Result<(), SessionError> {
            let actual = self.expected.prompt().handoff().task();
            if task != actual.data() || !actual.reference_bindings().contains(binding) {
                return Err(SessionError::AuthorityDenied);
            }
            let pin = Pin {
                id: ReferenceId::new(&binding.id).map_err(|_| invalid())?,
                scope: binding.scope.clone(),
                contract: binding.contract.clone(),
                contract_version: binding.contract_version,
                revision: binding.revision.clone(),
                digest: binding.digest.clone(),
            };
            if pin.scope != self.view.binding().scope {
                return Err(SessionError::ScopeDenied);
            }
            let value = self.view.runtime().loaded()?.document(&pin)?;
            if context_content_digest(&canonical(value)?) != pin.digest {
                return Err(SessionError::StaleRevision);
            }
            super::document_references(&pin, value)?;
            Ok(())
        };
        validate().map_err(|_| ValidationError::InvalidStateCombination {
            reason: "current retained proposal reference unavailable",
        })
    }
}
impl InvocationAdmissionPort for ExactInvocation<'_> {
    fn validate_current(&self, actual: &InvocationIr) -> Result<(), InvocationIrError> {
        if canonical(actual).map_err(|_| InvocationIrError::InvalidBinding)?
            != canonical(self.expected).map_err(|_| InvocationIrError::InvalidBinding)?
        {
            return Err(InvocationIrError::InvalidBinding);
        }
        Ok(())
    }
}
struct CurrentCoordinates<'a> {
    view: &'a ModelInputsCurrent,
}
impl ConnectorSessionBindingPort for CurrentCoordinates<'_> {
    fn current_binding(&self, ir: &InvocationIr) -> Result<ConnectorSessionBinding, ProposalError> {
        let task = &ir.prepared().invocation().data().task;
        if task.scope != self.view.binding().scope
            || &task.id
                != self
                    .view
                    .producer_context()
                    .context()
                    .execution_context()
                    .task()
                    .id()
        {
            return Err(ProposalError::StaleInvocation);
        }
        Ok(ConnectorSessionBinding {
            target: self.view.inventory().target.clone(),
            step: self.view.binding().step.clone(),
            execution: self.view.action_execution().clone(),
        })
    }
}
/// Truthful retained native discovery metadata. No transport is available.
struct RetainedInventory<'a> {
    view: &'a ModelInputsCurrent,
}
fn unavailable(correlation: NonEmptyText) -> ConnectorFailure {
    ConnectorFailure {
        code: ConnectorFailureCode::PermissionDenied,
        effect: ConnectorEffect::NotDispatched,
        correlation,
    }
}
impl ConnectorApplicationPort for RetainedInventory<'_> {
    fn discover(&self, target: &ConnectorTarget) -> Result<ConnectorInventory, ConnectorFailure> {
        if target != &self.view.inventory().target {
            return Err(unavailable(
                NonEmptyText::new("native-consent-inventory").expect("static"),
            ));
        }
        Ok(self.view.inventory().clone())
    }
    fn invoke(&self, r: &ConnectorRequest) -> Result<ConnectorObservation, ConnectorFailure> {
        Err(unavailable(r.correlation.clone()))
    }
    fn read_resource(
        &self,
        r: &ConnectorRequest,
        _: &SourceId,
    ) -> Result<ConnectorObservation, ConnectorFailure> {
        Err(unavailable(r.correlation.clone()))
    }
    fn lifecycle(
        &self,
        _: &ConnectorTarget,
        _: ConnectorLifecycleCommand,
    ) -> Result<ConnectorLifecycleState, ConnectorFailure> {
        Err(unavailable(
            NonEmptyText::new("native-consent-lifecycle").expect("static"),
        ))
    }
}

/// One bounded owner namespace used by both the writer and authenticated
/// historical reader. All actual invocation/body digest bits are retained.
pub(super) fn producer_input_reference(
    invocation: &gateway_domain::prompt_ir::InvocationIrIdentity,
    bytes: &[u8],
) -> Result<RecordRef, SessionError> {
    let digest = context_content_digest(bytes);
    let namespace = context_content_digest(&canonical((
        "cg.native-producer-record/1.0",
        &invocation.invocation_digest,
        &digest,
    ))?);
    content_reference(&format!("native-producer-{}", namespace.as_str()), bytes)
}
fn retained(
    package: &gateway_domain::prompt_ir::InvocationIrIdentity,
    role: &str,
    value: Value,
    kind: RecordKind,
) -> Result<RecordWrite, SessionError> {
    let envelope = json!({"contract":"cg.native-producer-retained","contract_version":"1.0","role":role,"value":value});
    if !gateway_application::codex::security::credential_free(&envelope) {
        return Err(SessionError::AuthorityDenied);
    }
    let bytes = canonical(envelope)?;
    if bytes.len() > MAX_CONNECTOR_RECORD_BYTES {
        return Err(SessionError::LimitExceeded);
    }
    let reference = producer_input_reference(package, &bytes)?;
    Ok(RecordWrite {
        reference,
        kind,
        bytes,
    })
}
fn producer_records(
    view: &ModelInputsCurrent,
    turn: &PreparedConnectorTurn,
) -> Result<Vec<RecordWrite>, SessionError> {
    let package = turn.invocation().identity();
    let mut records = vec![
        retained(
            &package,
            "runtime",
            json!({"pin":view.runtime().reference()?,"body":view.runtime().0}),
            RecordKind::ConnectorProducerInput,
        )?,
        retained(
            &package,
            "original-invocation",
            serde_json::to_value(turn.invocation()).map_err(|_| invalid())?,
            RecordKind::ConnectorProducerInput,
        )?,
    ];
    let resolution = gateway_application::resolution_application::DeclarativeResolutionApplication
        .serialize_resolution(view.resolved(), Default::default())
        .map_err(|_| SessionError::LimitExceeded)?;
    records.push(retained(
        &package,
        "native-resolution",
        serde_json::from_str(&resolution).map_err(|_| invalid())?,
        RecordKind::ConnectorProducerInput,
    )?);
    records.push(retained(
        &package,
        "historical-inventory",
        super::historical_inventory::encode(view.inventory())?,
        RecordKind::ConnectorProducerInput,
    )?);
    let cp = view.checkpoint();
    let binding = ConnectorStepBinding {
        owner: cp.owner.clone(),
        scope: view.binding().scope.clone(),
        session: cp.snapshot.session.clone(),
        run: cp.snapshot.run.clone(),
        revision: cp.snapshot.revision,
        task: turn
            .invocation()
            .prepared()
            .invocation()
            .data()
            .task
            .id
            .clone(),
        step: view.binding().step.clone(),
        dispatch: view.binding().dispatch.clone(),
        invocation: turn.invocation().identity(),
    };
    let bytes = before_snapshot_record(&binding, turn.before_snapshot())?;
    let reference = content_reference(
        &format!("native-original-before-{}", context_content_digest(&bytes)),
        &bytes,
    )?;
    records.push(RecordWrite {
        reference,
        kind: RecordKind::ConnectorProducerInput,
        bytes,
    });
    for document in view.runtime().loaded()?.documents.values() {
        records.push(retained(
            &package,
            "registered-runtime-document",
            json!({"pin":document.pin,"body":document.value}),
            RecordKind::ConnectorProducerInput,
        )?);
    }
    for (pin, body) in view.canonical_documents() {
        records.push(retained(
            &package,
            "canonical-owner-document",
            json!({"pin":pin,"body":body}),
            RecordKind::ConnectorProducerInput,
        )?);
    }
    validate_records(&records)?;
    Ok(records)
}
fn validate_records(records: &[RecordWrite]) -> Result<(), SessionError> {
    if records.len() > MAX_CONNECTOR_STEP_COMMIT_RECORDS
        || records
            .iter()
            .any(|r| r.bytes.len() > MAX_CONNECTOR_RECORD_BYTES)
        || records
            .iter()
            .try_fold(0usize, |n, r| n.checked_add(r.bytes.len()))
            .is_none_or(|n| n > MAX_CONNECTOR_STEP_COMMIT_BYTES)
    {
        return Err(SessionError::LimitExceeded);
    }
    Ok(())
}

#[cfg(test)]
pub(super) mod tests;
