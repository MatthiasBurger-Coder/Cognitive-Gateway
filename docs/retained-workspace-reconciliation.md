# Retained connector/session workspace reconciliation (#412)

The complete original integration workspace is retained in Git commit `1b6c98902b5f0108b93c2ee30d07ecc5f8f7291b`.
All 193 initially outstanding files, including source, tests, historical evidence
and two generated raw profiles, are preserved. The [manifest](evidence/retained-workspace-reconciliation.json)
records each original SHA-256, its actual Git blob, stored hash and line-ending
restoration metadata. No original payload needs to remain untracked locally.

The checkpoint is historical input, not release acceptance. Its prototype model,
connector execution and budget implementations overlap newer native deliveries.
The reconciliation merges its ancestry while retaining current main's production
tree. It does not install deprecated paths, overwrite qualified native services,
or relabel historical reports as current evidence. An original file is available
with `git show <preservation-commit>:<path>`; the manifest also identifies CRLF
restoration when Git normalized text. Generated profiles remain in history and
are ignored in the active checkout.

Before-journal delivery is tracked by #407 / PR #409. Actual atomic acquisition
is tracked separately by #408 / PR #413. These bounded deliveries do not close
the wider #271/#274/#277 model, effect, reconciliation and system criteria.

After verified merges, the integration and delivery branch references can be
removed without losing the preservation commit, since main retains its ancestry.
