# Connector policy and consent integration — #230

Historical evidence and removed review documents are linked to their archived Git
revision. They are not current acceptance evidence. Generate fresh reports in an
output directory or retain them as CI artifacts.

All connector tool invocations and resource reads enter `ConnectorApplication`
and the mapped `McpConnectorHost`. `Cg09ConnectorAdmission` calls the existing
CG-09 `PolicyEngine` before dispatch, and the host repeats current admission
immediately before the transport effect. Discovery supplies an inventory; it
never supplies authorization or consent.

The installed `cg-mcp` session path now composes this boundary for the registered
read-only goal observation profile:

```text
cg-mcp → LocalSessionHost → SessionCoordinator → ObservedGoalRuntime
       → GoalClaim / GoalSource → Cg09ConnectorAdmission → mapped MCP host
```

`GoalSource` joins the authenticated current workspace owner and canonical
native step to a live PostgreSQL claim and a coordinator-owned resource
allocation. It checks the current launcher admission, all pinned definition
contents, the project scope and full discovered inventory/schema association.
The request must match the registered capability, operation class, typed
arguments, source, task, step and execution identity. The opaque allocation also
binds owner, session, run, revision, fence, dispatch, reservation and deadline.
The host revalidates after mapping/schema preparation and before sending.

The source clears declared consent facts. Global policy and mapping consent
requirements are additive. A `GRANTED` value in a declared native fact does not
supply an authenticated request-bound receipt. The installed observation
profile has no consent issuer for connector actions and denies consent-required
reads, mutation and administration. It cannot borrow artifact-action consent
or ordinary inspection authority. Callers needing mutation must supply explicit
current mutation authorization, exact live consent and administrative membership
where applicable through the existing trusted admission contract; the CG-09
adapter and controlled MCP tests qualify that contract.

Actual evaluated policy decisions are retained in the owner-scoped PostgreSQL
observation audit, with request/argument and consent digests and normalized
findings. Audit persistence must succeed before dispatch. Audit entries are
explanation data, never reusable grants. Failures have typed, content-free
codes, and changed authority or tooling causes zero tool/resource calls.

## Delivery review

Readiness is `READY_FOR_WORKFLOW` for qualification of the existing policy
integration. Requirements, production ownership, prerequisite ordering and the
real PostgreSQL/stdio qualification commands are resolved. The estimated work
for this acceptance slice is 0.25–0.75 engineer-days for tests, qualification and
documentation; no new production foundation is included or required.

| Dependency / owner | Inspected implementation at base | Actual state / consumer gate |
| --- | --- | --- |
| #369 / application policy | `connector_policy.rs`, `connectors.rs` | Native CG-09 contract and adapter implemented; exact-binding matrix and >=95% gate required |
| #399 / native policy assembly | `policy_application.rs`, `host_mapping` | Actual resolved-step facts implemented; installed capture must reuse this owner |
| #401 / proposal bridge and MCP | `connector_proposals.rs`, `mcp_client/host.rs` | Controlled native proposal dispatch implemented; pre-effect deny cases required |
| #403 / installed observation and store | `local_sessions/observed_goal`, `session_store_observation_audit.rs` | Concrete installed source, claim and audit implemented; shipped positive/denied paths and durable records required |

The implementation owner remains unchanged for each dependency. Qualification
uses the shipped binaries and a controlled MCP peer with real PostgreSQL;
installed Codex-client interoperability and real SaaS effects belong to their
separate existing qualification scopes.

## Acceptance traceability

The requirement, architecture, automation and testing passes were performed by
one agent. The base is `e5dfc99`; the candidate changes qualification and
documentation, with no production-policy behavior change. The parent #223
explicitly permits an initial read-only slice; #230 supplies policy enforcement
for the connector boundary. Full installed model proposals, authorized mutation
orchestration and consent continuation remain #274 responsibilities.

| ID | Requirement | Production owner/path | Verification | Status |
| --- | --- | --- | --- | --- |
| P1 | Every connector invocation passes policy authorization | `ConnectorApplication`, `McpConnectorHost`, `Cg09ConnectorAdmission`; installed `GoalSource` | Native adapter matrix; controlled real MCP invocation/denial tests; installed positive goal reads | VERIFIED |
| P2 | Tool presence never grants permission | Explicit mapping, bound association and current source; no discovery grant | Installed required-consent denials after actual discovery; native request-substitution checks | VERIFIED |
| P3 | Default-denied mutation/admin; Inspect remains separate | CG-09 capability class mapping and explicit admin set; installed read-only definitions/source | All five operation classes, consent/admin positive and negative adapter cases; controlled mutation denial; installed class-escalation cases | VERIFIED |
| P4 | Capability/policy can require consent | Mapping `ConsentHint`, global admission and current-source receipt | Installed mapping/global consent denial and forged declared consent; adapter exact receipt/revocation checks | VERIFIED |
| P5 | Deterministic, explainable denials | Native policy reports, typed failures and PostgreSQL audit | Stable installed diagnostic allowlist, zero actual calls, persisted denial and absent receipt; audit snapshot tests | VERIFIED |
| P6 | Provider independence | Domain/application policy owner; MCP remains an outer adapter | Architecture guard and tests; controlled provider over unchanged CG-09 | VERIFIED |
| X1 | Bind capability, arguments, task/step/execution, project, inventory and current consent | `GoalSource.route`, native read association, allocation and exact `ConnectorDispatch` | Native live-source substitutions through the actual mapped run; adapter binding matrix | VERIFIED |
| X2 | Revalidate immediately before dispatch; stale/replayed consent or changed tools cause zero action | MCP host pre-effect `revalidate` and current-dispatch liveness checks | Actual controlled-peer stale-consent tests; installed discovery barriers for changed consent, policy and tool schema; live-claim revocation/cancellation | VERIFIED |

Read the source-bound [completion evidence](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/blob/59f6db7e29478e2903cf69d6e5c7e65632876710/docs/evidence/EPIC-07.07-completion.json)
for the executed commands, results, evidence levels and limits. The existing
[admission contract](connector-admission-v1.md) remains normative for new sources.
The added installed denial matrix runs in the existing connector-policy quality
gate and the all-target observed-goal qualification with real PostgreSQL.

This acceptance does not close #274 or either parent epic, qualify a live SaaS
provider, or enable mutation in the installed observation profile. Those are
separate delivery boundaries; fail-closed policy enforcement also applies to
unsupported actions.
