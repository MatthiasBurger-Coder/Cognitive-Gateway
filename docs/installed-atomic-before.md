# Installed atomic Before capture (#408)

The native readonly session runtime can prepare a complete Before snapshot when
its original connector goal remains unfulfilled. It returns Runnable/Prepared
only after the existing coordinator commits and reloads the actual PostgreSQL
record. No model, action, After snapshot or goal receipt is created. Already
satisfied goals retain their existing observation/verification path.

The optional `observed_goal_runtime.complete_before` registration names full
`universe`, `profile`, `provider` pins and one `read` admission. Each pin has its
own entry in `documents`; paths are absolute, bounded, regular files. See the
executable configuration builder in
`crates/gateway-daemon/tests/support/complete_before_session.py` for a complete
native CLI generated example. These definitions are distinct from the older
`cg.connector-state-universe` export contract.

The universe uses `cg.scoped-snapshot-universe/1.0`; all observable members,
subjects and version subjects are explicitly registered independently of goal
conditions and model context. The profile uses `cg.scoped-snapshot-profile/1.0`
and this installed consumer supports only `ATOMIC_COMPLETE_EXPORT`. Native object
descriptors must bind members to the task target and actual task input references.
The provider document `cg.connector-atomic-export-provider/1.0` binds the exact
native task, projection, mapping, universe, profile, root read route, target,
complete member pins, selector coordinates and response identity fields. Its
semantics is `ATOMIC_COMPLETE_EXPORT_V1`.

That semantic declaration is an operator qualification requirement. It does not
turn an arbitrary single RPC into proof of consistency. The controlled provider
used here reads one immutable complete world document once, with atomic rename
for updates, emits a genuine global non-reused epoch, observed membership digest
and per-object versions, and requires its admitted credential environment. An
external provider requires separate evidence that it implements this contract.
The stored acquisition keeps the actual export-root request and metadata; member
reads reference that one acquisition without inventing per-object requests.

Discovery, raw schemas, selectors, normalization, quotes and every read pass the
existing credential, native Process/Policy/Context/CG09, current definition,
claim/fence, resource and response identity guards. The observed export revision
must equal the observed global epoch. Full membership and object version evidence
are checked by the complete snapshot owner. Current definition/context basis is
rechecked around persistence. Historical inspection grants no current eligibility.

Profile boundaries are explicit: at most 256 flat primitive normalized fields
including epoch, membership and object versions; at most 262144 raw scalar JSON
bytes; bounded registered metadata; and the existing 1,000,000-byte Before record
and private operator envelope limits. Discovery and all selected goal/world reads
share one cumulative reservation and spending cursor. Oversized/unsupported
worlds fail; the universe is never truncated. Non-atomic live profiles and mixed
or mutating native closures remain unsupported. Shipped RPC, runtime and claim
deadlines remain unchanged. Qualification uses optimized binaries; a debug build
can exceed those real deadlines on the larger complete-world fixture.

`cg-local --operation session.authority` with the existing strict
`session_id`/`issuer`/`decision` request and `decision: inspect-before` reads the
immutable historical envelope under its actual owner/selected store. Restart,
duplicate command, inspection, recovery and marker-only Continue perform no new
provider acquisition, reservation, timestamp refresh or model/action dispatch.
A new marker Continue remains unsupported until its model consumer is delivered.
Uncommitted interrupted claims and uncertain commits are resolved from durable
state through the existing coordinator; no blind provider retry is introduced.

Reproduce installed qualification with:

```sh
CARGO_BUILD_JOBS=1 python3 scripts/cognitive-test-host.py -- \
  bash scripts/qualify-atomic-before.sh /tmp/atomic-before-evidence
```

The disposable host supplies real PostgreSQL. Without it, conditional installed
tests provide no installed qualification. Controlled native executable evidence
is not external installed-client or production-provider acceptance. The wider
#271/#274/#277 model, mutation, consent, After and system criteria remain open.
