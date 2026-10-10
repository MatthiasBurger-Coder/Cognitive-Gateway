#!/usr/bin/env python3
"""Validate live stdio discovery and every frozen request against independent schemas."""
import argparse
import json
from pathlib import Path
import subprocess
import queue
import threading

from jsonschema import Draft202012Validator
from referencing import Registry, Resource

ROOT = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, help="Built cg-mcp executable")
    args = parser.parse_args()
    binary = args.binary
    if binary is None:
        metadata = json.loads(subprocess.check_output(
            ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"], cwd=ROOT))
        binary = Path(metadata["target_directory"]) / "debug/cg-mcp"
    schemas = {path.name: json.loads(path.read_text())
               for path in (ROOT / "schemas/codex/v1").glob("*.schema.json")}
    registry = Registry().with_resources(
        (schema["$id"], Resource.from_contents(schema)) for schema in schemas.values())
    response_validator = Draft202012Validator(schemas["response.schema.json"], registry=registry)
    frames = [json.loads(line) for line in
              (ROOT / "tests/fixtures/local-mcp/lifecycle.jsonl").read_text().splitlines()]
    catalog = json.loads((ROOT / "schemas/codex/v1/catalog.json").read_text())
    calls = []
    for index, tool in enumerate(catalog["tools"], 100):
        envelope = json.loads((ROOT / "tests/fixtures/codex-v1" /
                               (tool["operation"] + ".request.json")).read_text())
        calls.append((tool, envelope))
        frames.append({"jsonrpc": "2.0", "id": index, "method": "tools/call",
                       "params": {"name": tool["name"], "arguments": envelope}})
    command = [str(binary.resolve()), "--client-name", "codex", "--client-version", "1.0",
               "--principal", "operator", "--workspace", "workspace-example",
               "--project", "project-example", "--binding", "binding-example"]
    process = subprocess.Popen(command, text=True, stdin=subprocess.PIPE,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE, env={})
    replies = queue.Queue()
    def read_replies():
        for line in process.stdout:
            replies.put(line)
        replies.put(None)
    reader = threading.Thread(target=read_replies, daemon=True)
    reader.start()
    messages = []
    try:
        for frame in frames:
            process.stdin.write(json.dumps(frame) + "\n")
            process.stdin.flush()
            if "id" in frame:
                line = replies.get(timeout=10)
                if line is None:
                    raise ValueError("MCP closed before responding")
                messages.append(json.loads(line))
        process.stdin.close()
        if process.wait(timeout=10) != 0:
            raise ValueError("MCP process failed")
        reader.join(timeout=10)
        if replies.get(timeout=10) is not None:
            raise ValueError("Unexpected extra MCP response")
        diagnostics = process.stderr.read()
    finally:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=10)
        process.stdout.close()
        process.stderr.close()
        if not process.stdin.closed:
            process.stdin.close()
    for line in diagnostics.splitlines():
        event = json.loads(line)
        if set(event) != {"event", "correlation_id", "failure_class", "elapsed_ms"}:
            raise ValueError("Unexpected MCP diagnostic fields")
        if event["event"] != "local_call_finished":
            raise ValueError("Unexpected MCP diagnostic event")
    if len(messages) != 6 + len(calls):
        raise ValueError("Unexpected MCP response count")
    results = {str(message["id"]): message["result"] for message in messages}
    discovered = {tool["name"]: tool for tool in results["3"]["tools"]}
    if set(discovered) != {tool["name"] for tool in catalog["tools"]}:
        raise ValueError("Incomplete MCP tool surface")
    for index, (tool, envelope) in enumerate(calls, 100):
        projection = discovered[tool["name"]]
        for key in ("inputSchema", "outputSchema"):
            Draft202012Validator.check_schema(projection[key])
        Draft202012Validator(projection["inputSchema"]).validate(envelope)
        result = results[str(index)]
        response_validator.validate(result["structuredContent"])
        Draft202012Validator(projection["outputSchema"]).validate(result["structuredContent"])
        if (result["isError"] is not True
                or result["structuredContent"]["diagnostics"][0]["code"] != "CG_UNSUPPORTED_CAPABILITY"
                or json.loads(result["content"][0]["text"]) != result["structuredContent"]):
            raise ValueError("Invalid MCP tool result projection")
    print("Local MCP independent protocol/schema conformance passed (13 tools, no provider environment)")


if __name__ == "__main__":
    main()
