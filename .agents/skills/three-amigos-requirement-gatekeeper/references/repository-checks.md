# Cognitive Gateway review inputs

Resolve these paths from the target checkout root. Inspect their current content;
this map is navigation, not a frozen claim about delivery or operating mode.

## Authority and scope

- `.agents/AGENTS.md`: operating mode and agent governance. Apply the hardening
  workflow only when its mode and requested scope call for it.
- `docs/adr/`, especially ADR-008/009/020/021 for inbound and shared sessions;
  `docs/shared-session-contract.md`: ownership, trusted authority and v1/v2 limits.
- The complete named issue and parent, including later additions;
  `docs/epic-04-acceptance.md` and `scripts/epic04-acceptance.json`: all 24
  EPIC-04 criteria, production owners and required evidence levels.
- `docs/epic-04-gap-plan.md`: historical intake, demonstrated causes versus
  hypotheses, foundation effort and sequencing. Its historical matrix is not
  the present dependency state.

## Delivered path

Follow the `cg-mcp` entrypoint configured in `crates/gateway-daemon/Cargo.toml`
and `crates/gateway-daemon/src/bin/cg-local.rs` through `local_mcp/`,
`codex_workspace.rs`, `codex_canonical.rs` and `local_sessions.rs` to
`crates/gateway-application/src/codex/` and the shared session services. Inspect
the actual constructors, admission and concrete persistence wiring. Follow
installation/configuration in `scripts/bootstrap-codex-local.py` and operator
docs; an injected host is a separate path.

Confirm ownership and durability from `docs/shared-session-implementation.md`,
the application code and daemon storage/migrations. For a different slice,
trace its own launcher and effect boundary instead of requiring session work.

## Commands and evidence

`scripts/quality-gates.json` and `.github/workflows/rust.yml` define the current
Rust/Python checks. Resolve the commands applicable to the slice and record
which were actually executed. A documentation/skill change does not require
rerunning unrelated production qualification to claim that bounded change.

- Skill package: run the available skill-creator `scripts/quick_validate.py`
  against `.agents/skills/three-amigos-requirement-gatekeeper`; also resolve its
  relative reference links. The validator establishes structure, not behavior.
- Architecture/evidence regression: `python3 -m unittest discover -s tests/architecture -p 'test_*.py'`
  and `bash scripts/check-architecture.sh`.
- Component evidence: `scripts/qualify-codex-local.py` and
  `docs/codex-release-qualification.md`.
- Shared-service/database evidence: `scripts/qualify-shared-sessions.sh` and
  `docs/evidence/EPIC-04.13-shared-runtime.json`.
- Installed-client evidence: `scripts/qualify-installed-codex.py` and
  `docs/codex-installed-client-qualification.md`. Inspect substitutions,
  initialize identity, client/binary identity, source binding and actual calls.
- Full-parent acceptance: `python3 scripts/qualify-epic04.py --run --output target/epic04-acceptance`.
  Inspect the runner's prerequisites before executing; missing database/client/
  toolchain evidence is NOT_RUN/BLOCKED. A reconciliation without those reports
  must reject closure.

Retained reports under `docs/evidence/` may describe earlier revisions. Compare
source/artifact bindings and limitations before using them for current acceptance.
Do not change a report status or reduce parent criteria to obtain a green gate.

For this skill's origin read [source.md](source.md); for concrete role-pass
validation see `docs/three-amigos-skill-validation.md` in the checkout. Keep
review provenance explicit; scenario walkthroughs are not automated model tests.
