# CG-18: governed memory and experience retrieval

Issue: [#196](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/196). Contract version: `MEMORY_SCHEMA_VERSION = 1`.

## Contract and ownership

`gateway-domain::memory` defines `ExperienceRecord`, `MemoryEntry`, lifecycle state, stable reason IDs and `MemoryEligibilityReference`. `gateway-application::memory` owns admission, curation, search, retrieval, export-reference validation and the `MemoryStore` port. `gateway-daemon::memory::InMemoryMemoryStore` is a replaceable proof adapter; it atomically checks revisions, replaces the current projection and appends a curation decision. It is process local and is not a durable deployment store. A durable adapter must preserve the same atomic compare-and-swap and erasure semantics before production use.

Every record carries schema version 1, a stable ID and scope, provenance ID, immutable source snapshot reference, source version and SHA-256 digest, created/observed/valid-from/expiry times, explicit maximum age, quality (including confidence, sensitivity, freshness, uncertainty and conflict), validation reference, outcome and label basis, and inline or reference payload. Confidential and secret payloads require reference representation. The record is derived information; its text has no capability, policy or process authority.

The application requires an explicit timestamp and reason reference on every decision. `CurationDecision` stores scope, input snapshot and revision, action, reason, time, output ID and revision. Decisions are append only. Admission rejects duplicate IDs and identical source snapshots; the same snapshot with a changed digest is an explicit conflict. Refresh requires a different snapshot and returns to pending validation. Validation requires a validation reference, scored confidence, fresh and conflict-free quality, a valid interval, and reference representation for sensitive payloads. Rejected, invalidated, superseded and forgotten records cannot be recalled. A forgotten record retains a tombstone and decision history but no payload; it cannot be refreshed or readmitted under the same ID.

`MemoryEntry::reasons(at)` returns stable reason IDs for current-context eligibility. `learning_reasons(at)` additionally requires outcome and label basis. A record must have only `MEMORY_ELIGIBLE` to qualify. The explicit evaluation time enforces validity and maximum age, even if stored quality was initially fresh. Search is deterministic lexical matching over eligible inline payload, outcome and source snapshot identity, with explicit project scope, sensitivity ceiling and result limit. Reference payloads require an external index for content search.

The CG-10 bridge emits `FragmentKind::Memory` with derived-assessment trust, source revision and validation reference. Reference payloads remain references in the semantic context. The compiler retains its existing authority boundary: memory is dynamic data and cannot change policies, capabilities or process transitions. Current evidence is a separate fragment kind and is never overwritten by memory.

An export pins a `MemoryEligibilityReference` containing schema version, scope, record ID, revision, eligibility version and source snapshot/digest. Consumers must call `revalidate_reference` at use time. Invalidation, supersession, refresh or forgetting increments the eligibility version; a pinned reference then fails. Expiry or staleness can also make the reference ineligible without a write. Existing manifests stay historical; no consumer may treat a previous positive result as a permanent grant. Export assembly and train/test splitting remain with CG-20 and EPIC-03.

## Requirement to verification matrix

| Requirement sentence | Implementation evidence | Verification evidence |
| --- | --- | --- |
| R01: Pending, stale, expired, uncertain, conflicted, invalid, superseded and forgotten records are excluded from current validated memory. | `MemoryEntry::reasons`, `MemoryApplication::curate`, `recall` | `governed_memory`: lifecycle, quality, search, supersession |
| R02: Recall is scoped and CG-10 fragments carry source revision, validation, provenance and derived trust without overwriting evidence. | `MemoryApplication::context_fragment`, `ContextFragment::memory_reference` | `governed_memory`: lifecycle, sensitive reference, isolation |
| R03: Admission and curation capture input snapshot, decision rule, action and output identity, with explicit duplicate/conflict handling. | `CurationDecision`, `MemoryStore::commit`, `MemoryApplication::admit/curate` | `governed_memory`: lifecycle, duplicate/conflict, refresh |
| R04: Learning eligibility requires stable source/temporal identity, schema, validation, outcome and label basis; sensitive payloads can remain references. | `ExperienceRecord`, `MemoryEntry::learning_reasons`, `MemoryEligibilityReference` | `governed_memory`: missing basis, sensitive reference |
| R05: Old export references become ineligible after refresh, invalidation, supersession or forgetting, and forgotten payload access fails. | `MemoryApplication::revalidate_reference`, revision and eligibility version, tombstone | `governed_memory`: lifecycle, refresh, supersession |
| R06: Project scopes are isolated; memory cannot create authority-bearing context. | `MemoryStore` scoped keys, `search`, CG-10 external constructor | `governed_memory`: project isolation and instruction-like text |

## Verification

Run `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`, `bash scripts/check-architecture.sh`, and `python3 scripts/quality-gate.py`. The complete gate records revision-bound evidence under `target/release-evidence/`.
