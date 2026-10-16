# Completeness validation and clarification planning

EPIC-05.10 (#188) delivers `gateway_application::completeness`, following
[ADR-015](adr/ADR-015-semantic-task-ir-boundary.md), the
[gap contracts](semantic-resolution-contracts.md) and
[deterministic interpretation resolver](deterministic-interpretation-resolution.md).

`CompletenessValidator::validate` authenticates the captured context and the
complete, host-derived `SemanticField` manifest, validates its dependency graph,
resolves every field through #187, detects typed gaps and emits a versioned
`ClarificationPlan`. `canonical_bytes` provides the result's stable diagnostic
sidecar, including the full resolution trace and provenance. None of these
records grants execution, policy authority or mutation consent.

## Trusted integration contract

The host derives the manifest from the compiled semantics, including structurally
optional inputs/state/evidence that output or verification requirements depend on.
`CompletenessHost::validate_requirements` MUST authenticate this entire manifest
against the exact input, task and semantic revision. It must reject omitted or
downgraded requirements. This is a trusted application port, not a caller-supplied
list of questions. A pinned task binding is required before this service emits a
plan, including for a newly allocated task. Structural mandatory fields remain
independently enforced by the CGSL frontend and `SemanticTaskIR` constructor.

The host admits current input/context and captured verified memory/retrieval facts
through the existing Interpretation Context contract. Entity lookup uses the
existing registry/project ports and finite permitted sources with per-source
result limits. There are no additional memory permissions, retry loops, network
clients or parallel knowledge coordinators. All fields are resolved before the
result is returned; an unavailable permitted source leaves a gap. Acquisition
adapters must enforce their own I/O deadlines under the existing port contracts.

There are at most 128 fields per validation. Unknown dependencies, cycles,
duplicate fields, missing task correlation and mandatory dependencies on advisory
fields reject the manifest before lookup. Fields use stable topological order
with lexical field-path tie breaks. A field stays blocked while any prerequisite
is unresolved, even if that field itself has a candidate value.

Only unresolved mandatory fields at the dependency frontier become questions.
Each question contains a stable SHA-256 ID, field, expected meaning, typed reason
and diagnostic code. Its identity binds task/request/scope, semantic revision and
the exact capture. Advisory gaps stay in diagnostics and must be omitted from the
compiled task. Invalid explicit references request input repair; unsupported
intent remains unsupported. Neither is converted to a question about missing
knowledge. Candidate details remain in diagnostics for host-controlled disclosure.

`compile` invokes the normal semantic compiler only after completeness succeeds,
then revalidates the capture, resolved decisions, task identity, scope and all
semantic reference bindings. The supplied compiler is trusted: it must bind the
resolved meanings to its operands and preserve independent source declarations.
The callback is a composition seam, not a sandbox for untrusted code. A complete
result alone is not an executable handoff. Process/Policy gates remain in force.

The host captures a clarification reply as fresh input and derives fresh
requirements, then calls this same entrypoint. It cannot patch an old task or
reuse an old completeness result. Changed context must pass all normal checks.
#275 owns persisted questions, expiry, reply correlation and duplicate refusal;
this service owns no conversation state and exposes no consent operation.

## End-to-end examples

The runnable examples are in
[`tests/completeness.rs`](../crates/gateway-application/tests/completeness.rs).
They use real domain/resolver/frontend code with an injected fixture authority.

| Input | Context / dependency | Result |
| --- | --- | --- |
| Missing target | No admitted facts | Mandatory `SEM_MISSING_REQUIRED_FIELD`; no invented target |
| Missing target | Unique eligible situation, memory or retrieval target | Complete without a question |
| Target, goal and evidence missing | evidence depends on goal; goal depends on target | Ask target only; retain all three gaps |
| Two independent missing fields | No dependency | Both questions, ordered by field path |
| Optional history missing | Target resolved | Complete, history omitted, advisory diagnostic retained |
| Two possible targets | Equal eligible focus tier | Ambiguity retained; no confidence winner |
| Explicit canonical target absent | Exact lookup fails | Input repair; no fallback target |
| Reply supplies target | Fresh admitted current input | Same completeness entrypoint, real CGSL compilation, then reference validation |
| Reply uses expired or changed basis | Prior context no longer valid | Reject before compilation |
| Compiler emits foreign scope/task | Shape may be valid | Reject at application boundary |

## Requirement and architecture review

Single-agent requirement, architecture, automation and testing role passes used
baseline `b5433533c49bde2b2e0c7b07f2f6865f53f5c0c8`, #188 (including its cross-plane
additions), parent #177 and implemented #182/#187 contracts. No independent
reviewer approval is claimed. Readiness: READY_FOR_WORKFLOW. Scope is the
application service and component integration; installed runtime wiring remains
#190/#275/#279. Initial implementation/qualification estimate: 1–3 engineer-days,
assuming the existing resolver and host contracts; external host rollout excluded.

| ID | Requirement | Production owner/path | Component evidence |
| --- | --- | --- | --- |
| C1 | Never invent required facts | Validator -> typed gap | Missing/ambiguous/conflicting inputs; compiler not called while blocked |
| C2 | Exhaust deterministic context | #187 resolver and permitted ports | Input, situation, memory, retrieval and alias lookup fixtures |
| C3 | Ask only necessary questions | Dependency-frontier planner | Advisory, blocked child and independent-root cases |
| C4 | Deterministic dependent ordering | Validated DAG, lexical tie break | Reversed manifests and cycle/missing-edge rejection |
| C5 | Provider-independent result | Versioned typed plan/diagnostic sidecar | Canonical bytes and wire metadata assertions |
| C6 | Resume without validation bypass | Same `compile` entrypoint | Real CGSL frontend; expired/changed/denied context and reference failures |
| C7 | Stable task/scope/revision correlation | Authenticated binding and hashed question IDs | Task required; capture/request changes alter IDs |
| C8 | Reply is never consent | Semantic-only return types | No policy or mutation operation; runtime enforcement remains host-owned |

The only new crate edge is application -> CGSL **dev-only**, to exercise the real
frontend in the integration fixture. The architecture guard rejects production
and build edges and has explicit mutation tests for that restriction.

## Verification

```sh
cargo test -p gateway-domain -p gateway-cgsl -p gateway-application --locked
cargo llvm-cov -p gateway-application --test completeness --locked --json --output-path target/completeness-coverage.json
python3 scripts/check-completeness-coverage.py --self-test
python3 scripts/check-completeness-coverage.py target/completeness-coverage.json
python3 scripts/check-dependencies.py
python3 -m unittest discover -s tests/architecture -p test_dependencies.py
cargo fmt --all --check
cargo clippy -p gateway-application --all-targets --locked -- -D warnings
```

The quality runner now requires measured 95% coverage of the new production
module. This is component evidence, not installed-client, persisted interaction
or full parent-epic acceptance. See the source-bound
[evidence record](evidence/EPIC-05.10-completeness.json) for executed outcomes.
