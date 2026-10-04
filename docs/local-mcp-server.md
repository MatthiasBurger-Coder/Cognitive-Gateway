# Local MCP server adapter

EPIC-04.03 #238 implements the inbound adapter in `gateway-daemon::local_mcp`
and the `cg-mcp` executable. No Cargo dependency or inner-crate dependency edge
is added. This path requires no model, provider SDK, OpenAI API key, or Codex
credential. EPIC-07 outbound connectors remain separate.

Install from the checkout:

```sh
cargo install --path crates/gateway-daemon --bin cg-mcp --locked --force
```

A trusted launcher supplies exact client name/version, local principal and opaque
workspace/project/binding IDs. All six arguments are mandatory, unique and
bounded ASCII tokens. There is no ambient cwd, environment, filesystem or account
credential fallback. Claims in `initialize` must match the configured client.
Private pipe ownership and executable integrity are operator responsibilities;
a client name is not proof of vendor identity. CG-owned scope records, client session binding and filesystem admission are implemented by [#240 workspace admission](codex-scope-isolation.md).

For a provider-free protocol smoke test from the repository root:

```sh
cargo build --bin cg-mcp --locked
env -i target/debug/cg-mcp \
  --client-name codex --client-version 1.0 --principal operator \
  --workspace workspace-example --project project-example --binding binding-example \
  < tests/fixtures/local-mcp/lifecycle.jsonl
```

The fixture's client version is illustrative. An actual client launch must pin
its supported name/version rather than reuse the fixture identity. The launcher
must pass an explicit environment allowlist excluding provider credentials;
`env -i` demonstrates an empty environment. The adapter does not read credential
variables, auth stores, config files or home directories.

## Surface and application availability

Initialization supports exactly MCP `2025-11-25`; a mismatch or failed client
admission returns a sanitized JSON-RPC error and closes the connection. The
server requires `notifications/initialized` before tool/resource access. Ping is
available during initialization. Capabilities advertise only tools and resources.
There are no tasks, subscriptions, sampling, external fetches or automatic retries.

Discovery publishes all 13 frozen tool contracts, their annotations and complete
operation-constrained input/output schemas. Shared `$defs` and references are
bundled offline. Six static contract artifacts can be read. The catalog retains
its immutable `defined`/`unsupported` contract markers; tool descriptions and
initialization instructions state current application availability.

The [shared application facade](codex-application-facade.md) (#239) is implemented
and injectable through `Server::with_application`. The standalone launcher accepts an explicit #240 admission file, cwd, repository and client session; its local host admits situation queries and scoped resource reads. A discovery-only launch uses the unavailable host and returns `CG_UNSUPPORTED_CAPABILITY`. Session commands delegate
only to shared application services when installed. Scoped resources require the admitted workspace host. No canonical use case is bypassed or reimplemented.

Recognized tool calls validate envelope version, operation/name agreement,
strict frozen request shape and exact launch scope in that order. Results carry
identical structured and serialized envelopes. Errors use frozen diagnostic
triples without rejected payloads. The validator implements only the keywords
and two patterns in the frozen request/common schemas; it is not a general
schema engine and cannot authorize or validate canonical domain documents.

## Transport, correlation and termination

`Transport` isolates receive/send from protocol lifecycle and can be replaced
by a fixture transport. `StdioTransport` implements newline-delimited UTF-8 JSON
using bounded worker channels. Input and output frames have a 1 MiB ceiling,
JSON nesting is limited to 64, and duplicate keys are rejected at every level.
Stdout carries protocol frames; stderr carries fixed sanitized diagnostics.

JSON-RPC IDs are correlated exactly, restricted to bounded strings or safe
integers, and unique for a connection. At most 10,000 requests are processed.
The executable uses a five-minute input inactivity/partial-frame deadline and a
two-second output deadline. Worker threads are detached so a blocked OS pipe
cannot hold process termination; alternative library transports must honor the
supplied deadlines. These bounds are an initial adapter baseline; extended
execution and uncertain mutation outcomes remain #243.

All current requests finish inline. Cancellation notifications for completed or
unknown request IDs are ignored without retaining reasons, cancelling CG task
sessions or granting authority. There is no in-flight application work in this
slice. Closing stdin signals shutdown; EOF, framing/transport failure, handshake
failure, limits and timeout close the session. There is no invented MCP shutdown
method. Future asynchronous application dispatch requires bounded cancellation
at the shared facade boundary before being exposed.

## Evidence

```sh
cargo test -p gateway-daemon --lib local_mcp --locked
cargo test -p gateway-daemon --test local_mcp --locked
cargo clippy -p gateway-daemon --all-targets --locked -- -D warnings
python3 -m unittest discover -s tests/contracts
python3 scripts/check-local-mcp-protocol.py
bash scripts/check-architecture.sh
python3 -m unittest discover -s tests/architecture
cargo llvm-cov -p gateway-application -p gateway-daemon --lib --bin cg-mcp \
  --test local_mcp --test codex_facade --test codex_isolation \
  --json --output-path target/local-mcp-coverage.json
python3 scripts/check-local-mcp-coverage.py target/local-mcp-coverage.json
```

The conformance fixture exercises initialization, notification gating, resource
and tool discovery, static read, contracted unsupported invocation, cancellation,
ping and EOF in a real subprocess with an empty environment. Tests also cover
version/client rejection, malformed/duplicate/deep JSON, task rejection, scope
mismatch, ID reuse, framing limits, blocked pipes and sanitized transport failure.
Independent Draft 2020-12 validation also checks the live discovery schemas,
all frozen requests and every runtime failure envelope. This conformance check
and the per-file 95% coverage gate are included in the release quality manifest.
EOF exits without a model/provider or key; live Codex application qualification
remains #245 after the facade and admission slices are integrated.
