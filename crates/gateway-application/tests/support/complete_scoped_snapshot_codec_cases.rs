use crate::fixture::{self, Fixture};
use gateway_application::sessions::scoped_snapshots::*;
use gateway_domain::*;
use serde_json::{Value, json};

fn reload(f: &Fixture, bytes: &[u8]) -> Result<CompleteScopedSnapshot, SnapshotError> {
    CompleteScopedSnapshot::decode(bytes, &f.binding, &f.universe, &f.profile, f.now_ms)
}
fn document(f: &Fixture) -> Value {
    serde_json::from_slice(&f.validate().unwrap().encode().unwrap()).unwrap()
}
fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}

#[test]
fn canonical_reload_preserves_complete_native_data_for_all_consistency_profiles() {
    for f in [
        Fixture::atomic(),
        Fixture::versioned(),
        Fixture::bracketed(),
    ] {
        let complete = f.validate().unwrap();
        let encoded = complete.encode().unwrap();
        let loaded = reload(&f, &encoded).unwrap();
        assert_eq!(loaded, complete);
        assert_eq!(loaded.encode().unwrap(), encoded);
        assert_eq!(loaded.object_versions().len(), f.universe.objects.len());
        assert_eq!(loaded.acquisitions(), f.acquisitions);
        // The complete universe includes actual non-goal integer facts and
        // an object whose ready condition is false; completeness is not truth.
        assert_eq!(loaded.universe().objects.len(), 2);
    }
}

#[test]
fn reload_requires_independently_selected_full_binding_and_definition_pins() {
    let f = Fixture::atomic();
    let encoded = f.validate().unwrap().encode().unwrap();
    let mut binding = f.binding.clone();
    binding.execution = fixture::text("foreign-execution");
    assert_eq!(
        CompleteScopedSnapshot::decode(&encoded, &binding, &f.universe, &f.profile, f.now_ms),
        Err(SnapshotError::BindingMismatch)
    );
    let mut universe = f.universe.clone();
    universe.objects.reverse();
    assert_eq!(
        CompleteScopedSnapshot::decode(&encoded, &f.binding, &universe, &f.profile, f.now_ms),
        Err(SnapshotError::UniverseMismatch)
    );
    let mut profile = f.profile.clone();
    profile.authority_domain.revision = ReferenceId::new("revision-19").unwrap();
    assert_eq!(
        CompleteScopedSnapshot::decode(&encoded, &f.binding, &f.universe, &profile, f.now_ms),
        Err(SnapshotError::ProfileMismatch)
    );
    assert_eq!(
        CompleteScopedSnapshot::decode(
            &encoded,
            &f.binding,
            &f.universe,
            &f.profile,
            f.now_ms + (f.profile.max_age_seconds + 1) * 1000
        ),
        Err(SnapshotError::Expired)
    );
}

#[test]
fn closed_codec_rejects_unknown_fields_at_every_owned_layer() {
    let f = Fixture::versioned();
    let original = document(&f);
    for path in [
        "",
        "/data",
        "/data/binding",
        "/data/universe",
        "/data/universe/objects/0",
        "/data/universe/objects/0/route",
        "/data/universe/objects/0/route/target",
        "/data/profile",
        "/data/acquisitions/0",
        "/data/acquisitions/0/request",
        "/data/acquisitions/0/observation",
        "/data/acquisitions/0/observation/metadata",
        "/data/acquisitions/0/batch",
        "/data/object_reads/0",
        "/data/consistency",
        "/data/consistency/anchor",
    ] {
        let mut value = original.clone();
        value
            .pointer_mut(path)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("forged_complete".into(), json!(true));
        assert_eq!(
            reload(&f, &bytes(&value)),
            Err(SnapshotError::InvalidEncoding),
            "layer {path}"
        );
    }
}

#[test]
fn duplicate_json_keys_are_rejected_in_outer_nested_and_native_record_payloads() {
    let f = Fixture::atomic();
    let original = document(&f);
    let encoded = String::from_utf8(bytes(&original)).unwrap();
    for key in [
        "contract",
        "execution",
        "object_reference",
        "source",
        "partial",
    ] {
        let needle = format!("\"{key}\":");
        let malformed = encoded.replacen(&needle, &format!("\"{key}\":null,{needle}"), 1);
        assert_ne!(malformed, encoded);
        assert_eq!(
            reload(&f, malformed.as_bytes()),
            Err(SnapshotError::InvalidEncoding)
        );
    }
    // Duplicate keys inside opaque native record JSON are checked before the
    // native parser, including escaped aliases of the same decoded key.
    for path in [
        "/data/acquisitions/0/observation/records",
        "/data/acquisitions/0/batch/records",
    ] {
        let mut value = original.clone();
        let raw = value.pointer(path).unwrap().as_str().unwrap();
        let first_brace = raw.find('{').unwrap();
        let malformed = format!(
            "{}\"duplicate\":1,\"dupli\\u0063ate\":2,{}",
            &raw[..first_brace + 1],
            &raw[first_brace + 1..]
        );
        *value.pointer_mut(path).unwrap() = json!(malformed);
        assert_eq!(
            reload(&f, &bytes(&value)),
            Err(SnapshotError::InvalidEncoding)
        );
    }
}

#[test]
fn reload_rejects_noncanonical_trailing_deep_oversized_and_unsupported_envelopes() {
    let f = Fixture::atomic();
    let value = document(&f);
    let encoded = bytes(&value);
    let mut trailing = encoded.clone();
    trailing.extend_from_slice(b" {}");
    assert_eq!(reload(&f, &trailing), Err(SnapshotError::InvalidEncoding));
    let mut spaced = vec![b' '];
    spaced.extend_from_slice(&encoded);
    assert_eq!(reload(&f, &spaced), Err(SnapshotError::InvalidEncoding));
    assert_eq!(
        reload(&f, &vec![b' '; MAX_SNAPSHOT_BYTES + 1]),
        Err(SnapshotError::LimitExceeded)
    );
    let deep = format!(
        "{}0{}",
        "[".repeat(MAX_SNAPSHOT_DEPTH + 2),
        "]".repeat(MAX_SNAPSHOT_DEPTH + 2)
    );
    assert!(reload(&f, deep.as_bytes()).is_err());
    for (key, replacement) in [
        ("contract", json!("cg.some-other-record")),
        ("contract_version", json!("9.0")),
        ("digest", json!("invalid-digest")),
    ] {
        let mut altered = value.clone();
        altered[key] = replacement;
        assert_eq!(
            reload(&f, &bytes(&altered)),
            Err(SnapshotError::InvalidEncoding)
        );
    }
}

#[test]
fn encoded_digest_and_actual_native_evidence_are_both_revalidated() {
    let f = Fixture::atomic();
    let original = document(&f);
    let mut wrong_digest = original.clone();
    wrong_digest["digest"] =
        serde_json::to_value(context_content_digest(b"different immutable data")).unwrap();
    assert_eq!(
        reload(&f, &bytes(&wrong_digest)),
        Err(SnapshotError::DigestMismatch)
    );
    for (path, replacement) in [
        (
            "/data/acquisitions/0/observation/metadata/partial",
            json!(true),
        ),
        (
            "/data/acquisitions/0/observation/metadata/effect",
            json!("UNKNOWN"),
        ),
        (
            "/data/acquisitions/0/observation/basis",
            json!("PROVIDER_ACKNOWLEDGEMENT"),
        ),
        ("/data/acquisitions/0/request/step", json!("foreign-step")),
        ("/data/object_reads/0/source", json!("foreign-source")),
        ("/data/acquisitions/0/batch/records", json!("{}")),
    ] {
        let mut altered = original.clone();
        *altered.pointer_mut(path).unwrap() = replacement;
        assert!(reload(&f, &bytes(&altered)).is_err(), "altered {path}");
    }
}

#[test]
fn before_and_after_data_keep_distinct_exact_reservation_identity_without_effect_grants() {
    let before = Fixture::atomic();
    let mut after = Fixture::atomic();
    after.binding.phase = SnapshotPhase::After;
    after.binding.reservation = Some(SnapshotReservation {
        reservation: context_content_digest(b"actual retained reservation identity"),
        fence: gateway_application::sessions::FenceToken::new(7).unwrap(),
    });
    // This only binds the declared DATA phase. It does not establish that an
    // action happened, or that these readings causally follow an action.
    let snapshot = after.validate().unwrap();
    let encoded = snapshot.encode().unwrap();
    assert_ne!(snapshot.digest(), before.validate().unwrap().digest());
    assert_eq!(reload(&after, &encoded).unwrap(), snapshot);
    assert_eq!(
        reload(&before, &encoded),
        Err(SnapshotError::BindingMismatch)
    );
    let mut invalid_fence: Value = serde_json::from_slice(&encoded).unwrap();
    invalid_fence["data"]["binding"]["reservation"]["fence"] = json!(0);
    assert_eq!(
        reload(&after, &bytes(&invalid_fence)),
        Err(SnapshotError::InvalidEncoding)
    );
}

#[test]
fn duplicate_quality_subjects_and_unrepresentable_native_records_cannot_survive_reload() {
    let f = Fixture::atomic();
    let original = document(&f);
    let mut duplicate = original.clone();
    let quality = duplicate["data"]["acquisitions"][0]["batch"]["quality_metadata"]
        .as_array_mut()
        .unwrap();
    quality.push(quality[0].clone());
    assert_eq!(
        reload(&f, &bytes(&duplicate)),
        Err(SnapshotError::InvalidEncoding)
    );
    for path in [
        "/data/acquisitions/0/observation/records",
        "/data/acquisitions/0/batch/records",
    ] {
        let mut invalid = original.clone();
        *invalid.pointer_mut(path).unwrap() = json!("{\"observations\":[1.5]}");
        assert_eq!(
            reload(&f, &bytes(&invalid)),
            Err(SnapshotError::InvalidEvidence)
        );
    }
}

#[test]
fn supported_search_and_inspect_reads_roundtrip_without_becoming_effects() {
    use gateway_application::connectors::ConnectorOperationClass;
    for class in [
        ConnectorOperationClass::Search,
        ConnectorOperationClass::Inspect,
    ] {
        let mut f = Fixture::atomic();
        let mut actual = f.acquisitions[0].clone();
        actual.request.class = class;
        f.replace_acquisition(0, actual);
        for object in &mut f.universe.objects {
            object.route.class = class;
            fixture::repin_route(&mut object.route);
        }
        f.profile.anchor_route.class = class;
        fixture::repin_route(&mut f.profile.anchor_route);
        f.repin_universe();
        f.repin_profile();
        let complete = f.validate().unwrap();
        let loaded = reload(&f, &complete.encode().unwrap()).unwrap();
        assert_eq!(loaded, complete);
        assert_eq!(loaded.acquisitions()[0].request.class, class);
    }
}

#[test]
fn encoded_effect_classes_acknowledgements_and_post_effect_claims_are_not_direct_snapshot_reads() {
    let f = Fixture::atomic();
    let original = document(&f);
    for class in ["MUTATE", "ADMIN"] {
        let mut altered = original.clone();
        altered["data"]["acquisitions"][0]["request"]["class"] = json!(class);
        assert_eq!(
            reload(&f, &bytes(&altered)),
            Err(SnapshotError::InvalidEvidence)
        );
    }
    for effect in ["NOT_DISPATCHED", "ACKNOWLEDGED"] {
        let mut altered = original.clone();
        altered["data"]["acquisitions"][0]["observation"]["metadata"]["effect"] = json!(effect);
        assert_eq!(
            reload(&f, &bytes(&altered)),
            Err(SnapshotError::InvalidEvidence)
        );
    }
    let mut post_effect = original.clone();
    post_effect["data"]["acquisitions"][0]["observation"]["basis"] =
        json!("INDEPENDENT_POST_EFFECT_READ");
    post_effect["data"]["acquisitions"][0]["observation"]["metadata"]["verifies"] =
        original["data"]["acquisitions"][0]["request"].clone();
    assert_eq!(
        reload(&f, &bytes(&post_effect)),
        Err(SnapshotError::InvalidEvidence)
    );
}

#[test]
fn pin_bound_reload_checks_full_immutable_definitions_without_current_runtime_loading() {
    for f in [
        Fixture::atomic(),
        Fixture::versioned(),
        Fixture::bracketed(),
    ] {
        let complete = f.validate().unwrap();
        let encoded = complete.encode().unwrap();
        assert_eq!(
            CompleteScopedSnapshot::decode_bound(
                &encoded,
                &f.binding,
                &f.universe.pin,
                &f.profile.pin,
                f.now_ms
            )
            .unwrap(),
            complete
        );
        for field in [
            "id",
            "scope",
            "contract",
            "contract_version",
            "revision",
            "digest",
        ] {
            let mut pin = serde_json::to_value(&f.universe.pin).unwrap();
            pin[field] = match field {
                "contract_version" => json!("2.0"),
                "digest" => {
                    serde_json::to_value(context_content_digest(b"foreign definition")).unwrap()
                }
                _ => json!("foreign-definition"),
            };
            let foreign = serde_json::from_value::<SnapshotPin>(pin).unwrap();
            assert_eq!(
                CompleteScopedSnapshot::decode_bound(
                    &encoded,
                    &f.binding,
                    &foreign,
                    &f.profile.pin,
                    f.now_ms
                ),
                Err(SnapshotError::UniverseMismatch)
            );
            assert_eq!(
                CompleteScopedSnapshot::decode_bound(
                    &encoded,
                    &f.binding,
                    &f.universe.pin,
                    &foreign,
                    f.now_ms
                ),
                Err(SnapshotError::ProfileMismatch)
            );
        }
        let mut altered = document(&f);
        altered["data"]["universe"]["objects"][0]["object_reference"] = json!("foreign leaf");
        assert_eq!(
            CompleteScopedSnapshot::decode_bound(
                &bytes(&altered),
                &f.binding,
                &f.universe.pin,
                &f.profile.pin,
                f.now_ms
            ),
            Err(SnapshotError::UniverseMismatch)
        );
        let mut altered = document(&f);
        altered["data"]["profile"]["max_age_seconds"] = json!(f.profile.max_age_seconds + 1);
        assert_eq!(
            CompleteScopedSnapshot::decode_bound(
                &bytes(&altered),
                &f.binding,
                &f.universe.pin,
                &f.profile.pin,
                f.now_ms
            ),
            Err(SnapshotError::ProfileMismatch)
        );
    }
}

#[test]
fn snapshot_binding_data_serde_reuses_exact_closed_fence_and_step_wire_contract() {
    let mut f = Fixture::atomic();
    f.binding.reservation = Some(SnapshotReservation {
        reservation: context_content_digest(b"binding data only"),
        fence: gateway_application::sessions::FenceToken::new(2).unwrap(),
    });
    let value = serde_json::to_value(&f.binding).unwrap();
    assert_eq!(
        serde_json::from_value::<SnapshotBinding>(value.clone()).unwrap(),
        f.binding
    );
    assert_eq!(value["reservation"]["fence"], json!(2));
    for (path, invalid) in [
        ("/reservation/fence", json!(0)),
        ("/capture/step", json!("")),
    ] {
        let mut altered = value.clone();
        *altered.pointer_mut(path).unwrap() = invalid;
        assert!(serde_json::from_value::<SnapshotBinding>(altered).is_err());
    }
    let mut unknown = value;
    unknown["live_authority"] = json!(true);
    assert!(serde_json::from_value::<SnapshotBinding>(unknown).is_err());
}

#[test]
fn native_atomic_fixture_keeps_all_full_pin_and_evidence_bindings_in_actual_selected_scope() {
    let original = Fixture::atomic();
    let f = Fixture::atomic_in_scope(ContextScopeId::new("independent-snapshot-scope").unwrap());
    let complete = f.validate().unwrap();
    let encoded = complete.encode().unwrap();
    assert_eq!(reload(&f, &encoded).unwrap(), complete);
    assert_ne!(
        complete.universe().membership_digest().unwrap(),
        original.universe.membership_digest().unwrap()
    );
    assert!(
        complete
            .acquisitions()
            .iter()
            .all(|a| a.request.target.scope == f.binding.capture.scope
                && a.batch.scope() == &f.binding.capture.scope)
    );
    assert_eq!(
        reload(&original, &encoded),
        Err(SnapshotError::BindingMismatch)
    );
}

#[test]
fn all_profile_fixture_definitions_preserve_actual_selected_scope_before_codec_admission() {
    for kind in [
        SnapshotProfileKind::AtomicCompleteExport,
        SnapshotProfileKind::SnapshotScopedReads,
        SnapshotProfileKind::BracketedNonReusedAnchor,
    ] {
        let f = Fixture::in_scope(kind, ContextScopeId::new("profile-selected-scope").unwrap());
        let complete = f.validate().unwrap();
        assert_eq!(reload(&f, &complete.encode().unwrap()).unwrap(), complete);
        assert!(
            complete
                .universe()
                .objects
                .iter()
                .all(|o| o.object.scope == f.binding.capture.scope
                    && o.route.authority_domain.scope == f.binding.capture.scope)
        );
    }
}
