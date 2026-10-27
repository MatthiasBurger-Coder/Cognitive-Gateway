//! Native producer composition. Captured definitions and IR are data; every
//! invocation still requires current source, model allocation and output gates.
use super::{
    connector_source::ModelInputsCurrent,
    model_documents::{NativeModelDefinitions, SatisfactionBinding, read_model},
    source_context::CapturedSourceContextMaterial,
    target_factory::ScopedTarget,
    *,
};
use gateway_application::{
    context_application::{CompileStepInput, CompiledStep, ContextApplication},
    context_planning::{
        ContextAnalysisInput, ContextPlanningHost, analyze_context_requirements,
        select_context_sources,
    },
    model_invocation::{PreparedModelInvocation, prepare_model_invocation},
    prompt_rendering::ToolDefinition,
    sessions::content_reference,
};
use gateway_context::{
    CompiledContext, ContextCompiler, ContextHandoffContracts, PlannedContextHandoff,
};
use gateway_domain::{model_invocation::*, semantic_output::*, *};
use gateway_policy::PolicyDecision;
use std::sync::Arc;

fn invalid() -> SessionError {
    SessionError::InvalidInput
}
fn canonical(value: impl serde::Serialize) -> Result<Vec<u8>, SessionError> {
    serde_json::to_vec(&serde_json::to_value(value).map_err(|_| invalid())?).map_err(|_| invalid())
}
fn artifact<I: Clone>(
    id: &str,
    typed_id: I,
    kind: &str,
    scope: &ContextScopeId,
    bytes: &[u8],
) -> Result<ResolvedReference<I>, SessionError> {
    let reference = content_reference(id, bytes)?;
    Ok(ResolvedReference {
        id: typed_id,
        scope: scope.clone(),
        contract: ReferenceId::new(kind).map_err(|_| invalid())?,
        contract_version: SchemaVersion::V1,
        revision: ReferenceId::new(reference.revision().as_str()).map_err(|_| invalid())?,
        digest: context_content_digest(bytes),
    })
}
/// Pure native assembly shared with retained-data reconstruction. Callers must
/// authenticate definitions and source provenance; this creates no current
/// invocation, availability, resource or effect permission.
pub(super) struct NativeHandoffAssembly {
    pub(super) task: SemanticTaskIR,
    pub(super) requirements: ContextRequirementsIR,
    pub(super) revision: ReferenceId,
    pub(super) context: CompiledContext,
    pub(super) expected_context: Pin,
    pub(super) sources: Vec<ContextSourceMetadata>,
    pub(super) contracts: ContextHandoffContracts,
}
pub(super) fn assemble_native_handoff(
    input: NativeHandoffAssembly,
) -> Result<PlannedContextHandoff, SessionError> {
    let plan = select_context_sources(&input.requirements, input.revision, &input.sources)
        .map_err(|_| SessionError::AuthorityDenied)?;
    ContextCompiler::handoff_planned_context(
        input.task,
        input.requirements,
        plan,
        input.context,
        input.expected_context,
        &[],
        input.contracts,
    )
    .map_err(|_| SessionError::AuthorityDenied)
}

/// Genuine native captured artifacts. The existing content-reference owner
/// supplies their revisions; no native Plan revision is guessed. Complete bytes
/// accompany the pins and must enter the coordinator's Before record.
struct NativeBasisArtifacts {
    basis: ContextPlanningBasis,
    plan: Vec<u8>,
    situation: Vec<u8>,
}
impl NativeBasisArtifacts {
    fn capture(
        resolved: &gateway_application::resolution_application::ResolvedPlan,
        task: ResolvedReference<TaskId>,
    ) -> Result<Self, SessionError> {
        gateway_application::resolution_application::DeclarativeResolutionApplication
            .validate_resolution(resolved)
            .map_err(|_| invalid())?;
        let input = resolved.snapshot.input();
        let scope = &task.scope;
        if scope != &input.scope {
            return Err(SessionError::ScopeDenied);
        }
        let plan = canonical(&input.plan)?;
        let situation = canonical(input.situation.situation())?;
        let plan_pin = artifact(
            input.plan.id().as_str(),
            input.plan.id().clone(),
            "cg.captured-native-plan",
            scope,
            &plan,
        )?;
        let situation_pin = artifact(
            input.situation.situation().id().as_str(),
            input.situation.situation().id().clone(),
            "cg.captured-native-situation",
            scope,
            &situation,
        )?;
        Ok(Self {
            basis: ContextPlanningBasis {
                task,
                situation: Some(situation_pin),
                plan: Some(plan_pin),
            },
            plan,
            situation,
        })
    }
    fn payload(&self) -> Result<Value, SessionError> {
        Ok(
            json!({"contract":"cg.captured-native-model-basis","contract_version":"1.0","storage":"CAPTURED_IN_MEMORY","basis":self.basis,"native_plan":serde_json::from_slice::<Value>(&self.plan).map_err(|_|invalid())?,"native_situation":serde_json::from_slice::<Value>(&self.situation).map_err(|_|invalid())?}),
        )
    }
}

/// Preparation assembled only from the same currently authenticated source and
/// the coordinator's genuine model allocation. There is no deserialize path.
pub(super) struct NativeModelProducerPlan {
    access: Arc<LocalSessionAccess>,
    checkpoint: SessionCheckpoint,
    runtime: RuntimeAdmission,
    admission: ModelAdmission,
    definitions: NativeModelDefinitions,
    resolved: gateway_application::resolution_application::ResolvedPlan,
    producer: CompiledStep,
    basis: NativeBasisArtifacts,
    task: SemanticTaskIR,
    requirements: ContextRequirementsIR,
    output: OutputContract,
    verification: VerificationContract,
    tools: BTreeMap<CapabilityId, ToolDefinition>,
    model_reservation: ContentDigest,
    origin_binding: ResourceReservationBinding,
}
impl NativeModelProducerPlan {
    pub(super) fn capture(
        target: &ScopedTarget,
        checkpoint: &SessionCheckpoint,
        allocation: &SessionResourceAllocation,
    ) -> Result<Self, SessionError> {
        let access = target.context_source().model_access();
        if allocation.phase() != SessionResourcePhase::Preparation
            || allocation.amounts().model_attempts == 0
            || allocation.binding() != target.context_source().context_resource_binding()
        {
            return Err(SessionError::AuthorityDenied);
        }
        access.validate_allocation(allocation)?;
        let source = target.context_source();
        // One fresh native source snapshot supplies all capture inputs. The
        // original model child is independently checked by the resource owner.
        let view = source.model_inputs_current()?;
        if view.checkpoint() != checkpoint || view.binding() != allocation.binding() {
            return Err(SessionError::StaleRevision);
        }
        let resolved = view.resolved().clone();
        let runtime = view.runtime().clone();
        let step = runtime
            .0
            .steps
            .iter()
            .find(|step| step.step == allocation.binding().step.as_str())
            .ok_or(SessionError::ScopeDenied)?;
        let ProducerAdmission::Model(admission) = &step.producer else {
            return Err(SessionError::UnsupportedCapability);
        };
        let admission = (**admission).clone();
        let loaded = runtime.loaded()?;
        let definitions = read_model(&admission.model, &loaded.documents)?;
        if definitions.model.task != admission.task
            || definitions.model.analysis != admission.context_analysis
            || definitions.model.sources != admission.source_metadata
            || definitions.adapter.pricing.cost_unit != *allocation.cost_unit()
        {
            return Err(SessionError::StaleRevision);
        }
        let task: SemanticTaskIR =
            serde_json::from_value(loaded.document(&admission.task)?.clone())
                .map_err(|_| invalid())?;
        definitions
            .model
            .validate_task_alias(&admission.task, &task)?;
        if task.data().desired_state.as_ref() != Some(&resolved.snapshot.input().desired)
            || &resolved.snapshot.input().desired != checkpoint.goal.intent().desired_state()
            || definitions.analysis.maximum_sensitivity > definitions.model.sensitivity
        {
            return Err(SessionError::StaleRevision);
        }
        let output: OutputContract =
            serde_json::from_value(loaded.document(&admission.output)?.clone())
                .map_err(|_| invalid())?;
        let verification: VerificationContract =
            serde_json::from_value(loaded.document(&admission.verification)?.clone())
                .map_err(|_| invalid())?;
        validate_contracts(&task, &output, &verification).map_err(|_| invalid())?;
        validate_verifiers(&runtime, &verification)?;
        if loaded.document(&output.schema)? != &output.shape.json_schema() {
            return Err(SessionError::StaleRevision);
        }
        if !matches!(output.shape, OutputType::String {}) {
            return Err(SessionError::UnsupportedCapability);
        }
        let now = access.now_ms()?;
        if !definitions.status.in_window(now)
            || definitions.status.availability != ModelAvailability::Available
        {
            return Err(SessionError::Unavailable);
        }
        let producer = view.producer_context().clone();
        if producer.basis() != &resolved.report.basis
            || producer.policy().decision != PolicyDecision::Allow
        {
            return Err(SessionError::AuthorityDenied);
        }
        let basis =
            NativeBasisArtifacts::capture(&resolved, definitions.model.invocation_task.clone())?;
        let planning = gateway_domain::derive_planning_inputs(
            &resolved.snapshot.input().delta,
            task.data().desired_state.as_ref().ok_or_else(invalid)?,
            &Default::default(),
        )
        .map_err(|_| invalid())?;
        let host = PlanningAdmission {
            task: &task,
            runtime: &runtime,
            resolved: &resolved,
            basis: &basis.basis,
        };
        let requirements = analyze_context_requirements(
            &task,
            ContextAnalysisInput {
                revision: admission.context_analysis.revision.clone(),
                basis: basis.basis.clone(),
                additional: definitions.analysis.additional()?,
                planning_inputs: planning,
                budget_per_requirement: definitions.analysis.budget_per_requirement.native()?,
                total_tokens: TokenBudget(definitions.analysis.total_tokens),
                maximum_sensitivity: definitions.analysis.maximum_sensitivity,
            },
            &host,
        )
        .map_err(|_| SessionError::AuthorityDenied)?;
        let tools = view.proposal_tools().clone();
        if tools.is_empty() {
            return Err(SessionError::UnsupportedCapability);
        }
        drop(view);
        source.validate_producer_current()?;
        Ok(Self {
            access,
            checkpoint: checkpoint.clone(),
            runtime,
            admission,
            definitions,
            resolved,
            producer,
            basis,
            task,
            requirements,
            output,
            verification,
            tools,
            model_reservation: allocation.reservation_id().clone(),
            origin_binding: allocation.binding().clone(),
        })
    }
    pub(super) fn task(&self) -> &SemanticTaskIR {
        &self.task
    }
    pub(super) fn requirements(&self) -> &ContextRequirementsIR {
        &self.requirements
    }
    pub(super) fn source_routes(&self) -> &[super::model_documents::SourceBindingDocument] {
        &self.definitions.sources
    }
}

struct PlanningAdmission<'a> {
    task: &'a SemanticTaskIR,
    runtime: &'a RuntimeAdmission,
    resolved: &'a gateway_application::resolution_application::ResolvedPlan,
    basis: &'a ContextPlanningBasis,
}
impl SemanticReferenceValidator for PlanningAdmission<'_> {
    fn validate(
        &self,
        task: &SemanticTaskData,
        binding: &SemanticReferenceBinding,
    ) -> Result<(), ValidationError> {
        let result = (|| -> Result<(), SessionError> {
            if task != self.task.data() || !self.task.reference_bindings().contains(binding) {
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
            let body = self.runtime.loaded()?.document(&pin)?;
            let bytes = canonical(body)?;
            if context_content_digest(&bytes) != pin.digest {
                return Err(SessionError::StaleRevision);
            }
            if pin.scope != self.runtime.0.scope {
                return Err(SessionError::ScopeDenied);
            }
            super::document_references(&pin, body)?;
            Ok(())
        })();
        result.map_err(|_| ValidationError::InvalidStateCombination {
            reason: "current native model reference unavailable",
        })
    }
}
impl ContextPlanningHost for PlanningAdmission<'_> {
    fn validate_context_basis(
        &self,
        task: &SemanticTaskIR,
        input: &ContextAnalysisInput,
    ) -> Result<(), ValidationError> {
        let result = (|| -> Result<(), SessionError> {
            if task != self.task || &input.basis != self.basis {
                return Err(SessionError::StaleRevision);
            }
            let actual = NativeBasisArtifacts::capture(self.resolved, input.basis.task.clone())?;
            if actual.basis != input.basis {
                return Err(SessionError::StaleRevision);
            }
            Ok(())
        })();
        result.map_err(|_| ValidationError::InvalidStateCombination {
            reason: "native model context basis mismatch",
        })
    }
}

fn system_quality(trust: TrustClass, sensitivity: SensitivityClass) -> QualityMetadata {
    QualityMetadata::new(
        trust,
        sensitivity,
        Confidence::NotApplicable,
        FreshnessStatus::Fresh,
        Uncertainty::None,
    )
}
struct PresentSourceIdentity {
    id: &'static str,
    contract: &'static str,
    covers: &'static str,
}
fn present_source(
    identity: PresentSourceIdentity,
    scope: &ContextScopeId,
    bytes: &[u8],
    class: ContextSourceClass,
    quality: QualityMetadata,
    present: ContextSatisfaction,
) -> Result<ContextSourceMetadata, SessionError> {
    let id = ReferenceId::new(identity.id).map_err(|_| invalid())?;
    Ok(ContextSourceMetadata {
        pin: artifact(id.as_str(), id.clone(), identity.contract, scope, bytes)?,
        payload_digest: Some(context_content_digest(bytes)),
        class,
        covers: BTreeSet::from([ReferenceId::new(identity.covers).map_err(|_| invalid())?]),
        quality,
        evidence: BTreeSet::new(),
        provenances: BTreeSet::new(),
        tokens: bytes.len() as u64,
        available: true,
        present: Some(present),
    })
}
impl NativeModelProducerPlan {
    /// Pure derivation from ONE newly read, sealed Source view. The caller never
    /// retains this view or reuses it for a later authority check.
    fn compile(
        &self,
        view: &ModelInputsCurrent,
        materials: &[CapturedSourceContextMaterial],
    ) -> Result<CompiledStep, SessionError> {
        if view.checkpoint() != &self.checkpoint
            || view.binding() != &self.origin_binding
            || view.runtime().reference()? != self.runtime.reference()?
            || view.resolved() != &self.resolved
            || view.proposal_tools() != &self.tools
            || view.producer_context().to_json().map_err(|_| invalid())?
                != self.producer.to_json().map_err(|_| invalid())?
            || &view.resolved().snapshot.input().desired
                != self.checkpoint.goal.intent().desired_state()
        {
            return Err(SessionError::StaleRevision);
        }
        let mut command = crate::declarative_cli::host_mapping::compile(
            view.resolved().clone(),
            view.canonical_policy().clone(),
            view.producer_projection().clone(),
        )
        .map_err(error)?;
        for facts in command.policy_context.steps.values_mut() {
            facts.consents.clear();
        }
        for material in materials {
            material.validate_against_view(view, &self.checkpoint)?;
            for fragment in material.fragments() {
                command.candidates.push(fragment.clone());
                command.selected.insert(fragment.id().clone());
            }
        }
        let compiled = ContextApplication
            .compile_step(CompileStepInput {
                resolved: &command.resolved,
                authority: &command.authority,
                policy_context: &command.policy_context,
                catalog: &command.catalog,
                projection: &command.projection,
                candidates: &command.candidates,
                selected: &command.selected,
            })
            .map_err(|failure| {
                #[cfg(test)]
                eprintln!(
                    "native context compile failure class={:?}",
                    std::mem::discriminant(&failure)
                );
                #[cfg(not(test))]
                let _ = failure;
                SessionError::AuthorityDenied
            })?;
        if compiled
            .context()
            .fragments()
            .iter()
            .any(|f| f.metadata().quality.sensitivity() > self.definitions.model.sensitivity)
        {
            return Err(SessionError::AuthorityDenied);
        }
        if compiled.original_input() != self.checkpoint.goal.intent().original_input() {
            return Err(SessionError::StaleRevision);
        }
        // Preserve original caller input in the exact registered semantic Task;
        // no model-side rewritten task or substituted smaller goal is admitted.
        if let Some(original) = compiled.original_input() {
            let represented = self.task.data().inputs.iter().any(|input| {
                input.role.as_str() == "original-input"
                    && match (&input.value, original) {
                        (
                            SemanticInputValue::Literal(TypedValue::String(actual)),
                            OriginalInput::Inline(expected),
                        ) => actual.as_str() == expected.as_str(),
                        (
                            SemanticInputValue::Reference(actual),
                            OriginalInput::Reference(expected),
                        ) => &actual.id == expected,
                        _ => false,
                    }
            });
            if !represented {
                return Err(SessionError::UnsupportedCapability);
            }
        }
        // Pure compilation can outlive a source's age window. Check the same
        // immutable material against this same call's view again; never refresh
        // its observation timestamp or perform a second source lookup here.
        for material in materials {
            material.validate_against_view(view, &self.checkpoint)?;
        }
        Ok(compiled)
    }
    pub(super) fn acquire(
        self,
        target: &mut ScopedTarget,
    ) -> Result<AcquiredNativeModelPlan, SessionError> {
        let expected: BTreeSet<_> = match &self.runtime.0.snapshots {
            SnapshotAdmission::AtomicCompleteRead { read, .. } => {
                BTreeSet::from([read.mapping_reference.as_str()])
            }
            SnapshotAdmission::VersionedCompleteSet { reads, .. } => reads
                .iter()
                .map(|read| read.mapping_reference.as_str())
                .collect(),
        };
        let actual: Vec<_> = self
            .definitions
            .sources
            .iter()
            .filter_map(|route| {
                if let SatisfactionBinding::BeforeRead {
                    mapping_reference, ..
                } = &route.satisfaction
                {
                    Some(mapping_reference.as_str())
                } else {
                    None
                }
            })
            .collect();
        if actual.len() != expected.len()
            || actual.iter().copied().collect::<BTreeSet<_>>() != expected
        {
            return Err(SessionError::UnsupportedCapability);
        }
        let mut materials = Vec::new();
        for route in &self.definitions.sources {
            match &route.satisfaction {
                SatisfactionBinding::BeforeRead { .. } => {
                    materials.push(CapturedSourceContextMaterial::acquire(
                        target,
                        &self.checkpoint,
                        &self.task,
                        &self.requirements,
                        route,
                    )?)
                }
                SatisfactionBinding::Catalog { projection }
                    if projection == &self.admission.projection => {}
                SatisfactionBinding::Task { task } if task == &self.admission.task => {}
                SatisfactionBinding::OutputContract {
                    output,
                    verification,
                } if output == &self.admission.output
                    && verification == &self.admission.verification => {}
                // Unqualified opaque static fragments cannot become current data.
                _ => return Err(SessionError::UnsupportedCapability),
            }
        }
        let members=materials.iter().map(|material|{
            let observation=material.observation();
            Ok(gateway_application::retained_observations::RetainedObservationMember{
                binding:context_content_digest(material.payload()),
                version_domain:context_content_digest(&canonical(json!({"scope":observation.target.scope,"connector":observation.target.connector,"source":observation.source,"kind":observation.metadata.source_kind.to_string(),"object":observation.metadata.object_reference}))?),
                batch:material.batch().clone(),
            })
        }).collect::<Result<Vec<_>,SessionError>>()?;
        gateway_application::retained_observations::RetainedObservationReadset::new(
            self.runtime.0.scope.clone(),
            members,
        )
        .map_err(|_| SessionError::InvalidInteraction)?;
        #[cfg(test)]
        eprintln!("native factory acquired actual materials; compiling context");
        let view = target.context_source().model_inputs_current()?;
        let compiled = self.compile(&view, &materials)?;
        drop(view);
        #[cfg(test)]
        eprintln!("native factory actual context compiled; selecting sources");
        let scope = &self.runtime.0.scope;
        let compiled_json: Value =
            serde_json::from_str(&compiled.context().to_json().map_err(|_| invalid())?)
                .map_err(|_| invalid())?;
        let mut sources = vec![
            present_source(
                PresentSourceIdentity {
                    id: "native-model-catalog",
                    contract: "cg.captured-native-catalog",
                    covers: "authority",
                },
                scope,
                &canonical(&compiled_json["stable"])?,
                ContextSourceClass::CatalogAuthority,
                system_quality(
                    TrustClass::CanonicalReference,
                    self.definitions.model.sensitivity,
                ),
                ContextSatisfaction::Catalog,
            )?,
            present_source(
                PresentSourceIdentity {
                    id: "native-model-task",
                    contract: "cg.captured-native-task",
                    covers: "task-input",
                },
                scope,
                &canonical(&self.task)?,
                ContextSourceClass::CurrentInput,
                system_quality(TrustClass::CallerInput, self.definitions.model.sensitivity),
                ContextSatisfaction::Task,
            )?,
            present_source(
                PresentSourceIdentity {
                    id: "native-model-output",
                    contract: "cg.captured-native-output",
                    covers: "output-verification",
                },
                scope,
                &canonical(json!({"output":self.output,"verification":self.verification}))?,
                ContextSourceClass::TaskState,
                system_quality(TrustClass::CallerInput, self.definitions.model.sensitivity),
                ContextSatisfaction::OutputContract,
            )?,
        ];
        for material in &materials {
            sources.extend_from_slice(material.sources());
        }
        let compiled_bytes = canonical(&compiled_json)?;
        let context_pin = artifact(
            "compiled-context",
            ReferenceId::new("compiled-context").map_err(|_| invalid())?,
            "compiled-context",
            scope,
            &compiled_bytes,
        )?;
        let handoff = assemble_native_handoff(NativeHandoffAssembly {
            task: self.task.clone(),
            requirements: self.requirements.clone(),
            revision: self.admission.context_analysis.revision.clone(),
            context: compiled.context().clone(),
            expected_context: context_pin.clone(),
            sources,
            contracts: ContextHandoffContracts {
                output: self.output.clone(),
                verification: self.verification.clone(),
                verification_revision: self.admission.verification.revision.clone(),
            },
        })?;
        if !gateway_application::codex::security::credential_free(&handoff.to_value()) {
            return Err(SessionError::AuthorityDenied);
        }
        #[cfg(test)]
        eprintln!("native factory actual handoff built; preparing model");
        let authority_bytes = canonical(
            json!({"domain":"cg.native-model-producer-authority","runtime":self.runtime.reference()?,"basis":self.basis.payload()?,"producer":compiled.to_json().map_err(|_|invalid())?}),
        )?;
        let authority = artifact(
            "native-model-authority",
            ReferenceId::new("native-model-authority").map_err(|_| invalid())?,
            "cg.captured-native-model-authority",
            scope,
            &authority_bytes,
        )?;
        let invocation_id=ReferenceId::new(format!("model-{}",context_content_digest(&canonical(json!({"owner":self.checkpoint.owner,"scope":scope,"session":self.checkpoint.snapshot.session,"run":self.checkpoint.snapshot.run,"step":target.context_source().context_resource_binding().step.as_str(),"dispatch":target.context_source().context_resource_binding().dispatch,"reservation_revision":target.context_source().context_resource_binding().revision}))?))).map_err(|_|invalid())?;
        let permitted_capabilities = self
            .tools
            .iter()
            .map(|(id, tool)| (id.clone(), tool.binding.clone()))
            .collect();
        let invocation = ModelInvocation::new(
            ModelInvocationData {
                schema_version: SchemaVersion::V1,
                invocation_id,
                task: self.definitions.model.invocation_task.clone(),
                compiled_context: context_pin.clone(),
                capability: self.definitions.capability.data().identity.clone(),
                task_class: self.definitions.model.task_class,
                input_contract: self.definitions.model.input_contract.clone(),
                output: self.output.clone(),
                strategy: self.definitions.model.strategy.clone(),
                permitted_capabilities,
                generation: self.definitions.generation.clone(),
                input_tokens: self.definitions.model.input_tokens,
                timeout_ms: self.definitions.model.timeout_ms,
                max_attempts: self.definitions.model.max_attempts,
                sensitivity: self.definitions.model.sensitivity,
                permitted_localities: self.definitions.model.permitted_localities.clone(),
                authority: authority.clone(),
                provenance: self.admission.model.clone(),
            },
            ModelInvocationBasis {
                task: &self.task,
                output: &self.output,
                compiled_context: &context_pin,
                permitted_capabilities: &self
                    .tools
                    .iter()
                    .map(|(id, tool)| (id.clone(), tool.binding.clone()))
                    .collect(),
                sensitivity: self.definitions.model.sensitivity,
                permitted_localities: &self.definitions.model.permitted_localities,
                authority: &authority,
            },
        )
        .map_err(|_| SessionError::AuthorityDenied)?;
        let prepared_status_checked_at_ms = self.access.now_ms()?;
        if !self
            .definitions
            .status
            .in_window(prepared_status_checked_at_ms)
            || self.definitions.status.availability != ModelAvailability::Available
        {
            return Err(SessionError::Unavailable);
        }
        let status = ModelRuntimeStatus {
            provider: self.definitions.status.provider.clone(),
            availability: self.definitions.status.availability,
            observation: self.definitions.status.observation.clone(),
        };
        let prepared = prepare_model_invocation(
            invocation,
            self.definitions.capability.clone(),
            self.definitions.provider.clone(),
            &status,
        )
        .map_err(|_| SessionError::AuthorityDenied)?;
        let before = ScopedConnectorSnapshotPayload::new(
            scope.clone(),
            canonical(
                json!({"contract":"cg.native-model-complete-before","contract_version":"1.0","owner":self.checkpoint.owner,"scope":scope,"session":self.checkpoint.snapshot.session,"run":self.checkpoint.snapshot.run,"resource_binding":target.context_source().context_resource_binding(),"native_basis":self.basis.payload()?,"sources":materials.iter().map(|m|serde_json::from_slice::<Value>(m.payload()).map_err(|_|invalid())).collect::<Result<Vec<_>,_>>()?}),
            )?,
        )?;
        Ok(AcquiredNativeModelPlan {
            plan: self,
            materials,
            compiled,
            handoff,
            prepared,
            prepared_status: status,
            prepared_status_checked_at_ms,
            before,
        })
    }
}
pub(super) struct AcquiredNativeModelPlan {
    plan: NativeModelProducerPlan,
    materials: Vec<CapturedSourceContextMaterial>,
    compiled: CompiledStep,
    handoff: PlannedContextHandoff,
    prepared: PreparedModelInvocation,
    prepared_status: ModelRuntimeStatus,
    prepared_status_checked_at_ms: u64,
    before: ScopedConnectorSnapshotPayload,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeCheckDefinition {
    schema_version: SchemaVersion,
    profile: NativeCheckProfile,
    check: SemanticVerificationCheck,
}
#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum NativeCheckProfile {
    NativeModelProposalRuleV1,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeVerifierDefinition {
    schema_version: SchemaVersion,
    profile: NativeVerifierProfile,
}
#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum NativeVerifierProfile {
    NativeModelProposalVerifierV1,
}
fn validate_verifiers(
    runtime: &RuntimeAdmission,
    contract: &VerificationContract,
) -> Result<(), SessionError> {
    let loaded = runtime.loaded()?;
    for rule in &contract.rules {
        if rule.rule.contract.as_str() != "cg.verification-rule"
            || rule.verifier.contract.as_str() != "cg.verifier"
            || !matches!(
                rule.check,
                SemanticVerificationCheck::OutputSchemaValid
                    | SemanticVerificationCheck::NoUnresolvedReference
            )
        {
            return Err(SessionError::UnsupportedCapability);
        }
        let check: NativeCheckDefinition =
            serde_json::from_value(loaded.document(&rule.rule)?.clone()).map_err(|_| invalid())?;
        let verifier: NativeVerifierDefinition =
            serde_json::from_value(loaded.document(&rule.verifier)?.clone())
                .map_err(|_| invalid())?;
        if check.schema_version != SchemaVersion::V1
            || verifier.schema_version != SchemaVersion::V1
            || check.check != rule.check
        {
            return Err(SessionError::StaleRevision);
        }
        match (check.profile, verifier.profile) {
            (
                NativeCheckProfile::NativeModelProposalRuleV1,
                NativeVerifierProfile::NativeModelProposalVerifierV1,
            ) => {}
        }
    }
    Ok(())
}
struct CurrentModelHost<'a> {
    acquired: &'a AcquiredNativeModelPlan,
    target: &'a ScopedTarget,
}
impl CurrentModelHost<'_> {
    fn current(&self) -> Result<(), SessionError> {
        let plan = &self.acquired.plan;
        if !Arc::ptr_eq(&plan.access, &self.target.context_source().model_access())
            || self.target.context_source().runtime().reference()? != plan.runtime.reference()?
        {
            return Err(SessionError::StaleRevision);
        }
        let view = self.target.context_source().model_inputs_current()?;
        let compiled = plan.compile(&view, &self.acquired.materials)?;
        // A view is data for this call only; the next boundary reads anew.
        drop(view);
        if !plan.definitions.status.in_window(plan.access.now_ms()?)
            || plan.definitions.status.availability != ModelAvailability::Available
        {
            return Err(SessionError::Unavailable);
        }
        if compiled.to_json().map_err(|_| invalid())?
            != self.acquired.compiled.to_json().map_err(|_| invalid())?
        {
            return Err(SessionError::StaleRevision);
        }
        validate_verifiers(&plan.runtime, &plan.verification)?;
        Ok(())
    }
}
impl SemanticReferenceValidator for CurrentModelHost<'_> {
    fn validate(
        &self,
        task: &SemanticTaskData,
        binding: &SemanticReferenceBinding,
    ) -> Result<(), ValidationError> {
        let plan = &self.acquired.plan;
        PlanningAdmission {
            task: &plan.task,
            runtime: &plan.runtime,
            resolved: &plan.resolved,
            basis: &plan.basis.basis,
        }
        .validate(task, binding)
    }
}
impl gateway_application::prompt_ir::InvocationAdmissionPort for CurrentModelHost<'_> {
    fn validate_current(
        &self,
        ir: &gateway_application::prompt_ir::InvocationIr,
    ) -> Result<(), gateway_domain::prompt_ir::InvocationIrError> {
        self.current()
            .map_err(|_| gateway_domain::prompt_ir::InvocationIrError::StaleInput)?;
        if ir.prompt().handoff().to_value() != self.acquired.handoff.to_value()
            || canonical(ir.prepared()).ok() != canonical(&self.acquired.prepared).ok()
            || ir.prompt().tools() != &self.acquired.plan.tools
        {
            return Err(gateway_domain::prompt_ir::InvocationIrError::InvalidBinding);
        }
        Ok(())
    }
}
impl gateway_application::output_verification::OutputVerificationPort for CurrentModelHost<'_> {
    fn resolve_reference(
        &self,
        _: &gateway_application::prompt_ir::InvocationIr,
        reference: &gateway_domain::output_verification::OutputReference,
    ) -> Result<(), gateway_domain::output_verification::VerificationError> {
        let known = self
            .acquired
            .plan
            .task
            .reference_bindings()
            .iter()
            .any(|b| {
                b.id == reference.binding.id.as_str()
                    && b.scope == reference.binding.scope
                    && b.contract == reference.binding.contract
                    && b.contract_version == reference.binding.contract_version
                    && b.revision == reference.binding.revision
                    && b.digest == reference.binding.digest
            });
        if !known
            || self
                .acquired
                .plan
                .runtime
                .loaded()
                .and_then(|l| l.document(&reference.binding))
                .is_err()
        {
            return Err(
                gateway_domain::output_verification::VerificationError::UnsupportedReference,
            );
        }
        Ok(())
    }
    fn evaluate(
        &self,
        ir: &gateway_application::prompt_ir::InvocationIr,
        output: &gateway_application::output_verification::ParsedOutput,
    ) -> Result<
        gateway_application::output_verification::ConstraintReport,
        gateway_domain::output_verification::VerificationError,
    > {
        use gateway_domain::output_verification::VerificationError as E;
        gateway_application::prompt_ir::InvocationAdmissionPort::validate_current(self, ir)
            .map_err(|_| E::AuthorityDenied)?;
        gateway_domain::output_verification::validate_output_proposal(
            &ir.identity(),
            &self.acquired.plan.output,
            output.data(),
        )?;
        if !gateway_application::codex::security::credential_free(
            &serde_json::to_value(output.data()).map_err(|_| E::VerificationUnavailable)?,
        ) {
            return Err(E::ConstraintViolation);
        }
        let view = self
            .target
            .context_source()
            .model_inputs_current()
            .map_err(|_| E::AuthorityDenied)?;
        let requests = super::consent_preparation::bind_proposals(&view, ir, &output.data().tools)
            .map_err(|_| E::InvalidToolProposal)?;
        for request in &requests {
            let preview = self
                .target
                .context_source()
                .preview_in_view(&view, request)
                .map_err(|_| E::AuthorityDenied)?;
            if preview.report.findings.iter().any(|finding| {
                finding.decision != PolicyDecision::Allow
                    && finding.reason != gateway_policy::PolicyReason::ConsentMissing
            }) {
                return Err(E::AuthorityDenied);
            }
        }
        drop(view);
        let mut checks = BTreeMap::new();
        for rule in &self.acquired.plan.verification.rules {
            match rule.check {
                SemanticVerificationCheck::OutputSchemaValid => {}
                SemanticVerificationCheck::NoUnresolvedReference => {
                    for reference in &output.data().references {
                        self.resolve_reference(ir, reference)?;
                    }
                    checks.insert(rule.check, CheckOutcome::Passed);
                }
                _ => return Err(E::VerificationUnavailable),
            }
        }
        self.current().map_err(|_| E::AuthorityDenied)?;
        Ok(gateway_application::output_verification::ConstraintReport {
            binding: output.binding(),
            policy: CheckOutcome::Passed,
            process: CheckOutcome::Passed,
            task_completeness: CheckOutcome::Passed,
            semantic_output: CheckOutcome::Passed,
            checks,
        })
    }
}

struct WireCounter<'a> {
    profile: gateway_domain::prompt_rendering::TokenizerProfile,
    provenance: Pin,
    handoff: &'a PlannedContextHandoff,
    tools: &'a BTreeMap<CapabilityId, ToolDefinition>,
}
impl gateway_application::prompt_rendering::TokenizerPort for WireCounter<'_> {
    fn count(
        &self,
        request: &gateway_application::prompt_rendering::TokenCountRequest<'_>,
    ) -> Result<
        gateway_domain::prompt_rendering::TokenCountResult,
        gateway_domain::prompt_rendering::RenderDiagnostic,
    > {
        use gateway_domain::prompt_rendering::*;
        if request.profile != &self.profile
            || request.payload_digest != &context_content_digest(request.payload.as_bytes())
            || request.sections.handoff() != &self.handoff.to_value()
            || request.sections.tools() != self.tools
        {
            return Err(RenderDiagnostic::InvalidAccounting);
        }
        let bytes = |key: &str| {
            canonical(&request.sections.handoff()[key])
                .map(|v| v.len() as u64)
                .map_err(|_| RenderDiagnostic::InvalidAccounting)
        };
        let mut usage = TokenUsage {
            instruction_tokens: bytes("instructions")?,
            task_tokens: bytes("task")?,
            context_tokens: bytes("context")?,
            evidence_tokens: bytes("evidence")?,
            tool_schema_tokens: (canonical(request.sections.output_schema())
                .map_err(|_| RenderDiagnostic::InvalidAccounting)?
                .len() as u64)
                .checked_add(
                    canonical(request.sections.tools())
                        .map_err(|_| RenderDiagnostic::InvalidAccounting)?
                        .len() as u64,
                )
                .ok_or(RenderDiagnostic::InvalidAccounting)?,
            framing_tokens: 0,
        };
        let bound = (request.payload.len() as u64)
            .checked_add(256)
            .ok_or(RenderDiagnostic::InvalidAccounting)?;
        usage.framing_tokens = bound
            .checked_sub(usage.total().ok_or(RenderDiagnostic::InvalidAccounting)?)
            .ok_or(RenderDiagnostic::InvalidAccounting)?;
        Ok(TokenCountResult {
            profile: self.profile.clone(),
            payload_digest: request.payload_digest.clone(),
            mode: CountMode::Estimated,
            provenance: self.provenance.clone(),
            usage,
        })
    }
}
struct ModelRunAuthority<'a> {
    host: &'a CurrentModelHost<'a>,
    counter: &'a WireCounter<'a>,
}
impl SessionResourceAuthorityPort for ModelRunAuthority<'_> {
    fn validate_allocation(
        &self,
        allocation: &SessionResourceAllocation,
    ) -> Result<(), SessionError> {
        let plan = &self.host.acquired.plan;
        if allocation.reservation_id() != &plan.model_reservation
            || allocation.binding() != &plan.origin_binding
        {
            return Err(SessionError::StaleRevision);
        }
        // The resource owner checks the actual model child against fresh auth,
        // registration, limits and PG claim. The following native Source gate
        // independently checks the full exact checkpoint, all sidecars and
        // current policy inputs; do not resolve those same inputs twice here.
        plan.access.validate_allocation(allocation)?;
        self.host.current()
    }
    fn validate_invocation_allocation(
        &self,
        allocation: &SessionResourceAllocation,
        ir: &gateway_application::prompt_ir::InvocationIr,
    ) -> Result<(), SessionError> {
        self.validate_allocation(allocation)?;
        gateway_application::prompt_ir::InvocationAdmissionPort::validate_current(self.host, ir)
            .map_err(|_| SessionError::AuthorityDenied)
    }
    fn validate_token_bound(
        &self,
        allocation: &SessionResourceAllocation,
        count: &gateway_domain::prompt_rendering::TokenCountResult,
        payload: &str,
    ) -> Result<(), SessionError> {
        self.validate_allocation(allocation)?;
        if count.profile != self.counter.profile
            || count.provenance != self.counter.provenance
            || count.mode != gateway_domain::prompt_rendering::CountMode::Estimated
            || count.payload_digest != context_content_digest(payload.as_bytes())
            || count.usage.total()
                != Some(
                    (payload.len() as u64)
                        .checked_add(256)
                        .ok_or(SessionError::LimitExceeded)?,
                )
        {
            return Err(SessionError::AuthorityDenied);
        }
        Ok(())
    }
}

pub(super) struct NativeModelProducerRun {
    acquired: AcquiredNativeModelPlan,
    invocation: crate::invocation_host::session_tools::SessionToolInvocation,
    run: crate::invocation_host::session_tools::SessionToolRun,
    last_verified: Option<gateway_application::output_verification::VerifiedOutput>,
    generation_checks: Option<(u64, u64)>,
}
impl AcquiredNativeModelPlan {
    pub(super) fn start(
        self,
        target: &ScopedTarget,
        allocation: SessionResourceAllocation,
        context: gateway_application::codex::RequestContext,
    ) -> Result<NativeModelProducerRun, SessionError> {
        use crate::invocation_host::session_tools::*;
        if allocation.reservation_id() != &self.plan.model_reservation {
            return Err(SessionError::StaleRevision);
        }
        let host = CurrentModelHost {
            acquired: &self,
            target,
        };
        // Native admission performs the current check after pure data assembly.
        let invocation = SessionToolInvocation::admit(
            self.handoff.clone(),
            self.prepared.clone(),
            self.plan.tools.clone(),
            &host,
        )
        .map_err(|_| SessionError::AuthorityDenied)?;
        let model = &self.plan.definitions.model;
        let amounts = allocation.amounts();
        let limits = gateway_domain::invocation_resilience::InvocationRunLimits {
            schema_version: SchemaVersion::V1,
            max_attempts: model.max_attempts,
            timeout_ms: model.timeout_ms,
            invocation_input_tokens: u64::from(model.input_tokens),
            invocation_output_tokens: u64::from(
                self.plan.definitions.generation.data().max_output_tokens,
            ),
            run_input_tokens: amounts.input_tokens,
            run_output_tokens: amounts.output_tokens,
            cost_microunits: amounts.cost_microunits,
            output_bytes: amounts.output_bytes,
            reconciliation_tolerance: 0,
        };
        let run = SessionToolRun::new(
            invocation.invocation().clone(),
            allocation,
            limits,
            context,
            &self.plan.definitions.adapter.pricing.cost_unit,
        )
        .map_err(|_| SessionError::LimitExceeded)?;
        Ok(NativeModelProducerRun {
            acquired: self,
            invocation,
            run,
            last_verified: None,
            generation_checks: None,
        })
    }
}
impl NativeModelProducerRun {
    /// One actual CG-14-authorized attempt. Retries reuse this SAME mutable run
    /// and require the caller's actual next CG-14 decision; no implicit retry.
    pub(super) fn attempt(
        &mut self,
        target: &ScopedTarget,
        decision: gateway_application::closed_loop::LoopDecision,
    ) -> Result<PreparedConnectorTurn, SessionError> {
        use crate::{invocation_host::session_tools::*, model_adapters::*};
        use gateway_application::prompt_rendering::PromptRendererPort;
        let host = CurrentModelHost {
            acquired: &self.acquired,
            target,
        };
        // Renderer construction is pure; current authority is checked after
        // resolving the credential and again at the actual run/transport gates.
        let definitions = &self.acquired.plan.definitions;
        let binding = RuntimeModelBinding {
            canonical: definitions.capability.data().model.clone(),
            runtime_id: definitions.target.runtime_id.clone(),
        };
        let renderer = SessionToolHttpRenderer::for_runtime(
            AdapterKind::ChatCompletionsV1,
            self.acquired.prepared.clone(),
            self.acquired.handoff.clone(),
            self.acquired.plan.tools.clone(),
            binding.clone(),
        )
        .map_err(|_| SessionError::UnsupportedCapability)?
        .with_wire_mode(definitions.adapter.wire_mode);
        let (renderer_id, template_version) = renderer.identity();
        let profile = gateway_domain::prompt_rendering::TokenizerProfile {
            schema_version: SchemaVersion::V1,
            model: definitions.capability.data().model.clone(),
            generation_profile: definitions.generation.content_digest(),
            tokenizer: definitions.adapter.tokenizer.id.clone(),
            tokenizer_version: definitions.adapter.tokenizer.revision.clone(),
            renderer: renderer_id,
            template_version,
        };
        let counter = WireCounter {
            profile: profile.clone(),
            provenance: definitions.adapter.tokenizer.clone(),
            handoff: &self.acquired.handoff,
            tools: &self.acquired.plan.tools,
        };
        let authority = ModelRunAuthority {
            host: &host,
            counter: &counter,
        };
        let credential = definitions
            .adapter
            .credential_source
            .as_ref()
            .map(|locator| locator.resolve())
            .transpose()?;
        // Resolving a control-plane secret cannot refresh current permission.
        host.current()?;
        let adapter = SessionToolHttpAdapter::new(
            AdapterKind::ChatCompletionsV1,
            definitions.adapter.endpoint.clone(),
            credential,
            Some(binding),
        )
        .map_err(|_| SessionError::UnsupportedCapability)?
        .with_response_mode(definitions.adapter.response_mode)
        .with_wire_mode(definitions.adapter.wire_mode);
        let transport = SessionToolProviderAdapter::new(adapter);
        let cost = definitions.adapter.pricing.maximum(
            u64::from(definitions.model.input_tokens),
            u64::from(definitions.generation.data().max_output_tokens),
            1,
        )?;
        let before_attempt_status_checked_at_ms = self.acquired.plan.access.now_ms()?;
        if !definitions
            .status
            .in_window(before_attempt_status_checked_at_ms)
        {
            return Err(SessionError::Unavailable);
        }
        let verified = self
            .invocation
            .invoke_in(
                &mut self.run,
                SessionToolAttemptExecution {
                    host: &host,
                    authority: &authority,
                    profile,
                    renderer: &renderer,
                    tokenizer: &counter,
                    reserved_cost_microunits: cost,
                    cost_unit: &definitions.adapter.pricing.cost_unit,
                    transport: &transport,
                    decision,
                },
            )
            .map_err(|failure| {
                #[cfg(test)]
                {
                    use crate::invocation_host::session_tools::SessionToolInvocationError as E;
                    match &failure {
                        E::Resource(code) => eprintln!("native model resource denial={code:?}"),
                        E::Run(code) => eprintln!("native model run denial={code:?}"),
                        E::Admission(code) => eprintln!("native model admission denial={code:?}"),
                        E::Rendering(code) => eprintln!(
                            "native model render denial class={:?}",
                            std::mem::discriminant(code)
                        ),
                        _ => eprintln!(
                            "native model output denial class={:?}",
                            std::mem::discriminant(&failure)
                        ),
                    }
                }
                #[cfg(not(test))]
                let _ = failure;
                SessionError::AuthorityDenied
            })?;
        host.current()?;
        let verified_status_checked_at_ms = self.acquired.plan.access.now_ms()?;
        if !definitions.status.in_window(verified_status_checked_at_ms) {
            return Err(SessionError::Unavailable);
        }
        self.last_verified = Some(verified.clone());
        self.generation_checks = Some((
            before_attempt_status_checked_at_ms,
            verified_status_checked_at_ms,
        ));
        PreparedConnectorTurn::new(
            self.invocation.invocation().clone(),
            verified,
            self.acquired.before.clone(),
        )
    }
    /// Historical evidence of the actual native preparation and successful live
    /// attempt. These times are checks, never claimed HTTP-send timestamps or
    /// permission to run another model request.
    pub(super) fn generation_evidence(&self) -> Result<Value, SessionError> {
        let output = self
            .last_verified
            .as_ref()
            .ok_or(SessionError::InvalidState)?;
        let (before, after) = self.generation_checks.ok_or(SessionError::InvalidState)?;
        if self.acquired.prepared_status_checked_at_ms > before
            || before > after
            || [self.acquired.prepared_status_checked_at_ms, before, after]
                .iter()
                .any(|time| !self.acquired.plan.definitions.status.in_window(*time))
        {
            return Err(SessionError::Unavailable);
        }
        let status = &self.acquired.prepared_status;
        Ok(
            json!({"contract":"cg.native-model-generation-evidence","contract_version":"1.0","invocation":self.invocation.invocation().identity(),"output_digest":output.output().binding().output_digest,"verification_record":output.record(),"status":{"provider":status.provider,"availability":status.availability,"observation":status.observation},"prepared_status_checked_at_ms":self.acquired.prepared_status_checked_at_ms,"before_attempt_status_checked_at_ms":before,"verified_status_checked_at_ms":after}),
        )
    }
    /// Live preparation only. Retained data after a restart must use a separate
    /// rebuilding/verification owner; this method cannot recreate this run.
    pub(super) fn with_current_preparation<T>(
        &self,
        target: &ScopedTarget,
        turn: &PreparedConnectorTurn,
        body: impl FnOnce(&ModelInputsCurrent) -> Result<T, SessionError>,
    ) -> Result<T, SessionError> {
        let actual = self
            .last_verified
            .as_ref()
            .ok_or(SessionError::InvalidState)?;
        if canonical(turn.invocation())? != canonical(self.invocation.invocation())?
            || canonical(turn.verified_output().record())? != canonical(actual.record())?
            || turn.verified_output().output().raw_bytes() != actual.output().raw_bytes()
            || turn.before_snapshot().scope() != self.acquired.before.scope()
            || turn.before_snapshot().bytes() != self.acquired.before.bytes()
            || !Arc::ptr_eq(
                &self.acquired.plan.access,
                &target.context_source().model_access(),
            )
        {
            return Err(SessionError::StaleRevision);
        }
        let view = self.preparation_view(target)?;
        self.run
            .validate_current(
                &PreparationAuthority {
                    plan: &self.acquired.plan,
                    view: &view,
                    original: self.invocation.invocation(),
                },
                self.invocation.invocation(),
            )
            .map_err(|_| SessionError::AuthorityDenied)?;
        let result = body(&view)?;
        drop(view);
        let final_view = self.preparation_view(target)?;
        self.run
            .validate_current(
                &PreparationAuthority {
                    plan: &self.acquired.plan,
                    view: &final_view,
                    original: self.invocation.invocation(),
                },
                self.invocation.invocation(),
            )
            .map_err(|_| SessionError::AuthorityDenied)?;
        Ok(result)
    }
    fn preparation_view(&self, target: &ScopedTarget) -> Result<ModelInputsCurrent, SessionError> {
        let view = target.context_source().model_inputs_current()?;
        let compiled = self
            .acquired
            .plan
            .compile(&view, &self.acquired.materials)?;
        if compiled.to_json().map_err(|_| invalid())?
            != self.acquired.compiled.to_json().map_err(|_| invalid())?
        {
            return Err(SessionError::StaleRevision);
        }
        if !self
            .acquired
            .plan
            .definitions
            .status
            .in_window(self.acquired.plan.access.now_ms()?)
        {
            return Err(SessionError::Unavailable);
        }
        validate_verifiers(view.runtime(), &self.acquired.plan.verification)?;
        Ok(view)
    }
    pub(super) fn usage(&self) -> &SessionResourceAmounts {
        self.run.conservative_usage()
    }
}

#[cfg(test)]
pub(super) mod tests;

/// This scoped authority owns no caller-created allowance. SessionToolRun supplies
/// its original private child; source semantics were freshly checked for this
/// exact view and will be checked again after the pure preparation callback.
struct PreparationAuthority<'a> {
    plan: &'a NativeModelProducerPlan,
    view: &'a ModelInputsCurrent,
    original: &'a gateway_application::prompt_ir::InvocationIr,
}
impl SessionResourceAuthorityPort for PreparationAuthority<'_> {
    fn validate_allocation(
        &self,
        allocation: &SessionResourceAllocation,
    ) -> Result<(), SessionError> {
        if allocation.reservation_id() != &self.plan.model_reservation
            || allocation.binding() != &self.plan.origin_binding
            || allocation.binding() != self.view.binding()
            || allocation.phase() != SessionResourcePhase::Preparation
            || allocation.amounts().model_attempts == 0
            || self.view.checkpoint() != &self.plan.checkpoint
        {
            return Err(SessionError::AuthorityDenied);
        }
        self.plan.access.validate_allocation(allocation)
    }
    fn validate_invocation_allocation(
        &self,
        allocation: &SessionResourceAllocation,
        ir: &gateway_application::prompt_ir::InvocationIr,
    ) -> Result<(), SessionError> {
        self.validate_allocation(allocation)?;
        if canonical(ir)? != canonical(self.original)? {
            return Err(SessionError::StaleRevision);
        }
        Ok(())
    }
}
