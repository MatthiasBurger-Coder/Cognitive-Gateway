# CG-08.05 Agent responsibilities

`bind_agent_candidates` derives per-PlanStep responsibility alternatives from a
validated ResolutionSnapshot and explicit CandidateRules/AgentRules. It reruns
canonical candidate discovery rather than trusting a mutable supplied report.
Every responsibility retains its capability requirement, provider, Agent and
relationship source. No candidate ordering decides an executor.

A direct Agent provider yields its own identity. A Skill provider yields its
canonical owner, when present, and Agents whose declared Skill roots include
that Skill in CG-03's required dependency closure. Sources distinguish direct
provider, Skill owner and Agent-Skill root reference. Owner and Agent-Skill
relationships are additive eligibility sources, not an inferred exclusive
ownership rule. There is no inference from prose, names or neighboring catalog
entries. An unowned, unlinked Skill remains explicitly unbound even if unrelated
Agents exist.

Primary candidates are Agents with at least one capability responsibility in
that step. Explicit primary constraints restrict this set. Required participants
remain separate and must also have a capability responsibility; they cannot
simultaneously occupy the primary role. Missing/incompatible required roles and
conflicting primary constraints are retained as diagnostics. All requirement
alternatives remain available for CG-08.08's global composition, which must
prove complete coverage and actual required participation. A non-noop step with
no primary remains unresolved. No-op creates no artificial primary.

## Canonical Process roles

An optional ProcessRoleReference names an exact DefinitionIdentity and declared
ActivityId. The reference is validated against the captured ProcessRegistry and
active instance pin. It does not select or migrate a Process. The referenced
activity must include a candidate's capability before its Agent responsibility
can enter the step. Canonical activity constraints `primary-agent=<AgentId>` and
`participating-agent=<AgentId>` add role restrictions; malformed IDs fail closed.
Caller and canonical primary constraints cannot overwrite one another.

The complete ActivityDefinition remains in the output. Other activity constraints
are preserved for their subsequent Skill/applicability owners, not interpreted
as roles or discarded. Later composition must also verify that the activity is
one of the selected Process candidate's compatible activity mappings. Agent
eligibility alone does not establish Skill closure, lifecycle readiness or
permission.

Results retain the source basis and both rule inputs. Diagnostics distinguish
unbound Skills, missing providers, missing/incompatible primary, missing
participants, conflicting primary and Process-capability mismatch. There is no
runtime launch, tool invocation, process transition or provider prompt API.

## Evidence

Six integration tests cover direct providers, Skill ownership and Agent-Skill
links, unbound Skill-only providers, distinct primary/participating roles,
incompatible/unknown Agents, canonical role conflicts, Process capability
mismatch, malformed role IDs, invalid process references and no-op. Synthetic
Agents obey the existing non-empty Skill-reference invariant.

Run `cargo test -p gateway-application --test resolution_agents` and
`cargo llvm-cov -p gateway-application --all-targets --json --output-path
target/cg08-coverage.json`. Measured on 2026-09-09 with cargo-llvm-cov 0.9.0:
**186/186 production lines covered (100%)**, no exclusions. Workspace tests,
Clippy, format and architecture guard pass.
