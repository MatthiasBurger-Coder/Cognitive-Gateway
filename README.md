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
├── gateway-registry/
├── gateway-workflow/
├── gateway-policy/
├── gateway-context/
└── gateway-daemon/
```

## Build and quality

A Rust toolchain with `rustfmt` and `clippy` is required.

```bash
./scripts/check-architecture.sh
cargo fmt --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

The GitHub Actions workflow `.github/workflows/rust.yml` runs the same quality baseline for pull requests and pushes to `main`.

CG-03 registry and registry-CLI code is also subject to a 95% line-coverage
gate. Run the local coverage checks with `cargo-llvm-cov`:

```bash
cargo llvm-cov -p gateway-registry --all-targets --fail-under-lines 95
cargo llvm-cov -p gateway-daemon --all-targets --fail-under-lines 95
```

### Run the registry CLI from WSL/Linux

Build and install the read-only `cg-registry` CLI from the repository root:

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

## Documentation

The technical architecture is maintained in the repository as the canonical source of truth:

- [`docs/arc42/`](docs/arc42/) — arc42 architecture documentation
- [`docs/adr/`](docs/adr/) — Architecture Decision Records
- [`docs/registry-inspection-cli.md`](docs/registry-inspection-cli.md) — `cg-registry` installation and inspection commands
- [`docs/process-application-api.md`](docs/process-application-api.md) — Rust process application ports, simulation and explainability
- [`docs/declarative-planning.md`](docs/declarative-planning.md) — CG-07 declarative planning IR and capability requirements
- [`docs/plan-graph.md`](docs/plan-graph.md) — CG-07.06 Plan DAG, deterministic order and verification semantics
- [`docs/deterministic-planner.md`](docs/deterministic-planner.md) — CG-07.07 deterministic rule-based planner and fail-closed diagnostics
- [`docs/plan-validation.md`](docs/plan-validation.md) — CG-07.08 plan validation, canonical serialization and explainability

The GitHub Wiki is intended for simplified end-user documentation, tutorials and usage guidance. If Wiki content and repository architecture documentation ever conflict, the repository documentation is authoritative.

## Current v0.1 Direction

- Rust for the deterministic gateway core and long-running daemon
- Python for optional cognitive services such as local SLMs, embeddings and RAG
- Kotlin only if a dedicated IntelliJ integration becomes necessary
- deterministic workflow/agent/skill resolution before probabilistic retrieval
- Git as source of truth
- RAG as knowledge retrieval, not authority
- consuming-project configuration as request-scoped application input, never
  as catalog membership or execution authority
- MCP/tool adapters as controlled capabilities
- execution runtimes remain replaceable

See EPIC #1 and the CG-01…CG-10 issues for the v0.1 implementation plan.
