# Bounded local Codex runtime — EPIC-04.08 #243

The Rust local adapter owns invocation bounds, transport lifecycle and sanitized
observability. Application policy, verification, durable sessions and uncertain
side effects remain owned by the shared facade and admitted host services.

## Configuration and operation

`cg-mcp --diagnostics` prints the default configuration to stderr without opening
a transport or reading credentials. A trusted launcher may pass
`--runtime-limits FILE` alongside the existing binding/admission arguments. The
file is UTF-8 JSON, at most 4096 bytes, with exactly these required fields:

```json
{
  "input_bytes": 1048576,
  "output_bytes": 1048576,
  "requests": 10000,
  "request_timeout_ms": 30000,
  "idle_timeout_ms": 300000,
  "write_timeout_ms": 2000
}
```

Byte ceilings include the newline. Byte limits must be 1024–1048576, the frame
budget 1–10000, and every timeout 1–300000 ms. Unknown fields, invalid values,
missing fields and duplicate fields fail launch. This configuration cannot grant
capabilities or widen scope. Input limits also apply to direct `Server::handle`;
production callers use `serve` for deadline and disconnect handling. Library
transport timeouts are capped by configuration. JSON depth/duplicate-key limits
and credential admission remain enforced.

One application invocation may run per connection. There is no pending
application queue. A concurrent request receives `CG_OVERLOADED`, is counted
against the connection frame budget, and its ID is reserved against later reuse.
The underlying private pipe workers each have a single-slot channel; the reader
may hold one additional bounded frame while its channel is full. OS pipe buffers
provide backpressure. Every inbound frame, including notifications and rejected
frames, consumes the connection budget to bound flood handling. Output encoding
stops at the configured ceiling; a bounded failure replaces oversized output.
No runtime error triggers a retry.

After MCP initialization, `resources/list` advertises `cg://runtime/health`.
`resources/read` exposes adapter health/readiness, configured limits, concurrency,
queue and disconnect behavior, and counters for completed response attempts,
failed attempts, rejections, cancellations and deadlines. Readiness means the
adapter handshake completed; it does not assert that a protected operation's
host, policy, consent or evidence is available. Health cannot reveal other
projects, request bodies, resource contents, filesystem paths or credentials.

## Deadlines, cancellation and disconnect

Each invocation has a generated adapter correlation ID, an absolute monotonic
deadline and a shared cancellation token. `CodexApplicationPort` context methods
preserve this through the facade into `Call.runtime`, including scoped reads and
session host calls. The facade checks the context before policy evaluation and
before session dispatch; existing canonical validation and policy gates remain
mandatory. Nested hosts must reuse this context, check it before later dispatch,
and pass it to their workers. Cloning the context never resets the deadline or
cancellation state. This is the integration seam for aggregate session budgets
in #277; it does not implement or reset durable session budgets.

A valid `notifications/cancelled` naming the active JSON-RPC request cancels that
invocation. Completed or unknown IDs have no effect. Cancellation reasons are
never logged or retained. Cancellation/expiry returns `CG_CANCELLED`/`CG_TIMEOUT`
and closes the connection. Closing stdin, transport failure, output failure or
frame exhaustion also cancels the active invocation and closes the connection.
The adapter does not wait indefinitely for an uncooperative host or blocked pipe.

Disconnect **requests invocation cancellation and detaches task observation**.
It does not issue `session.cancel`, erase a durable task, or claim that an already
dispatched external effect was rolled back. Runtime diagnostics carry
`outcome: unknown_if_dispatched`, `task_observation: detached` and `retry: false`.
The host must preserve durable session/fencing/uncertain-dispatch state and reconcile
through the shared session API. Reconnect creates a new admitted connection and
never restarts a task. A timeout, panic or lost response is not evidence that a
mutation failed or succeeded, nor permission to bypass verification.

Rust cannot forcibly stop a thread safely. A detached library worker may finish
existing work after cancellation; cooperative hosts must stop later dispatch.
The executable exits after `serve` closes, so pipe/host workers cannot hold process
exit. For graceful launcher shutdown, close stdin and allow the configured write
bound. Forced OS termination cannot promise effect rollback or durable completion.

## Diagnostics and observability

Every response emitted by `serve` has an adapter correlation ID in result `_meta`
(`cg/correlation_id`) or error `data.correlation_id`. The original JSON-RPC ID and
validated application correlation remain intact. Generated IDs do not contain
client claims. A `local_call_finished` JSON event on stderr carries only the
adapter ID, elapsed milliseconds and typed failure class; it is the invocation
trace completion event. A `local_transport_closed` event carries a deterministic
transport code and class. Stdout remains protocol-only. The executable installs
a fixed panic diagnostic hook so panic messages cannot echo sensitive host data.

| Failure class | Examples |
| --- | --- |
| Validation | Parse, invalid request/params, unknown method, invalid canonical input |
| Policy | Scope, sensitivity, policy denial, consent or evidence required |
| Application | Unsupported capability, internal worker failure, canonical service failure |
| Runtime | `CG_OVERLOADED`, `CG_LIMIT_EXCEEDED`, `CG_CANCELLED`, `CG_TIMEOUT` |
| Transport | `CG_TRANSPORT_IO`, `CG_TRANSPORT_FRAME`, `CG_TRANSPORT_TIMEOUT` |

Existing frozen tool failure envelopes are unchanged. Adapter JSON-RPC failures
carry deterministic diagnostic codes separately from those envelopes. Logs and
metrics never include request IDs supplied by clients, cancellation reasons,
operation arguments, URIs, principal/scope claims, documents or environment values.

## Requirement and evidence mapping

| Requirement | Implementation | Automated evidence |
| --- | --- | --- |
| Time/size bounds, configuration | `local_mcp/runtime.rs`, `transport.rs`, `cg-mcp.rs` | Limit/configuration, bounded serialization, pipe and deadline tests |
| Cancellation/context propagation | `codex/runtime.rs`, facade context methods, `Call.runtime` | Shared token/deadline and facade policy tests; active cancellation fault |
| Disconnect and uncertainty | Single worker owner, cancellation drop guard, no replay | EOF, write failure, transport fault and panic injection |
| Concurrency/backpressure | One invocation, zero application queue, bounded channels/frame budget | Overload, unknown cancellation, reserved IDs and flood budget tests |
| Failure classes and safe diagnostics | `FailureClass`, fixed codes, response metadata, health resource | Taxonomy/health and credential-injection regressions |
| Policy/verification preserved | Existing facade authorization and session host delegation | `codex_facade`, `codex_isolation` and architecture tests |

Run the checks listed in [local-mcp-server.md](local-mcp-server.md). The coverage
gate includes both new runtime modules at >=95% line coverage. Fault injection
uses local fixture ports and transports; no provider or live Codex account is
required. Live client qualification remains #245.

Verification on 2026-10-10: `cargo test --workspace --locked` passed 777 tests
with three existing optional tests ignored. Workspace Clippy, formatting,
architecture checks (17 tests), contract checks (11 tests), and independent
13-tool protocol/schema conformance passed. The final local MCP coverage gate
passed all twelve required files at >=95%; the application invocation context
measured 100%, adapter runtime 97.33%, and launcher 95.12%. Local reports are
`target/local-mcp-coverage.json`, `target/epic-04-08-coverage.log` and
`target/epic-04-08-workspace-tests.log`. CI retains the same coverage evidence
through the existing release quality manifest. This verification does not claim
live Codex qualification or execution of the separate full release gate.
