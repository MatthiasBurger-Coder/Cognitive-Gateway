# CG-17 knowledge graph and graph retrieval

Issue: [#195](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/195).

## Requirement matrix

| Requirement sentence | Owner and implementation | Verification |
| --- | --- | --- |
| Nodes, edges and projections have typed identities and a version; dangling references, duplicate identities, missing source snapshots and unsupported versions fail construction. | `gateway-domain::knowledge_graph` | `gateway-domain/tests/knowledge_graph.rs` contract cases |
| Every node and edge retains source scope, snapshot, provenance, trust, freshness, sensitivity and uncertainty. Observed and inferred edges are distinct; inferred links cannot add evidence or authority. | `gateway-domain::knowledge_graph`; `gateway-application::graph_retrieval` | Contract and traversal tests with observed and inferred paths |
| Traversal follows stable identity order and stops on depth, nodes, edges, results, elapsed time or cost; cycles cannot repeat a node on a path. | `gateway-application::graph_retrieval` | Cycle, each budget, reordered-input and fake-clock tests |
| A projection is a derived read model; changed source snapshots or schema invalidate it, and the same inputs rebuild identically. | `gateway-domain::knowledge_graph`; `gateway-daemon::graph_retrieval` | Manifest, rebuild and stale-index tests |
| Traversal starts in the request scope. Crossing scopes requires an explicit permitted scope set and retains the original scope and provenance on every path element. | `gateway-application::graph_retrieval` | Two-scope denial and permitted-crossing tests |
| Graph results use the selected CG-15 source and strategy and CG-16 candidate envelope; adapter outage is surfaced through federation. | `gateway-daemon::graph_retrieval` | Adapter and federation tests |
| The graph grants no registry, Process or Policy authority and requires no database service. | Domain data shape and outbound graph storage port; in-memory outer adapter | Architecture guard and contract tests |

## Public contract and execution

`GraphProjection::new` validates one version for the projection and all its
nodes and edges. It requires unique graph and fragment identities, existing
edge endpoints, an edge claim scoped to one endpoint, a snapshot manifest entry
for every node and edge source, and explicit uncertainty on inferred edges.
Input order has no effect on the resulting projection. The manifest key is the
pair `(ContextScopeId, SourceId)`; equal source names in different projects do
not share a snapshot. `ensure_current` compares the whole manifest with the
current source manifest. `InMemoryGraphStore::replace` refuses a stale rebuild;
`invalidate` discards a projection. Graph version 1 is the only supported schema.

`GraphTraversalRequest` names seed nodes, accepted trust classes, maximum
sensitivity, freshness, explicit additional scopes and all six limits. The
application traverses outgoing edges in edge-ID order and uses a caller-supplied
elapsed-millisecond clock. It returns breadth-first paths with original node
and edge records, visited counts, cost and one stable stop reason. A node is
visited at most once, so cycles terminate. An edge costs one unit. Limits are
checked before expanding the next edge or emitting the next node. A time limit
is reproducible for a fixed clock stream; real elapsed time may cut a traversal
at a different point on a busy host.

The CG-15 `GraphRetrievalAdapter` selects seed nodes from exact substring
matches against node IDs or content, using only the request scope. It is
selected only by a `Graph` source and `GraphTraversal` strategy. It checks the
source manifest before retrieval, then returns CG-16 `HybridCandidate` values
with inspectable `graph_paths`. Federation retains graph path details in the
CG-15 result explanation, including edge relation, basis, source reference,
snapshot and uncertainty. The CG-15 adapter keeps additional scopes empty;
callers needing an explicitly permitted cross-project path use
`GraphRetrievalAdapter::retrieve_paths` with `GraphTraversalRequest`. That API
returns original source scopes on every path element. An inferred path clears
the result fragment's evidence IDs and changes its trust to
`DerivedAssessment`; a request must explicitly accept that class to receive
the result. No graph field carries an authorization grant.

The in-memory store is a replaceable outer adapter and is not a production
database. A source collector must update its current manifest when revisions
change; the store cannot detect changes independently. A stale graph returns
`StaleIndex`, and an absent graph returns `ServiceUnavailable`. Cross-scope
paths are returned by the explicit path API; the CG-15 result batch remains
isolated to its initiating scope.
