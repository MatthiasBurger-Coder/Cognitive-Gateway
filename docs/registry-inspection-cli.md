# Registry inspection CLI

`cg-registry` is the CG-03 read-only proof and debugging interface for the
Git-owned Agent, Skill and Capability catalog. It loads one deterministic
catalog snapshot through `gateway-registry`; it does not connect to a project,
LLM, retrieval service, runtime or policy engine. It is intentionally separate
from the later CG-11 declarative product CLI.

## Build and install

From the repository root on WSL/Linux, build the workspace and install the
canonical `cg-registry` executable into Cargo's standard user bin directory:

```bash
cargo build --workspace
cargo install --path crates/gateway-daemon --bin cg-registry --locked --force
```

Cargo installs the executable in `$HOME/.cargo/bin`. Ensure that directory is
on `PATH`, then verify that the command is shell-resolvable:

```bash
export PATH="$HOME/.cargo/bin:$PATH"  # only needed if Rustup did not add it
command -v cg-registry
cg-registry --help
```

Run installed commands from the repository root, or provide a catalog root
explicitly with `--catalog <dir>`:

```bash
cg-registry agent list
cg-registry agent show system-architect
cg-registry skill list
cg-registry skill show architecture-hexagonal
cg-registry skill graph architecture-hexagonal
cg-registry capability list
cg-registry capability show architecture.dependency-analysis
cg-registry capability resolve architecture.dependency-analysis
```

For a one-off invocation without installing, Cargo can run the same binary:

```bash
cargo run --bin cg-registry -- agent list
cargo run --bin cg-registry -- --catalog catalog capability resolve architecture.dependency-analysis
```

The catalog root contains the canonical `agents/` and `skills/` directories.
Every command loads and integrity-validates the complete snapshot before
reporting anything. Invalid documents, broken relationships and conflicting
capability declarations fail closed.

## Commands

```text
cg-registry agent list
cg-registry agent show <agent-id>

cg-registry skill list
cg-registry skill show <skill-id>
cg-registry skill graph <skill-id>

cg-registry capability list
cg-registry capability show <capability-id>
cg-registry capability resolve <capability-id>
```

Agent and Skill IDs are canonical catalog IDs. Capability resolution uses the
exact typed capability ID and reports every matching Agent/Skill candidate,
provider relationships, mandatory Skill dependency closures and the matched
capability selector. Multiple providers are reported as an explicit
`ambiguous` outcome while remaining resolvable for inspection; no provider is
silently selected.

`--json` emits deterministic machine-readable output for acceptance tests and
automation. The human and JSON forms are generated from the same report data.
JSON errors use the same output stream and include `code`, `message` and
`exit_code` fields.

Exit codes are stable:

| Code | Meaning |
| ---: | --- |
| 0 | Successful inspection |
| 2 | Invalid command, option or identifier |
| 3 | Catalog loading or integrity failure |
| 4 | Unknown canonical identifier |

The adapter is read-only. It has no mutation command and cannot change catalog
membership, permissions, project state or runtime behavior.
