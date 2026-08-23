# Example Project Profile

This is the structural placeholder for a project-specific Cognitive Gateway profile. Project configuration must remain outside the generic gateway core.

The profile is organized into the following independent concerns:

```text
profiles/example-project/
├── skills/
├── workflows/
├── policies/
├── retrieval/
└── tools/
```

Reusable Agents are loaded exclusively from the generic catalog. This profile
contains only project-specific Skills and reserved workflow, policy, retrieval
and tool boundaries. Workflow, policy, retrieval and tool loaders are
introduced by later vertical slices.
