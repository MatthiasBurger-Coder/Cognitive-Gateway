# Complete Before persistence and historical inspection

[#407](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/407)
owns the session journal and PostgreSQL boundary for complete Before data.
[#408](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/408)
must supply the actual installed MCP acquisition. This document describes the
contract; passing runtime qualification must be recorded separately.

The existing coordinator accepts a complete Before through its trusted readonly
preparation facet. The legacy goal-satisfied and artifact paths retain their
behavior. A Before is observation data: it cannot become an invocation,
authorization grant, effect receipt, After or goal-verification receipt.

The coordinator validates the exact capture owner/project/scope, session/run,
revision, step and dispatch against its real reservation and fence. The phase
must be Before. The typed reference retains the snapshot digest, complete
universe/profile/runtime pins, native compiled-context digest, actual record
reference, commit revision and validation time. Capture and commit revisions
are distinct. The validation time comes from the coordinator host clock after
capture and current guards; a producer cannot supply or refresh it.

A dedicated CompleteBefore record and its checkpoint reference are written in
one owned PostgreSQL append. A failed append cannot leave an orphan or a false
Prepared result. Before returning `Runnable/Prepared`, the coordinator loads the
actual stored bytes and verifies owner, kind, hash, full pin and checkpoint
linkage, current freshness and the current fence/deadline. No final evidence is
manufactured. The existing record and operator-frame bounds apply to the entire
encoded envelope, including overhead; oversized data is rejected whole.

## Historical data and current eligibility

The immutable journal reference and record hash authenticate the original
capture binding, expected universe/profile definitions and validation time.
The historical loader reuses the complete-snapshot codec without accepting
expected definitions from unchecked payload. This loader belongs to #407 and
does not depend on #408's live provider-definition loader.

Historical inspection and marker recovery validate data at the authenticated
original validation instant. They preserve the original runtime/context pins,
capture time and observations. Data remains inspectable after its freshness
window expires. Historical loading does not establish current eligibility;
future model/effect use requires actual current time, current definitions and
fresh authority in its own admission path.

Marker-only continuation remains explicitly unsupported while the model
consumer is absent. It must not append a revision, consume a new command,
reserve resources or call a provider. Exact duplicate commands retain their
original replay semantics. Recovery never reacquires data or refreshes a
timestamp. An interrupted uncommitted claim is resolved from its durable
state after lease expiry. An uncertain commit is reconciled from PostgreSQL,
without another provider read.

## Private operator inspection

The existing private `cg-local --operation session.authority` path gains the
explicit `inspect-before` decision while retaining its strict three-field
request. The readonly branch runs before authority issuance and uses the same
authenticated owner, selected database and scope protections. It creates no
revision, lease, budget reservation or current grant.

The returned inner contract is `cg.complete-before-inspection/1.0`. It contains
the original Before reference, capture binding, commit revision, validation
instant and canonical snapshot data, explicitly as historical data. It is not
projected as `final_evidence` or an artifact. Frozen MCP/session v2 schemas are
unchanged. This private inspection path does not promise a new public MCP
snapshot resource.

## Qualification boundary

The #407 component tests must use native CG-06 snapshot data, genuine
coordinator reservations/claims and actual PostgreSQL transactions. Required
cases cover immutable roundtrip, foreign/corrupt/orphaned records, failed or
uncertain commit, stale fences, cancellation, restart, ageing and pure
inspection/recovery. Legacy behavior and every materially changed executable
production file retain their existing regression and 95% coverage gates.

The coordinator's generic host/repository implementations need combined
lifecycle tests with the same concrete types. LLVM's function-group summary
uses the maximum covered-line count across instantiations, rather than the
union displayed in the annotated source. The
[LLVM implementation](https://github.com/llvm/llvm-project/blob/main/llvm/tools/llvm-cov/CoverageSummaryInfo.cpp)
and [line-summary merge](https://github.com/llvm/llvm-project/blob/main/llvm/tools/llvm-cov/CoverageSummaryInfo.h)
define that aggregation. Qualification retains the native summary and exact
95% threshold; it does not substitute the annotated-source count.

Those tests cannot prove that an MCP server exported a complete world. #408
must demonstrate that through the shipped binary and a qualified real
provider. Model proposals, authorized actions, consent, independent After,
two-step workflows and the remaining #274/#271 criteria stay open.
