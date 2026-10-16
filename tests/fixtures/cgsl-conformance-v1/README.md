# CGSL 1.0 conformance corpus

The normative contract is [CGSL 1.0](../../../docs/cgsl-language-v1.md).
This corpus is data, independent of Rust test discovery. Implementations MUST NOT
regenerate expectations from the implementation under test. Frozen canonical task
bytes and SHA-256 values reuse the separately admitted SemanticTaskIR fixtures;
new negation and injection-data vectors were derived independently using JSON key
sorting and SHA-256. Expected canonical CGSL uses the inverse normative root
mapping and two-space formatting. Review fixture changes as contract changes.

`manifest.json` has version `1.0` and cases with stable `id` and relative `source`.
Read sources as bytes (including deliberately invalid UTF-8). Exactly one outcome
applies:

- `code`, `phase`: compilation fails with that first diagnostic and no IR.
- `inspection_only`: compilation retains typed sidecars, with no task projection.
- `canonical_ir`, `sha256`: task canonical bytes and digest must match exactly;
  `<id>.canonical.cgsl` freezes the canonical semantic source bytes including LF.

Canonical IR has no trailing newline. Existing canonical IR fixtures may have a
transport newline; strip it before byte comparison and hashing. Hash the UTF-8
canonical IR, never the CGSL source. `format.cgsl` / `format.expected.cgsl` freeze
formatting of exact number tokens independently of semantic admission. The format
vector is intentionally outside the closed language vocabulary.

`resolution.json` contains full captured context files and a small query profile.
All queries use field `target`, source span `[0,10]`, caller provenance with id
`query`, source id `source`, source reference `query`, no producer/timestamps or
parents, and no subject. Query expressions are:

| Expression | Expected meaning | Query |
| --- | --- | --- |
| `focus` | SERVICE target | Typed conversation focus |
| `alias` | SERVICE target | Exact alias label `payments` |
| `absent` | SERVICE target | Exact canonical entity id `absent` |
| `yesterday` | Temporal | Yesterday relative to anchor id `now` |

Use fixture `at` as the clock. Registry/project lookups return no additional
facts. The trusted test host authenticates and validates captured pins, facts,
query lineage and projections when `host=allow`; `host=deny` rejects admission.
This is an explicit component premise, not production authentication evidence.
`state` is the expected resolution state, or admission error `Scope`/`Admission`.
`code`, when present, is the failed mandatory resolution code. `candidates` freezes complete values and provenance for every admitted decision.
`selected`, when present, is the exact target id; scope, contract, revision and digest must equal
the captured target, with no invented pin. Ambiguous/conflicting states retain
all candidates and cannot supply executable semantics. Replaying each input
must preserve the complete diagnostic/decision envelope, including provenance.

Adapter proposal mutations remain in
[`interpretation-adapters-v1`](../interpretation-adapters-v1/failures.json), with
its frozen model policy. Their executable host/port profile is documented in the
[conformance matrix](../../../docs/cgsl-conformance.md). Clarification resumption
is tested against the same completeness compiler used for initial requests;
this corpus does not define a persisted reply protocol or tool runtime.
