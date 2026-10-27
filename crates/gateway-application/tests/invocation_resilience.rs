#[path = "../../../tests/fixtures/prompt-rendering-v1/support.rs"]
mod support;
use gateway_application::{prompt_ir::*, prompt_rendering::*};
use gateway_domain::{prompt_ir::*, prompt_rendering::*, *};
use serde_json::{Value, json};
use std::cell::Cell;
use support::{examples::id, model::pin, *};

// Synthetic trusted store: authenticates the entire expected IR, including the
// complete producer handoff. State changes simulate independent host revocation.
struct Host {
    expected: Value,
    error: Cell<Option<InvocationIrError>>,
    calls: Cell<usize>,
}
impl SemanticReferenceValidator for Host {
    fn validate(
        &self,
        _: &SemanticTaskData,
        _: &SemanticReferenceBinding,
    ) -> Result<(), ValidationError> {
        if self.error.get() == Some(InvocationIrError::InvalidBinding) {
            Err(ValidationError::InvalidStateCombination {
                reason: "stale reference",
            })
        } else {
            Ok(())
        }
    }
}
impl InvocationAdmissionPort for Host {
    fn validate_current(&self, ir: &InvocationIr) -> Result<(), InvocationIrError> {
        self.calls.set(self.calls.get() + 1);
        if let Some(error) = self.error.get() {
            return Err(error);
        }
        if self.expected != json!(ir) {
            return Err(InvocationIrError::StaleInput);
        }
        Ok(())
    }
}
// First capture is explicitly a fixture-only authority, never production admission.
struct Capture;
impl SemanticReferenceValidator for Capture {
    fn validate(
        &self,
        _: &SemanticTaskData,
        _: &SemanticReferenceBinding,
    ) -> Result<(), ValidationError> {
        Ok(())
    }
}
impl InvocationAdmissionPort for Capture {
    fn validate_current(&self, _: &InvocationIr) -> Result<(), InvocationIrError> {
        Ok(())
    }
}
fn fixture() -> (InvocationIr, Host) {
    let h = handoff();
    let p = prepared(&h, 100_000, true);
    let tools = p
        .invocation()
        .data()
        .permitted_capabilities
        .iter()
        .map(|(id, pin)| {
            (
                id.clone(),
                ToolDefinition {
                    binding: pin.clone(),
                    description: NonEmptyText::new("Inspect").unwrap(),
                    input: gateway_domain::semantic_output::OutputType::String {},
                    connector_inventory: None,
                },
            )
        })
        .collect();
    let ir = admit_invocation_ir(SchemaVersion::V1, h, p, tools, &Capture).unwrap();
    let host = Host {
        expected: json!(ir),
        error: Cell::new(None),
        calls: Cell::new(0),
    };
    (ir, host)
}
struct Renderer;
impl PromptRendererPort for Renderer {
    fn identity(&self) -> (ReferenceId, ReferenceId) {
        (id("fixture"), id("1"))
    }
    fn render(&self, input: &RenderInput) -> Result<Value, RenderDiagnostic> {
        Ok(json!({"system":input.handoff()["instructions"], "data":render_data(input)}))
    }
}
struct Counter;
impl TokenizerPort for Counter {
    fn count(&self, r: &TokenCountRequest<'_>) -> Result<TokenCountResult, RenderDiagnostic> {
        Ok(TokenCountResult {
            profile: r.profile.clone(),
            payload_digest: r.payload_digest.clone(),
            mode: CountMode::Estimated,
            provenance: pin("fixture-bound"),
            usage: TokenUsage {
                instruction_tokens: 0,
                task_tokens: 0,
                context_tokens: 0,
                evidence_tokens: 0,
                tool_schema_tokens: 0,
                framing_tokens: r.payload.len() as u64,
            },
        })
    }
}
fn profile(ir: &InvocationIr) -> TokenizerProfile {
    TokenizerProfile {
        schema_version: SchemaVersion::V1,
        model: ir.prepared().capability().data().model.clone(),
        generation_profile: ir
            .prepared()
            .invocation()
            .data()
            .generation
            .content_digest(),
        tokenizer: id("fixture-bound"),
        tokenizer_version: id("1"),
        renderer: id("fixture"),
        template_version: id("1"),
    }
}
use InvocationRunDiagnostic as D;
use gateway_application::{
    closed_loop::LoopDecision, codex::RequestContext, invocation_resilience::*,
};
use gateway_domain::invocation_resilience::*;
use std::time::{Duration, Instant};

fn limits() -> InvocationRunLimits {
    InvocationRunLimits {
        schema_version: SchemaVersion::V1,
        max_attempts: 2,
        timeout_ms: 10_000,
        invocation_input_tokens: 100_000,
        invocation_output_tokens: 128,
        run_input_tokens: 200_000,
        run_output_tokens: 256,
        cost_microunits: 100,
        output_bytes: 3_000_000,
        reconciliation_tolerance: 100,
    }
}
fn context() -> RequestContext {
    RequestContext::new("run-268".into(), Instant::now() + Duration::from_secs(10))
}
fn rendered(ir: &InvocationIr, host: &dyn InvocationAdmissionPort) -> RenderedInvocationIr {
    render_invocation_ir(ir, host, profile(ir), &Renderer, &Counter).unwrap()
}
fn run(ir: &InvocationIr, l: InvocationRunLimits) -> InvocationRun {
    InvocationRun::new(ir.clone(), l, context()).unwrap()
}
#[derive(Clone, Copy)]
enum Fault {
    Malformed,
    Valid,
    Transient,
    Hard,
    Uncertain,
    Cancel,
    Expire,
    Usage(u64, u64, u64),
    WrongBinding(u8),
    Oversize,
}
struct Port {
    calls: Cell<usize>,
    fault: Fault,
}
impl Port {
    fn new(fault: Fault) -> Self {
        Self {
            calls: Cell::new(0),
            fault,
        }
    }
}
impl BoundedInvocationPort for Port {
    fn invoke(&self, r: BoundedInvocationRequest<'_>) -> InvocationAttemptResponse {
        self.calls.set(self.calls.get() + 1);
        r.context.check().unwrap();
        assert_eq!(r.binding.invocation, r.ir.identity());
        assert_eq!(
            &r.binding.profile,
            &r.rendered.rendered().accounting().count.profile
        );
        assert_eq!(r.reserved_cost_microunits, 10);
        let mut usage = None;
        let output = match self.fault {
            Fault::Transient => Err(InvocationAttemptFailure::Transient),
            Fault::Hard => Err(InvocationAttemptFailure::Hard),
            Fault::Uncertain => Err(InvocationAttemptFailure::UncertainSideEffect),
            Fault::Valid => Ok(serde_json::to_vec(&json!({"schema_version":"1.0", "invocation":r.ir.identity(), "result":"entity", "references":[], "tools":[]})).unwrap()),
            Fault::Oversize => Ok(vec![b'x'; r.max_output_bytes + 1]),
            Fault::Cancel => { r.context.cancel(); Ok(b"{".to_vec()) },
            Fault::Expire => { std::thread::sleep(r.context.deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(1)); Ok(b"{".to_vec()) },
            Fault::Usage(input, output, cost) => {
                usage = Some(ProviderUsage { binding: r.binding.clone(), input_tokens: input, output_tokens: output, cost_microunits: cost });
                Ok(b"{".to_vec())
            }
            Fault::WrongBinding(field) => {
                let mut binding = r.binding.clone();
                match field {
                    0 => binding.attempt += 1,
                    1 => binding.profile.model = pin("other-model"),
                    2 => binding.profile.tokenizer_version = id("2"),
                    3 => binding.payload_digest = context_content_digest(b"other"),
                    4 => binding.invocation.ir_digest = context_content_digest(b"other"),
                    _ => binding.schema_version = SchemaVersion::new(2,0).unwrap(),
                }
                usage = Some(ProviderUsage { binding, input_tokens: 1, output_tokens: 1, cost_microunits: 1 });
                Ok(b"{".to_vec())
            }
            Fault::Malformed => Ok(b"{".to_vec()),
        };
        InvocationAttemptResponse { output, usage }
    }
}
fn dispatch(
    run: &mut InvocationRun,
    ir: &InvocationIr,
    r: &RenderedInvocationIr,
    host: &dyn InvocationAdmissionPort,
    port: &Port,
) -> Result<InvocationAttemptResult, D> {
    run.dispatch(ir, r, LoopDecision::Continue, 10, host, port)
}

#[test]
fn malformed_repair_and_retry_share_all_budgets_and_stop() {
    let (ir, host) = fixture();
    let r = rendered(&ir, &host);
    let port = Port::new(Fault::Malformed);
    let mut session = run(&ir, limits());
    for _ in 0..2 {
        assert!(matches!(
            dispatch(&mut session, &ir, &r, &host, &port).unwrap(),
            InvocationAttemptResult::Malformed(_)
        ));
    }
    assert_eq!(
        dispatch(&mut session, &ir, &r, &host, &port).unwrap_err(),
        D::AttemptBudgetExceeded
    );
    assert_eq!(port.calls.get(), 2);
    assert_eq!(session.usage().output_tokens, 256);
    assert_eq!(session.usage().cost_microunits, 20);
    assert_eq!(session.usage().output_bytes, 2);
    assert_eq!(session.records().len(), 2);
    assert_eq!(
        session.records()[0].local_provenance,
        UsageProvenance::LocalEstimated
    );
    assert_eq!(session.records()[1].binding.attempt, 2);
    assert!(session.terminal().is_none());
}

#[test]
fn reserves_output_and_enforces_each_limit_before_dispatch() {
    let (ir, host) = fixture();
    let r = rendered(&ir, &host);
    let input = r.rendered().accounting().total_input_tokens;
    let port = Port::new(Fault::Malformed);
    let cases: Vec<(InvocationRunLimits, D)> = vec![
        (
            InvocationRunLimits {
                invocation_input_tokens: input - 1,
                ..limits()
            },
            D::InputTokenBudgetExceeded,
        ),
        (
            InvocationRunLimits {
                run_input_tokens: input - 1,
                ..limits()
            },
            D::InputTokenBudgetExceeded,
        ),
        (
            InvocationRunLimits {
                invocation_output_tokens: 127,
                ..limits()
            },
            D::OutputTokenBudgetExceeded,
        ),
        (
            InvocationRunLimits {
                run_output_tokens: 127,
                ..limits()
            },
            D::OutputTokenBudgetExceeded,
        ),
        (
            InvocationRunLimits {
                cost_microunits: 9,
                ..limits()
            },
            D::CostBudgetExceeded,
        ),
    ];
    for (l, e) in cases {
        let mut session = run(&ir, l);
        assert_eq!(
            dispatch(&mut session, &ir, &r, &host, &port).unwrap_err(),
            e
        );
        assert_eq!(session.usage(), &InvocationRunUsage::default());
    }
    assert_eq!(port.calls.get(), 0);
    for (l, e) in [
        (
            InvocationRunLimits {
                run_input_tokens: input * 2 - 1,
                ..limits()
            },
            D::InputTokenBudgetExceeded,
        ),
        (
            InvocationRunLimits {
                run_output_tokens: 255,
                ..limits()
            },
            D::OutputTokenBudgetExceeded,
        ),
        (
            InvocationRunLimits {
                cost_microunits: 19,
                ..limits()
            },
            D::CostBudgetExceeded,
        ),
        (
            InvocationRunLimits {
                output_bytes: 1,
                ..limits()
            },
            D::OutputBudgetExceeded,
        ),
        (
            InvocationRunLimits {
                max_attempts: 1,
                ..limits()
            },
            D::AttemptBudgetExceeded,
        ),
    ] {
        let mut session = run(&ir, l);
        dispatch(&mut session, &ir, &r, &host, &port).unwrap();
        assert_eq!(
            dispatch(&mut session, &ir, &r, &host, &port).unwrap_err(),
            e
        );
        assert_eq!(session.usage().attempts, 1);
    }
}

#[test]
fn cancellation_deadline_and_cg14_decisions_block_subsequent_dispatch() {
    let (ir, host) = fixture();
    let r = rendered(&ir, &host);
    let port = Port::new(Fault::Malformed);
    let token = context();
    let mut session = InvocationRun::new(ir.clone(), limits(), token.clone()).unwrap();
    token.cancel();
    assert_eq!(
        dispatch(&mut session, &ir, &r, &host, &port).unwrap_err(),
        D::Cancelled
    );
    let expired = RequestContext::new("expired".into(), Instant::now());
    let mut session = InvocationRun::new(ir.clone(), limits(), expired).unwrap();
    assert_eq!(
        dispatch(&mut session, &ir, &r, &host, &port).unwrap_err(),
        D::DeadlineExceeded
    );
    let mut session = run(&ir, limits());
    for decision in [
        LoopDecision::Pause,
        LoopDecision::Stopped,
        LoopDecision::Success,
    ] {
        assert_eq!(
            session
                .dispatch(&ir, &r, decision, 10, &host, &port)
                .unwrap_err(),
            D::LifecycleBlocked
        );
    }
    assert_eq!(port.calls.get(), 0);
    session
        .dispatch(&ir, &r, LoopDecision::Replan, 10, &host, &port)
        .unwrap();
    for (fault, error) in [
        (Fault::Cancel, D::Cancelled),
        (Fault::Hard, D::HardFailure),
        (Fault::Uncertain, D::UncertainSideEffect),
    ] {
        let port = Port::new(fault);
        let mut session = run(&ir, limits());
        assert_eq!(
            dispatch(&mut session, &ir, &r, &host, &port).unwrap_err(),
            error
        );
        assert_eq!(session.terminal(), Some(error));
        assert_eq!(
            dispatch(&mut session, &ir, &r, &host, &port).unwrap_err(),
            error
        );
        assert_eq!(port.calls.get(), 1);
        assert_eq!(session.usage().attempts, 1);
    }
}

#[test]
fn provider_reconciliation_keeps_original_values_and_exact_attempt_profile() {
    let (ir, host) = fixture();
    let r = rendered(&ir, &host);
    let input = r.rendered().accounting().total_input_tokens;
    let port = Port::new(Fault::Usage(input + 101, 12, 3));
    let mut session = run(&ir, limits());
    dispatch(&mut session, &ir, &r, &host, &port).unwrap();
    let record = &session.records()[0];
    assert_eq!(record.local_input_tokens, input);
    assert_eq!(record.reported.as_ref().unwrap().input_tokens, input + 101);
    assert_eq!(
        record.reported_provenance,
        Some(UsageProvenance::ProviderReported)
    );
    assert_eq!(record.diagnostics, vec![D::UsageReconciliationMismatch]);
    assert_eq!(session.usage().input_tokens, input + 101);
    assert_eq!(session.usage().output_tokens, 12);
    assert_eq!(session.usage().cost_microunits, 3);
    // Under-reported input never refunds local counts.
    dispatch(
        &mut session,
        &ir,
        &r,
        &host,
        &Port::new(Fault::Usage(1, 1, 1)),
    )
    .unwrap();
    assert_eq!(session.usage().input_tokens, input * 2 + 101);
    for field in 0..6 {
        let port = Port::new(Fault::WrongBinding(field));
        let mut session = run(&ir, limits());
        assert_eq!(
            dispatch(&mut session, &ir, &r, &host, &port).unwrap_err(),
            D::InvalidBinding
        );
        assert!(session.records()[0].reported.is_some());
        assert_eq!(session.usage().output_tokens, 128);
        assert_eq!(session.usage().cost_microunits, 10);
    }
}

#[test]
fn overreported_usage_and_oversize_output_stop_run_after_charging() {
    let (ir, host) = fixture();
    let r = rendered(&ir, &host);
    let input = r.rendered().accounting().total_input_tokens;
    for (fault, error) in [
        (Fault::Usage(100_001, 1, 1), D::InputTokenBudgetExceeded),
        (Fault::Usage(input, 129, 1), D::OutputTokenBudgetExceeded),
        (Fault::Usage(input, 1, 11), D::CostBudgetExceeded),
        (Fault::Oversize, D::OutputBudgetExceeded),
    ] {
        let port = Port::new(fault);
        let mut session = run(&ir, limits());
        assert_eq!(
            dispatch(&mut session, &ir, &r, &host, &port).unwrap_err(),
            error
        );
        assert_eq!(
            dispatch(&mut session, &ir, &r, &host, &port).unwrap_err(),
            error
        );
        assert_eq!(port.calls.get(), 1);
        assert!(session.records()[0].diagnostics.contains(&error));
    }
    let mut session = run(&ir, limits());
    let port = Port::new(Fault::Transient);
    dispatch(&mut session, &ir, &r, &host, &port).unwrap();
    assert_eq!(session.usage().output_tokens, 128);
    assert_eq!(session.usage().output_bytes, 1_048_576);
    dispatch(&mut session, &ir, &r, &host, &port).unwrap();
    assert_eq!(session.usage().output_tokens, 256);
}

#[test]
fn late_output_is_charged_and_never_accepted_after_deadline() {
    let (ir, host) = fixture();
    let r = rendered(&ir, &host);
    let mut l = limits();
    l.timeout_ms = 1000;
    let mut session = run(&ir, l);
    let port = Port::new(Fault::Expire);
    assert_eq!(
        dispatch(&mut session, &ir, &r, &host, &port).unwrap_err(),
        D::DeadlineExceeded
    );
    assert_eq!(session.usage().output_bytes, 1);
    assert_eq!(session.usage().output_tokens, 128);
    assert_eq!(
        dispatch(&mut session, &ir, &r, &host, &port).unwrap_err(),
        D::DeadlineExceeded
    );
    assert_eq!(port.calls.get(), 1);
}

#[test]
fn valid_output_is_only_parsed_and_current_admission_is_mandatory() {
    let (ir, host) = fixture();
    let r = rendered(&ir, &host);
    let mut session = run(&ir, limits());
    let port = Port::new(Fault::Valid);
    assert!(matches!(
        dispatch(&mut session, &ir, &r, &host, &port).unwrap(),
        InvocationAttemptResult::Parsed(_)
    ));
    host.error.set(Some(InvocationIrError::AuthorityDenied));
    assert_eq!(
        dispatch(&mut session, &ir, &r, &host, &port).unwrap_err(),
        D::AuthorityMismatch
    );
    assert_eq!(session.usage().attempts, 1);
}

#[test]
fn limits_wire_version_unknown_fields_and_zero_bounds_fail() {
    assert_eq!(limits().validate(), Ok(()));
    let mut value = json!(limits());
    value["schema_version"] = json!("2.0");
    assert_eq!(
        serde_json::from_value::<InvocationRunLimits>(value.clone())
            .unwrap()
            .validate(),
        Err(D::IncompatibleVersion)
    );
    value = json!(limits());
    value["vendor"] = json!(true);
    assert!(serde_json::from_value::<InvocationRunLimits>(value).is_err());
    for field in [
        "max_attempts",
        "timeout_ms",
        "invocation_input_tokens",
        "invocation_output_tokens",
        "run_input_tokens",
        "run_output_tokens",
        "output_bytes",
    ] {
        let mut value = json!(limits());
        value[field] = json!(0);
        let l = serde_json::from_value::<InvocationRunLimits>(value).unwrap();
        assert_eq!(l.validate(), Err(D::InvalidLimits));
        assert!(InvocationRun::new(fixture().0, l, context()).is_err());
    }
}

fn alternate(
    ir: &InvocationIr,
    change: impl FnOnce(&mut gateway_domain::model_invocation::ModelInvocationData),
) -> InvocationIr {
    alternate_at_locality(ir, change, None)
}

fn alternate_at_locality(
    ir: &InvocationIr,
    change: impl FnOnce(&mut gateway_domain::model_invocation::ModelInvocationData),
    locality: Option<gateway_domain::cognitive_routing::PrivacyBoundary>,
) -> InvocationIr {
    use gateway_domain::model_invocation::*;
    let h = ir.prompt().handoff();
    let mut data = ir.prepared().invocation().data().clone();
    let mut cap = ir.prepared().capability().data().clone();
    let mut provider = ir.prepared().provider().data().clone();
    cap.identity = pin("fallback-capability");
    cap.model = pin("fallback-model");
    cap.provider = pin("fallback-provider");
    provider.identity = cap.provider.clone();
    if let Some(locality) = locality {
        provider.locality = locality;
    }
    data.capability = cap.identity.clone();
    change(&mut data);
    let invocation = ModelInvocation::new(
        data.clone(),
        ModelInvocationBasis {
            task: h.task(),
            output: &data.output,
            compiled_context: &data.compiled_context,
            permitted_capabilities: &data.permitted_capabilities,
            sensitivity: data.sensitivity,
            permitted_localities: &data.permitted_localities,
            authority: &data.authority,
        },
    )
    .unwrap();
    let mut runtime = model::runtime();
    runtime.provider = provider.identity.clone();
    let prepared = gateway_application::model_invocation::prepare_model_invocation(
        invocation,
        ModelCapability::try_from(cap).unwrap(),
        ModelProvider::try_from(provider).unwrap(),
        &runtime,
    )
    .unwrap();
    admit_invocation_ir(
        SchemaVersion::V1,
        h.clone(),
        prepared,
        ir.prompt().tools().clone(),
        &Capture,
    )
    .unwrap()
}

#[test]
fn fallback_changes_provider_model_and_tokenizer_without_resetting_usage_or_authority() {
    let (ir, host) = fixture();
    let r = rendered(&ir, &host);
    let fallback = alternate(&ir, |_| {});
    let mut fp = profile(&fallback);
    fp.tokenizer = id("other-tokenizer");
    let fr = render_invocation_ir(&fallback, &Capture, fp, &Renderer, &Counter).unwrap();
    let input = r.rendered().accounting().total_input_tokens;
    let fallback_input = fr.rendered().accounting().total_input_tokens;
    for (l, error) in [
        (
            InvocationRunLimits {
                run_input_tokens: input + fallback_input - 1,
                ..limits()
            },
            D::InputTokenBudgetExceeded,
        ),
        (
            InvocationRunLimits {
                run_output_tokens: 255,
                ..limits()
            },
            D::OutputTokenBudgetExceeded,
        ),
        (
            InvocationRunLimits {
                cost_microunits: 19,
                ..limits()
            },
            D::CostBudgetExceeded,
        ),
    ] {
        let mut session = run(&ir, l);
        let port = Port::new(Fault::Malformed);
        dispatch(&mut session, &ir, &r, &host, &port).unwrap();
        assert_eq!(
            dispatch(&mut session, &fallback, &fr, &Capture, &port).unwrap_err(),
            error
        );
        assert_eq!(port.calls.get(), 1);
    }
    let mut session = run(&ir, limits());
    let port = Port::new(Fault::Malformed);
    dispatch(&mut session, &ir, &r, &host, &port).unwrap();
    dispatch(&mut session, &fallback, &fr, &Capture, &port).unwrap();
    assert_eq!(session.usage().input_tokens, input + fallback_input);
    assert_eq!(
        session.records()[1].binding.profile.tokenizer,
        id("other-tokenizer")
    );
    assert_ne!(
        session.records()[0].binding.profile.model,
        session.records()[1].binding.profile.model
    );
    let changed = alternate(&ir, |data| data.authority = pin("different-consent"));
    let cr = rendered(&changed, &Capture);
    let mut session = run(&ir, limits());
    assert_eq!(
        dispatch(&mut session, &changed, &cr, &Capture, &port).unwrap_err(),
        D::AuthorityMismatch
    );
    let changed = alternate(&ir, |data| data.max_attempts = 3);
    let cr = rendered(&changed, &Capture);
    assert_eq!(
        dispatch(&mut session, &changed, &cr, &Capture, &port).unwrap_err(),
        D::AuthorityMismatch
    );
    assert_eq!(
        dispatch(&mut session, &fallback, &r, &Capture, &port).unwrap_err(),
        D::InvalidBinding
    );
}

#[test]
fn cancellation_from_current_admission_prevents_transport_and_accounting() {
    let (ir, host) = fixture();
    let r = rendered(&ir, &host);
    let token = context();
    struct CancelHost(RequestContext);
    impl SemanticReferenceValidator for CancelHost {
        fn validate(
            &self,
            _: &SemanticTaskData,
            _: &SemanticReferenceBinding,
        ) -> Result<(), ValidationError> {
            Ok(())
        }
    }
    impl InvocationAdmissionPort for CancelHost {
        fn validate_current(&self, _: &InvocationIr) -> Result<(), InvocationIrError> {
            self.0.cancel();
            Ok(())
        }
    }
    let mut session = InvocationRun::new(ir.clone(), limits(), token.clone()).unwrap();
    let port = Port::new(Fault::Malformed);
    assert_eq!(
        dispatch(&mut session, &ir, &r, &CancelHost(token), &port).unwrap_err(),
        D::Cancelled
    );
    assert_eq!(port.calls.get(), 0);
    assert_eq!(session.usage().attempts, 0);
}

#[test]
fn exact_count_context_output_reserve_and_reported_context_overflow() {
    struct ExactCounter(u64);
    impl TokenizerPort for ExactCounter {
        fn count(
            &self,
            request: &TokenCountRequest<'_>,
        ) -> Result<TokenCountResult, RenderDiagnostic> {
            let mut count = Counter.count(request)?;
            count.mode = CountMode::Exact;
            count.usage.framing_tokens = self.0;
            Ok(count)
        }
    }
    let (ir, host) = fixture();
    // Synthetic counter tests the reserve boundary; no production tokenizer claim.
    let r =
        render_invocation_ir(&ir, &host, profile(&ir), &Renderer, &ExactCounter(99_872)).unwrap();
    let mut session = run(&ir, limits());
    let port = Port::new(Fault::Malformed);
    dispatch(&mut session, &ir, &r, &host, &port).unwrap();
    assert_eq!(
        session.records()[0].local_provenance,
        UsageProvenance::LocalExact
    );
    assert!(matches!(
        render_invocation_ir(&ir, &host, profile(&ir), &Renderer, &ExactCounter(99_873)),
        Err(InvocationPreparationError::Rendering(
            RenderError::ContextLimitExceeded(_)
        ))
    ));
    let r = rendered(&ir, &host);
    let mut session = run(&ir, limits());
    let port = Port::new(Fault::Usage(100_000, 1, 1));
    assert_eq!(
        dispatch(&mut session, &ir, &r, &host, &port).unwrap_err(),
        D::ContextWindowExceeded
    );
}

#[test]
fn fallback_cannot_change_locality_even_when_both_locations_are_permitted() {
    use gateway_domain::cognitive_routing::PrivacyBoundary;
    let (ir, _) = fixture();
    let original = alternate(&ir, |d| {
        d.permitted_localities.insert(PrivacyBoundary::External);
    });
    let changed = alternate_at_locality(&original, |_| {}, Some(PrivacyBoundary::External));
    let r = rendered(&changed, &Capture);
    let mut session = run(&original, limits());
    let port = Port::new(Fault::Malformed);
    assert_eq!(
        dispatch(&mut session, &changed, &r, &Capture, &port).unwrap_err(),
        D::AuthorityMismatch
    );
    assert_eq!(port.calls.get(), 0);
}

#[test]
fn frozen_diagnostic_and_provenance_wire_contract() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/invocation-resilience-v1/diagnostics.json"
    ))
    .unwrap();
    assert_eq!(fixture["schema_version"], json!(SchemaVersion::V1));
    assert_eq!(
        fixture["diagnostics"],
        json!([
            D::IncompatibleVersion,
            D::InvalidLimits,
            D::InvalidBinding,
            D::AuthorityMismatch,
            D::LifecycleBlocked,
            D::Cancelled,
            D::DeadlineExceeded,
            D::AttemptBudgetExceeded,
            D::TokenCountUnavailable,
            D::InputTokenBudgetExceeded,
            D::OutputTokenBudgetExceeded,
            D::ContextWindowExceeded,
            D::CostBudgetExceeded,
            D::OutputBudgetExceeded,
            D::UsageReconciliationMismatch,
            D::UncertainSideEffect,
            D::HardFailure,
        ])
    );
    assert_eq!(
        fixture["usage_provenance"],
        json!([
            UsageProvenance::LocalExact,
            UsageProvenance::LocalEstimated,
            UsageProvenance::ProviderReported
        ])
    );
}
