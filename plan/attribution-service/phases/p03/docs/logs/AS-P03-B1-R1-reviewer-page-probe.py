"""Review-only runtime probe; owns and removes its DB, role and processes."""
import base64
import datetime
import http.server
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[6]
sys.path.insert(0, str(ROOT / "services/attribution/scripts"))
from fixture import database, sql

USER = str(uuid.uuid4())
GRANT = "g" * 43

class Authority(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass
    def reply(self, value):
        data = json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)
    def do_GET(self):
        if self.path.startswith("/api/attribution/v1/companies"):
            entries = []
            for _ in range(100):
                company = str(uuid.uuid4())
                entries.append({"company_id": company, "name": "😀" * 120,
                    "permissions": ["read", "links:manage", "integrations:manage"],
                    "lifecycle": {"company_id": company, "version": 1, "state": "active",
                                  "changed_at": "2026-11-16T00:00:00Z"}})
            payload = {"items": entries, "next_cursor": None}
            print(json.dumps({"probe": "valid_company_page_wire_size", "items": 100,
                "name_characters": 120, "utf8_body_bytes": len(json.dumps(payload, ensure_ascii=False, separators=(",", ":")).encode())}))
            self.reply(payload)
            return
        self.reply({"user_id": USER, "display_name": "Probe", "expires_at":
                    (datetime.datetime.now(datetime.UTC) + datetime.timedelta(hours=1)).isoformat()})
    def do_POST(self):
        self.rfile.read(int(self.headers.get("Content-Length", 0)))
        if self.path.endswith("/token"):
            self.reply({"access_token": GRANT, "token_type": "Bearer", "expires_in": 3600})
        else:
            self.send_response(204)
            self.end_headers()

class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *_args):
        return None

def request(origin, path, headers=None, method="GET"):
    try:
        response = urllib.request.build_opener(NoRedirect).open(
            urllib.request.Request(origin + path, headers=headers or {}, method=method), timeout=3)
    except urllib.error.HTTPError as response_error:
        response = response_error
    with response:
        return response.status, response.headers, response.read().decode()

authority = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Authority)
thread = threading.Thread(target=authority.serve_forever)
thread.start()
try:
    with database() as url:
        with socket.socket() as reserved:
            reserved.bind(("127.0.0.1", 0))
            port = reserved.getsockname()[1]
        origin = f"http://127.0.0.1:{port}"
        encode = lambda byte: base64.urlsafe_b64encode(bytes([byte]) * 32).decode().rstrip("=")
        env = dict(os.environ, ATTRIBUTION_DATABASE_URL=url, ATTRIBUTION_BIND_ADDR=f"127.0.0.1:{port}",
                   ATTRIBUTION_PUBLIC_ORIGIN=origin,
                   ATTRIBUTION_OPENBOT_ORIGIN=f"http://127.0.0.1:{authority.server_port}",
                   ATTRIBUTION_CLIENT_SECRET=encode(71), ATTRIBUTION_LIFECYCLE_SECRET=encode(72),
                   ATTRIBUTION_SESSION_KEYS=json.dumps({"active": 1, "keys": [{"version": 1, "key": encode(73)}]}))
        process = subprocess.Popen(["/tmp/as-attribution-target/debug/attribution-service", "serve"],
                                   cwd=ROOT / "services/attribution", env=env,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            deadline = time.monotonic() + 15
            while True:
                try:
                    if request(origin, "/readyz")[0] == 204:
                        break
                except urllib.error.URLError:
                    pass
                assert process.poll() is None and time.monotonic() < deadline
                time.sleep(0.05)
            def login():
                status, headers, _ = request(origin, "/auth/login")
                assert status == 302
                state = urllib.parse.parse_qs(urllib.parse.urlsplit(headers["Location"]).query)["state"][0]
                correlation = headers["Set-Cookie"].split(";", 1)[0]
                status, headers, _ = request(origin, f"/auth/callback?state={state}&code={'c' * 43}", {"Cookie": correlation})
                assert status == 302
                session = next(v for v in headers.get_all("Set-Cookie") if v.startswith("__Host-attribution="))
                cookie = session.split(";", 1)[0]
                status, _, body = request(origin, "/v1/session", {"Cookie": cookie})
                assert status == 200
                return cookie, json.loads(body)["csrf_token"]
            cookie, csrf = login()
            page = request(origin, "/v1/companies?limit=100", {"Cookie": cookie})
            print(json.dumps({"probe": "valid_company_page_response", "service_status": page[0]}))
            assert page[0] == 503
            headers = {"Cookie": cookie, "Origin": origin, "X-CSRF-Token": csrf}
            first = request(origin, "/auth/logout", headers, "POST")
            retry = request(origin, "/auth/logout", headers, "POST")
            after = request(origin, "/", {"Cookie": cookie})
            print(json.dumps({"probe": "lost_logout_response_retry", "first_status": first[0],
                              "retry_status": retry[0], "retry_clears_cookie": bool(retry[1].get("Set-Cookie")),
                              "shell_status": after[0], "shell_sign_in_link": "/auth/login" in after[2]}))
            cookie, csrf = login()
            sql(url, "UPDATE attribution_sessions SET idle_expires_at = clock_timestamp() - interval '1 second';")
            expired = request(origin, "/", {"Cookie": cookie})
            public = request(origin, "/")
            print(json.dumps({"probe": "idle_expiry_browser_recovery", "with_cookie_status": expired[0],
                              "with_cookie_sign_in_link": "/auth/login" in expired[2],
                              "with_cookie_cleared": bool(expired[1].get("Set-Cookie")),
                              "without_cookie_status": public[0], "without_cookie_sign_in_link": "/auth/login" in public[2]}))
            assert first[0] == 204 and retry[0] == 401 and after[0] == 401
            assert expired[0] == 401 and "/auth/login" not in expired[2] and public[0] == 200
        finally:
            process.terminate()
            try:
                process.wait(timeout=8)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=2)
finally:
    authority.shutdown()
    authority.server_close()
    thread.join()
