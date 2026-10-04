# Codex boundary v1 golden examples

Normative EPIC-04.02 examples, not captured MCP server responses. See
[the contract](../../../docs/codex-facing-contracts.md) for ownership,
compatibility, canonical ordering and diagnostic rules.

Every operation has a request/response pair. Session pairs return unsupported
because #272 is planned. `projection.*` responses are future session shapes,
not current capability claims. `CG_*.response.json` freezes every diagnostic.
All-zero digests denote symbolic unresolved references; they grant no authority.
`assessment.resource.json` alone pins the exact bytes of the existing CG-12
assessment fixture and retains its canonical document. The assessment tool
result contains that same existing payload unchanged.

Validate without network schema access:

```sh
python3 -m pip install -r tests/contracts/requirements.txt
python3 -m unittest discover -s tests/contracts -v
cargo test -p gateway-daemon --test codex_contracts --locked
```

Tests do not regenerate expectations. Intentional changes require review against
the exact published version, updating a new version when compatibility changes.
