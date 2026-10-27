use super::*;
use crate::local_sessions::runtime_admission::tests::Fixture;

fn plan() -> SessionResourcePlan {
    SessionResourcePlan {
        preparation: SessionResourceAmounts {
            model_attempts: 2,
            input_tokens: 100,
            output_tokens: 20,
            output_bytes: 4096,
            connector_calls: 4,
            retrieval_rounds: 4,
            retrieval_results: 16,
            retrieval_tokens: 4096,
            cost_microunits: 400,
            ..Default::default()
        },
        execution: SessionResourceAmounts {
            connector_calls: 2,
            cost_microunits: 18,
            ..Default::default()
        },
        ..Default::default()
    }
}
fn tariff() -> ConnectorTariff {
    ConnectorTariff {
        discovery: 7,
        operation: 11,
    }
}
fn model() -> ModelBudget {
    ModelBudget {
        attempts: 2,
        input: 100,
        output: 20,
        timeout_ms: 30_000,
        pricing: PricingDeclaration {
            cost_unit: ReferenceId::new("credits").unwrap(),
            fixed_per_attempt_microunits: 10,
            input_microunits_per_unit: 1,
            output_microunits_per_unit: 2,
        },
    }
}
#[test]
fn disjoint_children_preserve_all_resources_without_multiplying_run_tokens() {
    let p = plan();
    let result = split(&p, tariff(), model()).unwrap();
    assert_eq!(result.connector.cost_microunits, 40);
    assert_eq!(result.model.cost_microunits, 360);
    assert_eq!(result.model.input_tokens, 100); // Two attempts share this ceiling.
    assert_eq!(result.model.output_tokens, 20);
    assert_eq!(result.model.model_attempts, 2);
    assert_eq!(result.model.connector_calls, 0);
    assert_eq!(result.connector.model_attempts, 0);
    assert_eq!(
        result.model.checked_add(&result.connector).unwrap(),
        p.preparation
    );
    assert_eq!(p.execution.cost_microunits, 18);
}
#[test]
fn model_cost_funds_all_attempts_and_connector_cost_is_never_borrowed() {
    let mut p = plan();
    p.preparation.cost_microunits = 340; // Connector40 + model(100+40+10)*2.
    assert!(split(&p, tariff(), model()).is_ok());
    p.preparation.cost_microunits -= 1;
    assert!(matches!(
        split(&p, tariff(), model()),
        Err(SessionError::LimitExceeded)
    ));
    p.preparation.cost_microunits = 39;
    assert!(matches!(
        split(&p, tariff(), model()),
        Err(SessionError::LimitExceeded)
    ));
}
#[test]
fn checked_tariffs_and_model_pricing_reject_overflow() {
    assert!(matches!(
        tariff().maximum(0),
        Err(SessionError::LimitExceeded)
    ));
    assert!(matches!(
        ConnectorTariff {
            discovery: 1,
            operation: u64::MAX
        }
        .maximum(3),
        Err(SessionError::LimitExceeded)
    ));
    let mut m = model();
    m.pricing.input_microunits_per_unit = u64::MAX;
    assert!(matches!(
        split(&plan(), tariff(), m),
        Err(SessionError::LimitExceeded)
    ));
    let mut p = plan();
    p.execution.cost_microunits = u64::MAX;
    assert!(matches!(
        split(&p, tariff(), model()),
        Err(SessionError::LimitExceeded)
    ));
}
#[test]
fn native_phase_rules_and_unsupported_compute_are_not_silently_redistributed() {
    let mut p = plan();
    p.approved_resume.model_attempts = 1;
    assert!(matches!(
        split(&p, tariff(), model()),
        Err(SessionError::InvalidInput)
    ));
    p = plan();
    p.execution.input_tokens = 1;
    assert!(matches!(
        split(&p, tariff(), model()),
        Err(SessionError::InvalidInput)
    ));
    p = plan();
    p.goal_verification.retrieval_results = 1;
    assert!(matches!(
        split(&p, tariff(), model()),
        Err(SessionError::UnsupportedCapability)
    ));
    for field in ["worker_attempts", "worker_compute_units", "repair_attempts"] {
        let mut value = serde_json::to_value(plan()).unwrap();
        value["preparation"][field] = json!(1);
        let p = serde_json::from_value(value).unwrap();
        assert!(matches!(
            split(&p, tariff(), model()),
            Err(SessionError::UnsupportedCapability)
        ));
    }
    p = plan();
    p.execution.cost_microunits = 17;
    assert!(matches!(
        split(&p, tariff(), model()),
        Err(SessionError::LimitExceeded)
    ));
}
#[test]
fn actual_invocation_caps_and_output_bytes_are_required_before_allocation() {
    for field in [
        "model_attempts",
        "input_tokens",
        "output_tokens",
        "output_bytes",
    ] {
        let mut value = serde_json::to_value(plan()).unwrap();
        value["preparation"][field] = json!(0);
        let p = serde_json::from_value(value).unwrap();
        assert!(matches!(
            split(&p, tariff(), model()),
            Err(SessionError::LimitExceeded)
        ));
    }
    let mut m = model();
    m.input += 1;
    assert!(matches!(
        split(&plan(), tariff(), m),
        Err(SessionError::LimitExceeded)
    ));
    let mut m = model();
    m.output += 1;
    assert!(matches!(
        split(&plan(), tariff(), m),
        Err(SessionError::LimitExceeded)
    ));
    let mut m = model();
    m.attempts += 1;
    assert!(matches!(
        split(&plan(), tariff(), m),
        Err(SessionError::LimitExceeded)
    ));
}

fn document(f: &mut Fixture, id: &str) -> Value {
    let path = f.runtime()["documents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["pin"]["id"] == id)
        .unwrap()["path"]
        .as_str()
        .unwrap()
        .to_owned();
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}
#[test]
fn current_native_model_and_registered_tariff_drive_the_plan_with_exact_units() {
    let mut f = Fixture::with_scope("phase-planning-native");
    let native = f.install_actual_mixed_model();
    super::super::model_factory::tests::install(&mut f, "http://127.0.0.1:1", None, &native);
    let mut model = document(&mut f, "model");
    model["max_attempts"] = json!(2);
    f.rewrite("model", model);
    let mut quote = document(&mut f, "quotes");
    quote["tariff"]["discovery_cost_microunits"] = json!(7);
    quote["tariff"]["operation_cost_microunits"] = json!(11);
    f.rewrite("quotes", quote);
    let mut resources = plan();
    resources.preparation.input_tokens = 131072;
    resources.preparation.output_tokens = 4096;
    f.runtime()["steps"][0]["resources"] = serde_json::to_value(&resources).unwrap();
    f.wire["mappings"][0]["sessions"]["resource_limits"]["amounts"] =
        serde_json::to_value(resources.total().unwrap()).unwrap();
    let host = f.host().unwrap();
    let mut runtime = host
        .sessions
        .as_ref()
        .unwrap()
        .connector_runtime
        .as_ref()
        .unwrap()
        .clone();
    let step = PlanStepId::new(&runtime.0.steps[0].step).unwrap();
    let unit = ReferenceId::new("credits").unwrap();
    // No loaded evidence means no plan; capture never loads or refreshes itself.
    assert!(NativePhasePlan::capture(&runtime, &step, &unit).is_err());
    let loaded = runtime.load(&host).unwrap();
    runtime.retain(loaded);
    let result = NativePhasePlan::capture(&runtime, &step, &unit).unwrap();
    assert_eq!(result.resources(), &resources);
    assert_eq!(result.preparation().connector().cost_microunits, 40);
    assert_eq!(result.preparation().model().cost_microunits, 360);
    assert_eq!(result.preparation().model().input_tokens, 131072);
    assert_eq!(
        result
            .preparation()
            .model()
            .checked_add(result.preparation().connector())
            .unwrap(),
        resources.preparation
    );
    assert!(matches!(
        NativePhasePlan::capture(
            &runtime,
            &step,
            &ReferenceId::new("other-currency").unwrap()
        ),
        Err(SessionError::StaleRevision)
    ));
    assert!(matches!(
        NativePhasePlan::capture(&runtime, &PlanStepId::new("foreign-step").unwrap(), &unit),
        Err(SessionError::AuthorityDenied)
    ));
}
