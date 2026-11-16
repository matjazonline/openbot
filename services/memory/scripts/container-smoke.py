#!/usr/bin/env python3
"""The same non-root, offline model and failed-startup image checks used in CI."""
import argparse
import json
import subprocess
import uuid


def docker(*args, success=True):
    result = subprocess.run(["docker", *args], capture_output=True, text=True, timeout=180)
    if success and result.returncode:
        raise RuntimeError("memory image check failed: " + result.stderr[-2000:])
    return result


def probe(image, command, options=(), success=True):
    name = "memory_image_" + uuid.uuid4().hex
    try:
        return docker("run", "--name", name, "--rm", "--network", "none", "--read-only",
                      *options, image, *command, success=success)
    finally:
        docker("rm", "--force", name, success=False)


def check(image):
    user = docker("image", "inspect", image, "--format", "{{.Config.User}}").stdout.strip()
    if user != "memory:memory":
        raise RuntimeError("memory image must run as memory:memory")
    probe(image, ["version"])
    model = json.loads(probe(image, ["model-check"]).stdout)
    if not (model["dimensions"] == 384 and model["sessions"] == 2
            and model["oversized_input_refused"]
            and model["peak_rss_bytes"] <= 2560 * 1024 * 1024):
        raise RuntimeError("memory model shape, input bounds or RSS budget failed")
    result = probe(image, ["serve"], options=(
        "--env", "MEMORY_DATABASE_URL=postgres://x@127.0.0.1:1/x",
        "--env", "MEMORY_DB_CONNECT_TIMEOUT_MS=1000",
    ), success=False)
    if result.returncode == 0 or "cannot connect to the memory database" not in result.stdout + result.stderr:
        raise RuntimeError("memory image did not refuse startup without its database")
    print("memory image passed: non-root, offline/read-only model, RSS budget, database refusal")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", required=True)
    args = parser.parse_args()
    check(args.image)


if __name__ == "__main__":
    main()
