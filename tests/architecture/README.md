# Architecture Tests

Cross-crate architecture tests and dependency-boundary checks belong here. They verify that the source layout and Cargo dependency graph continue to follow the inward dependency rule:

```text
Driving Adapters -> Application Ports -> Domain/Core
Application + Domain/Core -> Outbound Ports -> Driven Adapters
```

The current executable check is [`../../scripts/check-architecture.sh`](../../scripts/check-architecture.sh). Future contract tests may be added here without moving domain behavior into this directory.
