# CG-20A Knowledge Plane hardening

Issue: [#199](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/199).

The context compiler keeps external fragments in `dynamic` and derives `stable`
authority only from the validated execution projection. Retrieved text, memory,
and caller input are data. Tool adapters must use these external data contracts
for tool output. JSON serialization escapes external bytes;
provider renderers must retain the same structural separation.

## Requirement and verification matrix

| Requirement sentence | Contract and implementation | Verification | Status |
| --- | --- | --- | --- |
| Imperative text and forged policy markup cannot alter selected authority or capabilities. | `gateway-context/src/compiled.rs` constructs `stable` from `ExecutionContextIR`; `ContextFragment::external` admits only data kinds. | `forged_envelopes_remain_escaped_external_data`; existing policy and context application tests. | Implemented at semantic JSON boundary. |
| Retrieved results cannot claim canonical authority even when a request includes that trust class. | `gateway-domain/src/retrieval_plane/result.rs` rejects `CanonicalReference` on returned fragments. | `retrieved_content_cannot_claim_canonical_authority_even_if_requested`. | Implemented. |
| Compaction must retain trust, sensitivity, provenance, scope, step, and representation. | `gateway-context/src/budgeted.rs` compares the compacted fragment with every source, including inline/reference representation. | `compaction_preserves_lineage_and_rejects_metadata_relabeling`; `compaction_cannot_turn_a_reference_into_inline_content`. | Implemented. |
| Retrieval and evidence metadata that contradicts an authenticated source must be rejected. | `ContextFragment::knowledge` and `ContextFragment::evidence` compare provenance and evidence links before assembly. | `retrieval_adapter_rejects_proposed_metadata_that_relabels_the_source`; `evidence_context_uses_captured_provenance_and_omits_raw_evidence`. | Implemented at context ingress. |
| Authenticated quarantine decisions must exclude optional fragments and fail closed for required fragments. | `select_context_with_exclusions` and `compile_budgeted_step_with_exclusions` reject required IDs and any compaction artifact containing an excluded source. | `quarantine_excludes_optional_data_and_blocks_required_data_or_compaction`; budgeted application integration. | Implemented as an explicit host hook. |
| Contaminated and unsatisfied evidence cannot be counted as sufficient; audit reasons must be stable. | `gateway-domain/src/retrieval_plane/sufficiency.rs` rejects contaminated candidates, retains per-fragment rejection findings and exposes `SufficiencyFinding::as_str`. Retrieval and selection reasons also have stable codes. | Existing sufficiency tests; `sufficiency_audit_reasons_are_stable_codes_without_source_text`; `retrieval_explanations_discard_adapter_supplied_secret_text`. | Implemented for typed boundaries. |
| Sensitive external fragment content and descriptive metadata must be redacted for a disclosure limited handoff. | `ContextDisclosurePolicy` is supplied by the host and `CompiledContext::to_json_with_policy` redacts content, source, revision, evidence links, rationale, and validation when the maximum sensitivity is exceeded or external content is excluded. `CompiledStep::to_json_with_policy` also omits caller input and derived task fields when requested. | `disclosure_policy_redacts_sensitive_data_and_source_strings`; `original_input_keeps_inline_bytes_and_reference_semantics`. | Implemented for explicit export. |
| Audit logs must not copy raw external or caller payloads. | `ClosedLoop::audit` and `to_json` retain references, counts and stable codes; execution contexts use the redacted export. `RetrievalBatch::new` replaces adapter-supplied explanation prose with stable reason codes. | `observes_success_and_retains_complete_deterministic_audit`; `retrieval_explanations_discard_adapter_supplied_secret_text`. | Implemented for application audit. |
| Malformed or forged serialized data cannot be reinterpreted as Gateway authority. | Typed fragment construction and JSON escaping in `CompiledContext::to_json`. | `forged_envelopes_remain_escaped_external_data`. | Semantic JSON verified; provider rendering remains #178. |
| Benign instruction-like documentation remains usable when policy permits. | No text detector authorizes or excludes content; selection uses typed metadata and policy. | `forged_envelopes_remain_escaped_external_data`; existing context and retrieval tests. | Implemented at semantic boundary. |

The host authenticates `ContextDisclosurePolicy`. An untrusted document cannot
select its own disclosure level. The existing compiled-context `to_json()`
methods serialize the complete semantic envelope for authorized runtime handoff.
The disclosure-limited export can omit all external content and derived task
fields, including the execution projection. The Closed Loop uses that form for
its audit by default. Reference IDs use the domain's restricted identifier
alphabet and should be opaque, not secret-bearing names.

`SufficiencyFinding::Contaminated` is a structural finding supplied by the
evidence boundary. Optional instruction-like-text detectors can add diagnostics
but cannot authorize a fragment or override typed trust and policy checks.

## Validation

Run the repository gates from the root:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
bash scripts/check-architecture.sh
python3 scripts/quality-gate.py
```
