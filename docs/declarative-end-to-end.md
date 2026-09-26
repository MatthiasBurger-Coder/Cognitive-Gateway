# External project acceptance proof (CG-12)

[CG-12](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/11)
is proven by the real `cg` executable in the
[CLI acceptance suite](../crates/gateway-daemon/tests/declarative_cli/cg12.rs).
It uses this explicit structured Intent:

> Ensure the domain layer does not depend on infrastructure and test coverage is at least 95%.

The text is retained as original input. The caller also supplies typed
DesiredState conditions; no natural-language parser or LLM is involved.

| Condition | External observation | Delta | Required capability |
| --- | --- | --- | --- |
| `architecture.dependency EQUALS false` | `true` | `UNSATISFIED_CONDITION` | `project.quality-change` (`MUTATE`) |
| `coverage.percent GREATER_OR_EQUAL 95` | `92` | `UNSATISFIED_CONDITION` | `project.quality-change` (`MUTATE`) |

## Boundaries exercised

1. **External input → Situation (CG-06).** The test creates synthetic caller
   JSON in a temporary directory outside the checkout. Both observations have
   facts, supporting report evidence and external tool provenance. `cg assess`
   normalizes them; `cg plan` consumes the resulting assessment file.
2. **DesiredState → Delta → Plan (CG-07).** Both unsatisfied conditions produce
   change steps with abstract capability requirements. The Delta retains exact
   fact, observation, evidence, provenance, CurrentState and Situation IDs.
   Concrete Agent and Skill identities are absent from the Plan.
3. **Plan → bindings (CG-03/CG-08).** The CLI loads a synthetic reusable catalog
   and its CapabilityIndex. One Skill provides the mutation capability, one
   Agent owns the work, and a process defines its matching activity. The
   resolution explanation links conditions, requirements, providers and
   process readiness. Selection alone grants no authority.
4. **Process + policy (CG-04/CG-09).** A validated process instance supplies
   lifecycle readiness. Separate operator policy inputs grant the capability
   and mutation consent for each step, with explicit prerequisite evidence.
   Test approvals are created from the exact resolution basis. This is fixture
   setup, not a mechanism for granting production approval automatically.
5. **Projection + minimal context (CG-02/CG-10).** The operator explicitly maps
   each resolved step to a Workflow and CG-02 state. Both steps compile and
   deserialize through `ExecutionContextIR::from_json`. Each compiled result
   retains original input and one selected evidence reference with its captured
   provenance. Raw reports and unselected knowledge are omitted.

Workflow mapping is an explicit projection decision; the CLI does not infer a
Workflow from prose. Compilation does not execute a change, update process
state, or claim that the project now meets its goals. Verification after
execution is provided by the separate [CG-14 closed-loop application API](closed-loop-execution.md).

The synthetic catalog carries no external project identity. All caller state,
knowledge, evidence, policy decisions and outputs are generated at test time.
No `profiles/<project>/` directory or project configuration is added to the
Gateway repository. Real projects remain authoritative for their own reports;
the CLI accepts captured snapshots through its explicit file boundary.

## Reproduce and inspect

Run the acceptance matrix:

```sh
cargo test -p gateway-daemon --test declarative_cli cg12::
```

To retain the exact tested inputs and outputs outside the checkout, export to
a **new directory whose parent exists**:

```sh
proof_parent=$(mktemp -d)
export CG12_EXPORT_DIR="$proof_parent/proof"
cargo test -p gateway-daemon --test declarative_cli cg12::
unset CG12_EXPORT_DIR
cargo install --path crates/gateway-daemon --bin cg --locked
cd "$proof_parent/proof"
```

The test refuses to overwrite an existing export directory. The directory
contains caller context and Intent, the synthetic catalog, rules, the process
snapshot, operator policy, two projections, and every successful output.
Replay from that external directory:

```sh
mkdir replay
cg assess --context external-context.json --json > replay/assessment.json
cg plan --context replay/assessment.json --intent external-intent.json \
  --catalog . --rules rules.json --json > replay/plan.json
cg resolve --plan replay/plan.json --catalog . --rules rules.json \
  --process process.json --json > replay/resolution.json
cg explain --plan replay/plan.json --catalog . --rules rules.json \
  --process process.json --policy policy.json --json > replay/explanation.json
for step in step-condition.0.0 step-condition.0.1; do
  cg compile --plan replay/plan.json --catalog . --rules rules.json \
    --process process.json --policy policy.json \
    --projection "projection-$step.json" --json > "replay/compiled-$step.json"
done
python3 - <<'PY'
import json
from pathlib import Path
artifacts = list(Path("replay").glob("*.json"))
assert len(artifacts) == 6
for replay in artifacts:
    assert json.loads(replay.read_text()) == json.loads(Path(replay.name).read_text()), replay.name
print("All six replay artifacts match the acceptance proof")
PY
```

`assessment.json` exposes the observed state, records and Situation.
`plan.json` exposes the DesiredState, Delta, abstract Plan and planning
explanation. `resolution.json` and `explanation.json` expose the concrete
selection graph, process inspection, snapshot basis and policy decisions.
`compiled-step-*.json` contains each minimal context and CG-02 projection.
See [the CLI contracts](declarative-cli.md) for field definitions and exits.

## Acceptance matrix

| Proof | Assertion |
| --- | --- |
| Both known violations | Two change steps, exact evidence lineage, both compile with explicit approvals |
| Determinism | Repeated compilation and reversed external record order produce equal JSON artifacts |
| Minimal context | Only the selected report reference enters each context; raw evidence and unrelated knowledge remain out |
| Missing authorization or mutation consent | Exit 8; no ExecutionContext |
| Explicit capability denial | Exit 8; no ExecutionContext |
| Paused process | Resolution exits 7; fresh policy evaluation reports `PROCESS_BLOCKED`, compile exits 8 |
| Changed external report | Old approval fails with `STALE_BASIS`, even when observed values still agree |
| Missing or unknown capability binding | Exit 5, explicit diagnostics, no executable Plan |
| False or unmapped provider precondition | Exit 6 with a resolution explanation |
| Unknown observed state | Two `UNKNOWN_STATE` Delta items; missing observation capability blocks planning |
| Mutation capability used for observation | Exit 5; incompatible safety class cannot produce a Plan |
| Malformed observation | Exit 3 with `INVALID_INPUT` |

The existing CG-11 tests additionally cover ambiguous providers, satisfied-goal
NoOp behavior, stale revisions/projections and external context trust errors.
The CG-12 tests reuse their subprocess harness without changing product code.

## Quality evidence

CI runs the full workspace, these acceptance tests, and the exported chain
through the installed CLI from the external directory. It compares all six
replayed artifacts with the tested outputs. Existing aggregate and per-file
CLI coverage gates remain at 95%.

Reproduce the quality checks from the repository root:

```sh
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
bash scripts/check-architecture.sh
cargo llvm-cov -p gateway-daemon --all-targets --fail-under-lines 95 \
  --json --output-path /tmp/cg12-coverage.json
python3 scripts/check-cli-coverage.py --self-test
python3 scripts/check-cli-coverage.py /tmp/cg12-coverage.json
git diff --check
```

For a shared Windows/Linux checkout, set separate Linux `CARGO_TARGET_DIR`
and `CARGO_LLVM_COV_TARGET_DIR` paths. CG-12 changes tests, CI and documentation;
no production behavior or coverage threshold changes.

Local Linux verification passed: 503 workspace tests, all four CG-12 tests,
formatting, Clippy with warnings denied, the architecture guard and diff checks.
All six exported artifacts matched a replay through an installed debug `cg`.
Daemon coverage was 1,749 / 1,777 lines (98.42%); the five CLI production files
measured 100%, 99.55%, 97.46%, 95.98% and 97.87%, passing the unchanged 95% gates.
