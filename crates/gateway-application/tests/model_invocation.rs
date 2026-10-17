#[allow(dead_code)]
#[path = "../../../tests/fixtures/model-invocation-v1/support.rs"]
mod support;
use gateway_application::model_invocation::*;
use gateway_domain::{model_invocation::*, *};
use std::cell::Cell;
use support::*;

#[test]
fn prepared_adapter_boundary_carries_all_controls_and_audit_provenance() {
    let request = prepare_model_invocation(
        admit(invocation_data()).unwrap(),
        capability(),
        provider(),
        &runtime(),
    )
    .unwrap();
    assert_eq!(
        request.invocation().data().invocation_id.as_str(),
        "invocation-1"
    );
    assert_eq!(request.capability(), &capability());
    assert_eq!(request.provider(), &provider());
    assert_eq!(request.effective_generation().controls.len(), 5);
    assert!(
        serde_json::to_value(&request)
            .unwrap()
            .get("effective_generation")
            .is_some()
    );
    struct Adapter {
        calls: Cell<usize>,
    }
    impl ModelInvocationPort for Adapter {
        fn invoke(
            &self,
            input: &PreparedModelInvocation,
        ) -> Result<ModelProposal, ModelContractError> {
            self.calls.set(self.calls.get() + 1);
            assert_eq!(
                input.effective_generation().requested_profile_digest,
                profile().content_digest()
            );
            Ok(ModelProposal {
                invocation_digest: input.invocation().content_digest(),
                raw_output: pin("unverified-output"),
            })
        }
    }
    let adapter = Adapter {
        calls: Cell::new(0),
    };
    let proposal = adapter.invoke(&request).unwrap();
    assert_eq!(adapter.calls.get(), 1);
    assert_eq!(
        proposal.invocation_digest,
        request.invocation().content_digest()
    );
    assert!(
        serde_json::to_value(proposal)
            .unwrap()
            .get("raw_output")
            .is_some()
    );
}

#[test]
fn health_is_a_separate_check_after_semantic_eligibility() {
    let inv = admit(invocation_data()).unwrap();
    for state in [ModelAvailability::Unavailable, ModelAvailability::Unknown] {
        let mut health = runtime();
        health.availability = state;
        assert_eq!(
            validate_model_eligibility(&inv, &capability(), &provider()),
            Ok(())
        );
        assert_eq!(
            prepare_model_invocation(inv.clone(), capability(), provider(), &health).unwrap_err(),
            ModelContractError::ModelUnavailable
        );
    }
    let mut health = runtime();
    health.provider.revision = ReferenceId::new("8").unwrap();
    assert_eq!(
        prepare_model_invocation(inv.clone(), capability(), provider(), &health).unwrap_err(),
        ModelContractError::InvalidBinding
    );
    let mut health = runtime();
    health.provider.contract_version = SchemaVersion::V2;
    assert_eq!(
        prepare_model_invocation(inv.clone(), capability(), provider(), &health).unwrap_err(),
        ModelContractError::IncompatibleVersion
    );
    let mut health = runtime();
    health.observation.digest = ContentDigest::new("A".repeat(64)).unwrap();
    assert_eq!(
        prepare_model_invocation(inv, capability(), provider(), &health).unwrap_err(),
        ModelContractError::InvalidBinding
    );
}

#[test]
fn unsupported_controls_and_domains_cannot_cross_preparation() {
    let inv = admit(invocation_data()).unwrap();
    for control in profile().data().controls.keys() {
        let mut p = provider_data();
        p.controls.remove(control);
        p.defaults.remove(control);
        p.unexposed_defaults.remove(control);
        assert_eq!(
            prepare_model_invocation(
                inv.clone(),
                capability(),
                ModelProvider::try_from(p).unwrap(),
                &runtime()
            )
            .unwrap_err(),
            ModelContractError::ModelCapabilityMismatch
        );
    }
    let mut p = provider_data();
    p.controls.insert(
        GenerationControl::Seed,
        GenerationDomain::Integer { min: 0, max: 1 },
    );
    assert_eq!(
        prepare_model_invocation(
            inv,
            capability(),
            ModelProvider::try_from(p).unwrap(),
            &runtime()
        )
        .unwrap_err(),
        ModelContractError::ModelCapabilityMismatch
    );
}
