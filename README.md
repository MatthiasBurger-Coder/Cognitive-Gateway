# Cognitive Gateway

Cognitive Gateway is a local, model-independent **AI Context & Agent Control Plane** between clients such as IDEs, CLI tools and CI/CD systems and one or more execution runtimes such as Codex, PraisonAI, local LLMs or cloud models.

The gateway is not another agent framework and not merely a RAG system. Its responsibility is to determine which workflows, agents, skills, policies, knowledge and capabilities are relevant and allowed for a task, then compile a minimal execution context for the selected runtime.

> **Authority defines the boundaries. State describes the situation. Cognitive routing determines what is needed. Retrieval supplies knowledge. MCP/tools supply capabilities. The execution runtime acts inside those boundaries.**

## Architecture

The deterministic Rust core follows **Hexagonal Architecture / Ports & Adapters**. Dependencies point inward toward domain and application abstractions. RAG, MCP/tool integrations and execution runtimes attach through ports and remain replaceable adapters.

Initial workspace:

```text
crates/
├── gateway-domain/
├── gateway-application/
├── gateway-process/
├── gateway-registry/
├── gateway-workflow/
├── gateway-policy/
├── gateway-context/
└── gateway-daemon/
```

## Installation on Linux

With Rust and Cargo installed, run the installer from the repository root:

```bash
./scripts/install-linux.sh
export PATH="$HOME/.cargo/bin:$PATH"
cg --help
cg-registry --help
```

The script installs `cg` and `cg-registry` from this checkout. By default,
it uses `CARGO_INSTALL_ROOT`, `CARGO_HOME`, or `$HOME/.cargo` as the install
root, in that order. The executables are placed in the root's `bin` directory.
The `PATH` command above applies to the `$HOME/.cargo` default.
For a custom location, run `./scripts/install-linux.sh --root /absolute/path`
and add `/absolute/path/bin` to `PATH`. Rerunning the script updates both CLIs
to the current checkout. The script can also be called from another directory
by its absolute path.

### Optional PostgreSQL service

To install the CLIs and start PostgreSQL with persistent storage, use:

```bash
./scripts/install-linux.sh --with-postgres
```

For an existing CLI installation, start only the database with
`./scripts/start-postgres.sh`. The first start creates a private
`~/.config/cognitive-gateway/postgres.env` with a random password. Compose
stores database files in a named Docker volume, which survives container
recreation and `./scripts/postgres-compose.sh down`. The service
listens on `127.0.0.1:55432` by default. See
[`docs/postgres-compose.md`](docs/postgres-compose.md) for configuration,
Nexus image override, checks and backup instructions.

The CG-22 PostgreSQL adapters persist governed memory and verified-execution
references; `./scripts/test-postgres.sh` exercises restart-safe revalidation
against this service. `cg patterns --scope <project-scope> --json` inspects the
stored experience. Verified outcomes are written through the Rust adapter.
See the adapter boundary in
[`docs/experience-patterns.md`](docs/experience-patterns.md).

### Run the registry CLI

To install only the read-only `cg-registry` CLI manually, run from the repository root:

```bash
cargo build --workspace
cargo install --path crates/gateway-daemon --bin cg-registry --locked --force
```

Cargo installs binaries into `$HOME/.cargo/bin`. Rustup normally adds that
directory to `PATH`; if it is not already present, add it for the current
shell before verifying the installation:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
command -v cg-registry
cg-registry --help
```

Run the installed CLI from the repository root so the default `catalog`
directory is found. Use `--catalog <dir>` when running it elsewhere:

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

The same commands can be run without installation through Cargo with
`cargo run --bin cg-registry -- <command>`, but the installation flow above
makes `cg-registry` directly resolvable from the shell.

## Build and quality

Run the complete declarative v0.1 gate from the repository root:

```bash
python3 scripts/quality-gate.py
```

Requires Bash, Python 3.11+, Git, Rust with `rustfmt`, `clippy` and
`llvm-tools-preview`, and `cargo-llvm-cov`. The command runs every workspace
and architecture test, the CLI installation/replay checks and all established
95% coverage gates. It retains logs, coverage reports, external-project proof
and a machine-readable summary under `target/release-evidence/`.

[Quality gates and the release checklist](docs/declarative-quality-gates.md)
describe evidence review and focused commands. The `Rust Quality` GitHub
Actions workflow runs the same command and uploads evidence on success or
failure. A green gate qualifies the tested revision for release review.

## Documentation

The technical architecture is maintained in the repository as the canonical source of truth.

**Start with [`docs/current-architecture-state.md`](docs/current-architecture-state.md)** for the dated implemented-vs-planned status map. This is important because the target architecture now contains planned CGSL and MCP connector boundaries alongside an implemented optional local model reference service.

Canonical entry points:

- [`docs/arc42/`](docs/arc42/) — arc42 architecture documentation
- [`docs/README.md`](docs/README.md) — complete technical documentation index
- [`docs/current-architecture-state.md`](docs/current-architecture-state.md) — current implementation/plan status matrix
- [`docs/semantic-language-and-interpretation.md`](docs/semantic-language-and-interpretation.md) — planned CGSL / SemanticTaskIR boundary
- [`docs/codex-local-integration.md`](docs/codex-local-integration.md) — Codex -> CG local no-key MCP trust contract
- [`docs/local-mcp-server.md`](docs/local-mcp-server.md) — implemented stdio MCP lifecycle/discovery; application dispatch pending
- [`docs/mcp-connector-runtime.md`](docs/mcp-connector-runtime.md) — planned CG -> external MCP connector/plugin runtime
- [`docs/local-model-runtime.md`](docs/local-model-runtime.md) — optional containerized local model service, CPU qualification and model lifecycle
- [`docs/offline-learning.md`](docs/offline-learning.md) — CG-28 governed signals, offline dataset/training interfaces and model release/rollback.
- [`docs/learned-procedures.md`](docs/learned-procedures.md) — implemented CG-21 learning-domain foundation
- [`docs/procedure-evaluation.md`](docs/procedure-evaluation.md) — CG-23 validation, replay, simulation and evaluation evidence
- [`docs/procedure-promotion.md`](docs/procedure-promotion.md) — CG-24 registry, promotion, bounded canary, supersession and rollback
- [`docs/adr/`](docs/adr/) — Architecture Decision Records
- [`docs/registry-inspection-cli.md`](docs/registry-inspection-cli.md) — `cg-registry` installation and inspection commands
- [`docs/process-application-api.md`](docs/process-application-api.md) — Rust process application ports, simulation and explainability
- [`docs/declarative-planning.md`](docs/declarative-planning.md) — CG-07 declarative planning IR and capability requirements
- [`docs/plan-graph.md`](docs/plan-graph.md) — CG-07.06 Plan DAG, deterministic order and verification semantics
- [`docs/deterministic-planner.md`](docs/deterministic-planner.md) — CG-07.07 deterministic rule-based planner and fail-closed diagnostics
- [`docs/plan-validation.md`](docs/plan-validation.md) — CG-07.08 plan validation, canonical serialization and explainability
- [`docs/planning-application.md`](docs/planning-application.md) — CG-07.09 declarative planning application APIs and snapshot boundaries
- [`docs/planning-end-to-end.md`](docs/planning-end-to-end.md) — CG-07.10 end-to-end planning acceptance proof and boundary matrix

The GitHub Wiki is intended for simplified end-user documentation, tutorials and usage guidance. If Wiki content and repository architecture documentation ever conflict, the repository documentation is authoritative.

## Current product direction

- Rust for the deterministic gateway core and long-running daemon
- optional cognitive services behind stable ports; local SLM/LxM inference is available as a separately deployable, replaceable reference service with CPU baseline and explicit model qualification
- Kotlin only if a dedicated IntelliJ integration becomes necessary
- deterministic workflow/agent/skill resolution before probabilistic retrieval
- Git as source of truth
- RAG as knowledge retrieval, not authority
- consuming-project configuration as request-scoped application input, never
  as catalog membership or execution authority
- MCP/tool adapters as controlled capabilities; EPIC-07 defines the external connector/plugin runtime while EPIC-04 separately defines Codex as an inbound local client
- execution runtimes remain replaceable
- CGSL/SemanticTaskIR is the planned formal boundary from natural language into deterministic task semantics
- learned procedures are governed, immutable/versioned artifacts and never create policy authority

See [`docs/current-architecture-state.md`](docs/current-architecture-state.md) for the authoritative dated status and the relevant Epic/issue anchors.

### Policy authorization (CG-09)

The deterministic Policy Engine evaluates resolved plan steps against canonical
capability contracts, governance, explicit authorization, consent and evidence.
It preserves Inspect/Mutate separation and feeds decisions into Process Engine
gates. See [Policy Engine](docs/policy-engine.md) for the API and trust boundary.

### Context compilation (CG-10)

The [Context Compiler](docs/context-compiler.md) assembles one authorized plan
step into a typed semantic context and the existing ExecutionContextIR. It
preserves source/trust metadata, minimizes selected context and keeps original
input separate from gateway-generated material.

## Declarative CLI

Use `cg assess`, `cg plan`, `cg resolve`, `cg explain` and `cg compile` to drive
the deterministic application APIs with structured external context. The
Linux installation script above installs `cg`; a manual alternative is
`cargo install --path crates/gateway-daemon --bin cg --locked`.
See the [CLI contracts and runnable walkthrough](docs/declarative-cli.md).

The [CG-12 external project proof](docs/declarative-end-to-end.md) carries
architecture and coverage evidence through planning, resolution, authorization
and compilation, with an exportable CLI replay.

## Closed-loop execution (CG-14)

The [closed-loop application API](docs/closed-loop-execution.md) dispatches
authorized steps through a replaceable runtime port, reassesses observed
evidence, and continues or replans within explicit iteration and retry limits.

The optional local model service can be started with `scripts/model.sh start`;
see [the operator guide](docs/local-model-runtime.md) for installation, qualification,
promotion, rollback and the independent Gateway container.
