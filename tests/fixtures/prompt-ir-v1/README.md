# Prompt/Invocation IR v1 fixtures

`denials.json` freezes stable admission diagnostic spellings. The positive fixture
reuses `../prompt-rendering-v1/support.rs`: actual canonical task, context planning,
example selection, evidence and CG-10 handoff, including hostile data. Tests in
`gateway-application/tests/prompt_ir.rs` bind it to #261 prepared contracts and tool
definitions, authenticate it with a synthetic trusted record and exercise admission,
rendering and proposal-only transport. Store mutations are negative fixtures for
stale task/context/schema/verification/plan/example/tool/authority input. No fixture
pin authenticates a production artifact or grants real authority.
