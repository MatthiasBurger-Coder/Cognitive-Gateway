# CG-16: contextual, federated and hybrid retrieval

CG-15 defines immutable retrieval requests, scoped result envelopes and embedding
lineage. `gateway-application::retrieval_pipeline` composes provider adapters over
those contracts. Adapters implement `RetrievalSourceAdapter` for one explicitly
selected source/strategy pair. `federate` calls every registered selected pair, rejects
results with another scope or identity, and retains each unavailable or failed
pair in its failure set. One source failure does not erase successful results.

`HybridCandidate` carries optional document, section, symbol and revision context,
separate lexical and semantic scores, and an exact-identifier signal. Scores use
CG-15 millionths. `fuse_candidates` applies caller-supplied integer weights,
deduplicates whitespace/case-normalized identical content only within the same
scope, source provenance, snapshot, quality and evidence lineage, and sorts exact hits
ahead of all other candidates. Stable source, strategy and fragment IDs settle
ties. An exact hit is the winning fragment when duplicate content also came
from a vector source. The winning candidate keeps its original fragment,
scope, quality, provenance and snapshot; every duplicate contributor remains
available with its own lineage. Duplicate scores are combined by retaining
each available channel's maximum before fusion.

`RetrievalReranker` receives candidates and returns bounded scores keyed by
fragment ID. It cannot return replacement content or metadata. A malformed,
unavailable, incomplete or duplicate score response leaves the deterministic
fusion order intact and reports the unavailable reranker. Successful reranking
retains each score and its model/version. Exact matches remain ahead of
reranked semantic results.
An unavailable configured reranker produces a `Reranker` explanation in a
degraded CG-15 batch while the deterministic fusion order remains usable.

Embedding generation and index compatibility remain behind CG-15's `EmbeddingPort`
and `EmbeddingIndexMetadata::ensure_compatible`. Adapters must partition by scope
and full model identity; source snapshot changes require invalidation/re-embedding.
`gateway-daemon::retrieval` provides a filesystem/Git lexical adapter and a
replaceable, in-memory vector index using `EmbeddingPort`. The repository root
is bound to one scope. The adapter scans text files up to 16 KiB, skips symlinks
and generated directories, computes SHA-256 snapshots from the read bytes, and
records Git HEAD when present. The vector adapter builds, rebuilds and
invalidates its index explicitly. Every search compares current document
snapshots with the indexed sources and reports `StaleIndex` after a change.

`FederatedRetrievalPort` implements the CG-15 `KnowledgeRetrievalPort` for a
single-round plan. It conservatively accounts query and retained-context bytes
against token and context budgets, applies the selected fusion policy, and
publishes only validated `RetrievalBatch` envelopes. Missing optional sources
produce `Degraded` with an explanation; a failed required source returns an
error. The current executor rejects multi-round plans. Retrieval results remain
advisory and cannot grant authority.

The CG-16 coverage gate checks at least 95% measured line coverage in the
application pipeline and outer adapter modules. Reproduce it with:

```sh
CARGO_BUILD_JOBS=1 cargo llvm-cov --workspace --tests --json --output-path target/cg16-workspace-coverage.json
python3 scripts/check-cg16-coverage.py target/cg16-workspace-coverage.json
```
