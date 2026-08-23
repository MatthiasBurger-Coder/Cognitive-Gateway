# Example Skills

Reserved for project-specific skill contracts, dependencies, abstract
capability requirements and retrieval queries. Skills describe requirements;
they do not grant permissions by themselves.

Skill definitions in this directory use the strict versioned JSON contract in
[`../../../schemas/skill.schema.json`](../../../schemas/skill.schema.json).
`gateway-registry::SkillRegistry` loads them deterministically; dependency
graph integrity is validated separately from document loading.
