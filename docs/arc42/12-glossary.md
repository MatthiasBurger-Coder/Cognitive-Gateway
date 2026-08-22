# 12. Glossary

## Authority
Canonical governance or policy information that defines hard constraints.

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

## Knowledge Plane
Retrieval side of the architecture. Answers what the runtime needs to know.

## Capability Plane
Tool/action side of the architecture. Answers what the runtime is allowed to do.

## MCP
Model Context Protocol. Used as one possible standardized tool/capability boundary.

## Operating Mode
Project phase: `DEVELOPMENT`, `HARDENING` or `RELEASE_QUALIFICATION`.

## Project Profile
Project-specific declarative agents, skills, workflows, policies, retrieval metadata and tool bindings.

## RAG
Retrieval-Augmented Generation. In Cognitive Gateway, RAG is a knowledge accelerator and never the sole governance authority.

## Resolver
Deterministic component that selects valid workflows, agents and skills from registry data and project state.

## Skill
Declarative capability/knowledge contract with dependencies, ownership, constraints and required tools.

## SLM
Small Language Model used locally for semantic perception tasks such as classification or relevance scoring.
