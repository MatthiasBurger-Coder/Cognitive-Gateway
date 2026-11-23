# Retained connector/session workspace reconciliation (#412)

Historical evidence and removed review documents are linked to their archived Git
revision. They are not current acceptance evidence. Generate fresh reports in an
output directory or retain them as CI artifacts.

The complete original integration workspace is retained in Git commit `1b6c98902b5f0108b93c2ee30d07ecc5f8f7291b`.
All 193 initially outstanding files, including source, tests, historical evidence
and two generated raw profiles, are preserved. The [manifest](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/blob/59f6db7e29478e2903cf69d6e5c7e65632876710/docs/evidence/retained-workspace-reconciliation.json)
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

The old credential-isolation branch was delivered by squash PR #368. Its original
commit ancestry is also retained through a tree-preserving merge. The old
sub-issue-skill branch (PR #370) is already an ancestor of the original integration
snapshot. This allows removal of these stale branch references after their
preservation reaches main, without dropping otherwise unreachable commits.

The final production-tree basis is `a81ca8c` (merged #408 / PR #413), following
#407 / PR #409 and #231 / PR #411. Relative to that qualified main tree, this
reconciliation changes only this documentation, its hash manifest and the root
profile ignore rule. The original seven-file #408 draft is also retained in
commit `5edad65805fe037afe13a7b019f8165f1b37fadb`, now an ancestor of main.
All 193 original byte hashes were reconstructed again before final delivery.
