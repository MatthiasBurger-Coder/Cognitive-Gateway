# Invocation resilience v1 fixtures

`diagnostics.json` freezes version, diagnostic and provenance spellings; consumed
by `gateway-application/tests/invocation_resilience.rs`. That integration suite
constructs real admitted IRs from the existing full CG-10 producer fixture and
renders them before each dispatch. Synthetic hosts/providers inject malformed,
valid, transient, hard, uncertain, late, cancelled, over-budget and wrong-profile
responses. Fallback changes exact provider/model/tokenizer pins. Tests assert
actual dispatch counts and retained cumulative accounting, not just error labels.
Synthetic token counters test boundary arithmetic; they do not qualify any
production model tokenizer, SDK interruption or live provider pricing.
