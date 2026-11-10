#!/usr/bin/env python3
"""Pinned, disposable Doop integration service. No production credentials required."""

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import secrets
import select
import shutil
import signal
import socket
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
from types import SimpleNamespace
import urllib.error
import urllib.request


REVISION = "01584b34f6cadcf21767127280cc6e320a6d748a"
SOURCE = "https://github.com/kgoedecke/doop.git"
BUN_VERSION = "1.3.10"
SCRIPTS = Path(__file__).resolve().parent / "doop-test"
PACKAGE = Path(__file__).resolve().parents[1] / "integrations/doop"


def execute(command, timeout=300, **kwargs):
    process = subprocess.Popen(command, start_new_session=True, **kwargs)
    try:
        result = process.wait(timeout=timeout)
        if result:
            raise subprocess.CalledProcessError(result, command)
    finally:
        stop(process)


def digest(path):
    result = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(128 * 1024), b""):
            result.update(chunk)
    return result.hexdigest()


def clean_environment(home):
    """A parent's API keys, database URL, proxy and NODE_OPTIONS never enter Doop."""
    return {
        "PATH": os.environ.get("PATH", os.defpath),
        "HOME": str(home),
        "TMPDIR": str(home / "tmp"),
        "NODE_ENV": "test",
        "REQUIRE_EMAIL_VERIFICATION": "false",
        "RESIDENT_TASK_LIMIT": "0",
        "DO_NOT_TRACK": "1",
    }


def setup(directory):
    directory.mkdir(parents=True, exist_ok=False)
    source = directory / "source"
    tools = directory / "tools"
    print("Installing pinned Doop test dependencies; this first step needs network access.", flush=True)
    execute(["git", "init", "--quiet", str(source)])
    execute(["git", "-C", str(source), "remote", "add", "origin", SOURCE])
    execute(["git", "-C", str(source), "fetch", "--quiet", "--depth=1", "origin", REVISION])
    execute(["git", "-C", str(source), "checkout", "--quiet", "--detach", "FETCH_HEAD"])
    execute([
        "npm", "install", "--prefix", str(tools), "--cache", str(directory / "npm-cache"),
        "--ignore-scripts", "--no-audit", "--no-fund", "bun@" + BUN_VERSION,
    ])
    # Enable only the pinned Bun package's platform-binary selection step.
    execute(["node", "install.js"], cwd=tools / "node_modules/bun")
    bun = tools / "node_modules" / ".bin" / "bun"
    version = subprocess.check_output([str(bun), "--version"], text=True, timeout=10).strip()
    if version != BUN_VERSION:
        raise ValueError("Bun version does not match the pinned version")
    execute([str(bun), "install", "--frozen-lockfile", "--ignore-scripts"], cwd=source)
    manifest = {
        "version": 1, "revision": REVISION, "source": SOURCE, "bun": BUN_VERSION,
        "lock_sha256": digest(source / "bun.lock"),
        "sdk": json.loads((source / "node_modules/@modelcontextprotocol/sdk/package.json").read_text())["version"],
    }
    (directory / "ready.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print("Ready: " + str(directory), flush=True)


def validate(directory):
    manifest = json.loads((directory / "ready.json").read_text())
    if (manifest.get("version"), manifest.get("revision"), manifest.get("source"), manifest.get("bun")) != (1, REVISION, SOURCE, BUN_VERSION):
        raise ValueError("Setup identity differs; create a new setup directory")
    source = directory / "source"
    revision = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True, timeout=10).strip()
    if revision != REVISION or digest(source / "bun.lock") != manifest["lock_sha256"]:
        raise ValueError("Pinned source/lockfile changed; refusing to run")
    for args in (["diff", "--exit-code"], ["diff", "--cached", "--exit-code"]):
        result = subprocess.run(["git", "-C", str(source), *args], capture_output=True, timeout=10)
        if result.returncode:
            raise ValueError("Pinned source has tracked edits; refusing to run")
    sdk = json.loads((source / "node_modules/@modelcontextprotocol/sdk/package.json").read_text())["version"]
    if sdk != manifest["sdk"]:
        raise ValueError("Installed MCP SDK differs from the setup identity")
    return source, manifest


def fingerprint_paths(source, revision, names):
    if len(names) > 8192:
        raise ValueError("Developer source exceeds the file bound")
    result = hashlib.sha256(("doop-test-source-v1\0" + revision + "\0").encode())
    total = 0
    for name in sorted(set(names)):
        path = source / os.fsdecode(name)
        if path.is_symlink():
            raise ValueError("Nonignored source symlinks are not accepted")
        result.update(name + b"\0")
        if not path.exists():
            result.update(b"deleted\0")
            continue
        info = path.stat()
        if not stat.S_ISREG(info.st_mode) or info.st_size > 32 * 1024 * 1024:
            raise ValueError("Invalid or oversized developer source file")
        total += info.st_size
        if total > 256 * 1024 * 1024:
            raise ValueError("Developer source exceeds the byte bound")
        result.update(str(stat.S_IMODE(info.st_mode)).encode() + b"\0")
        result.update(hashlib.sha256(path.read_bytes()).digest())
    return result.hexdigest()


def source_fingerprint(source):
    revision = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True, timeout=10).strip()
    paths = subprocess.check_output(["git", "-C", str(source), "ls-files", "--cached", "--others", "--exclude-standard", "-z"], timeout=10)
    return revision, fingerprint_paths(source, revision, [name for name in paths.split(b"\0") if name])


def package_source(directory, source, expected):
    """Explicit release packaging; repeated test runs never change this package."""
    _pristine, manifest = validate(directory)
    selected = {**manifest, "lock_sha256": digest(source / "bun.lock")}
    reviewed_source(source, expected, selected)
    patch = bytearray()
    commands = [["git", "diff", "HEAD", "--binary", "--full-index", "--no-ext-diff", "--no-textconv"]]
    names = subprocess.check_output(["git", "ls-files", "--others", "--exclude-standard", "-z"], cwd=source, timeout=10)
    for name in sorted(set(name for name in names.split(b"\0") if name)):
        commands.append(["git", "diff", "--no-index", "--binary", "--full-index", "--no-ext-diff", "--no-textconv",
            "--", "/dev/null", os.fsdecode(name)])
    for command in commands:
        with tempfile.TemporaryFile() as output, tempfile.TemporaryFile() as errors:
            try:
                execute(command, cwd=source, stdout=output, stderr=errors, timeout=30)
            except subprocess.CalledProcessError as error:
                if "--no-index" not in command or error.returncode != 1:
                    raise
            output.seek(0, os.SEEK_END)
            if output.tell() > 2 * 1024 * 1024 - len(patch):
                raise ValueError("Managed source patch exceeds its bound")
            output.seek(0)
            patch.extend(output.read())
    reviewed_source(source, expected, selected)
    identity = {**selected, "patch_sha256": hashlib.sha256(patch).hexdigest(), "fingerprint": expected}
    pending = []
    try:
        for name, body in [("managed.patch", patch), ("source.json", (json.dumps(identity, indent=2) + "\n").encode())]:
            with tempfile.NamedTemporaryFile(dir=PACKAGE, prefix=".package-", delete=False) as stream:
                path = Path(stream.name)
                pending.append((path, PACKAGE / name))
                stream.write(body)
                stream.flush()
                os.fsync(stream.fileno())
            path.chmod(0o644)
        # The manifest is the final marker; an interrupted pair fails its hash check.
        for temporary, destination in pending:
            os.replace(temporary, destination)
    finally:
        for temporary, _destination in pending:
            temporary.unlink(missing_ok=True)
    print("Managed package ready: " + expected, flush=True)
    return 0


def package_identity(manifest):
    with (PACKAGE / "source.json").open("rb") as stream:
        encoded = stream.read(16385)
    if len(encoded) > 16384:
        raise ValueError("Managed source manifest exceeds its bound")
    identity = json.loads(encoded)
    required = {"version", "revision", "source", "bun", "lock_sha256", "sdk", "patch_sha256", "fingerprint"}
    if not isinstance(identity, dict) or set(identity) != required or any(identity[key] != manifest[key] for key in required - {"lock_sha256", "patch_sha256", "fingerprint"}):
        raise ValueError("Managed package differs from the pinned setup identity")
    for field in ("lock_sha256", "patch_sha256", "fingerprint"):
        value = identity[field]
        if not isinstance(value, str) or len(value) != 64 or any(char not in "0123456789abcdef" for char in value):
            raise ValueError("Invalid managed source fingerprint")
    patch = PACKAGE / "managed.patch"
    if not patch.is_file() or patch.is_symlink() or patch.stat().st_size > 2 * 1024 * 1024:
        raise ValueError("Invalid or oversized managed source patch")
    if digest(patch) != identity["patch_sha256"]:
        raise ValueError("Managed source patch digest differs")
    return identity


def managed_source(directory, manifest, identity):
    target = directory / "managed-source"
    ready = directory / "managed.json"
    if target.is_symlink() or not target.is_dir() or not ready.is_file() or ready.is_symlink():
        raise ValueError("Managed setup is incomplete; use a new dedicated setup directory")
    if json.loads(ready.read_text()) != identity:
        raise ValueError("Managed setup is stale; use a new dedicated setup directory")
    dependencies = target / "node_modules"
    if dependencies.is_symlink() or not dependencies.is_dir():
        raise ValueError("Managed dependency cache must be a separate installed directory")
    selected = {**manifest, "lock_sha256": identity["lock_sha256"]}
    actual = reviewed_source(target, identity["fingerprint"], selected)
    actual["kind"] = "packaged managed source"
    return target, selected, actual


def prepare_managed(directory):
    source, manifest = validate(directory)
    identity = package_identity(manifest)
    target = directory / "managed-source"
    if target.exists() or target.is_symlink():
        return managed_source(directory, manifest, identity)[0]
    if (directory / "managed.json").exists():
        raise ValueError("Managed setup is incomplete; use a new dedicated setup directory")
    execute(["git", "clone", "--quiet", "--no-hardlinks", "--no-checkout", str(source), str(target)], umask=0o022)
    execute(["git", "-C", str(target), "checkout", "--quiet", "--detach", REVISION], umask=0o022)
    execute(["git", "-C", str(target), "apply", "--binary", "--whitespace=error", str(PACKAGE / "managed.patch")], umask=0o022)
    selected = {**manifest, "lock_sha256": identity["lock_sha256"]}
    reviewed_tree(target, identity["fingerprint"], selected)
    bun = directory / "tools/node_modules/.bin/bun"
    version = subprocess.check_output([str(bun), "--version"], text=True, timeout=10).strip()
    if version != BUN_VERSION:
        raise ValueError("Bun version does not match the pinned version")
    cache_home = directory / "managed-home"
    (cache_home / "tmp").mkdir(mode=0o700, parents=True)
    execute([str(bun), "install", "--frozen-lockfile", "--ignore-scripts"],
        cwd=target, env=clean_environment(cache_home))
    dependencies = target / "node_modules"
    if dependencies.is_symlink() or not dependencies.is_dir():
        raise ValueError("Managed dependency cache must be a separate installed directory")
    reviewed_source(target, identity["fingerprint"], selected)
    (directory / "managed.json").write_text(json.dumps(identity, indent=2) + "\n")
    print("Managed source ready: " + identity["fingerprint"], flush=True)
    return target


def select_source(directory, developer_source, expected_source, managed):
    source, manifest = validate(directory)
    identity = {"revision": REVISION, "kind": "pristine pinned setup"}
    if managed:
        package = package_identity(manifest)
        return managed_source(directory, manifest, package)
    if developer_source is not None:
        source = developer_source.resolve()
        identity = reviewed_source(source, expected_source, manifest)
    return source, manifest, identity


def source_archive(source, destination, expected):
    revision, before = source_fingerprint(source)
    if before != expected:
        raise ValueError("Image source fingerprint differs before archiving")
    paths = subprocess.check_output(["git", "-C", str(source), "ls-files", "--cached", "--others", "--exclude-standard", "-z"], timeout=10)
    with tarfile.open(destination, "w") as archive:
        for name in sorted(set(paths.split(b"\0"))):
            if not name:
                continue
            relative = os.fsdecode(name)
            path = source / relative
            if not path.exists():
                continue
            info = archive.gettarinfo(str(path), arcname=relative)
            info.mtime = info.uid = info.gid = 0
            info.uname = info.gname = ""
            with path.open("rb") as stream:
                archive.addfile(info, stream)
    if source_fingerprint(source) != (revision, expected):
        raise ValueError("Image source changed while archiving")


def build_image(directory, output, developer_source, expected_source, managed):
    source, manifest, identity = select_source(directory, developer_source, expected_source, managed)
    if "fingerprint" not in identity:
        raise ValueError("Image builds require explicit fingerprinted or packaged managed source")
    output.mkdir(parents=True, exist_ok=False)
    tag = "openbot-doop-test:" + identity["fingerprint"][:12] + "-" + secrets.token_hex(4)
    with tempfile.TemporaryDirectory(prefix="openbot-doop-image-") as temporary:
        context = Path(temporary) / "context.tar"
        source_archive(source, context, identity["fingerprint"])
        archive = output / "image.tar"
        with context.open("rb") as data, (output / "build.log").open("wb") as log:
            execute(["docker", "buildx", "build", "--progress=plain", "--tag", tag,
                "--metadata-file", str(output / "build.json"), "--build-arg", "DOOP_SOURCE_FINGERPRINT=" + identity["fingerprint"],
                "--provenance=false", "--output", "type=oci,dest=" + str(archive), "--file", "Dockerfile", "-"],
                stdin=data, stdout=log, stderr=log)
        with (output / "load.log").open("wb") as log:
            execute(["docker", "load", "--input", str(archive)], stdout=log, stderr=log)
    metadata = json.loads((output / "build.json").read_text())
    image_ref = metadata["containerimage.digest"]
    inspected = json.loads(subprocess.check_output(["docker", "image", "inspect", "--format",
        '{{json .Config.Labels}}', image_ref], text=True, timeout=10))
    # Query individual public fields; never retain Docker runtime environment/credentials.
    image_id = subprocess.check_output(["docker", "image", "inspect", "--format", "{{.Id}}", image_ref], text=True, timeout=10).strip()
    user = subprocess.check_output(["docker", "image", "inspect", "--format", "{{.Config.User}}", image_ref], text=True, timeout=10).strip()
    if user != "node" or not isinstance(inspected, dict) or inspected.get("io.openbot.doop.source-fingerprint") != identity["fingerprint"]:
        raise ValueError("Built image user/source identity differs")
    reviewed_source(source, identity["fingerprint"], manifest)
    result = {"source_identity": identity, "setup": manifest, "tag": tag, "image_id": image_id,
        "manifest_digest": metadata["containerimage.digest"], "config_digest": metadata["containerimage.config.digest"], "archive_sha256": digest(archive), "user": user}
    (output / "image.json").write_text(json.dumps(result, indent=2) + "\n")
    print("Image ready: " + image_id, flush=True)
    return 0


def image_identity(path, source_identity, runtime):
    with path.open("rb") as stream:
        encoded = stream.read(16385)
    if len(encoded) > 16384:
        raise ValueError("Image identity exceeds its bound")
    identity = json.loads(encoded)
    expected = source_identity.get("fingerprint")
    image_id = identity.get("image_id", "")
    if (not expected or identity.get("source_identity", {}).get("fingerprint") != expected
        or identity.get("user") != "node" or not image_id.startswith("sha256:") or len(image_id) != 71
        or any(char not in "0123456789abcdef" for char in image_id[7:])):
        raise ValueError("Image/source identity differs from the selected artifact")
    archive = path.parent / "image.tar"
    if digest(archive) != identity.get("archive_sha256"):
        raise ValueError("Image archive digest differs")
    with (runtime / "image-load.log").open("wb") as log:
        execute(["docker", "load", "--input", str(archive)], stdout=log, stderr=log)
    public = json.loads(subprocess.check_output(["docker", "image", "inspect", "--format",
        '{"user":{{json .Config.User}},"labels":{{json .Config.Labels}}}', image_id], text=True, timeout=10))
    if public["user"] != "node" or public["labels"].get("io.openbot.doop.source-fingerprint") != expected:
        raise ValueError("Loaded image has a different user/source identity")
    return identity


def image_service(source, runtime, environment, listener, output, identity):
    spec = importlib.util.spec_from_file_location("openbot_doop_image_service", SCRIPTS / "image_service.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.ImageService(source, runtime, environment, listener, output, identity, execute)


def run_private_deployment(args):
    spec = importlib.util.spec_from_file_location("openbot_doop_private_deployment", SCRIPTS / "private_deployment.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.run(args.directory.resolve(), args.output.resolve(), args.private_deployment, args.image.resolve(),
        args.openbot_test_binary, args.openbot_test_database, args.ui_chrome, SimpleNamespace(**globals()), suite=args.suite)


def run_acceptance(args):
    spec = importlib.util.spec_from_file_location("openbot_doop_acceptance", SCRIPTS / "acceptance.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.run(args, SimpleNamespace(**globals()))


def recover_private_deployment(args):
    spec = importlib.util.spec_from_file_location("openbot_doop_private_recovery", SCRIPTS / "private_deployment.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.recover(args.runtime_dir.absolute(), args.output.resolve(), SimpleNamespace(**globals()))


def reviewed_tree(source, expected, manifest):
    revision, actual = source_fingerprint(source)
    if revision != REVISION or actual != expected:
        raise ValueError("Developer source base/fingerprint does not match the reviewed input")
    if digest(source / "bun.lock") != manifest["lock_sha256"]:
        raise ValueError("Developer source lockfile differs from the pinned setup")
    return {"revision": revision, "fingerprint": actual, "kind": "explicit reviewed developer source"}


def reviewed_source(source, expected, manifest):
    identity = reviewed_tree(source, expected, manifest)
    sdk = json.loads((source / "node_modules/@modelcontextprotocol/sdk/package.json").read_text())["version"]
    if sdk != manifest["sdk"]:
        raise ValueError("Developer source SDK differs from the pinned setup")
    return identity


def wait_ready(process, origin):
    deadline = time.monotonic() + 90
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError("Local Doop exited before readiness (private log retained only during this run)")
        try:
            with opener.open(origin + "/healthz", timeout=1) as response:
                if json.load(response) == {"ok": True}:
                    return
        except (OSError, urllib.error.URLError, ValueError):
            pass
        time.sleep(0.1)
    raise TimeoutError("Local Doop readiness exceeded 90 seconds")


def stop(process, grace=10):
    """Only the process group created by this runner is owned by this runner."""
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        process.wait(timeout=10)
        return
    deadline = time.monotonic() + grace
    while True:
        # Reap an exiting leader before asking about its group. macOS may
        # report EPERM for a zombie-only group during that transition.
        try:
            process.wait(timeout=min(0.05, max(0, deadline - time.monotonic())))
        except subprocess.TimeoutExpired:
            pass
        try:
            os.killpg(process.pid, 0)
        except ProcessLookupError:
            break
        except PermissionError:
            if time.monotonic() < deadline:
                time.sleep(0.01)
                continue
            raise  # Never claim cleanup of a group we still cannot inspect.
        if time.monotonic() >= deadline:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            break
        time.sleep(0.05)
    process.wait(timeout=10)


def start_server(source, runtime, environment, listener, output, number):
    log_path = runtime / ("server-" + str(number) + ".log")
    command = [
        "node", "--import", str(SCRIPTS / "loopback.mjs"),
        "--import", str(source / "node_modules/tsx/dist/loader.mjs"), str(source / "server/index.ts"),
    ]
    process = None
    try:
        with log_path.open("wb") as log:
            process = subprocess.Popen(command, cwd=runtime, env=environment, stdout=log, stderr=log,
                                       pass_fds=(listener.fileno(),), start_new_session=True)
        wait_ready(process, environment["BETTER_AUTH_URL"])
        return process
    except BaseException as original:
        # Ownership transfers only after readiness. Even diagnostic failures and
        # Ctrl-C must clean this child, including a not-yet-returned restart.
        try:
            try:
                if isinstance(original, (RuntimeError, TimeoutError)):
                    diagnostic = log_path.read_bytes()[-16384:].decode("utf-8", errors="replace")
                    (output / "startup.txt").write_text(diagnostic.replace(environment["BETTER_AUTH_SECRET"], "[redacted]"))
            finally:
                if process is not None:
                    stop(process)
        except BaseException as cleanup:
            raise original from cleanup
        raise


def drive_probe(probe, restart, restore=None):
    deadline = time.monotonic() + 180
    pending = b""
    configured = False
    restarted = False
    restored = False
    lines = 0
    while time.monotonic() < deadline:
        readable, _, _ = select.select([probe.stdout], [], [], 0.1)
        if not readable:
            continue
        chunk = os.read(probe.stdout.fileno(), 4096)
        if not chunk:
            return probe.wait(timeout=5)
        pending += chunk
        if len(pending) > 65536:
            raise ValueError("Probe control/output exceeds its bound")
        while b"\n" in pending:
            raw, pending = pending.split(b"\n", 1)
            lines += 1
            if lines > 160:
                raise ValueError("Probe line budget exceeded")
            line = raw.decode("utf-8")
            if not line.startswith("DOOP_TEST_CONTROL "):
                print(line, flush=True)
                continue
            control = json.loads(line[len("DOOP_TEST_CONTROL "):])
            if control.get("kind") == "configure" and not configured:
                bindings = control.get("bindings")
                if not isinstance(bindings, list) or not 1 <= len(bindings) <= 128:
                    raise ValueError("Invalid local binding control message")
                restart(bindings)
                configured = True
            elif control == {"kind": "restart"} and configured and not restarted:
                restart(None)
                restarted = True
            elif (control == {"kind": "restore"} and configured and not restored
                    and not restarted and restore is not None):
                restore()
                restored = True
            else:
                raise ValueError("Unexpected local probe control stage")
            probe.stdin.write(b"ready\n")
            probe.stdin.flush()
    raise TimeoutError("Probe runtime exceeded 180 seconds")


class CleanupFailure(RuntimeError):
    def __init__(self, failures):
        self.failures = tuple(failures)
        super().__init__("Owned cleanup failed: " + ", ".join(
            name + " (" + type(error).__name__ + ")" for name, error in failures))


def cleanup_run(probe, container, process, primary):
    cleanups = []
    if probe is not None:
        cleanups.extend((("probe process", lambda: stop(probe)),
            ("probe stdin", probe.stdin.close), ("probe stdout", probe.stdout.close)))
    if container is not None:
        cleanups.append(("image resources", container.close))
    elif process is not None:
        cleanups.append(("native service", lambda: stop(process)))
    failures = []
    for name, cleanup in cleanups:
        try:
            cleanup()
        except BaseException as error:
            failures.append((name, error))
    if failures:
        failure = CleanupFailure(failures)
        if primary is not None:
            raise primary from failure
        raise failure from failures[0][1]


def run(directory, output, suite, chrome=None, developer_source=None, expected_source=None, managed=False, openbot_binary=None, openbot_database=None, image=None):
    source, manifest, source_identity = select_source(directory, developer_source, expected_source, managed)
    if openbot_binary is not None and (not openbot_binary.is_file() or "fingerprint" not in source_identity):
        raise ValueError("Openbot setup needs an explicit compiled test binary and fingerprinted Doop source")
    helper_digest = digest(openbot_binary) if openbot_binary is not None else None
    output.mkdir(parents=True, exist_ok=False)
    # No shared database, source writes, persistent accounts, or port selection race.
    with tempfile.TemporaryDirectory(prefix="openbot-doop-run-") as temporary:
        runtime = Path(temporary)
        (runtime / "home/tmp").mkdir(parents=True)
        environment = clean_environment(runtime / "home")
        image_metadata = image_identity(image, source_identity, runtime) if image is not None else None
        if chrome is not None:
            if not chrome.is_file():
                raise ValueError("--chrome must name an installed Chrome/Chromium executable")
            environment["CHROME_PATH"] = str(chrome.resolve())
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            listener.listen()
            origin = "http://127.0.0.1:" + str(listener.getsockname()[1])
            environment.update({
                "PORT": str(listener.getsockname()[1]), "BETTER_AUTH_URL": origin,
                "BETTER_AUTH_SECRET": secrets.token_urlsafe(48),
                "DOOP_TEST_SOCKET_FD": str(listener.fileno()),
                "DOOP_TEST_SOURCE": str(source),
                "DOOP_TEST_SEED_METADATA": str(runtime / "seed.json"),
                "DOOP_TEST_WS_MODULE": str(source / "node_modules/ws/wrapper.mjs"),
                "DOOP_TEST_SOURCE_REVISION": REVISION,
                "DOOP_TEST_SOURCE_FINGERPRINT": source_identity.get("fingerprint", ""),
            })
            if openbot_binary is not None:
                environment["DOOP_TEST_OPENBOT_BINARY"] = str(openbot_binary.resolve())
                environment["DOOP_TEST_OPENBOT_DATABASE"] = openbot_database
            process = None
            container = image_service(source, runtime, environment, listener, output, image_metadata) if image_metadata is not None else None
            number = 0
            probe = None
            primary = None
            def restart(bindings):
                nonlocal process, number
                if container is not None:
                    container.stop()
                elif process is not None:
                    stop(process)
                if bindings is not None:
                    environment["MANAGED_COMPANY_WORKSPACES"] = json.dumps(bindings, separators=(",", ":"))
                number += 1
                if container is not None:
                    container.start(number)
                else:
                    process = start_server(source, runtime, environment, listener, output, number)
            try:
                if container is not None:
                    import base64
                    environment["DOOP_TEST_IMAGE_ENCRYPTION_KEYS"] = "test:" + base64.b64encode(secrets.token_bytes(32)).decode()
                    container.prepare()
                    container.start(0)
                else:
                    process = start_server(source, runtime, environment, listener, output, 0)
                with (runtime / "probe.log").open("wb") as log:
                    probe = subprocess.Popen(["node", str(SCRIPTS / "probe.mjs"), origin, str(output), suite],
                        cwd=runtime, env=environment, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                        stderr=log, start_new_session=True)
                code = drive_probe(probe, restart)
                if process is not None and process.poll() is not None:
                    raise RuntimeError("Doop exited during probes")
                report = output / "report.json"
                if not report.exists():
                    raise RuntimeError("Probes did not produce a report")
                if "fingerprint" in source_identity:
                    reviewed_source(source, source_identity["fingerprint"], manifest)
                result = json.loads(report.read_text())
                result.update({"setup": manifest, "source_identity": source_identity,
                    "restarts": number, "deployment": "disposable pinned container image" if image_metadata is not None else "disposable local source process; no container image"})
                if image_metadata is not None:
                    result["image"] = {key: image_metadata[key] for key in ("image_id", "manifest_digest", "archive_sha256", "user")}
                if openbot_binary is not None:
                    if digest(openbot_binary) != helper_digest:
                        raise ValueError("Openbot helper binary changed during the run")
                    result["openbot_helper_sha256"] = helper_digest
                report.write_text(json.dumps(result, indent=2) + "\n")
                return code
            except BaseException as error:
                primary = error
                raise
            finally:
                cleanup_run(probe, container, process, primary)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    install = commands.add_parser("setup", help="one-time pinned dependency setup; requires network")
    install.add_argument("--directory", type=Path, required=True, help="new dedicated directory")
    install.add_argument("--managed", action="store_true", help="also prepare the verified managed-company source package")
    prepare = commands.add_parser("prepare-managed", help="prepare packaged source using an existing pristine setup")
    prepare.add_argument("--directory", type=Path, required=True)
    recover = commands.add_parser("cleanup-private", help="reconcile retained positively owned private deployment resources")
    recover.add_argument("--runtime-dir", type=Path, required=True, help="private run directory from cleanup-pending.json")
    recover.add_argument("--output", type=Path, required=True, help="new directory for public cleanup evidence")
    identity = commands.add_parser("fingerprint", help="identify an explicit developer source artifact")
    identity.add_argument("--source", type=Path, required=True)
    package = commands.add_parser("package", help="explicitly regenerate the reviewed managed source package")
    package.add_argument("--directory", type=Path, required=True, help="verified pristine tool/dependency setup")
    package.add_argument("--source", type=Path, required=True, help="reviewed actual Doop checkout")
    package.add_argument("--source-fingerprint", required=True)
    test = commands.add_parser("run", help="fresh isolated local service; reuses setup without installing")
    test.add_argument("--directory", type=Path, required=True)
    test.add_argument("--output", type=Path, required=True, help="new directory for sanitized evidence")
    test.add_argument("--suite", choices=("contract", "all", "e2e"), default="all")
    test.add_argument("--chrome", type=Path, help="explicit Chrome/Chromium executable for screenshot/PDF checks")
    test.add_argument("--source", type=Path, help="explicit reviewed local Doop worktree")
    test.add_argument("--source-fingerprint", help="exact fingerprint required with --source")
    test.add_argument("--managed", action="store_true", help="use the verified packaged managed-company source")
    test.add_argument("--openbot-test-binary", type=Path, help="explicit compiled libtest binary for live company setup proof")
    test.add_argument("--openbot-test-database", help="owned PostgreSQL test server URL; helper creates/drops its own database")
    test.add_argument("--image", type=Path, help="verified image.json from this runner's image command")
    test.add_argument("--private-deployment", choices=("disk", "bucket"), help="actual-image native PostgreSQL/TLS private acceptance")
    test.add_argument("--ui-chrome", type=Path, help="installed browser for private human acceptance; renderer stays in the image")
    acceptance = commands.add_parser("acceptance", help="four fresh private runs: all/e2e in disk/bucket modes")
    acceptance.add_argument("--directory", type=Path, required=True)
    acceptance.add_argument("--output", type=Path, required=True, help="new directory for complete release evidence")
    acceptance.add_argument("--image", type=Path, required=True, help="verified managed-source image.json")
    acceptance.add_argument("--openbot-test-binary", type=Path, required=True, help="current immutable Openbot libtest executable")
    acceptance.add_argument("--openbot-test-database", required=True, help="owned PostgreSQL; each helper creates/drops its database")
    acceptance.add_argument("--ui-chrome", type=Path, help="installed browser for ordinary human acceptance")
    acceptance.set_defaults(managed=True)
    build = commands.add_parser("image", help="build and retain an immutable non-root Doop image")
    build.add_argument("--directory", type=Path, required=True)
    build.add_argument("--output", type=Path, required=True, help="new directory for image archive and public build evidence")
    build.add_argument("--source", type=Path)
    build.add_argument("--source-fingerprint")
    build.add_argument("--managed", action="store_true")
    args = parser.parse_args()
    if os.name != "posix":
        parser.error("This runner needs POSIX inherited sockets and process groups")
    required_tools = ("docker",) if args.command == "cleanup-private" else ("git", "node") + (("npm",) if args.command == "setup" else ())
    for tool in required_tools:
        if not shutil.which(tool):
            parser.error("Missing required executable: " + tool)
    try:
        if args.command == "setup":
            setup(args.directory.resolve())
            if args.managed:
                prepare_managed(args.directory.resolve())
            return 0
        if args.command == "prepare-managed":
            prepare_managed(args.directory.resolve())
            return 0
        if args.command == "cleanup-private":
            return recover_private_deployment(args)
        if args.command == "fingerprint":
            revision, value = source_fingerprint(args.source.resolve())
            print(json.dumps({"revision": revision, "fingerprint": value}))
            return 0
        if args.command == "package":
            return package_source(args.directory.resolve(), args.source.resolve(), args.source_fingerprint)
        if args.command == "acceptance":
            return run_acceptance(args)
        if (args.source is None) != (args.source_fingerprint is None):
            parser.error("--source and --source-fingerprint must be supplied together")
        if args.managed and args.source is not None:
            parser.error("--managed and --source are mutually exclusive")
        if args.command == "image":
            return build_image(args.directory.resolve(), args.output.resolve(), args.source, args.source_fingerprint, args.managed)
        if (args.openbot_test_binary is None) != (args.openbot_test_database is None):
            parser.error("--openbot-test-binary and --openbot-test-database must be supplied together")
        if args.image is not None and args.chrome is not None:
            parser.error("--chrome selects a native browser; image runs use their pinned Chromium")
        if args.private_deployment is not None:
            if not args.managed or args.image is None or args.suite not in ("all", "e2e"):
                parser.error("--private-deployment requires --managed, --image and --suite all or e2e")
            if args.openbot_test_binary is None:
                parser.error("--private-deployment requires --openbot-test-binary and --openbot-test-database for lifecycle acceptance")
            return run_private_deployment(args)
        if args.suite == "e2e":
            parser.error("--suite e2e requires --private-deployment")
        if args.ui_chrome is not None:
            parser.error("--ui-chrome requires --private-deployment")
        return run(args.directory.resolve(), args.output.resolve(), args.suite, args.chrome, args.source, args.source_fingerprint, args.managed, args.openbot_test_binary, args.openbot_test_database, args.image)
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print("Doop test setup/run failed: " + str(error), file=sys.stderr)
        if isinstance(error.__cause__, CleanupFailure):
            print(str(error.__cause__), file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
