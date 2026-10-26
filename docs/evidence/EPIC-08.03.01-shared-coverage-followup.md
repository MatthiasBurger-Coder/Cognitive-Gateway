# Shared-session qualification follow-up for #403

The external quality run [40376465498](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/actions/runs/40376465498)
failed the existing shared-session coordinator coverage gate at 719/769 lines.
Its selection omitted the newly installed observed-goal integration suite.

The corrected qualifier includes all seven `observed_goal_sessions` cases in
addition to its existing suites. It runs test cases serially, retaining tests
that explicitly exercise concurrent processes. The threshold, coverage engine
and production code are unchanged. The build directory can be selected through
`CARGO_LLVM_COV_TARGET_DIR`; `show-env` receives that directory through Cargo's
target setting so the executed binaries and profiles use the same build.

The fresh local run passed **331 Rust tests and eight real PostgreSQL/shared
session scenarios**, including restart, consent, competing processes and
post-commit recovery. The strict existing gate passed all 14 files; coordinator
coverage is **731/769 (95.06%)**. The seven installed goal cases passed with the
existing 10-second RPC and 30-second claim limits.

The [compressed source-bound report](EPIC-08.03.01-shared-coverage-followup.json.gz)
contains the complete LLVM report, raw log, executed command and qualifier/test
hashes. Its SHA256 is
`9a2b6c0dafbed679da437799c2feb55b11ed76b672e660f4ead6cf0d1e744e53`.
Production remains the candidate at `3a1f0e9`.

This is local evidence for the repaired shared-session gate. The corrected PR
still requires external CI; it is not a merge or parent-issue closure claim.
