# Canonical operations in the local host — EPIC-04.11

The shipped `cg-mcp` and `cg-local` hosts optionally provide
`capabilities.resolve`, `state.explain` and `context.compile`. They reuse the
strict declarative CLI record mapper and the existing resolution, explanation,
policy and context application services. No subprocess or alternative planning
or policy engine is used by the host.

## Operator admission

Add `canonical` to the selected mapping in the explicit admission document:

```json
{
  "canonical": {
    "catalog": "/absolute/admitted/repository/catalog",
    "plan": {"id": "plan", "contract": "cg.plan", "contract_version": "1.0", "revision": "1", "digest": "sha256:ACTUAL_DIGEST"},
    "rules": {"id": "rules", "contract": "cg.composition-rules", "contract_version": "1.0", "revision": "1", "digest": "sha256:ACTUAL_DIGEST"},
    "process": {"id": "process", "contract": "cg.process-snapshot", "contract_version": "1.0", "revision": "1", "digest": "sha256:ACTUAL_DIGEST"},
    "policy": {}
  }
}
```

This shape is illustrative and deliberately contains no usable grants or
digests. `policy` must be the complete strict policy document accepted by
`cg compile --policy`, including its exact resolution basis, policies, mode,
profile and step facts. It is trusted operator configuration, never tool input.
The catalog must be an absolute existing directory within the explicitly
admitted repository. Protect the catalog and admission file as authority inputs.
Admission captures the canonical resolution basis, including catalog and process
fingerprints. Each operation reloads the catalog and compares its basis with that
launch-time pin; drift returns `CG_STALE_REVISION`. Admission snapshots and policy
remain immutable for that launch. Changing them requires a new validated launch.

Each pinned source must exist in `mapping.resources` with its exact reference,
canonical JSON document digest, mapping scope and explicit provenance. SECRET
sources are refused. Source IDs cannot use the reserved generated ID
`local-resolution`. The plan is the complete `cg plan --json` document; rules
and process use the existing declarative CLI contracts. The mapping's
`canonical_scope`, operating mode and execution profile must match the records.
The current local operation admission remains DEVELOPMENT / FULL_PATH with
mutations disabled.

## Calls and result references

Resolve takes the exact admitted `plan`, `rules` and `process` references;
identical documents under alternate IDs cannot substitute for these pins. Explain
and compile accept only the generated resolution identity. Its response
provenance includes a generated `cg.resolution / 1.0` reference with ID
`local-resolution`, the admission mapping revision and the canonical artifact's
SHA-256 digest. The generated reference preserves the source lineage and highest
admitted sensitivity. Pass that exact reference to explain or compile. It can
also be read through the existing scoped resource URI:

```text
cg://workspaces/WORKSPACE/projects/PROJECT/bindings/BINDING/references/local-resolution/REVISION/DIGEST
```

The host recomputes and verifies the resolution before accepting its reference.
Wrong scope, revision or digest fails closed. Compile additionally takes a pinned
`cg.context-projection` resource and exact `step_id`. Projection fragments use
the existing strict CLI mapping. Additional external candidate references are
explicitly unsupported by this first canonical host path; use `candidates: []`.
This limit does not silently select or replace candidate records.

Policy and process checks still execute in the canonical application service.
Missing approval returns consent-required; a denied policy or stale basis cannot
be bypassed by caller claims. Context output uses the existing NORMAL disclosure
policy and redacts caller input, external content, task, output and constraints.
Full MCP and `cg-local` envelopes agree; raw `cg compile` output may contain
fields that the admitted host deliberately redacts.

## Verification

```sh
cargo build -p gateway-daemon --bin cg --bin cg-mcp --bin cg-local --locked
cargo test -p gateway-daemon --test codex_canonical --test declarative_cli --locked
```

The executable scenarios cover parity, determinism, provenance, generated
resource reads, missing/stale/cross-scope references, current policy, consent,
invalid projections, catalog confinement and SECRET admission refusal. The
coverage gate includes the new host and mapper plus the shared CLI pipeline.
This delivers canonical operations; it does not install shared session services.

## Rollback

Remove the selected mapping's `canonical` block and restart `cg-mcp` / `cg-local`
with the validated admission document. The host then admits only inspection,
assessment and resource reads; resolve, explain and compile return unsupported.
Existing inspection resources remain available, including resources whose ID is
`local-resolution` when canonical operations are disabled.
