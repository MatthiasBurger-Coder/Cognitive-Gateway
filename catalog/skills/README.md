# Generic skills

Place reusable, provider-independent Skill definition documents here. Each
JSON document must satisfy [`../../schemas/skill.schema.json`](../../schemas/skill.schema.json)
and contain its complete reusable semantics in the document itself.

The CG-03.08 migration contains the 37 canonical skills classified as
catalog-owned in the CG-03.02 migration matrix. Every document keeps the
normalized responsibility in `name` and `description`, structured guidance in
`authoritative_sources`, `rules` and `verification`, and typed references in
`requires` and `related_skills`. Project/profile applicability, provenance and
external content references are not encoded in the Skill contract.

`requires` is reserved for an explicit mandatory reusable dependency and is
resolved as part of the catalog dependency graph. `related_skills` records
optional or contextual guidance only; it never activates a Skill or grants a
capability. All references are canonical IDs from this directory, and every
target is validated when the catalog loads.
