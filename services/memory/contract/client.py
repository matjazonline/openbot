#!/usr/bin/env python3
"""A second, independent memory-service client (plan 03B.6).

It uses only the published contract: operation paths come from `openapi.json` by operation
id, MCP tool names and input schemas from `tools/list`, and request bodies name only fields the
published request schemas declare. Python standard library only; no service or OpenBot code.

    python3 -I contract/client.py --base-url http://127.0.0.1:8080 --token msk_cli_... \
        --openapi openapi.json --namespace notes

It saves three observations, searches (MCP), fetches the selected detail (HTTP), assembles a
context pack (MCP) and validates it (HTTP). It prints a JSON summary and exits 0, or names
the failed step and exits 1.
"""
import argparse
import datetime
import json
import sys
import urllib.error
import urllib.request
import uuid


class Failure(Exception):
    pass


def check(condition, message):
    if not condition:
        raise Failure(message)


class Client:
    def __init__(self, base_url, token, openapi):
        self.base_url = base_url.rstrip("/")
        self.token = token
        self.document = openapi
        self.paths = {}
        for path, methods in openapi["paths"].items():
            for method, operation in methods.items():
                self.paths[operation["operationId"]] = (method.upper(), path, operation)
        self.rpc_id = 0

    def _send(self, method, path, body):
        data = None if body is None else json.dumps(body).encode("utf-8")
        request = urllib.request.Request(self.base_url + path, data=data, method=method)
        request.add_header("Authorization", "Bearer " + self.token)
        request.add_header("Accept", "application/json, text/event-stream")
        if data is not None:
            request.add_header("Content-Type", "application/json")
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                return response.status, json.loads(response.read() or b"null")
        except urllib.error.HTTPError as error:
            return error.code, json.loads(error.read() or b"null")

    def _request_schema(self, operation):
        content = operation.get("requestBody", {}).get("content", {})
        reference = content.get("application/json", {}).get("schema", {}).get("$ref")
        if reference is None:
            return None
        name = reference.rsplit("/", 1)[-1]
        return self.document["components"]["schemas"][name]

    def http(self, operation_id, body=None):
        check(operation_id in self.paths, f"{operation_id} is not published")
        method, path, operation = self.paths[operation_id]
        schema = self._request_schema(operation)
        if body is not None and schema is not None:
            declared = set(schema.get("properties", {}))
            check(set(body) <= declared, f"{operation_id}: undeclared fields {set(body) - declared}")
            missing = set(schema.get("required", [])) - set(body)
            check(not missing, f"{operation_id}: missing required fields {missing}")
        status, result = self._send(method, path, body)
        check(status == 200, f"{operation_id} answered {status}: {result}")
        return result

    def mcp(self, method, params):
        self.rpc_id += 1
        status, result = self._send(
            "POST", "/mcp", {"jsonrpc": "2.0", "id": self.rpc_id, "method": method, "params": params}
        )
        check(status == 200, f"MCP {method} answered {status}: {result}")
        check("result" in result, f"MCP {method} failed: {result}")
        return result["result"]

    def tool(self, name, arguments):
        result = self.mcp("tools/call", {"name": name, "arguments": arguments})
        check(not result.get("isError"), f"{name} failed: {result}")
        return result["structuredContent"]


def run(arguments):
    with open(arguments.openapi, encoding="utf-8") as handle:
        client = Client(arguments.base_url, arguments.token, json.load(handle))
    namespace = arguments.namespace
    now = datetime.datetime.now(datetime.timezone.utc)
    deadline = (now + datetime.timedelta(hours=1)).isoformat()

    credential = client.http("credential.get")
    grant = next((g for g in credential["grants"] if g["namespace"] == namespace), None)
    check(grant is not None and grant["read_max"] is not None, "no read grant for the namespace")
    ceiling = grant["read_max"]
    level = min(grant["write_max"], ceiling)

    tools = {tool["name"]: tool for tool in client.mcp("tools/list", {})["tools"]}
    for name in ("memory_search", "memory_context_pack"):
        check(name in tools and "properties" in tools[name]["inputSchema"], f"{name} is not listed")

    notes = [
        ("Release ORBIT-42", "The ORBIT-42 release moved to Thursday after the load test."),
        ("Load test", "The load test passed at 2,000 requests per second."),
        ("Unrelated", "The office plants were watered."),
    ]
    saved = client.http(
        "observations.save",
        {
            "idempotency_key": "py-" + uuid.uuid4().hex,
            "expires_at": deadline,
            "namespace": namespace,
            "session_id": None,
            "context": {
                "producing_ceiling": level,
                "consumed": {"kind": "exact", "level": level},
                "sources": [],
                "external_sources": [],
                "expected_access_level": level,
            },
            "items": [
                {"kind": "fact", "title": title, "body": body, "occurred_at": now.isoformat(),
                 "expires_at": None}
                for title, body in notes
            ],
        },
    )
    ids = [item["observation_id"] for item in saved["observations"]]
    check(len(ids) == 3 and all(item["access_level"] == level for item in saved["observations"]),
          "the saved labels differ from the attested context")

    index = client.tool(
        "memory_search",
        {"namespaces": [namespace], "read_ceiling": ceiling, "query": "ORBIT-42", "limit": 5},
    )
    check(index["items"] and index["items"][0]["observation_id"] == ids[0],
          f"search did not rank the identifier first: {index}")
    check("body" not in index["items"][0], "the index carries a body")

    details = client.http(
        "observations.fetch",
        {"namespaces": [namespace], "read_ceiling": ceiling, "ids": [ids[0], str(uuid.uuid4())]},
    )
    check(details["items"][0]["body"] == notes[0][1], "fetch returned other text")
    check(len(details["unavailable"]) == 1, "an unknown id was not reported unavailable")

    budget = 4096
    pack = client.tool(
        "memory_context_pack",
        {"namespaces": [namespace], "read_ceiling": ceiling, "query": "ORBIT-42 release",
         "max_tokens": budget, "max_bytes": 65536, "detail": "full"},
    )
    check(pack["items"] and pack["items"][0]["observation_id"] == ids[0], f"pack order: {pack}")
    check(pack["token_count"] <= budget and pack["byte_count"] <= 65536, "the pack exceeds its budget")
    measured = sum(len(json.dumps(item, separators=(",", ":"), ensure_ascii=False).encode("utf-8"))
                   for item in pack["items"])
    check(measured == pack["byte_count"], f"byte_count {pack['byte_count']} != measured {measured}")
    check(pack["exposed_level"] <= ceiling, "the pack is above the ceiling")
    check(pack["items"][0]["content_sha256"] == details["items"][0]["content_sha256"],
          "pack and fetch digests differ")

    frozen = [{"observation_id": item["observation_id"], "revision": item["revision"],
               "sources_sha256": item["sources_sha256"]}
              for item in pack["items"]]
    checked = client.http(
        "context.validate",
        {"namespaces": [namespace], "read_ceiling": ceiling, "items": frozen},
    )
    check([item["status"] for item in checked["items"]] == ["valid"] * len(frozen),
          f"the frozen pack is not valid: {checked}")

    return {
        "saved": len(ids),
        "found": len(index["items"]),
        "fetched": len(details["items"]),
        "packed": len(pack["items"]),
        "validated": len(checked["items"]),
        "exposed_level": pack["exposed_level"],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--base-url", required=True)
    parser.add_argument("--token", required=True)
    parser.add_argument("--openapi", required=True)
    parser.add_argument("--namespace", required=True)
    try:
        summary = run(parser.parse_args())
    except Failure as failure:
        print(f"client: {failure}", file=sys.stderr)
        return 1
    print(json.dumps(summary))
    return 0


if __name__ == "__main__":
    sys.exit(main())
