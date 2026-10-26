# Complete scoped snapshot data

Issue [#405](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/405)
defines the application data contract needed by
[#274](https://github.com/MatthiasBurger-Coder/Cognitive-Gateway/issues/274).
It does not install a snapshot acquirer, invoke a model, dispatch an effect,
accept consent or establish goal success.

The snapshot binds the supplied owner identity and project, canonical scope,
session, run, task, execution, capture revision, step, dispatch and data phase.
An actual resource reservation and fence are retained when present. A later
database commit revision is distinct from the capture revision. No invocation
record is invented before a model invocation. The runtime consumer must
authenticate the owner; the codec does not perform authentication.

The closed universe is supplied by the qualified scope provider independently
of the original goal or selected context. It contains every registered
observable object, its full pin, source identity, actual object reference,
required subjects and exact pinned read route. Two objects can share one source
and mapping. A goal-only projection cannot establish a complete snapshot.
Distinct objects cannot reuse the same canonical subject as though it described
both objects. Corroborating sources for the same fully pinned object may share
subjects.

An immutable complete wrapper is created only after checking exact membership,
subject coverage, native observation provenance, quality, freshness and global
consistency. Its digest is derived from the validated canonical data. There is
no caller-supplied completeness flag. Pinned profile declarations describe
provider contracts; authenticating and qualifying the actual provider remains
the responsibility of the runtime consumer.

The supported profile retains direct, non-probabilistic values with
`Confidence::NotApplicable`, fresh observed evidence and no unresolved
uncertainty or conflict. It requires matching affirmed facts and their actual
observation lineage, supported evidence and no challenge edges. A negated fact
does not establish a direct value; an affirmed Boolean `false` does. Unknown or
probabilistic confidence cannot be converted into certainty by the codec.

## Global consistency

An atomic complete export requires a provider contract explicitly qualifying
the export as atomic for the entire scope. One actual acquisition must cover
all universe members. The observation retains the actual export-root provenance;
the snapshot does not manufacture separate object requests or copy the root
revision into each object's version. One RPC alone does not prove atomicity.

A versioned complete set requires a genuine global epoch and membership
anchor under the same full authority/domain pin. Every object read must
declare that exact authority/domain pin in its qualified route and
participate in that epoch, either through snapshot-scoped requests or through
qualified non-reused global anchors captured before and after all reads.
Matching strings from different domains and independent object revisions do
not establish a common epoch. Per-object versions remain separate evidence.
Missing anchors, changed membership, weak or conflicting evidence fail closed.

Before and After are data phases. An After-labelled data object does not prove
that an effect occurred, that a later read was causally independent, or that
the goal succeeded. Those admissions belong to the session journal, effect
owner and independent goal verifier.

## Persistence and reload

The codec uses bounded canonical data, rejects duplicate and unknown fields
at every encoded level, validates native CG-06 records and reconstructs the
complete wrapper through the same validation boundary. Reload compares the
expected owner/capture binding, universe and qualified profile and checks the
derived digest. Reloaded data is historical evidence; it grants no current
policy, consent, budget or connector authority.

## Required next delivery

The mandatory consumer must acquire a complete world from a real qualified MCP
provider, including non-goal objects and subjects. It must persist Before
through the actual PostgreSQL/session owner and reload it before any model or
effect. Its integration tests must exercise incomplete universes, mixed global
versions, cancellation and deadlines. This data contract cannot satisfy those
runtime criteria or close #274 or its parent #271.

The preserved integration worktree is reference material. Existing installed
artifact and readonly-goal paths, frozen public v2 schemas and execution gates
remain regression boundaries.

Qualification uses `scripts/qualify-complete-snapshots.sh` and the unchanged
strict 95% line gate for each of the four executable production modules.
`types.rs` contains declarations only and has no executable LLVM line entry;
its types participate in compilation and the contract tests. It is not assigned
an invented coverage percentage.
