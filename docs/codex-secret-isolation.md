# Codex credential and sensitive-data isolation

EPIC-04.06 #241 enforces the no-key boundary from ADR-020. Codex owns its
authentication; Cognitive Gateway does not load, retain or relay it. These
checks grant no policy, consent or mutation authority (#242).

## Launch and configuration

Launch `cg-mcp` with an explicit environment allowlist. A provider-free example:

```sh
env -i /absolute/path/to/cg-mcp \
  --client-name codex --client-version 1.0 --principal operator \
  --workspace workspace-example --project project-example --binding binding-example
```

Replace the example identity with the trusted client identity. Add the explicit
workspace admission arguments described in [scope isolation](codex-scope-isolation.md)
to use the local application host.

Before application startup, the executable inspects inherited environment names
and refuses credential-bearing names, including OpenAI/Codex API keys, access,
refresh, ID, authentication and session tokens, client secrets, passwords,
private keys, AWS secret access keys and GitHub/Hugging Face tokens. Matching
ignores ASCII case and punctuation and includes credential-name suffixes.
Even an empty credential variable is refused; remove it from the child environment.
The error is fixed and prints neither its name nor its value. Environment values
are not used for authentication, request state or evidence. `--help` requires no
startup or admission. HOME/CODEX_HOME authentication stores are never opened.
Only the explicitly supplied admission file is loaded.

Admission JSON is scanned before deserialization into retained mappings.
Credential fields/material in identity, resources, documents or provenance
reject the configuration. Unknown fields, duplicate keys, bad bindings and
unsupported contracts retain their existing fail-closed behavior.

## Requests and disclosure

The shared facade rejects credential-bearing requests before authorization or
canonical dispatch. The transport checks the complete frame, including IDs,
metadata, initialization and resource URIs. Credential-bearing requests receive
an anonymous fixed JSON-RPC error (`id: null`); such notifications are discarded.
The facade's anonymous `CG_SENSITIVITY_DENIED` envelope likewise omits untrusted
scope/correlation. Safe, validated bound requests still retain correlation when
a host result is denied.

The guard checks object keys, nested values and encoded JSON, credential
assignments, recognizable API-key tokens, bearer/basic credentials, JWTs and
private-key material. Nesting/node/string and encoded-JSON limits bound scans.
The scanner supplements classification; it cannot determine whether an arbitrary
opaque string is a secret. Trusted hosts must classify sources, authorize current
disclosure and avoid accepting client authentication through alternative fields.
No detector provides a universal secret guarantee for unclassified arbitrary text.

Reference records are checked before canonical parsing; host projections,
session results and response metadata are checked before publication. A second
transport guard also protects injected application ports. These guards apply to
structured results and their text projection, resources, explanation/evidence
links and provenance. Credentials in launch bindings or cache-key inputs are
refused before a trace or key is derived.

SECRET provenance cannot accompany an inline query result. Structured documents
marked SECRET or `reference_only: true` are denied as inline payloads. A host
may disclose an opaque, version/digest-pinned reference with SECRET provenance;
its document is absent and subsequent raw resource reads remain denied. This
slice introduces no permission to disclose a secret. Existing approved
disclosure owners remain responsible for safe projections and classification.

Rejected output is replaced as a whole by a fixed diagnostic. No sensitive
substring, source existence, rejected URI or raw authentication error is echoed.
Canonical documents, provenance and digests are never modified by redaction.
The adapter does not log raw frames, authentication state or environment dumps.

## Reproducible evidence

```sh
cargo test -p gateway-application --test codex_facade --locked
cargo test -p gateway-daemon --test codex_isolation --test codex_local_cli --test local_mcp --locked
cargo llvm-cov -p gateway-application -p gateway-daemon --lib --bin cg-mcp --bin cg-local \
  --test local_mcp --test codex_facade --test codex_isolation --test codex_local_cli --locked \
  --json --output-path target/codex-security-coverage.json
python3 scripts/check-local-mcp-coverage.py target/codex-security-coverage.json
python3 scripts/quality-gate.py
```

Synthetic credential fixtures are checked in under `tests/fixtures/codex-security`.
Tests cover clean startup and actual stdio requests, inherited credential names,
auth-store independence, admission injection, pre-dispatch refusal, ID and
metadata leakage, host projections, references, opaque SECRET results, fixed
authentication errors, cache/trace input checks and scan limits. Every established
quality gate remains required, including measured >=95% per-file coverage for
the new security module. Full live Codex-client qualification remains #245.
