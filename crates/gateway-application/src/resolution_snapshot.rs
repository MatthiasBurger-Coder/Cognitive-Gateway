//! CG-08.02 immutable read-only capture. No mutation port reaches resolution.

use gateway_domain::{
    ContextScopeId, DeclarativeContextSituationDocument, Delta, DesiredState, ExecutionProfile,
    OperatingMode, Plan, ReferenceId, SchemaVersion,
};
use gateway_process::{ProcessInstance, ProcessInstanceRevision, ProcessRegistry};
use gateway_registry::{CapabilityIndex, Registry};

use crate::{
    DeclarativeSituationApplication, ProcessSituationReference, ProcessSnapshotInput,
    resolution::{ContentFingerprint, RequirementAlternatives, ResolutionBasis, ResolutionRequest},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotError {
    InputUnavailable,
    UnsupportedVersion,
    ScopeMismatch,
    InvalidPlan,
    SituationMismatch,
    InvalidCatalog,
    MixedIndex,
    MissingProcessDefinition,
    InvalidProcess,
    StaleRevision,
    ProcessMismatch,
    InvalidRequest,
}

/// One owned read from the adapter. Scope labels describe provenance, not access rights.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionSnapshotInput {
    pub version: SchemaVersion,
    pub scope: ContextScopeId,
    pub plan_scope: ContextScopeId,
    pub situation_scope: ContextScopeId,
    pub plan: Plan,
    pub desired: DesiredState,
    pub delta: Delta,
    pub situation: DeclarativeContextSituationDocument,
    pub operating_mode: OperatingMode,
    pub execution_profile: ExecutionProfile,
    pub registry: Registry,
    pub index: CapabilityIndex,
    pub processes: ProcessRegistry,
    pub instance: Option<ProcessInstance>,
    pub expected_revision: Option<ProcessInstanceRevision>,
    pub situation_process: Option<ProcessSituationReference>,
    pub admission: Option<ReferenceId>,
    pub rule_version: SchemaVersion,
    pub alternatives: Vec<RequirementAlternatives>,
}

/// Implementations return a coherent owned read; the resolver calls this once.
pub trait ResolutionSnapshotPort {
    fn capture(&self) -> Result<ResolutionSnapshotInput, SnapshotError>;
}

/// Adapter for explicitly supplied in-memory snapshots, without external state.
impl ResolutionSnapshotPort for ResolutionSnapshotInput {
    fn capture(&self) -> Result<ResolutionSnapshotInput, SnapshotError> {
        Ok(self.clone())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessEvidenceAvailability {
    Present,
    AbsentOptional,
    UnavailableRequired,
}

/// The private owned input is accessible only by shared references after validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionSnapshot {
    input: ResolutionSnapshotInput,
    request: ResolutionRequest,
    process: Option<ProcessSituationReference>,
    process_availability: ProcessEvidenceAvailability,
}

impl ResolutionSnapshot {
    pub fn capture(port: &impl ResolutionSnapshotPort) -> Result<Self, SnapshotError> {
        let mut input = port.capture()?;
        if input.version != SchemaVersion::V1 || input.rule_version != SchemaVersion::V1 {
            return Err(SnapshotError::UnsupportedVersion);
        }
        if input.scope != input.plan_scope || input.scope != input.situation_scope {
            return Err(SnapshotError::ScopeMismatch);
        }
        input
            .plan
            .validate_for_resolution(&input.desired, &input.delta)
            .map_err(|_| SnapshotError::InvalidPlan)?;
        let situation = input.situation.situation();
        if input.delta.situation() != Some(situation.id())
            || input
                .situation
                .intent()
                .is_some_and(|i| i.desired_state() != &input.desired)
            || input.delta.items().iter().any(|item| {
                item.basis()
                    .situation()
                    .is_some_and(|id| id != situation.id())
                    || item.basis().current_state().is_some_and(|id| {
                        id.as_str() != input.situation.observed_state().id().as_str()
                    })
            })
        {
            return Err(SnapshotError::SituationMismatch);
        }
        let rebuilt = input
            .registry
            .capability_index()
            .map_err(|_| SnapshotError::InvalidCatalog)?;
        if input.index != rebuilt {
            return Err(SnapshotError::MixedIndex);
        }
        let process = capture_process(&input)?;
        let process_availability = if process.is_some() {
            ProcessEvidenceAvailability::Present
        } else if input
            .plan
            .steps()
            .iter()
            .any(|s| s.lifecycle_requirement().is_some())
        {
            ProcessEvidenceAvailability::UnavailableRequired
        } else {
            ProcessEvidenceAvailability::AbsentOptional
        };
        input
            .alternatives
            .sort_by(|a, b| a.step.cmp(&b.step).then(a.members.cmp(&b.members)));
        let registry_frames = input
            .registry
            .agents()
            .documents()
            .iter()
            .map(|a| a.to_json())
            .chain(
                input
                    .registry
                    .skills()
                    .documents()
                    .iter()
                    .map(|s| s.to_json()),
            )
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| SnapshotError::InvalidCatalog)?;
        let process_frames = input
            .processes
            .definitions()
            .map(|p| p.identity().digest().as_str().to_owned())
            .collect::<Vec<_>>();
        let mut context_frames = vec![
            input
                .situation
                .to_json()
                .map_err(|_| SnapshotError::SituationMismatch)?,
            input
                .desired
                .to_json()
                .map_err(|_| SnapshotError::InvalidPlan)?,
            input
                .delta
                .to_json()
                .map_err(|_| SnapshotError::InvalidPlan)?,
            input.operating_mode.to_string(),
            input.execution_profile.to_string(),
        ];
        for group in &input.alternatives {
            context_frames.push(group.step.to_string());
            context_frames.push(group.cardinality.to_string());
            context_frames.push(group.members.len().to_string());
            context_frames.extend(group.members.iter().map(ToString::to_string));
        }
        let state = process
            .as_ref()
            .map(|p| p.inspection().to_json())
            .transpose()
            .map_err(|_| SnapshotError::InvalidProcess)?;
        let basis = ResolutionBasis {
            plan: input.plan.id().clone(),
            plan_fingerprint: ContentFingerprint::of_bytes(
                input
                    .plan
                    .to_json()
                    .map_err(|_| SnapshotError::InvalidPlan)?
                    .as_bytes(),
            ),
            admission: input.admission.clone(),
            situation: situation.id().clone(),
            scope: input.scope.clone(),
            situation_fingerprint: fingerprint_frames(&context_frames),
            registry_fingerprint: fingerprint_frames(&registry_frames),
            process_catalog_fingerprint: fingerprint_frames(&process_frames),
            process_state_fingerprint: fingerprint_frames(&state.into_iter().collect::<Vec<_>>()),
            rule_version: input.rule_version,
        };
        let request = ResolutionRequest::new(input.plan.clone(), basis, input.alternatives.clone())
            .map_err(|_| SnapshotError::InvalidRequest)?;
        Ok(Self {
            input,
            request,
            process,
            process_availability,
        })
    }

    #[must_use]
    pub fn request(&self) -> &ResolutionRequest {
        &self.request
    }
    #[must_use]
    pub fn input(&self) -> &ResolutionSnapshotInput {
        &self.input
    }
    #[must_use]
    pub fn process(&self) -> Option<&ProcessSituationReference> {
        self.process.as_ref()
    }
    #[must_use]
    pub const fn process_availability(&self) -> ProcessEvidenceAvailability {
        self.process_availability
    }
    /// Equality is offline content equivalence, never a claim about current live state.
    #[must_use]
    pub fn same_basis(&self, other: &Self) -> bool {
        self.request.basis() == other.request.basis()
    }
}

fn capture_process(
    input: &ResolutionSnapshotInput,
) -> Result<Option<ProcessSituationReference>, SnapshotError> {
    let Some(instance) = &input.instance else {
        if input.situation_process.is_some() || input.expected_revision.is_some() {
            return Err(SnapshotError::ProcessMismatch);
        }
        return Ok(None);
    };
    let definition = input
        .processes
        .get(instance.definition_id(), instance.definition_version())
        .ok_or(SnapshotError::MissingProcessDefinition)?;
    if input
        .expected_revision
        .is_some_and(|revision| revision != instance.revision())
    {
        return Err(SnapshotError::StaleRevision);
    }
    let reference = DeclarativeSituationApplication::new()
        .process_reference(ProcessSnapshotInput::new(definition, instance))
        .map_err(|_| SnapshotError::InvalidProcess)?;
    if input
        .situation_process
        .as_ref()
        .is_some_and(|p| p != &reference)
    {
        return Err(SnapshotError::ProcessMismatch);
    }
    Ok(Some(reference))
}

/// Length framing prevents ambiguity even when values contain delimiters.
fn fingerprint_frames(frames: &[String]) -> ContentFingerprint {
    let mut bytes = Vec::new();
    for frame in frames {
        bytes.extend_from_slice(&(frame.len() as u64).to_be_bytes());
        bytes.extend_from_slice(frame.as_bytes());
    }
    ContentFingerprint::of_bytes(&bytes)
}
