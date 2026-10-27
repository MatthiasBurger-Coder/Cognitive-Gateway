use super::*;

#[test]
fn local_definition_reader_rejects_non_regular_oversized_and_stale_content() {
    let root = std::env::temp_dir().join(format!(
        "cg403-definition-reader-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    assert!(matches!(
        read_regular(&root, 64),
        Err(SessionError::InvalidInput)
    ));
    let path = root.join("definition.json");
    std::fs::write(&path, b"{\"id\":\"original\"}").unwrap();
    let bytes = read_regular(&path, 64).unwrap();
    let original =
        super::super::super::definition_json::parse(std::str::from_utf8(&bytes).unwrap()).unwrap();
    std::fs::write(&path, b"{\"id\":\"changed\"}").unwrap();
    let changed = super::super::super::definition_json::parse(
        std::str::from_utf8(&read_regular(&path, 64).unwrap()).unwrap(),
    )
    .unwrap();
    assert_ne!(hash(&original).unwrap(), hash(&changed).unwrap());
    std::fs::write(&path, vec![b'x'; 65]).unwrap();
    assert!(matches!(
        read_regular(&path, 64),
        Err(SessionError::LimitExceeded)
    ));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn canonical_definition_identity_is_whitespace_independent_but_binds_all_fields() {
    let first =
        super::super::super::definition_json::parse("{\"nested\":{\"a\":1},\"kind\":\"native\"}")
            .unwrap();
    let same = super::super::super::definition_json::parse(
        " { \"kind\":\"native\", \"nested\":{\"a\":1} } ",
    )
    .unwrap();
    assert_eq!(hash(&first).unwrap(), hash(&same).unwrap());
    let mut changed = same;
    changed["nested"]["a"] = json!(2);
    assert_ne!(hash(&first).unwrap(), hash(&changed).unwrap());
    assert!(super::super::super::definition_json::parse("{\"id\":1,\"id\":2}").is_err());
}

#[test]
fn catalog_manifest_is_bounded_and_detects_even_unused_definition_drift() {
    let root = std::env::temp_dir().join(format!(
        "cg403-catalog-manifest-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("unused.md");
    std::fs::write(&path, b"definition one").unwrap();
    let capture = || {
        let mut values = vec![];
        let mut bytes = 0;
        catalog_manifest(&root, &root, 0, &mut values, &mut bytes).unwrap();
        values
    };
    let first = capture();
    std::fs::write(&path, b"definition two").unwrap();
    assert_ne!(first, capture());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&root, root.join("cycle")).unwrap();
        assert!(catalog_manifest(&root, &root, 0, &mut vec![], &mut 0).is_err());
    }
    std::fs::remove_dir_all(root).unwrap();
}

// These exercise the metadata owner only. The documents deliberately confer no
// native resolution/allow; actual native capture is qualified by installed tests.
fn metadata_fixture() -> (PathBuf, ReadonlyRuntimeAdmission, LocalCodexHost) {
    let root = std::env::temp_dir().join(format!(
        "cg403-metadata-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let catalog = root.join("catalog");
    std::fs::create_dir_all(&catalog).unwrap();
    std::fs::write(catalog.join("definition.md"), b"bounded metadata").unwrap();
    let scope = ContextScopeId::new("project").unwrap();
    let mut documents = vec![];
    let mut add = |id: &str, contract: &str, value: Value| {
        let pin = Pin {
            id: ReferenceId::new(id).unwrap(),
            scope: scope.clone(),
            contract: ReferenceId::new(contract).unwrap(),
            contract_version: SchemaVersion::V1,
            revision: ReferenceId::new("operator-1").unwrap(),
            digest: hash(&value).unwrap(),
        };
        let path = root.join(format!("{id}.json"));
        std::fs::write(&path, value.to_string()).unwrap();
        documents.push(DocumentAdmission {
            pin: pin.clone(),
            path,
        });
        pin
    };
    let goal = add("goal", "cg.connector-goal", json!({"metadata":"goal"}));
    let policy = add(
        "policy",
        "cg.connector-canonical-policy",
        json!({"policies":[]}),
    );
    let manifest = add(
        "manifest",
        "cg.connector-manifest",
        json!({"metadata":"manifest"}),
    );
    let mapping = add(
        "mapping",
        "cg.connector-mapping",
        json!({"metadata":"mapping"}),
    );
    let association = add(
        "association",
        "cg.connector-read-association",
        json!({"metadata":"association"}),
    );
    let task = add("task", "cg.semantic-task", json!({"metadata":"task"}));
    let projection = add(
        "projection",
        "cg.context-projection",
        json!({"metadata":"projection"}),
    );
    let quotes = add(
        "quotes",
        "cg.connector-quotes",
        json!({"metadata":"quotes"}),
    );
    let verifier = add(
        "verifier",
        "cg.connector-verifier",
        json!({"metadata":"verifier"}),
    );
    let runtime = ReadonlyRuntimeAdmission {
        schema_version: SchemaVersion::V1,
        id: ReferenceId::new("runtime").unwrap(),
        revision: ReferenceId::new("operator-runtime-1").unwrap(),
        scope: scope.clone(),
        plan: content_reference("plan", b"metadata-plan").unwrap(),
        goal,
        execution: RequestedExecution {
            mode: OperatingMode::Development,
            profile: ExecutionProfile::FullPath,
        },
        connector: NonEmptyText::new("connector").unwrap(),
        policy,
        manifest,
        mapping,
        association,
        task,
        projection,
        step: "step".into(),
        quotes,
        verifier,
        stdio: StdioAdmission {
            executable: PathBuf::from("/usr/bin/python3"),
            arguments: vec![],
            directory: root.clone(),
            environment: BTreeMap::new(),
        },
        compatibility: CompatibilityAdmission {
            gateway: ConnectorVersion {
                major: 0,
                minor: 1,
                patch: 0,
            },
            transports: BTreeSet::from([ConnectorTransport::Stdio]),
        },
        limits: LimitAdmission {
            frame_bytes: 1024,
            inventory_bytes: 4096,
            items: 16,
            pages: 4,
            events_per_request: 16,
            call_timeout_ms: 1000,
            shutdown_ms: 100,
        },
        credential_store: root.join("credentials-not-read"),
        access_profile: NonEmptyText::new("operator-profile").unwrap(),
        documents,
        goal_reads: vec![],
    };
    let intent_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/declarative-cli/intent.json");
    let intent = Intent::from_json(&std::fs::read_to_string(intent_path).unwrap()).unwrap();
    let config = crate::local_sessions::SessionAdmission {
        store_file: root.join("database-not-read"),
        intent,
        basis: None,
        connector_goal: None,
        resource_limits: None,
        observed_goal_runtime: Some(runtime.clone()),
        execution: runtime.execution,
        max_actions: 2,
        max_retries: 1,
        ttl_ms: 60_000,
        consent_required: false,
        enabled: true,
        issuers: vec![PrincipalId::new("operator").unwrap()],
    };
    let host = LocalCodexHost {
        binding: gateway_application::codex::ScopeBinding {
            scope: json!({"workspace_id":"workspace","project_id":"project","binding_id":"binding"}),
            canonical_scope: scope,
            session: gateway_application::codex::SessionContext {
                principal: "operator".into(),
                session_id: "client".into(),
                connection_id: "connection".into(),
            },
            mapping_revision: "operator-map-1".into(),
        },
        resources: vec![],
        canonical: Some(crate::codex_canonical::CanonicalAdmission {
            catalog,
            plan: json!({}),
            rules: json!({}),
            process: json!({}),
            policy: json!({"policies":[]}),
            pinned_basis: None,
        }),
        sessions: Some(config),
    };
    (root, runtime, host)
}

#[test]
fn current_metadata_binds_actual_registration_catalog_and_definition_bytes() {
    let (root, runtime, mut host) = metadata_fixture();
    let original = runtime.current_fingerprint(&host).unwrap();
    host.sessions.as_mut().unwrap().max_actions += 1;
    assert_ne!(runtime.current_fingerprint(&host).unwrap(), original);
    host.sessions.as_mut().unwrap().max_actions -= 1;
    host.binding.session.principal = "other-operator".into();
    assert_ne!(runtime.current_fingerprint(&host).unwrap(), original);
    host.binding.session.principal = "operator".into();
    let catalog = host.canonical.as_ref().unwrap().catalog.clone();
    std::fs::write(
        catalog.join("unselected.md"),
        b"unused but current definition",
    )
    .unwrap();
    assert_ne!(runtime.current_fingerprint(&host).unwrap(), original);
    std::fs::remove_file(catalog.join("unselected.md")).unwrap();
    assert_eq!(runtime.current_fingerprint(&host).unwrap(), original);
    host.sessions
        .as_mut()
        .unwrap()
        .observed_goal_runtime
        .as_mut()
        .unwrap()
        .revision = ReferenceId::new("operator-runtime-2").unwrap();
    assert_eq!(
        runtime.current_fingerprint(&host),
        Err(SessionError::StaleRevision)
    );
    host.sessions.as_mut().unwrap().observed_goal_runtime = Some(runtime.clone());
    std::fs::write(&runtime.documents[0].path, b"{\"metadata\":\"changed\"}").unwrap();
    assert_eq!(
        runtime.current_fingerprint(&host),
        Err(SessionError::StaleRevision)
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn route_identity_reuses_exact_reads_but_rejects_a_different_profile_pin() {
    let source = SourceId::new("records.source").unwrap();
    let pin = Pin {
        id: ReferenceId::new("normalization.one").unwrap(),
        scope: ContextScopeId::new("project").unwrap(),
        contract: ReferenceId::new("cg.connector-normalization").unwrap(),
        contract_version: SchemaVersion::V1,
        revision: ReferenceId::new("operator-1").unwrap(),
        digest: context_content_digest(b"actual profile one"),
    };
    let read = ReadAdmission {
        mapping_reference: "mapping.read.v1".into(),
        capability: CapabilityId::new("records.read").unwrap(),
        source: source.clone(),
        arguments: BTreeMap::from([(
            NonEmptyText::new("query").unwrap(),
            TypedValue::string("one").unwrap(),
        )]),
        normalization: pin,
    };
    let doc = NormalizationDocument {
        schema_version: SchemaVersion::V1,
        acquisition: AcquisitionDocument {
            source_kind: SourceKind::Tool,
            sensitivity: SensitivityClass::Internal,
            authenticated_profile: NonEmptyText::new("operator-profile").unwrap(),
            semantics: ReadSemantics::DirectCurrentReadProfile {
                max_age_seconds: 60,
                time_basis: ReadTimeBasis::RequestStartLowerBound,
            },
        },
        source,
        object_field: "object".into(),
        revision_field: None,
        digest_field: None,
        fields: BTreeMap::from([(
            "ready".into(),
            SubjectPath::new(["object", "one", "ready"]).unwrap(),
        )]),
    };
    let entries = vec![ReadNormalization {
        read: read.clone(),
        document: doc,
    }];
    assert_eq!(normalization_index(&entries, &read).unwrap(), Some(0));
    let mut other = read.clone();
    other.normalization.revision = ReferenceId::new("operator-2").unwrap();
    assert_eq!(
        normalization_index(&entries, &other),
        Err(SessionError::InvalidInteraction)
    );
    other = read;
    other.arguments.insert(
        NonEmptyText::new("query").unwrap(),
        TypedValue::string("two").unwrap(),
    );
    assert_eq!(normalization_index(&entries, &other).unwrap(), None);
}

fn registered_metadata_fixture() -> (
    PathBuf,
    ReadonlyRuntimeAdmission,
    LocalCodexHost,
    SupportedConnectorGoal,
) {
    let (root, mut runtime, mut host) = metadata_fixture();
    let intent = host.sessions.as_ref().unwrap().intent.clone();
    let condition = intent.desired_state().conditions()[0].id().clone();
    let goal = SupportedConnectorGoal::from_host_registration(
        intent,
        runtime.scope.clone(),
        runtime.plan.clone(),
        vec![PlanStepId::new(&runtime.step).unwrap()],
        vec![
            ConditionObservationMapping::new(
                condition.clone(),
                SourceId::new("source").unwrap(),
                CapabilityId::new("architecture.dependency-analysis").unwrap(),
                gateway_application::connectors::ConnectorOperationClass::Read,
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let value = json!({"schema_version":"1.0","metadata":"normalization-data-only"});
    let pin = Pin {
        id: ReferenceId::new("normalization").unwrap(),
        scope: runtime.scope.clone(),
        contract: ReferenceId::new("cg.connector-normalization").unwrap(),
        contract_version: SchemaVersion::V1,
        revision: ReferenceId::new("operator-1").unwrap(),
        digest: hash(&value).unwrap(),
    };
    let path = root.join("normalization.json");
    std::fs::write(&path, value.to_string()).unwrap();
    runtime.documents.push(DocumentAdmission {
        pin: pin.clone(),
        path,
    });
    runtime.goal_reads = vec![GoalReadAdmission {
        condition: condition.as_str().into(),
        read: ReadAdmission {
            mapping_reference: "mapping.read".into(),
            capability: CapabilityId::new("architecture.dependency-analysis").unwrap(),
            source: SourceId::new("source").unwrap(),
            arguments: BTreeMap::new(),
            normalization: pin,
        },
    }];
    host.sessions.as_mut().unwrap().observed_goal_runtime = Some(runtime.clone());
    (root, runtime, host, goal)
}

#[test]
fn registration_rejects_incomplete_duplicate_foreign_and_unowned_read_definitions() {
    let (root, runtime, _, goal) = registered_metadata_fixture();
    runtime
        .validate_registration(&goal, runtime.execution)
        .unwrap();
    for case in [
        "no-documents",
        "no-reads",
        "step",
        "credential-path",
        "wrong-contract",
        "foreign-pin",
        "foreign-document",
        "relative-document",
        "duplicate-document",
        "duplicate-condition",
        "missing-condition",
        "wrong-source",
        "wrong-capability",
        "overlong-map",
        "many-operands",
    ] {
        let mut r = runtime.clone();
        match case {
            "no-documents" => r.documents.clear(),
            "no-reads" => r.goal_reads.clear(),
            "step" => r.step = "foreign-step".into(),
            "credential-path" => r.credential_store = PathBuf::from("relative"),
            "wrong-contract" => r.goal.contract = ReferenceId::new("cg.semantic-task").unwrap(),
            "foreign-pin" => r.goal.scope = ContextScopeId::new("other").unwrap(),
            "foreign-document" => r.documents[0].pin.scope = ContextScopeId::new("other").unwrap(),
            "relative-document" => r.documents[0].path = PathBuf::from("relative.json"),
            "duplicate-document" => r.documents.push(r.documents[0].clone()),
            "duplicate-condition" => r.goal_reads.push(r.goal_reads[0].clone()),
            "missing-condition" => r.goal_reads[0].condition = "not-original-goal".into(),
            "wrong-source" => r.goal_reads[0].read.source = SourceId::new("other").unwrap(),
            "wrong-capability" => {
                r.goal_reads[0].read.capability = CapabilityId::new("other.read").unwrap()
            }
            "overlong-map" => r.goal_reads[0].read.mapping_reference = "m".repeat(129),
            "many-operands" => {
                r.goal_reads[0].read.arguments = (0..257)
                    .map(|i| {
                        (
                            NonEmptyText::new(format!("arg{i}")).unwrap(),
                            TypedValue::Integer(i),
                        )
                    })
                    .collect()
            }
            _ => unreachable!(),
        }
        assert!(
            r.validate_registration(&goal, r.execution).is_err(),
            "{case}"
        );
    }
    let mut r = runtime.clone();
    r.goal_reads.push(GoalReadAdmission {
        condition: "extra-condition".into(),
        read: r.goal_reads[0].read.clone(),
    });
    assert!(r.validate_registration(&goal, r.execution).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn bounded_stdio_and_wire_limits_reject_unbounded_or_ambient_execution() {
    let (root, runtime, _, _) = registered_metadata_fixture();
    for case in [
        "frame",
        "inventory",
        "items",
        "pages",
        "events",
        "timeout",
        "shutdown",
    ] {
        let mut l = runtime.limits.clone();
        match case {
            "frame" => l.frame_bytes = 16_777_217,
            "inventory" => l.inventory_bytes = 67_108_865,
            "items" => l.items = 65_537,
            "pages" => l.pages = 1025,
            "events" => l.events_per_request = 4097,
            "timeout" => l.call_timeout_ms = 3_600_001,
            "shutdown" => l.shutdown_ms = 5001,
            _ => unreachable!(),
        }
        assert!(l.build().is_err(), "{case}");
    }
    for case in [
        "relative-executable",
        "relative-directory",
        "too-many-args",
        "too-long-arg",
        "ambient-env",
    ] {
        let mut s = runtime.stdio.clone();
        match case {
            "relative-executable" => s.executable = PathBuf::from("python3"),
            "relative-directory" => s.directory = PathBuf::from("project"),
            "too-many-args" => s.arguments = vec!["argument".into(); 33],
            "too-long-arg" => s.arguments = vec!["a".repeat(4097)],
            "ambient-env" => {
                s.environment
                    .insert("UNQUALIFIED_ENV".into(), "value".into());
            }
            _ => unreachable!(),
        }
        assert!(s.build().is_err(), "{case}");
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn metadata_rejects_missing_owner_roots_duplicate_ids_and_non_utf8_document_bytes() {
    for case in [
        "no-session",
        "no-runtime",
        "no-canonical",
        "missing-root",
        "stale-root",
        "duplicate-id",
        "non-utf8",
        "invalid-json",
        "missing-file",
    ] {
        let (root, mut runtime, mut host) = metadata_fixture();
        match case {
            "no-session" => host.sessions = None,
            "no-runtime" => host.sessions.as_mut().unwrap().observed_goal_runtime = None,
            "no-canonical" => host.canonical = None,
            "missing-root" => runtime.goal.id = ReferenceId::new("absent").unwrap(),
            "stale-root" => runtime.goal.revision = ReferenceId::new("foreign-revision").unwrap(),
            "duplicate-id" => runtime.documents.push(runtime.documents[0].clone()),
            "non-utf8" => std::fs::write(&runtime.documents[0].path, [255]).unwrap(),
            "invalid-json" => std::fs::write(&runtime.documents[0].path, b"{").unwrap(),
            "missing-file" => std::fs::remove_file(&runtime.documents[0].path).unwrap(),
            _ => unreachable!(),
        }
        // Match the current registered body to isolate the inner owner check.
        if let Some(config) = host.sessions.as_mut()
            && config.observed_goal_runtime.is_some()
        {
            config.observed_goal_runtime = Some(runtime.clone());
        }
        assert!(runtime.current_fingerprint(&host).is_err(), "{case}");
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn catalog_collection_bounds_depth_total_bytes_and_external_paths() {
    let (root, _, host) = metadata_fixture();
    let catalog = &host.canonical.as_ref().unwrap().catalog;
    assert!(catalog_manifest(catalog, catalog, 17, &mut vec![], &mut 0).is_err());
    assert!(
        catalog_manifest(
            catalog,
            catalog,
            0,
            &mut vec![(PathBuf::new(), context_content_digest(b"existing")); 513],
            &mut 0
        )
        .is_err()
    );
    assert!(catalog_manifest(catalog, &root, 0, &mut vec![], &mut 0).is_err());
    let file = catalog.join("definition.md");
    let mut bytes = MAX_TOTAL;
    assert!(catalog_manifest(catalog, &file, 0, &mut vec![], &mut bytes).is_err());
    assert!(read_regular(Path::new("relative.json"), 64).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn semantic_reference_validation_rechecks_all_current_native_pin_fields() {
    let task = SemanticTaskIR::from_json(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/semantic-task-v1/minimal.json"
    )))
    .unwrap();
    let pins = [
        task.data().target.reference.clone(),
        task.data().output_contract.schema.clone(),
    ];
    let documents = pins
        .iter()
        .map(|p| {
            (
                p.id.clone(),
                (p.clone(), json!({"metadata":"reference-owner-only"})),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let scope = task.data().target.reference.scope.clone();
    task.validate_references(&References {
        scope: &scope,
        documents: &documents,
    })
    .unwrap();
    for case in ["missing", "revision", "digest", "scope", "contract"] {
        let mut current = documents.clone();
        let id = &task.data().target.reference.id;
        match case {
            "missing" => {
                current.remove(id);
            }
            "revision" => {
                current.get_mut(id).unwrap().0.revision =
                    ReferenceId::new("new-current-revision").unwrap()
            }
            "digest" => {
                current.get_mut(id).unwrap().0.digest = context_content_digest(b"changed-owner")
            }
            "scope" => current.get_mut(id).unwrap().0.scope = ContextScopeId::new("other").unwrap(),
            "contract" => {
                current.get_mut(id).unwrap().0.contract =
                    ReferenceId::new("cg.connector-object").unwrap()
            }
            _ => unreachable!(),
        }
        assert!(
            task.validate_references(&References {
                scope: &scope,
                documents: &current
            })
            .is_err(),
            "{case}"
        );
    }
    assert!(
        task.validate_references(&References {
            scope: &ContextScopeId::new("foreign").unwrap(),
            documents: &documents
        })
        .is_err()
    );
}

#[test]
fn genuine_native_capture_rejects_re_pinned_foreign_task_route_and_verifier_definitions() {
    if std::env::var_os("CG_COGNITIVE_TEST_DATABASE").is_none() {
        return;
    }
    // Actual CLI planning/resolution/CG10 records; no model, allocation or peer call.
    let fixture = super::super::source::boundary_tests::Fixture::new();
    let mut host = fixture.access().current().unwrap();
    let config = host.sessions.as_ref().unwrap();
    let original = config.observed_goal_runtime.as_ref().unwrap().clone();
    let goal =
        crate::local_sessions::registered_connector_goal(config, &host.binding.canonical_scope)
            .unwrap();
    original.capture(&host, &goal, original.execution).unwrap();
    host.binding.canonical_scope = ContextScopeId::new("foreign-native-scope").unwrap();
    assert!(matches!(
        original.capture(&host, &goal, original.execution),
        Err(SessionError::ScopeDenied)
    ));
    host = fixture.access().current().unwrap();
    host.canonical.as_mut().unwrap().plan["revision"] = json!("other-current-plan");
    assert!(matches!(
        original.capture(&host, &goal, original.execution),
        Err(SessionError::StaleRevision)
    ));
    for case in [
        "different-goal",
        "task-identity",
        "task-reference",
        "task-desired",
        "manifest-connector",
        "mapping-class",
        "mapping-source",
        "mapping-schema",
        "normalizer-age",
        "normalizer-source-kind",
        "normalizer-subject",
        "normalizer-closed",
        "quote-identity",
        "quote-reference",
        "quote-model",
        "quote-unit",
        "verifier-goal",
        "verifier-iterations",
    ] {
        let mut host = fixture.access().current().unwrap();
        let mut runtime = original.clone();
        let selected = match case {
            "different-goal" => runtime.goal.clone(),
            "task-identity" | "task-reference" | "task-desired" => runtime.task.clone(),
            "manifest-connector" => runtime.manifest.clone(),
            "mapping-class" | "mapping-source" | "mapping-schema" => runtime.mapping.clone(),
            "normalizer-age"
            | "normalizer-source-kind"
            | "normalizer-subject"
            | "normalizer-closed" => runtime.goal_reads[0].read.normalization.clone(),
            "quote-identity" | "quote-reference" | "quote-model" | "quote-unit" => {
                runtime.quotes.clone()
            }
            _ => runtime.verifier.clone(),
        };
        let document = runtime
            .documents
            .iter_mut()
            .find(|d| d.pin == selected)
            .unwrap();
        let bytes = std::fs::read(&document.path).unwrap();
        let mut body: Value = serde_json::from_slice(&bytes).unwrap();
        match case {
            "different-goal" => {
                body["desired_state"]["conditions"][0]["expected"]["value"] = json!(false)
            }
            "task-identity" => body["id"] = json!("other-native-task"),
            "task-reference" => {
                body["target"]["reference"]["revision"] = json!("other-object-revision")
            }
            "task-desired" => {
                body["desired_state"]["conditions"][0]["expected"]["value"] = json!(false)
            }
            "manifest-connector" => body["connector_id"] = json!("other.connector"),
            "mapping-class" => {
                body["records"][0]["class"] = json!("MUTATE");
                body["records"][0]["side_effects"] = json!("mutation");
                body["records"][0]["retry_safety"] = json!("never");
            }
            "mapping-source" => body["records"][0]["source"] = json!("unregistered.source"),
            "mapping-schema" => body["mapping_version"] = json!("2.0"),
            "normalizer-age" => body["acquisition"]["semantics"]["max_age_seconds"] = json!(0),
            "normalizer-source-kind" => body["acquisition"]["source_kind"] = json!("REPOSITORY"),
            "normalizer-subject" => body["fields"]["first"] = json!("foreign.subject"),
            "normalizer-closed" => body["unqualified_extra_rule"] = json!(true),
            "quote-identity" => body["manifest"]["revision"] = json!("different-manifest"),
            "quote-reference" => {
                body["operations"][0]["mapping_reference"] = json!("not-a-declared-mapping")
            }
            "quote-model" => body["operations"][0]["amounts"]["model_attempts"] = json!(1),
            "quote-unit" => body["cost_unit"] = json!("foreign-unit"),
            "verifier-goal" => body["goal"]["revision"] = json!("other-registered-goal"),
            "verifier-iterations" => body["max_iterations"] = json!(0),
            _ => unreachable!(),
        }
        document.pin.digest = hash(&body).unwrap();
        let replacement = document.pin.clone();
        let path = document.path.clone();
        std::fs::write(&path, serde_json::to_vec(&body).unwrap()).unwrap();
        match case {
            "different-goal" => runtime.goal = replacement,
            "task-identity" | "task-reference" | "task-desired" => runtime.task = replacement,
            "manifest-connector" => runtime.manifest = replacement,
            "mapping-class" | "mapping-source" | "mapping-schema" => runtime.mapping = replacement,
            "normalizer-age"
            | "normalizer-source-kind"
            | "normalizer-subject"
            | "normalizer-closed" => {
                for route in &mut runtime.goal_reads {
                    if route.read.normalization == selected {
                        route.read.normalization = replacement.clone();
                    }
                }
            }
            "quote-identity" | "quote-reference" | "quote-model" | "quote-unit" => {
                runtime.quotes = replacement
            }
            _ => runtime.verifier = replacement,
        }
        host.sessions.as_mut().unwrap().observed_goal_runtime = Some(runtime.clone());
        // All outer pins and bytes are current: this must fail in the native
        // owner/shape relationship below metadata loading, not at an old digest.
        runtime.load_metadata(&host).unwrap();
        let error = runtime
            .capture(&host, &goal, runtime.execution)
            .err()
            .expect("invalid native relationship must be denied");
        std::fs::write(path, bytes).unwrap();
        let expected = match case {
            "different-goal" | "task-identity" | "task-desired" | "mapping-source"
            | "normalizer-subject" | "quote-unit" => SessionError::StaleRevision,
            "task-reference" | "mapping-class" => SessionError::AuthorityDenied,
            "quote-model" => SessionError::UnsupportedCapability,
            _ => SessionError::InvalidInput,
        };
        assert_eq!(error, expected, "native capture stage: {case}");
    }
    // Reopening unchanged native definitions still succeeds after every rejection.
    let host = fixture.access().current().unwrap();
    original.capture(&host, &goal, original.execution).unwrap();
}
