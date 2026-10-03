# Declarative CLI (CG-11)

`cg` is a local driving adapter over the CG-06 through CG-10 application APIs.
It normalizes explicit external observations, assesses the Situation, derives a
Delta and Plan, resolves catalog bindings, evaluates policy and compiles one
selected step. It requires neither a daemon nor an LLM.

```sh
cargo install --path crates/gateway-daemon --bin cg --locked
cg --help
```

`cg-registry` remains the catalog inspection tool. `cg` has no project profile
option. `--catalog` selects a reusable Agent/Skill/Process catalog directory;
it defaults to `catalog` relative to the current working directory.

For CG-22, `cg patterns --scope <project-scope> --json` reads the local
PostgreSQL experience and memory stores and rechecks eligibility before
reporting patterns. Use `--at <unix-seconds>` for a fixed inspection time.
`cg patterns --report <file-or-json> --json` displays an existing report.
Both forms are read-only. Start the database with `./scripts/start-postgres.sh`
and see [the PostgreSQL setup](postgres-compose.md) for credential handling.

## Runnable walkthrough

The [fixture directory](../tests/fixtures/declarative-cli) contains a synthetic
external project, an isolated reusable inspection catalog, a process snapshot,
and explicit test policy and projection decisions. Its policy and projection
files are pinned to the exact fixture basis. These are test decisions, not
production approvals.

Run from the repository root after installing `cg`:

```sh
fixture=tests/fixtures/declarative-cli
cg assess --context "$fixture/context.json" --json > /tmp/cg-assessment.json
cg plan --context /tmp/cg-assessment.json --intent "$fixture/intent.json" \
  --catalog "$fixture/catalog" --rules "$fixture/rules.json" --json > /tmp/cg-plan.json
cg resolve --plan /tmp/cg-plan.json --catalog "$fixture/catalog" \
  --rules "$fixture/rules.json" --process "$fixture/process.json" --json > /tmp/cg-resolution.json
cg explain --plan /tmp/cg-plan.json --catalog "$fixture/catalog" \
  --rules "$fixture/rules.json" --process "$fixture/process.json" \
  --policy "$fixture/policy.json"
cg compile --plan /tmp/cg-plan.json --catalog "$fixture/catalog" \
  --rules "$fixture/rules.json" --process "$fixture/process.json" \
  --policy "$fixture/policy.json" --projection "$fixture/projection.json" \
  --json > /tmp/cg-compiled.json
```

The output contains the minimal semantic context and validated CG-02
`execution_context`. No runtime is invoked and no process state is persisted.
Changing the source context, catalog or process snapshot requires new policy
and projection inputs pinned to the resulting basis.

## Input and output conventions

- `--context`, `--intent`, `--plan`, `--rules` and `--process` accept a JSON file,
  an inline JSON object, or `-` for stdin. Only one input may consume stdin.
- `--policy` and `--projection` accept local files only. The operator supplies
  these independently of external project data. This local CLI trusts those
  operator-selected files; fingerprints check consistency, not authenticity.
  Callers exposing the CLI through a service must authenticate this boundary.
- Adapter documents require integer `schema_version: 1`. Embedded CG-02,
  CG-06 and CG-07 documents keep their existing versions and validating serde
  contracts. Unknown fields, duplicate JSON keys, invalid IDs, unsupported
  versions and conflicting command options fail closed.
- `--json` writes a single JSON document to stdout, including on failure.
  Human output has a command heading and indented structured details. Human
  errors go to stderr. No progress messages contaminate JSON output.
- Reports with unresolved work are emitted before returning a nonzero status.
  Invocation/input errors use `{"schema_version":1,"error":{"code":...,"message":...}}`.
  Inner error names are diagnostic detail; `error.code` and exit status are the
  adapter's stable failure categories.

### Assess

`cg assess --context INPUT` accepts this normalization request:

```json
{
  "schema_version": 1,
  "scope": "external-project",
  "operating_mode": "DEVELOPMENT",
  "execution_profile": "FULL_PATH",
  "context": {"schema_version":"1.0","id":"external-context"},
  "observed_state_id": "external-state",
  "situation_id": "external-situation",
  "records": {"provenances":[],"observations":[],"facts":[],"evidence":[]},
  "unknown_subjects": ["architecture.clean"],
  "intent": null
}
```

Records use CG-06's [observation/evidence contracts](declarative-context-situation.md).
Unknown subjects are explicit dotted paths; no observation or evidence is
invented. `intent` is optional. Scope, Operating Mode and Execution Profile
are separate explicit values and grant no authority.

The assessment output contains `schema_version`, `scope`, `operating_mode`,
`execution_profile` and `document`. `document` is the canonical
`DeclarativeContextSituationDocument`, retaining CurrentState, Situation,
records and optional Intent. This output can be supplied to `assess`, `plan`
or `explain` again as a validated snapshot. Successful assessment can describe
unknown or conflicting state; those are modeled results, not parsing failures.

### Plan

`cg plan --intent INPUT --context INPUT [--rules INPUT]` accepts a canonical
CG-06 Intent containing its DesiredState. An Intent already captured in the
context must agree exactly with the explicit Intent. See the
[example](../tests/fixtures/declarative-cli/intent.json).

Capability bindings are explicit; there is no capability selection from prose:

```json
{
  "schema_version": 1,
  "planning": {"observation":"architecture.dependency-analysis"}
}
```

Supported `planning` fields are `domain_change`, `evidence_acquisition`,
`observation`, `input_acquisition`, `conflict_resolution` and `assessment`.
Each is a canonical Capability ID. Safety-class checks remain in CG-07.
Omitted bindings stay missing. Comparison, Delta and planner rules use the
versioned v1 defaults; their versions and the capability snapshot digest are
included in the explanation. Delta IDs use the SHA-256 digest of the DesiredState
ID, keeping derived identifiers valid even for maximum-length source IDs.

A successful Plan document contains `schema_version`, `assessment`,
`desired_state`, `delta`, `plan` and `explanation`. `delta` and `plan` use their
canonical domain JSON. The envelope retains the upstream contracts needed to
validate later resolution. Missing/unsupported planning inputs emit the
available assessment, Delta and structured planner diagnostics with exit 5; they never produce
an executable placeholder Plan.

### Resolve

`cg resolve --plan INPUT` consumes the complete Plan document above. A bare
Plan does not contain the situation and Delta needed by CG-08 validation.
Resolution rereads and validates the catalog; it never trusts previously
serialized selections. The `resolution` field contains the canonical CG-08
artifact, including its exact `basis`, ranked alternatives and diagnostics.
Assessment, Delta, Plan, resolution explanation, process inspection and
optional policy report remain inspectable in the output.

The optional `resolution` section of `--rules` supports:

| Field | Input |
| --- | --- |
| `required_process` | CG-04 `DefinitionIdentity` (ID, version, digest); omission requests no template. |
| `activities` | PlanStep ID → CG-04 Activity ID. |
| `primary_agents` | PlanStep ID → Agent ID. |
| `participants` | PlanStep ID → array of Agent IDs. |
| `semantics` | Exact canonical precondition text → explicit condition. |
| `priorities` | Array of `{ "provider":{"skill":"id"}, "priority":10 }` or an `agent` provider. Duplicate provider entries are rejected. |

Conditions are `"ALWAYS"`, `"NEVER"`, or single-key objects such as
`{"MODE":"DEVELOPMENT"}`, `{"PROFILE":"FULL_PATH"}`,
`{"PROCESS_STATE":"START"}`, `{"DESIRED_CONDITION":"clean"}` and
`{"UNSUPPORTED":"rule-reference"}`. These are explicit rule semantics, not
policy grants. Unmapped preconditions retain unknown readiness.

The adapter uses the core's deterministic search with a fixed 10,000-visit
budget and no optional-provider preference. Ties remain ambiguous. Advanced
nested-provider overrides, alternative requirement groups, lifecycle-contract
mappings and predecessor-completion evidence are not exposed by this v1 CLI;
requirements that need them remain unsupported or blocked by the core.

`--process` accepts `{ "schema_version":1, "instance": <CG-04 instance>,
"expected_revision":0 }`. The definition is loaded from `catalog/processes`.
The existing process application validates definition identity, state and
revision and returns its read-only inspection. The CLI cannot manufacture an
activity authorization or transition a process to satisfy a blocker.

### Explain and policy

- `cg explain --context INPUT` shows normalized state, assessments, risks,
  diagnostics and their source references.
- Adding `--intent INPUT` derives and explains the DesiredState → Delta → Plan
  chain using the same planning operation.
- `cg explain --plan INPUT` resolves again and returns the full CG-08 selection
  and rejection graph. `--process` includes CG-04 inspection; `--policy` adds
  freshly evaluated CG-09 decisions and reason codes.

`resolve` also accepts `--policy`. Without it, the policy field is null and no
policy decision is claimed. Compile always requires policy input.

The [policy fixture](../tests/fixtures/declarative-cli/policy.json) illustrates:

- the complete `basis` copied from the current resolution artifact;
- matching `operating_mode` and `execution_profile`;
- `policies`: definitions with `id`, `description`, `allowed_capabilities` and
  optional `denied_capabilities`;
- optional CG-02 `constraints` and Capability ID → string-array
  `required_evidence`;
- `steps`: PlanStep ID → explicit facts. Facts support Capability ID →
  `"GRANTED"`/`"DENIED"` maps for `authorizations` and `consents`, string arrays
  `evidence` and `satisfied_constraints`, optional `work_class`
  (`"FEATURE"`/`"MAINTENANCE"`), and `prerequisites_satisfied` (default false).

The CLI gets canonical capability contracts from the validated catalog.
Missing approvals stay missing. Policy is evaluated separately for concrete
bindings; ambiguous alternatives are never unioned into an authorization.

### Compile

`cg compile --plan INPUT --policy FILE --projection FILE` resolves again,
checks the supplied basis and evaluates policy before calling
`ContextApplication::compile_step`. That application revalidates the
resolution, policy and CG-02 projection. Non-ALLOW decisions emit their report
and no execution context.

The [projection fixture](../tests/fixtures/declarative-cli/projection.json)
requires `basis`, `step`, `process` identity, `workflow`, workflow-mapping
`decision_reference`, `state_decision`, execution-context `id`, normalized
`task`, CG-02 `state`, `target_runtime` and explicit `workflows`. Each workflow
has `id`, `description`, `primary_agent_id`, `skill_ids`, and `policy_id`.
`knowledge_queries`, `fragments` and `selected` are optional arrays.

Fragments have `id`, `kind`, `content`, `scope`, `step`, `source`, optional
`revision`, CG-06 `quality`, `rationale`, optional `evidence` reference array
and optional `validation`. Kinds are `knowledge`, `memory`, `user_input` and
`evidence`. Evidence `content` identifies a captured CG-06 Evidence record;
its provenance is taken from those records and the output is a reference.
Memory requires a revision and validation reference. External fragments cannot
claim an authority/catalog kind or an incompatible trust class.

Only IDs listed in `selected` enter the minimal context. Missing selections,
conflicting duplicates, wrong scope/step and invalid trust fail compilation.
Original Intent input is preserved separately from normalized task text.
The compiler retains policy findings, restrictions and mapping provenance.

NoOp, unsupported v1 binding shapes and absent owner mappings produce no
execution context. See [CG-10's projection boundaries](context-compiler.md).
Compilation is a snapshot result, not a reusable execution grant.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Operation completed (includes a valid NoOp Plan or resolution). |
| 2 | Invalid command, option combination or missing argument. |
| 3 | Input/output error, malformed JSON, unsupported adapter version or invalid wire/domain value. |
| 4 | Normalization or Situation assessment failed. |
| 5 | Planning failed, has unresolved diagnostics, or explicit Intent disagrees with captured Intent. |
| 6 | Catalog or resolution failed; includes missing/ambiguous/incomplete bindings. |
| 7 | Process catalog/snapshot error or resolved bindings have blocked/deferred/unknown readiness. |
| 8 | Policy error, stale policy basis, denial, missing consent or missing evidence. |
| 9 | Projection/mapping/context compilation failed. |

When resolution and policy both report failures, policy's exit 8 takes
precedence; both reports remain available. JSON decoding errors use exit 3
regardless of the stage. A failed output write uses exit 3.

## Verification

```sh
cargo test -p gateway-daemon --all-targets
cargo test --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
bash scripts/check-architecture.sh
cargo llvm-cov -p gateway-daemon --all-targets --json --output-path target/cg11-coverage.json
python3 scripts/check-cli-coverage.py --self-test
python3 scripts/check-cli-coverage.py target/cg11-coverage.json
```

The CG-11 coverage gate checks each new production file independently at 95%.
Contract tests exercise subprocess stdout/stderr/status, the complete external
project chain, deterministic repetition, policy denial, stale basis/revisions,
input validation, provider ambiguity, blocked readiness and context trust.

### Recorded verification

Local Linux verification passed with workspace tests, Clippy with warnings
denied, formatting, the architecture guard and the existing aggregate daemon
coverage gate. The 15 CG-11 subprocess contract tests and the strict JSON
boundary test passed alongside the existing registry CLI tests.

| New production file | Covered / measured lines | Coverage |
| --- | --- | --- |
| `src/bin/cg.rs` | 2 / 2 | 100.00% |
| `src/declarative_cli/mod.rs` | 221 / 222 | 99.55% |
| `src/declarative_cli/inputs.rs` | 192 / 197 | 97.46% |
| `src/declarative_cli/pipeline.rs` | 406 / 423 | 95.98% |
| `src/declarative_cli/json_input.rs` | 46 / 47 | 97.87% |

Paths are relative to `crates/gateway-daemon`. CI repeats the per-file gate and
installs `cg` to run the complete documented fixture chain. For shared
Windows/Linux checkouts, use separate `CARGO_TARGET_DIR` and
`CARGO_LLVM_COV_TARGET_DIR` directories for Linux artifacts.

## Learned procedure evaluation (CG-23)

`cg evaluate` evaluates a supplied procedure/dataset; `cg simulate` adds
counterfactuals from historical positives; `cg replay` verifies a self-contained
evidence bundle. Evaluation and simulation require `--procedure`, `--dataset` and
`--runtime-version`; replay requires `--bundle`. Use `--json` to retain the complete
bundle. Exit 11 returns a failed evaluation with its report, and exit 3 rejects
invalid or altered artifacts. These commands execute no capabilities or process
mutations. See [the evaluation contract and example](procedure-evaluation.md).

## Learned procedure registry inspection (CG-24)

`cg procedures --registry <journal-file> [--json]` validates and reconstructs the
CG-24 journal, showing immutable versions, lifecycle states, evaluation evidence,
canary bounds, predecessor references, execution outcomes and complete audit history.
It supports stdin (`--registry -`) and inline JSON. Invalid journals exit 3; missing
arguments exit 2. The command requires no LLM and writes no registry state. See
[procedure promotion](procedure-promotion.md) for authority and rollback contracts.
