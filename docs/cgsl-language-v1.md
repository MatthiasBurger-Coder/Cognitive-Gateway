# CGSL 1.0 grammar, serialization and diagnostics

## Status and authority

This is the normative CGSL 1.0 language specification, completed and indexed for
EPIC-05.16 #194 in the [implementer guide](cgsl-implementer-guide.md). It originated in EPIC-05.07
[#185](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/185).
MUST and MUST NOT express conformance requirements. It selects a surface syntax
for the [20 canonical constructs](cgsl-scope-and-vocabulary.md), reusing
[SemanticTaskIR 1.0](semantic-task-ir-v1.md), [Interpretation Context](interpretation-context.md),
[resolution](semantic-resolution-contracts.md), [epistemic](semantic-epistemic-contract.md)
and [output/verification](semantic-output-contracts.md) contracts.

This is the language specification. #186 implements the
[offline Rust frontend](cgsl-frontend.md); #187 owns resolution, #188
completeness and #193 full conformance.
Existing task JSON readers MUST NOT be described as CGSL readers. Historical
indentation-based examples in epic sketches are informative, not alternate syntax.

## Surface choice

A `.cgsl` file is one strict UTF-8 JSON object. JSON provides explicit boundaries,
familiar tooling, one string-escaping system and no indentation-dependent meaning.
The finite root names and typed operands make this a language profile, not an
arbitrary JSON-to-IR passthrough. No model, prompt template, network, YAML parser,
macro expansion or natural-language inference is needed to parse or format it.

The root contains `"cgsl": "1.0"` and `"id": <TaskId string>`. These are document
metadata, not additional semantic constructs. The version member may occur in
any position; a reader MUST inspect it before interpreting operands. `schema_version`
is not a root alias. CGSL 1.0 maps to task schema 1.0 explicitly; referenced
contract versions remain independent. Malformed version spelling is
`VER_INVALID_VERSION`; well-formed unsupported major or minor versions are
`VER_UNSUPPORTED_VERSION`, with no subset parsing or downgrade. Version strings
have two nonnegative decimal integers separated by a dot, without leading zeros.
The compatibility and deprecation discipline of the task specification applies
independently to CGSL. There are no deprecated constructs in 1.0.

## Normative grammar and lexical rules

EBNF uses `=` for definition, `|` for alternatives, comma for concatenation,
braces for repetition and brackets for optionality. Quoted terminals denote
literal characters. `EOF` is end of input; `scalar` is a Unicode scalar value.
Whitespace is allowed only where `ws` appears. Lexical rules below complete the
`unescaped` and `escape` productions; there are no implicit tokens.

```ebnf
document = ws, object, ws, EOF ;
object = "{", ws, [member, {ws, ",", ws, member}], ws, "}" ;
member = string, ws, ":", ws, value ;
array = "[", ws, [value, {ws, ",", ws, value}], ws, "]" ;
value = object | array | string | number | "true" | "false" | "null" ;
string = '"', {unescaped | escape}, '"' ;
number = ["-"], integer, [".", digit, {digit}], [exponent] ;
integer = "0" | nonzero, {digit} ;
exponent = ("e" | "E"), ["+" | "-"], digit, {digit} ;
digit = "0" | nonzero ;
nonzero = "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" ;
ws = { U+0020 | U+0009 | U+000A | U+000D } ;
```

`unescaped` is any scalar at or above U+0020 except quote and backslash.
`escape` is backslash followed by exactly one of `"`, `\`, `/`, `b`, `f`, `n`,
`r`, `t`, or `u` plus exactly four hexadecimal digits. A high-surrogate `\u`
escape MUST immediately be followed by a low-surrogate `\u` escape; together
they decode to one scalar. Lone surrogates, invalid UTF-8 and a leading BOM
are lexical errors. No Unicode normalization is performed. Keys compare by
decoded scalar sequences, so `"task"` and `"\u0074ask"` are the same key.

Comments are deliberately unsupported: `#`, `//` and `/* */` outside strings
are syntax errors. Comment-looking text inside strings is ordinary data.
Objects are the only blocks; arrays are the only collection syntax. There are
no bare identifiers, URI tokens, interpolation, includes, anchors, operators,
multiline strings, trailing commas, implicit units, NaN or Infinity. CRLF and
LF both parse as whitespace. A second document after the root is an error.

The parser MUST retain number tokens exactly (not first convert to binary64),
decoded strings, ordered array elements, object member spans and source byte
offsets. It MUST reject duplicate decoded keys at every nesting depth before
constructing a map. Object member order is not meaningful. Numeric domain
bounds and typed-literal rules are semantic checks, not lexical guessing.

The first character distinguishes all value alternatives. Quoted keys and
mandatory colons/commas delimit members; delimiters are inert inside strings.
Number components have fixed order, and escape decoding is unique. These
properties give every syntactically valid document one parse. Duplicate-key
rejection additionally prevents different map implementations choosing winners.

## Closed construct and operand registry

Only metadata `cgsl`, `id` and the following root construct names are allowed.
Unknown root names, wrong case and aliases such as `objective`, `inputs`,
`current_state`, `constraints`, `capabilities`, `workflow` and `prompt` produce
`SEM_UNSUPPORTED_CONSTRUCT`. Parsing the JSON structure does not admit these
names into the language. Nested domain field names are operands, not aliases
for root constructs. Unknown nested members fail typed admission.

| CGSL construct | Task 1.0 field / exact operand contract |
| --- | --- |
| `task` | `task_type`: string `ANALYZE`, `ANALYZE_PERFORMANCE`, `INSPECT`, `CREATE`, `MODIFY`, `VERIFY` |
| `goal` | `goal`: `SemanticGoal` object with `outcome`, `description` |
| `target` | `target`: `SemanticTarget`, kind and full pinned reference |
| `input` | `inputs`: array of `SemanticTaskInput` role/value objects |
| `context` | `context_refs`: array of pinned context references |
| `state` | `current_state`: pinned observed-state reference or null |
| `desired_state` | `desired_state`: existing CG-06 `DesiredState` object or null |
| `observation` | `observations`: array of pinned observation references |
| `history` | `history`: array of pinned historical references |
| `constraint` | `constraints`: array of existing CG-02 constraints |
| `requires` | `capability_requirements`: array of pinned abstract capability references |
| `evidence` | `evidence_refs`: array of pinned evidence references |
| `knowledge_gap` | Array of source gap records defined below; interpretation sidecar only |
| `ambiguity` | Array of source candidate records defined below; interpretation sidecar only |
| `assumption` | `assumptions`: array of existing `SemanticAssumption` objects |
| `confidence` | Array of source confidence attachments defined below; interpretation sidecar only |
| `policy` | `policy_refs`: array of pinned policy references |
| `output` | `output_contract`: existing object containing a pinned `schema` |
| `verification` | `verification_contract`: existing object containing `checks` |
| `process` | `process`: existing definition `id`, positive integer `version`, SHA-256 `digest`, or null |

For mapped operands, the closed wire shapes in
[`semantic-task.schema.json`](../schemas/semantic-task.schema.json) and domain
admission rules are normative. Source-to-wire mapping changes only the listed
root names; it MUST NOT reinterpret nested data. This includes exact-decimal
coefficient/scale, typed literal tags, confidence quantization and CG-06 predicates.
Output definitions remain external pinned definitions, not inline provider schemas.

`cgsl` and `id` are required for a source document. Task compilation additionally
requires `task`, `goal`, `target`, `output` and `verification`. Missing mandatory
meaning yields `SEM_MISSING_REQUIRED_FIELD`; inspection may retain the candidate.
Absent collection constructs mean `[]`; absent `state`, `desired_state` and
`process` mean null. Null is invalid for all other constructs. These defaults
declare absence only and cannot satisfy task-specific completeness requirements.
Explicit empty and omitted collections compile identically. Repeated construct
members are duplicate-key errors, never implicit collection concatenation.

When an omitted field has a gap or ambiguity declaration, retain that declared
resolution state through interpretation validation instead of replacing it with
a generic missing-field error. It still blocks mandatory completeness. A bare
omission without such a declaration produces `SEM_MISSING_REQUIRED_FIELD`.

A pinned reference is exactly `id`, `scope`, `contract`, `contract_version`,
`revision`, `digest`, all strings. Digests are 64 lowercase hexadecimal digits.
No shorthand string, relative lookup, alias expansion, mutable latest pointer
or inherited scope is permitted. The task scope is its target scope; all bindings
must agree with it and the authenticated request. Process scope is projected
using the existing task contract. Syntax and shape validation do not prove the
referenced content exists. Offline compilation needs a supplied authoritative
captured basis; an unavailable basis blocks handoff, not parsing or formatting.

## Interpretation-only source operands

This section defines source encodings, not a new persistent snapshot wire
contract. Every object is closed and every listed member is required.
Provenance arrays contain complete existing CG-06 `Provenance` wire objects as
defined by [`observation.rs`](../crates/gateway-domain/src/observation.rs).
They must be nonempty, identity-unique and validated against the captured basis.
Caller-authored provenance alone is not trusted lineage.

* A `knowledge_gap` element has `subject`, `missing_field`, `reason`, `provenance`,
  `required_resolution_class`, `requirement`. It maps one-to-one to `KnowledgeGap`.
  `subject` is `{ "kind": "TARGET", "value": <SemanticTarget> }`,
  `{ "kind": "PATH", "value": <SubjectPath string> }`, or
  `{ "kind": "REQUEST", "value": <host-bound ReferenceId string> }`.
  `missing_field` is a CG-06 `SubjectPath`; reasons, routes and requirement enums
  are exactly those in the resolution contract. A request subject cannot invent
  a target. A gap reporting ambiguity/conflict does not replace its candidates.
* An `ambiguity` element has `field`, `requirement`, `candidates`. `field` names
  one root construct with an IR mapping; candidate `value` is its complete
  operand type. Each candidate is `{ "value": ..., "provenance": [...] }` and
  maps to `ResolutionCandidate<T>`. At least two distinct, type-compatible
  values are required. `requirement` is `MANDATORY` or `ADVISORY`. A non-absent
  concrete declaration of the same field is incompatible with this declaration
  and yields `SEM_CONFLICTING_CONTEXT`; it does not silently choose a candidate.
  Candidate order conveys no preference. Incompatible assertions in context
  are CONFLICTING, not merely AMBIGUOUS; no new `conflict` construct is introduced.
* A `confidence` element has `field`, `value`, `basis`. `field` names one present
  mapped root construct; `value` is the existing CG-06 `Confidence` wire shape
  and `basis` is a full pinned reference. It attaches `InterpretationConfidence<T>`
  to that complete operand. There is at most one attachment per field. It does
  not override confidence inside an assumption or source record, verify a claim,
  or select a candidate. Root confidence and premise confidence describe different
  subjects. Missing subjects are `SEM_INVALID_ATTACHMENT`.

Duplicate gap `(subject, missing_field)` identities or ambiguity fields are
`SEM_DUPLICATE_IDENTITY`. All attachments remain outside SemanticTaskIR and
its digest. Mandatory gaps, ambiguity and conflict block executable compilation;
advisory cases may survive only with the affected operand omitted and no required
dependency, as specified by #182. Writers MUST preserve sidecars when saving an
interpretation document. Exporting only IR is explicitly a projection and cannot
be advertised as lossless source roundtrip. Clarification and policy authority
remain host-owned; no declaration of advisory status can bypass that boundary.

## Canonical formatting and semantic serialization

There are two different artifacts with explicit roundtrip guarantees:

1. **Source formatter `F(AST)`** operates without reference resolution and can
   format semantically invalid or incomplete but syntactically valid documents.
   It sorts every object's decoded keys by Unicode scalar order, uses two spaces
   per nesting level, one member/array element per line, `: ` between key/value,
   no trailing commas/spaces, `{}` and `[]` for empty containers, LF line endings
   and exactly one final LF. Array order and number token spelling are preserved.
   Strings use the task canonical encoder's escaping rules (short control escapes,
   lowercase hex for other controls, literal UTF-8 otherwise, no escaped slash).
   No field is inserted, removed or renamed. Object key order and escape spelling
   are not AST meaning; number spelling is retained until typed admission.
   `parse(F(parse(s))) = parse(s)` ignoring member order and spans, and
   `F(parse(F(parse(s)))) = F(parse(s))` byte-for-byte. Consequently `1e0` and
   `1.0` need not format identically; formatting is not semantic normalization.
2. **Canonical semantic source `S(IR)`** first obtains the admitted task's
   `to_canonical_json()` value, replaces `schema_version` with `cgsl: "1.0"`,
   applies the inverse root mapping above, adds empty `knowledge_gap`, `ambiguity`
   and `confidence` arrays, then applies `F`. All mapped collections and nullable
   fields are emitted. Typed numbers and set order therefore come from existing
   domain canonicalization, never arbitrary source order or binary64 conversion.
   Given the same admitted reference basis, `compile(S(IR)).IR = IR`, including
   identical task canonical JSON and digest, and `S(compile(S(IR)).IR) = S(IR)`.
   Source attachments and original formatting are not reconstructible from IR.

Canonical executable interchange remains the existing compact task JSON with
no trailing newline, including its fixed collection/check ordering, decimal
precision, null fields and SHA-256 rules. CGSL source bytes are not the task
digest input. No new task fields, digest algorithm or domain sorting rules are
introduced. A changed source spelling may have the same IR digest; changed
meaning, IDs or pins must not be hidden by formatting. No serializer may invent
missing values or emit executable IR after failed admission.

## Diagnostic taxonomy and reporting contract

Codes are stable machine identifiers; messages are explanatory and not parsed.
Syntax diagnostics describe malformed token streams/structure. Type, name,
cardinality, domain or ownership errors are semantic even when discovered during
typed decoding. Resolution requires the captured basis. Version checking is
separate from all three. Existing #182 codes retain their meanings.

| Phase | Code | Trigger |
| --- | --- | --- |
| LEX | `SYN_INVALID_ENCODING` | Invalid UTF-8 or BOM |
| LEX | `SYN_INVALID_TOKEN` | Bad number, escape, raw control, comment or unrecognized token |
| PARSE | `SYN_EXPECTED_TOKEN` | Missing delimiter/value or unexpected EOF |
| PARSE | `SYN_DUPLICATE_MEMBER` | Duplicate decoded object key, at any depth |
| PARSE | `SYN_TRAILING_CONTENT` | Content after the root object |
| VERSION | `VER_INVALID_VERSION` | Missing, nonstring or malformed `cgsl` |
| VERSION | `VER_UNSUPPORTED_VERSION` | Unsupported well-formed language version |
| SEMANTIC | `SEM_UNSUPPORTED_CONSTRUCT` | Unknown/alias/wrong-case root construct |
| SEMANTIC | `SEM_INVALID_OPERAND` | Wrong type, unknown nested field/enum, invalid domain value |
| SEMANTIC | `SEM_DUPLICATE_IDENTITY` | Duplicate collection identity |
| SEMANTIC | `SEM_INVALID_ATTACHMENT` | Invalid confidence/gap/candidate attachment |
| SEMANTIC | `SEM_BOUNDARY_VIOLATION` | Well-shaped declaration used for process execution, provider binding or authority escalation |
| SEMANTIC | `SEM_MISSING_REQUIRED_FIELD` | Missing mandatory meaning |
| RESOLUTION | `SEM_UNKNOWN_ENTITY` | No eligible entity on captured basis |
| RESOLUTION | `SEM_AMBIGUOUS_ENTITY`, `SEM_AMBIGUOUS_REFERENCE` | Multiple eligible meanings |
| RESOLUTION | `SEM_CONFLICTING_CONTEXT` | Incompatible assertions, including explicit declaration/candidate conflict |
| RESOLUTION | `SEM_STALE_REFERENCE` | Stale implicit context pin excluded |
| RESOLUTION | `SEM_INVALID_REFERENCE` | Invalid explicit pin, including stale, digest/kind/scope mismatch |
| RESOLUTION | `SEM_UNSUPPORTED_INTENT`, `SEM_INSUFFICIENT_CONFIDENCE` | Existing resolution-contract reasons |
| RESOLUTION | `SEM_INVALID_RESOLUTION_CONTRACT` | Malformed resolution state/payload combination |
| RESOLUTION | `SEM_POLICY_INCOMPATIBLE_REFERENCE` | Trusted policy/source-access owner denies reference use |
| RESOLUTION | `SEM_REFERENCE_UNAVAILABLE` | Required captured source/validator unavailable; not proof of absence |
| VERSION | `VER_INCOMPATIBLE_CONTRACT` | Unsupported referenced contract or target schema version |
| LIMIT | `CGSL_RESOURCE_LIMIT` | Published parser/compiler resource budget exceeded |

Each diagnostic carries `code`, `phase`, `severity` (`ERROR` or `WARNING`),
`message`, `source_id`, `span` (half-open zero-based UTF-8 byte offsets), `path`
(array of decoded member strings and integer array indexes), and `related`
(source/span/path locations). Missing members use a zero-width span at the
containing object's closing brace; EOF errors use the input byte length.
Duplicate members point to the later key with the first key as related location.
Invalid UTF-8 spans address original bytes. Optional displayed line/column
positions are one-based, count Unicode scalars and treat CRLF as one line break.
The machine contract uses byte offsets, not display columns.

Resolution diagnostics additionally retain the request/context revision, field,
resolution state, candidates, provenance, considered tiers and exclusions in
the #187 envelope. Host disclosure rules govern exposure; messages MUST NOT
leak foreign source contents. Diagnostics MUST NOT become executable operands.

Implementations MUST stop semantic processing on syntax or version failure.
The minimum syntax contract reports the first error by byte position; recovery
may add errors but cannot produce admitted IR. Semantic validation precedes
reference lookup, which precedes completeness/handoff validation. A malformed
operand gets `SEM_INVALID_OPERAND` before any attempted reference lookup.
An unsupported task kind gets `SEM_INVALID_OPERAND`; well-shaped unsupported
intent in interpretation retains #182's `SEM_UNSUPPORTED_INTENT`.
Within a completed phase, order diagnostics by source ID, span start, span end,
code and path (strings by scalar order, indexes numerically, strings before
indexes; shorter equal prefixes first). Identical code/path/span entries merge
their related locations, sorted by the same location order. No downstream
cascade is required for invalid operands. All syntax, version and domain
invalidity errors block compilation. Only eligible advisory incompleteness
may be WARNING; caller confidence cannot downgrade an error.

Resource limits must be explicit in the implementation profile and produce a
limit diagnostic, never truncation, partial success or an invented language
syntax restriction. Given the same source, profile and captured basis, outcomes
must be deterministic. Runtime policy evaluation and current-clock retrieval
are not hidden formatter inputs.

## Examples and acceptance trace

[`examples/cgsl-v1/`](../examples/cgsl-v1/) contains complete sources, expected
formatter bytes, semantic mappings to existing frozen task fixtures and staged
counterexamples. Synthetic pins demonstrate shape, not a production registry.
The performance example exercises all mapped constructs; the incomplete example
exercises nonempty gap, ambiguity and confidence operands and deliberately blocks
executable compilation. No example requires an LLM.

| #185 criterion | Specification evidence |
| --- | --- |
| Every valid source has one parse | Complete EBNF, scalar/escape rules, explicit delimiters, duplicate rejection |
| No semantic synonyms | Closed 20-name registry, separate metadata and explicit alias rejection |
| Deterministic canonical serialization | `F` and `S` algorithms, existing canonical IR encoder, frozen examples |
| Syntax versus semantic/resolution diagnostics | Phase/code taxonomy, spans, validation ordering and counterexamples |
| All core constructs covered | Complete mapped performance source and interpretation-only source |
| Human/machine/model-independent authoring | Strict JSON, explicit pins and typed operands; offline parsing/formatting |

Reviewed against #179–#184 and ADR-015 as a single-agent specification review.
This establishes the #185 design contract only. Automated example checks can
establish fixture integrity and mapping, but parser, compiler and resolver
conformance must be demonstrated by #186/#187/#193 before runtime claims.

Validation on 2026-10-16: both complete examples were mechanically mapped back
to their frozen task canonical bytes and SHA-256 digests; canonical source
formatting, 20-construct coverage, incomplete-example structure, lexical
counterexamples and local links were checked. The existing three task contract
tests passed with `uv run --no-project --with jsonschema python -m unittest
discover -s tests/contracts -p test_semantic_task_contract.py`. This checks
existing JSON Schema and IR fixtures, not the proposed CGSL compiler. No
production code changed, so production-code coverage is not applicable.

## Implementer companions

The [reference walkthroughs](cgsl-reference-examples.md) cover all 20 constructs
and connect formal documents to resolution, clarification, planning, process and
prompt boundaries. The [migration guide](cgsl-migration.md) defines the supported
entry paths and applies the normative versioning rules without implicit downgrade.
The [conformance guide](cgsl-conformance.md) identifies frozen expectations,
reproduction commands and currently reserved diagnostic emitters.
