# Example Project Profile

This is the structural placeholder for a project-specific Cognitive Gateway profile. Project configuration must remain outside the generic gateway core.

The profile is organized into the following independent concerns:

```text
profiles/example-project/
├── agents/
├── skills/
├── workflows/
├── policies/
├── retrieval/
└── tools/
```

The agent and skill directories are ready for strict versioned JSON
definitions loaded by `gateway-registry`. Workflow, policy, retrieval and tool
directories currently contain boundary documentation only; their loaders are
introduced by later vertical slices.
