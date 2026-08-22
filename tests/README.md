# Tests

This directory contains cross-crate architecture and contract-test specifications. Unit tests remain colocated with the owning crate; integration tests that exercise more than one crate belong here when the workspace has behavior to exercise.

The bootstrap quality baseline is the executable architecture guard at [`../scripts/check-architecture.sh`](../scripts/check-architecture.sh), together with the workspace commands documented in the root README:

```text
cargo fmt --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

The [`architecture/`](architecture/) area records cross-crate architecture-test contracts. Schema and Execution Context IR compatibility tests are added with the corresponding domain and profile-loading slices; CG-01 intentionally introduces no provider, RAG or MCP behavior to test.
