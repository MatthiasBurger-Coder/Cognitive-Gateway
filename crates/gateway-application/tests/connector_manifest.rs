use gateway_application::connector_manifest::*;
use serde_json::{Value, json};
use std::collections::BTreeSet;

const EXAMPLE: &[u8] =
    include_bytes!("../../../tests/fixtures/connector-manifest-v1/valid-stdio.json");
fn definition() -> Value {
    serde_json::from_slice::<Value>(EXAMPLE).unwrap()["manifest"].clone()
}
fn build(value: &Value) -> Result<ConnectorManifest, ManifestError> {
    ConnectorManifest::from_definition_json(&serde_json::to_vec(value).unwrap())
}
fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../../tests/fixtures/connector-manifest-v1/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

#[test]
fn golden_digests_round_trip_and_http_declaration() {
    for bytes in [EXAMPLE.to_vec(), fixture("valid-http.json")] {
        let m = ConnectorManifest::from_json(&bytes).unwrap();
        let expected: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(m.digest(), expected["digest"].as_str().unwrap());
        assert_eq!(
            m.canonical_json(),
            ConnectorManifest::from_json(&m.canonical_json())
                .unwrap()
                .canonical_json()
        );
        assert_eq!(m, build(&expected["manifest"]).unwrap());
    }
}

#[test]
fn hostile_fixture_corpus_fails_with_content_free_errors() {
    let cases: Value = serde_json::from_slice(&fixture("invalid-cases.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let result = ConnectorManifest::from_json(&fixture(case["file"].as_str().unwrap()));
        assert_eq!(
            format!("{:?}", result.unwrap_err()),
            case["error"].as_str().unwrap(),
            "{}",
            case["file"]
        );
    }
}

#[test]
fn bounded_parsing_rejects_malformed_json_and_duplicate_fields() {
    for bytes in [
        b"invalid".to_vec(),
        vec![b' '; MAX_MANIFEST_BYTES + 1],
        b"{}".to_vec(),
    ] {
        assert_eq!(
            ConnectorManifest::from_json(&bytes),
            Err(ManifestError::Encoding)
        );
        assert_eq!(
            ConnectorManifest::from_definition_json(&bytes),
            Err(ManifestError::Encoding)
        );
    }
    let d = definition().to_string();
    let duplicate = d.replacen(
        "\"connector_id\":",
        "\"connector_id\":\"other\",\"connector_id\":",
        1,
    );
    assert_eq!(
        ConnectorManifest::from_definition_json(duplicate.as_bytes()),
        Err(ManifestError::Encoding)
    );
}

#[test]
fn semantic_validation_is_shared_by_authoring_and_loading() {
    let mut cases = Vec::new();
    for path in ["connector_id", "server_id", "configuration_ref"] {
        for value in ["", "UPPER", "a/b", "a b", "é", &"a".repeat(97)] {
            let mut d = definition();
            d[path] = json!(value);
            cases.push(d);
        }
    }
    for key in ["authentication", "trust", "sensitivity"] {
        let mut d = definition();
        d["profiles"][key] = json!("bad:secret");
        cases.push(d);
    }
    for (key, value) in [
        ("contract_ref", json!("")),
        ("interval_seconds", json!(0)),
        ("interval_seconds", json!(86401)),
        ("timeout_seconds", json!(0)),
        ("timeout_seconds", json!(31)),
    ] {
        let mut d = definition();
        d["health"][key] = value;
        cases.push(d);
    }
    for (key, value) in [
        ("name", json!("")),
        ("name", json!("a".repeat(97))),
        ("name", json!("has space")),
        ("schema_digest", json!("a".repeat(63))),
        ("schema_digest", json!("g".repeat(64))),
        ("capability_mapping_ref", json!("bad/ref")),
    ] {
        let mut d = definition();
        d["inventory"][0][key] = value;
        cases.push(d);
    }
    let mut d = definition();
    d["inventory"][1]["source_mapping_ref"] = Value::Null;
    cases.push(d);
    let mut d = definition();
    d["inventory"][2]["source_mapping_ref"] = json!("mapping.bad");
    cases.push(d);
    let mut d = definition();
    d["inventory"] = json!(vec![d["inventory"][0].clone(); 257]);
    cases.push(d);
    for replacement in ["", "example.records"] {
        let mut d = definition();
        d["lifecycle"] = json!("deprecated");
        d["deprecation"] = json!({"replacement_connector_id":replacement,"sunset_gateway_minor":2});
        cases.push(d);
    }
    let mut d = definition();
    d["lifecycle"] = json!("deprecated");
    d["deprecation"] = json!({"replacement_connector_id":"example.next","sunset_gateway_minor":0});
    cases.push(d);
    for d in cases {
        assert_eq!(build(&d), Err(ManifestError::InvalidMetadata), "{d}");
        let wire = json!({"manifest":d,"digest":"0".repeat(64)});
        assert_eq!(
            ConnectorManifest::from_json(&serde_json::to_vec(&wire).unwrap()),
            Err(ManifestError::InvalidMetadata)
        );
    }
    for (path, key, value) in [
        ("protocol", "name", "other"),
        ("protocol", "rpc_version", "3.0"),
        ("compatibility", "port_version", "unknown"),
    ] {
        let mut d = definition();
        d[path][key] = json!(value);
        assert_eq!(build(&d), Err(ManifestError::UnsupportedVersion));
    }
}

#[test]
fn strict_nested_fields_enums_and_missing_required_metadata() {
    for path in [
        "protocol",
        "profiles",
        "health",
        "compatibility",
        "version",
        "server_version",
    ] {
        let mut d = definition();
        d[path]["password"] = json!("canary");
        assert_eq!(build(&d), Err(ManifestError::Encoding));
    }
    let mut d = definition();
    d["inventory"][0]["token"] = json!("canary");
    assert_eq!(build(&d), Err(ManifestError::Encoding));
    for (key, value) in [("transport", "sse"), ("lifecycle", "ready")] {
        let mut d = definition();
        d[key] = json!(value);
        assert_eq!(build(&d), Err(ManifestError::Encoding));
    }
    let mut d = definition();
    d["inventory"][0]["kind"] = json!("experimental");
    assert_eq!(build(&d), Err(ManifestError::Encoding));
    for key in [
        "connector_id",
        "inventory",
        "profiles",
        "protocol",
        "health",
        "compatibility",
    ] {
        let mut d = definition();
        d.as_object_mut().unwrap().remove(key);
        assert_eq!(build(&d), Err(ManifestError::Encoding));
    }
}

#[test]
fn property_inventory_permutations_and_json_layout_do_not_change_identity() {
    let original = ConnectorManifest::from_json(EXAMPLE).unwrap();
    for permutation in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let mut d = definition();
        let entries = d["inventory"].clone();
        d["inventory"] = json!(permutation.map(|i| entries[i].clone()));
        assert_eq!(build(&d).unwrap(), original);
    }
    // Property sweep across release boundaries and opaque references.
    for minor in 0..64 {
        let mut d = definition();
        d["version"]["minor"] = json!(minor);
        d["configuration_ref"] = json!(format!("config.instance-{minor}"));
        let m = build(&d).unwrap();
        assert_eq!(
            m,
            ConnectorManifest::from_json(&m.canonical_json()).unwrap()
        );
        assert_ne!(m.digest(), original.digest());
        let pretty = serde_json::to_vec_pretty(
            &serde_json::from_slice::<Value>(&m.canonical_json()).unwrap(),
        )
        .unwrap();
        assert_eq!(m, ConnectorManifest::from_json(&pretty).unwrap());
    }
    let mut empty = definition();
    empty["inventory"] = json!([]);
    assert!(build(&empty).is_ok());
}

#[test]
fn compatibility_matrix_uses_only_explicit_runtime_metadata() {
    let cases: Value = serde_json::from_slice(&fixture("compatibility-matrix.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let mut d = definition();
        for (key, value) in case["patch"].as_object().unwrap() {
            d[key] = value.clone();
        }
        let runtime = ConnectorCompatibilityProfile {
            gateway: serde_json::from_value(case["gateway"].clone()).unwrap(),
            transports: serde_json::from_value(case["transports"].clone()).unwrap(),
        };
        assert_eq!(
            format!("{:?}", build(&d).unwrap().compatibility(&runtime)),
            case["expected"].as_str().unwrap(),
            "{}",
            case["name"]
        );
    }
    let http = ConnectorManifest::from_json(&fixture("valid-http.json")).unwrap();
    assert_eq!(
        http.compatibility(&ConnectorCompatibilityProfile {
            gateway: ConnectorVersion {
                major: 0,
                minor: 1,
                patch: 0
            },
            transports: BTreeSet::from([ConnectorTransport::StreamableHttp])
        }),
        CompatibilityDecision::Compatible
    );
}

#[test]
fn duplicate_identity_conflict_and_version_rules_are_deterministic() {
    let baseline = ConnectorManifest::from_json(EXAMPLE).unwrap();
    assert_eq!(baseline.connector_id(), "example.records");
    assert_eq!(baseline.definition().transport, ConnectorTransport::Stdio);
    assert_eq!(baseline.server_id(), "example.records.local");
    assert_eq!(baseline.upgrade_from(&baseline), UpgradeDecision::Unchanged);
    assert_eq!(validate_manifest_set(&[]), Ok(()));
    assert_eq!(
        validate_manifest_set(std::slice::from_ref(&baseline)),
        Ok(())
    );
    assert_eq!(
        validate_manifest_set(&[baseline.clone(), baseline.clone()]),
        Err(ManifestError::DuplicateIdentity)
    );
    let mut d = definition();
    d["configuration_ref"] = json!("config.changed");
    let changed = build(&d).unwrap();
    assert_eq!(
        changed.upgrade_from(&baseline),
        UpgradeDecision::ConflictingRelease
    );
    assert_eq!(
        validate_manifest_set(&[baseline.clone(), changed]),
        Err(ManifestError::ConflictingIdentity)
    );
    d["version"]["patch"] = json!(1);
    assert_eq!(
        build(&d).unwrap().upgrade_from(&baseline),
        UpgradeDecision::ReadmissionRequired
    );
    assert_eq!(
        baseline.upgrade_from(&build(&d).unwrap()),
        UpgradeDecision::Incompatible
    );
    d["version"]["major"] = json!(2);
    assert_eq!(
        build(&d).unwrap().upgrade_from(&baseline),
        UpgradeDecision::Incompatible
    );
    for key in ["connector_id", "server_id"] {
        let mut d = definition();
        d[key] = json!("other.identity");
        let m = build(&d).unwrap();
        assert_eq!(m.upgrade_from(&baseline), UpgradeDecision::Incompatible);
        assert_eq!(
            validate_manifest_set(&[baseline.clone(), m]),
            Err(ManifestError::ConflictingIdentity)
        );
    }
    let http = ConnectorManifest::from_json(&fixture("valid-http.json")).unwrap();
    assert_eq!(validate_manifest_set(&[baseline, http]), Ok(()));
}

#[test]
fn golden_canonical_bytes_and_absent_optional_values_normalize() {
    let baseline = ConnectorManifest::from_json(EXAMPLE).unwrap();
    assert_eq!(baseline.canonical_json(), fixture("canonical-stdio.json"));
    let mut d = definition();
    d.as_object_mut().unwrap().remove("deprecation");
    for entry in d["inventory"].as_array_mut().unwrap() {
        for key in ["capability_mapping_ref", "source_mapping_ref"] {
            if entry[key].is_null() {
                entry.as_object_mut().unwrap().remove(key);
            }
        }
    }
    assert_eq!(build(&d).unwrap(), baseline);
    let wire = json!({"manifest": d, "digest": baseline.digest()});
    assert_eq!(
        ConnectorManifest::from_json(&serde_json::to_vec(&wire).unwrap()).unwrap(),
        baseline
    );
}

#[test]
fn same_kind_entries_sort_by_name_and_metadata_is_bound_to_digest() {
    let mut d = definition();
    let mut tool = d["inventory"][0].clone();
    tool["name"] = json!("aaa.read");
    d["inventory"].as_array_mut().unwrap().push(tool);
    let snapshot = build(&d).unwrap();
    assert_eq!(snapshot.definition().inventory[0].name, "aaa.read");
    d["inventory"].as_array_mut().unwrap().reverse();
    assert_eq!(build(&d).unwrap(), snapshot);
    let baseline = ConnectorManifest::from_json(EXAMPLE).unwrap();
    for (parent, key, value) in [
        ("profiles", "authentication", json!("auth.other")),
        ("profiles", "trust", json!("trust.other")),
        ("profiles", "sensitivity", json!("sensitivity.other")),
        ("health", "timeout_seconds", json!(6)),
        ("server_version", "patch", json!(1)),
    ] {
        let mut wire: Value = serde_json::from_slice(EXAMPLE).unwrap();
        wire["manifest"][parent][key] = value;
        assert_eq!(
            ConnectorManifest::from_json(&serde_json::to_vec(&wire).unwrap()),
            Err(ManifestError::DigestMismatch)
        );
        assert_ne!(
            build(&wire["manifest"]).unwrap().digest(),
            baseline.digest()
        );
    }
}
