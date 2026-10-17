# Semantic rendering v1 fixtures

`support.rs` composes actual #286 selection, #285 planning, CG-10 handoff and
#261 model preparation. `hostile.txt` is an untrusted example input containing
XML/role/template delimiters, quotes, a literal escape, Unicode and a newline.
The same content also enters a mandatory observed-evidence fragment with source,
trust and sensitivity metadata. Both production outer renderers must preserve
both copies in their separate user-data sections.

`gateway-daemon/tests/prompt_rendering.rs` supplies the explicitly synthetic
`fixture-byte` exact tokenizer and a conservative estimated variant. Counts are
not estimates for any real LLM. Positive cases cover both layouts and count
modes, exact fits and complete decoded section equality. Negative cases cover
stale context/model/profile/template/tool pins, missing tools, unknown renderers,
unavailable tokenizers, mismatched count bindings/provenance, zero/overflowing
counts and context overflow. No live authorization, tokenizer or provider is
implied by fixture pins or successful preparation.
