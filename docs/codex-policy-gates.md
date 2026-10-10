# Codex operation policy and consent gates

EPIC-04.07 #242 makes the application facade enforce CG authorization before
canonical dispatch, session commands, reference resolution or scoped resource
reads. `CodexHost::authorize` remains an availability and disclosure check;
returning success does not grant permission. The additional
`CodexHost::operation_policy` defaults to `CG_POLICY_DENIED`.

## Operation classification

CG owns the closed mapping in `codex::authorization::operation_class`.
Catalog annotations are discovery metadata. Each operation requires the exact
provider-independent capability ID `cg.<operation>` in trusted CG authority.

| Operation | Class | CG capability class | Effect |
| --- | --- | --- | --- |
| `resource.read` | READ | INSPECT | Read an admitted immutable resource |
| `situation.inspect`, `situation.assess` | INSPECT | INSPECT | Inspect or derive a situation |
| `capabilities.resolve` | INSPECT | INSPECT | Resolve declared capabilities; grants none |
| `state.explain`, `context.compile` | INSPECT | INSPECT | Explain or compile existing authorized context |
| `registry.inspect`, `evidence.inspect` | INSPECT | INSPECT | Inspect existing definitions/evidence |
| `session.inspect` | INSPECT | INSPECT | Inspect through the shared session service |
| `session.start`, `session.approve`, `session.cancel`, `session.clarify`, `session.continue` | MUTATE | MUTATE | Submit a bounded command to the shared session service |
| SEARCH, ADMIN | Reserved; no exposed operation | INSPECT / MUTATE | Unknown names fail closed |

There is no operation for granting permissions/capabilities, changing policy or
advancing process state directly. Session service owners remain responsible for
validating exact pending commands, revisions and process transitions. The facade
contains no process coordinator or consent authority. Read-only payloads cannot
select another handler or supply extra command fields.

## Trusted authority and consent hooks

The host loads a fresh `OperationPolicy` for the admitted principal, canonical
scope, client session, mapping revision and exact invocation. It contains the
existing `PolicyAuthority`, verified `StepFacts`, process readiness and trusted
operating mode/execution profile. These Rust inputs cannot be deserialized from
the tool envelope. Client mode/profile values must match trusted values.

`OperationPolicy::evaluate` invokes the existing CG `PolicyEngine`, preserving
allowlists, deny precedence, capability contracts, feature freeze, authorization,
consent, evidence, constraints and process readiness. Missing authority denies.
Read/inspect definitions must be INSPECT; mutation definitions must be MUTATE.
A substituted capability contract or class denies. Mutations additionally require
explicit `mutations_enabled` and existing CG authorization and consent; neither
runtime enablement nor a policy allowlist alone is sufficient.

The existing `StepFacts::consents` is the consent hook: only the trusted consent
owner can set `Approval::Granted` after validating the relevant record and its
binding to the current action. A `session.approve` request's `consent_record`
reference is not itself consent and does not authorize its own command.

The immutable local workspace admission installs a CG policy for exactly
`situation.inspect`, `situation.assess` and `resource.read`, with DEVELOPMENT /
FULL_PATH settings and mutations disabled. Its existing source classification,
secret isolation and scope checks still apply. Shared session services remain
unavailable in this launcher. Discovery-only launch grants no authority.

## Decisions and diagnostics

The host's `policy_decision` audit hook receives the output-only
`StepPolicyReport` before dispatch, for both allow and deny decisions. Its sorted
findings provide deterministic reasons including NOT_ALLOWLISTED, EXPLICIT_DENY,
AUTHORIZATION_MISSING, CONSENT_MISSING, CONSENT_DENIED, EVIDENCE_MISSING,
CONTRACT_MISMATCH and PROCESS_BLOCKED. Runtime mutation disablement produces
AUTHORIZATION_DENIED; a requested execution-setting mismatch produces
INVALID_EXECUTION_PROFILE. The hook may retain reports through the trusted
explanation/audit owner; reports never become reusable authorization tokens.

The frozen v1 response retains sanitized `CG_POLICY_DENIED`,
`CG_CONSENT_REQUIRED` and `CG_EVIDENCE_REQUIRED` diagnostics. It does not echo
policy contents, consent records or rejected input. Unavailable services still
return `CG_UNSUPPORTED_CAPABILITY`. No transport or provider-specific
implementation evaluates policy.

## Regression evidence

`codex_facade::policy_gates` tests every mutation with absent policy/authorization/
consent, denied consent, disabled mutations, class substitution, requested mode
changes, missing evidence, blocked processes, read-only constraints and explicit
deny. Replays compare both responses and policy findings and assert zero
side-effect dispatches. Positive cases require explicit policy and verified
consent. Additional cases prove discovery and permissive availability hooks do
not grant authority, forged read/inspect input is rejected, unknown administrative
operations are unavailable, and resource reads cannot bypass policy.

`codex_isolation::local_policy_rejects_execution_authority_changes_and_session_mutations`
verifies the admitted local host boundary.

```sh
cargo test -p gateway-application --test codex_facade --locked
cargo test -p gateway-daemon --test codex_isolation --test local_mcp --locked
cargo test -p gateway-policy --test policy_engine --locked
```
