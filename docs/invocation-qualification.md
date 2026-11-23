# Invocation trace, replay and qualification — EPIC-06.10 (#270)

Historical evidence and removed review documents are linked to their archived Git
revision. They are not current acceptance evidence. Generate fresh reports in an
output directory or retain them as CI artifacts.

The invocation plane now provides content-free trace capture, deterministic
revalidation of recorded outputs, role-specific metric gates and an executable
revision-bound fixture qualification runner. Models propose; deterministic Gateway
checks accept or reject. This work does not establish reproducible stochastic
generation, execute tools, promote a model or qualify the installed host.

## Requirement and delivery gate

Inspected base: `e30647c` (#269), initially clean worktree, DEVELOPMENT mode.
The complete #178 acceptance criteria and #270 task-specific metric additions were
reviewed as requirement, architecture, automation and tester role passes by the
implementing agent. No independent reviewer or approval is claimed.

Decision: **READY_FOR_WORKFLOW** for the component qualification mechanism.
Existing #261–#269 contracts, CG-10 producer, current-admission and independent
verification ports are the prerequisites. They exist; fixture tests exercise the
actual application implementations and two actual HTTP adapters against loopback
providers. Current-store authentication, Process/Policy, grounding and semantic
checks remain trusted host responsibilities. Installed host integration belongs
to #271/#279. Transport readiness cannot establish semantic model fitness.

The bounded slice owns the new domain/application qualification modules, replay
fixtures, daemon metric-evaluator example, runner, manifest, checks and these docs.
It preserves the existing renderer, parser, grounding and cumulative run services.
Rollback removes the optional qualification modules/example/runner and their module
exports and gate registration; no storage migration or deployed behavior changes.
Stop conditions are missing criterion mappings, provenance/source drift, failed
gates, absent measurements or a requirement to bypass existing >=95% gates.

Planning estimate at intake: 1–2 engineer-days implementation plus 1–2 days fixture
qualification and review, medium confidence. Existing foundations are included as
available dependencies, with no new host foundation work. Live dataset execution,
installed host qualification and model promotion require separate environment and
operator work; this range is engineering effort, not elapsed calendar time.

## Ownership and production path

`gateway-domain::invocation_qualification` contains strict v1 profile, provenance,
measurement, role and diagnostic contracts. No provider SDK types enter the core.
`gateway-application::invocation_qualification` owns `trace_output`, `replay_output`
and `qualify`. It has no dispatch port. The daemon's `qualify-invocation` example
is a read-only CLI composition root over the authoritative Rust evaluator.

The host captures `InvocationTrace` from the immutable admitted Invocation IR,
its correlated rendered payload and exact response bytes. It includes task,
context, output/rule pins, full IR/tool digests, model/provider identity, effective
generation digest, renderer/tokenizer profile and exact/estimated token accounting.
Existing `InvocationRun` attempt records carry attempt identity, provider usage,
local estimates, reservations, discrepancy diagnostics and cumulative usage. Keep
these correlated records together under the existing task/project scope. Renderer,
generation and raw result content stay in policy-permitted stores; digests identify
them without duplicating raw prompts or outputs. No hidden chain-of-thought is
required. IDs and digests can themselves be sensitive; access and retention are
host policy decisions. This slice supplies no persistence or permissive store.

Replay receives the original admitted IR, recorded binding, exact bytes loaded
under host policy and current `OutputVerificationPort`. It checks version and
byte/IR digests, reparses the complete output with the existing strict parser and
rechecks references, tool inventory/argument schemas, current authority, semantic
constraints and required grounding. A stale or revoked invocation fails. Historical
PASS is never authority. The return value contains only an audit receipt and
stable diagnostic: it cannot be passed to tool dispatch as a verified result.
Fragments and repair remain bounded provisional paths. No provider or effect port
is available to replay. Tests replay both adapter responses after their one-request
HTTP fixture servers have terminated; no second transport request is possible.

## Role-specific model fitness

Each `QualificationProfile` pins the role, thresholds and model family/id/revision/
digest, tokenizer/version, runtime/version, quantization, generation profile digest,
renderer/template revision, dataset/version/digest and hardware class. The profile
itself has an identity/revision pin; decision receipts additionally hash the full
profile and measurement content, including thresholds and optional diagnostics.

Extraction requires schema conformance, field correctness, required completeness,
reference accuracy, clarification correctness, unsupported-claim rate and citation
validity. Tool proposal roles require tool conformance, identifier validity,
argument validity and unauthorized/unavailable-tool refusal. Both require bounded
recovery, context-limit and failure/fallback behavior, cold/warm latency, throughput,
CPU/RAM, input/output token usage; GPU/VRAM measurements are mandatory when used.
Each metric has its own minimum, maximum and minimum sample count. Missing metrics,
invalid counts, insufficient samples, mismatched provenance or any failed threshold
reject the candidate; there is no aggregate score.

Rates use integer successes/total and thresholds in basis points, compared without
rounding using u128. Unsupported claims are independently counted adverse events
with an upper bound. Scalars use denominator one and units named by the metric:
latency milliseconds, throughput in thousandths of cases/second, CPU/GPU milliseconds,
RAM/VRAM bytes and token counts. The trusted dataset evaluator chooses and documents
its aggregation (for example p95 latency and peak memory), hardware, cold/warm setup,
case records and sampling method. GPU metrics are explicitly conditional. The
`schema_version` freezes units and metric semantics; profile revisions freeze
thresholds and evaluation methodology.

Confidence-bearing models without a declared, passing calibration-error gate are
explicitly **uncalibrated**. Calibration error uses basis points and must come from
a trusted labeled calibration dataset; confidence never grants acceptance authority.
Perplexity and cross-entropy are optional diagnostic strings associated with the
pinned dataset/tokenizer/model. They do not participate in promotion. Cross-model
comparisons require separate tokenizer/dataset justification.

Measurements are **trusted evaluator input**, not claims that an uploaded JSON file
is authentic evidence. The evaluator must retain every case and deterministic grader
result, independently measure unsupported claims, and authenticate all pins against
its stores. The core cannot attest to hardware or provider artifacts from labels.
Fixture inputs cannot become live evidence by changing a flag. The `Live` kind is
reserved for an authenticated host evaluator; its decision indicates metric
eligibility only. Promotion still belongs to the existing release authority.

## Operator commands

Prerequisites: the repository Rust toolchain, llvm-tools/cargo-llvm-cov, Python
contract dependencies and coverage.py (>=7). No model runtime or credentials are
needed for fixtures. Run fixture qualification (new output directory required):

```sh
python3 tests/scripts/qualify-invocation.py --run --output target/invocation-qualification
```

Without `--run`, it writes a NOT_RUN plan and exits nonzero. The runner invokes the
actual Rust component suites under llvm-cov, all ten EPIC-06 per-file coverage gates,
the Python local-model tests with >=95% canonical bridge coverage,
architecture/contracts, formatting, clippy and >=95% measured runner coverage. The
`local-model-python` gate retains its measured coverage JSON and blocks both parent
qualification and mapped adapter/coverage criteria if tests or coverage fail.
It retains per-gate logs and hashes,
criterion outcomes, Git revision and every tracked/untracked source digest, and
rejects source drift. Historical `docs/evidence/` artifacts and the output directory
are excluded from source fingerprints; their contents are never substituted for
freshly executed gates. Failure or zero executed Rust tests stops qualification;
remaining gates are NOT_RUN. `COMPONENT_PASS` describes fixture mechanisms only;
`release_status` stays NOT_COMPLETE and live/promotion stay NOT_RUN.

Evaluate authenticated role measurements through the Rust CLI:

```sh
cargo run -p gateway-daemon --example qualify-invocation --locked --   path/to/profile.json path/to/measurements.json
```

The CLI emits a decision including failing metric IDs and input digests. It exits
nonzero for a failed gate or fixture-only input. The checked-in synthetic example
is expected to exit nonzero. It performs no inference or promotion. Thresholds and
sample counts in the example are test values, not deployment recommendations.

Live transport opt-in remains documented in [adapter qualification](model-provider-adapters.md)
and `tests/fixtures/model-adapters-v1/live-profile.example.json`. It uses runtime-only
credentials and explicit immutable installed model pins. TRANSPORT_PASS from that
profile does not qualify task metrics. No live run is claimed here; concrete live
role metrics and installed-host composition remain required before full release.

## Criterion traceability

The machine-readable [acceptance manifest](../tests/scripts/invocation-acceptance.json)
contains exact test names and executable gate dependencies for all 22 parent criteria,
11 issue criteria and four tokenization/operational/security additions. The runner
verifies mapped test symbols exist, reruns their entire suites and requires each
named test to appear as passed in fresh logs. The evidence
level is COMPONENT: real production services, synthetic trusted hosts and loopback
provider fixtures. Every mapped criterion is a mechanism claim at this level;
full parent/installed/live acceptance remains PARTIAL until those paths are proven.

| ID | Requirement/source | Implementation owner | Verification | Status/evidence |
| --- | --- | --- | --- | --- |
| P01 | Models are represented as replaceable capabilities behind provider-independent ports. | existing #261–#269 services | Named tests and gates in manifest | See current qualification report; COMPONENT |
| P02 | No provider SDK type leaks into canonical domain/context/process contracts. | existing #261–#269 services | Named tests and gates in manifest | See current qualification report; COMPONENT |
| P03 | The same typed inputs and renderer version produce equivalent provider-facing prompt structure. | existing #261–#269 services | Named tests and gates in manifest | See current qualification report; COMPONENT |
| P04 | Conversation history is not implicitly injected. | existing #261–#269 services | Named tests and gates in manifest | See current qualification report; COMPONENT |
| P05 | Authority, user input, evidence, knowledge, memory and constraints remain distinguishable through rendering. | existing #261–#269 services | Named tests and gates in manifest | See current qualification report; COMPONENT |
| P06 | Serialization cannot promote untrusted content into Gateway authority. | existing #261–#269 services | Named tests and gates in manifest | See current qualification report; COMPONENT |
| P07 | Model routing is explicit, inspectable and bounded by capability and policy constraints. | existing #261–#269 services | Named tests and gates in manifest | See current qualification report; COMPONENT |
| P08 | No model is called when deterministic execution is sufficient. | existing #261–#269 services | Named tests and gates in manifest | See current qualification report; COMPONENT |
| P09 | Every model invocation carries an explicit output contract. | existing #261–#269 services | Named tests and gates in manifest | See current qualification report; COMPONENT |
| P10 | Model output is parsed and validated before becoming a Gateway result. | existing #261–#269 services | Named tests and gates in manifest | See current qualification report; COMPONENT |
| P11 | Schema, references, evidence and constraints can be verified independently of model confidence. | existing #261–#269 services | Named tests and gates in manifest | See current qualification report; COMPONENT |
| P12 | Optional model critics cannot be the sole acceptance authority. | existing #261–#269 services | Named tests and gates in manifest | See current qualification report; COMPONENT |
| P13 | Retry/fallback behavior is bounded and cannot expand scope or permissions. | existing #261–#269 services | Named tests and gates in manifest | See current qualification report; COMPONENT |
| P14 | Local SLM and remote/provider adapters can satisfy the same abstract capability where compatible. | existing #261–#269 services | Named tests and gates in manifest | See current qualification report; COMPONENT |
| P15 | Invocation provenance supports audit and replay without requiring chain-of-thought persistence. | domain/application qualification | Named tests and gates in manifest | See current qualification report; COMPONENT |
| P16 | Architecture, security and adversarial tests include prompt injection, malformed output, unsupported references, stale context and provider failure. | existing #261–#269 services | Named tests and gates in manifest | See current qualification report; COMPONENT |
| P17 | At least **95% coverage** for introduced/materially changed production code. | existing #261–#269 services | Named tests and gates in manifest | See current qualification report; COMPONENT |
| P18 | Stream fragments, malformed output and schema repair never trigger tool execution. | existing #261–#269 services | Named tests and gates in manifest | See current qualification report; COMPONENT |
| P19 | Tool proposals bind authorized inventory, arguments, scope, task and revision before host dispatch. | existing #261–#269 services | Named tests and gates in manifest | See current qualification report; COMPONENT |
| P20 | Invocation, repair and fallback consume the aggregate session budget; they cannot reset it. | existing #261–#269 services | Named tests and gates in manifest | See current qualification report; COMPONENT |
| P21 | Replay revalidates recorded outputs without redispatching external actions. | domain/application qualification | Named tests and gates in manifest | See current qualification report; COMPONENT |
| P22 | Deterministic rendering/validation claims are kept separate from probabilistic model generation. | domain/application qualification | Named tests and gates in manifest | See current qualification report; COMPONENT |
| Q01 | Each EPIC-06 criterion maps to tests | domain/application qualification | Named tests and gates in manifest | See current qualification report; COMPONENT |
| Q02 | two adapter paths pass conformance | domain/application qualification | Named tests and gates in manifest | See current qualification report; COMPONENT |
| Q03 | fixture and live evidence are clearly labelled | domain/application qualification | Named tests and gates in manifest | See current qualification report; COMPONENT |
| Q04 | deterministic replay cannot redispatch actions | domain/application qualification | Named tests and gates in manifest | See current qualification report; COMPONENT |
| Q05 | failed gates block qualification. | domain/application qualification | Named tests and gates in manifest | See current qualification report; COMPONENT |
| Q06 | Promotion gates use role-specific metric thresholds, not a single aggregate score. | domain/application qualification | Named tests and gates in manifest | See current qualification report; COMPONENT |
| Q07 | Schema-valid but semantically wrong output can fail qualification. | domain/application qualification | Named tests and gates in manifest | See current qualification report; COMPONENT |
| Q08 | Hallucination/unsupported-claim evidence is independently measured. | domain/application qualification | Named tests and gates in manifest | See current qualification report; COMPONENT |
| Q09 | Confidence-bearing models have calibration evidence or confidence is treated as an uncalibrated signal. | domain/application qualification | Named tests and gates in manifest | See current qualification report; COMPONENT |
| Q10 | Perplexity, when present, is labelled diagnostic and cannot independently pass a model. | domain/application qualification | Named tests and gates in manifest | See current qualification report; COMPONENT |
| Q11 | Qualification evidence is revision-bound to model, tokenizer, runtime, profile and dataset. | domain/application qualification | Named tests and gates in manifest | See current qualification report; COMPONENT |
| T01 | Tokenizer identity and exact/estimated counting, complete payload/output reserve and typed context overflow | existing #261–#269 services | Named tests and gates in manifest | See current qualification report; COMPONENT |
| T02 | Generation controls are explicit and unsupported controls are denied | existing #261–#269 services | Named tests and gates in manifest | See current qualification report; COMPONENT |
| T03 | Cold/warm latency, throughput, CPU/RAM and applicable GPU/VRAM, token usage and failure/context behavior have independent gates | domain/application qualification | Named tests and gates in manifest | See current qualification report; COMPONENT |
| T04 | Sensitive data isolation and cancellation are adversarially tested | existing #261–#269 services | Named tests and gates in manifest | See current qualification report; COMPONENT |

Review source and report hashes before using the evidence for another candidate.
The report records the precise candidate and outcomes; a historical green report
cannot qualify changed code. Parent issue closure and live model promotion are
not implied by completion of this component implementation.

Retained candidate evidence: [EPIC-06.10 qualification](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/blob/59f6db7e29478e2903cf69d6e5c7e65632876710/docs/evidence/EPIC-06.10-invocation-qualification.json).

## Operator invocation host

`gateway-daemon::invocation_host` and the shipped `cg-invoke` binary compose the
existing application services for a registered read-only Inspect/Analyze
proposal-evaluation role. Real source files, schema content digests, current
operator admission, model/generation declarations and tokenizer identity/version
are required. Every output leaf is compared with an independently loaded pinned
oracle and projected into CG-06 observations, facts and evidence for grounding.
A valid JSON shape cannot accept a wrong value.

Operator admission is trusted launcher configuration protected by the operator's
filesystem identity. Model/client JSON cannot supply grants or verifier outcomes.
The host reloads admission/source/oracle digests before dispatch and verification;
revocation, stale content, unsupported references and process-bound tasks deny.
Current CG-09 evaluation authorizes the inspect capability. CG-14/EPIC-08 retain
task planning, interaction and lifecycle ownership. The frozen Codex structured
artifact session goal remains its existing registered goal.

```sh
cargo run -p gateway-daemon --bin cg-invoke --locked -- \
  --admission /private/admission.json --principal operator \
  --invocation proposal-1 --operation invoke
```

Admission registers the exact SemanticTaskIR, ExecutionContextIR, output contract,
source/oracle pins, provider/capability, runtime observation, generation, endpoint,
optional runtime model mapping, budget group, expiry and absolute journal directory.
The live runner produces concrete admission files for its pinned cases. Other
roles require their own authoritative verifier/host integration. The result
verifies a proposal for the registered role; external effects require their
existing owners' independent observations.

A private exclusively locked journal per owner/scope/budget group retains the
original absolute deadline and aggregate usage across invocations and restarts.
The daemon requires Rust 1.89 or newer for native `File::lock`; the other workspace
crates retain their declared Rust 1.85 minimum. The lock is released by the OS on
process exit, including an interrupted invocation.
Every dispatch durably reserves tokens, bytes, cost and an attempt before
transport. Unknown usage retains conservative reservations; provider reports
cannot refund outer token reservations. Repeated IDs are refused, including an
interrupted in-flight dispatch. Atomic replacement and fsync retain IR/rendering,
raw output, parsed proposal, verification, elapsed time and attempt receipts.
Provider CPU/RAM are explicitly `NOT_REPORTED` at this transport boundary; the
live evaluator measures them separately.

Use `--operation inspect`, `cancel` or `replay` with the same options. Cancellation
persists and interrupts cooperating HTTP calls. Replay rechecks current authority
and returns an audit receipt with no dispatchable result or provider/effect port.
Deterministically sufficient tasks take the zero-model path. The bounded JSON
journal is limited to one MiB; storage failure denies acceptance and retains prior
reservations. On Unix its directory must support private permissions (0700).
For WSL drive mounts without permission metadata, use a private Linux filesystem
path. Writers serialize per budget group; shared task-session backpressure and
lifecycle recovery remain EPIC-08 responsibilities.

## Live role qualification through the shipped host

Explicitly install, base-qualify and activate an immutable candidate in an isolated
model service. The versioned 4B CPU candidate manifests and `semantic-proposal-v6` suite
retain the original five cases/limits plus the five previously added controls.
The final prompt clarifies JSON null versus the literal string "null"; no expected
case output, schema, operational limit or quality threshold is relaxed. Failed earlier
candidates remain failed; source manifests remain unqualified candidates.
Readiness does not promote a Gateway capability or prove full SemanticTaskIR
interpretation fitness.

The second endpoint must reach a real Chat Completions runtime and return its
exact configured wire model name. Both adapters may use the same backend; the
report labels this and makes no independent-provider or cloud-model claim.

```sh
python3 tests/scripts/qualify-invocation-live.py \
  --installed-profile /private/installed-model.json \
  --service cg-invocation-model-q8 \
  --runtime cg-invocation-runtime-live \
  --local-endpoint http://127.0.0.1:8093 \
  --chat-endpoint http://127.0.0.1:11437 \
  --output /home/operator/.cache/invocation-live-new
```

The runner builds and executes the actual host binary and Rust metric evaluator,
authenticates the active registry/runtime/model/template, and pins runtime
tokenizer metadata. Counting is explicitly estimated with a full-payload UTF-8
upper bound and chat framing allowance. Exact tokenizer availability is not
claimed. The immutable runtime snapshot pins `num_ctx` so native and compatible
chat requests share the declared context limit.

All ten cases run through each HTTP adapter. Exact field/reference/ambiguity and
unsupported-claim graders use frozen labels. Schema-valid wrong values fail
independently. Further executed probes cover outage, cancellation, context
overflow, aggregate budgets, semantic rejection and receipt-only replay. Each
metric family has its own Rust threshold. Measurements include cold/warm host
latency, throughput, runtime cgroup CPU, resident model/context RAM and correlated
tokens. Missing measurements, failed gates or source drift prevent
`LIVE_ROLE_PASS`. Gateway capability promotion remains a separate decision.

The evaluator must exit successfully and return a live decision with both
`gates_passed` and `eligible_for_promotion` set to the JSON boolean `true`.
An approval in a failed process, fixture decision or string cannot pass. Replay
must return exactly the audit receipt bound to the recorded output and invocation;
missing receipts, changed bindings and dispatchable result fields fail the probe.

Continuation evidence is retained in
[EPIC-06 host/live continuation](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/blob/59f6db7e29478e2903cf69d6e5c7e65632876710/docs/evidence/EPIC-06-host-live-continuation.json).
Live candidates are evaluated in the isolated Docker laboratory described below.
Offline checks and a base-suite pass cannot substitute for canonical-host HTTP
conformance and the complete role-specific live gates.

Candidate execution results and exact remaining acceptance gaps are retained in
[the host/live evidence record](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/blob/59f6db7e29478e2903cf69d6e5c7e65632876710/docs/evidence/EPIC-06-host-live.json). A base-suite pass
is separate from canonical-host qualification. Any failed semantic output,
missing hardware capacity or unsuccessful metric gate leaves the live role
unqualified; neither readiness nor a component pass closes the parent epic.
The operator host wire format is version 8 (`operator-task` version 3); the
original version 1 wire format remains available to existing adapter callers.

The daemon requires Rust 1.89 or later for the standard-library file locks used
by durable journals. Mechanical let-chain and argument-iteration updates preserve existing checks
under the corresponding Clippy rules. The complete workspace test run covers these
compatibility changes in addition to the invocation-specific gates.


The retained CPU candidate uses a separate Ollama 0.11.10 container with
`OLLAMA_FLASH_ATTENTION=1`, `OLLAMA_KV_CACHE_TYPE=q8_0`,
`OLLAMA_NUM_PARALLEL=1` and `OLLAMA_MAX_LOADED_MODELS=1`. Its native service
uses a separate registry volume and role. The compatible Chat Completions
adapter reaches that real runtime directly through a loopback-published port.
The existing standard service/runtime configuration is preserved. The Q8 cache
is distinct from the model's Q4_K_M weight quantization; changing either requires
fresh qualification. [Ollama's cache configuration](https://docs.ollama.com/faq#how-can-i-set-the-quantization-type-for-the-kv-cache)
describes the supported runtime settings.

Live evidence binds the container image digest, allowlisted Ollama environment
settings, memory/CPU limits and immutable runtime/model/template identities.
Runtime configuration is rechecked around each adapter's measurements. Only the
allowlisted public settings enter the report. Actual cache allocation is retained
with the runtime log; declared settings alone are not a memory measurement.


`OperatorTaskV3` preserves the complete task, compiled context, selected evidence,
examples, output definition, JSON schema and tool definitions inline. The
original full-data renderers remain available. The operator presentation replaces
only the derived Gateway binding/planning/verifier receipts with SHA-256
references to their canonical serialized IR sections; this is not another context
selection or truncation step. All task/context constraints and verification
requirements remain inline. The host retains the complete IR and actual rendering
in its private journal after every completed model run, including failures.
Those full records are not added to stdout. Replay and independent verification
continue against the complete canonical IR and current authority.

An optional integer `resources.cpu_threads` (1–256) pins `num_thread` into the
immutable Ollama snapshot beside `num_ctx`. Both HTTP paths inherit the same
model defaults. The final CPU candidate requests four threads; the actual snapshot
parameters are observed in the live report. Original profiles without this field
retain their existing runtime defaults. Profile/schema/implementation changes
invalidate earlier registry qualification bindings and require a new candidate.

The registered qualification goal is `ANALYZE`: it classifies an observed file
request rather than inspecting or modifying the named target file. The read-only
host policy and empty tool inventory stay the same. Its typed goal explicitly
relates ambiguity to an unknown action or empty target. A final focus field
repeats only the admitted goal and observed evidence content in the same user
message, after the audit references. It never includes expected oracle labels.

The operator system directive is fixed compiler-owned prose. It distinguishes
requested computation from Gateway permission authority, and identifies task_type
and target.reference as operation/source metadata. No task-goal or evidence prose
is interpolated into the system channel. The admitted analysis goal includes
rule examples with unrelated names (draft.yaml/report.ini); expected dataset
labels and oracle contents are never model inputs.

The candidate record identifies the actual selected immutable model profile.
The original Qwen3 4B and the instruction-tuned 2507 variant were evaluated
as separate candidates; only the instruction-tuned profile passed the final
host-role qualification. A failed candidate is never renamed into a pass. The
latter source is [Qwen3-4B-Instruct-2507](https://huggingface.co/Qwen/Qwen3-4B-Instruct-2507)
with its separately pinned Ollama artifact/template. A one-case reasoning-mode
diagnostic on the original model did not satisfy the frozen write expectation;
no reasoning control or acceptance threshold was changed on that basis.


The optional admission `registered_role: FILE_REQUEST_EXTRACTION_V1` explicitly
selects a bounded file-request analysis role. It requires the registered ANALYZE
goal, extractor agent/skill, matching scope and exact three-field projection.
Unmatched definitions and unknown roles fail before dispatch. The default generic
operator renderer remains version 8. The registered renderer is
`file-request-extraction` version 1, transport version 9. Its fixed trusted
instruction is stored in `crates/gateway-daemon/src/file_request_extraction_v1.txt`;
the admission, retained instruction-profile SHA-256 and live source fingerprint
bind the selection and exact text. Dynamic goals and evidence stay in the user
channel. The registered user object also exposes `request`, an exact alias of the
single selected evidence content, while preserving all six semantic sections,
output schema and audit references. Oracle labels never enter this payload.


The final live profile explicitly requests the supported `JSON` output mode,
with an unchanged exact three-field output contract and independent schema and
semantic verification. There is no mode fallback. A retained paired diagnostic
with the same complete registered-role packet showed that grammar-constrained
`JSON_SCHEMA` emitted `ambiguous` before `target` and misclassified an empty
target; JSON emitted target before deriving ambiguity and satisfied the invariant.
The supported mode selection changes generation provenance and requires a fresh
full qualification. All frozen cases, labels, limits and verifier gates remain
unchanged; malformed or semantically incorrect JSON is still rejected.


The executed final V12 candidate passed both real adapters (`LIVE_ROLE_PASS`),
with ten exact cases and six operational probes per adapter. Native cold/warm
maximum latency was 113627/75571 ms, runtime CPU 3394717 ms and resident model
memory 3796726272 bytes. Chat Completions measured 112403/75975 ms, CPU 3388121 ms
and memory 4602032640 bytes. These measurements apply to the pinned registered
role, model and isolated runtime configuration. The [evidence record](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/blob/59f6db7e29478e2903cf69d6e5c7e65632876710/docs/evidence/EPIC-06-host-live.json)
retains source-bound reports and synthetic full journals; earlier failed attempts
remain failed. No capability-catalog promotion was performed.
