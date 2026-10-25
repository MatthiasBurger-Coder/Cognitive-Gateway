# Discovery and mapping v1 vectors

`manifest.json`, `mapping.json` and `discovery.json` describe a synthetic connector
with one tool, one exact resource, one prompt and one metadata-only template.
`snapshot.json` is the exact compact canonical audit bytes, with no trailing
newline; `revision.txt` is their SHA-256. Python contract tests independently
reconstruct the entire vector and all manifest schema digests. Rust tests compare
the same bytes and use the mapping with the independent stdio conformance peer.

`unknown-tool.json` advertises an undeclared tool with permissive MCP hints. It
must fail closed; hints grant neither classification nor permission. Unit tests
also mutate the canonical inputs to cover conflicts, stale revisions, unsupported
schemas and invalid metadata. See `docs/connector-mapping-v1.md` for ownership,
encoding, diagnostics and qualification limits.

EPIC-07.05 regenerates the manifest digest, snapshot and inventory revision for
application port revision 2; discovery schemas and canonical mappings are unchanged.
