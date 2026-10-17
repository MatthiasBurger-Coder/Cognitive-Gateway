"""Independent offline CGSL 1.0 executable-source conformance witness.

Specification inputs: docs/cgsl-language-v1.md and semantic-task schema. This
implementation imports no Rust implementation, mapping, serializer or fixture
expectation. It implements the valid executable-source projection, not hosted
resolution or the frontend diagnostic/span protocol. Never used for admission.
Input/output: JSON arrays of source strings / canonical task records on stdio.
"""
import hashlib
import json
from pathlib import Path
import sys

from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parents[2]
SCHEMA = Draft202012Validator(json.loads(
    (ROOT / "schemas/semantic-task.schema.json").read_text(encoding="utf-8")))
# Independently transcribed from the normative root mapping, not loaded from code.
FIELDS = dict(zip(
    "task goal target input context state desired_state observation history constraint requires evidence assumption policy output verification process".split(),
    "task_type goal target inputs context_refs current_state desired_state observations history constraints capability_requirements evidence_refs assumptions policy_refs output_contract verification_contract process".split()))
COLLECTIONS = "inputs context_refs observations history constraints capability_requirements evidence_refs assumptions policy_refs".split()
CHECKS = ["OUTPUT_SCHEMA_VALID", "EVIDENCE_REQUIRED", "ALL_CLAIMS_SUPPORTED", "NO_UNRESOLVED_REFERENCE"]


def require(condition, message):
    if not condition:
        raise ValueError(message)


def unique_pairs(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate member")
        result[key] = value
    return result


def canonical(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False)


def ordered(values, field):
    require(len({v[field] for v in values}) == len(values), "duplicate identity")
    return sorted(values, key=lambda v: v[field])


def literal(value):
    kind, item = value["kind"], value["value"]
    if kind == "DECIMAL":
        coefficient = int(item["unscaled"])
        require(-(2**127) <= coefficient < 2**127, "decimal overflow")
        item["unscaled"] = str(coefficient)
    elif kind == "SET":
        require(len({v["kind"] for v in item}) == 1, "heterogeneous set")
        for v in item:
            literal(v)
        require(len({canonical(v) for v in item}) == len(item), "duplicate set value")
        item.sort(key=lambda v: (int(v["value"]["unscaled"]), v["value"]["scale"])
                  if v["kind"] == "DECIMAL" else v["value"])


def expression(value, known):
    kind, item = value["kind"], value["value"]
    if kind == "CONDITION":
        require(item in known, "unknown condition")
        return "CONDITION:" + item
    if kind == "NOT":
        return "NOT:" + expression(item, known)
    keys = [(expression(v, known), v) for v in item]
    keys.sort(key=lambda p: p[0])
    value["value"] = [v for _, v in keys]
    return kind + ":[" + ",".join(k for k, _ in keys) + "]"


def compile_source(source):
    require(len(source.encode("utf-8")) <= 1048576, "source limit")
    document = json.loads(source, object_pairs_hook=unique_pairs,
                          parse_constant=lambda _: (_ for _ in ()).throw(ValueError("non-JSON number")))
    require(isinstance(document, dict) and document.get("cgsl") == "1.0", "language version")
    require(set(document) <= set(FIELDS) | {"cgsl", "id", "knowledge_gap", "ambiguity", "confidence"}, "unknown construct")
    require(not any(document.get(f, []) for f in ("knowledge_gap", "ambiguity", "confidence")), "inspection-only source")
    task = {"schema_version": "1.0", "id": document["id"]}
    for source_name, task_name in FIELDS.items():
        if source_name in document:
            task[task_name] = document[source_name]
        elif task_name in COLLECTIONS:
            task[task_name] = []
        elif task_name in ("current_state", "desired_state", "process"):
            task[task_name] = None
    SCHEMA.validate(task)
    for name in COLLECTIONS:
        task[name] = ordered(task[name], "role" if name == "inputs" else "id")
    for item in task["inputs"]:
        if item["value"]["kind"] == "LITERAL":
            literal(item["value"]["value"])
    for item in task["assumptions"]:
        literal(item["premise"])
        if item["confidence"]["kind"] == "SCORE":
            # Confidence is a bounded basis-point score in the CG-06 contract.
            item["confidence"]["value"] = int(item["confidence"]["value"] * 10000 + 0.5) / 10000
    task["verification_contract"]["checks"].sort(key=CHECKS.index)
    scope = task["target"]["reference"]["scope"]

    def references(v):
        if isinstance(v, dict):
            if "contract_version" in v:
                require(v["scope"] == scope, "foreign scope")
            for child in v.values():
                references(child)
        elif isinstance(v, list):
            for child in v:
                references(child)
    references(task)
    desired = task["desired_state"]
    if desired:
        for name in ("conditions", "constraints", "acceptance_criteria"):
            desired[name] = ordered(desired[name], "id")
        known = {c["id"] for c in desired["conditions"]}
        for condition in desired["conditions"]:
            condition.setdefault("expected", None)
            if condition["expected"] is not None:
                literal(condition["expected"])
        expression(desired["expression"], known)
        for c in desired["constraints"] + desired["acceptance_criteria"]:
            expression(c["expression"], known)
    result = canonical(task)
    return {"canonical_ir": result, "sha256": hashlib.sha256(result.encode("utf-8")).hexdigest()}


if __name__ == "__main__":
    sources = json.load(sys.stdin)
    json.dump([compile_source(s) for s in sources], sys.stdout, ensure_ascii=False)
