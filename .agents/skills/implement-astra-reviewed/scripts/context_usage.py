#!/usr/bin/env python3
"""Read-only estimates of per-agent context usage from Codex JSONL logs."""

import argparse
from datetime import datetime
import json
import math
import os
from pathlib import Path
import sys


def positive_number(value, allow_zero=False):
    return (
        isinstance(value, (int, float))
        and not isinstance(value, bool)
        and math.isfinite(value)
        and (value >= 0 if allow_zero else value > 0)
    )


def reverse_lines(path):
    """Read the log tail first; tolerate an incomplete final JSONL record."""
    with path.open("rb") as stream:
        stream.seek(0, 2)
        position = stream.tell()
        remainder = b""
        while position:
            size = min(position, 65536)
            position -= size
            stream.seek(position)
            parts = (stream.read(size) + remainder).split(b"\n")
            remainder = parts[0]
            yield from reversed(parts[1:])
        if remainder:
            yield remainder


def header(path):
    try:
        with path.open("rb") as stream:
            event = json.loads(stream.readline())
        if not isinstance(event, dict) or event.get("type") != "session_meta":
            return None
        meta = event.get("payload")
        if not isinstance(meta, dict) or not isinstance(meta.get("id"), str):
            return None
        parent = meta.get("parent_thread_id")
        source = meta.get("source")
        if not parent and isinstance(source, dict):
            subagent = source.get("subagent")
            if isinstance(subagent, dict):
                spawn = subagent.get("thread_spawn")
                if isinstance(spawn, dict):
                    parent = spawn.get("parent_thread_id")
        return {
            "agent_id": meta["id"],
            "parent_id": parent if isinstance(parent, str) else None,
            "forked": bool(meta.get("forked_from_id")),
            "path": path,
        }
    except (OSError, ValueError):
        return None


def discover(codex_home):
    sessions = {}
    skipped = 0
    for directory in (codex_home / "sessions", codex_home / "archived_sessions"):
        if not directory.is_dir():
            continue
        for path in directory.rglob("*.jsonl"):
            record = header(path)
            if record is None:
                skipped += 1
                continue
            previous = sessions.get(record["agent_id"])
            if previous is None:
                sessions[record["agent_id"]] = record
            else:
                # File modification time does not establish which history is current.
                previous["ambiguous_logs"] = True
    return sessions, skipped


def measure(record, depth):
    result = {
        "agent_id": record["agent_id"],
        "parent_id": record["parent_id"],
        "depth": depth,
        "input_tokens": None,
        "available_context_tokens": None,
        "percent_used": None,
        "percent_remaining": None,
        "measured_at": None,
        "reason": "No recorded token measurement",
    }
    result["measurement_source"] = None
    result["capacity_source"] = None
    if record.get("ambiguous_logs"):
        result["reason"] = "Multiple logs for this session; authoritative log identity is unresolved"
        return result
    pending = None
    try:
        for line in reverse_lines(record["path"]):
            if not any(marker in line for marker in (
                b'"token_count"', b'"token_usage_record"', b'"task_started"',
                b'"compacted"', b'"context_compacted"',
            )):
                continue
            try:
                event = json.loads(line)
            except (ValueError, UnicodeDecodeError):
                continue
            if not isinstance(event, dict):
                continue
            payload = event.get("payload")
            kind = event.get("type")
            compacted = kind == "compacted" or (
                kind == "event_msg" and isinstance(payload, dict)
                and payload.get("type") == "context_compacted"
            )
            if pending is None and compacted:
                result["reason"] = "Compaction occurred after the last measurement"
                return result
            if not isinstance(payload, dict):
                continue
            if pending is not None:
                # Raw usage has no capacity. Only use the start of its own turn;
                # an older turn may have used a different model/window.
                if kind == "event_msg" and payload.get("type") == "task_started":
                    if pending.get("turn_id") is not None and payload.get("turn_id") == pending["turn_id"]:
                        capacity = payload.get("model_context_window")
                        if positive_number(capacity):
                            result["available_context_tokens"] = capacity
                            result["capacity_source"] = "task_started.model_context_window"
                    break
                continue
            if kind == "event_msg" and payload.get("type") == "task_started":
                result["reason"] = "Current turn has no recorded measurement; older usage is stale"
                return result
            if kind == "token_usage_record":
                # Forked histories may contain the parent's raw usage records.
                if payload.get("thread_id") != record["agent_id"]:
                    continue
                usage = payload.get("usage")
                capacity = None
                source = "token_usage_record.usage"
            elif kind == "event_msg" and payload.get("type") == "token_count":
                info = payload.get("info")
                if info is None:
                    # Rate-limit notifications carry no measurement to attribute.
                    continue
                owner = payload.get("thread_id")
                if owner is not None and owner != record["agent_id"]:
                    continue
                if record.get("forked") and owner != record["agent_id"]:
                    result["reason"] = "Forked token event lacks verified session ownership"
                    return result
                if not isinstance(info, dict):
                    result["reason"] = "Latest token event has invalid usage data"
                    return result
                usage = info.get("last_token_usage")
                capacity = info.get("model_context_window")
                source = "token_count.info.last_token_usage"
            else:
                continue
            used = usage.get("input_tokens") if isinstance(usage, dict) else None
            result["measured_at"] = event.get("timestamp")
            result["measurement_source"] = source
            try:
                timestamp = datetime.fromisoformat(result["measured_at"].replace("Z", "+00:00"))
                if timestamp.tzinfo is None:
                    raise ValueError("Timestamp must include a timezone")
            except (AttributeError, TypeError, ValueError):
                result["reason"] = "Latest measurement lacks a valid timestamp; freshness is unresolved"
                return result
            if positive_number(used, allow_zero=True):
                result["input_tokens"] = used
            if positive_number(capacity):
                result["available_context_tokens"] = capacity
                result["capacity_source"] = "token_count.info.model_context_window"
            if result["input_tokens"] is None:
                result["reason"] = "Latest measurement lacks valid input tokens"
                return result
            if kind == "token_usage_record":
                pending = payload
                continue
            break
    except OSError:
        result["reason"] = "Session log is not readable"
        return result
    if result["measurement_source"] is None:
        return result
    used, capacity = result["input_tokens"], result["available_context_tokens"]
    if capacity is None:
        result["reason"] = "Latest measurement lacks valid context capacity for its turn"
        return result
    percent = 100 * used / capacity
    result.update(
        percent_used=round(percent, 2),
        percent_remaining=round(max(0, 100 - percent), 2),
        reason=None if percent <= 100 else "Recorded input exceeds reported capacity; check runtime accounting",
    )
    return result


def report(codex_home, thread_id):
    sessions, skipped = discover(codex_home)
    if thread_id not in sessions:
        raise ValueError("Requested thread has no readable session log in this Codex directory")
    return tree_report(sessions, skipped, thread_id, measure, "Latest recorded input tokens / runtime-reported available context * 100")


def tree_report(sessions, skipped, thread_id, measure_fn, measurement):
    children = {}
    for record in sessions.values():
        children.setdefault(record["parent_id"], []).append(record["agent_id"])
    pending = [(thread_id, 0)]
    seen = set()
    rows = []
    while pending:
        agent_id, depth = pending.pop()
        if agent_id in seen:
            continue
        seen.add(agent_id)
        rows.append(measure_fn(sessions[agent_id], depth))
        pending.extend((child, depth + 1) for child in sorted(children.get(agent_id, []), reverse=True))
    return {
        "root_agent_id": thread_id,
        "measurement": measurement,
        "estimate": True,
        "skipped_session_headers": skipped,
        "agents": rows,
    }


def render_table(data):
    print("Latest recorded context estimates (each agent has a separate context window).")
    print("| Agent ID | Depth | Input tokens | Available context | Used | Remaining | Measured at |")
    print("| --- | ---: | ---: | ---: | ---: | ---: | --- |")
    for row in data["agents"]:
        used = "unknown" if row["percent_used"] is None else f'{row["percent_used"]:.2f}%'
        remaining = "unknown" if row["percent_remaining"] is None else f'{row["percent_remaining"]:.2f}%'
        tokens = "unknown" if row["input_tokens"] is None else f'{row["input_tokens"]:,}'
        capacity = "unknown" if row["available_context_tokens"] is None else f'{row["available_context_tokens"]:,}'
        print(f'| {row["agent_id"]} | {row["depth"]} | {tokens} | {capacity} | {used} | {remaining} | {row["measured_at"] or "unknown"} |')
    for row in data["agents"]:
        if row["reason"]:
            print(f'{row["agent_id"]}: {row["reason"]}.')
    for row in data["agents"]:
        if row.get("model") and row.get("capacity_source") and row["capacity_source"] != "flag":
            print(f'{row["agent_id"]}: context window {row["available_context_tokens"]:,} assumed from {row["capacity_source"]} for {row["model"]}.')
    if data["skipped_session_headers"]:
        print(f'Unrecognized or unreadable session headers: {data["skipped_session_headers"]}; discovery may be incomplete.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", choices=("codex",), default="codex")
    parser.add_argument("--thread-id", help="Verified session UUID; defaults to CODEX_THREAD_ID")
    parser.add_argument("--codex-home", type=Path, default=Path(os.environ.get("CODEX_HOME") or Path.home() / ".codex"))
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()
    try:
        thread_id = args.thread_id or os.environ.get("CODEX_THREAD_ID")
        if not thread_id:
            raise ValueError("CODEX_THREAD_ID is unavailable; pass --thread-id with a verified session UUID")
        data = report(args.codex_home.expanduser(), thread_id)
        data["runtime"] = "codex"
    except (ValueError, OSError) as exc:
        message = str(exc) if isinstance(exc, ValueError) else "Cannot read local session logs"
        if args.json:
            print(json.dumps({"error": message, "agents": []}))
        else:
            print(message, file=sys.stderr)
        return 1
    if args.json:
        print(json.dumps(data, indent=2))
    else:
        render_table(data)
    return 0


if __name__ == "__main__":
    sys.exit(main())
