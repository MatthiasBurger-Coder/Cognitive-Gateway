# CGSL migration and compatibility

The [language specification](cgsl-language-v1.md),
[task versioning contract](semantic-task-ir-v1.md) and
[conformance compatibility matrix](cgsl-conformance.md) are normative.
CGSL source, task schema, interpretation capture and referenced contracts have
independent versions. Current source and task readers support exactly **1.0**.
There is no automatic negotiation, downgrade or permissive subset reader.

## Choose the existing entry point

| Existing input | Migration path | Preserve and validate |
| --- | --- | --- |
| CG-06 Intent/context/situation | Keep the existing structured planning API, or use `map_cg06_inputs` then `plan_semantic_task` | Exact desired-state predicates and source document; explicitly supply context/state pins; reject conflicting task predicates |
| SemanticTaskIR 1.0 JSON | Use `SemanticTaskIR::from_json`, then trusted reference/source/completeness admission | `schema_version`, all canonical fields and independent pins; JSON shape is insufficient for execution |
| Formal CGSL | Use `gateway_cgsl::compile`; inspect sidecars before requesting a task shape | All mandatory meaning and original source/provenance; application host admits references and requirements |
| Natural-language request | Build an authenticated Interpretation Context, resolve fields and use completeness compilation | Request/task/scope/revision correlation; recapture clarification replies through the same validation path |
| Historical indentation-based epic sketch | Explicitly author a strict JSON CGSL document using the closed registry | No YAML parser or alias conversion; supply typed values and full references rather than guessing |

Existing `DeclarativePlanningApplication` and `DeclarativeResolutionApplication`
remain supported. No production CGSL CLI command is introduced. The source mapper
does not infer predicates from goal text. Follow the [planning adapter contract](semantic-planning-boundaries.md)
for the host's mandatory source/capability authentication. Keep `SemanticPlanHandoff`
beside the resolved plan: extracting Plan alone loses output, verification,
evidence and other task obligations.

## Source and task conversion

To save an interpretation document, parse and use `format_source(&Document)`.
This retains gap, ambiguity and confidence attachments, array order and number
tokens. It can format syntactically valid incomplete documents; it cannot make
them executable.

To export admitted task meaning, use `serialize(&SemanticTaskIR)` for canonical
CGSL or `to_canonical_json()` for compact task JSON. Use the specification's exact
root mapping (`task` to `task_type`, `input` to `inputs`, and so on); changing only
the version key is insufficient. Semantic serialization emits empty interpretation
sidecars and cannot recover original source attachments. Hash canonical task
JSON, with no final newline, using `content_digest()`; never hash pretty CGSL as
the task identity. Recompilation requires the same admitted reference basis.

## Evolve versions without changing old meaning

| Change | Required treatment |
| --- | --- |
| New optional field with an absence preserving old meaning | Minor version with explicit defaults, canonical representation and versioned fixtures; older readers reject the new version until upgraded |
| New enum value or opt-in feature | Gate behind the declared schema version and supported reader; do not accept it under 1.0 |
| Renamed/removed field, new mandatory meaning, changed authority/scope or changed canonical bytes for an existing schema | New major version and explicit validated migration |
| Deprecation | Record first version, rationale, replacement and migration vectors; retain original meaning/encoding throughout the supported major |
| Unsupported major or minor | Fail closed; CGSL returns `VER_UNSUPPORTED_VERSION`; task admission rejects unsupported schema versions |
| Malformed CGSL version | `VER_INVALID_VERSION`; stop before operand interpretation |

There are no deprecated constructs or members in 1.0. Backward compatibility
means a newer reader explicitly supports older schemas with their original
meaning and bytes; it does not make an old reader forward compatible. An older
version may be emitted only by an explicit lossless projection that preserves
all mandatory semantics and verification obligations.

For an intentional migration, retain original source/task and exact digest,
record source/target versions and mapping, authenticate the captured reference
basis, validate the new schema and invariants, rerun completeness and downstream
admission, and record the resulting canonical bytes/digest. Repinning a revision
is a semantic change, not a formatting repair. Do not reuse prior authorization
or evidence as proof for changed semantics. Rollback restores the old artifact
and its bindings, then revalidates current admission rather than trusting stale
Process/Policy decisions. Update Rust admission, JSON Schema, fixtures and
documentation together; compare both positive vectors and denied paths from the
[conformance guide](cgsl-conformance.md).
