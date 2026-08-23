# Generic skills

Place reusable, provider-independent Skill definition documents here. Each
JSON document must satisfy [`../../schemas/skill.schema.json`](../../schemas/skill.schema.json)
and retain provenance in its `origin` object.

The CG-03.08 migration contains only the 37 canonical skills classified as
catalog-owned in the CG-03.02 migration matrix. Every document keeps the
normalized responsibility in `description` and retrieval hints in
`knowledge_queries`; `dependency_ids` and `required_capability_ids` are
explicit even when their arrays are empty. Project/profile applicability is
not encoded with an extra field because the v1 Skill contract intentionally
rejects profile selectors and execution constraints.
