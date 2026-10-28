# Optional interpretation adapters — EPIC-05.11

`CompletenessValidator::assist` runs the #187/#188 deterministic path first.
Complete requests never invoke models, even with invalid model configuration.
For incomplete requests, configured classifier, embedding and SLM ports are
attempted in that fixed order, at most once each. Missing ports are skipped.
Unavailable, malformed, below-threshold and rejected proposals escalate to the
next port. Exhaustion retains the exact original completeness/clarification
result; no facts are synthesized and no question is suppressed.

The application `InterpretationAdapter` contract is shared by three independently
optional slots. `LocalSemanticAdapter` delegates to the existing #249
`LocalInferencePort`; a host can wrap `HttpLocalInferenceAdapter` with it. Model
installation, qualification, runtime endpoints and promotion remain CG-27 owned.
No new provider configuration or dependencies enter the domain crate.

The request contains versioned typed gaps and deterministic decisions, including
request, input, scope and semantic-revision correlation. It does not replay chat
history. The response contract `semantic-proposal/1.0` contains a full task,
signal source, ranking score and optional embedding-space identity. Its schema
embeds the existing SemanticTaskIR schema. Adapters must return CG-27 model,
artifact and invocation provenance. The fixture model is a test double, not a
qualified production model profile.

## Validation and authority

Every eligible response passes `SemanticTaskIR::from_json`, exact reference
validation, host meaning validation and independent host permission validation.
Task identity and scope must match the captured request. Hosts implement
`SemanticProposalHost` alongside their existing completeness and reference ports;
they must check the entire proposed meaning against trusted request evidence,
including task type, target kind, output and verification requirements, existing
resolved meanings and proposed permissions. A model cannot implement the trusted
host. Captured basis is rechecked after calls, including failed calls.

The result retains the original gaps even when a proposal passes these checks.
The returned envelope is inspection-only proposal data, never execution admission.
Consumers must obtain trusted facts or clarification and re-enter normal
completeness/compilation before planning. Consent and execution remain CG-09 and
host owned. No model similarity, classification or JSON validity creates facts.

## Ranking and embedding provenance

`InterpretationPolicy` is inspectable and serialized with the result. Version
`1.0` fixes escalation order and fallback semantics. Each threshold identifies
signal source, exact model ID/version/artifact, embedding space when applicable,
and a named fixture dataset. Duplicate threshold identities are rejected. Scores
are finite ranks in [0, 1]; equality with the threshold passes ranking. They are
explicitly uncalibrated: neither logits, softmax nor self-reported scores are
represented as factual probabilities. No calibrated-confidence claim is made.

Embedding proposals must carry a nonempty space ID. Different model versions,
artifacts or spaces never share a threshold implicitly. Vector computation stays
inside the embedding adapter; the application never compares cross-space vectors.
Retained accepted envelopes preserve model/version/artifact/space provenance for
any downstream evidence record. Persistence must store that envelope with its
captured deterministic basis, not strip it to a canonical entity ID.

## Requirement review and evidence

Review performed as requirement, architecture, automation and testing role passes
by one agent. Base revision: `44b84e940425472057f25e61d24201178eeaa027` plus this
working-tree change. Decision: READY_FOR_WORKFLOW for the application adapter
slice. #187/#188 implementations and #249's Rust port are present; consumers
follow deterministic admission, proposal generation, validation, then ordinary
clarification/compilation. No dependency cycle or new model lifecycle is needed.
Estimated implementation 1–2 engineer-days, qualification 0.5–1 day, no missing
foundation work for this component scope; medium confidence. Rollback removes
the optional composition call and retains deterministic resolution.

| ID | Requirement/source | Production owner/path | Dependencies | Evidence level | Status |
| --- | --- | --- | --- | --- | --- |
| A1 | #189 no-model resolution | completeness → assist | #187/#188 present | COMPONENT: complete path makes zero calls | VERIFIED |
| A2 | proposals only; typed validation | SemanticTaskIR + host validation | existing typed IR and reference checks | COMPONENT: malformed/invented/foreign tasks refused | VERIFIED |
| A3 | independent entities/permissions | reference and permission ports | trusted host implementation required | COMPONENT: separate reference and policy denial cases | VERIFIED |
| A4 | explicit versioned thresholds | InterpretationPolicy | named fixture policy/dataset | COMPONENT: boundary, invalid/duplicate policy, escalation | VERIFIED |
| A5 | deterministic fallback | original CompletenessResult retained | clarification plans from #188 | COMPONENT: unavailable/invalid/low/denied outputs | VERIFIED |
| A6 | provider independence; #249 reuse | LocalSemanticAdapter → LocalInferencePort | existing HTTP adapter/service | COMPONENT: real wrapper over test-double inference port | VERIFIED |
| A7 | full semantic conformance | existing IR parser and reference validator | semantic-task-v1 fixtures | COMPONENT: valid full task and adversarial mutations | VERIFIED |
| A8 | embedding provenance; no similarity authority | model/version/artifact/space match | exact threshold configuration | COMPONENT: changed model/version/space rejected | VERIFIED |
| A9 | stale basis refusal | InterpretationContext::revalidate | existing admitted capture | COMPONENT: basis changes during success and failure | VERIFIED |

The fixture suite is `tests/fixtures/interpretation-adapters-v1`, exercised by
`crates/gateway-application/tests/support/interpretation_adapters.rs` from the
completeness test target. Run:

```text
cargo test -p gateway-application --test completeness --test interpretation --locked
cargo llvm-cov -p gateway-application --test completeness --locked --json --output-path target/interpretation-adapters-coverage.json
python3 tests/scripts/check-interpretation-adapters-coverage.py --self-test
python3 tests/scripts/check-interpretation-adapters-coverage.py target/interpretation-adapters-coverage.json
```

The per-module line coverage gate requires 95% and is registered in the shared
quality gate runner. This is component conformance evidence, not qualification
of a live model, an installed client or full natural-language runtime (#279).
The existing MCP launcher is not extended by this slice. No issue or parent
acceptance is automatically closed by these component tests.

### Local verification — 2026-10-16

On the base revision plus this change, Windows/Rust 1.94:

- Focused completeness/adapter and interpretation suites: 33 tests passed.
- CG-27 HTTP local inference regression: 2 tests passed.
- Adapter coverage: 172/172 lines (100%); coverage checker self-test passed.
- Workspace build, format check, dependency and architecture guards passed.
- Contract suite: 17 tests passed using an isolated environment with the pinned
  `tests/contracts/requirements.txt` dependencies.
- Full workspace tests did not pass: the unchanged MCP
  `active_cancel_disconnect_errors_and_deadline_are_bounded` test panics at
  `local_mcp/runtime.rs:545`; an isolated rerun reproduced it.
- Clippy is blocked by `worker_fabric.rs:291` (`nonminimal_bool`) in unchanged
  code. The workspace build also reports an existing Windows `unused_mut`
  warning in `bounded_process.rs:57`.
- Architecture regression suite: 36 tests ran with five errors in existing
  EPIC-03 benchmark, EPIC-04 source-fixture reconciliation and installed-client
  report paths. These are not passing evidence for repository-wide acceptance.

Raw local coverage and workspace output are under `target/` (not committed).
The CI gate retains its coverage JSON in `EVIDENCE_DIR`. Full repository release
acceptance remains blocked by the failures above; live model quality is untested.
