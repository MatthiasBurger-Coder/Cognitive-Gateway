# Invocation qualification v1

All inputs are **synthetic fixture evidence**. No installed model or performance
measurement is represented by these numbers. `profile.json` exercises independent
role gates, with strict 100% correctness and zero unsupported claims across ten
cases. Scalar numbers are synthetic gate inputs in the units named by the metrics.
`measurements.json` is the positive gate fixture; it cannot promote a model.

`replay-cases.json` includes nine correct answers and a schema-valid invented
entity. The Rust test parses each recorded result under the admitted contract,
compares fields against labels, independently counts claims outside the allowed
set, and replays against current synthetic host checks. Semantic and unsupported
claim gates fail even though schema conformance remains 100%.

Production qualification must supply a versioned, role-specific dataset, trusted
grader/case records, honest sample counts and exact model/runtime/tokenizer/profile/
renderer/dataset pins. The examples are not suitable production thresholds or pins.
Raw outputs require policy-permitted scoped storage. Trace receipts contain only
correlation metadata. No hidden chain-of-thought or credentials belong in evidence.
