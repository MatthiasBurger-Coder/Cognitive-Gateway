# #407 Before journal qualification

**LOCAL_QUALIFIED — READY_FOR_REVIEW.** This is native-data component
qualification with actual PostgreSQL and the real private `cg-local` inspector.
It does not establish actual MCP snapshot acquisition, a model invocation,
an external effect, After or goal success. #408 and full #274/#271 acceptance
remain mandatory.

The [structured report](EPIC-08.03.03-before-journal-review.json) binds 34 source
and prerequisite files against base `a5b87db`. The final
[validation archive](EPIC-08.03.03-before-journal-validation.json.gz) contains
those source snapshots, complete native LLVM data and raw logs. SHA256:
`3e5eea1f6f0f5d05a8261d930549b71b8178b7deb2629ed525afe12ac3fcc96d`.

| ID | Requirement / owner | Actual evidence | Status |
| --- | --- | --- | --- |
| AC1 | Existing coordinator owns genuine reservation, claim and Before-only state | Native compiled-context/CG06 fixtures enter through the actual preparation cursor; Runnable/Prepared with no final evidence/model/effect | VERIFIED_LOCAL |
| AC2 | Exact immutable reference and atomic owned PG record | Actual PG all-three-profile roundtrip and current-byte reload; separate capture/commit revisions, full pins and trusted time | VERIFIED_LOCAL |
| AC3 | Invalid data cannot be persisted/disclosed | Foreign scope/runtime, corrupt owner/kind/hash, orphaned and oversized records; actual PG trigger rejects publishing update without a partial record | VERIFIED_LOCAL |
| AC4 | Historical data never refreshes current eligibility | Real `cg-local` reads aged data after runtime removal, unchanged entire PG journal; recognized credential canary denies the whole projection | VERIFIED_LOCAL |
| AC5 | Recovery/commands/cancel/expiry do not reacquire | Exact duplicate rejection, unsupported marker Continue, lease/freshness boundaries, storage callbacks and real competing-coordinator cancel | VERIFIED_LOCAL |
| AC6 | Legacy compatibility, coverage, contracts and docs | 840 all-target tests; additive legacy default-host case; all nine executable owners >=95%; Clippy/format/architecture/frozen contracts pass | VERIFIED_LOCAL |

The final clean production epoch passed **840 tests across 74 test binaries,
with four explicitly ignored live-profile cases**. Its native summary initially
reported 771/816 coordinator lines. The mixed-host tests had stopped exercising
the original trait defaults. An additive legacy-host case now invokes the real
default registration, completes a native artifact and confirms the default
connector facet is absent. The complete session-runtime suite then passed
**40/40 both normally and instrumented**. All nine production-file hashes stayed
unchanged throughout this epoch. The complete 840-test run was not rerun after
that additive test and is not relabelled as an 841-test full run.

The final native [strict counts](EPIC-08.03.03-before-journal-coverage-counts.txt)
pass every owner independently: coordinator 784/816 (96.08%), journal 274/285
(96.14%), Before codec 201/205 (98.05%), Before reference 65/66 (98.48%),
readonly preparation 37/37, snapshot codec 574/589, snapshot module 52/53,
local sessions 594/610 and PG store 650/662. The existing complete-snapshot
validation/identity gate also passes. No thresholds or LLVM counts were rewritten.
The [contract documentation](../complete-before-journal.md) explains LLVM's
generic-instantiation aggregation and why the tests use combined concrete hosts.

Seven separate normal real-PG/private-inspector tests passed; the full
instrumented epoch also runs them against freshly prebuilt CLI binaries.
Workspace/all-target Clippy with denied warnings and formatting pass after the
additive test. Architecture passes 83 tests with one skip; frozen contracts pass
27; the architecture guard passes. Public v2 schemas have zero diff.

Independent reviews are scoped: `connector_gate_review` reviewed foreign core,
codecs, automation and genuine mixed-host tests; `connector_phase_resources`
reviewed foreign PG/inspector/core and automation; `connector_budget_adapter`
reviewed the foreign codecs. Authors did not self-approve their production code.
Root owns the additive default test and final evidence reconciliation.

The earlier failed coverage and mixed-profile diagnostics are not acceptance.
The archive explicitly distinguishes the clean full run and its additive test.
External CI and authorized merge remain separate gates; this report does not
close #407 or a parent. #408 must demonstrate complete-world acquisition from
an actual qualified MCP provider through the installed runtime.
