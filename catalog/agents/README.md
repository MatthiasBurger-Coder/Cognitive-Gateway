# Generic agents

Place every reusable, provider-independent Agent definition here. This is the
only Agent discovery boundary. Each JSON document must satisfy
[`../../schemas/agent.schema.json`](../../schemas/agent.schema.json) and use
the v2 self-contained contract without consuming-project identity, paths or
provenance.

Technology-specific expertise is still reusable catalog content. Consuming
project knowledge, repository conventions and activation evidence arrive
through runtime or retrieval boundaries and are not embedded in an Agent.

The catalog includes the promoted `devops`, `python-automation-developer`,
`react-frontend` and `ux-designer` specialists. All Agent-to-Skill references
resolve against the catalog's canonical Skill IDs.
