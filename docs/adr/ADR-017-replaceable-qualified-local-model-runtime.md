# ADR-017 — Replaceable and Qualified Local Model Runtime

- **Status:** Accepted
- **Date:** 2026-10-03

## Context

Cognitive Gateway benefits from local SLM/LxM assistance for semantic classification, extraction, ranking and other bounded cognitive tasks, but model generations and serving runtimes change independently from the deterministic core.

## Decision

Local inference is accessed through a stable provider-neutral port and deployed as an independently replaceable service/process, with a Docker-first reference deployment.

Model profiles record identity, version/revision, artifact digest, runtime/version, quantization, capabilities, input/output compatibility, prompt/template provenance and qualification state.

Qwen3-8B quantized is a reference candidate only. A compatible future model may replace it through configuration/profile plus qualification without changing authoritative Gateway domain contracts.

Model promotion is explicit and reversible; unqualified automatic upgrades are not allowed.

## Consequences

- CPU-only operation is the baseline; optional GPU acceleration does not change core contracts.
- Model artifacts can persist independently of Gateway images.
- Runtime failure degrades deterministically.
- Model output remains non-authoritative proposal/signal data.
- CG-27 #218 and CG-27.01 #249 own local runtime/benchmark/lifecycle implementation; EPIC-06 owns general model invocation semantics.
