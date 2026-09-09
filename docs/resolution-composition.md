# CG-08.08: deterministic whole-binding composition

`compose_resolution(snapshot, CompositionRules)` enumerates full canonical
bindings, not a greedy list of preferred providers. The v1 rules retain exact
candidate, process, Agent, Skill and applicability inputs. All mutable execution,
policy, retrieval, compilation and runtime concerns remain outside this module.

## Search and selection

For each compatible whole-plan process definition (or explicit no-template
context), the search enumerates activities, requirement providers, explicit one-of
groups, optional omissions, recursive Skill capability providers, responsible
Agents for providers and every effective Skill, and meaningful primary roles.
Plan-level products share one process definition; steps cannot silently select
different templates. Hard role, closure and applicability restrictions filter
before ranking. Failed closure and applicability branches retain provider-specific
rejection records and closure traces. Canonically equal complete bindings are
deduplicated without removing requirement links or Skill inclusion paths.

Supported source restrictions are conjunctive. Canonical process primary/participant
constraints and required-skill inclusion are checked structurally. Exact explicit
lifecycle mappings are already checked by process selection. Other process
constraints need explicit semantics and cannot disappear on future templates.
Nested required capability preconditions and read-only class constraints receive
the same checks as root contracts. A selected Agent does not automatically activate
all of its advertised Skills. Additional required roots must be supplied explicitly
or declared with the canonical process `required-skill` constraint.

Ranking v1 is an explicit lexicographic pair: sum of caller-supplied integer
provider priorities (per selected requirement and effective nested capability),
then selected-requirement count **only** when `prefer_optional` is true. Missing
priorities are zero. Scores never supply authorization or override hard constraints.
Highest-ranked ties remain AMBIGUOUS; canonical traversal order is presentation,
not a semantic tie-break. Independent best complete step results survive partial
plan failure. Optional omission always has a requirement-specific reason; mandatory
group cardinality and required Skill closure cannot be omitted.

Process BLOCKED/DEFERRED readiness is retained with an identified binding, not
turned into permission. Unsatisfied/unsupported intrinsic restrictions and unknown
current prerequisites are not accepted as complete bindings. Future prerequisites
behind pending predecessors remain deferred through CG-08.07.

## Bounds and outcomes

Every Cartesian expansion, closure branch and emitted role attempt consumes a
deterministic budget unit. `max_visits` must be 1..100000; Skill closure retains its
own explicit budget and depth bound. Any exhaustion produces SEARCH_LIMIT, even
if a valid candidate was seen. Alternatives are then diagnostic only, never a
claim that all better/equal branches were searched. No elapsed-time or random
stopping rule exists. Inputs and catalog enumeration are canonical.

The report retains discovery distinctions (unknown capability, absent provider,
incompatible contract), process rejection distinctions, rejected closure/role/
applicability reasons, every viable step alternative, highest-ranked complete plan
alternatives, explicit per-step outcomes and overall NO_OP/RESOLVED/AMBIGUOUS/
PARTIAL/MISSING/CONFLICTING/UNSUPPORTED/SEARCH_LIMIT. Invalid rules are typed errors.
Only one non-exhausted complete plan alternative is uniquely resolved. This is
still not an executable plan or a policy approval.

## Evidence

Five integration suites cover a greedy trap, canonical input permutations, honest
semantic ties, recursive capability choices/cycles, explicit optional and one-of
semantics, independent partial failure, deterministic search limits, complete
multi-step process consistency, process-required Skills and roles, active instance
contracts, unsupported constraints, explicit and empty no-op plans, invalid rules.
Fixtures are synthetic; they do not extend the built-in catalog.

```powershell
cargo test -p gateway-application --test resolution_composition
$env:CARGO_TARGET_DIR='D:\Projects\Cognitive-Gateway\target\cg08'
cargo llvm-cov -p gateway-application --all-targets --json --output-path target/cg08-coverage.json
```

New production module: **645/656 lines, 98.32%**. Workspace tests, formatting,
Clippy with warnings denied and architecture guard pass.
