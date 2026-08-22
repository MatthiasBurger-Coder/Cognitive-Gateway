# ADR-006 — Operating Mode and Execution Profile Are Independent

- Status: Accepted
- Date: 2026-08-22

## Context

Project phase and verification depth are different concerns. Mixing them would make workflows harder to reason about and policy rules less explicit.

## Decision

Model them as independent dimensions.

Operating Modes:

- `DEVELOPMENT`
- `HARDENING`
- `RELEASE_QUALIFICATION`

Execution Profiles:

- `FAST_PATH`
- `NORMAL_PATH`
- `FULL_PATH`

Example:

```yaml
operating_mode: HARDENING
execution_profile: FULL_PATH
```

Policies may restrict combinations, for example requiring `FULL_PATH` in release qualification.

## Consequences

- project lifecycle and test depth remain orthogonal;
- policies can reason explicitly about both dimensions;
- hardening does not become an artificial fourth execution profile;
- profiles can be reused across multiple operating modes.
