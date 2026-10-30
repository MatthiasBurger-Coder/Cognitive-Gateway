# CGSL implementer guide

Historical evidence and removed review documents are linked to their archived Git
revision. They are not current acceptance evidence. Generate fresh reports in an
output directory or retain them as CI artifacts.

This is the entry point for implementing and independently reproducing CGSL 1.0
and SemanticTaskIR 1.0. It completes the documentation scope of EPIC-05.16 #194.
The specifications below define the contract; issue history is not required.
MUST/MUST NOT in those specifications are conformance requirements. This guide
does not introduce another syntax, wire schema or authority model.

## Read and implement in this order

| Step | Normative contract | Implementation responsibility |
| --- | --- | --- |
| 1 | [Scope and finite vocabulary](cgsl-scope-and-vocabulary.md) | Task meaning only; reuse CG-02/06 identities, state and evidence |
| 2 | [Language 1.0](cgsl-language-v1.md) | Strict UTF-8 JSON, closed operands, exact numbers, AST spans, ordered diagnostics, formatter and semantic serializer |
| 3 | [SemanticTaskIR 1.0](semantic-task-ir-v1.md) and [wire schema](../schemas/semantic-task.schema.json) | Typed invariants, canonical bytes and SHA-256, independently versioned exact references |
| 4 | [Interpretation Context](interpretation-context.md) and [deterministic resolution](deterministic-interpretation-resolution.md) | Authenticate a captured request/scope/revision; apply deterministic precedence and retain candidate provenance |
| 5 | [Resolution states](semantic-resolution-contracts.md), [epistemic contract](semantic-epistemic-contract.md), [completeness](semantic-completeness.md) | Preserve gaps/conflicts/ambiguity; derive all mandatory requirements; clarify only the unresolved dependency frontier |
| 6 | [Output and verification](semantic-output-contracts.md) | Authenticate external schema/check definitions; keep obligations distinct from successful verification |
| 7 | [Planning](semantic-planning-boundaries.md), [Process IR](semantic-process-boundary.md), [Prompt IR](prompt-ir-handoff-boundary.md) | Retain complete semantics alongside downstream artifacts; enforce each owner's admission boundary |
| 8 | [Migration](cgsl-migration.md), [conformance](cgsl-conformance.md), [reference walkthroughs](cgsl-reference-examples.md) | Run portable frozen vectors and denied paths; distinguish source formatting from task identity |

## Compilation stages and admission

```mermaid
flowchart TD
    F[Formal CGSL bytes] --> P[Parse UTF-8 and JSON to source AST]
    P --> V[Check CGSL version and closed typed operands]
    V --> S[Shape or interpretation candidate with sidecars]
    N[Natural language and relevant captured context] --> R[Deterministic contextual resolution]
    S --> R
    R --> C[Authenticate manifest and check completeness]
    C --> Q[Unresolved frontier: host clarification]
    Q --> N
    C --> A[Compile and revalidate exact task references]
    A --> H[Validated SemanticTaskIR and source basis]
    H --> PL[Existing CG-07 and CG-08]
    H --> CC[Context planning and CG-10 boundary]
```

The arrows describe composition responsibilities. They do not assert a shipped
natural-language CLI or a single production coordinator. The Rust frontend
`gateway_cgsl::compile` stops at typed shape/sidecars. `task_shape()` is not
executable admission. The application resolver and completeness compiler need
trusted host ports and a captured authoritative basis. An unavailable source is
not proof that an entity does not exist. Source-authored provenance and high
confidence cannot authenticate a reference or grant consent.

Facts, observations, evidence, inferences, hypotheses and assumptions retain
their classifications and original provenance. Freshness, scope and source
eligibility are checked before a candidate can satisfy a mandatory requirement.
Confidence cannot break ambiguity or turn an assumption into a fact. Interpretation
sidecars are retained by source formatting and deliberately excluded from the
task digest; discarding them is a projection, not a lossless source roundtrip.

## Delivered component and future runtime boundary

As inspected on 2026-10-17, the offline frontend, canonical formatter/serializer,
typed task/output/resolution contracts, deterministic resolver, completeness and
optional proposal adapters exist. The CG-06/07/08 adapter and Process IR boundary
have executable component proofs. The portable conformance suite freezes source,
canonical IR, digest and resolution expectations.

The Prompt IR boundary has two offline reference renderings. Its supplemental
planning/example records are test-only consumer views. The [canonical context requirements/source planner](context-requirement-planning.md)
(#285) now has a real CG-10 producer handoff and offline conformance proof.
The [canonical dynamic example selector](dynamic-example-selection.md) (#286)
and its dedicated CG-10 examples handoff are implemented and have offline producer
conformance proof. Production Prompt/Invocation IR admission (#263), persisted clarification hosting (#275) and
full runtime proof (#279) remain separate integration work. No complete arbitrary
natural-language interpreter, real model invocation or external effect is
qualified by these component tests. See [architecture state](current-architecture-state.md).

## Supported natural-language inspection profile

`gateway_application::inspection_language` implements the bounded English
`artifact-inspection/1.0` profile. Invoke
`CompletenessValidator::compile_inspection_request(context, text, output, at)`
with the original text, an admitted context and a host-authenticated pinned output
contract. Supported complete sentences are `Inspect artifact <canonical-id>` and
`Inspect it`, optionally ending in one period and surrounded by whitespace.
Case, internal spacing and identifier spelling are exact. Other operations,
negation, extra clauses and instruction-like suffixes return `UnsupportedRequest`.
Input is bounded to 4096 UTF-8 bytes and its exact SHA-256 must match the admitted
input pin before interpretation. Spans address the original bytes.

The profile derives `INSPECT`, goal `inspect-artifact` / `Report the artifact
properties`, and the mandatory schema/reference verification checks. It derives
the target query from the text, rather than accepting a preinterpreted query or
caller-selected target. Explicit IDs use the bounded permitted registry/project
ports; `it` uses the normal deterministic context tiers. A missing focus produces
a correlated clarification plan; equal eligible focus targets preserve ambiguity;
an unknown explicit ID never falls back to the conversation target.

The `InspectionLanguageHost` must authenticate the output definition and reject
any task/schema needing additional mandatory semantics outside this profile.
Existing host query/manifest validation and reference validation remain required.
`inspection_revision` binds the profile, original text, output and capture digest.
The normal completeness compiler constructs and revalidates the final task; a
clarification answer enters this same text API with a fresh authenticated capture.
The result retains original-input provenance and resolution traces alongside IR.
No permission, policy decision, process transition or execution is produced.

Run `cargo test -p gateway-application --test inspection_language --locked`.
The actual original sentences compile to the exact frozen minimal formal task;
negative cases cover unsupported/hostile text, changed input, missing/ambiguous
targets, unknown explicit IDs, foreign scope, expired/changed basis, rejected
manifest/output/reference and fresh clarification resumption. This is a public
application service with component-host evidence. A general interpreter and an
installed natural-language client remain separate; neither is required to use
this supported deterministic profile.

## Reproduce without an installed runtime

Run from the checkout root with the repository Rust toolchain:

```sh
cargo test -p gateway-cgsl --all-targets --locked
cargo test -p gateway-application --test interpretation --test completeness --test semantic_planning --locked
cargo test -p gateway-domain --test semantic_task_v1 --test semantic_resolution --test semantic_output --locked
cargo test -p gateway-context --test prompt_handoff_reference --locked
```

These tests require no model, network, database or tool credentials. A second
implementation should consume the [data corpus and host/query profile](../tests/fixtures/cgsl-conformance-v1/README.md)
directly and compare frozen bytes, digests, diagnostic classes and complete
resolution candidates. Passing the Rust tests establishes reference-component
behavior. The [independent valid-source comparison](cgsl-conformance.md#independent-valid-source-conformance-witness)
additionally qualifies IR/digest equivalence with a separately implemented
offline reader. Neither establishes a deployed host.

The [EPIC-05.16 review and evidence record](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/blob/59f6db7e29478e2903cf69d6e5c7e65632876710/docs/epic-05.16-documentation-review.md)
maps documentation acceptance to source inspection and executed regressions.
The subsequent [parent acceptance record](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/blob/59f6db7e29478e2903cf69d6e5c7e65632876710/docs/evidence/EPIC-05-parent-acceptance.json)
reconciles all 29 parent criteria, the late additions and the three audit findings
against its exact source hashes, retained logs and measured coverage.
