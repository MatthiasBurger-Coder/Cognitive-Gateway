#!/usr/bin/env python3
"""Independent stdio conformance peer. Test-only; never a production connector."""
import json
import os
import sys
import time

mode = sys.argv[1]
initialized = False
call_count = 0

def emit(value):
    print(json.dumps(value, separators=(",", ":")), flush=True)

def record(value):
    if len(sys.argv) > 2:
        with open(sys.argv[2], "a", encoding="utf-8") as log:
            log.write(json.dumps(value) + "\n")

record({"pid": os.getpid(), "environment": dict(os.environ), "directory": os.getcwd()})
for line in sys.stdin:
    request = json.loads(line)
    record(request)
    method = request.get("method")
    if method == "notifications/initialized":
        initialized = True
        continue
    if "id" not in request:
        continue
    ident = request["id"]
    if (mode, method) in (("rate-init", "initialize"), ("rate-discovery", "tools/list"), ("rate-health", "ping")):
        emit({"jsonrpc": "2.0", "id": ident, "error": {"code": -32029, "message": "SECRET", "data": {"retryAfterMs": 1000}}})
        continue
    if method == "initialize":
        if mode == "slow-init":
            time.sleep(2)
        result = {"protocolVersion": "2099-01-01" if mode == "version" else "2025-11-25",
                  "capabilities": {} if mode == "empty" else {"tools": {}, "resources": {}, "prompts": {}},
                  "serverInfo": {"name": "independent-conformance-peer", "version": "1.0.0"}}
    else:
        assert initialized, "operation before initialized notification"
        if method == "tools/list":
            schema = {"type": "object", "properties": {"query": {"type": "string"}}, "required": ["query"], "additionalProperties": False}
            result = {"tools": [{"name": "records.read", "inputSchema": schema}]}
            if mode == "pages" and "cursor" not in request["params"]:
                result = {"tools": [{"name": "z", "inputSchema": schema}], "nextCursor": "page-2"}
            if mode in ("output-schema", "output-mismatch"):
                result["tools"][0]["outputSchema"] = {"type": "object", "properties": {"value": {"type": "string"}}, "required": ["value"], "additionalProperties": False}
            if mode == "duplicate":
                result["tools"] *= 2
            if mode == "cycle":
                result["nextCursor"] = "same"
        elif method == "resources/list":
            result = {"resources": [{"uri": "records://one", "name": "one"}]}
        elif method == "resources/templates/list":
            result = {"resourceTemplates": [{"uriTemplate": "records://{id}", "name": "record"}]}
        elif method == "prompts/list":
            result = {"prompts": [{"name": "summary"}]}
        elif method == "tools/call":
            call_count += 1
            if mode in ("rate-limit", "rate-once", "rate-long", "rate-invalid", "rate-default", "rate-policy-short") and (mode != "rate-once" or call_count == 1):
                error = {"code": -32029, "message": "SECRET_PROVIDER_MESSAGE"}
                if mode != "rate-default":
                    error["data"] = {"retryAfterMs": "bad" if mode == "rate-invalid" else (4000 if mode == "rate-policy-short" else (1000 if mode == "rate-long" else 5))}
                emit({"jsonrpc": "2.0", "id": ident, "error": error})
                continue
            if mode in ("slow", "cancel", "uncertain"):
                time.sleep(0.4)
            if mode == "eof":
                sys.exit(0)
            if mode == "invalidated":
                emit({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"})
            if mode == "server-request":
                emit({"jsonrpc": "2.0", "id": "forbidden", "method": "sampling/createMessage", "params": {}})
                answer = json.loads(sys.stdin.readline())
                record(answer)
                assert answer["error"]["code"] == -32601
                emit({"jsonrpc": "2.0", "id": "ping", "method": "ping"})
                assert json.loads(sys.stdin.readline())["result"] == {}
            if mode == "flood":
                for _ in range(20):
                    emit({"jsonrpc": "2.0", "method": "notifications/progress", "params": {}})
            if mode == "oversize":
                print("x" * 10000, flush=True)
                continue
            if mode == "malformed":
                print('{"jsonrpc":"2.0","id":1,"id":2,"result":{}}', flush=True)
                continue
            if mode == "remote":
                emit({"jsonrpc": "2.0", "id": ident, "error": {"code": -32603, "message": "SECRET_PROVIDER_MESSAGE", "data": "SECRET"}})
                continue
            value = os.environ.get("TEST_TOKEN", "fixture value") if mode == "secret-echo" else "fixture value"
            result = {"content": [{"type": "text", "text": value}], "structuredContent": {"value": value}, "isError": mode == "tool-error"}
            if mode == "output-mismatch":
                result["structuredContent"]["value"] = 123
            if mode == "bad-iserror":
                result["isError"] = "wrong"
            if mode == "bad-structured":
                result["structuredContent"] = []
            if mode == "no-iserror":
                del result["isError"]
        elif method == "resources/read":
            result = {"contents": [{"uri": request["params"]["uri"], "text": "fixture value"}]}
            if mode == "bad-resource":
                result["contents"][0]["uri"] = "wrong://source"
        elif method == "ping":
            result = {}
        else:
            raise AssertionError(method)
    emit({"jsonrpc": "2.0", "id": ident + 1 if mode == "wrong-id" else ident, "result": result})
