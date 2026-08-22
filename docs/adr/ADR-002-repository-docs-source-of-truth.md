# ADR-002 — Repository Documentation Is the Technical Source of Truth

- Status: Accepted
- Date: 2026-08-22

## Context

Cognitive Gateway needs both deeply technical architecture documentation and simple end-user guidance.

## Decision

Maintain the canonical technical documentation in the repository:

- `docs/arc42/` for architecture;
- `docs/adr/` for architecture decisions;
- future reference/development documentation beside the source.

Use GitHub Wiki for simplified end-user documentation, tutorials, examples, FAQ and onboarding.

## Rule

> The Wiki explains. The repository specifies.

If Wiki content conflicts with repository architecture documentation, the repository documentation is authoritative.

## Consequences

- architecture changes remain versioned with code;
- reviews and branches include documentation changes;
- Wiki content can be optimized for users without becoming governance authority;
- a later synchronization/generation pipeline may publish selected repository documentation into the Wiki.
