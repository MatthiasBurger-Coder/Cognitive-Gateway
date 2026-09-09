# CG-08.10: canonical artifacts and independent replay validation

The application exports `serialize_resolution`, `parse_resolution` and
`validate_resolution` over explicit immutable snapshots, rules and limits.
Parsing returns a freshly recomputed `CompositionReport`, never a trusted object
constructed merely from serialized claims. Historical reconstruction requires
the corresponding archived canonical snapshot and rules; current applicability
requires the current snapshot. The artifact is not a self-contained registry,
evidence store, policy decision or execution token.

The v1 envelope includes the exact basis, full Plan, explicit alternative groups,
rule fingerprint, all step/plan alternatives and rejection records, canonical
capability contracts and provider fingerprints, Process pins/activities, effective
Skill closure and ordered paths, readiness, diagnostics and the common trace.
Rules are externally supplied and identified by complete canonical fingerprint;
raw Situation evidence is not copied into the artifact. Plan and canonical
contract text remain their existing governance data, not executable instructions.

Validation recomputes discovery, roles, required closure, applicability and global
composition against CG-03/04/06/07 sources. This detects forged providers/classes,
missing Skill dependencies, omitted mandatory work, dangling references, changed
Process digests/revisions and inconsistent outcomes. Full replay also validates
the trace; no result or trace can inject an ALLOW decision. All eight emitted
composition outcomes round-trip, including explicit/empty NO_OP, no-template,
AMBIGUOUS, PARTIAL, MISSING, CONFLICTING, UNSUPPORTED and SEARCH_LIMIT. Invalid input
remains a typed API error rather than a fabricated successful report.

## Canonical semantics and trust

Declared unordered collections are sorted by canonical JSON value: step and
candidate sets, equivalent alternatives, requirements/groups, diagnostics,
rejections, trace nodes/edges and outer inclusion-path collections. Dependencies
remain explicit DAG edges. Dependency-first Skill order, individual path-node
order, tuple positions and other ordered arrays are not normalized away. Duplicate
objects/identities are never deduplicated during validation. Object key order is
nonsemantic; duplicate JSON keys, including nested keys, are rejected by parsing.

The artifact fingerprint is SHA-256 over the canonical envelope without its own
fingerprint field. Transport metadata is outside this envelope; unsupported fields
are rejected, not silently excluded. A matching hash proves consistency only.
Even a deliberately rehashed altered result must equal independent canonical
replay. Changed basis, rule mismatch, hash mismatch, unsupported version, unknown
field, malformed JSON and semantic mismatch are typed failures. Root and nested
unknown governance data cannot be dropped into a successful parsed result.

## Deterministic limits

Caller limits must be positive and cannot exceed 4 MiB encoded bytes, 200000 JSON
nodes or depth 64. JSON parsing also retains serde_json's recursion guard. Source
checks cap Plans at 1024 steps/4096 requirements, combined Agent/Skill definitions
at 4096 and Process definitions at 4096; report expansion is checked before
encoding. Required trace is limited to 10000 nodes; optional detail is omitted
with explicit trace metadata. Canonical tree size includes the fingerprint field,
so serialize/parse use the same bound. No size or parse failure panics or turns
an incomplete search into no-match.

## Evidence

Five integration suites cover all emitted outcomes, repeated byte-identical
serialization, allowed set permutation, forbidden Skill-order changes, partial
and unsupported work, optional/alternative groups, nested providers, invalid
rules/reports, forged contract classes and Process pins, incomplete closures,
duplicate keys/identities, unknown fields, malformed/deep/oversized inputs and
rehashed status/authorization tampering.

```powershell
cargo test -p gateway-application --test resolution_artifact
$env:CARGO_TARGET_DIR='D:\Projects\Cognitive-Gateway\target\cg08'
cargo llvm-cov -p gateway-application --all-targets --json --output-path target/cg08-coverage.json
```

New production module: **318/325 lines (97.85%)**. Workspace tests, formatting,
Clippy with warnings denied and the architecture guard pass.
