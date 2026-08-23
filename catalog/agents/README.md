# Generic agents

Place every reusable, provider-independent Agent definition here. This is the
only Agent discovery boundary; project profiles do not define or override
Agents. Each JSON document must satisfy
[`../../schemas/agent.schema.json`](../../schemas/agent.schema.json) and use
the v2 self-contained contract without project identity, paths or provenance.

Technology-specific expertise is still reusable catalog content. Project
knowledge, repository conventions and activation evidence arrive through the
selected runtime or retrieval boundary and are not embedded in an Agent.

The catalog includes the promoted `devops`, `python-automation-developer`,
`react-frontend` and `ux-designer` specialists. Their former profile-only
references to project Skills were removed; a consuming profile can still
provide project-specific Skill context independently.
