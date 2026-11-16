"""Disposable PostgreSQL resources owned by this invocation, never existing data."""
import contextlib
import os
import subprocess
import uuid
from urllib.parse import quote, urlsplit, urlunsplit


def sql(url, statement):
    env = dict(os.environ, PGCONNECT_TIMEOUT="5")
    result = subprocess.run(
        ["psql", "--dbname", url, "-X", "--no-psqlrc", "--set", "ON_ERROR_STOP=1", "--quiet"],
        input=statement, text=True, capture_output=True, env=env, timeout=30,
    )
    if result.returncode:
        # Never propagate SQL/passwords or connection URLs from client errors.
        raise RuntimeError("owned fixture SQL failed; check maintenance access")


@contextlib.contextmanager
def database():
    admin = os.environ.get("ATTRIBUTION_TEST_ADMIN_URL")
    if not admin:
        raise RuntimeError("ATTRIBUTION_TEST_ADMIN_URL is required; database checks cannot be skipped")
    parsed = urlsplit(admin)
    if parsed.scheme not in ("postgres", "postgresql") or parsed.path != "/postgres" or not parsed.hostname:
        raise RuntimeError("test admin URL must name the postgres maintenance database over TCP")
    name = "as_verify_" + uuid.uuid4().hex
    secret = uuid.uuid4().hex
    host = parsed.hostname
    if ":" in host:
        host = "[" + host + "]"
    port = f":{parsed.port}" if parsed.port else ""
    own_url = urlunsplit((parsed.scheme, f"{name}:{quote(secret)}@{host}{port}", "/" + name, parsed.query, ""))
    role_created = database_created = False
    try:
        sql(admin, f"CREATE ROLE {name} LOGIN PASSWORD '{secret}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;")
        role_created = True
        sql(admin, f"CREATE DATABASE {name} OWNER {name};")
        database_created = True
        sql(admin, f"REVOKE ALL ON DATABASE {name} FROM PUBLIC;")
        yield own_url
    finally:
        if database_created:
            sql(admin, f"DROP DATABASE {name} WITH (FORCE);")
        if role_created:
            sql(admin, f"DROP ROLE {name};")
