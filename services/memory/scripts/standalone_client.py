#!/usr/bin/env python3
"""A generic memory-service client journey (plan 06B.1), standard library only.

It knows only the service's published contract (openapi.json) and nothing about OpenBot. Against
a running service it provisions a tenant, namespaces and client keys, saves a note, finds it by
search (HTTP and MCP), fetches it, reads its timeline, packs and validates context, runs one
generation through the configured provider, corrects and deletes the note, purges the namespace
and deletes the tenant. Each step prints `memory_standalone_<step> ok`; the first failure exits
non-zero with what was expected. No token is ever printed.

Environment:
  MEMORY_URL                 base URL of the service, e.g. http://127.0.0.1:18095 (required)
  MEMORY_OPERATOR_TOKEN      an operator admin token (required)
  MEMORY_JOURNEY_CAP         the tenant's daily generation token cap (default 1000000)
  MEMORY_JOURNEY_GENERATED   "saved" to require that the generation saved memories (the stand-in
                             always does); "any" accepts `nothing_to_save` too (a real model)
"""
import datetime
import json
import os
import sys
import time
import urllib.error
import urllib.request
import uuid

BASE = os.environ["MEMORY_URL"].rstrip("/")
OPERATOR = os.environ["MEMORY_OPERATOR_TOKEN"]
CAP = int(os.environ.get("MEMORY_JOURNEY_CAP", "1000000"))
GENERATED = os.environ.get("MEMORY_JOURNEY_GENERATED", "saved")
RUN = uuid.uuid4().hex[:12]
ALL = ["save", "search", "timeline", "fetch", "correct", "delete", "context", "sources",
       "sessions", "generate", "generation_status", "purge"]


class Failed(Exception):
    pass


def call(method, path, token, body=None, headers=None):
    """(status, decoded JSON or None, response headers)."""
    data = None if body is None else json.dumps(body).encode()
    request = urllib.request.Request(BASE + path, data=data, method=method)
    request.add_header("authorization", f"Bearer {token}")
    if data is not None:
        request.add_header("content-type", "application/json")
    for key, value in (headers or {}).items():
        request.add_header(key, value)
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            raw = response.read()
            return response.status, (json.loads(raw) if raw else None), response.headers
    except urllib.error.HTTPError as error:
        raw = error.read()
        try:
            decoded = json.loads(raw) if raw else None
        except ValueError:
            decoded = None
        return error.code, decoded, error.headers


def ok(method, path, token, body=None):
    status, answer, _ = call(method, path, token, body)
    if status != 200:
        code = (answer or {}).get("code") if isinstance(answer, dict) else None
        raise Failed(f"{method} {path} answered {status} {code}")
    return answer


def expect(condition, what):
    if not condition:
        raise Failed(what)


def step(name):
    print(f"memory_standalone_{name} ok", flush=True)


def later(hours=1):
    moment = datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(hours=hours)
    return moment.isoformat().replace("+00:00", "Z")


def context(level):
    return {"producing_ceiling": level, "consumed": {"kind": "exact", "level": level},
            "sources": [], "external_sources": [], "expected_access_level": None}


def journey():
    tenant = f"journey-{RUN}"
    # Provisioning: a tenant, two namespaces, a generation cap, and two client keys.
    ok("POST", "/admin/v1/tenants/create", OPERATOR, {"external_id": tenant})
    for namespace in ("notes", "other"):
        ok("POST", "/admin/v1/namespaces/create", OPERATOR,
           {"tenant": tenant, "namespace": namespace})
    ok("POST", "/admin/v1/tenants/settings", OPERATOR,
       {"tenant": tenant, "generation_daily_token_cap": CAP})
    grant = {"namespace": "notes", "read_max": 9, "write_max": 9, "operations": ALL}
    client = ok("POST", "/admin/v1/credentials/issue", OPERATOR,
                {"tenant": tenant, "label": "journey client", "expires_at": None,
                 "grants": [grant]})["token"]
    low = ok("POST", "/admin/v1/credentials/issue", OPERATOR,
             {"tenant": tenant, "label": "journey low reader", "expires_at": None,
              "grants": [{"namespace": "notes", "read_max": 2, "write_max": 2,
                          "operations": ["search", "fetch"]}]})["token"]
    me = ok("GET", "/v1/credential", client)
    expect(me.get("tenant") == tenant, "the client key describes its own tenant")
    step("provision")

    # Save one note at level 3, then replay it.
    note = {"kind": "fact", "title": "Deploy window",
            "body": "The weekly deploy window is Tuesday at 14:00 UTC.",
            "occurred_at": later(-1), "expires_at": None}
    save = {"idempotency_key": f"journey.save.{RUN}", "expires_at": later(),
            "namespace": "notes", "session_id": None, "context": context(3), "items": [note]}
    saved = ok("POST", "/v1/observations/save", client, save)
    observation = saved["observations"][0]["observation_id"]
    replay = ok("POST", "/v1/observations/save", client, save)
    expect(replay.get("replayed") is True, "a retried save replays its first answer")
    expect(replay["observations"][0]["observation_id"] == observation, "the replay has the same id")
    step("save")

    # Search until the note is indexed. The query shares no word with the note, so only the
    # semantic leg (the local embedding model) can find it.
    query = {"namespaces": ["notes"], "read_ceiling": 9,
             "query": "which day do releases ship", "limit": 5}
    deadline = time.monotonic() + 120
    while True:
        found = ok("POST", "/v1/observations/search", client, query)
        ids = [item["observation_id"] for item in found["items"]]
        if observation in ids:
            break
        expect(time.monotonic() < deadline, "the note becomes searchable within 120 s")
        time.sleep(0.5)
    step("search")

    # A reader below the note's level sees nothing of it.
    hidden = ok("POST", "/v1/observations/search", low, dict(query, read_ceiling=2))
    expect(observation not in [item["observation_id"] for item in hidden["items"]],
           "a level-2 reader does not find a level-3 note")
    fetched_low = ok("POST", "/v1/observations/fetch", low,
                     {"namespaces": ["notes"], "read_ceiling": 2, "ids": [observation]})
    expect(fetched_low["items"] == [], "a level-2 reader cannot fetch it")
    step("classification")

    fetched = ok("POST", "/v1/observations/fetch", client,
                 {"namespaces": ["notes"], "read_ceiling": 9, "ids": [observation]})
    expect(len(fetched["items"]) == 1 and fetched["items"][0]["body"] == note["body"],
           "fetch returns the whole note")
    step("fetch")

    timeline = ok("POST", "/v1/observations/timeline", client,
                  {"namespaces": ["notes"], "read_ceiling": 9, "anchor": observation})
    expect(observation in [item["observation_id"] for item in timeline["items"]],
           "the timeline includes its anchor")
    step("timeline")

    pack = ok("POST", "/v1/context/pack", client,
              {"namespaces": ["notes"], "read_ceiling": 9, "query": "deploy window"})
    packed = [item for item in pack["items"] if item["observation_id"] == observation]
    expect(packed, "the context pack includes the note")
    items = [{"observation_id": item["observation_id"], "revision": item["revision"],
              "sources_sha256": item["sources_sha256"]} for item in packed]
    valid = ok("POST", "/v1/context/validate", client,
               {"namespaces": ["notes"], "read_ceiling": 9, "items": items})
    expect([item["status"] for item in valid["items"]] == ["valid"],
           f"the packed item validates: {valid['items']}")
    step("context")

    # MCP: the same search as a tool.
    mcp_headers = {"accept": "application/json, text/event-stream"}
    status, listed, _ = call("POST", "/mcp", client,
                             {"jsonrpc": "2.0", "id": 1, "method": "tools/list"}, mcp_headers)
    expect(status == 200, f"MCP tools/list answered {status}")
    names = [tool["name"] for tool in listed["result"]["tools"]]
    expect("memory_search" in names, "MCP lists memory_search")
    status, called, _ = call("POST", "/mcp", client,
                             {"jsonrpc": "2.0", "id": 2, "method": "tools/call",
                              "params": {"name": "memory_search", "arguments": query}},
                             mcp_headers)
    expect(status == 200 and not called["result"].get("isError"), "the MCP search succeeds")
    expect(observation in [item["observation_id"]
                           for item in called["result"]["structuredContent"]["items"]],
           "the MCP search finds the note")
    step("mcp")

    # One generation through the configured provider, polled to its end.
    events = [
        {"event_id": "e1", "source_ref": f"journey:{RUN}:1", "revision": "1",
         "occurred_at": later(-1), "access_level": 2, "role": "user",
         "text": "We agreed the release checklist must be signed off by two people.",
         "exclude": False},
        {"event_id": "e2", "source_ref": f"journey:{RUN}:2", "revision": "1",
         "occurred_at": later(-1), "access_level": 2, "role": "assistant",
         "text": "Noted: two sign-offs are required before every release.", "exclude": False},
    ]
    generation = ok("POST", "/v1/generations/request", client,
                    {"idempotency_key": f"journey.gen.{RUN}", "expires_at": later(),
                     "namespace": "notes", "session_id": None, "producing_ceiling": 6,
                     "consumed": {"kind": "unknown"}, "description": "standalone journey",
                     "events": events, "allowance_tokens": 40000, "max_output_tokens": 1024,
                     "summary": False})
    operation = generation["operation_id"]
    deadline = time.monotonic() + 180
    while True:
        status = ok("POST", "/v1/generations/status", client,
                    {"namespace": "notes", "operation_id": operation, "read_ceiling": 9})
        if status["state"] not in ("accepted", "claimed", "dispatched"):
            break
        expect(time.monotonic() < deadline, "the generation finishes within 180 s")
        time.sleep(1)
    expect(status["state"] == "completed", f"the generation completed: {status['state']} "
                                           f"{status.get('outcome')}")
    if GENERATED == "saved":
        expect(status.get("outcome") == "saved" and status["observations"],
               f"the generation saved memories: {status.get('outcome')}")
    else:
        expect(status.get("outcome") in ("saved", "nothing_to_save"),
               f"the generation ended saved or nothing_to_save: {status.get('outcome')}")
    print(f"memory_standalone_generation outcome={status.get('outcome')} "
          f"memories={len(status['observations'])}", flush=True)
    step("generation")

    # Correct, then delete the note.
    corrected = ok("POST", "/v1/observations/correct", client,
                   {"idempotency_key": f"journey.correct.{RUN}", "expires_at": later(),
                    "namespace": "notes", "read_ceiling": 9, "observation_id": observation,
                    "expected_revision": 1, "title": "Deploy window",
                    "body": "The weekly deploy window moved to Wednesday at 15:00 UTC.",
                    "context": context(3)})
    expect(corrected.get("revision") == 2, f"the correction is revision 2: {corrected}")
    after = ok("POST", "/v1/observations/fetch", client,
               {"namespaces": ["notes"], "read_ceiling": 9, "ids": [observation]})
    expect("Wednesday" in after["items"][0]["body"], "fetch returns the corrected text")
    step("correct")
    ok("POST", "/v1/observations/delete", client,
       {"idempotency_key": f"journey.delete.{RUN}", "expires_at": later(),
        "namespace": "notes", "read_ceiling": 9, "observation_id": observation})
    gone = ok("POST", "/v1/observations/fetch", client,
              {"namespaces": ["notes"], "read_ceiling": 9, "ids": [observation]})
    expect(gone["items"] == [], "a deleted note is gone")
    step("delete")

    purged = ok("POST", "/v1/namespaces/purge", client,
                {"idempotency_key": f"journey.purge.{RUN}", "expires_at": later(),
                 "namespace": "notes"})
    expect(purged.get("purged") is True, "the namespace is purged")
    status, _, _ = call("POST", "/v1/observations/search", client, query)
    expect(status != 200, "a purged namespace is no longer searchable")
    step("purge")

    ok("POST", "/admin/v1/tenants/delete", OPERATOR, {"tenant": tenant})
    status, _, _ = call("GET", "/v1/credential", client)
    expect(status == 401, f"a deleted tenant's key answers 401, not {status}")
    step("tenant_delete")


if __name__ == "__main__":
    try:
        journey()
    except Failed as failure:
        print(f"memory_standalone FAILED: {failure}", file=sys.stderr)
        sys.exit(1)
    print("memory_standalone journey passed")
