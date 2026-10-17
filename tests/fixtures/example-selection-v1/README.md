# Example selection 1.0 fixtures

`support.rs` supplies captured CG-16 input/output fragments with source lineage,
quality, byte snapshots, exact candidate/validation bindings, task/output and
architecture compatibility, explicit source policy and conservative token bounds.
Both the domain and application conformance suites consume these fixtures.
They use an admitted fixture host; no live authentication claim is made.

| Fixture family | Executed proof |
| --- | --- |
| Valid curated/repository/retrieval/experience/procedure sources | Validated success; metadata and exact/estimated token counts preserved |
| Model/synthetic origin | Pending validation rejected; validated synthetic input remains non-authoritative |
| Invalid/unverified/failed | Wrong version/digest, invalid snapshot, pending/revoked lifecycle, unsuccessful outcome denied |
| Stale/incompatible | Source freshness, task/target, output revision and architecture checks precede advisory scores |
| Cross-scope | Foreign scope denied; exact pinned reuse policy permits explicit reuse with original scope retained |
| Contaminated | Example or original pair fragment quarantine prevents reuse |
| Budget pressure | Count, conservative token/context limits; zero optional and unmet mandatory paths |
| Redundancy/diversity | Exact pairs and >=90% token-set overlap suppressed; uncovered technology tags preferred |
| Hostile example payload | Policy-like text and role delimiters stay data through real CG-10 and two reference layouts |
| Sensitive output/audit | Strongest sensitivity enforced on both halves; CG-10 audit policy redacts example data |

Run `cargo test -p gateway-domain -p gateway-application --test example_selection
--locked`. The [implementation contract](../../../docs/dynamic-example-selection.md)
defines ownership, selection order, host obligations and coverage commands.
