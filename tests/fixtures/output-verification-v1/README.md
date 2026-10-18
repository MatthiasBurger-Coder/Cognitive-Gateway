# Output verification 1.0

`diagnostics.json` freezes the typed rejection codes. The executable positive
and negative fixtures in `crates/gateway-application/tests/output_verification.rs`
reuse the actual immutable CG-10 producer from `prompt-rendering-v1/support.rs`.
They bind schema, task, context, tool inventory and provider negotiation before
parsing or verification. A synthetic trusted store resolves exact entity pins
and supplies deterministic constraint/rule reports. It can revoke admission
while checks run, reject references, return stale reports, fail each constraint
axis or omit rule checks. No production authority or live grounding is claimed.

The wire envelope requires `schema_version`, `invocation`, `result`, `references`
and `tools`. Unknown/duplicate fields, incomplete JSON and unsupported versions
are denied. References use JSON pointers into the result and exact scoped pins.
Tool proposals carry an exact capability pin and typed JSON arguments. Proposals
are data and are never dispatched by this fixture or the verification service.
