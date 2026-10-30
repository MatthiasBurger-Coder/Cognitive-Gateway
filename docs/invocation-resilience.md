# Bounded invocation retry, cancellation and degradation — EPIC-06.08 (#268)

Historical evidence and removed review documents are linked to their archived Git
revision. They are not current acceptance evidence. Generate fresh reports in an
output directory or retain them as CI artifacts.

`gateway-application::invocation_resilience::InvocationRun` owns one in-memory
ledger across the initial invocation, malformed-output repair retries, transient
failures and pre-admitted provider/model fallback. The domain contracts are in
`gateway-domain::invocation_resilience`, version `1.0`, with unknown wire fields
rejected. A run is neither cloneable nor deserializable. Only its trusted CG-14
host constructs it, once for the original invocation. Creating a new controller
per retry would violate the integration contract.

## Admission and ownership

The host retains the run alongside its CG-14 state. It selects alternatives using
#262/CG-26, admits each through #263 and renders/counts it through #264. It then
calls `dispatch` with the existing `LoopDecision`, current admission port and a
trusted cost upper bound for that exact route. `Continue` and `Replan` may dispatch;
`Pause`, `Success` and `Stopped` may not. This service does not choose routes,
change Process/Policy decisions, retrieve/compact context or execute tools.

Fallback can change the capability/provider/model/tokenizer pins. Like #262's
frozen fallback basis, it must preserve the complete original invocation except
its capability pin, including task/scope, context, output schema, generation,
tools, sensitivity, provenance, authority/consent and original limits. Complete
handoff digests and tools must also match. Provider locality must stay identical;
a cross-locality change requires a new host-authorized planning decision outside
this fallback chain. Current semantic references and Process/Policy/disclosure
admission are authenticated again before every call. A stale or denied admission
consumes no attempt.

For a malformed result, the host can request another generation of the same
admitted schema-bound payload using the same run and CG-14 decision. That repair
retry pays the same token, cost, time, output and attempt charges. No raw error
text becomes instructions and no repair relaxes the output schema. Existing #265
`OutputRepairSession` proposals remain non-executable; their budget port alone
cannot authorize dispatch or replace this ledger. Richer repair prompts require
fresh IR admission and are outside the frozen-context retry interface.

## Reservations and reconciliation

Each call reserves full rendered input, effective generation output capacity and
the route's trusted maximum cost before transport. Per-invocation input/output
ceilings and cumulative input/output/cost/attempt ceilings all apply. The attempt
limit is the smaller of the run limit and original invocation limit. Output
reserve participates in context-window admission. Rendering failures, including
unavailable token counting, cannot produce a dispatchable input. Overflow denies
admission rather than wrapping a counter.

The budget currency is one host-selected currency expressed in integer
micro-units; all provider quotes and reports must use it. A zero cost budget
permits only a host-authenticated free route. The service does not infer prices
or trust a price proposed by model output.

Adapters receive the complete admitted IR/rendered payload, exact attempt binding,
shared cancellation/deadline context, output byte ceiling and cost reservation.
The effective output-token ceiling is in the prepared generation record. A call
returns bytes or a typed transient/hard/uncertain failure, and optional usage.
Every invocation attempt, including a failed repair, has a distinct sequence
number. Provider usage must exactly match this sequence, full invocation identity,
model/revision, generation/tokenizer/template profile and payload digest. A stale
or mismatched report is retained for diagnosis, leaves reservations charged and
terminates the run as `INVALID_BINDING`.

Records preserve the original local count (`local_exact` or `local_estimated`),
counting provenance in the rendered accounting, output/cost reservations and any
`provider_reported` input/output/cost values separately. Charged input is at least
the local count, increased for higher provider reports; lower reports cannot
refund it. Correlated provider reports replace output/cost reserves with actual
consumption. Missing usage retains the full reserves. An input discrepancy above
the configured absolute tolerance records `USAGE_RECONCILIATION_MISMATCH` without
silently replacing the local count. Reported overruns are charged and terminate
the run. A valid report cannot erase previous attempts' usage.

Raw output bytes count across all attempts, including malformed outputs. An
adapter must enforce the supplied remaining byte ceiling while reading, bounded
also by #265's one-MiB parser ceiling. A transient failure without returned output
conservatively consumes its byte allowance. An adapter exceeding a ceiling
produces a terminal diagnostic; the service never accepts the overrun as success.

## Cancellation, deadlines and recovery

The original `RequestContext` cancellation token is shared across all providers.
The captured monotonic deadline is the earliest of the caller deadline, run
timeout and original invocation timeout. Retries/fallback never renew it. Checks
occur before admission, after admission, immediately before transport and after
completion. Cancellation during admission prevents transport; cancellation or
expiry in flight still charges usage and rejects the returned result.

`BoundedInvocationPort` implementations must observe the supplied context and
bound their own blocking I/O. This synchronous application interface cannot
preempt a non-cooperating SDK call. Late results are rejected and no subsequent
call can dispatch. Live SDK interruption is an adapter qualification requirement
for #270, not established by these synthetic component tests. #269 supplies
[cooperating HTTP adapters](model-provider-adapters.md) with loopback interruption
conformance; live provider execution remains explicit opt-in.

`UncertainSideEffect` is always terminal, regardless of a subsequent lifecycle
retry decision. Invocation logic never retries a connector operation. Hard
failures are terminal too. Transport success returns only `ParsedOutput`, which
still requires #266/#267 verification; parsing grants no tool/effect authority.
No secret or raw output enters usage records. The host owns trace retention and
run recovery; this slice adds no database migration, automatic restart, pricing
service, live-provider adapter or installed runtime composition (#269/#271/#279).
Rollback is to stop routing hosts through this API; never fall back to an
unbudgeted call for a denied invocation.

## Stable diagnostics and evidence

Diagnostics serialize as `SCREAMING_SNAKE_CASE`: `INCOMPATIBLE_VERSION`,
`INVALID_LIMITS`, `INVALID_BINDING`, `AUTHORITY_MISMATCH`, `LIFECYCLE_BLOCKED`,
`CANCELLED`, `DEADLINE_EXCEEDED`, `ATTEMPT_BUDGET_EXCEEDED`,
`TOKEN_COUNT_UNAVAILABLE`, `INPUT_TOKEN_BUDGET_EXCEEDED`,
`OUTPUT_TOKEN_BUDGET_EXCEEDED`, `CONTEXT_WINDOW_EXCEEDED`,
`COST_BUDGET_EXCEEDED`, `OUTPUT_BUDGET_EXCEEDED`,
`USAGE_RECONCILIATION_MISMATCH`, `UNCERTAIN_SIDE_EFFECT`, `HARD_FAILURE`.
The existing renderer reports unavailable counting before dispatch as
`TOKENIZER_UNAVAILABLE`. Budget exhaustion requires a host decision; it never
truncates constraints/evidence or changes locality implicitly.

Requirement, architecture, automation and tester role passes were performed by
the implementing agent; no independent reviewer or approval is claimed.
Development mode permits this feature. Dependencies #262/#265 supply the frozen
fallback basis and parsing boundary. Implementation order: domain accounting →
shared run dispatch → adversarial integration fixtures → measured gates → docs.

| ID | Requirement/source | Production owner | Automated evidence (synthetic-host component level) |
| --- | --- | --- | --- |
| B01 | #268 malformed output/repair stops within aggregate limits | `InvocationRun::dispatch`, existing parser | `malformed_repair_and_retry_share_all_budgets_and_stop`, `reserves_output_and_enforces_each_limit_before_dispatch` |
| B02 | #268 cancellation prevents subsequent dispatch; cumulative deadline | Shared `RequestContext`, pre/post checks | `cancellation_deadline_and_cg14_decisions_block_subsequent_dispatch`, `cancellation_from_current_admission_prevents_transport_and_accounting`, `late_output_is_charged_and_never_accepted_after_deadline` |
| B03 | #268 uncertain effects never retried; CG-14 ownership | Terminal run state, existing `LoopDecision` | Cancellation/CG-14 test covers hard/uncertain failures and attempted redispatch |
| B04 | #268 fallback preserves scope/consent/locality and budgets | Frozen original IR, current admission | `fallback_changes_provider_model_and_tokenizer_without_resetting_usage_or_authority`, `valid_output_is_only_parsed_and_current_admission_is_mandatory` |
| B05 | #268 reserve output; per-call and cumulative tokens | Rendered accounting, immutable run limits | Budget tests; `exact_count_context_output_reserve_and_reported_context_overflow` |
| B06 | #268 exact correlated usage/provenance; observable mismatch | Attempt binding and separate local/provider records | `provider_reconciliation_keeps_original_values_and_exact_attempt_profile`, `overreported_usage_and_oversize_output_stop_run_after_charging` |
| B07 | #268 versioned contracts, >=95% materially changed production coverage | Domain/application, CI gate | `limits_wire_version_unknown_fields_and_zero_bounds_fail`, `check-invocation-resilience-coverage.py`, architecture regression |

Run `cargo test -p gateway-application --test invocation_resilience --locked`.
The EPIC-06.08 entry in `tests/scripts/quality-gates.json` measures each introduced
production file at >=95%, rejects absent/duplicate/invalid evidence, and runs the
checker self-tests. Executed results and candidate source hashes are recorded in
[evidence](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/blob/59f6db7e29478e2903cf69d6e5c7e65632876710/docs/evidence/EPIC-06.08-invocation-resilience.json). Live providers and
installed-host E2E remain separate qualification scopes.
