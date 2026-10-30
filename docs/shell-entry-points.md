# Shell entry points

Shell scripts orchestrate existing Cargo, Docker Compose and service commands.
Gateway setup and admission are implemented in the Rust infrastructure adapter
`cg-local --setup`; shell scripts do not implement gateway policy or application logic.
Python checks and measurement helpers belong to [`tests/scripts/`](../tests/scripts/).

Use `--help` on any shell entry point to see its arguments, prerequisites and effects
without installing anything, starting containers or running tests. Operation scripts
resolve paths from their own location and can be called from another working directory.

## Installation

| Entry point | Responsibility | Effects |
| --- | --- | --- |
| [`install/linux-cli.sh`](install/linux-cli.sh) | Build/install `cg` and `cg-registry` on Linux; optional `--root ABSOLUTE_DIR` | Cargo may download dependencies; replaces these executables in `ROOT/bin`. Does not start services or edit PATH. |

Installation and service startup are separate commands. The former
`install-linux.sh --with-postgres` combination is replaced by the installer followed
by `operations/postgres/start.sh`.

## Service operations

| Entry point | Responsibility | Effects |
| --- | --- | --- |
| [`operations/postgres/start.sh`](operations/postgres/start.sh) | Prepare private PostgreSQL configuration if absent; start and wait for the persistent service | Creates the credential file if needed, sets mode 600, starts containers. Preserves existing credentials and data. |
| [`operations/postgres/compose.sh`](operations/postgres/compose.sh) | Forward Compose commands using the explicit PostgreSQL credential file | Depends on the command. `down` retains volumes; `down -v` deletes them. Never generates credentials. |
| [`operations/local-model/compose.sh`](operations/local-model/compose.sh) | Select the acceleration overlay and forward Compose arguments | Depends on the requested Compose command. No model lifecycle decisions. |
| [`operations/local-model/service.sh`](operations/local-model/service.sh) | `config`, `start`, `stop`, `logs`, `ready` for the local model stack | Starts/stops containers, reads logs/health or renders configuration. Stop retains volumes. |
| [`operations/local-model/models.sh`](operations/local-model/models.sh) | `install`, `test`, `promote`, `rollback`, `disable`, `inspect` against the running model service | Delegates model download/pinned snapshot registration, inference qualification and release/alias changes to the service. Profile paths refer to the container filesystem. Never starts the stack. |

PostgreSQL uses `CG_POSTGRES_ENV_FILE`, otherwise
`${XDG_CONFIG_HOME:-$HOME/.config}/cognitive-gateway/postgres.env`.
Model Compose uses `CG_MODEL_ACCELERATION=cpu|nvidia|gpu|amd|vulkan`; CPU is the default.
Service health uses `CG_MODEL_SERVICE_PORT` (default 8091).
Other service settings remain in the Compose files and their existing operator guides:
[PostgreSQL](../docs/postgres-compose.md), [local models](../docs/local-model-runtime.md).

## Test runners

These remain under `tests/scripts/`. Run them from the checkout root; each supports
`--help`. Qualification runners require exactly one evidence directory. They reset
coverage build state and write reports; reusing a directory can overwrite evidence.
Native runtime qualification (shared sessions, observed goals, Before journal and
atomic Before) pins both Cargo dev and test optimization to level 1 for one
consistent instrumented test/CLI profile. These process-local settings do not
change `Cargo.toml` or installed binaries.
PostgreSQL qualification uses the disposable host supplied by
`python3 tests/scripts/cognitive-test-host.py -- bash RUNNER EVIDENCE_DIR`.

| Runner | Responsibility |
| --- | --- |
| [`with-coverage-env.sh`](../tests/scripts/with-coverage-env.sh) | Apply coverage compiler flags to the current shell when sourced, or execute a supplied command with those flags. |
| [`check-architecture.sh`](../tests/scripts/check-architecture.sh) | Check Cargo dependency direction and catalog/project boundaries. Starts no services. |
| [`qualify-shared-sessions.sh`](../tests/scripts/qualify-shared-sessions.sh) | Shared-session runtime, persistence and CLI/MCP parity. |
| [`qualify-observed-goal-sessions.sh`](../tests/scripts/qualify-observed-goal-sessions.sh) | Observed-goal sessions with PostgreSQL and coverage evidence. |
| [`qualify-complete-snapshots.sh`](../tests/scripts/qualify-complete-snapshots.sh) | Complete scoped snapshot data/contracts; no installed MCP/PostgreSQL proof. |
| [`qualify-before-journal.sh`](../tests/scripts/qualify-before-journal.sh) | Before journal persistence and historical inspection. |
| [`qualify-atomic-before.sh`](../tests/scripts/qualify-atomic-before.sh) | Native atomic Before capture with MCP and PostgreSQL. |
| [`test-postgres.sh`](../tests/scripts/test-postgres.sh) | Start the persistent operator database and run PostgreSQL integration tests. This is separate from disposable qualification and writes test data. |

Usage errors exit 2. Other failures propagate the invoked tool's status; setup
failures can exit 1. Help exits 0. Historical reports under `docs/evidence/` retain
the paths and commands recorded when they were produced.

Rust coverage builds use `with-coverage-env.sh` to retain unreferenced
function bodies with `-C link-dead-code`. This keeps LLVM's combined report from
hiding an executed
accessor behind an unused coverage stub. Existing compiler flags are preserved,
including Cargo's encoded-flags precedence. This affects instrumented builds
only; test selections and coverage requirements stay fixed.

Native runners with several Cargo invocations source the shared environment;
single-build runners and manifest commands execute Cargo through the wrapper.
This keeps the measurement policy in one place across all Rust coverage gates.

The full quality runner uses one coverage build cache and cleans generated
workspace artifacts after each successful gate. JSON reports, logs and hashes
stay in the evidence bundle. A failed gate stops before cleanup, preserving its
build and profiling data for diagnosis. Standalone runners still accept an
explicit `CARGO_LLVM_COV_TARGET_DIR`.
