# Local and independent model adapters — EPIC-06.09 (#269)

The optional `gateway-daemon::model_adapters` module implements the existing
`BoundedInvocationPort` for the CG-27 local model service (`/v1/invoke`) and an
independent Chat Completions provider (`/v1/chat/completions`). Rust domain and
application crates have no provider dependency. No service or credential is
required for deterministic core operation or the default conformance suite.

## Integration and ownership

A trusted host prepares and admits an `InvocationIr`, constructs
`AdapterRenderer::new(kind, ir.prepared().clone())`, and passes that renderer to
`render_invocation_ir` with its model-bound tokenizer. The renderer includes the
complete wire body, generation controls and schema before token accounting.
Dispatch uses the existing `InvocationRun::dispatch` with `HttpModelAdapter` as
its port. The host retains the same run for every attempt. Current admission,
Process/Policy authority, token/cost reservations and parsing remain application
responsibilities. Neither adapter executes tools, changes scope or promotes a
model capability. Installed CG-14 host composition remains #271/#279.

Both adapters currently support JSON and JSON-schema **result-only** generation.
Requests exposing tools, tool-proposal/text modes, or unsupported controls fail
explicitly. The local bridge maps temperature, top_p, top_k, stop and seed to
Ollama options. The independent adapter maps temperature, top_p, stop and seed;
top_k fails rather than disappearing. Provider/model schema subsets and control
support must be declared accurately by the trusted capability catalog. A seed is
a hint, not a deterministic generation guarantee. Unknown material defaults
remain marked as unexposed in the effective generation record.

Transport output is wrapped in a canonical proposal containing the exact
admitted invocation identity, the raw JSON result, and empty reference/tool
lists. Raw result bytes are preserved for the strict duplicate-key parser.
Malformed output remains a parsing failure; an HTTP success is never a verified
result. Reference, constraint and grounding verification are still mandatory.
Tasks requiring generated evidence-reference metadata need a future explicitly
qualified adapter mode; these result-only adapters cannot supply that evidence.

## Local service and identity

`/v1/infer` retains the CG-27 legacy contract. `/v1/invoke` selects the exact
installed model profile ID, requires an active, qualified profile, checks its
explicit input/output contract inventory, model version and artifact digest,
then calls `/api/chat` with the admitted messages and controls. The service checks
runtime/model/template provenance before and after generation and rejects an
unexpected runtime model or incomplete/tool-call response. It never substitutes
an active alias or injects the old semantic-proposal suite's system prompt.
The Docker image includes the canonical bridge.

The shipped `models/qwen3-8b-q4.json` advertises only the legacy contract. Do not
add canonical support merely because `/health` or `/ready` succeeds. An operator
must install and qualify a distinct immutable candidate profile with the
required contract inventory and adequate context capacity, then supply separate
task-specific semantic qualification evidence before catalog promotion. Legacy
CG-27 qualification and this transport suite do not establish SemanticTaskIR
accuracy. No model profile has been promoted by this change.

A successful adapter observation records the exact model/provider pins, actual
returned model ID, renderer/template/tokenizer profile and effective generation.
Local artifact/version mismatches and independent returned-model mismatches
fail closed. For a remote provider, the exact returned snapshot ID is checked;
artifact/revision pins are catalog/qualification evidence, not a claim that the
remote API cryptographically attests model weights. The host must collect each
observation immediately; the adapter keeps only its latest observation.

## Transport and credentials

Construct `HttpModelAdapter::new(kind, endpoint, runtime_credential)` only in the
outer host. Credentials have no serialization or Debug representation. The live
profile contains an environment-variable **name**, never its value. curl receives
authorization through stdin, not argv, disk, prompt or receipt. Ambient curlrc,
proxy/credential environment, redirects and internal retries are disabled;
stderr and provider error bodies are discarded. A response reflecting the
configured credential is rejected before it can become output. HTTPS is required
except for literal `127.0.0.1`/`localhost`; an ON_DEVICE provider cannot use a
remote endpoint. Endpoint userinfo, query, fragment and invalid ports are denied.
Deploy behind a trusted endpoint without redirects. curl must be installed.

The transport polls shared cancellation/deadline state, kills and reaps curl on
cancellation, bounds response reads, and caps the final canonical output bytes.
The byte ceiling includes proposal-envelope overhead. HTTP 429/5xx and transport
availability failures are transient; other HTTP errors, identity errors and
malformed provider envelopes are terminal adapter failures. Stable diagnostic
codes are in `tests/fixtures/model-adapters-v1/diagnostics.json`; observations
never contain raw provider errors. Provider usage is correlated to the exact
attempt. Missing usage retains reservations; the trusted cost upper bound is
retained because this adapter does not invent provider pricing. Cancelling the
HTTP client does not prove the server has stopped computation or billing.

## Qualification commands

Default tests use loopback HTTP fixtures and synthetic trusted admission, with
no real credentials or models:

```sh
cargo test -p gateway-daemon --test model_adapters --locked
python3 -m unittest discover -s tests/local-model
```

The quality gate measures >=95% line coverage for the Rust adapter module and
Python canonical bridge. Existing local service regressions cover both routes.
The conformance suite includes full-body rendering, untrusted evidence isolation,
all mapped controls, strict identity, malformed/oversized output, usage,
HTTP denial/failure, cancellation and the existing run/parser boundary.

Copy `tests/fixtures/model-adapters-v1/live-profile.example.json` outside the repo
and replace both exact model pins, endpoints, localities and context limits.
The first entry is local, the second independent. Local profiles must explicitly
support `prompt-ir-v1` and the fixture's `ArtifactInspection` output contract; their context limit must cover the
conservative UTF-8 byte upper bound plus output reserve. The independent model
must support JSON-schema object output and the requested generation controls.
The example endpoint follows the [Chat Completions API reference](https://developers.openai.com/api/reference/resources/chat).
This profile sends synthetic fixture context only; it is not production host
admission or task accuracy qualification.

```sh
CG_MODEL_ADAPTER_LIVE_PROFILE=/private/model-adapter-profile.json \
CG_MODEL_ADAPTER_LIVE_OUTPUT=/private/model-adapter-live-report.json \
cargo test -p gateway-daemon --test model_adapters --locked \
  live_adapter_profile -- --exact --ignored
```

Supply `CG_INDEPENDENT_PROVIDER_KEY` through the runtime environment when required.
The report records `TRANSPORT_PASS`, receipts, and `semantic_qualification:
NOT_CLAIMED`. The default suite ignores this test. Live qualification has not
been executed for this candidate; no live provider/model quality is claimed.

## Requirement review and traceability

Requirement, architecture, automation and tester perspectives were role passes
by the implementing agent, not independent approvals. At the inspected base
revision (recorded in the evidence report), #264 and #266–268 supply rendering,
parsing/verification and resilience. The missing local canonical route is included
here, so the implementation ordering is acyclic: renderer/transport → service
bridge → conformance/run integration → gates/documentation/evidence.
Readiness decision: READY_FOR_WORKFLOW for this bounded adapter slice.
Indicative effort: 1–2 engineer-days implementation, 0.5–1 qualification; live
runtime setup depends on installed models/provider access and is separate.

| ID | Requirement/source | Production owner/path | Dependency/state | Verification/evidence level | Status |
| --- | --- | --- | --- | --- | --- |
| A01 | #269 both adapters share contracts | model_adapters.rs / BoundedInvocationPort | #264/#268 implemented | model_adapters Rust conformance and run/parser loopback integration | VERIFIED |
| A02 | #269 core works without either | inward dependency graph, optional daemon module | no SDK/service/core startup dependency | workspace tests, architecture guard | VERIFIED |
| A03 | #269 actual model/renderer identity | AdapterReceipt, local canonical provenance checks | pinned catalog plus provider response | positive and mismatched-model/version/artifact fixtures | VERIFIED |
| A04 | #269 readiness is not semantic qualification | canonical.py contract inventory gate | CG-27 qualification remains bounded | Python readiness/unsupported-contract denial | VERIFIED |
| A05 | #269 credentials stay runtime-only | HttpModelAdapter stdin authorization | trusted host config | credential-header/payload/receipt/reflection negative fixtures | VERIFIED |
| A06 | parent generation/budget/isolation controls | AdapterRenderer / InvocationRun | #263/#264/#268 | wire equality, controls, hostile data, cancellation/output ceilings | VERIFIED |
| A07 | #269 opt-in live qualification profile | ignored live_adapter_profile | real runtime/model/provider required | profile supplied; actual live execution NOT_RUN | VERIFIED profile; live NOT_RUN |

Retained candidate evidence: `docs/evidence/EPIC-06.09-model-adapters.json`.
Rollback removes the optional module/route; legacy inference remains separately
covered. No issue closure, installed-host qualification or parent epic acceptance
is claimed.
