# Rust CGSL frontend

EPIC-05.08 (#186) implements the offline frontend in
[`gateway-cgsl`](../crates/gateway-cgsl/src/lib.rs), following the
[CGSL 1.0 specification](cgsl-language-v1.md). Dependencies are limited to
`gateway-domain`, `serde` and `serde_json`; the architecture guard rejects
provider, transport and application dependencies, including build/dev aliases.

`parse(source_id, bytes)` returns a source AST or the first syntax diagnostic.
Every value, member and key has a half-open UTF-8 byte span. Numbers retain their
original tokens, strings are decoded without Unicode normalization, duplicate
decoded keys fail at every depth, and arrays preserve order. `source_location`
resolves decoded member/index paths back to the original AST. `Node::json()` is
an exact-number compact renderer, not the canonical source formatter specified
for subsequent tooling work.

`compile(source_id, bytes)` checks the language version before interpreting
operands and returns `CompiledSource` or ordered semantic diagnostics. Its
`operands` contain the existing Rust domain types under the closed CGSL root
names. Defaults apply only to omitted collections and nullable operands. Domain
task admission owns canonical collection ordering, cross-field validation and
the existing IR digest; CGSL source bytes never become the task digest input.

The result retains the full AST, typed knowledge gaps, typed ambiguity candidates
with provenance, and confidence attachments. Declared unresolved fields survive
as inspection inputs; they do not become invented values or generic omission
errors. Conflicting concrete declarations, invalid operands and duplicate
identities fail admission. Provenance remains caller-supplied until a host
validates it against a captured basis.

`task_shape()` exposes an optional shape-validated `SemanticTaskIR` only for
complete inputs without sidecars. **It is not executable admission.** Reference
existence, authenticated scope, captured lineage, policy and task-specific
completeness still require #187/#188. Sources with any sidecars remain inspection
candidates, even when their attachments are advisory or only confidence. The
frontend never silently discards those attachments to return an IR projection.
No CLI, provider adapter, lookup service or executable handoff is introduced.

## Resource profile

The fixed implementation profile permits at most 1,048,576 source bytes,
100,000 AST values and value nesting depth 64 (root depth zero). Exceeding any
budget returns `CGSL_RESOURCE_LIMIT`; there is no partial success. Input byte
validation precedes AST construction. Identical source bytes and source ID use
the same profile and produce identical results without clock or network inputs.

## Verification

Run:

```sh
cargo test -p gateway-cgsl --locked
cargo clippy -p gateway-cgsl --all-targets --locked -- -D warnings
cargo llvm-cov -p gateway-cgsl --all-targets --locked --ignore-filename-regex 'gateway-domain' --fail-under-lines 95 --json --output-path target/cgsl-coverage.json
python3 scripts/check-dependencies.py
python3 -m unittest discover -s tests/architecture -p test_dependencies.py
```

The coverage selection measures the new crate's production code; the unchanged
domain dependency has its own existing coverage gate. CI runs the same frontend
95% gate and retains its JSON report. Tests exercise both complete frozen sources
against canonical task bytes, all published syntax/semantic rejection vectors,
nonempty sidecars, default equivalence, semantic errors, source spans, Unicode,
exact numbers, resource exhaustion and generated deterministic properties.
The property corpus includes 2,000 seeded JSON/byte cases and every prefix of
the minimal source. These tests establish frontend behavior, not the future
full runtime conformance scope of #193.
