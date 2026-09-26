# Architecture tests

Run `python3 scripts/quality-gate.py` for the complete release gate, or
`python3 -m unittest discover -s tests/architecture` for guard regression tests.

`check-architecture.sh` combines Cargo metadata dependency checks with catalog
and project-storage boundary checks. The exact reviewed dependency graph is
in [arc42 §5](../../docs/arc42/05-building-block-view.md). Mutation tests reject
outward and cross-component edges, aliases hiding forbidden packages,
target/dev/build dependencies, new workspace members and substituted sources.
Evidence-runner tests verify failure propagation, skipped later gates, log
retention, complete success and refusal to overwrite an existing bundle.

Behavioral architecture contracts remain with their Rust owners: versioned IR,
canonical capability resolution, process/policy authority and minimal context.
The [release checklist](../../docs/declarative-quality-gates.md) maps them to
EPIC acceptance criteria and describes frozen fixtures and retained evidence.
