# ADR-019 — Governed Offline Model Learning

- **Status:** Accepted
- **Date:** 2026-10-03

## Context

Validated runtime outcomes can support reproducible offline training without
making production execution, historical success or model output an authority
for permissions, procedure promotion or model deployment.

## Decision

Admit reference-only learning signals through current CG-18 memory eligibility
and a trusted evidence port that verifies the exact measurements and lineage.
Build deterministic versioned datasets within one project with explicit split,
content/episode leakage controls and complete duplicate provenance. Revalidate
all sources at each consumption boundary.

Separate offline training and held-out evaluation ports from production model
inference. Require independent host authorization for the exact offline job.
Qualify candidates with deterministic CG-20 metrics and baseline gates. Require
separate release authority for immutable releases, bounded verified canary
observations, activation and exact predecessor rollback. Record derived model
dependencies for upgrade impact without automatically recertifying artifacts.

## Consequences

- JSON manifests and reinforcement measurements remain information.
- Workers must implement isolation, controlled reference access, reproducible
  execution and trusted evidence verification outside the deterministic core.
- Production inference is unchanged and does not train by default.
- The process-local release coordinator is a reference implementation; hosts
  must provide atomic durable journal/routing persistence before deployment.
- Historical model rollback never restores forgotten training data.
- See [CG-28 contracts and acceptance evidence](../offline-learning.md).
