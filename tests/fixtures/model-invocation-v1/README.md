# Model invocation v1 fixtures

`profile.json` is a provider-neutral five-control generation profile.
`profile.canonical.json` freezes deterministic bytes and `profile.sha256` pins them.
`negative-profiles.json` contains incompatible-version, unbounded/zero-output and
provider-field rejection fixtures. `support.rs` constructs synthetic captured
provider/capability/task/context/authority bindings shared by domain/application
tests; it grants no real authority and performs no model calls.

Tests also mutate each boundary, including Rust-only NaN/infinities and unsupported
control/range declarations. See [the contract guide](../../../docs/model-invocation-contracts.md).
