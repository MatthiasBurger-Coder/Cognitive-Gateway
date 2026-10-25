# Connector credential isolation — EPIC-07.06 #229

## Boundary

`CredentialProfileRef` is an opaque operator identifier in the application port.
`ResolvedCredential` exists only in the outer daemon runtime and has neither
`Debug` nor serde support. Requests, dispatches, observations, provenance,
evidence, context and observer events contain no credential fields.

`ConnectorSecretResolver` is the environment/keyring/secret-manager adapter
boundary. The included `ReloadableCredentialResolver` is an in-memory adapter
for composition and qualification; deployments can implement the same port with
their selected protected store. `CredentialedStdioFactory` resolves on every
connect, rejects unprofiled auth environment values, binds exact connector and
manifest server identity, and checks every mapped tool against the credential's
explicit allowlist. Empty allowlists deny every tool. It injects only recognized
secret-style environment names and refuses operator environment collisions.
Direct `StdioConfig` launches reject auth-like variable names, so credentials
cannot bypass profile resolution through the legacy transport factory.

Reconnect after an admitted shutdown resolves the current profile generation;
rotation does not mutate any domain or evidence record. A denied/missing profile,
authentication failure, scope denial and secret disclosure map to fixed typed
failures. Provider error text is discarded by the protocol parser. Before
normalization, the host scans all nested tool/resource strings for resolved
credential values; an echo fails closed, degrades the connection and clears the
active runtime secret cache. No result containing the canary can reach evidence
or model-visible context.

## Three-Amigos acceptance matrix

| ID | Requirement | Production path/owner | Dependencies and state | Verification/evidence | Status |
| --- | --- | --- | --- | --- | --- |
| C01 | Credentials are referenced opaquely and never serialized into canonical CG state | Application ports → outer resolver | `CredentialProfileRef`; `ResolvedCredential` has no serde/Debug | Compile/API shape and credential boundary tests | VERIFIED at component boundary |
| C02 | Resolve runtime secrets from replaceable external stores | `ConnectorSecretResolver` → `CredentialedStdioFactory` | In-memory reloadable adapter included; OS keyring/secret manager supplied by deployment | Resolver reload/missing-profile tests | VERIFIED at adapter contract; deployment store not qualified |
| C03 | Narrow credentials to connector/server/tool | Credentialed factory and pre-call host gate | Exact manifest identity and explicit allowlist; no wildcard behavior | Wrong server/tool denials | VERIFIED at component boundary |
| C04 | Auth failures are typed and content-free | Resolver errors → `Error` → `ConnectorFailureCode` | Fixed variants; protocol provider text discarded | Authentication classification and missing-profile denial | VERIFIED at component boundary |
| C05 | Secrets cannot reach evidence/context or diagnostics | Host result gate before normalization; bounded observers | Nested result echo rejected; errors are content-free | Tool/resource nested string scanning and redaction canaries | VERIFIED at component boundary |
| C06 | Rotation/reload requires no domain mutation | Resolver resolves each explicit reconnect | Existing admitted shutdown/reconnect lifecycle | Profile generation replacement test | VERIFIED at component boundary |
Review provenance: requirement, architecture, automation and test perspectives
were performed as role passes by one agent; no independent reviewer is claimed.
The #229 boundary is component-qualified, including a real synthetic stdio
subprocess. Live provider deployment and an installed outbound coordinator are
parent EPIC-07 qualification, not claims made by this child slice.

The retained [qualification record](evidence/EPIC-07.06-credentials.json) binds
the component evidence and explicit live-provider limitation.
