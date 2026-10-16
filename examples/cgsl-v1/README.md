# CGSL 1.0 specification examples

These files accompany the [normative grammar](../../docs/cgsl-language-v1.md).
They originated as specification vectors for #185. The #186
[Rust frontend tests](../../crates/gateway-cgsl/tests/frontend.rs) now execute the
mapped and rejected vectors and preserve the incomplete source's sidecars.
All references use synthetic bindings; no registry or source authority is implied.

| Source | Expected result |
| --- | --- |
| `minimal.cgsl` | Complete inspection shape, mapping to the frozen minimal task IR |
| `performance-analysis.cgsl` | All 17 IR-mapped constructs populated, including state, desired state, observations, evidence, assumptions and process |
| `incomplete.cgsl` | Valid source structure with nonempty gap, ambiguity and confidence; target remains ambiguous and compilation blocks |
| `alias.cgsl` | `SEM_UNSUPPORTED_CONSTRUCT` for `objective` |
| `unsupported-version.cgsl` | `VER_UNSUPPORTED_VERSION` for 2.0 |
| `reference-shorthand.cgsl` | `SEM_INVALID_OPERAND`: a string cannot replace the typed target/pin |
| `provider-member.cgsl` | `SEM_INVALID_OPERAND`: output has an unknown provider member |
| `missing-goal.cgsl` | `SEM_MISSING_REQUIRED_FIELD` |
| `duplicate-key.cgsl` | `SYN_DUPLICATE_MEMBER`, including escape-equivalent names |
| `trailing-comma.cgsl` | `SYN_EXPECTED_TOKEN` |
| `comment.cgsl` | `SYN_INVALID_TOKEN`; comments are unsupported |
| `lone-surrogate.cgsl` | `SYN_INVALID_TOKEN`; scalar strings reject unpaired surrogates |

`expectations.json` records the expected stage/code and the frozen IR targets.
The minimal and performance files are also exact canonical semantic-source
byte expectations: sorted keys, two-space indentation and one final LF. Reverse
the specification's root mapping, remove the three empty interpretation arrays,
replace `cgsl` with `schema_version`, then encode compact sorted-key JSON: bytes
must equal the referenced `.canonical.json` and its existing SHA-256 fixture.
Neither these examples nor this mechanical mapping performs reference validation.

The incomplete example's provenance describes a synthetic captured input record.
Its two targets need an authoritative fixture basis containing both exact pins
and the original provenance to establish the expected resolution result.
An absent basis produces `SEM_REFERENCE_UNAVAILABLE`, not permission to guess.
Its confidence attachment applies to the goal, not to target selection.

Resolution counterexamples use the valid minimal source unchanged and vary the
captured basis. This distinguishes syntax/shape from external reference truth:

| Captured basis change | Required diagnostic/outcome |
| --- | --- |
| Target revision 7 exists with a different digest | `SEM_INVALID_REFERENCE`; no latest fallback |
| Target binding belongs to a foreign scope | `SEM_INVALID_REFERENCE`; no cross-project lookup |
| Trusted source-access policy denies the output-schema binding | `SEM_POLICY_INCOMPATIBLE_REFERENCE` |
| Required source validator is unavailable | `SEM_REFERENCE_UNAVAILABLE`; not a successful empty lookup |
| Same-priority explicit context asserts incompatible targets without supersession | `SEM_CONFLICTING_CONTEXT`; preserve both assertions |

Additional lexical counterexamples for implementers: `01`, `+1`, `1.`, `NaN`,
an unescaped newline in a string, a BOM and a second root object must be rejected.
`"// data"`, escaped newlines and paired surrogate escapes are valid strings.
An unknown `task` enum, out-of-range typed integer, duplicate input role and
inline process transition are semantic failures. A `requires` pin resolving to
a concrete Agent is a reference-kind failure even if its envelope parses.

All 20 constructs are exercised across the complete and incomplete sources.
The frontend suite adds resource limits and generated/adversarial input checks;
#193 supplies the [portable component conformance corpus](../../docs/cgsl-conformance.md),
including captured reference bases. Production host/runtime proof remains separate.

See the [reference walkthroughs](../../docs/cgsl-reference-examples.md) for natural-language
intent, every construct, clarification and the planning/process/prompt continuations;
see the [migration guide](../../docs/cgsl-migration.md) for compatibility.
