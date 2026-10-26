use super::*;
use crate::mcp_client::{
    ProtocolInventory,
    mapping::{DiscoveryMapper, discovery_schema_digest},
};
use gateway_application::{
    connector_manifest::ConnectorManifest,
    sessions::{ConditionObservationMapping, content_reference},
};
use gateway_domain::*;
use serde_json::json;
fn pin(id: &str, contract: &str, value: &Value) -> Pin {
    Pin {
        id: ReferenceId::new(id).unwrap(),
        scope: ContextScopeId::new("project-x").unwrap(),
        contract: ReferenceId::new(contract).unwrap(),
        contract_version: SchemaVersion::V1,
        revision: ReferenceId::new("1").unwrap(),
        digest: context_content_digest(&serde_json::to_vec(value).unwrap()),
    }
}
fn object(
    id: &str,
    kind: TargetKind,
    parent: Option<&Pin>,
    subjects: &[SubjectPath],
    reference: &str,
) -> ScopeObjectDocument {
    let value = json!({"schema_version":"1.0","id":id,"revision":"1","scope":"project-x","kind":kind,"connector":"example.records","parent_target":parent,"subjects":subjects,"coordinates":{"id":{"kind":"STRING","value":reference}},"object_reference":id});
    ScopeObjectDocument::from_json(
        &pin(id, "cg.connector-object", &value),
        &serde_json::to_vec(&value).unwrap(),
    )
    .unwrap()
}
struct Fixture {
    goal: SupportedConnectorGoal,
    task: SemanticTaskIR,
    task_pin: Pin,
    projection: Pin,
    step: PlanStepId,
    execution: NonEmptyText,
    mapping: Value,
    mapping_pin: Pin,
    inventory: MappedInventory,
    schemas: BTreeMap<CapabilityId, MappedToolSchema>,
    subjects: Vec<ReadSubjectBinding>,
    objects: Vec<ScopeObjectDocument>,
    association: Value,
    export: Option<Pin>,
}
impl Fixture {
    fn new() -> Self {
        Self::with_result_schema(
            json!({"type":"object","additionalProperties":false,"properties":{"object":{"type":"string"},"id":{"type":"string"},"ready":{"type":"boolean"}},"required":["object","id","ready"]}),
        )
    }
    fn with_result_schema(result_schema: Value) -> Self {
        Self::with_shapes(result_schema, "string")
    }
    fn with_shapes(result_schema: Value, first_query_type: &str) -> Self {
        let subjects = [
            SubjectPath::new(["state", "first"]).unwrap(),
            SubjectPath::new(["state", "second"]).unwrap(),
        ];
        let target = object(
            "target-project",
            TargetKind::Project,
            None,
            &subjects,
            "project-x",
        );
        let first = object(
            "object-one",
            TargetKind::Artifact,
            Some(&target.pin),
            &subjects[..1],
            "one",
        );
        let second = object(
            "object-two",
            TargetKind::Artifact,
            Some(&target.pin),
            &subjects[1..],
            "two",
        );
        let objects = vec![target, first, second];
        let conditions = subjects
            .iter()
            .enumerate()
            .map(|(i, s)| {
                DesiredCondition::new(
                    ConditionId::new(format!("condition-{i}")).unwrap(),
                    s.clone(),
                    ComparisonOperator::Equals,
                    Some(TypedValue::Boolean(true)),
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        let desired = DesiredState::new(
            DesiredStateId::new("complete-goal").unwrap(),
            conditions.clone(),
            ConditionExpression::all(
                conditions
                    .iter()
                    .map(|c| ConditionExpression::condition(c.id().clone()))
                    .collect(),
            )
            .unwrap(),
            vec![],
            vec![],
        )
        .unwrap();
        let step = PlanStepId::new("read-step").unwrap();
        let goal = SupportedConnectorGoal::from_host_registration(
            Intent::new(
                IntentId::new("observe-whole-project").unwrap(),
                desired.clone(),
            ),
            ContextScopeId::new("project-x").unwrap(),
            content_reference("registered-plan", b"association-data-fixture").unwrap(),
            vec![step.clone()],
            conditions
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    ConditionObservationMapping::new(
                        c.id().clone(),
                        SourceId::new(format!("source-{i}")).unwrap(),
                        CapabilityId::new(if i == 0 {
                            "records.read"
                        } else {
                            "records.second"
                        })
                        .unwrap(),
                        ConnectorOperationClass::Read,
                    )
                    .unwrap()
                })
                .collect(),
        )
        .unwrap();
        let mut task: Value = serde_json::from_slice(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/semantic-task-v1/minimal.json"
        )))
        .unwrap();
        task["target"] = json!({"kind":"PROJECT","reference":objects[0].pin});
        task["desired_state"] = json!(desired);
        task["inputs"] = json!([{"role":"first","value":{"kind":"REFERENCE","value":objects[1].pin}},{"role":"second","value":{"kind":"REFERENCE","value":objects[2].pin}},{"role":"limit","value":{"kind":"LITERAL","value":{"kind":"INTEGER","value":3}}}]);
        let task: SemanticTaskIR = serde_json::from_value(task).unwrap();
        let mut task_pin = pin(task.data().id.as_str(), "cg.semantic-task", &json!({}));
        task_pin.digest = task.content_digest().unwrap();
        let projection = pin(
            "read-projection",
            "cg.context-projection",
            &json!({"id":"read-context","step":"read-step"}),
        );
        let execution = NonEmptyText::new("read-context").unwrap();
        let mut mapping: Value = serde_json::from_slice(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/connector-mapping-v1/mapping.json"
        )))
        .unwrap();
        mapping["records"][0]["result_schema"] = result_schema;
        let mut second = mapping["records"][0].clone();
        second["reference"] = json!("mapping.records.second.v1");
        second["capability"] = json!("records.second");
        second["name"] = json!("records.second");
        mapping["records"].as_array_mut().unwrap().push(second);
        let mapping_pin = pin("mapping", "cg.connector-mapping", &mapping);
        let mut discovery: Value = serde_json::from_slice(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/connector-mapping-v1/discovery.json"
        )))
        .unwrap();
        discovery["tools"][0]["inputSchema"]["title"] = json!("Read exact object");
        discovery["tools"][0]["inputSchema"]["properties"]["limit"] = json!({"type":"integer"});
        discovery["tools"][0]["inputSchema"]["required"] = json!(["query", "limit"]);
        let mut second = discovery["tools"][0].clone();
        second["name"] = json!("records.second");
        discovery["tools"].as_array_mut().unwrap().push(second);
        discovery["tools"][0]["inputSchema"]["properties"]["query"] =
            json!({"type":first_query_type});
        let wire = ProtocolInventory {
            tools: discovery["tools"].as_array().unwrap().clone(),
            resources: discovery["resources"].as_array().unwrap().clone(),
            prompts: discovery["prompts"].as_array().unwrap().clone(),
            templates: discovery["templates"].as_array().unwrap().clone(),
        };
        let envelope: Value = serde_json::from_slice(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/connector-mapping-v1/manifest.json"
        )))
        .unwrap();
        let mut manifest = envelope["manifest"].clone();
        let mut second = manifest["inventory"][0].clone();
        second["name"] = json!("records.second");
        second["capability_mapping_ref"] = json!("mapping.records.second.v1");
        manifest["inventory"].as_array_mut().unwrap().push(second);
        for entry in manifest["inventory"].as_array_mut().unwrap() {
            let kind: InventoryKind = serde_json::from_value(entry["kind"].clone()).unwrap();
            let items = match kind {
                InventoryKind::Tool => &wire.tools,
                InventoryKind::Resource => &wire.resources,
                InventoryKind::Prompt => &wire.prompts,
            };
            if let Some(item) = items.iter().find(|v| v["name"] == entry["name"]) {
                entry["schema_digest"] = json!(discovery_schema_digest(kind, item));
            }
        }
        let mapper = DiscoveryMapper::new(
            ConnectorTarget {
                connector: NonEmptyText::new("example.records").unwrap(),
                scope: ContextScopeId::new("project-x").unwrap(),
            },
            ConnectorManifest::from_definition_json(&serde_json::to_vec(&manifest).unwrap())
                .unwrap(),
            MappingDocument::from_json(&serde_json::to_vec(&mapping).unwrap()).unwrap(),
        )
        .unwrap();
        let inventory = mapper.snapshot(&wire).unwrap();
        let schemas = ["records.read", "records.second"]
            .into_iter()
            .map(|id| {
                let cap = CapabilityId::new(id).unwrap();
                let schema = mapper
                    .tool_schema(&wire, &cap, &inventory.inventory().revision)
                    .unwrap();
                (cap, schema)
            })
            .collect();
        let subjects = (0..2)
            .map(|i| ReadSubjectBinding {
                mapping_reference: if i == 0 {
                    "mapping.records.read.v1".into()
                } else {
                    "mapping.records.second.v1".into()
                },
                arguments: BTreeMap::from([
                    (
                        NonEmptyText::new("query").unwrap(),
                        TypedValue::string(if i == 0 { "one" } else { "two" }).unwrap(),
                    ),
                    (NonEmptyText::new("limit").unwrap(), TypedValue::Integer(3)),
                ]),
                source: SourceId::new(format!("source-{i}")).unwrap(),
                object_field: "object".into(),
                condition_ids: BTreeSet::from(
                    [ConditionId::new(format!("condition-{i}")).unwrap()],
                ),
                subjects: BTreeSet::from([subjects[i].clone()]),
            })
            .collect();
        let operations=(0..2).map(|i|json!({"mapping_reference":if i==0{"mapping.records.read.v1"}else{"mapping.records.second.v1"},"purpose":"FIXED_READ","profile":"EXACT_TASK_OPERANDS_V1","subject":{"kind":"INPUT_REFERENCE","role":if i==0{"first"}else{"second"}},"object":objects[i+1].pin,"selector_arguments":{"query":"id"},"literal_arguments":{"limit":"limit"},"fixed_arguments":{},"response":{"object_field":"object","identity_fields":{"id":"id"}}})).collect::<Vec<_>>();
        let association = json!({"schema_version":"1.0","scope":"project-x","task":task_pin,"projection":projection,"mapping":mapping_pin,"step":"read-step","connector":"example.records","operations":operations});
        Self {
            goal,
            task,
            task_pin,
            projection,
            step,
            execution,
            mapping,
            mapping_pin,
            inventory,
            schemas,
            subjects,
            objects,
            association,
            export: None,
        }
    }
    fn admit(&self) -> Result<BoundReadAssociation, SessionError> {
        ReadAssociation::from_json(
            &pin(
                "association",
                "cg.connector-read-association",
                &self.association,
            ),
            &serde_json::to_vec(&self.association).unwrap(),
        )?
        .admit(ReadAssociationContext {
            goal: &self.goal,
            task_pin: &self.task_pin,
            task: &self.task,
            projection: &self.projection,
            step: &self.step,
            execution: &self.execution,
            mapping_pin: &self.mapping_pin,
            mapping_document: &self.mapping,
            inventory: &self.inventory,
            schemas: &self.schemas,
            subjects: &self.subjects,
            objects: &self.objects,
            scope_export: self.export.as_ref(),
        })
    }
    fn request(&self, i: usize) -> ConnectorRequest {
        ConnectorRequest {
            target: self.inventory.inventory().target.clone(),
            capability: CapabilityId::new(if i == 0 {
                "records.read"
            } else {
                "records.second"
            })
            .unwrap(),
            class: ConnectorOperationClass::Read,
            inventory_revision: self.inventory.inventory().revision.clone(),
            arguments: BTreeMap::from([
                (
                    NonEmptyText::new("query").unwrap(),
                    TypedValue::string(if i == 0 { "one" } else { "two" }).unwrap(),
                ),
                (NonEmptyText::new("limit").unwrap(), TypedValue::Integer(3)),
            ]),
            task: self.task.data().id.clone(),
            step: self.step.clone(),
            execution: self.execution.clone(),
            correlation: NonEmptyText::new(format!("read-{i}")).unwrap(),
        }
    }
    fn refresh_task(&mut self, value: Value) {
        self.task = serde_json::from_value(value).unwrap();
        self.task_pin.digest = self.task.content_digest().unwrap();
        self.association["task"] = json!(self.task_pin);
    }
}
fn join<'a>(mapping: &'a str, request: &'a ConnectorRequest) -> FixedReadJoin<'a> {
    FixedReadJoin {
        mapping_reference: mapping,
        source: None,
        arguments: &request.arguments,
        scope_export: None,
    }
}
#[test]
fn fullgoal_multiple_native_input_objects_have_separate_exact_operands_and_response_membership() {
    let f = Fixture::new();
    let bound = f.admit().unwrap();
    for i in 0..2 {
        let request = f.request(i);
        let mapping = if i == 0 {
            "mapping.records.read.v1"
        } else {
            "mapping.records.second.v1"
        };
        let matched = bound
            .validate_fixed_read(&request, join(mapping, &request))
            .unwrap();
        matched.validate_response(&ProtocolResult::Tool{content:vec![],structured:Some(json!({"object":if i==0{"object-one"}else{"object-two"},"id":if i==0{"one"}else{"two"},"ready":true})),is_error:false}).unwrap();
        for body in [
            json!({"object":"unrelated-object","id":"one"}),
            json!({"object":"object-one","id":"foreign-row"}),
        ] {
            let error = matched
                .validate_response(&ProtocolResult::Tool {
                    content: vec![],
                    structured: Some(body),
                    is_error: false,
                })
                .unwrap_err();
            assert_eq!(error.effect, ConnectorEffect::NoMutation);
            assert_eq!(error.correlation, request.correlation);
        }
    }
}
#[test]
fn request_coordinate_scope_inventory_step_execution_and_source_substitutions_are_denied() {
    let f = Fixture::new();
    let bound = f.admit().unwrap();
    for field in [
        "target",
        "scope",
        "inventory",
        "step",
        "execution",
        "task",
        "class",
        "capability",
        "arguments",
        "source",
    ] {
        let mut request = f.request(0);
        match field {
            "target" => request.target.connector = NonEmptyText::new("foreign").unwrap(),
            "scope" => request.target.scope = ContextScopeId::new("foreign").unwrap(),
            "inventory" => {
                request.inventory_revision = context_content_digest(b"foreign inventory")
            }
            "step" => request.step = PlanStepId::new("foreign-step").unwrap(),
            "execution" => request.execution = NonEmptyText::new("foreign-context").unwrap(),
            "task" => request.task = TaskId::new("foreign-task").unwrap(),
            "class" => request.class = ConnectorOperationClass::Mutate,
            "capability" => request.capability = CapabilityId::new("records.second").unwrap(),
            "arguments" => {
                request.arguments.insert(
                    NonEmptyText::new("query").unwrap(),
                    TypedValue::string("two").unwrap(),
                );
            }
            _ => (),
        };
        let source = SourceId::new("foreign-source").unwrap();
        let mut read = join("mapping.records.read.v1", &request);
        if field == "source" {
            read.source = Some(&source);
        }
        assert!(
            bound.validate_fixed_read(&request, read).is_err(),
            "{field}"
        );
    }
}
#[test]
fn same_scope_unrelated_objects_parent_joins_and_cross_object_subjects_are_denied() {
    for case in [
        "object",
        "parent",
        "subject",
        "missing-condition",
        "reduced-goal",
        "literal",
    ] {
        let mut f = Fixture::new();
        match case {
            "object" => f.association["operations"][0]["object"] = json!(f.objects[2].pin),
            "parent" => {
                let subjects = f.objects[1].body.subjects.clone();
                f.objects[1] = object(
                    "object-one",
                    TargetKind::Artifact,
                    Some(&f.objects[2].pin),
                    &subjects,
                    "one",
                );
                f.association["operations"][0]["object"] = json!(f.objects[1].pin);
                let mut task = json!(f.task);
                task["inputs"][0]["value"]["value"] = json!(f.objects[1].pin);
                f.refresh_task(task);
            }
            "subject" => {
                f.subjects[0].subjects =
                    BTreeSet::from([SubjectPath::new(["state", "second"]).unwrap()]);
            }
            "missing-condition" => {
                f.association["operations"].as_array_mut().unwrap().pop();
            }
            "reduced-goal" => {
                let mut task = json!(f.task);
                task["desired_state"]["conditions"]
                    .as_array_mut()
                    .unwrap()
                    .pop();
                task["desired_state"]["expression"] =
                    json!({"kind":"CONDITION","value":"condition-0"});
                f.refresh_task(task);
            }
            _ => {
                f.association["operations"][0]["literal_arguments"]["limit"] = json!("first");
            }
        }
        assert!(f.admit().is_err(), "{case}");
    }
}
#[test]
fn arbitrary_constants_query_profiles_empty_selectors_and_metadata_pins_are_fail_closed() {
    for case in [
        "fixed",
        "dsl",
        "no-selector",
        "partial-selector",
        "response",
        "task-pin",
        "projection-pin",
        "mapping-pin",
        "raw-schema",
    ] {
        let mut f = Fixture::new();
        match case {
            "fixed" => {
                f.association["operations"][0]["fixed_arguments"] =
                    json!({"limit":{"kind":"INTEGER","value":3}})
            }
            "dsl" => f.association["operations"][0]["profile"] = json!("QUERY_DSL"),
            "no-selector" => f.association["operations"][0]["selector_arguments"] = json!({}),
            "partial-selector" => {
                let v = json!({"schema_version":"1.0","id":"object-one","revision":"1","scope":"project-x","kind":"ARTIFACT","connector":"example.records","parent_target":f.objects[0].pin,"subjects":f.objects[1].body.subjects,"coordinates":{"id":{"kind":"STRING","value":"one"},"tenant":{"kind":"STRING","value":"tenant"}},"object_reference":"object-one"});
                let p = pin("object-one", "cg.connector-object", &v);
                f.objects[1] =
                    ScopeObjectDocument::from_json(&p, &serde_json::to_vec(&v).unwrap()).unwrap();
                f.association["operations"][0]["object"] = json!(p);
                let mut task = json!(f.task);
                task["inputs"][0]["value"]["value"] = json!(p);
                f.refresh_task(task);
            }
            "response" => f.association["operations"][0]["response"]["identity_fields"] = json!({}),
            "task-pin" => f.association["task"]["revision"] = json!("2"),
            "projection-pin" => {
                f.association["projection"]["digest"] = json!(context_content_digest(b"changed"))
            }
            "mapping-pin" => f.mapping_pin.digest = context_content_digest(b"changed"),
            _ => {
                f.schemas
                    .get_mut(&CapabilityId::new("records.read").unwrap())
                    .unwrap()
                    .argument_schema["title"] = json!("changed");
            }
        }
        assert!(f.admit().is_err(), "{case}");
    }
}
#[test]
fn typed_document_edges_and_input_parser_reject_duplicate_fields_inline_secrets_and_drift() {
    let f = Fixture::new();
    let p = pin(
        "association",
        "cg.connector-read-association",
        &f.association,
    );
    let data = serde_json::to_vec(&f.association).unwrap();
    let association = ReadAssociation::from_json(&p, &data).unwrap();
    let edges = association.reference_bindings();
    assert!(edges.contains(&f.task_pin));
    assert!(edges.contains(&f.projection));
    assert!(edges.contains(&f.mapping_pin));
    assert!(edges.contains(&f.objects[1].pin));
    assert_eq!(
        f.objects[1].reference_bindings(),
        vec![f.objects[0].pin.clone()]
    );
    let mut changed = f.association.clone();
    changed["step"] = json!("other-step");
    assert!(ReadAssociation::from_json(&p, &serde_json::to_vec(&changed).unwrap()).is_err());
    let mut changed = f.association.clone();
    changed["credentials"] = json!("inline forbidden");
    assert!(
        ReadAssociation::from_json(
            &pin("association", "cg.connector-read-association", &changed),
            &serde_json::to_vec(&changed).unwrap()
        )
        .is_err()
    );
    let duplicated = "{\"scope\":\"project-x\",\"scope\":\"project-x\"}".to_owned();
    assert!(ReadAssociation::from_json(&p, duplicated.as_bytes()).is_err());
    assert!(ReadAssociation::from_json(&p, &vec![b'x'; BYTES + 1]).is_err());
}

#[test]
fn task_target_and_explicit_scope_export_use_actual_qualified_objects_without_action_contexts() {
    for exported in [false, true] {
        let mut f = Fixture::new();
        if exported {
            let provider = pin(
                "qualified-universe",
                "cg.connector-state-universe",
                &json!({"scope":"project-x","contract":"qualified-provider"}),
            );
            f.export = Some(provider.clone());
            f.objects[1] = object(
                "scope-export",
                TargetKind::Project,
                Some(&f.objects[0].pin),
                &f.objects[1].body.subjects.clone(),
                "one",
            );
            f.association["operations"][0]["subject"] =
                json!({"kind":"SCOPE_EXPORT","provider_contract":provider});
            f.association["operations"][0]["object"] = json!(f.objects[1].pin);
        } else {
            let subjects = f.objects[0].body.subjects.clone();
            let target = object("read-target", TargetKind::Artifact, None, &subjects, "one");
            let mut task = json!(f.task);
            task["target"] = json!({"kind":"ARTIFACT","reference":target.pin});
            f.objects[0] = target;
            for operation in f.association["operations"].as_array_mut().unwrap() {
                operation["subject"] = json!({"kind":"TASK_TARGET"});
                operation["object"] = json!(f.objects[0].pin);
            }
            f.refresh_task(task);
            for route in &mut f.subjects {
                route.arguments.insert(
                    NonEmptyText::new("query").unwrap(),
                    TypedValue::string("one").unwrap(),
                );
            }
        }
        let parsed = ReadAssociation::from_json(
            &pin(
                "association",
                "cg.connector-read-association",
                &f.association,
            ),
            &serde_json::to_vec(&f.association).unwrap(),
        )
        .unwrap();
        if let Some(provider) = &f.export {
            assert!(
                parsed.reference_bindings().contains(provider),
                "qualified provider must be retained by typed document closure"
            );
        }
        let bound = f.admit().unwrap();
        for i in 0..2 {
            let mut request = f.request(i);
            if !exported {
                request.arguments.insert(
                    NonEmptyText::new("query").unwrap(),
                    TypedValue::string("one").unwrap(),
                );
            }
            let mapping = if i == 0 {
                "mapping.records.read.v1"
            } else {
                "mapping.records.second.v1"
            };
            let mut read = join(mapping, &request);
            if exported && i == 0 {
                read.scope_export = f.export.as_ref();
            }
            assert_eq!(
                bound.fixed_read_export(&request, mapping, None).unwrap(),
                read.scope_export
            );
            let mut foreign_execution = request.clone();
            foreign_execution.execution = NonEmptyText::new("other-execution").unwrap();
            assert!(
                bound
                    .fixed_read_export(&foreign_execution, mapping, None)
                    .is_err()
            );
            let matched = bound.validate_fixed_read(&request, read).unwrap();
            let reference = if exported {
                if i == 0 { "scope-export" } else { "object-two" }
            } else {
                "read-target"
            };
            let id = if !exported || i == 0 { "one" } else { "two" };
            matched
                .validate_response(&ProtocolResult::Tool {
                    content: vec![],
                    structured: Some(json!({"object":reference,"id":id,"ready":true})),
                    is_error: false,
                })
                .unwrap();
        }
        let request = f.request(0);
        if exported {
            assert!(
                bound
                    .validate_fixed_read(&request, join("mapping.records.read.v1", &request))
                    .is_err()
            );
        }
    }
}
#[test]
fn exact_resource_uri_read_uses_native_input_and_canonical_goal_source() {
    let mut f = Fixture::new();
    let subjects = f.objects[1].body.subjects.clone();
    let body = json!({"schema_version":"1.0","id":"resource-one","revision":"1","scope":"project-x","kind":"ARTIFACT","connector":"example.records","parent_target":f.objects[0].pin,"subjects":subjects,"coordinates":{},"object_reference":"records://one"});
    let p = pin("resource-one", "cg.connector-object", &body);
    f.objects[1] = ScopeObjectDocument::from_json(&p, &serde_json::to_vec(&body).unwrap()).unwrap();
    let mut task = json!(f.task);
    task["inputs"][0]["value"]["value"] = json!(p);
    f.refresh_task(task);
    f.association["operations"][0]["object"] = json!(p);
    f.association["operations"][0]["mapping_reference"] = json!("mapping.records.source.v1");
    f.association["operations"][0]["selector_arguments"] = json!({});
    f.association["operations"][0]["literal_arguments"] = json!({});
    f.association["operations"][0]["response"]["identity_fields"] = json!({});
    f.subjects[0].mapping_reference = "mapping.records.source.v1".into();
    f.subjects[0].arguments.clear();
    f.subjects[0].source = SourceId::new("records.one").unwrap();
    let mut observations = f.goal.observations().to_vec();
    observations[0] = ConditionObservationMapping::new(
        observations[0].condition().clone(),
        SourceId::new("records.one").unwrap(),
        CapabilityId::new("records.resource.read").unwrap(),
        ConnectorOperationClass::Read,
    )
    .unwrap();
    f.goal = SupportedConnectorGoal::from_host_registration(
        f.goal.intent().clone(),
        f.goal.scope().clone(),
        f.goal.plan().clone(),
        f.goal.steps().to_vec(),
        observations,
    )
    .unwrap();
    let bound = f.admit().unwrap();
    let mut request = f.request(0);
    request.arguments.clear();
    request.capability = CapabilityId::new("records.resource.read").unwrap();
    let source = SourceId::new("records.one").unwrap();
    let matched = bound
        .validate_fixed_read(
            &request,
            FixedReadJoin {
                mapping_reference: "mapping.records.source.v1",
                source: Some(&source),
                arguments: &request.arguments,
                scope_export: None,
            },
        )
        .unwrap();
    matched
        .validate_response(&ProtocolResult::Resource {
            contents: vec![json!({"uri":"records://one","text":"qualified read data"})],
        })
        .unwrap();
    let failure = matched
        .validate_response(&ProtocolResult::Resource {
            contents: vec![json!({"uri":"records://two","text":"foreign"})],
        })
        .unwrap_err();
    assert_eq!(failure.effect, ConnectorEffect::NoMutation);
    assert!(
        bound
            .validate_fixed_read(&request, join("mapping.records.source.v1", &request))
            .is_err()
    );
}

#[test]
fn same_wire_capability_and_mapping_keep_both_native_object_routes_unambiguous() {
    let mut f = Fixture::new();
    f.association["operations"][1]["mapping_reference"] = json!("mapping.records.read.v1");
    f.subjects[1].mapping_reference = "mapping.records.read.v1".into();
    let mut observations = f.goal.observations().to_vec();
    observations[1] = ConditionObservationMapping::new(
        observations[1].condition().clone(),
        observations[1].source().clone(),
        CapabilityId::new("records.read").unwrap(),
        ConnectorOperationClass::Read,
    )
    .unwrap();
    f.goal = SupportedConnectorGoal::from_host_registration(
        f.goal.intent().clone(),
        f.goal.scope().clone(),
        f.goal.plan().clone(),
        f.goal.steps().to_vec(),
        observations,
    )
    .unwrap();
    let bound = f.admit().unwrap();
    assert_eq!(bound.operations.values().next().unwrap().len(), 2);
    for i in 0..2 {
        let mut request = f.request(i);
        request.capability = CapabilityId::new("records.read").unwrap();
        let matched = bound
            .validate_fixed_read(&request, join("mapping.records.read.v1", &request))
            .unwrap();
        matched.validate_response(&ProtocolResult::Tool{content:vec![],structured:Some(json!({"object":if i==0{"object-one"}else{"object-two"},"id":if i==0{"one"}else{"two"},"ready":true})),is_error:false}).unwrap();
    }
    let mut foreign = f.request(0);
    foreign.arguments.insert(
        NonEmptyText::new("query").unwrap(),
        TypedValue::string("unrelated-same-scope-object").unwrap(),
    );
    assert!(
        bound
            .validate_fixed_read(&foreign, join("mapping.records.read.v1", &foreign))
            .is_err()
    );
    let complete_operations = f.association["operations"].clone();
    f.association["operations"].as_array_mut().unwrap().pop();
    assert!(
        f.admit().is_err(),
        "an unused same-mapping normalization route cannot fill omitted goal coverage"
    );
    f.association["operations"] = complete_operations;
    // Two profiles for identical route arguments cannot pick an arbitrary first
    // object or silently drop one operation.
    f.subjects[1].arguments = f.subjects[0].arguments.clone();
    assert!(f.admit().is_err());
}

#[test]
fn actual_normalizer_object_field_must_equal_the_prevalidated_response_identity_field() {
    let schema = json!({"type":"object","additionalProperties":false,"properties":{"object":{"type":"string"},"member":{"type":"string"},"id":{"type":"string"},"ready":{"type":"boolean"}},"required":["object","member","id","ready"]});
    let mut f = Fixture::with_result_schema(schema);
    let body = json!({"member":"object-one","object":"foreign-object","id":"one","ready":true});
    assert_eq!(body[&f.subjects[0].object_field], "foreign-object");
    assert_eq!(f.subjects[0].object_field, "object");
    let bound = f.admit().unwrap();
    let request = f.request(0);
    assert!(
        bound
            .validate_fixed_read(&request, join("mapping.records.read.v1", &request))
            .unwrap()
            .validate_response(&ProtocolResult::Tool {
                content: vec![],
                structured: Some(body),
                is_error: false
            })
            .is_err()
    );
    // Both fields are required strings in the actual mapper-derived schema, so
    // schema validity alone cannot bind the normalized object to the task.
    f.association["operations"][0]["response"]["object_field"] = json!("member");
    assert!(matches!(f.admit(), Err(SessionError::ScopeDenied)));
    // A genuinely matching qualified normalizer field remains usable.
    f.subjects[0].object_field = "member".into();
    assert!(f.admit().is_ok());
}
#[test]
fn explicit_condition_ids_support_independent_sources_for_the_same_native_subject() {
    let mut f = Fixture::new();
    let subject = SubjectPath::new(["state", "first"]).unwrap();
    let conditions = (0..2)
        .map(|i| {
            DesiredCondition::new(
                ConditionId::new(format!("condition-{i}")).unwrap(),
                subject.clone(),
                ComparisonOperator::Equals,
                Some(TypedValue::Boolean(true)),
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let desired = DesiredState::new(
        DesiredStateId::new("corroborated-goal").unwrap(),
        conditions.clone(),
        ConditionExpression::all(
            conditions
                .iter()
                .map(|c| ConditionExpression::condition(c.id().clone()))
                .collect(),
        )
        .unwrap(),
        vec![],
        vec![],
    )
    .unwrap();
    f.goal = SupportedConnectorGoal::from_host_registration(
        Intent::new(
            IntentId::new("independent-sources").unwrap(),
            desired.clone(),
        ),
        f.goal.scope().clone(),
        f.goal.plan().clone(),
        f.goal.steps().to_vec(),
        f.goal.observations().to_vec(),
    )
    .unwrap();
    f.objects[2] = object(
        "object-two",
        TargetKind::Artifact,
        Some(&f.objects[0].pin),
        std::slice::from_ref(&subject),
        "two",
    );
    f.association["operations"][1]["object"] = json!(f.objects[2].pin);
    let mut task = json!(f.task);
    task["desired_state"] = json!(desired);
    task["inputs"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|input| input["role"] == "second")
        .unwrap()["value"]["value"] = json!(f.objects[2].pin);
    f.refresh_task(task);
    f.subjects[1].subjects = BTreeSet::from([subject]);
    let bound = f.admit().unwrap();
    for i in 0..2 {
        let request = f.request(i);
        let mapping = if i == 0 {
            "mapping.records.read.v1"
        } else {
            "mapping.records.second.v1"
        };
        bound
            .validate_fixed_read(&request, join(mapping, &request))
            .unwrap();
    }
    let complete_operations = f.association["operations"].clone();
    for malformed in ["foreign", "empty", "missing", "duplicate"] {
        let original = f.subjects[1].condition_ids.clone();
        match malformed {
            "foreign" => {
                f.subjects[1].condition_ids =
                    BTreeSet::from([ConditionId::new("unregistered").unwrap()])
            }
            "empty" => f.subjects[1].condition_ids.clear(),
            "missing" => {
                f.association["operations"].as_array_mut().unwrap().pop();
            }
            _ => {
                f.subjects[1]
                    .condition_ids
                    .insert(ConditionId::new("condition-0").unwrap());
            }
        }
        assert!(f.admit().is_err(), "{malformed}");
        f.subjects[1].condition_ids = original;
        f.association["operations"] = complete_operations.clone();
    }
}

#[test]
fn closed_read_documents_enforce_version_identity_and_bounded_routes_before_admission() {
    let f = Fixture::new();
    for case in [
        "no-operations",
        "body-version",
        "many-operations",
        "task-contract",
        "object-contract",
        "connector-bound",
        "empty-mapping",
        "duplicate-operation",
        "operand-bound",
        "duplicate-operand",
        "response-name",
        "self-response",
        "export-contract",
    ] {
        let mut v = f.association.clone();
        match case {
            "no-operations" => v["operations"] = json!([]),
            "body-version" => v["schema_version"] = json!("2.0"),
            "many-operations" => {
                v["operations"] = json!(vec![v["operations"][0].clone(); OPERATIONS + 1])
            }
            "task-contract" => v["task"]["contract"] = json!("cg.context-projection"),
            "object-contract" => v["operations"][0]["object"]["contract"] = json!("cg.artifact"),
            "connector-bound" => v["connector"] = json!("x".repeat(129)),
            "empty-mapping" => v["operations"][0]["mapping_reference"] = json!(""),
            "duplicate-operation" => {
                let op = v["operations"][0].clone();
                v["operations"].as_array_mut().unwrap().push(op);
            }
            "operand-bound" => {
                v["operations"][0]["selector_arguments"] = json!(
                    (0..=FIELDS)
                        .map(|i| (format!("arg{i}"), "id"))
                        .collect::<BTreeMap<_, _>>()
                )
            }
            "duplicate-operand" => {
                v["operations"][0]["literal_arguments"]["query"] = json!("limit")
            }
            "response-name" => v["operations"][0]["response"]["object_field"] = json!(""),
            "self-response" => {
                v["operations"][0]["response"]["identity_fields"]["object"] = json!("id")
            }
            "export-contract" => {
                v["operations"][0]["subject"] =
                    json!({"kind":"SCOPE_EXPORT","provider_contract":f.projection})
            }
            _ => unreachable!(),
        }
        let p = pin("association", "cg.connector-read-association", &v);
        assert!(
            ReadAssociation::from_json(&p, &serde_json::to_vec(&v).unwrap()).is_err(),
            "{case}"
        );
    }
    let bytes = serde_json::to_vec(&f.association).unwrap();
    let mut wrong = pin(
        "association",
        "cg.connector-read-association",
        &f.association,
    );
    wrong.contract = ReferenceId::new("cg.artifact").unwrap();
    assert!(ReadAssociation::from_json(&wrong, &bytes).is_err());
    wrong.contract = ReferenceId::new("cg.connector-read-association").unwrap();
    wrong.contract_version = SchemaVersion::V2;
    assert!(ReadAssociation::from_json(&wrong, &bytes).is_err());
    assert!(
        ReadAssociation::from_json(
            &pin(
                "association",
                "cg.connector-read-association",
                &f.association
            ),
            &[255]
        )
        .is_err()
    );
}

#[test]
fn scoped_object_descriptors_reject_stale_headers_foreign_parents_and_non_scalar_routing() {
    let f = Fixture::new();
    let base = serde_json::to_value(&f.objects[1].body.coordinates).unwrap();
    for case in [
        "header",
        "foreign-parent",
        "repeated-subject",
        "compound-coordinate",
        "long-name",
        "many-coordinates",
    ] {
        let mut v = json!({"schema_version":"1.0","id":"object-one","revision":"1","scope":"project-x","kind":"ARTIFACT","connector":"example.records","parent_target":f.objects[0].pin,"subjects":f.objects[1].body.subjects,"coordinates":base,"object_reference":"object-one"});
        match case {
            "header" => v["revision"] = json!("other"),
            "foreign-parent" => v["parent_target"]["scope"] = json!("other-project"),
            "repeated-subject" => {
                v["subjects"] = json!(vec![f.objects[1].body.subjects[0].clone(); 2])
            }
            "compound-coordinate" => {
                v["coordinates"]["id"] =
                    json!({"kind":"SET","value":[{"kind":"INTEGER","value":3}]})
            }
            "long-name" => {
                v["coordinates"] = json!(BTreeMap::from([(
                    "x".repeat(129),
                    json!({"kind":"STRING","value":"one"})
                )]));
            }
            "many-coordinates" => {
                v["coordinates"] = json!(
                    (0..=FIELDS)
                        .map(|i| (
                            format!("coordinate{i}"),
                            json!({"kind":"INTEGER","value":i})
                        ))
                        .collect::<BTreeMap<_, _>>()
                )
            }
            _ => unreachable!(),
        }
        assert!(
            ScopeObjectDocument::from_json(
                &pin("object-one", "cg.connector-object", &v),
                &serde_json::to_vec(&v).unwrap()
            )
            .is_err(),
            "{case}"
        );
    }
}

#[test]
fn integer_and_boolean_native_coordinates_have_exact_schema_and_response_membership() {
    for (kind, value, json_value, wrong) in [
        ("INTEGER", TypedValue::Integer(7), json!(7), json!(7.0)),
        (
            "BOOLEAN",
            TypedValue::Boolean(true),
            json!(true),
            json!(false),
        ),
    ] {
        let schema_type = if kind == "INTEGER" {
            "integer"
        } else {
            "boolean"
        };
        let mut f = Fixture::with_shapes(
            json!({"type":"object","additionalProperties":false,"properties":{"object":{"type":"string"},"id":{"type":schema_type},"ready":{"type":"boolean"}},"required":["object","id","ready"]}),
            schema_type,
        );
        // The independent second route retains its genuine string identity.
        f.association["operations"]
            .as_array_mut()
            .unwrap()
            .truncate(1);
        let v = json!({"schema_version":"1.0","id":"object-one","revision":"1","scope":"project-x","kind":"ARTIFACT","connector":"example.records","parent_target":f.objects[0].pin,"subjects":f.objects[0].body.subjects,"coordinates":{"id":value},"object_reference":"object-one"});
        let p = pin("object-one", "cg.connector-object", &v);
        f.objects[1] =
            ScopeObjectDocument::from_json(&p, &serde_json::to_vec(&v).unwrap()).unwrap();
        f.association["operations"][0]["object"] = json!(p);
        let mut task = json!(f.task);
        task["inputs"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|input| input["role"] == "first")
            .unwrap()["value"]["value"] = json!(p);
        f.refresh_task(task);
        // One declared route deliberately covers both original conditions.
        let second = f.subjects.pop().unwrap();
        f.subjects[0].subjects.extend(second.subjects);
        f.subjects[0].condition_ids.extend(second.condition_ids);
        f.subjects[0]
            .arguments
            .insert(NonEmptyText::new("query").unwrap(), value.clone());
        let old = f.goal.clone();
        f.goal = SupportedConnectorGoal::from_host_registration(
            old.intent().clone(),
            old.scope().clone(),
            old.plan().clone(),
            old.steps().to_vec(),
            old.observations()
                .iter()
                .map(|o| {
                    ConditionObservationMapping::new(
                        o.condition().clone(),
                        SourceId::new("source-0").unwrap(),
                        CapabilityId::new("records.read").unwrap(),
                        ConnectorOperationClass::Read,
                    )
                    .unwrap()
                })
                .collect(),
        )
        .unwrap();
        let bound = f.admit().unwrap();
        let mut request = f.request(0);
        request
            .arguments
            .insert(NonEmptyText::new("query").unwrap(), value);
        let matched = bound
            .validate_fixed_read(&request, join("mapping.records.read.v1", &request))
            .unwrap();
        matched
            .validate_response(&ProtocolResult::Tool {
                content: vec![],
                structured: Some(json!({"object":"object-one","id":json_value,"ready":true})),
                is_error: false,
            })
            .unwrap();
        assert!(
            matched
                .validate_response(&ProtocolResult::Tool {
                    content: vec![],
                    structured: Some(json!({"object":"object-one","id":wrong,"ready":true})),
                    is_error: false
                })
                .is_err()
        );
        assert!(
            matched
                .validate_response(&ProtocolResult::Resource { contents: vec![] })
                .is_err()
        );
    }
}

#[test]
fn native_target_catalog_and_literal_provenance_cannot_be_substituted_or_ambiguous() {
    for case in [
        "duplicate-object",
        "oversized-object-catalog",
        "wrong-target-kind",
        "reference-is-not-literal",
        "missing-schema-operand",
        "ambiguous-read-route",
    ] {
        let mut f = Fixture::new();
        match case {
            "duplicate-object" => {
                f.objects.push(object(
                    "object-one",
                    TargetKind::Artifact,
                    Some(&f.objects[0].pin),
                    &f.objects[1].body.subjects,
                    "one",
                ));
            }
            "oversized-object-catalog" => {
                for i in 0..OPERATIONS {
                    f.objects.push(object(
                        &format!("unrelated-{i}"),
                        TargetKind::Artifact,
                        Some(&f.objects[0].pin),
                        &f.objects[1].body.subjects,
                        "unrelated",
                    ));
                }
            }
            "wrong-target-kind" => {
                let mut task = json!(f.task);
                task["target"]["kind"] = json!("ARTIFACT");
                f.refresh_task(task);
            }
            "reference-is-not-literal" => {
                f.association["operations"][0]["literal_arguments"]["limit"] = json!("first")
            }
            "missing-schema-operand" => {
                f.association["operations"][0]["selector_arguments"] = json!({});
                f.subjects[0]
                    .arguments
                    .remove(&NonEmptyText::new("query").unwrap());
            }
            "ambiguous-read-route" => f.subjects.push(f.subjects[0].clone()),
            _ => unreachable!(),
        }
        assert!(f.admit().is_err(), "{case}");
    }
}

#[test]
fn read_admission_binds_current_connector_raw_mapping_and_complete_target_universe() {
    for case in [
        "connector",
        "raw-mapping",
        "smaller-target-universe",
        "literal-as-object",
        "compound-literal",
    ] {
        let mut f = Fixture::new();
        let expected = match case {
            "connector" => {
                f.association["connector"] = json!("foreign.connector");
                SessionError::ScopeDenied
            }
            "raw-mapping" => {
                f.mapping["mapping_version"] = json!("2.0");
                SessionError::StaleRevision
            }
            "smaller-target-universe" => {
                f.objects[0] = object(
                    "target-project",
                    TargetKind::Project,
                    None,
                    &f.objects[0].body.subjects[..1],
                    "project-x",
                );
                let mut task = json!(f.task);
                task["target"]["reference"] = json!(f.objects[0].pin);
                f.refresh_task(task);
                SessionError::ScopeDenied
            }
            "literal-as-object" => {
                f.association["operations"][0]["subject"] =
                    json!({"kind":"INPUT_REFERENCE","role":"limit"});
                SessionError::UnsupportedCapability
            }
            "compound-literal" => {
                let mut task = json!(f.task);
                task["inputs"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|input| input["role"] == "limit")
                    .unwrap()["value"] = json!({"kind":"LITERAL","value":{"kind":"SET","value":[{"kind":"INTEGER","value":3}]}});
                f.refresh_task(task);
                SessionError::UnsupportedCapability
            }
            _ => unreachable!(),
        };
        assert_eq!(
            f.admit().err().expect("unqualified read must be denied"),
            expected,
            "{case}"
        );
    }
    let f = Fixture::new();
    let bound = f.admit().unwrap();
    let request = f.request(0);
    let read = bound
        .validate_fixed_read(&request, join("mapping.records.read.v1", &request))
        .unwrap();
    for result in [
        ProtocolResult::Tool {
            content: vec![],
            structured: None,
            is_error: false,
        },
        ProtocolResult::Tool {
            content: vec![],
            structured: Some(json!({"object":"object-one","id":"one","ready":true})),
            is_error: true,
        },
    ] {
        let failure = read.validate_response(&result).unwrap_err();
        assert_eq!(failure.code, ConnectorFailureCode::ScopeMismatch);
        assert_eq!(failure.effect, ConnectorEffect::NoMutation);
        assert_eq!(failure.correlation, request.correlation);
    }
}
