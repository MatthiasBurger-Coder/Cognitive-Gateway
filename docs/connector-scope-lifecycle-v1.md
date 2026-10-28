# Scoped connector admission and lifecycle (#232)

The application `connector_admission` module owns an immutable operator allowlist
and its derived `ConnectorRegistry`. The daemon `mcp_client::plugins` module
composes private MCP hosts and implements `ConnectorApplicationPort` through
`PluginLifecycleManager`. Neither discovery nor consuming-project input can
insert a registration or modify global authority.

## Canonical configuration and scope

A `ConnectorRegistration` pins a validated immutable manifest, an enabled flag
and a `ConnectorScopeBinding`: project, CG-06 `ContextScopeId`, optional session
and optional request. Bindings come from the authenticated composition root.
`None` matches only `None`; there are no wildcard, parent-scope or global fallback
permissions. The host must authenticate the binding independently of the request.
The same scope ID cannot belong to different bindings, even when connector IDs
differ. Multiple connectors may participate in one exact binding.

`ConnectorRegistry::rebuild` validates the complete canonical snapshot before
publishing its sorted index. Duplicate connector identities, differing definitions
for one identity, and conflicting server identities within one scope are rejected.
Gateway/transport compatibility is checked using the existing manifest contract.
Disabled manifests can appear only in disabled registrations. Manifest parsing
continues to enforce protocol, schema/port revision and content digest.

Reuse of a server in another project requires a separate explicit registration
and a separate runtime/session. This grants no permission to copy observations,
credentials, inventories or cached data between scopes. Cross-scope data transfer
has no implicit admission route; CG-06 scope validation remains mandatory.

`bind` narrows an existing CG-09 admission source to one registration. It never
replaces policy, consent, source, task or aggregate budget checks. The scoped
admission checks its state both before and after trusted policy callbacks.
Revocation prevents even an already captured admission handle from reopening.
Diagnostics contain only fixed codes and existing correlation identifiers.

## Runtime lifecycle

`PluginLifecycleManager::new` accepts the registry, one authenticated binding and
operator-owned `PluginConfig` adapters. Each canonical enabled registration in
that binding needs exactly one matching manifest/target runtime definition.
Unknown, duplicate, missing or mismatched adapters reject construction before
opening any transport. Other bindings and disabled registrations do not acquire
runtime handles. `index()` returns the canonical registrations for this exact
binding, including configured disabled entries, in deterministic identity order.
It describes configuration, not live connection health.

- Connect, discover, invoke, resource read and health use current admission.
- Disable closes admission first, cancels in-flight work and clears the host
  inventory/connection. Repeated disable is deterministic.
- Shutdown also closes admission; it grants no reconnect permission.
- Enable is an explicit operator lifecycle operation. It shuts down the old host
  state before reopening admission. Connection and discovery are then required
  before invocation; an old request alone cannot restore an inventory.
- Remove withdraws the runtime entry and permanently revokes that generation.
  In-flight effects retain their uncertainty and are never automatically replayed.
- Rebuild validates the entire replacement first. Invalid configuration preserves
  the current manager. A valid replacement revokes every old admission before
  bounded transport cleanup and starts with fresh connections/inventories.
- Drop revokes all entries before attempting cleanup. Cleanup errors cannot
  restore admission; explicit cleanup methods return failures to their caller.

Runtime disable/removal are local administrative state. To persist them across
restart, change canonical enabled/configuration membership and rebuild. Rebuilding
from unchanged canonical configuration deliberately restores its configured state.
There is no runtime registration database to become competing authority.

Disappearance and degradation use the existing MCP failure/health machinery
(#231): discovery cannot publish a partial inventory, failed connections invalidate
cached inventory, and recovery requires current admission. No failure adds a
connector or copies state from another scope.

## Delivered path

The installed `cg-mcp` observed-goal composition root builds the same registry
from its pinned operator-owned runtime/manifest definitions. It binds the durable
checkpoint project, session and run (the request identity), then wraps its existing
CG-09/claim source before constructing the qualified scalar host. This path keeps
the existing session budget, owner and source-response validations. It uses its
session-owned lifecycle rather than introducing a second coordinator.

Plain MCP hosts also poll target admission during transport checks and after
normalization; this is no longer conditional on a session-budget preflight.
Tool-schema lookup checks current admission before and after mapping. Revoked
or removed providers cannot release results or metadata through stale handles.

## Requirement review and evidence

Inspected base revision: `d37610497c0fafd068238f428066da3f4f85db87`;
implementation and qualification apply to the working-tree changes for #232.

Review provenance: one agent performed requirement, architecture, automation and
test role passes. Governance: DEVELOPMENT/FULL_PATH; ADR-022 and the existing
connector contracts retain ownership. Initial decision: READY_FOR_WORKFLOW.
The changes reuse manifest, CG-09, MCP lifecycle and CG-06 boundaries without
new persistence, dependencies or provider-specific core behavior. Rollback is a
code/configuration rollback followed by a fresh runtime build.

| ID | Requirement/source | Production owner/path | Dependency | Automated evidence | Level | Status |
| --- | --- | --- | --- | --- | --- | --- |
| A1 | #232 explicit admission | Registry rebuild/bind; manager constructor | Validated manifest/profile | Unknown, disabled, missing, duplicate and mismatched runtime tests | COMPONENT | VERIFIED |
| A2 | Project/request/session isolation | Exact scope binding; installed observed-goal composition | Authenticated host/checkpoint; CG-06 | Cross-project, session/request substitution and scope-collision denials | COMPONENT | VERIFIED |
| A3 | Rebuildable state | Immutable sorted registry; manager rebuild | Canonical operator configuration | Permuted config equality, invalid replacement atomicity, fresh inventory required | COMPONENT / EXECUTABLE | VERIFIED |
| A4 | Deterministic disable/removal | Scoped admission and private hosts | Existing bounded shutdown/cancellation | Disable/enable/remove/re-add, revoked callbacks, in-flight uncertain mutation | EXECUTABLE (independent stdio fixture) | VERIFIED |
| A5 | Identity/compatibility conflicts | Atomic registry validation | Existing manifest validator | Duplicate connector/server, incompatible transport/gateway, disabled manifest tests | COMPONENT | VERIFIED |
| A6 | Project config cannot mutate authority | Immutable registry; operator-only composition | Existing canonical definition capture | Foreign binding and discovery denials; no project write API | COMPONENT | VERIFIED |

Run focused qualification with:

```bash
cargo test -p gateway-application --test connector_admission --locked
python3 scripts/cognitive-test-host.py -- cargo test -p gateway-daemon --lib --test mcp_client --test observed_goal_sessions --test complete_before_sessions --locked -- --test-threads=1
CARGO_BUILD_JOBS=1 CARGO_LLVM_COV_TARGET_DIR=target/mcp-client-coverage python3 scripts/cognitive-test-host.py -- cargo llvm-cov -p gateway-application -p gateway-daemon --lib --test connector_admission --test mcp_client --test observed_goal_sessions --test complete_before_sessions --locked --json --output-path target/connector-admission-coverage.json -- --test-threads=1
python3 scripts/check-connector-admission-coverage.py target/connector-admission-coverage.json
```

The quality gate requires at least 95% measured line coverage for each of the
application admission module, lifecycle manager, changed MCP host and installed
observed-goal composition root. The CI slice requires an actual database host;
it cannot silently qualify skipped integration tests. The existing composition-root
fixture point is conservatively charged as uncovered, preserving its prior gate. The tests
use an independent Python MCP peer, not a live provider. Full parent #223 live
provider/installed-client qualification remains owned by its acceptance suite;
component evidence here does not claim that broader parent is complete.

Qualification completed on 2026-10-28: [retained source-bound evidence](evidence/EPIC-07.09-scope-lifecycle.json) records 283 Rust tests in the comprehensive instrumented run, actual PostgreSQL/installed-binary scenarios, and all four per-file coverage gates above 95%. The new admission module reaches 100%. Architecture, contract, Clippy and formatting checks also pass. This qualifies #232 for review; it does not qualify the complete parent #223.
