# SemanticTaskIR 1.0 reference fixtures

`minimal.json` defines an inspection with absent optional state/process fields.
`performance-analysis.json` populates every field, including CG-06 desired
state, observed/evidence references, an unverified assumption, abstract
capability need and existing domain process reference.

Each fixture has an exact `.canonical.json` encoding (no trailing newline) and
its SHA-256 in `.sha256`. The references describe a synthetic captured basis;
they do not establish real source existence, authorization or executable
runtime support. See [the contract](../../../docs/semantic-task-ir-v1.md).

Rust tests exercise missing/unresolved/ambiguous mandatory values, invalid
references, unsupported versions, duplicates, unknown provider fields,
permutations, typed sets, malformed desired state and failed external binding
validation. Python tests independently validate the schema and frozen bytes.
