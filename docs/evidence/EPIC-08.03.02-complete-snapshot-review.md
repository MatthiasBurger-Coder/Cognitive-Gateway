# #405 complete scoped snapshot data qualification

**LOCAL_QUALIFIED — READY_FOR_REVIEW.** This is CONTRACT/COMPONENT evidence for
the application-owned data contract and codec. It does not establish an
installed MCP acquisition, a PostgreSQL/session consumer, an effect, current
execution authority or goal success. Parent #274 and full #271 / #272–#279
acceptance remain open.

The candidate adds five production source files under
`gateway-application/src/sessions/scoped_snapshots`, exported by the existing
session module. The [structured report](EPIC-08.03.02-complete-snapshot-review.json)
binds 17 source/prerequisite files against base `db7096d`. All frozen public v2
schema files have zero diff. Native CG-06 fixtures contain actual typed records
with explicit lineage; they do not fabricate invocation, dispatch, consent,
goal-verification or current authority grants.

| ID | Requirement and production owner | Final evidence | Status |
| --- | --- | --- | --- |
| AC1 | Typed owner/project/session/run/revision/step/dispatch/phase binding and closed universe; sealed wrapper | Native multi-object, same-source and complete-world fixtures; externally selected reload binding | VERIFIED_LOCAL |
| AC2 | World membership independent of goal/context selection | Non-goal fields retained; omitted/extra fields, phantom object subject reuse, duplicate/orphan reads denied | VERIFIED_LOCAL |
| AC3 | Atomic export and globally versioned sets | Actual export-root grouping; snapshot-scoped arguments; full per-route domain pins; actual epoch/membership anchors; complete bracket intervals | VERIFIED_LOCAL |
| AC4 | Qualified direct observed values and genuine object versions | Unknown/probabilistic/weak/stale/partial/acknowledgement/foreign/conflicting data denied; Caller/Synthetic claims cannot pass as Report evidence | VERIFIED_LOCAL |
| AC5 | Strict canonical bounded persistence | Deterministic roundtrips; unknown/duplicate fields including nested native records; expected binding/pin/digest/version substitutions and invalid bounds denied | VERIFIED_LOCAL |
| AC6 | Coverage, regressions, architecture and documentation | Fresh instrumented 25/25, normal application 122/122, all four executable files >=95%, workspace Clippy without warnings, format and architecture/contracts pass | VERIFIED_LOCAL |

The final normal run includes 25 new snapshot cases, 50 existing application
library tests, ten session-contract tests and 37 session-runtime tests. Separate
architecture checks pass 83 tests with one skip; independent frozen-contract
checks pass 27 tests. The architecture guard and `cargo fmt --check` pass.

The [strict counts](EPIC-08.03.02-complete-snapshot-coverage-counts.txt) retain
per-file coverage: module 52/53, validation 409/427, identity 104/105, codec
523/538. `types.rs` contains declarations only and has no executable LLVM line
entry; it is compiled and exercised through its owning modules, without an
invented coverage percentage. Fresh profiles were cleaned after the final
production change; earlier failed and 19-/24-test results are diagnostics only.

The [validation archive](EPIC-08.03.02-complete-snapshot-validation.json.gz)
contains the full final LLVM report, raw qualification/regression/Clippy logs,
commands, source snapshots and hashes. SHA256:
`d5fbf1376154a930f56b2f9fcc3045d6117aa7bc4559c087f38fe468df35d8c9`.
It contains data fixtures and source, not resolved operator credentials.

Three Amigos completion review is scoped and explicit: `connector_gate_review`
independently reviewed the other agents' final production API, validation,
identity and codec and returned SCOPED READY; its authored tests are not
misrepresented as independent test authorship. `connector_phase_resources`
independently reviewed Root's automation/documentation and the core validation,
excluding self-approval of its codec. Root owns final qualification and the
requirement/evidence boundary.

The mandatory next consumer must acquire the entire pinned world from a real
qualified MCP provider and persist/reload Before through the actual PostgreSQL
session owner before any model/effect. Model/action execution, exact consent,
causal independent After/readback and all parent operational/system criteria
remain required. External CI and authorized merge remain separate gates; this
report does not close the issue or its parent.
