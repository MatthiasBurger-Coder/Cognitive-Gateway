# Installed Codex interoperability — EPIC-04.14

The actual installed `codex-cli 0.162.1` has discovered the delivered local CG
tools and invoked inspection, resolution, explanation and context compilation.
Observed MCP identity is `codex-mcp-client / 0.162.1`; its requested protocol is
`2025-06-18`. CG accepts that explicit protocol alongside `2025-11-25`. The client
also sends object-valued `_meta` in discovery; this metadata carries no authority.

## Reproduce against the candidate

```sh
cargo build -p gateway-daemon --bin cg --bin cg-mcp --bin cg-local --locked
python3 scripts/qualify-installed-codex.py --canonical-fixture \
  --output target/installed-codex-canonical
```

The output directory must be new. Omit `--canonical-fixture` for inspection-only
qualification; `--codex` and `--bin-dir` can pin other installed executables.
The runner uses the real Codex app-server and its MCP tool-call API, isolated
HOME/CODEX_HOME settings, an ephemeral read-only thread, and empty CG environments.
It starts no inference turn, reads no user Codex configuration and transfers no
provider authentication. A forwarding observer records only initialize identity
and discovery field shapes, and forwards MCP frames unchanged to the real host.
The trusted launch pins the expected identity before observing it.

The app-server interface is documented in the
[official Codex documentation](https://learn.chatgpt.com/docs/app-server).
The installed version's generated JSON schema was inspected before use;
versions without the required interface must fail qualification rather than
being represented by a simulated client.

The canonical option uses neutral admitted fixtures from
`tests/codex-local/test_canonical_host.py`, including a temporary catalog and
strict plan/rules/process/policy/projection snapshots. These substitute operator
inputs; the client, transport, composition root and Rust services are real.
Responses are compared as complete envelopes with `cg-local`, and scope and stale
resolution refusals are required. The fixture's artificial desired state is not
a verified external project outcome.

## Evidence and boundary

`report.json`, `mcp-initialize.json` and `rpc-evidence.json` retain client
version/identity/protocol, discovered tools, actual calls/results, CLI parity,
scope refusals, source and executable hashes and explicit substitutions.
Successful status is `QUALIFIED_INSTALLED_CANONICAL` or
`QUALIFIED_INSTALLED_INSPECTION` for the narrower mode. Missing prerequisites
produce NOT_RUN or FAIL with a nonzero exit code.

Both successful modes retain `epic_04_status: NOT_COMPLETE`: they do not provide
the absent shared session coordinator, clarification/consent lifecycle or durable
recovery. Full EPIC-04.14 acceptance requires installed-client invocation of those
services after their implementation; EPIC-04 and #295 remain open until then.
EPIC-08 model/connector system qualification is a separate claim.
