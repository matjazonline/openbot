#!/usr/bin/env python3
"""Real non-root image startup, embedded assets, DB readiness and shutdown."""
import json
import base64
import os
from pathlib import Path
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import uuid


def docker(*args, **kwargs):
    result = subprocess.run(["docker", *args], check=False, text=True, capture_output=True, **kwargs)
    if result.returncode:
        raise RuntimeError("container command failed: " + args[0])
    return result.stdout.strip()


def status(url):
    try:
        with urllib.request.urlopen(url, timeout=4) as response:
            return response.status, response.read()
    except urllib.error.HTTPError as error:
        return error.code, error.read()


def main():
    root = Path(__file__).resolve().parents[1]
    image = os.environ.get("ATTRIBUTION_TEST_IMAGE", "attribution-service-ci")
    if docker("image", "inspect", image, "--format", "{{.Config.User}}") != "attribution:attribution":
        raise RuntimeError("image must run as attribution:attribution")
    assert docker("run", "--rm", "--read-only", "--network", "none", image, "version").startswith("attribution-service ")
    name = "as_image_" + uuid.uuid4().hex
    db, app = name + "_db", name + "_app"
    password = uuid.uuid4().hex
    with tempfile.TemporaryDirectory(prefix="as_image_") as temporary:
        envfile = Path(temporary) / "database.env"
        envfile.write_text(f"POSTGRES_PASSWORD={password}\nPOSTGRES_DB=postgres\n")
        envfile.chmod(0o600)
        docker("network", "create", name)
        try:
            docker("run", "--detach", "--name", db, "--network", name, "--env-file", str(envfile), "postgres:18.3-bookworm")
            for _ in range(60):
                # The official image first starts a temporary Unix-socket-only
                # server during initialization. Wait for its final TCP server.
                ready = subprocess.run(["docker", "exec", db, "pg_isready", "-h", "127.0.0.1", "-U", "postgres"], capture_output=True)
                if ready.returncode == 0:
                    break
                time.sleep(1)
            else:
                raise RuntimeError("image test PostgreSQL never became ready")
            docker("exec", "-i", db, "psql", "-U", "postgres", "-v", "ON_ERROR_STOP=1", input=f"CREATE ROLE attribution LOGIN PASSWORD '{password}';\nCREATE DATABASE attribution OWNER attribution;\nREVOKE ALL ON DATABASE attribution FROM PUBLIC;\n")
            client_secret = base64.urlsafe_b64encode(os.urandom(32)).decode().rstrip("=")
            lifecycle_secret = base64.urlsafe_b64encode(os.urandom(32)).decode().rstrip("=")
            session_key = base64.urlsafe_b64encode(os.urandom(32)).decode().rstrip("=")
            keys = json.dumps({"active": 1, "keys": [{"version": 1, "key": session_key}]})
            envfile.write_text(f"ATTRIBUTION_DATABASE_URL=postgres://attribution:{password}@{db}/attribution\nATTRIBUTION_PUBLIC_ORIGIN=http://localhost:8091\nATTRIBUTION_EVENT_MAX_AGE_DAYS=7\nATTRIBUTION_REFUND_WINDOW_DAYS=365\nATTRIBUTION_CLICK_RETENTION_DAYS=38\nATTRIBUTION_OBSERVATION_RETENTION_DAYS=30\nATTRIBUTION_FINANCIAL_RETENTION_DAYS=400\nATTRIBUTION_EVENT_RETENTION_DAYS=8\nATTRIBUTION_AUDIT_RETENTION_DAYS=90\nATTRIBUTION_OPENBOT_ORIGIN=http://127.0.0.1:8092\nATTRIBUTION_CLIENT_SECRET={client_secret}\nATTRIBUTION_LIFECYCLE_SECRET={lifecycle_secret}\nATTRIBUTION_SESSION_KEYS={keys}\n")
            docker("run", "--detach", "--name", app, "--network", name, "--read-only", "--cap-drop", "ALL", "--security-opt", "no-new-privileges", "--env-file", str(envfile), "--publish", "127.0.0.1::8091", image, "serve")
            port = docker("port", app, "8091/tcp").rsplit(":", 1)[1]
            origin = "http://127.0.0.1:" + port
            for _ in range(60):
                try:
                    if status(origin + "/readyz")[0] == 204:
                        break
                except (OSError, urllib.error.URLError):
                    pass
                time.sleep(1)
            else:
                raise RuntimeError("non-root image never became ready")
            assert status(origin + "/healthz")[0] == 204
            code, css = status(origin + "/assets/app.css")
            assert code == 200 and b"focus-visible" in css
            docker("stop", "--time", "5", db)
            assert status(origin + "/readyz")[0] == 503
            assert status(origin + "/healthz")[0] == 204
            docker("stop", "--time", "10", app)
            state = json.loads(docker("inspect", app))[0]["State"]
            assert state["ExitCode"] == 0 and not state["OOMKilled"]
            logs = docker("logs", app)
            assert all(secret not in logs for secret in [password, client_secret, lifecycle_secret, session_key]) and "postgres://" not in logs
            print("container smoke passed: non-root/read-only, assets, migrations, DB loss, graceful exit, redaction")
        finally:
            for container in (app, db):
                subprocess.run(["docker", "rm", "--force", "--volumes", container], capture_output=True)
            subprocess.run(["docker", "network", "rm", name], capture_output=True)


if __name__ == "__main__":
    main()
