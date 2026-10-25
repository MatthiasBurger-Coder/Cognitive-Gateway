# Connector identity and manifest v1

EPIC-07.02 #225 implements the foundation contract under [ADR-022](adr/ADR-022-outbound-connector-ownership.md)
and the [connector architecture](mcp-connector-runtime.md). Rust in
[`gateway_application::connector_manifest`](../crates/gateway-application/src/connector_manifest.rs)
owns deterministic validation, canonical encoding, compatibility and identity-set
checks. The [JSON Schema](../schemas/connector-manifest-v1.schema.json) defines the
strict wire shape. These descriptions grant no discovery, transport, policy,
credential, trust or execution authority. The [protocol runtime](mcp-client-runtime.md)
and [deterministic mapping](connector-mapping-v1.md) implement discovery and dispatch
behind required admission and normalization ports.

## Identity and ownership

A manifest envelope contains `manifest` and `digest`. Its definition contains:

| Field | Meaning and validation |
| --- | --- |
| `manifest_version` | Exact contract revision `1.0`; no inferred migration |
| `connector_id` | CG-configured stable connector instance, independent of release, transport or process/session |
| `server_id` | CG-configured server identity, never accepted solely from server-reported identity |
| `version`, `server_version` | Connector definition release and expected server release; unsigned 32-bit `major`, `minor`, `patch` components |
| `protocol` | Exact pinned architecture baseline: `mcp`, `2025-11-25`, JSON-RPC `2.0` |
| `transport` | `stdio` or `streamable_http` declaration; runtime qualification is separately required |
| `configuration_ref` | Opaque outer configuration association; no executable, arguments, environment, URL, headers or credentials |
| `inventory` | At most 256 declared tool/resource/prompt entries; complete empty inventory is valid |
| `profiles` | Opaque authentication, trust and sensitivity profile references; never assertions of permission/trust |
| `health` | Opaque health contract reference plus interval/timeout seconds, `1 <= timeout <= interval <= 86400` |
| `lifecycle` | Configured `enabled`, `disabled` or `deprecated`; live connection/health state belongs to runtime |
| `deprecation` | Required for deprecated state only: replacement connector ID and inclusive Gateway minor-version sunset |
| `compatibility` | Exact application port revision `2`, Gateway major and minimum minor |

All opaque IDs/references match `[a-z][a-z0-9.-]{0,95}` exactly, without case folding,
trimming or Unicode normalization. They are lookup keys, never URIs or secret
values. Server identity is distinct from protocol `serverInfo` display metadata.
Reinstallation, reconnect or inventory ordering does not rename the configured
identity. Endpoint, server binary and credentials must be independently bound to
this configured identity by #226/#229/#232; a content hash is not authentication.

`validate_manifest_set` checks one proposed admission-scope snapshot atomically.
A repeated connector ID and identical digest is `DuplicateIdentity`; a changed
digest for that connector, or reuse of one server ID by another connector, is
`ConflictingIdentity`. It publishes no partial index. Distinct servers use distinct
IDs; cross-scope sharing requires explicit admission under #232. An upgrade replaces
an existing snapshot through admission; old and new versions cannot coexist under
one connector ID in the same proposed set.

## Inventory and profile references

Each entry carries `kind`, `name`, `schema_digest`, `capability_mapping_ref` and
`source_mapping_ref`. Names use `[A-Za-z0-9_.-]{1,96}` and are case sensitive.
`schema_digest` is lowercase hex SHA-256 of the complete versioned canonical schema
bundle for that entry, including input/output contracts where applicable.
[Mapping v1](connector-mapping-v1.md) owns schema profile validation and verifies
the complete discovered entry bundle's digest.
Duplicate `(kind, name)` keys are rejected even if every other field matches;
conflicting schema or mapping metadata cannot silently replace an entry.

Tools require an explicit capability mapping reference. Resources require a source
mapping reference and may also carry a capability mapping. Prompts carry neither:
they are inspectable inventory only. An absent optional reference is normalized to
JSON `null`. Inventory metadata is descriptive and never authorizes prompt execution.

Mapping/profile/health references must resolve to immutable, explicitly versioned
CG-owned records; unresolved, mutable or conflicting references deny runtime
admission. #227 owns resolution and operation metadata, including operation class,
side effects, idempotency support, retry safety, and independently admitted read-back
capabilities. Neither a reference string nor a server hint establishes retry safety.
#229 resolves authentication outside the core. #230 evaluates policy/consent, and
#231 implements health and reconciliation. This slice validates reference syntax
and binding within the manifest, without inventing those downstream services.

The manifest has no generic free-form extension/description/payload slots. Every
object rejects unknown fields, including credential/token/header/environment fields.
Credentials belong only to outer configuration/secret storage. Opaque IDs must never
be populated with secrets; validation checks syntax and allowed fields, not the
meaning of an arbitrary otherwise valid identifier. Failures return content-free
`ManifestError` variants without echoing untrusted input. Transport/authentication
errors and redaction remain #229's responsibility.

## Canonical encoding and digests

The versioned encoding is **CG Connector Manifest Canonical JSON v1**, limited to
this schema's ASCII strings, integers, arrays, objects and null. It is not a general
JSON canonicalization implementation.

1. Reject malformed JSON, duplicate object fields, unknown fields, unsupported
   versions/enums, missing required metadata and definitions/envelopes over
   262144 input bytes. Every required object is strict, including nested objects.
2. Validate all semantic invariants. Normalize absent optional references and
   deprecation to null. Sort inventory by kind (`tool`, `resource`, `prompt`), then
   ASCII name; reject duplicates before publishing.
3. Encode recursively lexically sorted object keys as compact UTF-8 JSON. There
   is no BOM, trailing newline, whitespace, floats or optional omission. Array
   order is the normalized order; numbers are ordinary unsigned decimal integers.
4. Compute lowercase hex SHA-256 over the canonical **definition**, excluding the
   envelope and its `digest`. Include identity, both releases, protocol, transport,
   all schema/mapping/profile/configuration references, health/lifecycle and
   compatibility metadata. No timestamp or random/session value participates.
5. Encode the full envelope with the same object-key rules. On load recompute and
   require an exact digest match. Never replace a supplied wrong digest silently.

`from_definition_json` is an authoring helper: it validates a strict unsigned
definition and constructs the digest. `from_json` loads and verifies an envelope.
Only those constructors produce the immutable `ConnectorManifest`; the public
`ConnectorManifestDefinition` is an unvalidated authoring DTO. `definition()` exposes
a shared reference for typed inspection; `canonical_json()` returns the canonical
envelope. Serialization, load and reserialization are byte stable. Input key order,
whitespace, inventory permutation and absent-vs-null optional references normalize
to the same snapshot. Any substantive metadata change changes the digest.

The manifest digest is not the discovered `ConnectorInventory.revision`. #227 must
bind that revision to the manifest digest, actual negotiated protocol, complete
wire schema inventory and resolved canonical mapping content as required by #224.
This prevents a declared snapshot from standing in for actual complete discovery.

## Compatibility, deprecation and migration

Compatibility uses trusted `ConnectorCompatibilityProfile` data, never model output.
Checks have fixed precedence:

| Condition | Decision |
| --- | --- |
| Contract/protocol/RPC/port version unsupported | Parse fails `UnsupportedVersion` |
| Gateway major differs or minor is below minimum | `GatewayVersion` |
| Declared transport absent from runtime's qualified allowlist | `Transport` |
| Lifecycle disabled | `Disabled` |
| Deprecated and Gateway minor >= sunset | `Sunset` |
| Deprecated and before sunset | `CompatibleDeprecated` |
| Otherwise | `Compatible` |

A replacement ID must be valid and different from this connector; sunset cannot
precede the minimum Gateway minor. No wall clock or model judgment affects the
result. The caller must not treat `CompatibleDeprecated` as a permission grant.
A Streamable HTTP example demonstrates representation only; it does not claim an
implemented transport. Actual handshake must match the pinned protocol/server
release and independently pass admission.

The three release numbers are stable SemVer components: major marks breaking
changes, minor adds backward-compatible metadata/capabilities and patch corrects
compatible metadata. Pre-release and build labels are outside v1. Contract version,
connector release, server release, port revision and MCP version are independent.
`upgrade_from` is deliberately conservative:

| Candidate vs previous | Decision |
| --- | --- |
| Different connector/server identity, connector major, or release rollback | `Incompatible` |
| Same canonical digest | `Unchanged` |
| Same connector release with different digest | `ConflictingRelease` |
| Higher minor/patch under same major | `ReadmissionRequired` |

Even a patch or profile/reference change requires compatibility, reference resolution,
complete discovery, fresh scope/policy bindings and admission before use. The helper
does not certify that arbitrary schema changes obey SemVer. #227/#232 must compare
the resolved contracts and enforce those rules. Server release changes require a
new connector release and re-admission. No permission broadening follows a version
number. A rollback requires a new explicit admission decision; automatic in-place
rollback is denied by this helper.

Future contract revisions require a new schema, explicit translator and compatibility
fixtures. V1 rejects unknown mandatory **and optional** fields; there is no extension
bag. An optional field can be absent only where the v1 schema allows it. Preserve
old snapshots/digests for replay; migrate a copy, validate and rehash it, record the
old/new revisions and invalidate existing inventories/dispatch bindings. Never
reinterpret a historical digest under new encoding rules or silently downgrade.

## Traceability and verification

| ID / #225 criterion | Implementation | Executed evidence source |
| --- | --- | --- |
| M01 Stable deterministic identity | Configured IDs, canonical hashing | Golden vectors; all inventory permutations; 64 release/reference round trips |
| M02 Unknown mandatory fields / versions fail closed | Strict nested DTOs, version checks, bounded parser | Hostile corpus; required-field and enum tests |
| M03 Deterministic compatibility | Fixed version/transport/lifecycle and upgrade rules | Compatibility matrix and upgrade tests |
| M04 No raw secret fields | Reference-only strict shape; content-free errors | Credential field/URL/name canaries; nested secret-field denial |
| M05 Round-trip stable | Canonical definition and envelope encoding | Rust round trips; independently computed Python SHA-256 vectors |
| M06 Duplicate/conflicting identities | Atomic set validator; unique inventory keys | Duplicate/conflict and release collision tests |

The schema validates structure; Rust additionally checks digest equality,
identity/inventory uniqueness, cross-field health/deprecation constraints and
compatibility. `invalid-cases.json` records which hostile inputs are structurally
valid yet semantically denied. JSON Schema cannot detect duplicate JSON keys after
a decoder has erased them; Rust tests the original bytes.

Reproduce with:

```sh
cargo test -p gateway-application --test connector_manifest --test connectors --locked
python3 -m unittest discover -s tests/contracts -p test_connector_manifest.py
CARGO_BUILD_JOBS=1 CARGO_LLVM_COV_TARGET_DIR=target/connector-manifest-coverage cargo llvm-cov -p gateway-application --test connector_manifest --locked --json --output-path target/connector-manifest-coverage.json
python3 scripts/check-connector-manifest-coverage.py --self-test
python3 scripts/check-connector-manifest-coverage.py target/connector-manifest-coverage.json
bash scripts/check-architecture.sh
cargo clippy -p gateway-application --test connector_manifest --test connectors --locked -- -D warnings
cargo fmt --check
```

The [evidence record](evidence/EPIC-07.02-manifest.json) binds this contract to the
candidate files and actual results. No live connector, resolved credential/profile,
application authorization or parent EPIC-07 completion is claimed.

Port revision 2 adds mandatory [canonical result metadata](connector-result-v1.md);
port-1 manifests and derived inventory digests require explicit regeneration.
