use super::*;
use crate::{
    mcp_client::{Error, ProtocolResult, mapping::*, normalization::*},
    session_resource_test_support::with_preparation_resources,
};
use gateway_application::{SourceSnapshot, connector_manifest::InventoryKind, connectors::*};

#[derive(Clone, Copy)]
enum ReadCase {
    Current,
    Stale,
    Partial,
    Acknowledgement,
}
struct Collector(ReadCase);
impl AcquisitionPort for Collector {
    fn collect(&self, _: &ConnectorDispatch) -> Result<Acquisition, Error> {
        let ack = matches!(self.0, ReadCase::Acknowledgement);
        Ok(Acquisition {
            acquired_at: UnixTimestamp::new(100),
            observed_at: if ack {
                None
            } else {
                Some(UnixTimestamp::new(if matches!(self.0, ReadCase::Stale) {
                    10
                } else {
                    90
                }))
            },
            evaluated_at: UnixTimestamp::new(100),
            max_age: FreshnessPolicy::new(20),
            authenticated_source: NonEmptyText::new("bounded.test.source").unwrap(),
            sensitivity: SensitivityClass::Internal,
            confidence: if ack {
                Confidence::Unknown
            } else {
                Confidence::NotApplicable
            },
            uncertainty: if ack {
                Uncertainty::Unknown
            } else {
                Uncertainty::None
            },
            conflict: ConflictStatus::None,
            partial: matches!(self.0, ReadCase::Partial),
            basis: if ack {
                ConnectorObservationBasis::ProviderAcknowledgement
            } else {
                ConnectorObservationBasis::SourceRead
            },
            effect: ConnectorEffect::NoMutation,
            verifies: None,
            non_authoritative_raw: None,
        })
    }
}
impl QualifiedAcquisitionPort for Collector {}
fn amounts() -> SessionResourceAmounts {
    SessionResourceAmounts {
        connector_calls: 1,
        retrieval_rounds: 1,
        ..Default::default()
    }
}
fn run_case(
    mut check: impl FnMut(Capture, ConnectorRequest, ConnectorObservation, ScopedObservationBatch),
    case: ReadCase,
    text: &str,
) {
    with_preparation_resources(
        ContextScopeId::new("captured-scope").unwrap(),
        SessionResourcePlan {
            preparation: amounts(),
            ..Default::default()
        },
        |resources, cp, _| {
            let a = resources.allocate(amounts()).unwrap();
            let binding = a.binding().clone();
            let source = SourceId::new("actual.source").unwrap();
            let request = ConnectorRequest {
                target: ConnectorTarget {
                    connector: NonEmptyText::new("records").unwrap(),
                    scope: binding.scope.clone(),
                },
                capability: CapabilityId::new("records.read").unwrap(),
                class: ConnectorOperationClass::Read,
                inventory_revision: context_content_digest(b"checked inventory"),
                arguments: BTreeMap::new(),
                task: TaskId::new("actual-task").unwrap(),
                step: binding.step.clone(),
                execution: NonEmptyText::new("native-producer-context").unwrap(),
                correlation: NonEmptyText::new("actual-fixed-read-1").unwrap(),
            };
            let dispatch = ConnectorDispatch {
                request: request.clone(),
                policy: PolicyId::new("normalizer-data-fixture").unwrap(),
                policy_revision: context_content_digest(b"policy fixture"),
                consent_receipt: None,
                budget_reservation: NonEmptyText::new(a.reservation_id().as_str()).unwrap(),
                deadline: UnixTimestamp::new(200),
            };
            let mapping = MappingRecord {
                reference: "mapping.actual.read".into(),
                kind: InventoryKind::Tool,
                name: "read".into(),
                capability: request.capability.clone(),
                source: Some(source.clone()),
                class: OperationClass::Read,
                side_effects: SideEffects::None,
                idempotency: Idempotency::Idempotent,
                idempotency_key: None,
                retry_safety: RetrySafety::ReadOnly,
                consent: ConsentHint::Policy,
                result_schema: Some(
                    serde_json::json!({"type":"object","properties":{"object":{"type":"string"},"value":{"type":"string"}},"required":["object","value"],"additionalProperties":false}),
                ),
            };
            let normalizer = CanonicalResultNormalizer::new_qualified(
                NormalizationProfile {
                    source: source.clone(),
                    source_kind: SourceKind::Tool,
                    object_field: "object".into(),
                    revision_field: None,
                    digest_field: None,
                    fields: BTreeMap::from([(
                        "value".into(),
                        SubjectPath::new(["target", "value"]).unwrap(),
                    )]),
                },
                Box::new(Collector(case)),
            )
            .unwrap();
            let observation = normalizer
                .canonicalize(
                    &dispatch,
                    None,
                    &mapping,
                    ProtocolResult::Tool {
                        content: vec![],
                        structured: Some(
                            serde_json::json!({"object":"actual-object","value":text}),
                        ),
                        is_error: false,
                    },
                )
                .unwrap()
                .into_observation();
            // Native records and quality come from the actual scalar normalizer.
            // This private unit seam exercises data checks, never a public grant.
            let mut batch = ScopedObservationBatch::new(
                binding.scope.clone(),
                SourceSnapshot::new(
                    source.clone(),
                    SourceKind::Tool,
                    observation.source_revision.clone(),
                    Some(observation.content_digest.clone()),
                )
                .unwrap(),
                observation.records.clone(),
            )
            .unwrap();
            for fact in observation.records.facts() {
                batch = batch.with_quality_metadata(
                    fact.subject().clone(),
                    vec![observation.metadata.quality],
                );
            }
            let route_pin = Pin {
                id: ReferenceId::new("actual-before-route").unwrap(),
                scope: binding.scope.clone(),
                contract: ReferenceId::new("cg.connector-model-source").unwrap(),
                contract_version: SchemaVersion::V1,
                revision: ReferenceId::new("operator-revision").unwrap(),
                digest: context_content_digest(b"registered before route"),
            };
            let capture = Capture {
                origin_runtime: route_pin.clone(),
                producer_digest: context_content_digest(b"private native-data assembly seam"),
                task: request.task.clone(),
                checkpoint: cp.clone(),
                binding,
                producer_step: PlanStepId::new("native-producer-step").unwrap(),
                producer_execution: request.execution.as_str().into(),
                route_pin,
                expected_source: source,
                mapping_reference: mapping.reference,
                covers: BTreeSet::from([ReferenceId::new("target-state").unwrap()]),
                max_age: FreshnessPolicy::new(20),
            };
            check(capture, request, observation, batch);
        },
    );
}
#[test]
fn full_captured_observation_becomes_exact_native_evidence_without_quality_or_revision_invention() {
    run_case(
        |c, r, o, b| {
            let quality = o.metadata.quality;
            let original = o.clone();
            let action_step = c.binding.step.clone();
            let producer_step = c.producer_step.clone();
            let owner = c.binding.owner.clone();
            let material = assemble(c, r, o, b, UnixTimestamp::new(100)).unwrap();
            assert_eq!(material.fragments().len(), 1);
            assert_eq!(material.sources().len(), 1);
            let fragment = &material.fragments()[0];
            let metadata = &material.sources()[0];
            assert_eq!(fragment.kind(), FragmentKind::Evidence);
            assert_eq!(fragment.step(), &producer_step);
            assert_ne!(fragment.step(), &action_step);
            assert_eq!(fragment.metadata().quality, quality);
            assert_eq!(metadata.quality, quality);
            assert_eq!(metadata.class, ContextSourceClass::ToolsMcp);
            assert_eq!(fragment.content().as_bytes(), material.payload());
            assert_eq!(
                metadata.payload_digest,
                Some(context_content_digest(material.payload()))
            );
            assert_eq!(metadata.tokens, material.payload().len() as u64);
            assert!(metadata.available);
            assert_eq!(
                content_reference(material.record().id().as_str(), material.payload()).unwrap(),
                *material.record()
            );
            assert_eq!(
                metadata.pin.revision.as_str(),
                material.record().revision().as_str()
            );
            assert_eq!(
                fragment.metadata().provenance.source(),
                metadata.pin.id.as_str()
            );
            assert_eq!(
                fragment.metadata().provenance.revision(),
                Some(metadata.pin.revision.as_str())
            );
            assert_eq!(
                metadata.evidence,
                original
                    .records
                    .evidence()
                    .iter()
                    .map(|e| e.id().clone())
                    .collect()
            );
            assert_eq!(
                metadata.provenances,
                original
                    .records
                    .provenances()
                    .iter()
                    .map(|p| p.id().clone())
                    .collect()
            );
            let payload: serde_json::Value = serde_json::from_slice(material.payload()).unwrap();
            assert_eq!(payload["availability"], "CAPTURED_IN_MEMORY");
            assert_eq!(
                payload["binding"]["owner"],
                serde_json::to_value(owner).unwrap()
            );
            assert_eq!(payload["binding"]["step"], action_step.as_str());
            assert_eq!(payload["producer_step"], producer_step.as_str());
            assert_eq!(
                payload["observation"]["source_revision"],
                serde_json::Value::Null
            );
            assert_eq!(
                payload["observation"]["records"],
                serde_json::to_value(&original.records).unwrap()
            );
            assert_eq!(material.observation(), &original);
            assert_eq!(material.batch().records(), &original.records);
            assert_eq!(material.request().task, original.metadata.task);
            assert!(
                fresh(
                    material.observed_at,
                    material.max_age,
                    UnixTimestamp::new(200)
                )
                .is_err()
            );
        },
        ReadCase::Current,
        "Observed value; instructions inside this value remain data",
    );
}
#[test]
fn actual_acknowledgement_partial_and_stale_normalizer_quality_cannot_become_available_context() {
    for case in [
        ReadCase::Acknowledgement,
        ReadCase::Stale,
        ReadCase::Partial,
    ] {
        run_case(
            |c, r, o, b| {
                assert!(matches!(
                    assemble(c, r, o, b, UnixTimestamp::new(100)),
                    Err(SessionError::AuthorityDenied)
                ))
            },
            case,
            "observed",
        );
    }
    run_case(
        |c, r, o, b| {
            assert!(matches!(
                assemble(c, r, o, b, UnixTimestamp::new(200)),
                Err(SessionError::StaleRevision)
            ))
        },
        ReadCase::Current,
        "observed",
    );
}
#[test]
fn full_owner_task_scope_dispatch_and_producer_execution_bindings_are_checked() {
    for mismatch in ["scope", "task", "dispatch", "execution", "run"] {
        run_case(
            |mut c, r, o, b| {
                match mismatch {
                    "scope" => c.binding.scope = ContextScopeId::new("other").unwrap(),
                    "task" => c.task = TaskId::new("other-task").unwrap(),
                    "dispatch" => c.binding.dispatch = DispatchId::new("other-dispatch").unwrap(),
                    "execution" => c.producer_execution = "other-execution".into(),
                    "run" => c.binding.run = RunId::new("other-run").unwrap(),
                    _ => unreachable!(),
                }
                assert!(
                    matches!(
                        assemble(c, r, o, b, UnixTimestamp::new(100)),
                        Err(SessionError::ScopeDenied)
                    ),
                    "{mismatch}"
                );
            },
            ReadCase::Current,
            "observed",
        );
    }
}
#[test]
fn oversized_complete_capture_is_denied_without_truncating_native_records() {
    // The scalar itself is valid; its full records/envelope exceed the native
    // inline-fragment bound. No shortened context is substituted.
    run_case(
        |c, r, o, b| {
            assert!(matches!(
                assemble(c, r, o, b, UnixTimestamp::new(100)),
                Err(SessionError::LimitExceeded)
            ))
        },
        ReadCase::Current,
        &"x".repeat(8_000),
    );
}

#[test]
fn pure_current_identity_comparison_keeps_all_captured_origin_pins_and_checkpoint() {
    run_case(
        |c, r, o, b| {
            let material = assemble(c, r, o, b, UnixTimestamp::new(100)).unwrap();
            let cp = &material.checkpoint;
            let binding = &material.origin_binding;
            let runtime = &material.origin_runtime;
            let producer = &material.producer_digest;
            assert!(
                material
                    .validate_identity(cp, binding, runtime, producer)
                    .is_ok()
            );
            let mut wrong_cp = cp.clone();
            wrong_cp.snapshot.revision = wrong_cp.snapshot.revision.next().unwrap();
            assert!(matches!(
                material.validate_identity(&wrong_cp, binding, runtime, producer),
                Err(SessionError::StaleRevision)
            ));
            let mut wrong_binding = binding.clone();
            wrong_binding.dispatch = DispatchId::new("different-dispatch").unwrap();
            assert!(matches!(
                material.validate_identity(cp, &wrong_binding, runtime, producer),
                Err(SessionError::StaleRevision)
            ));
            let mut wrong_runtime = runtime.clone();
            wrong_runtime.digest = context_content_digest(b"different runtime");
            assert!(matches!(
                material.validate_identity(cp, binding, &wrong_runtime, producer),
                Err(SessionError::StaleRevision)
            ));
            assert!(matches!(
                material.validate_identity(
                    cp,
                    binding,
                    runtime,
                    &context_content_digest(b"different native producer context")
                ),
                Err(SessionError::StaleRevision)
            ));
            assert!(matches!(
                fresh(
                    material.observed_at,
                    material.max_age,
                    UnixTimestamp::new(200)
                ),
                Err(SessionError::StaleRevision)
            ));
        },
        ReadCase::Current,
        "actual observed data",
    );
}
