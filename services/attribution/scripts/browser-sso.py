#!/usr/bin/env python3
"""Real browser + standalone attribution process against a supplied Openbot fixture."""
import base64
import json
import os
from pathlib import Path
import subprocess
import time
import urllib.error
import urllib.request
from urllib.parse import urlsplit, urlunsplit
from fixture import database

ROOT = Path(__file__).resolve().parents[1]


def main():
    required = ["ATTRIBUTION_FIXTURE_OPENBOT_ORIGIN", "ATTRIBUTION_PUBLIC_ORIGIN",
                "ATTRIBUTION_BIND_ADDR", "ATTRIBUTION_FIXTURE_USERNAME",
                "ATTRIBUTION_FIXTURE_PASSWORD", "ATTRIBUTION_FIXTURE_USER_ID",
                "ATTRIBUTION_CLIENT_SECRET", "ATTRIBUTION_LIFECYCLE_SECRET",
                "ATTRIBUTION_TEST_BINARY", "ATTRIBUTION_FIXTURE_OPENBOT_DATABASE"]
    if any(not os.environ.get(key) for key in required):
        raise RuntimeError("browser fixture requires explicit account, origins and binary")
    with database() as url:
        own = urlsplit(url)
        foreign = urlunsplit((own.scheme, own.netloc, "/" + os.environ["ATTRIBUTION_FIXTURE_OPENBOT_DATABASE"], own.query, ""))
        denied = subprocess.run(["psql", "--dbname", foreign, "-X", "-At", "-c", "SELECT 1"],
                                capture_output=True, timeout=5)
        if denied.returncode == 0:
            raise RuntimeError("attribution role unexpectedly connected to Openbot database")
        key = base64.urlsafe_b64encode(os.urandom(32)).decode().rstrip("=")
        env = dict(os.environ, ATTRIBUTION_DATABASE_URL=url,
                   ATTRIBUTION_EVENT_MAX_AGE_DAYS="7", ATTRIBUTION_REFUND_WINDOW_DAYS="365",
                   ATTRIBUTION_CLICK_RETENTION_DAYS="38", ATTRIBUTION_OBSERVATION_RETENTION_DAYS="30",
                   ATTRIBUTION_FINANCIAL_RETENTION_DAYS="400", ATTRIBUTION_EVENT_RETENTION_DAYS="8", ATTRIBUTION_AUDIT_RETENTION_DAYS="90",
                   ATTRIBUTION_OPENBOT_ORIGIN=os.environ[required[0]],
                   ATTRIBUTION_SESSION_KEYS=json.dumps({"active": 1, "keys": [{"version": 1, "key": key}]}))
        process = subprocess.Popen([os.environ["ATTRIBUTION_TEST_BINARY"], "serve"],
                                   cwd=ROOT, env=env, stdout=subprocess.DEVNULL,
                                   stderr=subprocess.DEVNULL)
        try:
            deadline = time.monotonic() + 20
            while True:
                if process.poll() is not None:
                    raise RuntimeError("attribution process exited before readiness")
                try:
                    with urllib.request.urlopen(env["ATTRIBUTION_PUBLIC_ORIGIN"] + "/readyz", timeout=1) as response:
                        if response.status == 204:
                            break
                except (urllib.error.URLError, TimeoutError):
                    pass
                if time.monotonic() >= deadline:
                    raise RuntimeError("attribution readiness deadline exceeded")
                time.sleep(0.05)
            child = subprocess.run(["node", str(ROOT / "browser/sso.mjs")], env=env,
                                   timeout=100, check=False)
            if child.returncode:
                raise RuntimeError("browser checks failed")
        finally:
            process.terminate()
            try:
                process.wait(timeout=7)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=2)


if __name__ == "__main__":
    main()
