# CG-11 CLI walkthrough fixture

This synthetic external project has an unknown architecture condition. The
structured Intent asks for that condition to be true. Planning derives an
observation step, and the isolated catalog supplies one inspection Skill,
Agent and Process definition. These are test assets, not production catalog
entries or project profiles.

`policy.json` and `projection.json` are explicit synthetic operator decisions,
pinned to this fixture's exact resolution basis. The subprocess test
`checked_in_walkthrough_compiles_without_project_configuration` checks that
they still match the complete chain. If the inputs change, review the new
resolution and update those decisions deliberately; stale inputs must fail.

See [the CLI walkthrough](../../../docs/declarative-cli.md) for commands and
[the tests](../../../crates/gateway-daemon/tests/declarative_cli.rs) for failure
cases and context trust checks.
