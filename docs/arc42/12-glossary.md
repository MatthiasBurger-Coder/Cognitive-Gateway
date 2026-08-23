# 12. Glossary

This glossary includes the explicit execution-state, capability and constraint
domain concepts introduced by CG-02.03.

## Adapter

An outer component that translates between an external technology and a core-defined port. Adapters are replaceable and do not define domain authority.

## Application Port

A stable contract owned by the application/core boundary. Inbound application ports expose use cases to driving adapters; outbound application ports are implemented by driven adapters.

## Authority
Canonical governance or policy information that defines hard constraints.

## Domain/Core

The stable inner part of the hexagon containing domain concepts and deterministic application behavior. It must not depend on infrastructure or adapter technologies.

## Cognitive Gateway
The local AI Context & Agent Control Plane that mediates between clients and execution runtimes.

## Cognitive Service
Optional AI-oriented service, typically Python-based, for classification, embeddings, retrieval or summarization.

## Context Compiler
Component that transforms validated execution context data into minimal stable and dynamic runtime context.

## Execution Context IR
Versioned intermediate representation containing task, state, selected agent/skills, knowledge queries, capabilities, constraints and target runtime.

## Execution Profile
Verification depth: `FAST_PATH`, `NORMAL_PATH` or `FULL_PATH`.

## Execution Runtime
System that performs the actual model/agent execution, such as Codex or PraisonAI.

## Driving Adapter

An adapter that initiates a use case, such as a CLI, API, IDE or CI integration, by calling an inbound application port.

## Driven Adapter

An adapter called by the core through an outbound port, such as a Git/RAG, MCP/tool, evidence or execution-runtime integration.

## Hexagonal Architecture

An architecture that isolates the domain/application core behind ports and connects external systems through replaceable driving and driven adapters. Dependencies point inward toward the core.

## Knowledge Plane
Retrieval side of the architecture. Answers what the runtime needs to know.

## Capability Plane
Tool/action side of the architecture. Answers what the runtime is allowed to do.

## MCP
Model Context Protocol. Used as one possible standardized tool/capability boundary.

## Operating Mode
Project phase: `DEVELOPMENT`, `HARDENING` or `RELEASE_QUALIFICATION`.

## Port

A core-owned interface defining how the application communicates with an external actor. Inbound ports accept requests; outbound ports request external knowledge, capabilities, runtime execution or evidence persistence.

## Project Profile
Project-specific declarative Skills, workflows, policies, retrieval metadata
and tool bindings. Reusable Agents are catalog definitions and are not
profile-owned.

## RAG
Retrieval-Augmented Generation. In Cognitive Gateway, RAG is a knowledge accelerator and never the sole governance authority.

## Resolver
Deterministic component that selects valid workflows, agents and skills from registry data and project state.

## Skill
Declarative knowledge and capability requirement with optional ownership,
skill dependencies, required abstract capabilities and retrieval queries. A
skill does not grant permission and does not contain a tool implementation.

## SLM
Small Language Model used locally for semantic perception tasks such as classification or relevance scoring.

## Execution State

The validated combination of workflow, gate and blocker states for one run.

## Gate State

Lifecycle of the current quality or governance gate: `PENDING`, `IN_PROGRESS`,
`PASSED`, `FAILED`, `BLOCKED` or `SKIPPED`.

## Blocker State

Whether execution is `CLEAR`, `ACTIVE` or `RESOLVED` with respect to blockers.

## Capability Class

The safety classification of a capability: `INSPECT` is read-only, while
`MUTATE` may change state and requires policy evaluation.

## Constraint

A typed execution-planning rule, such as feature freeze or a requirement for
explicit consent before mutation. Constraints can restrict a mode/profile pair
without making those dimensions the same concept.
