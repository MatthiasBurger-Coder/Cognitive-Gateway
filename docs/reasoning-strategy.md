# Provider independent reasoning strategies (CG-20C)

`gateway_domain::reasoning_strategy` owns the version 1.0 attempt contract.
`gateway_application::reasoning_strategy` owns the typed adapter handoff. A
caller first obtains a `BudgetedCompiledStep` through CG-09 policy, CG-10
compilation and CG-20B bounded selection. Strategy selection cannot create an
authorized step, add tools, change project scope, or replace that context.

## Strategies and expectations

| Strategy | Adapter capability | Intended attempt |
| --- | --- | --- |
| `DIRECT` | none | One response to the compiled step. |
| `RETRIEVAL_ASSISTED` | `RETRIEVAL` | A response using bounded retrieval; `retrieval_rounds` must be positive. |
| `PLAN_EXECUTE` | `PLANNING` | An explicit plan followed by an attempt within the same limits. |
| `VERIFY` | `VERIFICATION` | A check against evidence; requires `EVIDENCE_BACKED` verification. |
| `MULTI_PASS` | `MULTIPLE_PASSES` | More than one pass within one aggregate budget. |

`required_capabilities` may add requirements beyond the strategy's intrinsic
capability. `output_contract` is a nonempty caller-defined output identity.
`verification` is either `NONE` or `EVIDENCE_BACKED`. Evidence-backed attempts
are rejected when CG-19 reports missing, conflicting, stale, untrusted,
contaminated or exhausted evidence. An adapter output never establishes
acceptance; CG-14 uses fresh observations and its existing evidence checks.

The wire representation uses fixed enum names and version `1.0`. Unknown
strategy names, unknown fields, unsupported versions and invalid budgets fail
deserialization. A rollback to an older reader rejects newer versions rather
than silently changing a strategy. No persistent provider state is defined.

## Budget and fallback

`StrategyBudget` limits aggregate iterations, retrieval rounds, cost units,
elapsed milliseconds and tokens. `cost_unit` is a validated identifier and
must match the owning shared budget, such as the CG-19 retrieval cost unit.
Adapter success and failure reports must name that same unit. A caller keeps
one `StrategySession` across attempts, replanning and fallback. The application
reserves one iteration before adapter invocation. The adapter returns measured
additional usage, including failed work; checked addition rejects overflow and
limits. The caller must stop after a budget error. The CG-19 retrieval and
CG-20B context limits continue to apply independently; the strategy limit may
only narrow them.

An unsupported strategy fails with `UnsupportedStrategy` unless the contract
declares one fallback with a nonempty reason. Fallback retains the same output
identity, verification expectation, authorization, scope and aggregate budget.
It may drop only the requested strategy's own mechanism; other required
capabilities must remain supported. A fallback with incompatible requirements
fails with `IncompatibleFallback`. The selection record carries both requested
and selected strategy and the fallback reason.

Example: a direct-only adapter rejects `MULTI_PASS` by default. A contract can
declare `DIRECT` fallback with reason `single bounded attempt acceptable` if
the requested output and verification still apply. An additional mandatory
`RETRIEVAL` capability makes that fallback incompatible for a direct-only
adapter.

## Public handoff and audit

`StrategyHandoff` contains references to the bounded compiled step, validated
contract, selection, CG-19 sufficiency assessment and prior cumulative usage.
The adapter returns an output reference, provenance references and usage. The
application decision records the compiled context ID, validated evidence IDs,
public findings and cumulative usage. It neither
requests nor stores hidden reasoning. Hosts record this decision alongside
existing CG-14 process and policy traces and apply their retention rules to
references. Provider prompt rendering, final window enforcement and actual
model invocation belong to EPIC-06; learned routing belongs to EPIC-03.

## Requirement and verification matrix

| Requirement sentence | Implementation | Verification |
| --- | --- | --- |
| R01: The five strategies have fixed names, version and capability requirements. | `gateway-domain/src/reasoning_strategy.rs`: `ReasoningStrategy`, `ReasoningCapability`, `ReasoningStrategyContract::new` | `gateway-domain/tests/reasoning_strategy.rs`: roundtrip and invalid capability tests |
| R01: Unknown values, versions, budgets and fields fail. | Domain `StrategyWire` validation and `StrategyUsage::validate` | Domain wire and aggregate usage tests |
| R02: A strategy cannot grant capability, change scope or bypass authorization. | Application `StrategyHandoff` accepts only `BudgetedCompiledStep`; domain selection checks adapter support | Application direct-only fake adapter test; existing CG-09/CG-10 context tests |
| R02: Iteration, retrieval, cost, latency and tokens stay cumulative. | Domain checked usage addition; application `StrategySession::attempt` reserves an iteration | Domain aggregate usage test and application authorized handoff test |
| R03: Unsupported selection fails unless a declared compatible fallback exists. | Domain `select` and `StrategyFallback`; application `select_strategy` | Domain fallback matrix and application direct-only fake adapter test |
| R04: Evidence sufficiency and bounded context are consumed without accepting model confidence. | Application evidence admission and bounded step handoff | Application conflicting/insufficient/stale/contaminated/exhausted evidence tests; CG-20B context tests |
| R05: The public decision records selection, reason, input context, evidence, output, provenance, findings and usage. | Application `StrategyDecision::to_json`; no hidden reasoning field | Application public trace test and authorized handoff test |

Run `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo fmt --check`, `bash scripts/check-architecture.sh` and
`python3 scripts/quality-gate.py` from the repository root. The established
quality gate writes revision-bound evidence under `target/release-evidence/`.
