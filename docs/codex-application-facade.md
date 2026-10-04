# Codex-facing application facade

EPIC-04.04 #239 implements `gateway_application::codex::CodexFacade` and the
`CodexApplicationPort` driving port. The MCP adapter calls this port once per
recognized tool call; its transport contains no normalization, planning,
resolution, policy evaluation or session coordinator.

The facade validates the frozen v1 envelope, exact tool/operation pairing and
explicit workspace/project/binding scope and trusted client session before dispatch. Requests are bounded
by size, node count and depth. OperatingMode and ExecutionProfile use the CG-02
Rust parsers and remain requested execution depth, never authority. The trusted
host explicitly maps the admitted scope to a canonical ContextScopeId.

| Operation | Canonical owner | Host dependency |
| --- | --- | --- |
| `situation.inspect` | `DeclarativeContextSituationDocument::from_json`, `DeclarativeSituationApplication::inspect_situation` | Inline document or admitted immutable reference |
| `situation.assess` | Shared `AssessmentInput::assess` mapper, existing normalization and situation assembly | Canonical assembly document/reference; scope and CG-02 values must agree |
| `capabilities.resolve` | `DeclarativeResolutionApplication::resolve_plan`, `ResolutionSnapshot::capture` | Typed snapshot and CompositionRules from pinned plan/rules/process records |
| `state.explain` | Existing resolution validation, canonical serialization and `explain_resolution` | Reconstruction of the pinned ResolvedPlan |
| `context.compile` | `ContextApplication::compile_step` | Typed compilation command with current PolicyAuthority, PolicyContext, catalog, projection and disclosure policy |
| `registry.inspect` | `Registry::capability_index`, existing Agent/Skill/Process registries | Trusted catalogs; filtering preserves their canonical order and definition versions/digests |
| `evidence.inspect` | `ObservationEvidenceSet` validated serialization contract | Inspection of existing admitted evidence; no new evidence repository |
| `session.*` | `CodexHost::session` reserved for shared #272/#273/#275 services | Unsupported by default; no facade lifecycle or consent authority |

`CodexHost` is a trusted outbound port. It admits operations, resolves records,
maps records to canonical commands and applies current disclosure policy.
Implementations must parse exact supported Rust payload contracts rather than
substitute unrelated commands. Its default methods fail closed. Caller/model
content cannot construct host policy, consent or capability grants.

Reference lookup receives the explicit admitted scope. Returned scope and all
reference fields must match, including contract version, immutable revision and
digest. The facade independently verifies SHA-256 over the record's canonical
UTF-8 JSON bytes. Resolution and compilation use existing snapshot validation,
including process definition identity/digest and instance revision. Explanation
and compilation also compare the referenced resolution with its validated
canonical serialization before use.

Canonical results pass through the host's disclosure projection. Projection
returns a canonical document or immutable reference with admitted explanation,
evidence and provenance links; the facade retains those arrays unchanged and
validates the complete response against the frozen schema. A document with
SECRET provenance is refused. Reference ingress also verifies the trusted client session and carries complete admitted source provenance through projection. Unclassified content must be refused by the host;
the facade supplies no permissive disclosure fallback. Diagnostics are fixed
code/message/retry triples; rejected data and exception text are never echoed.

The CLI assessment path uses the same mapper, preserving its existing output
wrapper and eliminating a second normalization implementation.

## Wiring and availability

A trusted runtime installs the facade with `Server::with_application` after
constructing it with the launch scope, canonical scope and admitted host. Tests
exercise this path with a deterministic transport and real canonical services.

The standalone `cg-mcp` launcher supports the [explicit workspace admission](codex-scope-isolation.md) from #240. Its immutable local host admits situation queries and scoped resource reads. A discovery-only launch retains `UnavailableHost`. Live Codex qualification remains #245. Shared
session services are absent in this checkout, so all six session operations
remain unsupported rather than creating an adapter-owned task lifecycle.

Validation:

```sh
cargo test -p gateway-application --test codex_facade --test context_application
cargo test -p gateway-daemon --lib --test local_mcp --test declarative_cli
cargo clippy --workspace --all-targets -- -D warnings
python3 -m unittest discover -s tests/contracts
python3 -m unittest discover -s tests/architecture
python3 scripts/check-local-mcp-protocol.py
```
