# Codex workspace and session isolation

EPIC-04.05 #240 adds the `WorkspaceResolver` port, filesystem-backed
`LocalWorkspaceResolver`, `ScopeBinding`, `SessionContext` and an immutable
`LocalCodexHost`. The MCP wire contracts remain frozen at v1. The trusted launcher
supplies session identity outside the request envelope; caller scope and session
IDs remain claims. CG receives no provider key or Codex authentication credential.

## Scope-binding contract

A trusted admission file contains `schema_version: 1` and a nonempty `mappings`
array. Each mapping has exactly these fields:

| Field | Meaning |
| --- | --- |
| `repository` | Explicit absolute repository directory; canonicalized at admission. |
| `scope` | Frozen `{workspace_id, project_id, binding_id}` scope. |
| `canonical_scope` | Explicit CG `ContextScopeId`; never inferred from a project name. |
| `principal` | Trusted opaque local principal ID. |
| `session_id` | Trusted opaque Codex client session ID, not an authentication token. |
| `revision` | Immutable mapping revision. |
| `resources` | Array of admitted v1 resource envelopes with scope, pinned reference, document and provenance. |

For example, this configuration admits a scope with no content:

```json
{
  "schema_version": 1,
  "mappings": [{
    "repository": "/absolute/path/to/project",
    "scope": {
      "workspace_id": "workspace-a",
      "project_id": "project-a",
      "binding_id": "connection-a"
    },
    "canonical_scope": "canonical-project-a",
    "principal": "operator",
    "session_id": "codex-session-a",
    "revision": "mapping-1",
    "resources": []
  }]
}
```

Launch with an explicitly configured client identity:

```sh
env -i target/debug/cg-mcp \
  --client-name codex --client-version 1.0 --principal operator \
  --workspace workspace-a --project project-a --binding connection-a \
  --admission /absolute/path/admission.json \
  --cwd /absolute/path/to/project/subdirectory \
  --repository /absolute/path/to/project --session codex-session-a
```

The four admission arguments are an all-or-nothing extension to the six original
launch arguments. The original discovery-only launch remains available and
cannot execute application operations. Admission errors use a fixed diagnostic
and never print paths, configuration or rejected values. Admission files are
trusted operator configuration; private pipe ownership and file integrity remain
the local trust boundary described by ADR-020.

Both repository and cwd must be explicit existing absolute directories. Cwd must
fall inside exactly one configured canonical repository root, and the supplied
repository must equal that root. Unmapped paths, overlapping/duplicate mappings,
relative paths, and symlinks that escape the admitted root fail closed. Different
workspace/project pairs cannot reuse a canonical CG scope. There is no ambient
cwd lookup, Git discovery, URL fetch, longest-prefix choice or project fallback.
The chosen mapping must also exactly match the launch principal, workspace,
project, connection and client session. A new session or connection requires a
new admission; an existing host cannot be rebound.

## Requests, resources and provenance

Every facade call carries the complete trusted binding, requested execution depth
and request/trace correlation. Every host lookup compares scope, canonical scope,
principal, client session, connection and mapping revision before returning
content. References resolve only in the selected host's immutable resource set.
A record returned by any host must independently match both the call scope and
session. The facade verifies the exact reference and SHA-256 of canonical JSON
bytes, requires validated nonempty provenance identifying that reference, and
preserves source identity, revision, digest, freshness, sensitivity and lineage
in the response. A trusted facade must be constructed with `with_binding`; its session cannot be inferred from request data. Conflicting metadata for the same reference fails closed.

The local host admits `situation.inspect`, `situation.assess` and scoped resource
reads. These queries still use the existing canonical application services.
Inline documents require an exact match to an admitted, classified immutable
resource. Admission of a resource is explicit read disclosure for this private
connection; SECRET sources are refused for both inline and reference access.
Other canonical hosts remain injectable through the facade and must implement
current policy/disclosure evaluation. This reference host supplies no mutation,
consent or policy authority.

Scoped `resources/read` implements the frozen catalog template. It checks the
launch scope before asking the application to resolve an exact ID/revision/digest.
All unavailable, malformed, foreign and secret resource reads return the same
fixed resource-unavailable error without revealing existence. Static discovery
resources remain public to an initialized admitted client.

Successful responses and fully validated requests denied after scope binding
retain scope, operation and correlation. Malformed or foreign-scope requests
receive an anonymous fixed diagnostic. Every successful bound response includes a pinned
scope trace in `explainability`. Read that reference through the scoped resource
template to inspect the canonical scope, principal, client session, connection
and mapping revision. It uses the existing trace graph contract and explicitly
records `SCOPE_BOUND` with policy `NOT_EVALUATED`; it grants no authority. Source
provenance remains in the response's provenance array.

`SessionContext` describes the client connection, not a CG task coordinator.
Task-session operations remain unsupported in the local host until shared
#272/#273/#275 services are installed. Injected shared services receive the trusted client binding on every call. The facade requires their `session_owner` lookup to match the complete immutable binding before dispatch and after result mapping; services also check current revision, consent and authority. No caller-supplied session state or prior query
result can broaden the immutable launch binding.

## Cache-key contract

`ScopeBinding::cache_key` defines a reference-context partition fingerprint:
SHA-256 of version, scope trace identity, operation, exact immutable references
and complete validated source provenance. Scope trace identity pins canonical
scope, principal, workspace, project, client session, connection and mapping
revision. Every reference requires matching classified provenance. SECRET
provenance is rejected before hashing. Paths, repository URLs, raw documents,
caller input, authentication tokens and credentials are not accepted as key
inputs; diagnostics never include the key basis.

This slice stores no cached results. The fingerprint is not sufficient to reuse
arbitrary inline queries or authorized results. A future result cache must also
pin requested execution depth, policy/disclosure revisions and the complete
nonsecret semantic command, and recheck authorization and disclosure at use time.

## Reproducible isolation evidence

```sh
cargo test -p gateway-daemon --test codex_isolation --locked
cargo test -p gateway-application --test codex_facade --locked
cargo test -p gateway-daemon --lib local_mcp --test local_mcp --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
python3 -m unittest discover -s tests/contracts
python3 -m unittest discover -s tests/architecture
bash scripts/check-architecture.sh
python3 scripts/check-local-mcp-protocol.py
cargo llvm-cov -p gateway-application -p gateway-daemon --lib --bin cg-mcp \
  --test local_mcp --test codex_facade --test codex_isolation --locked \
  --json --output-path target/codex-isolation-coverage.json
python3 scripts/check-local-mcp-coverage.py target/codex-isolation-coverage.json
```

The isolation suite covers unmapped/ambiguous roots, wrong repositories, symlink
escape, foreign projects/workspaces/connections/sessions/principals, mapping
revision changes, reference provenance and digest preservation, SECRET and
unclassified rejection, partition isolation, and a real empty-environment stdio
launch with successful scoped query/resource access and foreign-resource denial.
Live Codex client qualification remains #245.
