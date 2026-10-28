# CGSL conformance and compatibility

EPIC-05.15 [#193](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/193)
provides a portable [fixture corpus](../tests/fixtures/cgsl-conformance-v1/README.md),
frozen IR/digest/source expectations, generated properties, and adversarial
component tests. [CGSL 1.0](cgsl-language-v1.md),
[SemanticTaskIR](semantic-task-ir-v1.md), and the existing interpretation,
completeness, Process/Policy and prompt boundaries remain authoritative.

## Review and delivery boundary

Three Amigos review used requirement, architecture, automation and tester role
passes by one agent; no independent review or approval is claimed. The inspected
base is `e5a9140ea925ad24d72185b35f9ac3bae737f5ec`, initially clean. DEVELOPMENT
mode permits this slice. The review admitted work in this order: supply the
missing specified canonical formatter/serializer; freeze cross-implementation
vectors; exercise frontend, resolver, completeness and adapter boundaries; wire
CI and measure coverage. No new crate dependencies or storage contracts are
needed. Estimated effort was 1–3 engineer-days including qualification, assuming
existing #186–#192 components; confidence medium. Host deployment is excluded.

Requirement pass: all fifteen requested fixture categories and the late
resumption additions have component owners and executable tests. Architecture
pass: frontend returns shapes; reference, policy and scope admission still belong
to the host/application. Automation pass: Rust tests run through Cargo and the
existing quality manifest, with Python enforcing per-file coverage. Tester pass:
compare against frozen bytes as well as self-roundtrips; repeat complete results,
change meaning, permute canonical sets, and check denied/expired/changed scopes.

The added public frontend operations are `format_source(&Document)` and
`serialize(&SemanticTaskIR)`. Formatting sorts decoded object keys by Unicode
scalar order, preserves array order and exact number tokens, uses two spaces and
one final LF, and retains source sidecars. Serialization reuses the compiler's
closed root mapping and the domain's canonical numeric/set ordering. It emits all
mapped fields and empty sidecars. Errors preserve domain serialization failures
or frontend resource diagnostics; output exceeding the frontend profile fails
rather than returning source which that frontend cannot consume. This is a
projection from IR; use formatting to preserve interpretation documents.

## Requirement and evidence matrix

All rows have component evidence. The executed candidate, commands, hashes and
coverage are recorded in [the evidence record](evidence/EPIC-05.15-conformance.json).

| ID | Requirement/source | Production owner and dependencies | Verification | Status |
| --- | --- | --- | --- | --- |
| C1 | Canonical roundtrip (#193) | CGSL serializer/compiler; domain canonical IR | Four frozen source/IR/digest triples; 512 generated admitted tasks; byte/digest equality and serialization fixed points | VERIFIED |
| C2 | Determinism (#193) | Offline parser/compiler, captured resolver | Eight repeated full results per fixture; generated set permutations, changed-meaning digest checks | VERIFIED |
| C3 | Negative diagnostic classes (#177/#193) | Frontend and host/resolver admission | Syntax, semantics, unresolved, ambiguous, conflicting, unsupported, policy-denied and version fixtures; no executable result on failure | VERIFIED |
| C4 | Major incompatibility (#193) | Independent language/task version readers | Unsupported major/minor and malformed language/task versions; version before operands | VERIFIED |
| C5 | Adversarial context (#193) | Typed interpretation projections, host scope and freshness checks | Foreign scope, stale evidence, conflicts, aliases, temporal anchors, prompt-like text retained as data; no authority changes | VERIFIED |
| C6 | Portable artifacts (#193) | Normative language + task contracts | Data manifest, raw invalid encoding, frozen canonical source and IR, digests and documented host/query profile | VERIFIED |
| C7 | Changed production >=95% (#193) | CGSL production modules | Measured per-file LLVM line gate, including new serializer; gate rejects absent, duplicate and below-threshold records | VERIFIED |
| C8 | Resumption (#193 late addition) | Same completeness compiler, fresh host captures | Refused replies remain incomplete; stale/conflicting facts block compiler; duplicate captures normalize/replay deterministically; changed project rejects old operands and accepts rebound semantics | VERIFIED |
| C9 | Model-adapter failure (#177/#193) | Optional interpretation adapters, deterministic resolver | Existing unavailable/invalid-request/invalid-proposal and thirteen proposal mutation fixtures retain identical deterministic result and no executable proposal | VERIFIED |
| C10 | Preserve downstream boundaries (#190–#192) | Semantic planning, Process/Policy, prompt reference handoff | Existing planning/output/resolution/process frontend/prompt handoff regressions included in CI component gate | VERIFIED |

C3 covers diagnostic **classes**, not a claim that every code in the language
specification is emitted by this frontend. Reserved integration codes such as
`SEM_POLICY_INCOMPATIBLE_REFERENCE`, `SEM_REFERENCE_UNAVAILABLE`,
`VER_INCOMPATIBLE_CONTRACT` and `SEM_BOUNDARY_VIOLATION` have no CGSL emitter in
this candidate. Denial is exercised through actual host `Admission`/scope
failures, and unsupported process/provider members through existing semantic
admission. Tests do not invent emitters or relabel existing errors. Existing
[domain resolution tests](../crates/gateway-domain/tests/semantic_resolution.rs)
cover reason/state validation, including invalid resolution payloads.

## Compatibility matrix

| Reader/input | 1.0 | 1.1 | 2.0 | Malformed spelling | Unknown fields |
| --- | --- | --- | --- | --- | --- |
| CGSL source | Admit supported operands | `VER_UNSUPPORTED_VERSION` | `VER_UNSUPPORTED_VERSION` | `VER_INVALID_VERSION` | Root unsupported; nested invalid operand |
| SemanticTaskIR schema | Admit valid shape | Reject | Reject | Reject | Reject |
| Interpretation capture | Admit authenticated captured basis | Reject unsupported capture version | Reject unsupported capture version | Reject | Reject |
| Referenced contract pins | Host verifies exact independent version | Host-owned | Host-owned | Invalid pin shape rejects | Reject |

The reference version is independent of the language/task schema version.
Shape compilation does not prove that a referenced contract is supported; the
trusted host must validate its captured basis before handoff. No downgrade or
subset parsing is inferred. There are no deprecated CGSL 1.0 constructs.

## Reproduction and limits

```sh
cargo test -p gateway-cgsl --all-targets --locked
cargo test -p gateway-application --test interpretation --test completeness --test semantic_planning --locked
cargo test -p gateway-domain --test semantic_task_v1 --test semantic_resolution --test semantic_output --locked
cargo test -p gateway-context --test prompt_handoff_reference --locked
CARGO_TARGET_DIR=target/epic05-15-coverage cargo llvm-cov -p gateway-cgsl --all-targets --locked --ignore-filename-regex gateway-domain --fail-under-lines 95 --json --output-path target/cgsl-conformance-coverage.json
python3 tests/scripts/check-cgsl-conformance-coverage.py --self-test
python3 tests/scripts/check-cgsl-conformance-coverage.py target/cgsl-conformance-coverage.json
```

The separate coverage target directory avoids mixing Windows PDB artifacts with
Linux LLVM objects in a shared checkout. CI's existing clean target also works.
`quality-gates.json` runs the component suite and per-file coverage gate and the
workflow retains reports. All fixtures and seeds are fixed and require no model,
network, database, wall clock or real tool execution. Generated tests supplement
frozen expectations; they do not claim exhaustive input-space proof.

#275 owns persisted clarification reply correlation, expiry and duplicate refusal;
#279 owns full runtime integration and real effects. Component tests explicitly
model accepted/refused host projections and never equate semantic clarification
with consent. This issue retains evidence for those consumers and does not
qualify a second implementation, a deployed host, or full EPIC-05 acceptance
within that historical #193 report. The subsequent independent valid-source
comparison below adds separate evidence without changing the historical report.
Rollback removes the additive serializer/API, tests, corpus and gate entries;
task wire format, pins and digests remain unchanged.

## Independent valid-source conformance witness

The parent acceptance follow-up adds two independently executed implementations
of executable-source compilation: the Rust frontend and
[`tests/conformance/cgsl_reference.py`](../tests/conformance/cgsl_reference.py).
The Python witness independently parses JSON, transcribes the normative root
mapping, validates against the published wire schema, normalizes exact decimals,
typed sets, confidence, reference scope, collection/check and CG-06 expression
order, then serializes compact canonical JSON and hashes it. It imports no Rust
code, mapping, serializer or frozen expected IR. Its inputs are source strings
only. The normative schema is shared as a specification, not an implementation.

[`independent_conformance.rs`](../crates/gateway-cgsl/tests/independent_conformance.rs)
feeds both implementations the four frozen executable-source vectors, 64 varied
sources and an omitted-defaults source. It compares the complete canonical IR
bytes and SHA-256 for all 69 sources, including every task kind, Unicode/control
escapes, i64 extremes, exact decimal coefficients beyond binary64 precision,
homogeneous sets, confidence quantization and nested commutative expressions.
The separate Python contract suite also checks the four frozen expected IR/digest
vectors without invoking Rust and rejects malformed/version/scope/schema cases.

```sh
cargo test -p gateway-cgsl --test independent_conformance --locked
python3 -m unittest discover -s tests/contracts -p test_cgsl_independent_reference.py
```

The witness qualifies equivalence for valid executable CGSL source at the IR
boundary required by parent AC24. It is test tooling, not an alternative
authoritative runtime, a deployed resolver or an implementation of the frontend's
diagnostic/span protocol. It does not qualify live authentication, acquisition,
provider invocation or interoperability for every possible input. Python and the
pinned contract-test dependencies are required; a missing interpreter/dependency
fails the differential test instead of skipping it. Set `CG_CONFORMANCE_PYTHON`
to the pinned environment's interpreter when needed (default `python3` on Unix,
`python` on Windows). Existing Rust conformance
continues to own all diagnostics and denied runtime boundaries.
