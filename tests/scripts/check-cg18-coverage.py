#!/usr/bin/env python3
"""Require 95% measured line coverage in the CG-18 production modules."""
import json
import sys

EXPECTED = (
    "crates/gateway-domain/src/memory.rs",
    "crates/gateway-application/src/memory.rs",
    "crates/gateway-context/src/compiled.rs",
    "crates/gateway-daemon/src/memory.rs",
)


def check(report):
    files = [file for data in report["data"] for file in data["files"]]
    results = []
    for path in EXPECTED:
        matches = [file for file in files if file["filename"].replace("\\", "/").endswith("/" + path)]
        if len(matches) != 1:
            raise ValueError(f"Expected exactly one coverage entry: {path}")
        lines = matches[0]["summary"]["lines"]
        count, covered = lines["count"], lines["covered"]
        if type(count) is not int or type(covered) is not int or count <= 0 or not 0 <= covered <= count:
            raise ValueError(f"Invalid coverage counts: {path}")
        if 100 * covered < 95 * count:
            raise ValueError(f"Below 95% coverage: {path}: {covered}/{count}")
        results.append(f"{path}: {covered}/{count} lines ({100 * covered / count:.2f}%)")
    return results


def self_test():
    def fixture(covered=95, count=100):
        return {"data": [{"files": [{"filename": "/repo/" + path,
                 "summary": {"lines": {"count": count, "covered": covered}}} for path in EXPECTED]}]}
    assert len(check(fixture())) == len(EXPECTED)
    for invalid in (fixture(94), fixture(0, 0), fixture(101), fixture(True), fixture(95.0)):
        try:
            check(invalid)
        except ValueError:
            continue
        raise AssertionError("Coverage gate accepted invalid evidence")
    print("CG-18 coverage gate self-tests passed")


if __name__ == "__main__":
    if sys.argv[1:] == ["--self-test"]:
        self_test()
    else:
        with open(sys.argv[1], encoding="utf-8") as source:
            print("\n".join(check(json.load(source))))
