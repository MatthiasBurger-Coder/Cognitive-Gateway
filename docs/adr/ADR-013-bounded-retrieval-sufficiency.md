# ADR-013: Bounded retrieval and evidence sufficiency

Status: Accepted for the CG-19 development contract.

## Context

CG-15 defined finite retrieval budgets and CG-16 provided a single-round
executor. A retrieval score or raw evidence link cannot establish that a
required claim has enough valid support. A recursive executor also needs one
cumulative budget and an inspectable reason for every continuation or stop.

## Decision

The domain assesses `RequiredInformation` using only evidence IDs confirmed by
the owning evidence boundary, explicit provenance requirements, trust,
freshness, sensitivity, conflict and contamination metadata. It retains all
findings and missing references. The application runs bounded rounds through
the existing retrieval port; query refiners can propose only a new query set.
The immutable plan's scope, sources, strategies, requirements and budgets stay
fixed, while cumulative usage, repeated queries and no-progress counts remain
with the application run. The V2 batch contract gains a nonterminal
`Partial/MoreInformationNeeded` result; V1 keeps its existing meaning. CG-14 can pause on insufficiency;
retrieval cannot resume or authorize execution.

## Consequences

Adapters must report cumulative usage and reserve resources before dispatch.
An adapter error consumes an attempted round. The measured failure hook in
the retrieval port carries elapsed and consumed work; the local federated
adapter reports elapsed time and query bytes. Priced or remote adapters must
override that hook with their own measurements. No new database, provider prompt renderer or process-state
owner is introduced.
