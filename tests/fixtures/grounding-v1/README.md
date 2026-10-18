# Evidence grounding fixtures v1

`results.json` binds result payloads to the trusted `entity` fact and its current
CG-06 evidence basis. The application integration test parses each complete
output and verifies its grounding. Completed status and maximum model confidence
cannot substitute for support. `diagnostics.json` freezes the version 1 diagnostic
wire names and is checked by round-trip serialization.

The fixture store in `crates/gateway-application/tests/support/grounding_cases.rs`
uses real `KnowledgeProvenance`, `ObservationEvidenceSet`, quality and pin types.
It supplies authenticated-record test doubles, not a deployed evidence backend.
Negative cases alter scope, revision, source, age, sensitivity, trust, fact links,
contradictions, payload correlation and independent effect observations. Tests
also require evidence/claim checks through a real constructed invocation IR,
including rejection when a host reports Passed without a grounding adapter.
