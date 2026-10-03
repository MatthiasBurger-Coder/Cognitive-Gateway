#!/usr/bin/env python3
"""Require 95% measured line coverage in the CG-28A scheduler."""
import json
import sys

TARGET = "crates/gateway-application/src/parallel_execution.rs"


def check(report):
    files = [item for data in report["data"] for item in data["files"]]
    matches = [item for item in files if item["filename"].replace("\\", "/").endswith("/" + TARGET)]
    if len(matches) != 1:
        raise ValueError(f"Expected exactly one coverage entry: {TARGET}")
    lines = matches[0]["summary"]["lines"]
    count, covered = lines["count"], lines["covered"]
    if type(count) is not int or type(covered) is not int or count <= 0 or not 0 <= covered <= count:
        raise ValueError("Invalid CG-28A coverage counts")
    if 100 * covered < 95 * count:
        raise ValueError(f"Below 95% CG-28A coverage: {covered}/{count}")
    return f"{TARGET}: {covered}/{count} lines ({100 * covered / count:.2f}%)"


def self_test():
    def fixture(covered=95, count=100):
        return {"data": [{"files": [{"filename": "/repo/" + TARGET,
                 "summary": {"lines": {"count": count, "covered": covered}}}]}]}
    assert check(fixture()).endswith("95.00%)")
    for invalid in (fixture(94), fixture(0, 0), fixture(101), fixture(True), fixture(95.0),
                    {"data": [{"files": []}]}):
        try:
            check(invalid)
        except ValueError:
            continue
        raise AssertionError("Coverage gate accepted invalid evidence")
    print("CG-28A coverage gate self-tests passed")


if __name__ == "__main__":
    if sys.argv[1:] == ["--self-test"]:
        self_test()
    else:
        with open(sys.argv[1], encoding="utf-8") as source:
            print(check(json.load(source)))
