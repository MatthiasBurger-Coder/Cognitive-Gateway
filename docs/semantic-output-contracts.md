# Output and verification contracts

EPIC-05.06 [#184](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/184)
defines `semantic_output::{OutputContract, VerificationContract}` in the domain.
They elaborate the existing SemanticTaskIR 1.0 descriptors without changing its
wire format or canonical digests. The task still carries a pinned schema and
finite verification checks. Definition documents carry explicit version `1.0`;
unsupported versions fail admission. Any change to a definition requires a new
revision and digest; incompatible type/rule semantics require a new version.

## Admission and ownership

The host loads the output definition under the task's complete schema binding
(identity, scope, contract/version, revision and digest). `validate_contracts`
requires exact binding equality, supported definition versions and exactly one
rule for every task check, with no omissions, additions or duplicates. Each
rule has pinned rule and verifier references in the same scope. Bindings are
declarations: the authoritative host must resolve them, validate contract kinds
and supported versions, and verify content digests before use. A missing,
changed or unavailable definition blocks admission; there is no latest fallback.
The fixtures use synthetic binding digests and do not prove registry resolution.

The definition [JSON Schema](../schemas/semantic-output-contract.schema.json)
checks wire shape. Rust admission additionally checks task-relative invariants.
Deserializing a definition alone is not admission. Provider configuration and
unknown type members are rejected. Reference resolution and verifier execution
remain host-owned, as do evidence acquisition, policy and execution authority.

## Output type and projection rules

`OutputType` is a closed recursive type algebra. Objects declare named typed
fields with explicit `required` flags. Unknown properties are rejected at every
object depth; optional means absent is allowed, not null. Arrays are homogeneous,
may be empty, and retain order. No implicit string/number coercion is permitted.

| Domain type | JSON Schema 2020-12 | Rust projection |
| --- | --- | --- |
| STRING | `type: string` | `String` |
| BOOLEAN | `type: boolean` | `bool` |
| INTEGER | `type: integer` | integral `serde_json::Number`; bounded integer types require range checks |
| NUMBER | `type: number` | `serde_json::Number` |
| ARRAY | `type: array`, recursive `items` | `Vec<T>` |
| OBJECT | recursive `properties`, sorted `required`, `additionalProperties: false` | named serde struct with `deny_unknown_fields` |

Optional fields project to `Option<T>` with omission on serialization. Validate
the JSON value before typed deserialization because serde's `Option<T>` also
accepts null. Rename attributes preserve arbitrary JSON property names in Rust.
Integer validation follows JSON Schema semantics (including `1.0`); adapters
targeting Rust integer types must normalize integral JSON numbers and reject
overflow. Numerical precision is bounded by the existing serde_json number
representation. No general Rust source-code generator is introduced.

`OutputContract::json_schema` produces the deterministic provider-neutral
projection. `OutputType::accepts` validates parsed JSON against the same algebra.
The [reference output schema](../schemas/artifact-inspection-output.schema.json)
is checked against the Rust projection and independently with a JSON Schema
validator. Raw transport parsing must reject malformed or duplicate-member JSON
before this value-level API; a parsed map cannot recover overwritten members.

Structured-output adapters consume this projection outside the domain. They
may add transport envelopes, but MUST NOT silently drop constraints or change
optional fields to nullable ones. An unsupported provider schema subset requires
explicit adapter rejection or a documented reversible encoding followed by
canonical validation. Provider formatting never establishes output validity.

## Verification and failure semantics

Each `VerificationRule` associates a task check with versioned, revision-pinned
rule and verifier references. The baseline rule meanings are:

| Check | Required proof |
| --- | --- |
| OUTPUT_SCHEMA_VALID | Actual result satisfies the bound output shape; computed locally |
| EVIDENCE_REQUIRED | Required evidence references resolve and satisfy CG-06/EPIC-05.05 admissibility and provenance requirements |
| ALL_CLAIMS_SUPPORTED | Every claim requiring support has admissible supporting evidence; hypotheses and assumptions are not facts |
| NO_UNRESOLVED_REFERENCE | Every mandatory result reference resolves uniquely under captured scope/revision |

The task admission rules still require schema and reference checks; claim support
also requires evidence verification. Rule definitions specify task-specific
coverage (such as claim/evidence locations). These semantics do not invent a
second evidence, process, or authority model.

`assess_result` accepts execution success/failure and trusted host reports for
the exact admitted task and result revision. Reports are never model assertions.
The caller must correlate reports with the pinned verifier/rule, task digest and
result digest; this small domain function does not authenticate report provenance.
It computes schema validity itself, then uses the following precedence:

1. Invalid contract binding/version: `ContractError`; no result admission.
2. Execution error: `EXECUTION_FAILED`, regardless of any partial payload.
3. Successful execution with nonconforming output: `INVALID_OUTPUT`.
4. An undeclared report (including externally reported schema validity): `UNVERIFIED`.
5. A required verifier reports failure: `VERIFICATION_FAILED`.
6. Missing or indeterminate reports: `UNVERIFIED`.
7. All required checks pass: `VALID`.

`VALID` means those declared checks passed, not authorization, universal truth,
or permission to promote knowledge. A verifier timeout/unavailable dependency is
indeterminate, not a successful check or a task execution failure.

Future ResultIR owns payload/artifact references, execution status and task/result
digests. Future VerificationIR owns individual rule/verifier bindings, outcomes,
diagnostics and evidence references correlated to those digests. They can reuse
these enums and contracts without adding runtime/provider fields to SemanticTaskIR.
This slice supplies definition, projection and assessment semantics; it does not
implement those later IRs, a verifier registry, or provider/runtime integration.

## Verification

Rust integration tests cover projections, recursive type validation, closed wire
objects, task binding drift, unsupported versions, missing/duplicate checks,
cross-scope references, and every result state. Independent Python tests validate
definition/result fixtures against Draft 2020-12. The semantic coverage gate now
includes `semantic_output.rs` at the existing 95% threshold.

Run `cargo test -p gateway-domain --locked`,
`python -m unittest discover -s tests/contracts`, and the domain coverage gate in
`scripts/quality-gates.json`. Existing task 1.0 fixtures remain unchanged.
