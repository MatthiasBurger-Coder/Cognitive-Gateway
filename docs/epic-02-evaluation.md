# CG-20 evaluation and curated learning handoff

`gateway-domain::evaluation` defines evaluator version 1. The checked-in
`tests/fixtures/epic02-v0.2/golden.json` pins nine positive and negative cases,
the external project scope, source/index/embedding/model/estimator/strategy
versions, expected sufficiency, numerical baseline and minimum thresholds.
The model version `none` means no model is invoked. Reordered cases produce the
same report. Precision and recall are micro-aggregated over cases with a
nonempty relevant set. No-match contributes a zero precision and recall
observation. Empty denominators are missing metrics and block release.
Token efficiency is justified selected tokens divided by used dynamic-context
tokens; zero-use cases are not applicable to that ratio. All rates use integer
millionths; missing values, invalid versions, duplicate
cases/results, wrong scopes and arithmetic overflow fail explicitly.
Latency and cost are reported as measured fixture values, not quality scores.

The golden file is a **synthetic regression dataset**. Its expected and
observed fields test deterministic evaluator and threshold behavior; they are
not a claim of external retrieval service measurements. The CG-20D replay in
`gateway-application/tests/support/closed_loop.rs` separately exercises the
actual bounded retrieval, context and closed-loop components with fake ports.
Its `integration-baseline.json` pins a separate all-objective threshold for
eight measured positive, outage and adversarial assessments from that replay.
The complete gate writes the evaluator report to `cg20-evaluation.json` and
retains the full test and coverage logs in the evidence bundle. Optional
model-assisted judgments are outside this release decision.

`gateway-application::evaluation::profile` reports defect IDs and denominators
for missing labels/outcomes, repeated source snapshots, normalized near
duplicates, conflict, stale age, trust, schema and sensitive inline content.
The profile counts sensitivity classes and reports a numeric age range only
when records exist. It neither changes data nor decides truth or eligibility.

`export_snapshot` takes an explicit scope, time and source revision. It exports
only currently learning-eligible memory references, validation and label
bases, source snapshot/digest, time, outcome and sensitivity. Confidential and
secret outcome text is omitted. Inline payloads are omitted; external payloads
remain references. Items are sorted and the
manifest digest includes all exported fields. `revalidate_snapshot` checks the
digest and current memory revision, eligibility, source and record metadata
before use. Invalidation, supersession, expiry and forgetting revoke old
exports. A tombstone cannot restore forgotten bytes. Consumers remain
responsible for revalidation at their own use time. Training, dataset splitting
and cross-project aggregation remain outside this handoff.

The policy in the versioned fixture requires exact baseline parity for this
first deterministic dataset. A proposed baseline change must review both
the fixture and release policy. A green judge score cannot replace a missing
or failing objective metric. The existing quality runner adds a CG-20 file
coverage gate at 95% without lowering any prior threshold.
