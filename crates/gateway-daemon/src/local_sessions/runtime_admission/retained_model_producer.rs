//! Owner-bound historical producer DATA. No current admission, invocation,
//! allocation, credential resolution or connector operation is available here.
use super::{model_documents::read_model, *};
use gateway_application::{
    connectors::ConnectorInventory,
    model_invocation::prepare_model_invocation,
    output_verification::{ParsedOutput, parse_output},
    prompt_ir::{RetainedInvocationIr, reconstruct_retained_invocation_ir},
    prompt_rendering::ToolDefinition,
    sessions::content_reference,
};
use gateway_context::PlannedContextHandoff;
use gateway_domain::{model_invocation::*, output_verification::*, semantic_output::*, *};

fn invalid() -> SessionError {
    SessionError::InvalidInteraction
}
fn canonical(value: impl serde::Serialize) -> Result<Vec<u8>, SessionError> {
    serde_json::to_vec(&serde_json::to_value(value).map_err(|_| invalid())?).map_err(|_| invalid())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    contract: String,
    contract_version: SchemaVersion,
    role: String,
    value: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    pin: Pin,
    body: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Before {
    contract: String,
    contract_version: SchemaVersion,
    binding: ConnectorStepBinding,
    phase: String,
    payload: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Generation {
    contract: String,
    contract_version: SchemaVersion,
    invocation: prompt_ir::InvocationIrIdentity,
    output_digest: ContentDigest,
    verification_record: HistoricalRecord,
    status: ModelRuntimeStatus,
    prepared_status_checked_at_ms: u64,
    before_attempt_status_checked_at_ms: u64,
    verified_status_checked_at_ms: u64,
}
/// Decoded audit data, never deserialized into an application VerifiedOutput.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalRecord {
    admitted_invocation: prompt_ir::InvocationIrIdentity,
    schema_version: SchemaVersion,
    binding: VerificationBinding,
    validity: ResultValidity,
    diagnostic: Option<VerificationError>,
    critics: Vec<CheckOutcome>,
}
impl HistoricalRecord {
    fn into_record(self) -> VerificationRecord {
        VerificationRecord {
            admitted_invocation: self.admitted_invocation,
            schema_version: self.schema_version,
            binding: self.binding,
            validity: self.validity,
            diagnostic: self.diagnostic,
            critics: self.critics,
        }
    }
}

/// Construction requires actual owner-scoped journal reads. The original
/// checkpoint/resource binding remains historical and is never reminted.
pub(super) struct HistoricalProducerArtifact {
    original: Value,
    runtime: RuntimeAdmission,
    documents: BTreeMap<ReferenceId, PinnedDocument>,
    canonical_documents: Vec<(Pin, Value)>,
    resolution: Value,
    before: ScopedConnectorSnapshotPayload,
    original_binding: ConnectorStepBinding,
    generation: Generation,
    output: ParsedOutput,
    ordinal: u32,
    inventory:ConnectorInventory,
}
/// Lossless verified provenance plus native structural data, not current output
/// verification. Promotion still requires revalidate_retained_invocation_ir and
/// existing verify_output after a fresh budgeted read proves applicability.
pub(super) struct HistoricalVerifiedProducerData {
    retained: RetainedInvocationIr,
    output: ParsedOutput,
    record: VerificationRecord,
    before: ScopedConnectorSnapshotPayload,
    original_binding: ConnectorStepBinding,
    ordinal: u32,
    inventory:ConnectorInventory,
}
impl HistoricalProducerArtifact {
    pub(super) fn load(
        repository: &impl SessionRepositoryPort,
        checkpoint: &SessionCheckpoint,
        binding: &ConnectorConsentBinding,
        material: &ConnectorConsentResumeMaterial,
    ) -> Result<Self, SessionError> {
        binding.validate()?;
        material.matches(&binding.resume)?;
        let claim = repository.load(&checkpoint.owner, &checkpoint.snapshot.session)?;
        if &claim.checkpoint != checkpoint {
            return Err(SessionError::StaleRevision);
        }
        let linked = matches!(&checkpoint.pending, Some(PendingInteraction::ConnectorConsent(actual)) if actual.as_ref() == binding)
            || checkpoint
                .accepted_connector_consent
                .as_ref()
                .is_some_and(|actual| &actual.binding == binding);
        if !linked {
            return Err(SessionError::AuthorityDenied);
        }
        if checkpoint.owner != binding.owner
            || checkpoint.snapshot.session != binding.session
            || checkpoint.snapshot.run != binding.run
            || checkpoint
                .goal
                .as_connector()
                .is_none_or(|goal| goal.scope() != &binding.scope)
            || checkpoint.snapshot.dispatch.as_ref() != Some(&binding.dispatch)
            || checkpoint.snapshot.revision < binding.issued_revision
            || material.producer_pins.len() > MAX_CONNECTOR_STEP_COMMIT_RECORDS
        {
            return Err(SessionError::AuthorityDenied);
        }
        let mut roles = BTreeMap::new();
        let mut documents = BTreeMap::new();
        let mut canonical_documents = Vec::new();
        let mut before = None;
        let mut total = 0usize;
        for reference in &material.producer_pins {
            // Historical inputs are generic immutable artifacts, not active
            // checkpoint snapshots or a second proposal transition.
            if repository.record_kind(&binding.owner, reference)? != RecordKind::ConnectorProducerInput {
                return Err(invalid());
            }
            let bytes = repository.load_artifact(&binding.owner, reference)?;
            total = total
                .checked_add(bytes.len())
                .ok_or(SessionError::LimitExceeded)?;
            if bytes.len() > MAX_CONNECTOR_RECORD_BYTES || total > MAX_CONNECTOR_STEP_COMMIT_BYTES {
                return Err(SessionError::LimitExceeded);
            }
            let value = crate::local_mcp::decode::decode(&bytes).map_err(|_| invalid())?;
            if !gateway_application::codex::security::credential_free(&value) {
                return Err(SessionError::AuthorityDenied);
            }
            let prefix = match value["contract"].as_str() {
                Some("cg.connector-step-snapshot") => "native-original-before",
                Some("cg.native-producer-retained") => "native-producer",
                _ => return Err(invalid()),
            };
            let expected=if prefix=="native-producer" {
                super::consent_preparation::producer_input_reference(&material.invocation,&bytes)?
            }else{
                content_reference(&format!("{prefix}-{}",context_content_digest(&bytes)),&bytes)?
            };
            if expected!=*reference {return Err(SessionError::StaleRevision);}
            if prefix == "native-original-before" {
                if before
                    .replace(serde_json::from_value::<Before>(value).map_err(|_| invalid())?)
                    .is_some()
                {
                    return Err(invalid());
                }
                continue;
            }
            let envelope: Envelope = serde_json::from_value(value).map_err(|_| invalid())?;
            if envelope.contract != "cg.native-producer-retained"
                || envelope.contract_version != SchemaVersion::V1
            {
                return Err(invalid());
            }
            match envelope.role.as_str() {
                "registered-runtime-document" => {
                    let document = decode_document(envelope.value, &binding.scope)?;
                    if documents
                        .insert(document.pin.id.clone(), document)
                        .is_some()
                    {
                        return Err(invalid());
                    }
                }
                "canonical-owner-document" => {
                    let document = decode_document(envelope.value, &binding.scope)?;
                    if canonical_documents
                        .iter()
                        .any(|(p, _): &(Pin, Value)| p.id == document.pin.id)
                    {
                        return Err(invalid());
                    }
                    canonical_documents.push((document.pin, document.value));
                }
                "runtime" | "original-invocation" | "native-resolution" | "generation-evidence" | "historical-inventory" => {
                    if roles.insert(envelope.role, envelope.value).is_some() {
                        return Err(invalid());
                    }
                }
                _ => return Err(invalid()),
            }
        }
        let take =
            |roles: &mut BTreeMap<String, Value>, key: &str| roles.remove(key).ok_or_else(invalid);
        let runtime_doc = decode_document(take(&mut roles, "runtime")?, &binding.scope)?;
        let wire: RuntimeWire = serde_json::from_value(runtime_doc.value).map_err(|_| invalid())?;
        let runtime = RuntimeAdmission(wire, None);
        if runtime.reference()? != runtime_doc.pin
            || runtime.0.scope != binding.scope
            || runtime.0.documents.len() != documents.len()
            || runtime
                .0
                .documents
                .iter()
                .any(|d| documents.get(&d.pin.id).is_none_or(|v| v.pin != d.pin))
            || canonical_documents.len() != 3
        {
            return Err(SessionError::StaleRevision);
        }
        let contracts: BTreeSet<_> = canonical_documents
            .iter()
            .map(|(p, _)| p.contract.as_str())
            .collect();
        if contracts != BTreeSet::from(["cg.plan", "cg.composition-rules", "cg.process-snapshot"]) {
            return Err(invalid());
        }
        for document in documents.values() {
            super::document_references(&document.pin, &document.value)?;
        }
        let inventory=super::historical_inventory::decode(take(&mut roles,"historical-inventory")?,&runtime)?;
        let original = take(&mut roles, "original-invocation")?;
        // The native IR digest hashes native struct serialization, not a parsed
        // Value's sorted-key serialization. Its identity is checked by the
        // native reconstruction owner below; PG content-reference checks above
        // authenticate these actual retained bytes in the meantime.
        if original["prompt"]["handoff"]["task"]["desired_state"]
            != serde_json::to_value(checkpoint.goal.intent().desired_state())
                .map_err(|_| invalid())?
        {
            return Err(SessionError::StaleRevision);
        }
        let generation: Generation =
            serde_json::from_value(take(&mut roles, "generation-evidence")?)
                .map_err(|_| invalid())?;
        let resolution = take(&mut roles, "native-resolution")?;
        let before = before.ok_or_else(invalid)?;
        validate_before(&before, binding, material)?;
        let output = parse_output(&material.raw_output).map_err(|_| invalid())?;
        validate_generation(&generation, material, &output)?;
        let payload = ScopedConnectorSnapshotPayload::new(
            binding.scope.clone(),
            canonical(&before.payload)?,
        )?;
        if repository.load(&checkpoint.owner, &checkpoint.snapshot.session)? != claim {
            return Err(SessionError::StaleRevision);
        }
        Ok(Self {
            original,
            runtime,
            documents,
            canonical_documents,
            resolution,
            before: payload,
            original_binding: before.binding,
            generation,
            output,
            ordinal: material.proposal_ordinal,
            inventory,
        })
    }
    /// Compare data from a separately authenticated fresh capture. This does not
    /// itself establish current checkpoint, policy, freshness or effect authority.
    pub(super) fn validate_definitions(
        &self,
        runtime: &RuntimeAdmission,
        canonical_documents: &[(Pin, Value)],
    ) -> Result<(), SessionError> {
        if runtime.reference()? != self.runtime.reference()?
            || canonical_documents.len() != self.canonical_documents.len()
        {
            return Err(SessionError::StaleRevision);
        }
        let loaded = runtime.loaded()?;
        if loaded.documents.len() != self.documents.len()
            || self.documents.iter().any(|(id, d)| {
                loaded
                    .documents
                    .get(id)
                    .is_none_or(|v| v.pin != d.pin || v.value != d.value)
            })
            || self
                .canonical_documents
                .iter()
                .any(|d| !canonical_documents.contains(d))
        {
            return Err(SessionError::StaleRevision);
        }
        // Owner replays the retained artifact against the genuine current native
        // snapshot; an artifact is not a serialized Registry/Snapshot grant.
        gateway_application::resolution_artifact::parse_resolution(
            &loaded.resolved()?.snapshot,
            &loaded.resolved()?.report.rules,
            &String::from_utf8(canonical(&self.resolution)?).map_err(|_| invalid())?,
            Default::default(),
        )
        .map_err(|_| SessionError::StaleRevision)?;
        Ok(())
    }
    pub(super) fn inventory(&self)->&ConnectorInventory { &self.inventory }
    pub(super) fn original_value(&self) -> &Value {
        &self.original
    }
    pub(super) fn before(&self) -> &ScopedConnectorSnapshotPayload {
        &self.before
    }
    pub(super) fn original_binding(&self)->&ConnectorStepBinding { &self.original_binding }
    /// Handoff must be rebuilt by its native owner; full equality prevents a
    /// different source, quality, timestamp, requirement or selected fragment.
    pub(super) fn reconstruct(
        self,
        handoff: PlannedContextHandoff,
    ) -> Result<HistoricalVerifiedProducerData, SessionError> {
        let admission = self
            .runtime
            .0
            .steps
            .iter()
            .find(|s| s.step == self.original_binding.step.as_str())
            .ok_or_else(invalid)?;
        let ProducerAdmission::Model(model) = &admission.producer else {
            return Err(invalid());
        };
        let definitions = read_model(&model.model, &self.documents)?;
        let status = &self.generation.status;
        if status.provider != definitions.status.provider
            || status.observation != definitions.status.observation
            || status.availability != definitions.status.availability
            || status.availability != ModelAvailability::Available
            || [
                self.generation.prepared_status_checked_at_ms,
                self.generation.before_attempt_status_checked_at_ms,
                self.generation.verified_status_checked_at_ms,
            ]
            .into_iter()
            .any(|t| !definitions.status.in_window(t))
        {
            return Err(SessionError::AuthorityDenied);
        }
        let tools = decode_tools(self.original["prompt"]["tools"].clone())?;
        let data: ModelInvocationData =
            serde_json::from_value(self.original["prepared"]["invocation"].clone())
                .map_err(|_| invalid())?;
        let permitted = tools
            .iter()
            .map(|(id, t)| (id.clone(), t.binding.clone()))
            .collect();
        let task = handoff.task();
        let context = data.compiled_context.clone();
        let authority = data.authority.clone();
        let output_contract = data.output.clone();
        let invocation = ModelInvocation::new(
            data,
            ModelInvocationBasis {
                task,
                output: &output_contract,
                compiled_context: &context,
                permitted_capabilities: &permitted,
                sensitivity: definitions.model.sensitivity,
                permitted_localities: &definitions.model.permitted_localities,
                authority: &authority,
            },
        )
        .map_err(|_| invalid())?;
        let prepared = prepare_model_invocation(
            invocation,
            definitions.capability,
            definitions.provider,
            status,
        )
        .map_err(|_| invalid())?;
        let retained = reconstruct_retained_invocation_ir(
            SchemaVersion::V1,
            handoff,
            prepared,
            tools,
            &self.original,
            &self.generation.invocation,
        )
        .map_err(|_| invalid())?;
        Ok(HistoricalVerifiedProducerData {
            retained,
            output: self.output,
            record: self.generation.verification_record.into_record(),
            before: self.before,
            original_binding: self.original_binding,
            ordinal: self.ordinal,
            inventory:self.inventory,
        })
    }
}
impl HistoricalVerifiedProducerData {
    pub(super) fn inventory(&self)->&ConnectorInventory { &self.inventory }
    pub(super) fn retained(&self) -> &RetainedInvocationIr {
        &self.retained
    }
    pub(super) fn output(&self) -> &ParsedOutput {
        &self.output
    }
    pub(super) fn record(&self) -> &VerificationRecord {
        &self.record
    }
    pub(super) fn before(&self) -> &ScopedConnectorSnapshotPayload {
        &self.before
    }
    pub(super) fn original_binding(&self) -> &ConnectorStepBinding {
        &self.original_binding
    }
    pub(super) fn ordinal(&self) -> u32 {
        self.ordinal
    }
}
fn decode_document(value: Value, scope: &ContextScopeId) -> Result<PinnedDocument, SessionError> {
    let document: Document = serde_json::from_value(value).map_err(|_| invalid())?;
    validate_model_binding(&document.pin).map_err(|_| invalid())?;
    let canonical = canonical(&document.body)?;
    if &document.pin.scope != scope || context_content_digest(&canonical) != document.pin.digest {
        return Err(SessionError::StaleRevision);
    }
    Ok(PinnedDocument {
        pin: document.pin,
        value: document.body,
        canonical,
    })
}
fn validate_generation(
    g: &Generation,
    material: &ConnectorConsentResumeMaterial,
    output: &ParsedOutput,
) -> Result<(), SessionError> {
    let r = &g.verification_record;
    if g.contract != "cg.native-model-generation-evidence"
        || g.contract_version != SchemaVersion::V1
        || g.invocation != material.invocation
        || g.output_digest != material.output_digest
        || g.prepared_status_checked_at_ms == 0
        || g.prepared_status_checked_at_ms > g.before_attempt_status_checked_at_ms
        || g.before_attempt_status_checked_at_ms > g.verified_status_checked_at_ms
        || r.schema_version != SchemaVersion::V1
        || r.admitted_invocation != material.invocation
        || r.binding != output.binding()
        || r.validity != ResultValidity::Valid
        || r.diagnostic.is_some()
    {
        return Err(SessionError::StaleRevision);
    }
    Ok(())
}
fn validate_before(
    before: &Before,
    binding: &ConnectorConsentBinding,
    material: &ConnectorConsentResumeMaterial,
) -> Result<(), SessionError> {
    let b = &before.binding;
    if before.contract != "cg.connector-step-snapshot"
        || before.contract_version != SchemaVersion::V1
        || before.phase != "before"
        || b.owner != binding.owner
        || b.scope != binding.scope
        || b.session != binding.session
        || b.run != binding.run
        || b.task.as_str() != binding.request.task
        || b.step != binding.step
        || b.dispatch != binding.dispatch
        || b.revision.next()? != binding.issued_revision
        || b.invocation != material.invocation
        || before.payload["contract"] != "cg.native-model-complete-before"
        || before.payload["contract_version"] != "1.0"
        || before.payload["owner"] != serde_json::to_value(&binding.owner).map_err(|_| invalid())?
        || before.payload["scope"] != binding.scope.as_str()
        || before.payload["session"] != binding.session.as_str()
        || before.payload["run"] != binding.run.as_str()
    {
        return Err(SessionError::StaleRevision);
    }
    let reservation: ResourceReservationBinding =
        serde_json::from_value(before.payload["resource_binding"].clone())
            .map_err(|_| invalid())?;
    if reservation.owner != b.owner
        || reservation.scope != b.scope
        || reservation.session != b.session
        || reservation.run != b.run
        || reservation.step != b.step
        || reservation.dispatch != b.dispatch
        || reservation.revision != b.revision
    {
        return Err(SessionError::StaleRevision);
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolWire {
    binding: ResolvedReference<CapabilityId>,
    description: NonEmptyText,
    input: OutputType,
    connector_inventory: InventoryWire,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InventoryWire {
    connector: NonEmptyText,
    scope: ContextScopeId,
    revision: ContentDigest,
    argument_schema_digest: ContentDigest,
}
fn decode_tools(value: Value) -> Result<BTreeMap<CapabilityId, ToolDefinition>, SessionError> {
    let tools: BTreeMap<CapabilityId, ToolWire> =
        serde_json::from_value(value).map_err(|_| invalid())?;
    tools
        .into_iter()
        .map(|(id, t)| {
            if id != t.binding.id {
                return Err(invalid());
            }
            let definition = ToolDefinition::from_connector_schema(
                t.binding,
                t.description,
                gateway_application::connectors::ConnectorTarget {
                    connector: t.connector_inventory.connector,
                    scope: t.connector_inventory.scope,
                },
                t.connector_inventory.revision,
                t.connector_inventory.argument_schema_digest,
                &t.input.json_schema(),
            )
            .map_err(|_| invalid())?;
            Ok((id, definition))
        })
        .collect()
}

#[cfg(test)]
mod tests;
