#[allow(dead_code)]
#[path = "../../../tests/fixtures/model-invocation-v1/support.rs"]
mod support;
use gateway_domain::{cognitive_routing::TaskClass, model_invocation::*, *};
use serde_json::json;
use std::collections::BTreeSet;
use support::*;

#[test]
fn immutable_contracts_roundtrip_and_deterministic_profile_identity() {
    let p = profile();
    assert_eq!(
        p,
        serde_json::from_value(serde_json::to_value(&p).unwrap()).unwrap()
    );
    let mut equivalent = p.data().clone();
    equivalent.controls.insert(
        GenerationControl::Temperature,
        GenerationValue::Scalar(-0.0),
    );
    assert_eq!(
        p.content_digest(),
        GenerationProfile::try_from(equivalent)
            .unwrap()
            .content_digest()
    );
    assert_eq!(
        provider(),
        serde_json::from_value(serde_json::to_value(provider()).unwrap()).unwrap()
    );
    assert_eq!(
        capability(),
        serde_json::from_value(serde_json::to_value(capability()).unwrap()).unwrap()
    );
    let invocation = admit(invocation_data()).unwrap();
    assert_eq!(
        invocation.content_digest(),
        admit(invocation_data()).unwrap().content_digest()
    );
    assert_eq!(invocation.data().compiled_context, pin("compiled-context"));
    assert_eq!(invocation.data().output, output());
    assert_eq!(invocation.data().permitted_capabilities.len(), 0);
    assert!(
        serde_json::to_string(&invocation)
            .unwrap()
            .contains("compiled_context")
    );
    for error in [
        ModelContractError::IncompatibleVersion,
        ModelContractError::InvalidBinding,
        ModelContractError::InvalidLimits,
        ModelContractError::InvalidGenerationParameter,
        ModelContractError::ModelCapabilityMismatch,
        ModelContractError::ContextLimitExceeded,
        ModelContractError::AuthorityMismatch,
        ModelContractError::ModelUnavailable,
    ] {
        assert_eq!(
            error.to_string(),
            serde_json::to_value(error).unwrap().as_str().unwrap()
        );
    }
}

#[test]
fn incompatible_versions_unknown_provider_fields_and_unbounded_wire_values_fail() {
    let values = [
        serde_json::to_value(profile()).unwrap(),
        serde_json::to_value(provider()).unwrap(),
        serde_json::to_value(capability()).unwrap(),
    ];
    for (index, original) in values.into_iter().enumerate() {
        for (field, bad) in [
            ("schema_version", json!("2.0")),
            ("schema_version", json!("1.1")),
            ("vendor_options", json!({"arbitrary": 1})),
        ] {
            let mut value = original.clone();
            value[field] = bad;
            let rejected = match index {
                0 => serde_json::from_value::<GenerationProfile>(value).is_err(),
                1 => serde_json::from_value::<ModelProvider>(value).is_err(),
                _ => serde_json::from_value::<ModelCapability>(value).is_err(),
            };
            assert!(rejected);
        }
    }
    for bad in [
        json!(null),
        json!(-1),
        json!(4294967296u64),
        json!(0),
        json!("unlimited"),
    ] {
        let mut value = serde_json::to_value(profile()).unwrap();
        value["max_output_tokens"] = bad;
        assert!(serde_json::from_value::<GenerationProfile>(value).is_err());
    }
    let original = serde_json::to_string(&profile()).unwrap();
    assert!(serde_json::from_str::<GenerationProfile>(&original.replace("0.0", "1e999")).is_err());
    let mut bad = serde_json::to_value(invocation_data()).unwrap();
    bad["provider_sdk"] = json!({});
    assert!(serde_json::from_value::<ModelInvocationData>(bad).is_err());
}

#[test]
fn rejects_nonfinite_and_invalid_generation_controls_before_dispatch() {
    use GenerationControl::*;
    let invalid = [
        (Temperature, GenerationValue::Scalar(f64::NAN)),
        (Temperature, GenerationValue::Scalar(f64::INFINITY)),
        (Temperature, GenerationValue::Scalar(f64::NEG_INFINITY)),
        (Temperature, GenerationValue::Scalar(-0.1)),
        (TopP, GenerationValue::Scalar(0.0)),
        (TopP, GenerationValue::Scalar(1.01)),
        (TopK, GenerationValue::Integer(0)),
        (Temperature, GenerationValue::Integer(1)),
        (StopConditions, GenerationValue::Stops(BTreeSet::new())),
        (
            StopConditions,
            GenerationValue::Stops(BTreeSet::from([String::new()])),
        ),
        (
            StopConditions,
            GenerationValue::Stops(BTreeSet::from(["x".repeat(4097)])),
        ),
        (
            StopConditions,
            GenerationValue::Stops((0..129).map(|i| i.to_string()).collect()),
        ),
    ];
    for (control, value) in invalid {
        let mut d = profile().data().clone();
        d.controls.insert(control, value);
        assert_eq!(
            GenerationProfile::try_from(d),
            Err(ModelContractError::InvalidGenerationParameter)
        );
    }
    let mut d = profile().data().clone();
    d.reproducibility_class = ReproducibilityClass::Stochastic;
    assert!(GenerationProfile::try_from(d).is_err());
    let mut d = profile().data().clone();
    d.revision = ReferenceId::new("latest").unwrap();
    assert_eq!(
        GenerationProfile::try_from(d),
        Err(ModelContractError::InvalidBinding)
    );
}

#[test]
fn declarations_require_finite_typed_ranges_and_complete_default_provenance() {
    use GenerationControl::*;
    let domains = [
        (
            Temperature,
            GenerationDomain::Scalar {
                min: f64::NAN,
                max: 2.0,
            },
        ),
        (
            Temperature,
            GenerationDomain::Scalar {
                min: 0.0,
                max: f64::INFINITY,
            },
        ),
        (Temperature, GenerationDomain::Scalar { min: 2.0, max: 1.0 }),
        (
            Temperature,
            GenerationDomain::Scalar {
                min: -1.0,
                max: 1.0,
            },
        ),
        (TopP, GenerationDomain::Scalar { min: 0.0, max: 1.0 }),
        (TopP, GenerationDomain::Scalar { min: 0.1, max: 1.1 }),
        (TopK, GenerationDomain::Integer { min: 0, max: 1 }),
        (Seed, GenerationDomain::Integer { min: 2, max: 1 }),
        (
            StopConditions,
            GenerationDomain::Stops {
                max_count: 0,
                max_bytes: 100,
            },
        ),
        (
            StopConditions,
            GenerationDomain::Stops {
                max_count: 129,
                max_bytes: 100,
            },
        ),
        (
            StopConditions,
            GenerationDomain::Stops {
                max_count: 1,
                max_bytes: 0,
            },
        ),
        (
            StopConditions,
            GenerationDomain::Stops {
                max_count: 1,
                max_bytes: 4097,
            },
        ),
        (TopK, GenerationDomain::Scalar { min: 0.0, max: 1.0 }),
    ];
    for (control, domain) in domains {
        let mut d = provider_data();
        d.controls.insert(control, domain);
        assert_eq!(
            ModelProvider::try_from(d),
            Err(ModelContractError::InvalidLimits)
        );
    }
    let mut d = provider_data();
    d.defaults.insert(Temperature, GenerationValue::Scalar(3.0));
    assert_eq!(
        ModelProvider::try_from(d),
        Err(ModelContractError::ModelCapabilityMismatch)
    );
    let mut d = provider_data();
    d.defaults.insert(TopK, GenerationValue::Integer(0));
    assert!(ModelProvider::try_from(d).is_err());
    let mut d = provider_data();
    d.unexposed_defaults.insert(Temperature);
    assert_eq!(
        ModelProvider::try_from(d),
        Err(ModelContractError::InvalidGenerationParameter)
    );
    let mut d = provider_data();
    d.unexposed_defaults.clear();
    assert!(ModelProvider::try_from(d).is_err());
    for value in ["latest".to_owned(), "A".repeat(64)] {
        let mut d = provider_data();
        if value == "latest" {
            d.identity.revision = ReferenceId::new(value).unwrap();
        } else {
            d.identity.digest = ContentDigest::new(value).unwrap();
        }
        assert_eq!(
            ModelProvider::try_from(d),
            Err(ModelContractError::InvalidBinding)
        );
    }
    let mut d = provider_data();
    d.adapter.contract_version = SchemaVersion::V2;
    assert_eq!(
        ModelProvider::try_from(d),
        Err(ModelContractError::IncompatibleVersion)
    );
    let mut d = provider_data();
    d.provenance.digest = ContentDigest::new("A".repeat(64)).unwrap();
    assert!(ModelProvider::try_from(d).is_err());
}

#[test]
fn capability_limits_and_exact_contract_keys_are_validated() {
    let base = serde_json::to_value(capability_data()).unwrap();
    for (field, value) in [
        ("context_limit", json!(0)),
        ("max_output_tokens", json!(0)),
        ("max_output_tokens", json!(4097)),
        ("task_classes", json!([])),
        ("input_contracts", json!([])),
        ("output_contracts", json!({})),
        ("structured_output_modes", json!([])),
        ("reasoning_strategies", json!([])),
    ] {
        let mut d = base.clone();
        d[field] = value;
        assert!(serde_json::from_value::<ModelCapability>(d).is_err());
    }
    let mut d = capability_data();
    d.output_contracts.values_mut().next().unwrap().id = ReferenceId::new("other").unwrap();
    assert_eq!(
        ModelCapability::try_from(d),
        Err(ModelContractError::InvalidBinding)
    );
    let mut d = capability_data();
    d.output_contracts
        .values_mut()
        .next()
        .unwrap()
        .contract_version = SchemaVersion::V2;
    assert!(ModelCapability::try_from(d).is_err());
    let mut d = capability_data();
    d.model.digest = ContentDigest::new("A".repeat(64)).unwrap();
    assert!(ModelCapability::try_from(d).is_err());
}

#[test]
fn effective_controls_preserve_requests_defaults_and_unexposed_values() {
    let full = effective_generation(&profile(), &provider()).unwrap();
    assert_eq!(full.controls.len(), 5);
    assert!(full.unexposed_defaults.is_empty());
    for value in full.controls.values() {
        assert_eq!(value.origin, GenerationParameterOrigin::RequestedProfile);
        assert_eq!(value.provenance, profile().content_digest());
    }
    let mut d = profile().data().clone();
    d.controls.clear();
    d.reproducibility_class = ReproducibilityClass::Stochastic;
    let sparse = GenerationProfile::try_from(d).unwrap();
    let effective = effective_generation(&sparse, &provider()).unwrap();
    assert_eq!(
        effective.controls[&GenerationControl::Temperature].origin,
        GenerationParameterOrigin::AdapterDefault
    );
    assert_eq!(
        effective.controls[&GenerationControl::Temperature].provenance,
        provider().content_digest()
    );
    assert_eq!(effective.unexposed_defaults.len(), 4);
    assert_eq!(
        effective.provider_declaration_digest,
        provider().content_digest()
    );
    assert_eq!(effective.requested_profile_digest, sparse.content_digest());
    assert_eq!(effective.max_output_tokens, 128);
    assert_eq!(
        effective.structured_output_mode,
        StructuredOutputMode::JsonSchema
    );
    assert!(
        serde_json::to_value(effective)
            .unwrap()
            .get("unexposed_defaults")
            .is_some()
    );
    for control in profile().data().controls.keys() {
        let mut d = provider_data();
        d.controls.remove(control);
        d.defaults.remove(control);
        d.unexposed_defaults.remove(control);
        assert_eq!(
            effective_generation(&profile(), &ModelProvider::try_from(d).unwrap()),
            Err(ModelContractError::ModelCapabilityMismatch)
        );
    }
    for (control, domain) in [
        (
            GenerationControl::TopK,
            GenerationDomain::Integer { min: 1, max: 10 },
        ),
        (
            GenerationControl::StopConditions,
            GenerationDomain::Stops {
                max_count: 1,
                max_bytes: 3,
            },
        ),
    ] {
        let mut d = provider_data();
        d.controls.insert(control, domain);
        assert!(effective_generation(&profile(), &ModelProvider::try_from(d).unwrap()).is_err());
    }
}

#[test]
fn invocation_binds_captured_task_context_schema_scope_and_authority() {
    let original = serde_json::to_value(invocation_data()).unwrap();
    let cases = [
        ("/schema_version", json!("2.0")),
        ("/output/schema_version", json!("2.0")),
        ("/task/id", json!("another-task")),
        ("/task/digest", json!("b".repeat(64))),
        ("/task/scope", json!("other-project")),
        ("/task/contract", json!("other-contract")),
        ("/compiled_context/digest", json!("b".repeat(64))),
        ("/compiled_context/scope", json!("other-project")),
        ("/output/shape/type", json!("NUMBER")),
        ("/output/schema/revision", json!("8")),
        ("/authority/revision", json!("8")),
        ("/authority/scope", json!("other-project")),
        ("/sensitivity", json!("PUBLIC")),
        ("/timeout_ms", json!(0)),
        ("/max_attempts", json!(0)),
        ("/input_tokens", json!(0)),
        ("/provenance/digest", json!("A".repeat(64))),
    ];
    for (pointer, value) in cases {
        let mut changed = original.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        let data: ModelInvocationData = serde_json::from_value(changed).unwrap();
        assert!(admit(data).is_err(), "accepted {pointer}");
    }
    let mut d = invocation_data();
    d.permitted_capabilities.insert(
        CapabilityId::new("read").unwrap(),
        ResolvedReference {
            id: CapabilityId::new("read").unwrap(),
            scope: pin("scope").scope,
            contract: ReferenceId::new("tool").unwrap(),
            contract_version: SchemaVersion::V1,
            revision: ReferenceId::new("7").unwrap(),
            digest: pin("digest").digest,
        },
    );
    assert_eq!(admit(d.clone()), Err(ModelContractError::AuthorityMismatch));
    let granted = d.permitted_capabilities.clone();
    let admitted = ModelInvocation::new(
        d.clone(),
        ModelInvocationBasis {
            task: &task(),
            output: &output(),
            compiled_context: &pin("compiled-context"),
            permitted_capabilities: &granted,
            sensitivity: SensitivityClass::Internal,
            permitted_localities: &BTreeSet::from([
                gateway_domain::cognitive_routing::PrivacyBoundary::OnDevice,
            ]),
            authority: &pin("policy-process-admission"),
        },
    )
    .unwrap();
    assert_eq!(admitted.data().permitted_capabilities, granted);
    assert!(validate_model_eligibility(&admitted, &capability(), &provider()).is_ok());
    d.permitted_capabilities.values_mut().next().unwrap().digest =
        ContentDigest::new("A".repeat(64)).unwrap();
    assert!(admit(d).is_err());
}

#[test]
fn semantic_eligibility_is_exact_and_never_depends_on_health() {
    let inv = admit(invocation_data()).unwrap();
    assert_eq!(
        validate_model_eligibility(&inv, &capability(), &provider()),
        Ok(())
    );
    let base = serde_json::to_value(capability()).unwrap();
    for (pointer, value) in [
        ("/identity/revision", json!("8")),
        ("/provider/revision", json!("8")),
        ("/task_classes", json!(["REASONING"])),
        ("/input_contracts", json!(["other"])),
        ("/output_contracts/ArtifactInspection/revision", json!("8")),
        ("/reasoning_strategies", json!(["other"])),
        ("/structured_output_modes", json!(["TEXT"])),
        ("/context_limit", json!(1024)),
        ("/max_output_tokens", json!(100)),
    ] {
        let mut d = base.clone();
        *d.pointer_mut(pointer).unwrap() = value;
        let cap: ModelCapability = serde_json::from_value(d).unwrap();
        assert!(
            validate_model_eligibility(&inv, &cap, &provider()).is_err(),
            "accepted {pointer}"
        );
    }
    let mut p = provider_data();
    p.maximum_sensitivity = SensitivityClass::Public;
    assert_eq!(
        validate_model_eligibility(&inv, &capability(), &ModelProvider::try_from(p).unwrap()),
        Err(ModelContractError::ModelCapabilityMismatch)
    );
    let mut d = invocation_data();
    d.input_tokens = u32::MAX;
    assert_eq!(
        validate_model_eligibility(&admit(d).unwrap(), &capability(), &provider()),
        Err(ModelContractError::ContextLimitExceeded)
    );
    let mut d = invocation_data();
    let mut g = profile().data().clone();
    g.structured_output_mode = StructuredOutputMode::ToolProposal;
    d.generation = GenerationProfile::try_from(g).unwrap();
    let inv = admit(d).unwrap();
    let mut c = capability_data();
    c.tool_calling = false;
    assert_eq!(
        validate_model_eligibility(&inv, &ModelCapability::try_from(c).unwrap(), &provider()),
        Err(ModelContractError::ModelCapabilityMismatch)
    );
    let mut d = invocation_data();
    d.task_class = TaskClass::Reasoning;
    assert!(validate_model_eligibility(&admit(d).unwrap(), &capability(), &provider()).is_err());
}

#[test]
fn frozen_positive_and_negative_fixtures_are_enforced() {
    let canonical =
        include_str!("../../../tests/fixtures/model-invocation-v1/profile.canonical.json");
    assert_eq!(serde_json::to_string(&profile()).unwrap(), canonical);
    assert_eq!(
        profile().content_digest().as_str(),
        include_str!("../../../tests/fixtures/model-invocation-v1/profile.sha256").trim()
    );
    let negatives: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../tests/fixtures/model-invocation-v1/negative-profiles.json"
    ))
    .unwrap();
    for case in negatives {
        assert!(
            serde_json::from_value::<GenerationProfile>(case["profile"].clone()).is_err(),
            "{}",
            case["case"]
        );
    }
}

#[test]
fn disclosure_locality_cannot_expand_and_remote_provider_is_ineligible() {
    use gateway_domain::cognitive_routing::PrivacyBoundary;
    let mut data = invocation_data();
    data.permitted_localities.insert(PrivacyBoundary::External);
    assert_eq!(admit(data), Err(ModelContractError::AuthorityMismatch));
    let mut data = invocation_data();
    data.permitted_localities.clear();
    assert_eq!(admit(data), Err(ModelContractError::AuthorityMismatch));
    let mut p = provider_data();
    p.locality = PrivacyBoundary::External;
    assert_eq!(
        validate_model_eligibility(
            &admit(invocation_data()).unwrap(),
            &capability(),
            &ModelProvider::try_from(p).unwrap()
        ),
        Err(ModelContractError::ModelCapabilityMismatch)
    );
}

#[test]
fn profile_permutations_preserve_bytes_but_material_changes_change_identity() {
    let original = profile();
    let mut permuted = serde_json::to_value(&original).unwrap();
    permuted["controls"]["STOP_CONDITIONS"]["value"] = json!(["STOP", "END", "STOP"]);
    let equivalent: GenerationProfile = serde_json::from_value(permuted).unwrap();
    assert_eq!(
        serde_json::to_vec(&original).unwrap(),
        serde_json::to_vec(&equivalent).unwrap()
    );
    assert_eq!(original.content_digest(), equivalent.content_digest());
    let mut changed = original.data().clone();
    changed.max_output_tokens += 1;
    assert_ne!(
        original.content_digest(),
        GenerationProfile::try_from(changed)
            .unwrap()
            .content_digest()
    );
}
