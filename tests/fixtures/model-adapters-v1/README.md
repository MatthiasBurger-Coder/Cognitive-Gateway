# Model adapter conformance v1

`local-response.json` and `chat-response.json` express the same unverified result
through two different wire envelopes. `negative-responses.json` and Rust fixture
mutations cover malformed output, mismatched identity, incomplete responses,
usage, denial and output limits. `diagnostics.json` lists stable outer diagnostic
codes. Tests render the actual hostile-evidence CG-10 handoff before dispatch.

`live-profile.example.json` contains placeholders and a credential environment
variable name. Copy it outside the repository, replace exact model pins and
runtime settings, and use the ignored live test documented in
`docs/model-provider-adapters.md`. No live model or semantic capability is
qualified by these deterministic fixtures.
