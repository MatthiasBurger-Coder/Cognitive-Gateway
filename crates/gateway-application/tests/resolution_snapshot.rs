use gateway_application::{
    DeclarativePlanningApplication, DeclarativeSituationApplication, ProcessSnapshotInput,
    resolution::RequirementAlternatives, resolution_snapshot::*,
};
use gateway_domain::*;
use gateway_process::{
    ProcessInstance, ProcessInstanceId, ProcessInstanceRevision, ProcessRegistry,
};
use gateway_registry::{CapabilityIndex, Registry};
use std::{cell::Cell, collections::BTreeSet, path::PathBuf};

fn fixture() -> ResolutionSnapshotInput {
    let condition = DesiredCondition::new(
        ConditionId::new("condition").unwrap(),
        SubjectPath::new(["quality", "passed"]).unwrap(),
        ComparisonOperator::Equals,
        Some(TypedValue::Boolean(true)),
    )
    .unwrap();
    let desired = DesiredState::new(
        DesiredStateId::new("desired").unwrap(),
        vec![condition],
        ConditionExpression::condition(ConditionId::new("condition").unwrap()),
        vec![],
        vec![],
    )
    .unwrap();
    let current = CurrentState::new_v1(ObservedStateId::new("current").unwrap());
    let situation = SituationAssemblyInput::new(current.clone())
        .assemble(SituationId::new("situation").unwrap())
        .unwrap();
    let delta = DeclarativePlanningApplication::new()
        .derive_delta(
            DeltaId::new("delta").unwrap(),
            &desired,
            &current,
            Some(&situation),
            &ComparisonRules::default(),
            &DeltaDerivationRules::default(),
        )
        .unwrap()
        .delta()
        .clone();
    let requirements = delta
        .items()
        .iter()
        .map(|item| {
            CapabilityRequirement::new(
                CapabilityRequirementId::new("requirement").unwrap(),
                CapabilityId::new("architecture.dependency-analysis").unwrap(),
                RequirementCardinality::Mandatory,
                item.id().clone(),
                "observe unknown state",
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let plan = gateway_domain::plan(&desired, &delta, &requirements, &PlannerRules::default())
        .unwrap()
        .plan()
        .unwrap()
        .clone();
    let document = DeclarativeContextSituationDocument::new(
        DeclarativeContext::new_v1(DeclarativeContextId::new("context").unwrap()),
        Some(Intent::new(
            IntentId::new("intent").unwrap(),
            desired.clone(),
        )),
        None,
        current,
        situation,
    )
    .unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../catalog");
    let registry = Registry::load(&root).unwrap();
    let index = registry.capability_index().unwrap();
    ResolutionSnapshotInput {
        version: SchemaVersion::V1,
        scope: ContextScopeId::new("project-a").unwrap(),
        plan_scope: ContextScopeId::new("project-a").unwrap(),
        situation_scope: ContextScopeId::new("project-a").unwrap(),
        plan,
        desired,
        delta,
        situation: document,
        operating_mode: OperatingMode::Hardening,
        execution_profile: ExecutionProfile::FullPath,
        registry,
        index,
        processes: ProcessRegistry::load(root.join("processes")).unwrap(),
        instance: None,
        expected_revision: None,
        situation_process: None,
        admission: None,
        rule_version: SchemaVersion::V1,
        alternatives: vec![],
    }
}

fn with_process(mut input: ResolutionSnapshotInput) -> ResolutionSnapshotInput {
    let definition = input.processes.definitions().next().unwrap();
    let instance =
        ProcessInstance::start(definition, ProcessInstanceId::new("instance").unwrap()).unwrap();
    input.situation_process = Some(
        DeclarativeSituationApplication::new()
            .process_reference(ProcessSnapshotInput::new(definition, &instance))
            .unwrap(),
    );
    input.expected_revision = Some(instance.revision());
    input.instance = Some(instance);
    input
}

#[test]
fn owned_capture_preserves_all_bases_and_has_no_mutation() {
    struct Counting {
        reads: Cell<u32>,
        input: ResolutionSnapshotInput,
    }
    impl ResolutionSnapshotPort for Counting {
        fn capture(&self) -> Result<ResolutionSnapshotInput, SnapshotError> {
            self.reads.set(self.reads.get() + 1);
            Ok(self.input.clone())
        }
    }
    let mut source = Counting {
        reads: Cell::new(0),
        input: fixture(),
    };
    let snapshot = ResolutionSnapshot::capture(&source).unwrap();
    assert_eq!(source.reads.get(), 1);
    assert_eq!(snapshot.input(), &source.input);
    assert_eq!(snapshot.request().plan(), &source.input.plan);
    assert_eq!(
        snapshot.process_availability(),
        ProcessEvidenceAvailability::AbsentOptional
    );
    assert!(snapshot.process().is_none());
    let same = ResolutionSnapshot::capture(&source.input).unwrap();
    assert!(snapshot.same_basis(&same));
    source.input.operating_mode = OperatingMode::Development;
    assert!(!snapshot.same_basis(&ResolutionSnapshot::capture(&source.input).unwrap()));
    assert_eq!(snapshot.input().operating_mode, OperatingMode::Hardening);
    source.input.execution_profile = ExecutionProfile::FastPath;
    assert!(!same.same_basis(&ResolutionSnapshot::capture(&source.input).unwrap()));
}

#[test]
fn versions_scopes_and_plan_basis_fail_closed() {
    let input = fixture();
    let mut bad = input.clone();
    bad.version = SchemaVersion::V2;
    assert_eq!(
        ResolutionSnapshot::capture(&bad),
        Err(SnapshotError::UnsupportedVersion)
    );
    let mut bad = input.clone();
    bad.rule_version = SchemaVersion::V2;
    assert_eq!(
        ResolutionSnapshot::capture(&bad),
        Err(SnapshotError::UnsupportedVersion)
    );
    let mut bad = input.clone();
    bad.plan_scope = ContextScopeId::new("other").unwrap();
    assert_eq!(
        ResolutionSnapshot::capture(&bad),
        Err(SnapshotError::ScopeMismatch)
    );
    let mut bad = input.clone();
    bad.situation_scope = ContextScopeId::new("other").unwrap();
    assert_eq!(
        ResolutionSnapshot::capture(&bad),
        Err(SnapshotError::ScopeMismatch)
    );
    let mut bad = input.clone();
    bad.plan = Plan::new(
        PlanId::new("p").unwrap(),
        bad.desired.id().clone(),
        DeltaId::new("wrong").unwrap(),
        vec![],
        vec![],
    )
    .unwrap();
    assert_eq!(
        ResolutionSnapshot::capture(&bad),
        Err(SnapshotError::InvalidPlan)
    );
    let mut bad = input.clone();
    let wrong = SituationAssemblyInput::new(bad.situation.observed_state().clone())
        .assemble(SituationId::new("wrong").unwrap())
        .unwrap();
    bad.situation = DeclarativeContextSituationDocument::new(
        bad.situation.context().clone(),
        None,
        None,
        bad.situation.observed_state().clone(),
        wrong,
    )
    .unwrap();
    assert_eq!(
        ResolutionSnapshot::capture(&bad),
        Err(SnapshotError::SituationMismatch)
    );
    struct Unavailable;
    impl ResolutionSnapshotPort for Unavailable {
        fn capture(&self) -> Result<ResolutionSnapshotInput, SnapshotError> {
            Err(SnapshotError::InputUnavailable)
        }
    }
    assert_eq!(
        ResolutionSnapshot::capture(&Unavailable),
        Err(SnapshotError::InputUnavailable)
    );
}

#[test]
fn registry_index_provenance_and_scope_isolation() {
    let input = fixture();
    let mut bad = input.clone();
    bad.index = CapabilityIndex::default();
    assert_eq!(
        ResolutionSnapshot::capture(&bad),
        Err(SnapshotError::MixedIndex)
    );
    let mut equivalent = input.clone();
    let mut agents = input.registry.agents().documents().to_vec();
    agents.reverse();
    let mut skills = input.registry.skills().documents().to_vec();
    skills.reverse();
    equivalent.registry = Registry::from_documents(agents, skills).unwrap();
    equivalent.index = equivalent.registry.capability_index().unwrap();
    let a = ResolutionSnapshot::capture(&input).unwrap();
    assert!(a.same_basis(&ResolutionSnapshot::capture(&equivalent).unwrap()));
    equivalent.scope = ContextScopeId::new("project-b").unwrap();
    equivalent.plan_scope = equivalent.scope.clone();
    equivalent.situation_scope = equivalent.scope.clone();
    let b = ResolutionSnapshot::capture(&equivalent).unwrap();
    assert!(!a.same_basis(&b));
    assert_eq!(
        a.request().basis().registry_fingerprint,
        b.request().basis().registry_fingerprint
    );
    assert!(a.same_basis(&ResolutionSnapshot::capture(&input).unwrap()));
    // A catalog may be constructed before its cross-reference validation.
    bad.registry =
        Registry::from_documents(input.registry.agents().documents().to_vec(), vec![]).unwrap();
    assert_eq!(
        ResolutionSnapshot::capture(&bad),
        Err(SnapshotError::InvalidCatalog)
    );
}

#[test]
fn process_pin_revision_and_projection_are_checked() {
    let input = with_process(fixture());
    let snapshot = ResolutionSnapshot::capture(&input).unwrap();
    assert_eq!(
        snapshot.process_availability(),
        ProcessEvidenceAvailability::Present
    );
    assert_eq!(snapshot.process(), input.situation_process.as_ref());
    assert_eq!(snapshot.input().instance, input.instance);
    let mut bad = input.clone();
    bad.expected_revision = Some(ProcessInstanceRevision::new(9));
    assert_eq!(
        ResolutionSnapshot::capture(&bad),
        Err(SnapshotError::StaleRevision)
    );
    let mut bad = input.clone();
    bad.processes = ProcessRegistry::from_sources([]).unwrap();
    assert_eq!(
        ResolutionSnapshot::capture(&bad),
        Err(SnapshotError::MissingProcessDefinition)
    );
    let mut bad = input.clone();
    bad.instance = None;
    assert_eq!(
        ResolutionSnapshot::capture(&bad),
        Err(SnapshotError::ProcessMismatch)
    );
    let mut bad = input.clone();
    let definition = bad.processes.definitions().next().unwrap();
    bad.instance =
        Some(ProcessInstance::start(definition, ProcessInstanceId::new("other").unwrap()).unwrap());
    assert_eq!(
        ResolutionSnapshot::capture(&bad),
        Err(SnapshotError::ProcessMismatch)
    );
    let mut bad = input.clone();
    let mut wire: serde_json::Value =
        serde_json::from_str(&bad.instance.as_ref().unwrap().to_json().unwrap()).unwrap();
    wire["definition_digest"] = serde_json::json!("a".repeat(64));
    bad.instance = Some(serde_json::from_value(wire).unwrap());
    assert_eq!(
        ResolutionSnapshot::capture(&bad),
        Err(SnapshotError::InvalidProcess)
    );
    let mut without_projection = input.clone();
    without_projection.situation_process = None;
    assert!(snapshot.same_basis(&ResolutionSnapshot::capture(&without_projection).unwrap()));
}

#[test]
fn missing_required_process_and_invalid_groups_remain_explicit() {
    let mut input = fixture();
    let steps = input
        .plan
        .steps()
        .iter()
        .cloned()
        .map(|s| {
            s.with_lifecycle_requirement(
                LifecycleRequirement::new(
                    LifecycleRequirementKind::HumanInput,
                    "needs lifecycle evidence",
                )
                .unwrap(),
            )
        })
        .collect();
    input.plan = Plan::new(
        input.plan.id().clone(),
        input.desired.id().clone(),
        input.delta.id().clone(),
        input.plan.capability_requirements().to_vec(),
        steps,
    )
    .unwrap();
    assert_eq!(
        ResolutionSnapshot::capture(&input)
            .unwrap()
            .process_availability(),
        ProcessEvidenceAvailability::UnavailableRequired
    );
    input.alternatives.push(RequirementAlternatives {
        step: input.plan.steps()[0].id().clone(),
        members: BTreeSet::from([input.plan.capability_requirements()[0].id().clone()]),
        cardinality: RequirementCardinality::Mandatory,
    });
    assert_eq!(
        ResolutionSnapshot::capture(&input),
        Err(SnapshotError::InvalidRequest)
    );
}

#[test]
fn group_semantics_are_hashed_and_group_order_is_not() {
    let mut input = fixture();
    let mut requirements = input.plan.capability_requirements().to_vec();
    for id in ["optional-a", "optional-b", "optional-c"] {
        requirements.push(
            CapabilityRequirement::new(
                CapabilityRequirementId::new(id).unwrap(),
                requirements[0].capability().clone(),
                RequirementCardinality::Optional,
                requirements[0].originating_delta_item().clone(),
                "explicit alternative",
            )
            .unwrap(),
        );
    }
    let ids = requirements
        .iter()
        .map(|r| r.id().clone())
        .collect::<Vec<_>>();
    let step = input.plan.steps()[0]
        .clone()
        .with_capability_requirements(ids.clone())
        .unwrap();
    input.plan = Plan::new(
        input.plan.id().clone(),
        input.desired.id().clone(),
        input.delta.id().clone(),
        requirements,
        vec![step],
    )
    .unwrap();
    let ungrouped = ResolutionSnapshot::capture(&input).unwrap();
    for pair in ids.chunks(2) {
        input.alternatives.push(RequirementAlternatives {
            step: input.plan.steps()[0].id().clone(),
            members: pair.iter().cloned().collect(),
            cardinality: RequirementCardinality::Mandatory,
        });
    }
    let grouped = ResolutionSnapshot::capture(&input).unwrap();
    assert!(!grouped.same_basis(&ungrouped));
    input.alternatives.reverse();
    assert!(grouped.same_basis(&ResolutionSnapshot::capture(&input).unwrap()));
    input.alternatives[0].cardinality = RequirementCardinality::Optional;
    assert!(!grouped.same_basis(&ResolutionSnapshot::capture(&input).unwrap()));
}
