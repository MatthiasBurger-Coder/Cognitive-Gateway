//! #263 admission over actual immutable CG-10 producer output. No history,
//! acquisition, provider SDK, source inference or tool execution is available.
use crate::model_invocation::PreparedModelInvocation;
use crate::prompt_rendering::{
    PromptRendererPort, RenderError, RenderedInvocation, TokenizerPort, ToolDefinition,
    render_invocation,
};
use gateway_context::PlannedContextHandoff;
use gateway_domain::{prompt_ir::*, prompt_rendering::TokenizerProfile, *};
use serde::{Serialize, Serializer};
use std::collections::BTreeMap;

/// Canonical provider-independent prompt: reuses the complete typed producer,
/// retaining original task inputs, memory representations and trust metadata.
#[derive(Debug, Clone, Serialize)]
pub struct PromptIr {
    schema_version: SchemaVersion,
    bindings: PromptBindings,
    #[serde(serialize_with = "serialize_handoff")]
    handoff: PlannedContextHandoff,
    tools: BTreeMap<CapabilityId, ToolDefinition>,
}
fn serialize_handoff<S: Serializer>(h: &PlannedContextHandoff, s: S) -> Result<S::Ok, S::Error> {
    h.to_value().serialize(s)
}
impl PromptIr {
    pub fn bindings(&self) -> &PromptBindings {
        &self.bindings
    }
    pub fn handoff(&self) -> &PlannedContextHandoff {
        &self.handoff
    }
    pub fn tools(&self) -> &BTreeMap<CapabilityId, ToolDefinition> {
        &self.tools
    }
}

/// Output-only validated envelope. Neither JSON input nor an audit/redacted
/// export can construct an admitted invocation. All fields are immutable.
#[derive(Debug, Clone, Serialize)]
pub struct InvocationIr {
    schema_version: SchemaVersion,
    prompt: PromptIr,
    prepared: PreparedModelInvocation,
}
impl InvocationIr {
    pub fn prompt(&self) -> &PromptIr {
        &self.prompt
    }
    pub fn prepared(&self) -> &PreparedModelInvocation {
        &self.prepared
    }
    pub fn identity(&self) -> InvocationIrIdentity {
        InvocationIrIdentity {
            schema_version: self.schema_version,
            invocation_id: self.prepared.invocation().data().invocation_id.clone(),
            invocation_digest: self.prepared.invocation().content_digest(),
            handoff_digest: context_content_digest(
                &serde_json::to_vec(&self.prompt.handoff.to_value()).expect("handoff JSON"),
            ),
            tools_digest: context_content_digest(
                &serde_json::to_vec(&self.prompt.tools).expect("tools JSON"),
            ),
            ir_digest: context_content_digest(&serde_json::to_vec(self).expect("IR JSON")),
        }
    }
}

/// Trusted host integration port, called on admission and again before rendering.
/// Implementations MUST authenticate semantic completeness/reference admission,
/// every exact pin and source revision (including schema/rule/verifier contents),
/// source freshness/quarantine, stable catalog definitions, current Process/Policy
/// decisions, tool definitions and disclosure against trusted stores. Validate
/// task/context scope and sensitivity across the complete payload. Missing or stale
/// authority denies. Never infer authenticity from caller-supplied labels/digests.
/// Routing hosts call admission again for every prepared fallback/dispatch; the
/// existing CG-26 prepare step remains responsible for current CG-10 revalidation.
pub trait InvocationAdmissionPort: SemanticReferenceValidator {
    fn validate_current(&self, invocation: &InvocationIr) -> Result<(), InvocationIrError>;
}

pub(crate) fn validate_current<H: InvocationAdmissionPort + ?Sized>(
    ir: &InvocationIr,
    host: &H,
) -> Result<(), InvocationIrError> {
    struct References<'a, H: ?Sized>(&'a H);
    impl<H: InvocationAdmissionPort + ?Sized> SemanticReferenceValidator for References<'_, H> {
        fn validate(
            &self,
            task: &SemanticTaskData,
            binding: &SemanticReferenceBinding,
        ) -> Result<(), ValidationError> {
            self.0.validate(task, binding)
        }
    }
    ir.prompt
        .handoff
        .task()
        .validate_references(&References(host))
        .map_err(|_| InvocationIrError::InvalidBinding)?;
    host.validate_current(ir)
}

pub fn admit_invocation_ir(
    schema_version: SchemaVersion,
    handoff: PlannedContextHandoff,
    prepared: PreparedModelInvocation,
    tools: BTreeMap<CapabilityId, ToolDefinition>,
    host: &dyn InvocationAdmissionPort,
) -> Result<InvocationIr, InvocationIrError> {
    let ir = assemble_invocation_ir(schema_version, handoff, prepared, tools)?;
    validate_current(&ir, host)?;
    Ok(ir)
}

fn assemble_invocation_ir(
    schema_version: SchemaVersion,
    handoff: PlannedContextHandoff,
    prepared: PreparedModelInvocation,
    tools: BTreeMap<CapabilityId, ToolDefinition>,
) -> Result<InvocationIr, InvocationIrError> {
    use InvocationIrError as E;
    if schema_version != INVOCATION_IR_VERSION {
        return Err(E::IncompatibleVersion);
    }
    let full = handoff.to_value();
    let bindings: PromptBindings =
        serde_json::from_value(full["bindings"].clone()).map_err(|_| E::InvalidBinding)?;
    let d = prepared.invocation().data();
    if bindings.task != d.task
        || bindings.context != d.compiled_context
        || full["output"] != serde_json::to_value(&d.output).expect("output JSON")
        || tools.len() != d.permitted_capabilities.len()
        || tools
            .iter()
            .any(|(id, t)| d.permitted_capabilities.get(id) != Some(&t.binding))
        || tools.values().any(|tool| {
            tool.connector_inventory.as_ref().is_some_and(|pin| {
                pin.scope != d.task.scope
                    || serde_json::to_vec(&tool.input.json_schema())
                        .ok()
                        .map(|bytes| gateway_domain::context_content_digest(&bytes))
                        != Some(pin.argument_schema_digest.clone())
            })
        })
    {
        return Err(E::InvalidBinding);
    }
    // The producer has validated task/context/planning/output compatibility;
    // exact pins additionally reject aliases and unsupported binding versions.
    bindings.validate()?;
    let ir = InvocationIr {
        schema_version,
        prompt: PromptIr {
            schema_version,
            bindings,
            handoff,
            tools,
        },
        prepared,
    };
    Ok(ir)
}

/// Structurally reconstructed historical data. This is neither an admitted IR
/// nor proof that its source data, policy or model availability are current.
/// The storage owner must separately authenticate the original record and pins.
pub struct RetainedInvocationIr {
    ir: InvocationIr,
}
impl RetainedInvocationIr {
    pub fn identity(&self) -> InvocationIrIdentity {
        self.ir.identity()
    }
    pub fn to_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.ir).expect("native IR JSON")
    }
    /// Crate-internal data projection for canonical historical request binding.
    /// No admitted invocation or current authority escapes this wrapper.
    pub(crate) fn proposal_data(
        &self,
        capability: &CapabilityId,
    ) -> Option<(&ResolvedReference<TaskId>, &ToolDefinition)> {
        Some((
            &self.ir.prepared.invocation().data().task,
            self.ir.prompt.tools.get(capability)?,
        ))
    }
}

/// Rebuild only through the same native structural checks as live admission.
/// Expected bytes/identity must come from authenticated retained records; their
/// equality is not by itself source authenticity or current authorization.
pub fn reconstruct_retained_invocation_ir(
    schema_version: SchemaVersion,
    handoff: PlannedContextHandoff,
    prepared: PreparedModelInvocation,
    tools: BTreeMap<CapabilityId, ToolDefinition>,
    expected: &serde_json::Value,
    expected_identity: &InvocationIrIdentity,
) -> Result<RetainedInvocationIr, InvocationIrError> {
    let ir = assemble_invocation_ir(schema_version, handoff, prepared, tools)?;
    if serde_json::to_value(&ir).map_err(|_| InvocationIrError::InvalidBinding)? != *expected
        || ir.identity() != *expected_identity
    {
        return Err(InvocationIrError::InvalidBinding);
    }
    Ok(RetainedInvocationIr { ir })
}

/// This is the sole promotion from retained data to an admitted IR. The host
/// must prove actual current applicability, including budgeted fresh source
/// observations when the historical Before data has aged or the phase changed.
pub fn revalidate_retained_invocation_ir(
    retained: &RetainedInvocationIr,
    host: &dyn InvocationAdmissionPort,
) -> Result<InvocationIr, InvocationIrError> {
    validate_current(&retained.ir, host)?;
    Ok(retained.ir.clone())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "stage", content = "error", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InvocationPreparationError {
    Admission(InvocationIrError),
    Rendering(RenderError),
    Transport(gateway_domain::model_invocation::ModelContractError),
}
/// The exact IR identity accompanies the renderer's payload/template/accounting
/// record for downstream verification. Reusing an old IR still rechecks admission.
#[derive(Debug, Clone, Serialize)]
pub struct RenderedInvocationIr {
    identity: InvocationIrIdentity,
    rendered: RenderedInvocation,
}
pub fn render_invocation_ir(
    ir: &InvocationIr,
    host: &dyn InvocationAdmissionPort,
    profile: TokenizerProfile,
    renderer: &dyn PromptRendererPort,
    tokenizer: &dyn TokenizerPort,
) -> Result<RenderedInvocationIr, InvocationPreparationError> {
    validate_current(ir, host).map_err(InvocationPreparationError::Admission)?;
    let rendered = render_invocation(
        &ir.prompt.handoff,
        &ir.prepared,
        ir.prompt.tools.clone(),
        profile,
        renderer,
        tokenizer,
    )
    .map_err(InvocationPreparationError::Rendering)?;
    Ok(RenderedInvocationIr {
        identity: ir.identity(),
        rendered,
    })
}

impl RenderedInvocationIr {
    pub fn identity(&self) -> &InvocationIrIdentity {
        &self.identity
    }
    pub fn rendered(&self) -> &RenderedInvocation {
        &self.rendered
    }
}
/// Outer transport receives the complete canonical IR and its correlated payload.
/// Returns an unverified proposal; this port grants no tool/effect authority.
pub trait PromptInvocationPort {
    fn invoke(
        &self,
        ir: &InvocationIr,
        rendered: &RenderedInvocationIr,
    ) -> Result<
        crate::model_invocation::ModelProposal,
        gateway_domain::model_invocation::ModelContractError,
    >;
}
/// Revalidate immediately before transport, including after rendering or waiting.
/// Caller integrates this boundary in CG-26's model attempt, after prepare.
pub fn dispatch_invocation_ir(
    ir: &InvocationIr,
    rendered: &RenderedInvocationIr,
    host: &dyn InvocationAdmissionPort,
    transport: &dyn PromptInvocationPort,
) -> Result<crate::model_invocation::ModelProposal, InvocationPreparationError> {
    use InvocationPreparationError as E;
    if rendered.identity != ir.identity() {
        return Err(E::Admission(InvocationIrError::InvalidBinding));
    }
    validate_current(ir, host).map_err(E::Admission)?;
    let proposal = transport.invoke(ir, rendered).map_err(E::Transport)?;
    if proposal.invocation_digest != rendered.identity.invocation_digest {
        return Err(E::Admission(InvocationIrError::InvalidBinding));
    }
    Ok(proposal)
}
