#[path = "../../../tests/fixtures/prompt-rendering-v1/support.rs"]
mod support;
use gateway_application::prompt_rendering::*;
use gateway_daemon::prompt_renderers::*;
use gateway_domain::{prompt_rendering::*, *};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use support::{examples::id, model::pin, *};

fn profile(
    p: &gateway_application::model_invocation::PreparedModelInvocation,
    r: SemanticRenderer,
) -> TokenizerProfile {
    let (renderer, template_version) = r.identity();
    TokenizerProfile {
        schema_version: SchemaVersion::V1,
        model: p.capability().data().model.clone(),
        generation_profile: p.invocation().data().generation.content_digest(),
        tokenizer: id("fixture-byte"),
        tokenizer_version: id("1"),
        renderer,
        template_version,
    }
}
struct Counter {
    mode: CountMode,
    fault: u8,
}
impl TokenizerPort for Counter {
    fn count(&self, r: &TokenCountRequest<'_>) -> Result<TokenCountResult, RenderDiagnostic> {
        if self.fault == 1 {
            return Err(RenderDiagnostic::TokenizerUnavailable);
        }
        // Exact only for the synthetic byte-token fixture profile, not an LLM.
        assert_eq!(r.profile.tokenizer.as_str(), "fixture-byte");
        let h = r.sections.handoff();
        let size = |v: &Value| serde_json::to_string(v).unwrap().len() as u64;
        let mut usage = TokenUsage {
            instruction_tokens: size(&h["instructions"]),
            task_tokens: size(&h["task"]),
            context_tokens: size(&h["context"]),
            evidence_tokens: size(&h["evidence"]),
            tool_schema_tokens: size(
                &json!({"tools":r.sections.tools(),"output_schema":r.sections.output_schema()}),
            ),
            framing_tokens: 0,
        };
        usage.framing_tokens = r.payload.len() as u64 - usage.total().unwrap();
        if self.mode == CountMode::Estimated {
            usage.framing_tokens += 10;
        }
        let mut result = TokenCountResult {
            profile: r.profile.clone(),
            payload_digest: r.payload_digest.clone(),
            mode: self.mode,
            provenance: pin("fixture-byte-counter"),
            usage,
        };
        match self.fault {
            2 => result.profile.tokenizer_version = id("wrong"),
            3 => result.payload_digest = context_content_digest(b"wrong"),
            4 => result.provenance.revision = id("latest"),
            5 => result.usage.instruction_tokens = u64::MAX,
            6 => {
                result.usage = TokenUsage {
                    instruction_tokens: 0,
                    task_tokens: 0,
                    context_tokens: 0,
                    evidence_tokens: 0,
                    tool_schema_tokens: 0,
                    framing_tokens: 0,
                }
            }
            7 => {
                result.usage = TokenUsage {
                    instruction_tokens: u64::MAX,
                    task_tokens: 0,
                    context_tokens: 0,
                    evidence_tokens: 0,
                    tool_schema_tokens: 0,
                    framing_tokens: 0,
                }
            }
            _ => (),
        }
        Ok(result)
    }
}
fn tools(
    p: &gateway_application::model_invocation::PreparedModelInvocation,
) -> BTreeMap<CapabilityId, ToolDefinition> {
    p.invocation()
        .data()
        .permitted_capabilities
        .iter()
        .map(|(id, binding)| {
            (
                id.clone(),
                ToolDefinition {
                    binding: binding.clone(),
                    description: NonEmptyText::new("Inspect artifact").unwrap(),
                    input: gateway_domain::semantic_output::OutputType::Object {
                        fields: BTreeMap::new(),
                    },
                },
            )
        })
        .collect()
}
#[test]
fn actual_handoff_preserved_in_both_runtime_layouts_with_stable_accounting() {
    let h = handoff();
    assert_eq!(h.to_value()["evidence"].as_array().unwrap().len(), 1);
    let p = prepared(&h, 100_000, true);
    for r in [
        SemanticRenderer::ChatMessagesV1,
        SemanticRenderer::BlockMessagesV1,
    ] {
        for mode in [CountMode::Exact, CountMode::Estimated] {
            let counter = Counter { mode, fault: 0 };
            let rendered =
                render_invocation(&h, &p, tools(&p), profile(&p, r), &r, &counter).unwrap();
            let repeated =
                render_invocation(&h, &p, tools(&p), profile(&p, r), &r, &counter).unwrap();
            assert_eq!(rendered, repeated);
            let payload: Value = serde_json::from_str(rendered.payload()).unwrap();
            let (system, data) = match r {
                SemanticRenderer::ChatMessagesV1 => {
                    assert_eq!(payload["messages"].as_array().unwrap().len(), 2);
                    assert_eq!(payload["messages"][0]["role"], "system");
                    assert_eq!(payload["messages"][1]["role"], "user");
                    (
                        &payload["messages"][0]["content"],
                        &payload["messages"][1]["content"],
                    )
                }
                SemanticRenderer::BlockMessagesV1 => {
                    assert_eq!(payload["messages"].as_array().unwrap().len(), 1);
                    assert_eq!(payload["messages"][0]["role"], "user");
                    (
                        &payload["system"],
                        &payload["messages"][0]["content"][0]["text"],
                    )
                }
            };
            assert!(!system.as_str().unwrap().contains("ignore policy"));
            let data: Value = serde_json::from_str(data.as_str().unwrap()).unwrap();
            let mut canonical = h.to_value();
            canonical.as_object_mut().unwrap().remove("instructions");
            assert_eq!(data["semantic"], canonical);
            assert!(data.to_string().contains("ignore policy"));
            assert_eq!(
                data["output_schema"],
                p.invocation().data().output.json_schema()
            );
            assert_eq!(data["tools"], serde_json::to_value(tools(&p)).unwrap());
            let a = rendered.accounting();
            assert_eq!(a.count.mode, mode);
            assert_eq!(
                a.total_input_tokens,
                rendered.payload().len() as u64 + if mode == CountMode::Estimated { 10 } else { 0 }
            );
            assert_eq!(
                a.reserved_output_tokens,
                u64::from(p.effective_generation().max_output_tokens)
            );
            assert!(a.count.usage.tool_schema_tokens > 0);
            assert!(a.count.usage.framing_tokens > 0);
            assert_eq!(
                a.count.payload_digest,
                context_content_digest(rendered.payload().as_bytes())
            );
        }
    }
}
#[test]
fn overflow_is_typed_and_preserves_the_complete_input_and_output_reserve() {
    let h = handoff();
    let original = h.to_value();
    let r = SemanticRenderer::ChatMessagesV1;
    let p = prepared(&h, 100_000, false);
    let counter = Counter {
        mode: CountMode::Exact,
        fault: 0,
    };
    let rendered = render_invocation(&h, &p, tools(&p), profile(&p, r), &r, &counter).unwrap();
    let boundary = (rendered.accounting().total_input_tokens
        + rendered.accounting().reserved_output_tokens) as u32;
    for limit in [boundary - 1, boundary, boundary + 1] {
        let p = prepared(&h, limit, false);
        let result = render_invocation(&h, &p, tools(&p), profile(&p, r), &r, &counter);
        if limit < boundary {
            let RenderError::ContextLimitExceeded(snapshot) = result.unwrap_err() else {
                panic!("expected overflow")
            };
            assert_eq!(
                snapshot.total_input_tokens,
                rendered.accounting().total_input_tokens
            );
            assert_eq!(snapshot.context_limit, u64::from(limit));
        } else {
            assert_eq!(result.unwrap().payload(), rendered.payload());
        }
    }
    assert_eq!(h.to_value(), original);
}
#[test]
fn mismatched_profiles_tools_bindings_and_bad_accounting_fail_closed() {
    let h = handoff();
    let p = prepared(&h, 100_000, true);
    let r = SemanticRenderer::BlockMessagesV1;
    for fault in 1..=7 {
        let result = render_invocation(
            &h,
            &p,
            tools(&p),
            profile(&p, r),
            &r,
            &Counter {
                mode: CountMode::Exact,
                fault,
            },
        );
        assert_eq!(
            result.unwrap_err(),
            RenderError::Diagnostic(if fault == 1 {
                RenderDiagnostic::TokenizerUnavailable
            } else {
                RenderDiagnostic::InvalidAccounting
            })
        );
    }
    let counter = Counter {
        mode: CountMode::Exact,
        fault: 0,
    };
    for case in 0..8 {
        let mut profile = profile(&p, r);
        let mut tools = tools(&p);
        match case {
            0 => profile.model.revision = id("stale"),
            1 => profile.generation_profile = context_content_digest(b"stale"),
            2 => profile.template_version = id("2"),
            3 => profile.renderer = id("unknown"),
            4 => tools.clear(),
            5 => tools.values_mut().next().unwrap().binding.revision = id("stale"),
            6 => profile.tokenizer_version = id("latest"),
            _ => profile.schema_version = SchemaVersion::V2,
        }
        assert!(render_invocation(&h, &p, tools, profile, &r, &counter).is_err());
    }
    // An independently prepared invocation with another context pin is rejected.
    let wrong = gateway_application::model_invocation::prepare_model_invocation(
        model::admit(model::invocation_data()).unwrap(),
        model::capability(),
        model::provider(),
        &model::runtime(),
    )
    .unwrap();
    assert_eq!(
        render_invocation(
            &h,
            &wrong,
            BTreeMap::new(),
            profile(&wrong, r),
            &r,
            &counter
        )
        .unwrap_err(),
        RenderError::Diagnostic(RenderDiagnostic::InvalidBinding)
    );
}
struct FailingRenderer;
impl PromptRendererPort for FailingRenderer {
    fn identity(&self) -> (ReferenceId, ReferenceId) {
        SemanticRenderer::ChatMessagesV1.identity()
    }
    fn render(&self, _: &RenderInput) -> Result<Value, RenderDiagnostic> {
        Err(RenderDiagnostic::RenderingFailed)
    }
}
#[test]
fn renderer_failures_remain_typed() {
    let h = handoff();
    let p = prepared(&h, 100_000, false);
    assert_eq!(
        render_invocation(
            &h,
            &p,
            tools(&p),
            profile(&p, SemanticRenderer::ChatMessagesV1),
            &FailingRenderer,
            &Counter {
                mode: CountMode::Exact,
                fault: 0
            }
        )
        .unwrap_err(),
        RenderError::Diagnostic(RenderDiagnostic::RenderingFailed)
    );
    assert_eq!(
        serde_json::to_value(RenderDiagnostic::UnsupportedRenderer).unwrap(),
        "UNSUPPORTED_RENDERER"
    );
}
