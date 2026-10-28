//! One budgeted native discovery plus complete fixed read set; no model/effect path.
use super::{
    claim::{GoalClaim, now_ms},
    definitions::*,
    read_association::*,
    source::{GoalSource, PolicySource},
};
use crate::{
    local_sessions::LocalSessionAccess,
    mcp_client::{
        CallContext, HostConfig,
        credential_file::CredentialFileResolver,
        normalization::{CanonicalResultNormalizer, NormalizationRoute},
        scalar_quotes::{
            FixedLocalTariff, QualifiedScalarConfig, QualifiedScalarMetadata, qualified_scalar_host,
        },
        session_budget::{SessionConnectorComponents, SessionConnectorRun},
        transport::CredentialedStdioFactory,
    },
};
use gateway_application::{
    PlanningCapabilitySnapshot,
    closed_loop::{ClosedLoop, LoopRules},
    connector_admission::{ConnectorRegistration, ConnectorRegistry, ConnectorScopeBinding},
    connector_manifest::InventoryKind,
    connector_policy::Cg09ConnectorAdmission,
    connectors::*,
    sessions::*,
};
use gateway_domain::*;
use std::{collections::BTreeMap, sync::Arc};
pub(super) fn preparation_budget(
    defs: &ReadonlyDefinitions,
) -> Result<SessionResourceAmounts, SessionError> {
    let mut result = defs.quotes.discovery;
    for route in &defs.normalizations {
        let quote = defs
            .quotes
            .operations
            .iter()
            .find(|q| q.mapping_reference == route.read.mapping_reference)
            .ok_or(SessionError::InvalidInput)?;
        result = result.checked_add(&quote.amounts)?;
    }
    result.validate_readonly()?;
    Ok(result)
}
pub(super) fn rules(defs: &ReadonlyDefinitions) -> Result<LoopRules, SessionError> {
    let VerifierMode::RetainedObservations = &defs.verifier.mode;
    Ok(LoopRules {
        capabilities: PlanningCapabilitySnapshot::new(
            defs.command.resolved.snapshot.input().index.clone(),
            defs.command
                .resolved
                .report
                .basis
                .registry_fingerprint
                .as_str(),
            PlanningIrVersion::V1,
        )
        .map_err(|_| SessionError::InvalidInput)?,
        requirements: CapabilityRequirementRules::default(),
        planner: PlannerRules::default(),
        max_iterations: defs.verifier.max_iterations,
        max_retries: defs.verifier.max_retries,
    })
}
fn session_error(f: ConnectorFailure) -> SessionError {
    if matches!(
        f.effect,
        ConnectorEffect::Unknown | ConnectorEffect::Acknowledged
    ) || f.code == ConnectorFailureCode::MutationUncertain
    {
        return SessionError::OutcomeUnknown;
    }
    match f.code {
        ConnectorFailureCode::BudgetExhausted => SessionError::LimitExceeded,
        ConnectorFailureCode::ScopeMismatch | ConnectorFailureCode::CredentialScopeDenied => {
            SessionError::ScopeDenied
        }
        ConnectorFailureCode::PermissionDenied
        | ConnectorFailureCode::AuthenticationFailed
        | ConnectorFailureCode::SecretDisclosure => SessionError::AuthorityDenied,
        ConnectorFailureCode::InventoryChanged | ConnectorFailureCode::StaleResult => {
            SessionError::StaleRevision
        }
        ConnectorFailureCode::ProtocolError
        | ConnectorFailureCode::SchemaMismatch
        | ConnectorFailureCode::ConflictingResult => SessionError::InvalidInput,
        ConnectorFailureCode::IncompatibleVersion | ConnectorFailureCode::RetryUnsafe => {
            SessionError::UnsupportedCapability
        }
        ConnectorFailureCode::Unavailable
        | ConnectorFailureCode::CredentialUnavailable
        | ConnectorFailureCode::Timeout
        | ConnectorFailureCode::Cancelled
        | ConnectorFailureCode::RateLimited
        | ConnectorFailureCode::NotFound
        | ConnectorFailureCode::Backpressure => SessionError::Unavailable,
        ConnectorFailureCode::MutationUncertain => SessionError::OutcomeUnknown,
    }
}

#[cfg(test)]
#[path = "target_error_tests.rs"]
mod error_tests;
fn infrastructure(_: crate::mcp_client::Error) -> SessionError {
    SessionError::InvalidInput
}
fn bind_inventory(
    source: &GoalSource,
    metadata: &QualifiedScalarMetadata,
    summary: &ConnectorInventory,
) -> Result<(), SessionError> {
    source.current()?;
    let defs = &source.defs;
    let inventory = metadata.snapshot_for(summary).map_err(infrastructure)?;
    let mut schemas = BTreeMap::new();
    for record in defs.mapping.records() {
        if record.kind == InventoryKind::Tool {
            schemas.insert(
                record.capability.clone(),
                metadata
                    .tool_schema(&summary.target, &record.capability, &summary.revision)
                    .map_err(infrastructure)?,
            );
        }
    }
    let association = ReadAssociation::from_json(
        &defs.runtime.association,
        &serde_json::to_vec(defs.document(&defs.runtime.association)?)
            .map_err(|_| SessionError::InvalidInput)?,
    )?;
    for pin in association.reference_bindings() {
        defs.document(&pin)?;
    }
    let objects = defs
        .documents
        .values()
        .filter(|(pin, _)| pin.contract.as_str() == "cg.connector-object")
        .map(|(pin, body)| {
            ScopeObjectDocument::from_json(
                pin,
                &serde_json::to_vec(body).map_err(|_| SessionError::InvalidInput)?,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    for object in &objects {
        for pin in object.reference_bindings() {
            defs.document(&pin)?;
        }
    }
    let subjects = defs
        .normalizations
        .iter()
        .filter(|n| {
            !defs
                .before
                .as_ref()
                .is_some_and(|world| world.read == n.read)
        })
        .map(|n| {
            Ok(ReadSubjectBinding {
                mapping_reference: n.read.mapping_reference.clone(),
                arguments: n.read.arguments.clone(),
                source: n.read.source.clone(),
                subjects: n.document.fields.values().cloned().collect(),
                object_field: n.document.object_field.clone(),
                condition_ids: defs
                    .runtime
                    .goal_reads
                    .iter()
                    .filter(|r| r.read == n.read)
                    .map(|r| ConditionId::new(&r.condition).map_err(|_| SessionError::InvalidInput))
                    .collect::<Result<_, _>>()?,
            })
        })
        .collect::<Result<Vec<_>, SessionError>>()?;
    let execution = source.execution()?;
    let bound = association.admit(ReadAssociationContext {
        task_pin: &defs.runtime.task,
        task: &defs.task,
        goal: source
            .checkpoint
            .goal
            .as_connector()
            .ok_or(SessionError::InvalidState)?,
        projection: &defs.runtime.projection,
        step: &defs.native.step,
        execution: &execution,
        mapping_pin: &defs.runtime.mapping,
        mapping_document: defs.document(&defs.runtime.mapping)?,
        inventory: &inventory,
        schemas: &schemas,
        subjects: &subjects,
        objects: &objects,
        scope_export: source.export()?,
    })?;
    source.install_association(bound)?;
    if let Some(world) = &defs.before {
        let schema = schemas
            .get(&world.read.capability)
            .ok_or(SessionError::InvalidInput)?;
        source.install_atomic(super::atomic_association::BoundAtomicRead::admit(
            defs, world, &inventory, schema, execution,
        )?)?;
    }
    Ok(())
}
struct ReadResult {
    read: ReadAdmission,
    request: ConnectorRequest,
    observation: ConnectorObservation,
    batch: gateway_application::ScopedObservationBatch,
    max_age: u64,
}
pub(super) fn capture(
    access: Arc<LocalSessionAccess>,
    checkpoint: &SessionCheckpoint,
    resources: &mut SessionPhaseResources,
) -> Result<Option<PreExecutionGoalCandidate>, SessionError> {
    match capture_preparation(access, checkpoint, resources, false)? {
        Some(ReadonlyPreparation::GoalSatisfied(candidate)) => Ok(Some(*candidate)),
        _ => Ok(None),
    }
}
pub(super) fn prepare(
    access: Arc<LocalSessionAccess>,
    checkpoint: &SessionCheckpoint,
    resources: &mut SessionPhaseResources,
) -> Result<Option<ReadonlyPreparation>, SessionError> {
    capture_preparation(access, checkpoint, resources, true)
}
fn capture_preparation(
    access: Arc<LocalSessionAccess>,
    checkpoint: &SessionCheckpoint,
    resources: &mut SessionPhaseResources,
    capture_before: bool,
) -> Result<Option<ReadonlyPreparation>, SessionError> {
    let claim = Arc::new(GoalClaim::capture(access, checkpoint)?);
    let host = claim.current()?;
    let goal = checkpoint
        .goal
        .as_connector()
        .ok_or(SessionError::UnsupportedCapability)?;
    let runtime = host
        .sessions
        .as_ref()
        .and_then(|s| s.observed_goal_runtime.as_ref())
        .ok_or(SessionError::UnsupportedCapability)?;
    let defs = Arc::new(runtime.capture(&host, goal, checkpoint.execution)?);
    claim.validate_durable()?;
    let amounts = preparation_budget(&defs)?;
    let allocation = resources.allocate(amounts)?;
    let context = CallContext::new(
        NonEmptyText::new("goal-discovery").map_err(|_| SessionError::InvalidInput)?,
        allocation.deadline(),
    );
    let mut captured = None;
    let run = SessionConnectorRun::with_factory(allocation, claim.clone(), context, |view| {
        let source = Arc::new(GoalSource::new(
            claim.clone(),
            defs.clone(),
            checkpoint.clone(),
            view.clone(),
        ));
        source.current()?;
        let mut by_capability: BTreeMap<CapabilityId, Vec<NormalizationRoute>> = BTreeMap::new();
        for n in &defs.normalizations {
            by_capability
                .entry(n.read.capability.clone())
                .or_default()
                .push(NormalizationRoute {
                    capability: n.read.capability.clone(),
                    source: n.read.source.clone(),
                    arguments: n.read.arguments.clone(),
                    profile: n.document.profile()?,
                });
        }
        let normalizers = by_capability
            .into_iter()
            .map(|(cap, routes)| {
                Ok((
                    cap,
                    CanonicalResultNormalizer::new_qualified_routes(
                        routes,
                        Box::new(source.clone()),
                    )
                    .map_err(infrastructure)?,
                ))
            })
            .collect::<Result<BTreeMap<_, _>, SessionError>>()?;
        let FixedTariffDocument::FixedLocalNoEmbedding {
            discovery_cost_microunits,
            operation_cost_microunits,
        } = defs.quotes.tariff;
        let credentials = CredentialFileResolver::new(defs.runtime.credential_store.clone())
            .map_err(|_| SessionError::AuthorityDenied)?;
        let factory = CredentialedStdioFactory::new(
            defs.runtime.stdio.build()?,
            Arc::new(credentials),
            CredentialProfileRef::new(defs.runtime.access_profile.clone()),
            defs.runtime.connector.as_str(),
            defs.manifest.definition().server_id.clone(),
        );
        // The source already authenticates the durable owner/claim. Build the
        // derived allowlist only from the captured operator-owned runtime pin.
        let binding = ConnectorScopeBinding {
            project: NonEmptyText::new(checkpoint.owner.project.as_str())
                .map_err(|_| SessionError::InvalidInput)?,
            scope: source.target().scope,
            session: Some(
                NonEmptyText::new(checkpoint.snapshot.session.as_str())
                    .map_err(|_| SessionError::InvalidInput)?,
            ),
            request: Some(
                NonEmptyText::new(checkpoint.snapshot.run.as_str())
                    .map_err(|_| SessionError::InvalidInput)?,
            ),
        };
        let registry = ConnectorRegistry::rebuild(
            vec![ConnectorRegistration {
                binding: binding.clone(),
                manifest: defs.manifest.clone(),
                enabled: true,
            }],
            &defs.runtime.compatibility.build(),
        )
        .map_err(|_| SessionError::AuthorityDenied)?;
        let admission = registry
            .bind(
                &binding,
                &source.target(),
                Arc::new(Cg09ConnectorAdmission::new(PolicySource(source.clone()))),
            )
            .map_err(|_| SessionError::AuthorityDenied)?;
        let bundle = qualified_scalar_host(
            QualifiedScalarConfig {
                allocation: view,
                host: HostConfig {
                    target: source.target(),
                    manifest: defs.manifest.clone(),
                    profile: defs.runtime.compatibility.build(),
                    limits: defs.runtime.limits.build()?,
                },
                document: defs.mapping.clone(),
                normalizers,
                tariff: FixedLocalTariff::operator_fixed_no_embedding(
                    defs.quotes.cost_unit.clone(),
                    discovery_cost_microunits,
                    operation_cost_microunits,
                ),
            },
            Box::new(factory),
            admission,
            source.clone(),
            Arc::new(()),
        )
        .map_err(infrastructure)?;
        let metadata = bundle.metadata();
        captured = Some((source.clone(), metadata));
        Ok(SessionConnectorComponents::from_qualified_joined(
            bundle, source,
        ))
    })?;
    let (source, metadata) = captured.ok_or(SessionError::InvalidState)?;
    let summary = run.discover(&source.target()).map_err(session_error)?;
    bind_inventory(&source, &metadata, &summary)?;
    #[cfg(test)]
    super::source::boundary_tests::after_inventory(&source, &run, &summary)?;
    let execution = source.execution()?;
    let mut results = Vec::new();
    for (ordinal, n) in defs.normalizations.iter().enumerate() {
        if !defs
            .before
            .as_ref()
            .is_some_and(|world| world.read == n.read)
        {
            results.push(read_one(&source, &run, &summary, n, ordinal)?);
        }
    }
    source.current()?;
    let inputs = defs
        .runtime
        .goal_reads
        .iter()
        .map(|route| {
            let result = results
                .iter()
                .find(|r| r.read == route.read)
                .ok_or(SessionError::InvalidInput)?;
            let condition = goal
                .intent()
                .desired_state()
                .conditions()
                .iter()
                .find(|c| c.id().as_str() == route.condition)
                .ok_or(SessionError::InvalidInput)?;
            Ok(PreExecutionGoalReadInput {
                condition: condition.id(),
                read_request: &result.request,
                observation: &result.observation,
                batch: &result.batch,
                max_age_seconds: result.max_age,
            })
        })
        .collect::<Result<Vec<_>, SessionError>>()?;
    let binding = PreExecutionGoalBinding {
        acquisition: source.view.allocation().binding().clone(),
        reservation: source.view.allocation().reservation_id().clone(),
        claim_revision: checkpoint.snapshot.revision,
        task: defs.task.data().id.clone(),
        execution,
        context_digest: super::context_digest(&defs, source.global_consent_required()?)?,
    };
    let data = PreExecutionGoalData::new(
        goal,
        binding,
        defs.definition_pins.clone(),
        &inputs,
        now_ms()?,
    )?;
    let proof = ClosedLoop::verify_retained(
        data.execution()?,
        goal.scope().clone(),
        goal.intent().clone(),
        data.readset(),
        rules(&defs)?,
    )
    .map_err(|_| SessionError::InvalidInput)?;
    source.current()?;
    if let Some(proof) = proof {
        return Ok(Some(ReadonlyPreparation::GoalSatisfied(Box::new(
            PreExecutionGoalCandidate::new(data, &proof)?,
        ))));
    }
    if capture_before && let Some(world) = &defs.before {
        let route = defs
            .normalizations
            .iter()
            .find(|n| n.read == world.read)
            .ok_or(SessionError::InvalidInput)?;
        let result = read_one(&source, &run, &summary, route, defs.normalizations.len())?;
        let captured = super::atomic_capture::capture(
            &source,
            world,
            result.request,
            result.observation,
            result.batch,
        )?;
        source.current()?;
        drop(run);
        claim.validate_durable()?;
        return Ok(Some(ReadonlyPreparation::BeforeCaptured(Box::new(
            captured,
        ))));
    }
    Ok(None)
}

fn read_one(
    source: &GoalSource,
    run: &SessionConnectorRun,
    summary: &ConnectorInventory,
    n: &ReadNormalization,
    ordinal: usize,
) -> Result<ReadResult, SessionError> {
    let defs = &source.defs;
    let execution = source.execution()?;
    let record = defs
        .mapping
        .records()
        .iter()
        .find(|r| r.reference == n.read.mapping_reference)
        .ok_or(SessionError::InvalidInput)?;
    let correlation=context_content_digest(&serde_json::to_vec(&serde_json::json!({"purpose":"PRE_EXECUTION_GOAL","binding":source.view.allocation().binding(),"reservation":source.view.allocation().reservation_id(),"ordinal":ordinal,"read":n.read})).map_err(|_|SessionError::InvalidInput)?);
    let request = ConnectorRequest {
        target: source.target(),
        capability: n.read.capability.clone(),
        class: record.class.operation_class(),
        inventory_revision: summary.revision.clone(),
        arguments: n.read.arguments.clone(),
        task: defs.task.data().id.clone(),
        step: defs.native.step.clone(),
        execution: execution.clone(),
        correlation: NonEmptyText::new(format!("goal-read-{}", correlation.as_str()))
            .map_err(|_| SessionError::InvalidInput)?,
    };
    let selected = if record.kind == InventoryKind::Resource {
        Some(&n.read.source)
    } else {
        None
    };
    source.begin(&request, selected)?;
    let observation = match selected {
        Some(s) => run.read_resource(&request, s),
        None => run.invoke(&request),
    }
    .map_err(session_error)?;
    source.current()?;
    let batch = source.batch(&request, &observation)?;
    let ReadSemantics::DirectCurrentReadProfile {
        max_age_seconds, ..
    } = n.document.acquisition.semantics;
    Ok(ReadResult {
        read: n.read.clone(),
        request,
        observation,
        batch,
        max_age: max_age_seconds,
    })
}
