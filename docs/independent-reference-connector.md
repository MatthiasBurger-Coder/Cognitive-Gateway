# Independent MCP document conformance adapter — EPIC-07.11 #234

Historical evidence and removed review documents are linked to their archived Git
revision. They are not current acceptance evidence. Generate fresh reports in an
output directory or retain them as CI artifacts.

`cg-conformance-mcp` is a second, independent, read-only Rust MCP server.
It serves one synthetic document and has no GitHub dependency, network access,
filesystem reads, credentials or mutation surface. It proves the plugin boundary
using a different wire vocabulary and payload projection, not a second planner.
It is a bounded conformance reference for the repository's pinned MCP
2025-11-25 closed scalar profile, not certification of the entire MCP standard.

## Configure and run

```sh
cargo build -p gateway-daemon --bin cg-conformance-mcp --locked
target/debug/cg-conformance-mcp describe standard
target/debug/cg-conformance-mcp stdio standard
```

`legacy` is the alternative profile: the tool omits optional `outputSchema`.
Both profiles keep an explicit result schema in the Gateway mapping. Each has
its own discovery/manifest digest; switching profiles requires fresh admission
and discovery. Regenerate the retained `standard.json` / `legacy.json` fixtures
with `describe` when the adapter changes.

Configure the existing `McpConnectorHost` with an absolute executable and working
directory, arguments `["stdio", "standard"]` (or `legacy`) and empty environment.
The exported `config.conformance.documents.v1` is a placeholder: bind and pin the
entire operator-owned configuration through existing scoped admission before
use. `auth.conformance.none` explicitly describes an unauthenticated synthetic
provider; it neither resolves a secret nor claims external authentication.
Only coverage instrumentation is forwarded by the executable tests.

The server accepts `documents.fetch` with exactly `{"key":"guide"}` and returns
`uri`, `body`, `version`. Other tools/keys return a fixed tool failure; no provider
text becomes diagnostics. JSON-RPC frames are capped at 256 KiB, duplicate JSON
keys and malformed frames terminate the session, initialization precedes calls,
and runtime deadlines/cancellation remain with the existing client. No automatic
retry or hidden IO occurs. Missing documents become typed `NotFound`; unknown
error messages and malformed/substituted results fail schema validation.

`DocumentResultNormalizer` binds the expected wire tool, canonical capability,
requested key and document URI, then delegates to `CanonicalResultNormalizer`.
Supply a trusted `AcquisitionPort`; source identity/time/handling metadata are
host-owned. No default acquisition, policy grant or qualified goal-read profile
is provided. Reads produce candidate evidence, never proof of goal completion.

## Comparative architecture and conformance matrix

| Concern | GitHub reference | Independent reference | Shared boundary |
| --- | --- | --- | --- |
| Wire tool | `github.file.read` | `documents.fetch` | MCP tools/call, closed scalar input |
| Inputs | `path`, `ref` | `key` | Typed canonical arguments, explicit mapping |
| Canonical capability in comparison | `document.read` via mapping configuration | `document.read` | Same CG capability and READ/Inspect semantics |
| Source identity | `source.github.file.read` | `source.conformance.document` | SourceId, provenance, scope and inventory lineage |
| Provider output | `object`, `content`, `revision`, `partial` | `uri`, `body`, `version` | Configured scalar projection into `document.content` |
| IO | Credential-capable HTTPS adapter | Static local synthetic data | Existing stdio factory, MCP host and bounded lifecycle |
| Trust/auth | Host collector; controlled HTTP peer in this test | Host collector; no provider credentials | AcquisitionPort, unknown confidence, internal sensitivity |
| Policy | CG-09 controlled current policy facts | Same CG-09 fixture | Denied policy/scope/mutation/unknown capability sends zero requests |

The GitHub comparison changes **only test composition/configuration**: it remaps
`github.file.read` to `document.read` and projects its opaque content through the
existing canonical normalizer. The shipped GitHub-specific normalizer retains
its stricter native capability/URL binding and is separately regression tested.
The comparison does not claim to decode GitHub's base64 file envelope into the
same document text. It proves the shared capability, subject, lineage and context
contracts while retaining provider-specific source identities and opaque text.

Executable positive cases cover standard, legacy and GitHub configurations.
`tests/fixtures/conformance-connector-v1/variations.json` lists negative cases
and expected failure stage/code: version mismatch, deliberately digest-pinned
nested/open schemas (compatibility rejection independently of drift), schema
drift, unknown/duplicate tools, bad output types, foreign object URI, tool errors,
remote errors, malformed JSON and wrong response IDs. An independent Python
wire peer supplies these faults using pinned fixtures. The real synthetic Rust
binary handles positive discovery, calls, lifecycle and missing-document cases.
GitHub HTTPS alone is replaced by a deterministic file response peer.

## Review, traceability and completion boundary

Intake revision: `373eeb0303621ec0c2f6dddf13fd3f46d6faa33a`, clean working tree,
DEVELOPMENT/FULL_PATH. Requirement engineer, architect, automation developer and
tester reviews were role passes by one agent; no independent review is claimed.
Readiness: READY_FOR_WORKFLOW for this synthetic, read-only provider-independence
slice. All runtime/mapping/normalization, CG-09 and scope prerequisites are
implemented. No new core contracts, dependencies, storage or deployment are
required. Planning estimate: 0.5–1 engineer-day implementation and 0.5–1 day
qualification, medium confidence, existing foundations included as reuse.
Rollback removes the optional adapter, configuration and additive qualification.

| ID | Requirement/source | Production path/owner | Dependencies | Verification/evidence | Status |
| --- | --- | --- | --- | --- | --- |
| I01 | #234 second independent adapter | optional daemon server → existing stdio host | Existing MCP runtime | Actual Rust subprocess discovery/call | VERIFIED; retained report |
| I02 | Same runtime/mapping/normalization | DiscoveryMapper → CanonicalConnectorMapping → CanonicalResultNormalizer | Existing v1 contracts | Cross-provider executable contract test | VERIFIED; retained report |
| I03 | Provider behavior stays outside core | daemon adapter + explicit configuration | No core change or dependency | Architecture/dependency guards and source binding | VERIFIED; retained report |
| I04 | Protocol/schema variation | standard/legacy profiles and independent fault peer | Existing fail-closed scalar schema profile | Pinned conformance matrix and exact failure codes | VERIFIED; retained report |
| I05 | Conceptual capability overlap | GitHub file/document → `document.read` | Configuration-only remapping | Same subject, source lineage, batch/fragments contracts | VERIFIED; retained report |
| I06 | Scope, policy, failures, authentication where applicable | Existing CG-09 host, adapter projection | Host-owned policy/acquisition; synthetic auth N/A | Zero-dispatch denials, typed failures, no evidence on errors | VERIFIED; retained report |

Run the EPIC-07.11 command in `tests/scripts/quality-gates.json` for measured
>=95% line coverage per new production file. Also run GitHub/MCP regressions,
formatting, clippy, architecture checks and `git diff --check`.
Results and source/artifact bindings are retained in
[`docs/evidence/EPIC-07.11-independent-reference.json`](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/blob/59f6db7e29478e2903cf69d6e5c7e65632876710/docs/evidence/EPIC-07.11-independent-reference.json).
This qualifies #234's synthetic alternative. It does not qualify a live
Confluence/Jira/filesystem deployment, credentials for such a deployment,
installed shared-session persistence, external mutations, or parent EPIC-07
completion/EPIC-07.12 live effect acceptance.
