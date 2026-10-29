# GitHub MCP reference connector — EPIC-07.10 #233

The optional `cg-github-mcp` Rust binary is a repository-bound, read-only MCP
server. It translates the GitHub REST API into the existing closed scalar MCP
profile. It is a CG reference adapter, **not GitHub's official MCP server**.
The official server's broader nested schemas are not implicitly admitted.
No provider SDK, GitHub type, policy rule or planner branch is added to the core.

The server implements MCP **2025-11-25**, JSON-RPC 2.0 over stdio, version
**1.0.0**. HTTPS GET uses the operator-installed `/usr/bin/curl` with explicit connection and total time
limits, no redirects, no implicit retries and an empty environment.
The GitHub REST profile pins `X-GitHub-Api-Version: 2022-11-28`.
API authorities: [issues](https://docs.github.com/en/rest/issues/issues#get-an-issue),
[pull requests](https://docs.github.com/en/rest/pulls/pulls#get-a-pull-request),
[workflow runs](https://docs.github.com/en/rest/actions/workflow-runs#get-a-workflow-run),
[Git trees](https://docs.github.com/en/rest/git/trees#get-a-tree).

## Build and configure

```sh
cargo build -p gateway-daemon --bin cg-github-mcp --locked
target/debug/cg-github-mcp describe MatthiasBurger-Coder/Tiny-Swarm-World > target/github-connector.json
target/debug/cg-github-mcp resolve MatthiasBurger-Coder/Tiny-Swarm-World 'MatthiasBurger-Coder/Tiny-Swarm-World#361'
```

`describe` exports the exact discovery, content-addressed manifest, explicit
mapping document and scalar normalization fields without contacting GitHub.
The generated example for `owner/repo` is retained under
[`examples/github-connector-v1/`](../examples/github-connector-v1/).
The bundle's normalization entry describes shared fields; each mapping uses its
own exact `source.github.<operation>` source. Regenerate and readmit on adapter
changes; discovery descriptions and schemas are included in inventory digests.

Configure the existing stdio host with an absolute binary path, arguments
`["stdio", "OWNER/REPO"]`, absolute working directory and empty authentication
environment. An optional third argument selects an absolute curl executable;
this is trusted operator configuration, never a model or provider argument.
Bind and pin the **entire** configuration, including the repository and executable
arguments, using [scoped admission](connector-admission-v1.md). The exported
`config.github.reference.v1` reference is a placeholder requiring an actual
operator-owned admitted configuration. A manifest alone grants no access.

Use [credential profiles](connector-credentials-v1.md) to inject only
`GITHUB_PERSONAL_ACCESS_TOKEN` into this server's environment through
`CredentialedStdioFactory`. Bind the profile to `github.reference`,
`github.reference.local` and the exact required tool allowlist. Use repository
read permissions appropriate to the chosen tools (Contents, Issues, Pull
requests, Checks, Commit statuses and Actions). Public reads can run without a
token but must not be labelled authenticated by the host collector.

The server passes the authorization header to curl on stdin, never as argv or
child environment. It discards provider diagnostics, rejects token disclosures,
does not inherit curl configuration/proxy environment, and refuses redirects.
Credentials must stay outside the checkout, bundle, evidence and context.
A changed repository requires fresh scoped configuration admission. One server
instance serves exactly one repository; tool arguments cannot choose another.

## Explicit read surface

All capabilities are `READ` / `Inspect`, except `github.issue.search`, which is
`SEARCH` / `Inspect`. Each mapping declares no side effects, idempotence,
read-only retry safety and policy-owned consent. No mutation tool is exposed.

| Capability suffix after `github.` | Required scalar arguments | GitHub content |
| --- | --- | --- |
| `repository.read` | none | Repository metadata/state |
| `tree.read` | `ref` | Nonrecursive Git tree; `sha`, truncation indicator |
| `file.read` | `path`, `ref` | Contents envelope, file encoding/content/blob SHA or directory entries |
| `branches.read` | `page` | Branches and commit references |
| `commits.read` | `ref`, `page` | Commits |
| `commit.read` | `ref` | Commit metadata, SHA and available file patches |
| `issue.read` | `number` | Issue body, state, labels and metadata |
| `issue.comments`, `issue.labels`, `issue.subissues` | `number`, `page` | Comments, labels, native sub-issue relationships where available |
| `issue.search` | `query`, `page` | Repository-bound issue search |
| `pull_request.read` | `number` | PR, head/base revisions, merge state |
| `pull_request.files` | `number`, `page` | Changed files and available diff patches |
| `pull_request.reviews`, `pull_request.comments` | `number`, `page` | Reviews and inline review comments |
| `checks.read` | `ref`, `page` | Check runs |
| `status.read` | `ref` | Combined status |
| `workflow.runs` | `page` | Workflow runs |
| `workflow.read` | `number` | Run status, result, revision and evidence URLs |
| `workflow.jobs` | `number`, `page` | Jobs including step/result metadata |
| `workflow.job` | `number` | One job and its steps/results |
| `workflow.artifacts` | `number`, `page` | Artifact metadata/evidence references; no downloads |

Numbers must be positive signed integers; page is 1–1000, with 30 results per
page. Paths and refs are percent encoded and reject empty/traversal segments.
Search accepts a bounded ASCII phrase (letters, digits, spaces, `-_.`) and quotes
it after fixed `repo:OWNER/REPO is:issue` qualifiers. Arbitrary GitHub search
syntax is intentionally unsupported. Foreign repository qualifiers cannot escape
scope. Collection calls are always partial observations; neither an empty page
nor a successful call proves exhaustive traversal. Commit details and combined
statuses are also conservatively partial. GitHub can omit oversized/binary
patches; artifact metadata is not independently verified build evidence.

## Same canonical path for every operation

The shipped binary is consumed by the existing `McpConnectorHost`,
`CredentialedStdioFactory`, `DiscoveryMapper` and `CanonicalConnectorMapping`.
`GitHubResultNormalizer` implements the existing normalization port and delegates
to `CanonicalResultNormalizer`. Supply a trusted `AcquisitionPort`; there is no
default authenticated collector or authorization bypass.

Every tool returns `object` (the exact requested API URL), `content` (canonical
JSON as opaque untrusted text), `partial` and optional `revision`. The normalizer
reconstructs the URL from admitted typed arguments and the configured repository;
substitution is denied. Source identity is explicit per mapping. Provider
`updated_at` or `sha` is retained when present; unknown revision stays absent.
The existing canonical normalizer computes content digests and preserves scope,
task/step/execution, inventory and correlation. Acquisition time, authenticated
identity, sensitivity, confidence and freshness remain host-owned. Partial
results cannot gain complete quality. No scalar field is inferred from prose.

Use `ConnectorResult::new`, `batch()` and `fragments()` for the existing CG-06
scoped observation and EPIC-02/CG-10 context boundaries. Apply current CG-09
admission and disclosure/selection rules before ingestion or context compilation.
This is a generic driving-port integration; installed observed-goal sessions can
also use their existing explicitly pinned scalar normalization profile for a
specific source/route. This adapter does not declare itself a qualified goal-read
profile or prove goal completion from a returned body.

MCP frames are bounded at 256 KiB; API bodies at 96 KiB, plus a bounded HTTP status
suffix. Oversized responses fail rather than silently truncating. curl GET has a
5-second connection and 20-second total limit. Cancellation closes the MCP
server; an already-running read-only curl may survive until its 20-second bound.
There is no hidden pagination or retry loop. The generic runtime owns deadlines,
retry decisions and aggregate accounting. Provider failures expose fixed codes;
401/403/404/429 map to authentication/permission/not-found/rate-limit categories.
A 403 is conservatively denied; no provider prose is interpreted to infer retry
permission. Tool failures never produce observations. A rate-limit tool error is
not automatically retried by this profile.

## External issue resolution example

For `Implement GitHub issue: MatthiasBurger-Coder/Tiny-Swarm-World#361`:

1. Resolve the extracted reference using the repository-bound `resolve` command.
   It accepts `OWNER/REPO#N`, an exact HTTPS GitHub issue URL, or local `#N`.
   It emits the canonical `github.issue.read` capability and typed number;
   foreign repositories, query strings and fragments are rejected.
2. Under the current CG task/step/execution and `ContextScopeId`, discover the
   pinned server and construct the canonical request with its inventory digest.
3. Invoke through current application admission/CG-09. Discovery is not consent
   or authority. Do not call the server directly from a model.
4. Normalize and validate the returned evidence and source revision, ingest the
   scoped batch and select reference fragments for the semantic/context services.
5. Continue the existing semantic compiler → planning → resolution → policy →
   context compilation path. Issue text remains external content. Completion
   requires independent goal verification.

`resolve` performs deterministic reference parsing, not natural-language intent
compilation. The complete natural-language and execution lifecycle remains the
owner of EPIC-05/EPIC-08 and EPIC-07.12 qualification; this child does not add a
second coordinator. Optional `issue.comment`, `issue.update`, `branch.create` and
`pull_request.create` are denied and have no executable mapping. Adding them
requires new explicit mutation/consent/reconciliation qualification.

## Review and evidence

Intake: `43df656c92ea77c283f76154bb93e7efd562fe01`, clean DEVELOPMENT/FULL_PATH.
Requirement, architecture, automation and tester passes were performed by one
agent; no independent approval is claimed. Readiness: READY_FOR_WORKFLOW for
this read-only reference slice. Existing admission, mapping, credentials,
resilience, CG-09 and CG-06 contracts are reused. No storage/catalog migration.
Rollback removes the optional adapter/configuration and its additive tests.
Planning assumption: 1–2 engineer-days implementation and 0.5–1 qualification,
excluding operator credential/deployment provisioning.

| ID | Requirement | Production owner/path | Verification level |
| --- | --- | --- | --- |
| G01 | Issue into scoped context with provenance | MCP host → GitHub normalizer → canonical result batch/fragments | EXECUTABLE server, deterministic GitHub HTTPS substitute, real CG-09 engine |
| G02 | Repo/PR/CI same connector path | Same host/mapping/normalizer | Same executable integration test |
| G03 | Provider-free core | Only outer daemon adapter and optional server | Architecture/dependency guard |
| G04 | Policy/scope boundaries | Existing admitted host and CG-09 | Allowed reads, explicit native denial, foreign scope, mutation-class denial |
| G05 | No optional mutation authority | Static read-only routes and explicit mapping | Unknown mutation name, class mismatch and zero dispatch denial |
| G06 | External reference scenario | Repository-bound resolver and above sequence | Parser, executable command and lineage tests |
| G07 | Minimum read surface and revisions | Explicit 22-route adapter | All-route fixtures, schema binding, revision/partial and malformed input tests |
| G08 | Secret-safe bounded provider IO | Credentialed stdio + curl reader | Secret injection/disclosure, HTTP errors, byte limits and unavailable executable |

Run:

```sh
cargo test -p gateway-daemon --lib github_connector --locked
cargo test -p gateway-daemon --test github_connector --locked
cargo fmt --all -- --check
cargo clippy -p gateway-daemon --all-targets --locked -- -D warnings
bash tests/scripts/check-architecture.sh
```

The integration test replaces only HTTPS with an independent curl-compatible
peer and uses real child processes, credential isolation, canonical discovery,
CG-09 evaluation, result validation and scoped context conversion. It supplies
controlled policy/acquisition facts; it does not qualify installed shared-session
persistence, a live authenticated GitHub deployment, model execution or the
entire parent EPIC. Public live reads, if recorded separately, do not establish
private-repository authentication. Coverage and command results are retained in
`docs/evidence/EPIC-07.10-github-reference.json`.
