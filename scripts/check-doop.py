#!/usr/bin/env python3
"""Run the required managed Doop service and private integration gates in an owned directory."""
import argparse
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
FAILURE_TAIL_BYTES = 16_384


def report_failure(command, log):
    log.flush()
    log.seek(0, os.SEEK_END)
    log.seek(max(0, log.tell() - FAILURE_TAIL_BYTES))
    tail = log.read(FAILURE_TAIL_BYTES).decode(errors="replace")
    message = "Doop gate failed: " + " ".join(command) + "\n" + tail
    message = message.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")
    print("::error::" + message, flush=True)


def run(*args, **kwargs):
    cwd = kwargs.pop("cwd", ROOT)
    if "stderr" in kwargs:
        subprocess.run(args, cwd=cwd, check=True, **kwargs)
        return
    with tempfile.TemporaryFile() as errors:
        try:
            subprocess.run(args, cwd=cwd, check=True, stderr=errors, **kwargs)
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
            report_failure(args, errors)
            raise
        finally:
            errors.seek(0)
            for chunk in iter(lambda: errors.read(64 * 1024), b""):
                sys.stderr.write(chunk.decode(errors="replace"))


def service_gate(runner, setup, output, chrome, database):
    source, manifest, identity = runner.select_source(setup, None, None, True)
    service_home = output / "service-home"
    (service_home / "tmp").mkdir(mode=0o700, parents=True)
    environment = runner.clean_environment(service_home)
    environment["PATH"] = str(setup / "tools/node_modules/.bin") + os.pathsep + environment["PATH"]
    environment["CHROME_PATH"] = chrome
    environment["DOOP_TEST_DATABASE_URL"] = database
    bun = str(setup / "tools/node_modules/.bin/bun")
    commands = [[bun, "install", "--frozen-lockfile", "--ignore-scripts"]]
    commands += [[bun, "run", name] + (["--", "--maxWorkers=2"] if name == "test" else [])
                 for name in ("format:check", "typecheck", "lint", "doc-lint", "test", "build")]
    for index, command in enumerate(commands):
        print("Doop gate:", " ".join(command), flush=True)
        log_path = output / f"service-gate-{index}.log"
        with log_path.open("wb") as log:
            try:
                run(*command, cwd=source, env=environment, timeout=1200,
                    stdout=log, stderr=subprocess.STDOUT)
            except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
                with log_path.open("rb") as failure:
                    log.flush()
                    report_failure(command, failure)
                raise
    runner.reviewed_source(source, identity["fingerprint"], manifest)


def compile_helper(output):
    messages_file = output / "libtest-build.jsonl"
    with messages_file.open("w+b") as messages:
        try:
            run("cargo", "test", "--locked", "--offline", "--lib", "--no-run",
                "--message-format=json", stdout=messages)
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
            report_failure(["OpenBot libtest compilation"], messages)
            raise
    messages = [json.loads(line) for line in messages_file.read_text().splitlines()]
    binaries = [message["executable"] for message in messages
                if message.get("reason") == "compiler-artifact" and message.get("executable")
                and message["target"]["name"] == "open_bots" and message["target"]["kind"] == ["lib"]
                and message["profile"]["test"]]
    if len(binaries) != 1:
        raise RuntimeError("exactly one current OpenBot libtest executable is required")
    helper = output / "openbot-doop-libtest"
    shutil.copy2(binaries[0], helper)
    helper.chmod(0o500)
    names = subprocess.check_output([str(helper), "--list"], text=True).splitlines()
    for expected in ("doop_live_company_setup", "doop_lifecycle_live_company_keys", "doop_e2e_live_company_documents"):
        if sum(name.endswith("::" + expected + ": test") for name in names) != 1:
            raise RuntimeError("missing Doop acceptance test: " + expected)
    digest = hashlib.sha256()
    with helper.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    (output / "libtest-sha256.txt").write_text(digest.hexdigest() + "\n")
    return helper


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    chrome = os.environ.get("DOOP_TEST_UI_CHROME")
    database = os.environ.get("TEST_DATABASE_URL")
    if not chrome or not os.access(chrome, os.X_OK):
        raise RuntimeError("DOOP_TEST_UI_CHROME must name an executable sandboxed system Chrome")
    if not database:
        raise RuntimeError("TEST_DATABASE_URL must name the invocation-owned OpenBot test database")
    run(chrome, "--version")
    output = args.output.resolve()
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    setup = output / "source"
    spec = importlib.util.spec_from_file_location("doop_gate", ROOT / "scripts/doop-test.py")
    runner = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(runner)
    run("python3", "scripts/doop-test.py", "setup", "--managed", "--directory", str(setup))
    service_gate(runner, setup, output, chrome, database)
    helper = compile_helper(output)
    for image in (
        "postgres:18.6-alpine@sha256:123b17998ccaf47e53b502e8c6a1e79afd8f579c6f3eec83c563da026f957395",
        "dxflrs/garage:v2.5.0@sha256:677e5b758e4425a325b811c425c026bf35a4bf42f7ddbaa773de3f4fd779a5a5",
    ):
        run("docker", "pull", image)
    try:
        run("python3", "scripts/doop-test.py", "image", "--managed", "--directory", str(setup),
            "--output", str(output / "image"))
    except subprocess.CalledProcessError:
        for name in ("build.log", "load.log"):
            log = output / "image" / name
            if log.is_file():
                print(log.read_text(errors="replace"), flush=True)
        raise
    run("python3", "scripts/doop-test.py", "acceptance", "--directory", str(setup),
        "--image", str(output / "image/image.json"), "--output", str(output / "acceptance"),
        "--openbot-test-binary", str(helper), "--openbot-test-database", database, "--ui-chrome", chrome)


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        report_failure([Path(__file__).name], io.BytesIO(str(error).encode()))
        raise
