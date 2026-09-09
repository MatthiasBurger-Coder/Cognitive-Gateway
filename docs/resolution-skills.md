# CG-08.06 effective Skill closure

`resolve_skill_closure` expands one step's explicitly chosen canonical providers
and typed SkillRules. Chosen providers are checked against fresh deterministic
candidate discovery. A chosen Skill provider is a mandatory root. Explicit roots
can express Agent/Process requirements supplied by composition; an Agent's
available Skill list is not blanket activation. `related_skills`, knowledge
queries and narrative Skill rules never add roots or executable dependencies.

The existing CG-03 dependency graph supplies mandatory edges. The resolver walks
those validated edges, records every inclusion path, deduplicates effective
Skills and emits dependency-first order. Missing dependencies and pure Skill
cycles are rejected by CG-03 during snapshot capture, now with typed missing-edge
or cycle identities preserved. No replacement graph authority is introduced.

## Conditions

The finite v1 condition contract supports Always/Never, exact CG-02 mode/profile,
exact current Process state, or a condition identity from the captured
DesiredState. Desired-condition evaluation delegates to the existing typed
comparison engine and normalized CurrentState, with v1 default comparison rules.
No ambient time or prose evaluator exists. Missing Process state is unknown;
unknown desired-condition identities and explicit future/unsupported condition
references are unsupported. Condition keys must reference canonical Skills.

Outcomes preserve satisfied, unsatisfied, unknown, conflicted and unsupported.
Comparison evidence/input gaps remain unknown; incomparable comparisons remain
unsupported. An unsatisfied or unresolved mandatory Skill condition makes the
closure incomplete. An optional root can be excluded with its condition outcome
and optionality retained. Once a root is included, all its `requires` dependencies
are mandatory: an inadmissible dependency cannot be trimmed to produce success.

## Required capabilities and termination

Every effective Skill contributes its `required_capability_ids`, retaining the
canonical INSPECT/MUTATE class when known. Missing declarations remain explicit.
These are requirements for composition and CG-09, not provided or approved
capabilities. No policy decision is produced.

Nested required-capability providers must be explicitly supplied in SkillRules.
An absent binding stays unbound; an invalid provider relationship fails. A Skill
provider adds its own required closure; a direct Agent provider does not activate
its entire available Skill list. Alternating Skill -> required capability ->
provider Skill cycles are detected with their path. These cross-relationship
cycles supplement CG-03's pure Skill graph validation.

Traversal uses a caller-supplied positive visit budget up to 100,000 and a fixed
64-node path-depth limit. Reaching either produces `LimitExceeded` and an
incomplete result, never a false no-match. All paths are preserved within that
declared limit, including diamond paths. Equivalent inputs have deterministic
traversal and diagnostics.

`EffectiveSkills.complete` means only that this supplied candidate's closure is
complete. It does not assert whole-Plan requirement coverage, Agent assignment,
precondition evidence, lifecycle readiness, permission or execution. Partial
Skills and diagnostics remain inspectable. The result retains the Plan basis,
step, chosen providers and all rule inputs for subsequent canonical revalidation.

## Evidence

Five integration tests cover chains/diamonds and all paths, reordered catalogs,
related-Skill nonactivation, optional exclusion, mandatory condition failure,
mode/profile/Process/DesiredState conditions, transitive MUTATE requirements,
explicit nested providers, Skill/capability cycles, missing nodes and deterministic
limits. A focused unit test proves the complete CG-07 comparison-outcome mapping,
including conflicted and unsupported results. Test implementation lives under
`tests/`, separate from the production coverage files.

Run `cargo test -p gateway-application --test resolution_skills` and
`cargo llvm-cov -p gateway-application --all-targets --json --output-path
target/cg08-coverage.json`. On 2026-09-09 cargo-llvm-cov 0.9.0 reports
**200/201 production lines (99.50%)** for resolution_skills.rs and **196/197
(99.49%)** for the materially changed snapshot module, without exclusions.
Workspace tests, Clippy, format and architecture guard pass.
