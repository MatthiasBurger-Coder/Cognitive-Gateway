# Repository-bound GitHub MCP example

See [operator instructions and the external issue scenario](../../docs/github-reference-connector.md).
`bundle.json` is the generated `cg-github-mcp describe owner/repo` export.
It contains no credentials and grants no permission. Replace the repository and
bind the full executable/argument configuration through existing scoped admission.

The four independent [response fixtures](../../tests/fixtures/github-connector-v1/responses.json)
exercise repository, issue, PR and CI through the same canonical host in
`crates/gateway-daemon/tests/github_connector.rs`. `curl-peer.py` replaces HTTPS
only and must never be deployed as a production curl executable.
