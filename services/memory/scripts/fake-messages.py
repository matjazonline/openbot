#!/usr/bin/env python3
"""A loopback stand-in for the Anthropic Messages API, for local and CI journeys only.

Used by scripts/standalone-journey.sh here and by the repository's scripts/memory-integration.sh.
The memory service's generation worker posts one window of activity to /v1/messages and expects
a structured-output answer. This answers every request with one saved `fact` observation that
cites every event id of the window and carries the marker LIVE-GENERATED, so a test can find the
generated memory. Requests are appended to the log file named by the second argument.
Usage: fake-messages.py <port> <request-log>
"""
import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


def event_ids(value):
    """Every `event_id` anywhere in the request's user turn, which carries the window as JSON."""
    found = []
    if isinstance(value, dict):
        for key, item in value.items():
            if key == "event_id" and isinstance(item, str):
                found.append(item)
            else:
                found.extend(event_ids(item))
    elif isinstance(value, list):
        for item in value:
            found.extend(event_ids(item))
    elif isinstance(value, str):
        try:
            found.extend(event_ids(json.loads(value)))
        except ValueError:
            pass
    return found


class Messages(BaseHTTPRequestHandler):
    def do_POST(self):  # noqa: N802 (http.server naming)
        length = int(self.headers.get("content-length", "0"))
        body = json.loads(self.rfile.read(length) or b"{}")
        with open(sys.argv[2], "a", encoding="utf-8") as log:
            log.write(json.dumps({"path": self.path, "body": body}) + "\n")
        if self.path != "/v1/messages":
            self.send_response(404)
            self.end_headers()
            return
        ids = list(dict.fromkeys(event_ids(body.get("messages", []))))
        answer = {
            "outcome": "save",
            "observations": [
                {
                    "kind": "fact",
                    "title": "LIVE-GENERATED memory",
                    "body": "LIVE-GENERATED: the captured exchange was summarized.",
                    "event_ids": ids,
                }
            ],
            "summary": None,
        }
        message = {
            "id": "msg_live_fake",
            "type": "message",
            "role": "assistant",
            "model": body.get("model", "fake"),
            "content": [{"type": "text", "text": json.dumps(answer)}],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 50,
                "output_tokens": 40,
                "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 0,
            },
        }
        encoded = json.dumps(message).encode()
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("request-id", "req_live_fake")
        self.send_header("content-length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)

    def log_message(self, *_args):
        pass


if __name__ == "__main__":
    ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), Messages).serve_forever()
