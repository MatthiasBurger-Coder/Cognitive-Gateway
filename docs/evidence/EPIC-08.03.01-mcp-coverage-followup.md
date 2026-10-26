# MCP qualification selection follow-up for #403

External quality run [40382233582](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/actions/runs/40382233582)
passed gates 1–61, including the corrected shared-session and installed-goal
qualification. Gate 62 failed the old MCP-host coverage selection at 658/705.
Its selected tests omitted the installed observed-goal consumer of the new
session-aware host methods.

All four affected MCP selections (runtime, mapping, result normalization and
credential isolation) now include `observed_goal_sessions`. Existing suites,
checkers, limits and exact per-file 95% thresholds are unchanged. Production
code is unchanged.

The four corrected commands/checkers pass locally with real PostgreSQL and all
seven installed goal scenarios. Coverage includes host 671/705 (95.18%), mapping
513/520 (98.65%), normalization 341/347 (98.27%) and connector results 183/183.
The six MCP runtime files independently pass the unchanged gate.

The [source-bound archive](EPIC-08.03.01-mcp-coverage-followup.json.gz) retains all
four full LLVM reports, exact checker outputs, raw execution logs and current
automation sources. SHA256:
`e3678d337fec27c2ce93f44e01341f9dc60cecd4ea1ee5e76c177a0b329d90c5`.
An initial omitted-suite normalization run inside the retained log remains a
failed diagnostic; the separate final log proves corrected gates 64 and 65.

The original 79-binding archive remains a historical capture. This follow-up
supersedes only its `scripts/quality-gates.json` binding; every original
production/test binding still matches exactly. No old runtime result is
relabelled as a new source execution. External CI must run the corrected
candidate before merge. Issue #403 and full #274/#271 acceptance remain open.
