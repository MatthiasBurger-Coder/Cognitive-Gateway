# Connector manifest v1 fixtures

`valid-stdio.json` and `valid-http.json` are canonical-content SHA-256 golden
vectors, formatted with whitespace for review. `canonical-stdio.json` pins the
exact compact envelope bytes. HTTP is a declaration, not runtime
support. `invalid-cases.json` enumerates hostile documents, Rust error codes and
structural JSON Schema expectations. `compatibility-matrix.json` pins deterministic
Gateway/transport/lifecycle decisions. All secrets are synthetic canaries.

Rust integration tests read raw fixture bytes (including duplicate JSON fields).
Python contract tests independently validate the schema and recompute golden
SHA-256 digests. Cross-field/digest/uniqueness rules belong to Rust; see the
[contract](../../../docs/connector-manifest-v1.md).

EPIC-07.05 migrates these manifest-format-1.0 vectors to application port revision 2.
The unsupported-port fixture now represents legacy revision 1. Canonical manifest
bytes and derived mapping inventory vectors are regenerated together.
