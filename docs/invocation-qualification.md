# Invocation trace, replay and qualification — EPIC-06.10 (#270)

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
python3 scripts/qualify-invocation.py --run --output target/invocation-qualification
```

Without `--run`, it writes a NOT_RUN plan and exits nonzero. The runner invokes the
actual Rust component suites under llvm-cov, all ten EPIC-06 per-file coverage gates,
architecture/contracts, formatting, clippy and >=95% measured runner coverage. It retains per-gate logs and hashes,
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

The machine-readable [acceptance manifest](../scripts/invocation-acceptance.json)
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

Retained candidate evidence: [EPIC-06.10 qualification](evidence/EPIC-06.10-invocation-qualification.json).
