# #191 completion review and gate repairs

Review date: 2026-10-17. Base revision:
`492040bae2b039ef045d62ebd0160cb9e4683b6d`, with the local #191 specification,
fixture and tests. The user explicitly requested analysis and correct fixes for
the failing gates. This review uses the repository Three Amigos skill as role
passes by one agent; it does not claim independent reviewers or approvals.

## Requirement and architecture decision

#191 requires proof of the task/process boundary, not new runtime invocation or
full-parent EPIC-04/05 acceptance. Its five criteria map to P01–P05 in the
[boundary specification](semantic-process-boundary.md). The reference scenario
must compile both languages, preserve the exact pin and task obligations, and
leave transition legality with CG-04. Repository checks must not be waived merely
because failures predate this issue.

The requirement pass confirms that repairs to the reported validation blockers
are now authorized. The architecture pass finds three portability defects in
evidence tooling/tests, two equivalent Rust lint repairs, and a scheduling
assumption in a test. None requires changing process ownership, policy authority,
storage, or public task contracts. The automation pass requires native resource
measurement and portable evidence keys, preserving missing-prerequisite refusal.
The tester pass requires positive and negative evidence admission, native memory
measurement/failure checks, repeated timeout regression, and both Windows and
Linux validation where platform behavior differs.

Decision before repairs: **READY_FOR_WORKFLOW**. Order: fix evidence portability
and equivalent lints; correct timeout observation; run architecture/contracts,
Rust regressions and coverage; reconcile the source-bound #191 report. Estimated
effort: 0.5–1 engineer-day repairs plus 0.5 qualification, moderate confidence,
existing compiler/process/planner foundations included. No deployment prerequisite
is added. Rollback is the individual repair diff. Stop if correctness requires
weakening authority/evidence checks or expanding process semantics.

## Root causes and fixes

| ID | Failure / owner | Root cause | Correct fix and evidence |
| --- | --- | --- | --- |
| F01 | EPIC-03 benchmark subprocess / evidence tooling | Unconditional `resource` import fails on Windows before the experiment runs | `process_memory.py` uses native `GetProcessMemoryInfo` on Windows and existing `getrusage` elsewhere. Native failures raise; no zero, guessed metric or skipped experiment. Real benchmark/admission tests plus native/error/unit-conversion tests verify it. |
| F02 | Three `source_omitted` errors / EPIC-04 evidence | `str(Path.relative_to(...))` creates Windows backslashes while mutation keys and Git fingerprints use `/` | Canonical POSIX relative keys in fixture source/artifact maps, installed report writer and acceptance reader. Missing-source rejection still runs for component, quality and installed reports. Positive admission and tamper rejection remain required. |
| F03 | Missing installed-qualification report / test harness | `env={}` removes Git lookup from Windows PATH, failing before the intended missing-database prerequisite | Preserve the runner environment and remove only `CG_COGNITIVE_TEST_DATABASE` case-insensitively. Require the actual BLOCKED report, no client identity and no closure authority. This does not change the child CG credential-isolation environment. |
| F04 | `nonminimal_bool` / application worker fabric | Negated `is_some_and` is an unnecessarily complex expression | Equivalent `is_none_or` rejects both missing and wrong-token leases. Existing fencing/retry tests and measured worker-fabric coverage verify behavior. |
| F05 | `unused_mut` / daemon bounded process | Builder mutation occurs only under Unix configuration | Make the binding mutable only inside the Unix configuration block. Keep mode `0700` and process/resource limits unchanged; validate Windows and Linux builds. |
| F06 | Timeout-test `Option::unwrap` / MCP test harness | Deadline can expire before transport polling captures the context, or before host dispatch | Observe the host channel after timeout. A received context must be cancelled; channel disconnection establishes no host dispatch. A stalled worker still fails. Keep timeout response, timeout metric, closed phase and existing duration limits. |
| F07 | Broader Windows tests cannot launch `python3` / environment | Windows Store alias is not an interpreter; bare Python also lacks contract dependencies | Install pinned contract dependencies in a task-local virtual environment. Run the Linux-oriented full integration suite under existing Ubuntu WSL with an isolated Cargo target. No product fallback or skipped gate is introduced. |

Windows `PeakWorkingSetSize` is the operating system's peak resident working set
in bytes ([Microsoft contract](https://learn.microsoft.com/en-us/windows/win32/api/psapi/ns-psapi-process_memory_counters)).
Linux retains KiB-to-byte conversion; macOS retains its byte unit. All measurements
cover the current process including imports, as the original benchmark specifies.

## Completion evidence

| ID | Verification against the repaired candidate | Status |
| --- | --- | --- |
| F01 | Real experiment/admission under Windows and Linux; helper 100% measured line coverage; native failure and unit-conversion checks | VERIFIED |
| F02 | Positive admission plus missing-source, tamper and insufficient-evidence refusals in the architecture suite on Windows and Linux | VERIFIED |
| F03 | Missing-database runner returns BLOCKED before client launch on Windows and Linux | VERIFIED |
| F04 | Worker fabric coverage: domain 100%, application 95.10%, daemon 98.99%; worker regression passes under Linux | VERIFIED |
| F05 | Windows/Linux workspace Clippy passes; Linux bounded-process regression and 100% measured line coverage | VERIFIED |
| F06 | 20 consecutive Windows timeout-test runs pass; Linux workspace test passes; no production timeout change | VERIFIED |
| F07 | Task-local contract dependencies; full Linux workspace 884 passed, 3 existing PostgreSQL tests ignored; physical Linux host also passes architecture and contract suites | VERIFIED |

Final decision: **#191 READY FOR COMPLETION** at its required component and
architecture evidence level. Windows architecture: 40 tests passed. Linux
architecture: 39 passed, one Windows-native error-path test intentionally skipped
and executed on Windows. Contracts: 17 passed. Both Windows and Linux Clippy
pass with `-D warnings`; formatting and the architecture guard pass.

The user-provided Linux test host ran an isolated candidate snapshot under
`~/cg-tests/epic0513-20261017`, without changing an existing checkout. It passed
the same 40-test architecture suite (one Windows-only skip) and 17 contract tests.
It has no Rust toolchain, so Rust workspace and coverage evidence comes from
Ubuntu WSL. The first WSL target exhausted `/tmp` tmpfs; the corrected run used
disk storage with two build jobs. Relocated build artifacts containing old binary
paths were rebuilt before the successful run. No gate was disabled or relaxed.

The final [source-bound report](evidence/EPIC-05.13-process-boundary.json) records
executed checks, coverage and limits. Historical failed attempts remain distinct
from the corrected candidate. Full installed-client/database/container release
qualification is outside #191 and must not be inferred from these tests. Issue
closure readiness refers to the bounded #191 acceptance criteria, not its parent.
