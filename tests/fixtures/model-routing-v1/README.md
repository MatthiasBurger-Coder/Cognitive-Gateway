# Deterministic model routing v1

`support.rs` composes the existing CG-26 and EPIC-06.01 fixtures. It supplies two
explicitly admitted model alternatives with the same invocation meaning and
separate capability pins. No model runtime or network is involved.

`gateway-application/tests/model_routing.rs` derives negative fixtures for provider
health, model identity, task/output/strategy support, context limit, generation
controls, locality, sensitivity, hardware, cost and latency, and independently
admitted alternatives with changed authority/context/output/tools/budgets.
Integration cases compile actual CG-10 steps through the existing fixture and
record a fake host's calls, fallback decisions and cumulative reservations.
