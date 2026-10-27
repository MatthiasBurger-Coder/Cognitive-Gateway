//! Native CG-06 DATA fixtures. No invocation, dispatch grant or goal receipt.
use gateway_application::{
    ScopedObservationBatch, SourceSnapshot,
    connector_results::{ConnectorResult, ConnectorResultMetadata},
    connectors::*,
    sessions::{scoped_snapshots::*, *},
};
use gateway_domain::*;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub type Pin = ResolvedReference<ReferenceId>;

pub fn scope() -> ContextScopeId {
    ContextScopeId::new("complete-snapshot-project").unwrap()
}
pub fn text(value: &str) -> NonEmptyText {
    NonEmptyText::new(value).unwrap()
}
pub fn subject(value: &str) -> SubjectPath {
    SubjectPath::new(value.split('.')).unwrap()
}
pub fn string(value: &str) -> TypedValue {
    TypedValue::String(text(value))
}
pub fn pin(name: &str, contract: &str, body: &Value) -> Pin {
    pin_in_scope(&scope(), name, contract, body)
}
fn pin_in_scope(scope: &ContextScopeId, name: &str, contract: &str, body: &Value) -> Pin {
    serde_json::from_value(json!({
        "id":name,"scope":scope,"contract":contract,"contract_version":"1.0",
        "revision":"revision-1","digest":context_content_digest(&serde_json::to_vec(body).unwrap())
    }))
    .unwrap()
}
pub fn quality() -> QualityMetadata {
    QualityMetadata::new(
        TrustClass::ObservedEvidence,
        SensitivityClass::Internal,
        Confidence::NotApplicable,
        FreshnessStatus::Fresh,
        Uncertainty::None,
    )
}

#[derive(Debug, Clone)]
pub struct NativeRead {
    pub request: ConnectorRequest,
    pub observation: ConnectorObservation,
    pub batch: ScopedObservationBatch,
}

/// All fields originate in actual native records. Callers may later weaken or
/// substitute fixture DATA to verify that the complete wrapper refuses it.
pub fn native_read(
    name: &str,
    object: &str,
    revision: &str,
    acquired_seconds: i64,
    arguments: BTreeMap<NonEmptyText, TypedValue>,
    fields: Vec<(SubjectPath, TypedValue)>,
) -> NativeRead {
    native_read_in_scope(
        scope(),
        name,
        object,
        revision,
        acquired_seconds,
        arguments,
        fields,
    )
}
pub fn native_read_in_scope(
    scope: ContextScopeId,
    name: &str,
    object: &str,
    revision: &str,
    acquired_seconds: i64,
    arguments: BTreeMap<NonEmptyText, TypedValue>,
    fields: Vec<(SubjectPath, TypedValue)>,
) -> NativeRead {
    let source = SourceId::new("shared-state-provider").unwrap();
    let timestamp = SourceTimestamp::new(revision).unwrap();
    let provenance = Provenance::new(
        ProvenanceId::new(format!("{name}.provenance")).unwrap(),
        SourceKind::Tool,
        source.clone(),
        object,
    )
    .unwrap()
    .with_producer("qualified-snapshot-source")
    .unwrap()
    .with_acquired_at(SourceTimestamp::new(acquired_seconds.to_string()).unwrap())
    .with_source_timestamp(timestamp.clone());
    let mut observations = Vec::new();
    let mut facts = Vec::new();
    let mut evidence = Vec::new();
    for (index, (subject, value)) in fields.into_iter().enumerate() {
        let observation = Observation::new(
            ObservationId::new(format!("{name}.observation.{index}")).unwrap(),
            subject.clone(),
            value.clone(),
            provenance.id().clone(),
        )
        .unwrap();
        let fact = Fact::new(
            FactId::new(format!("{name}.fact.{index}")).unwrap(),
            subject,
            value,
            AssertionPolarity::Affirmed,
            vec![observation.id().clone()],
        )
        .unwrap();
        evidence.push(
            Evidence::new(
                EvidenceId::new(format!("{name}.evidence.{index}")).unwrap(),
                EvidenceKind::Report,
                "Qualified snapshot source record",
                EvidenceContent::inline("Native CG-06 state fixture, not a goal verdict").unwrap(),
                provenance.id().clone(),
                vec![EvidenceLink::new(
                    fact.id().clone(),
                    EvidenceRelation::Supports,
                )],
            )
            .unwrap(),
        );
        observations.push(observation);
        facts.push(fact);
    }
    let records =
        ObservationEvidenceSet::new(vec![provenance], observations, facts, evidence).unwrap();
    let digest = context_content_digest(records.to_json().unwrap().as_bytes());
    let request = ConnectorRequest {
        target: ConnectorTarget {
            connector: text("snapshot.provider"),
            scope: scope.clone(),
        },
        capability: CapabilityId::new("snapshot.read").unwrap(),
        class: ConnectorOperationClass::Read,
        inventory_revision: context_content_digest(b"native-fixture-inventory"),
        arguments,
        task: TaskId::new("snapshot-task").unwrap(),
        step: PlanStepId::new("snapshot-step").unwrap(),
        execution: text("snapshot-read-context"),
        correlation: text(name),
    };
    let observation = ConnectorObservation {
        metadata: ConnectorResultMetadata {
            task: request.task.clone(),
            step: request.step.clone(),
            execution: request.execution.clone(),
            object_reference: text(object),
            source_kind: SourceKind::Tool,
            source_digest: Some(digest.clone()),
            observed_at: Some(UnixTimestamp::new(acquired_seconds)),
            authenticated_source: text("qualified-snapshot-source"),
            quality: quality(),
            effect: ConnectorEffect::NoMutation,
            partial: false,
            verifies: None,
            non_authoritative_raw: None,
        },
        target: request.target.clone(),
        inventory_revision: request.inventory_revision.clone(),
        correlation: request.correlation.clone(),
        source,
        source_revision: Some(timestamp),
        content_digest: digest,
        acquired_at: UnixTimestamp::new(acquired_seconds),
        records,
        basis: ConnectorObservationBasis::SourceRead,
    };
    ConnectorResult::validate_for_request(&request, &observation).unwrap();
    let mut batch = ScopedObservationBatch::new(
        scope.clone(),
        SourceSnapshot::new(
            observation.source.clone(),
            observation.metadata.source_kind,
            observation.source_revision.clone(),
            Some(observation.content_digest.clone()),
        )
        .unwrap(),
        observation.records.clone(),
    )
    .unwrap();
    for record in observation.records.observations() {
        batch = batch.with_quality_metadata(record.subject().clone(), vec![quality()]);
    }
    NativeRead {
        request,
        observation,
        batch,
    }
}

#[derive(Clone)]
pub struct Fixture {
    pub binding: SnapshotBinding,
    pub universe: SnapshotUniverse,
    pub profile: SnapshotProfile,
    pub acquisitions: Vec<SnapshotAcquisition>,
    pub reads: Vec<SnapshotObjectRead>,
    pub consistency: SnapshotConsistency,
    pub now_ms: u64,
}
pub fn repin_route(route: &mut SnapshotReadRoute) {
    route.pin.digest = context_content_digest(&route.body_bytes().unwrap());
}
impl Fixture {
    pub fn atomic() -> Self {
        Self::build(SnapshotProfileKind::AtomicCompleteExport)
    }
    pub fn versioned() -> Self {
        Self::build(SnapshotProfileKind::SnapshotScopedReads)
    }
    pub fn bracketed() -> Self {
        Self::build(SnapshotProfileKind::BracketedNonReusedAnchor)
    }
    pub fn validate(&self) -> Result<CompleteScopedSnapshot, SnapshotError> {
        CompleteScopedSnapshot::validate(
            self.binding.clone(),
            self.universe.clone(),
            self.profile.clone(),
            self.acquisitions.clone(),
            self.reads.clone(),
            self.consistency.clone(),
            self.now_ms,
        )
    }
    pub fn repin_universe(&mut self) {
        self.universe.pin.digest = context_content_digest(&self.universe.body_bytes().unwrap());
    }
    pub fn repin_profile(&mut self) {
        self.profile.pin.digest = context_content_digest(&self.profile.body_bytes().unwrap());
    }
    pub fn replace_acquisition(&mut self, index: usize, value: SnapshotAcquisition) {
        let old = self.acquisitions[index].reference().unwrap();
        let new = value.reference().unwrap();
        for read in &mut self.reads {
            if read.acquisition == old {
                read.acquisition = new.clone();
            }
        }
        let replace = |anchor: &mut SnapshotAnchor| {
            if anchor.acquisition == old {
                anchor.acquisition = new.clone();
            }
        };
        match &mut self.consistency {
            SnapshotConsistency::Atomic { anchor }
            | SnapshotConsistency::SnapshotScoped { anchor } => replace(anchor),
            SnapshotConsistency::Bracketed { before, after } => {
                replace(before);
                replace(after);
            }
        }
        self.acquisitions[index] = value;
    }
    pub fn in_scope(kind: SnapshotProfileKind, scope: ContextScopeId) -> Self {
        Self::build_in_scope(kind, scope)
    }
    pub fn atomic_in_scope(scope: ContextScopeId) -> Self {
        Self::build_in_scope(SnapshotProfileKind::AtomicCompleteExport, scope)
    }
    fn build(kind: SnapshotProfileKind) -> Self {
        Self::build_in_scope(kind, scope())
    }
    fn build_in_scope(kind: SnapshotProfileKind, scope: ContextScopeId) -> Self {
        let atomic = kind == SnapshotProfileKind::AtomicCompleteExport;
        let scoped = kind == SnapshotProfileKind::SnapshotScopedReads;
        let source = SourceId::new("shared-state-provider").unwrap();
        let authority_domain = pin_in_scope(
            &scope,
            "global-epoch-authority",
            "cg.snapshot-global-authority",
            &json!({"scope":scope.clone(),"epoch_owner":"qualified-state-provider"}),
        );
        let mut export = SnapshotReadRoute {
            pin: pin_in_scope(
                &scope,
                "export-route",
                "cg.scoped-snapshot-read-route",
                &json!({"role":"export"}),
            ),
            target: ConnectorTarget {
                connector: text("snapshot.provider"),
                scope: scope.clone(),
            },
            authority_domain: authority_domain.clone(),
            capability: CapabilityId::new("snapshot.read").unwrap(),
            class: ConnectorOperationClass::Read,
            arguments: BTreeMap::from([(text("scope"), string(scope.clone().as_str()))]),
            object_reference: text("project-state-export"),
            epoch_argument: None,
        };
        repin_route(&mut export);
        let mut objects = Vec::new();
        let mut values = Vec::new();
        for (name, version, ready, non_goal) in [
            ("one", "object-v3", true, 128),
            ("two", "object-v9", false, 4096),
        ] {
            let version_subject = subject(&format!("state.{name}.version"));
            let fields = vec![
                (
                    subject(&format!("state.{name}.ready")),
                    TypedValue::Boolean(ready),
                ),
                (
                    subject(&format!("state.{name}.bytes")),
                    TypedValue::Integer(non_goal),
                ),
                (version_subject.clone(), string(version)),
            ];
            let subjects = fields
                .iter()
                .map(|(s, _)| s.clone())
                .collect::<BTreeSet<_>>();
            let object_reference = format!("object-{name}");
            let object = pin_in_scope(
                &scope,
                &object_reference,
                "cg.connector-object",
                &json!({
                    "schema_version":"1.0","id":object_reference,"scope":scope.clone(),"kind":"ENTITY",
                    "object_reference":object_reference,"observable_subjects":fields.iter().map(|(s,_)|
                        s.segments().iter().map(NonEmptyText::as_str).collect::<Vec<_>>()).collect::<Vec<_>>()
                }),
            );
            let route = if atomic {
                export.clone()
            } else {
                let mut route = SnapshotReadRoute {
                    pin: pin_in_scope(
                        &scope,
                        &format!("route-{name}"),
                        "cg.scoped-snapshot-read-route",
                        &json!({"object":name}),
                    ),
                    target: export.target.clone(),
                    authority_domain: authority_domain.clone(),
                    capability: export.capability.clone(),
                    class: export.class,
                    arguments: BTreeMap::from([(text("object"), string(&object_reference))]),
                    object_reference: text(&object_reference),
                    epoch_argument: scoped.then(|| text("snapshot_epoch")),
                };
                repin_route(&mut route);
                route
            };
            objects.push(SnapshotObject {
                object,
                source: source.clone(),
                source_kind: SourceKind::Tool,
                object_reference: text(&object_reference),
                subjects,
                version_subject,
                route,
            });
            values.push(fields);
        }
        let mut universe = SnapshotUniverse {
            pin: pin_in_scope(
                &scope,
                "scope-universe",
                "cg.scoped-snapshot-universe",
                &json!({"scope":scope.clone()}),
            ),
            objects,
        };
        universe.pin.digest = context_content_digest(&universe.body_bytes().unwrap());
        let mut profile = SnapshotProfile {
            pin: pin_in_scope(
                &scope,
                "snapshot-profile",
                "cg.scoped-snapshot-profile",
                &json!({"scope":scope.clone()}),
            ),
            authority_domain,
            kind,
            anchor_source: source,
            anchor_source_kind: SourceKind::Tool,
            anchor_route: export.clone(),
            epoch_subject: subject("scope.global_epoch"),
            membership_subject: subject("scope.membership"),
            max_age_seconds: 30,
        };
        profile.pin.digest = context_content_digest(&profile.body_bytes().unwrap());
        let anchor_fields = vec![
            (profile.epoch_subject.clone(), string("epoch-42")),
            (
                profile.membership_subject.clone(),
                string(universe.membership_digest().unwrap().as_str()),
            ),
        ];
        let read = |name: &str,
                    route: &SnapshotReadRoute,
                    version: &str,
                    at: i64,
                    fields: Vec<(SubjectPath, TypedValue)>| {
            let mut arguments = route.arguments.clone();
            if let Some(key) = &route.epoch_argument {
                arguments.insert(key.clone(), string("epoch-42"));
            }
            let native = native_read_in_scope(
                scope.clone(),
                name,
                route.object_reference.as_str(),
                version,
                at,
                arguments,
                fields,
            );
            SnapshotAcquisition {
                request: native.request,
                observation: native.observation,
                batch: native.batch,
            }
        };
        let (acquisitions, read_indices) = if atomic {
            let mut all = values.iter().flatten().cloned().collect::<Vec<_>>();
            all.extend(anchor_fields.clone());
            (
                vec![read("atomic-export", &export, "epoch-42", 100, all)],
                vec![0, 0],
            )
        } else {
            let mut acquisitions = vec![read(
                "anchor-before",
                &export,
                "epoch-42",
                99,
                anchor_fields.clone(),
            )];
            for (index, fields) in values.into_iter().enumerate() {
                acquisitions.push(read(
                    &format!("object-read-{index}"),
                    &universe.objects[index].route,
                    if index == 0 { "object-v3" } else { "object-v9" },
                    100 + index as i64,
                    fields,
                ));
            }
            if !scoped {
                acquisitions.push(read(
                    "anchor-after",
                    &export,
                    "epoch-42",
                    102,
                    anchor_fields,
                ));
            }
            (acquisitions, vec![1, 2])
        };
        let anchor = |index: usize| {
            let acquisition = &acquisitions[index];
            let find = |subject: &SubjectPath| {
                acquisition
                    .observation
                    .records
                    .observations()
                    .iter()
                    .find(|o| o.subject() == subject)
                    .unwrap()
                    .id()
                    .clone()
            };
            SnapshotAnchor {
                domain: profile.authority_domain.clone(),
                epoch: SourceTimestamp::new("epoch-42").unwrap(),
                membership: universe.membership_digest().unwrap(),
                acquisition: acquisition.reference().unwrap(),
                epoch_observation: find(&profile.epoch_subject),
                membership_observation: find(&profile.membership_subject),
            }
        };
        let consistency = match kind {
            SnapshotProfileKind::AtomicCompleteExport => {
                SnapshotConsistency::Atomic { anchor: anchor(0) }
            }
            SnapshotProfileKind::SnapshotScopedReads => {
                SnapshotConsistency::SnapshotScoped { anchor: anchor(0) }
            }
            SnapshotProfileKind::BracketedNonReusedAnchor => SnapshotConsistency::Bracketed {
                before: anchor(0),
                after: Box::new(anchor(3)),
            },
        };
        let reads = universe
            .objects
            .iter()
            .zip(read_indices)
            .map(|(object, index)| SnapshotObjectRead {
                object: object.object.clone(),
                source: object.source.clone(),
                source_kind: object.source_kind,
                acquisition: acquisitions[index].reference().unwrap(),
                version_observation: acquisitions[index]
                    .observation
                    .records
                    .observations()
                    .iter()
                    .find(|v| v.subject() == &object.version_subject)
                    .unwrap()
                    .id()
                    .clone(),
            })
            .collect();
        Self {
            binding: SnapshotBinding {
                capture: ResourceReservationBinding {
                    owner: OwnerBinding {
                        principal: PrincipalId::new("snapshot-owner").unwrap(),
                        workspace: WorkspaceId::new("workspace").unwrap(),
                        project: ProjectId::new("project").unwrap(),
                        binding: BindingId::new("binding").unwrap(),
                        client_owner: ClientOwnerId::new("client").unwrap(),
                    },
                    scope: scope.clone(),
                    session: SessionId::new("snapshot-session").unwrap(),
                    run: RunId::new("snapshot-run").unwrap(),
                    revision: Revision::new(7).unwrap(),
                    step: PlanStepId::new("snapshot-step").unwrap(),
                    dispatch: DispatchId::new("snapshot-capture").unwrap(),
                },
                phase: SnapshotPhase::Before,
                task: TaskId::new("snapshot-task").unwrap(),
                execution: text("snapshot-read-context"),
                reservation: None,
            },
            universe,
            profile,
            acquisitions,
            reads,
            consistency,
            now_ms: 103_000,
        }
    }
}
