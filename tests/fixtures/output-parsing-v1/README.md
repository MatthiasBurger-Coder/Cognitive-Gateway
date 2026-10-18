# Output parsing 1.0 fixtures

`results.json` contains raw result lexemes and expected stable diagnostics. The
Rust integration test inserts them into an envelope from the actual CG-10/admitted
Invocation IR fixture. Duplicate escaped keys and truncated strings remain raw
text so value deserialization cannot erase the defect. Stream interruption,
boundary sizes, inventory drift and repair accounting use that same producer in
`gateway-application/tests/output_verification.rs`.
