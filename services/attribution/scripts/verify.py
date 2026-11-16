#!/usr/bin/env python3
"""Service-only gates. Run from this directory or an extracted service copy."""
import os
from pathlib import Path
import subprocess
import sys

from fixture import database

ROOT = Path(__file__).resolve().parents[1]


def run(*args, env=None, capture=False):
    print("+ " + " ".join(args), flush=True)
    result = subprocess.run(args, cwd=ROOT, env=env, check=True, text=True,
                            stdout=subprocess.PIPE if capture else None)
    if capture:
        print(result.stdout, end="", flush=True)
        return result.stdout


def main():
    mode = sys.argv[1] if len(sys.argv) == 2 else "full"
    if mode not in ("full", "foundation", "prepare") or len(sys.argv) > 2:
        raise SystemExit("usage: verify.py [full|foundation|prepare]")
    if mode == "full" and sys.platform == "linux":
        run(sys.executable, "scripts/test-network-isolation-scope.py")
    with database() as url:
        online = dict(os.environ, DATABASE_URL=url, SQLX_OFFLINE="false", RUST_MIN_STACK="2097152")
        run("cargo", "sqlx", "migrate", "run", env=online)
        if mode == "prepare":
            run("cargo", "sqlx", "prepare", "--", "--locked", "--offline", "--all-targets", env=online)
            return
        run("cargo", "sqlx", "prepare", "--check", "--", "--locked", "--offline", "--all-targets", env=online)
    offline = dict(os.environ, SQLX_OFFLINE="true", RUST_MIN_STACK="2097152")
    offline.pop("DATABASE_URL", None)
    run("cargo", "fmt", "--all", "--", "--check", env=offline)
    run("cargo", "check", "--locked", "--offline", "--all-targets", env=offline)
    if mode == "full":
        run("cargo", "clippy", "--locked", "--offline", "--all-targets", "--", "-D", "warnings", env=offline)
    run(sys.executable, "scripts/boundary.py", env=offline)
    run(sys.executable, "scripts/test_boundary.py", env=offline)
    prefixes = ["attribution_startup_", "attribution_storage_boundary_"] if mode == "foundation" else [""]
    for prefix in prefixes:
        args = ["cargo", "test", "--locked", "--offline", "--all-targets"] + ([prefix] if prefix else [])
        inventory = run(*args, "--", "--list", env=offline, capture=True)
        count = sum(line.endswith(": test") for line in inventory.splitlines())
        if not count:
            raise RuntimeError(f"zero tests discovered for {prefix!r}")
        print(f"Selected {count} tests for {prefix or 'all targets'}", flush=True)
        if mode == "full":
            run(*args, "--no-run", env=offline)
            run("bash", "scripts/test-network-isolation.sh", *args, env=offline)
        else:
            run(*args, env=offline)

    run("bash", "scripts/check-contracts.sh", env=offline)
    if mode == "full":
        for script in ["assets/app.js", "assets/setup.js", "assets/reports.js", "assets/sdk-1.0.0.js", "browser/setup.mjs", "browser/website.mjs", "browser/reports.mjs", "browser/topology.mjs"]:
            run("node", "--check", script, env=offline)
        for prefix in ["attribution_setup_browser_", "attribution_checkout_handoff_browser_", "attribution_dashboard_browser_"]:
            args = ["cargo", "test", "--locked", "--offline", "--lib", prefix]
            inventory = run(*args, "--", "--ignored", "--list", env=offline, capture=True)
            if not any(line.endswith(": test") for line in inventory.splitlines()):
                raise RuntimeError(f"zero browser tests discovered for {prefix!r}")
            run("bash", "scripts/test-network-isolation.sh", *args, "--", "--ignored", "--nocapture", env=offline)


if __name__ == "__main__":
    main()
