#!/usr/bin/env python3
"""Require >=95% measured line coverage for every CG-11 production file."""
import json
import sys

EXPECTED = (
    "crates/gateway-daemon/src/bin/cg.rs",
    "crates/gateway-daemon/src/declarative_cli/mod.rs",
    "crates/gateway-daemon/src/declarative_cli/inputs.rs",
    "crates/gateway-daemon/src/declarative_cli/pipeline.rs",
    "crates/gateway-daemon/src/declarative_cli/json_input.rs",
)


def check(report):
    files = [file for data in report["data"] for file in data["files"]]
    result = []
    for path in EXPECTED:
        matches = [f for f in files if f["filename"].replace("\\", "/") == path
                   or f["filename"].replace("\\", "/").endswith("/" + path)]
        if len(matches) != 1:
            raise ValueError(f"Expected exactly one coverage entry: {path}")
        lines = matches[0]["summary"]["lines"]
        count, covered = lines["count"], lines["covered"]
        if (type(count) is not int or type(covered) is not int
                or count <= 0 or not 0 <= covered <= count):
            raise ValueError(f"Invalid coverage counts: {path}")
        if 100 * covered < 95 * count:
            raise ValueError(f"Below 95% coverage: {path}: {covered}/{count}")
        result.append(f"{path}: {covered}/{count} lines ({100 * covered / count:.2f}%)")
    return result


def self_test():
    def report(covered=95, count=100):
        return {"data": [{"files": [
            {"filename": "D:\\repo\\" + path.replace("/", "\\"),
             "summary": {"lines": {"count": count, "covered": covered}}}
            for path in EXPECTED]}]}
    assert len(check(report())) == len(EXPECTED)
    invalid = [report(94), report(9499, 10000), report(0, 0), report(101),
               report(-1), report(True), report(95.0), {"data": [{"files": []}]}]
    duplicate = report()
    duplicate["data"][0]["files"].append(duplicate["data"][0]["files"][0])
    invalid.append(duplicate)
    for case in invalid:
        try:
            check(case)
        except ValueError:
            continue
        raise AssertionError("Coverage gate accepted invalid evidence")
    print("CLI coverage gate self-tests passed")


if __name__ == "__main__":
    if sys.argv[1:] == ["--self-test"]:
        self_test()
    else:
        with open(sys.argv[1], encoding="utf-8") as source:
            print("\n".join(check(json.load(source))))
