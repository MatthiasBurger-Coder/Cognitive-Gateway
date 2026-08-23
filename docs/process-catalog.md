# Process Catalog and Registry

The catalog/processes directory contains project-agnostic source definitions.
A registry rebuild starts from a repository snapshot, discovers files in
sorted path order, parses them with the Rust frontend, compiles them to
Canonical Process IR v1, validates the result, verifies its ID/version/digest
identity and only then admits it.

Registry state is derived and disposable. A duplicate process definition
ID/version, an invalid definition, an unsupported version or a conflicting
digest fails closed. Resolving without a version selects the highest
registered version; explicit version lookup is exact. The registry does not
execute processes, agents or tools.

The lifecycle/scheduling boundary and the explicit CG-04.16 execution-graph
dispositions are documented in
[`execution-graph-extension-boundary.md`](execution-graph-extension-boundary.md).
