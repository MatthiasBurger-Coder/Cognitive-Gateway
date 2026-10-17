# Context planning reference tasks

These are valid SemanticTaskIR 1.0 task shapes with fixture pins. A trusted host
must authenticate references and completeness before application use.

| Example | Derived needs beyond authority, task input and output/verification |
| --- | --- |
| `coding.json` | Mandatory repository/current system state; optional repository/RAG domain knowledge |
| `investigation.json` | Mandatory current system state and evidence; bounded permitted evidence sources |
| `structured-data.json` | Mandatory structured application data; no implicit RAG, memory, examples or tools |

`crates/gateway-application/tests/context_planning.rs` analyzes all three without
acquisition. It also exercises host-authenticated CG-07 information needs,
conditional additional context and the actual CG-10 handoff. The contract,
canonical encoding and source-selection algorithm are specified in
[the planning guide](../../docs/context-requirement-planning.md).
