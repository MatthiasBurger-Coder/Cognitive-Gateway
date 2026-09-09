# CG-08.04 optional and pinned Process templates

`select_process(&ResolutionSnapshot, &ProcessSelectionRules)` reads compiled,
validated CG-04 registry entries. It never compiles Gherkin, starts an instance,
executes an activity, transitions a Process or migrates a definition pin.

With preference `None` and no mandatory constraints or active instance, the
result is `NoTemplate` with no fabricated identity. `Optional` considers
compatible templates but permits absence. A Plan lifecycle requirement,
`Required` preference, explicit definition/activity/output constraint, or active
instance makes the template mandatory. Missing/incompatible mandatory templates
remain failures. Multiple compatible templates remain `Ambiguous`; the rule
version has no implicit ranking or latest-version preference.

An active instance filters candidates to its exact ID/version/digest. An
incompatible explicit definition constraint fails rather than upgrading,
restarting or replacing the instance. Selected candidates retain the CG-04
DefinitionIdentity and optional instance ID/revision from the snapshot.

## Activity contracts and supported v1 mapping

Each non-noop PlanStep maps to all declared activities satisfying its contract.
The supported v1 composition unit is one declared activity per step, with all
matching activity alternatives retained for later composition. Each activity
must cover the step's ungrouped mandatory capabilities and at least one member
of each explicit mandatory alternative group. Optional capabilities do not
replace mandatory ones. Activity and output-evidence constraints are exact
typed matches; complete ActivityDefinitions preserve capabilities, output
evidence and constraints, including explicit Agent/Skill constraints for the
subsequent binding phase. No concrete constraint bypasses capability matching.

CG-04 activities do not natively label CG-07 LifecycleRequirementKind. The caller
therefore supplies an explicit versioned mapping from a lifecycle kind to an
existing canonical ActivityConstraint. An absent mapping produces `Unsupported`;
a mapping not present on the activity is incompatible. The resolver does not
infer semantics from prose or add that constraint to the catalog. These mappings
are metadata compatibility rules, not proof of current eligibility; CG-08.07
still checks readiness and CG-04 remains the only transition authority.

ProcessSelection retains its complete rules and source basis. Outcomes are
NoTemplate, Unique, Ambiguous, Missing, Incompatible and Unsupported. A Unique
template can still contain multiple activity alternatives and is not a complete
Agent/Skill/capability binding. Rejections identify exact definitions, optional
PlanStep references and the failed pin/definition/activity/lifecycle condition.

No-template results remain valid CG-08 data but require the explicit CG-02
projection compatibility assessment owned by CG-08.11.

## Evidence

Five integration tests in `resolution_process` cover optional absence, mandatory
missing/incompatible templates, unique/tied templates and registry permutations,
typed activity/output constraints, unsupported lifecycle mapping, exact
canonical mapping, old-version pinning, explicit definition conflicts, and
one-of groups. Synthetic Process catalog fixtures are explicitly labeled and
compiled before calling the resolver. Digest tampering is already rejected by
the snapshot tests before selection can run.

Run `cargo test -p gateway-application --test resolution_process` and
`cargo llvm-cov -p gateway-application --all-targets --json --output-path
target/cg08-coverage.json`. On 2026-09-09 cargo-llvm-cov 0.9.0 reports
**160/161 production lines covered (99.38%)**, no exclusions. Workspace tests,
Clippy, format and architecture guard pass.
