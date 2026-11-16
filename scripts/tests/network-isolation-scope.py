"""Linux socket proof: isolate test descendants without cutting off their supervisor.

Uses a local non-loopback TCP fixture, never a public endpoint. Also runs a temporary
UID-selector counterexample so an ineffective firewall cannot satisfy this regression.
"""
import errno
import json
import os
from pathlib import Path
import shutil
import socket
import socketserver
import subprocess
import sys
import tempfile
import threading
import time


ROOT = Path(__file__).resolve().parents[2]
MARKER = "exported fixture value with spaces ; $literal"
ARGUMENT = "argument with spaces ; $literal"


def exchange(address, port):
    with socket.create_connection((address, port), timeout=2) as client:
        assert client.recv(5) == b"scope"


def refused(address, port):
    try:
        exchange(address, port)
    except OSError as error:
        assert error.errno in {errno.EPERM, errno.EACCES, errno.ECONNREFUSED}, error
    else:
        raise AssertionError("guarded descendant reached the non-loopback fixture")


def child(arguments):
    directory, address, port, status, expected = arguments[:5]
    expected = json.loads(expected)
    directory = Path(directory)
    assert arguments[5:] == [ARGUMENT]
    assert os.getuid() == expected["uid"]
    assert set(os.getgroups()) == set(expected["groups"])
    for key, value in expected["environment"].items():
        assert os.environ.get(key) == value, key
    assert os.environ["CARGO_NET_OFFLINE"] == "true"
    assert all(key not in os.environ for key in ("HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"))
    port = int(port)
    exchange("127.0.0.1", port)
    refused(address, port)
    subprocess.run([sys.executable, "-I", str(Path(__file__).resolve()),
                    "--nested", address, str(port), str(os.getgid())], check=True, timeout=5)
    (directory / "ready.json").write_text(json.dumps({"gid": os.getgid()}))
    deadline = time.monotonic() + 15
    while not (directory / "release").exists():
        assert time.monotonic() < deadline, "supervisor did not release child"
        time.sleep(0.02)
    return int(status)


def owned_resources():
    groups = subprocess.check_output(["getent", "group"], text=True).splitlines()
    rules = [subprocess.check_output(["sudo", fw, "-w", "-S"], text=True)
             for fw in ("iptables", "ip6tables")]
    return ([line for line in groups if line.startswith("obnet_")],
            [[line for line in rule.splitlines()
              if "RIG_TEST_" in line or "MEMORY_TEST_" in line] for rule in rules])


def extracted_wrapper(directory, memory=False, old_selector=False, broken_preflight=False):
    directory.mkdir()
    scripts = directory / "scripts"
    scripts.mkdir()
    source = ROOT / ("services/memory" if memory else "") / "scripts/test-network-isolation.sh"
    wrapper = scripts / "test-network-isolation.sh"
    text = source.read_text()
    if old_selector:
        assert '--gid-owner "$test_gid"' in text
        text = text.replace('--gid-owner "$test_gid"', '--uid-owner "$test_uid"')
    wrapper.write_text(text)
    relative = Path("network-isolation.py") if memory else Path("tests/network-isolation.py")
    preflight = scripts / relative
    preflight.parent.mkdir(exist_ok=True)
    if broken_preflight:
        preflight.write_text("raise SystemExit(74)\n")
    else:
        shutil.copyfile(source.parent / relative, preflight)
    return wrapper


def exercise(wrappers, directory, address, port, expect_control=True):
    before = owned_resources()
    processes = []
    expected = {"uid": os.getuid(), "groups": sorted(set(os.getgroups()) | {os.getgid()}),
                "environment": {"HOME": os.environ["HOME"], "PATH": os.environ["PATH"],
                                "ISOLATION_SCOPE_MARKER": MARKER}}
    environment = dict(os.environ, ISOLATION_SCOPE_MARKER=MARKER, HTTP_PROXY="http://invalid")
    try:
        for index, wrapper in enumerate(wrappers):
            working = directory / str(index)
            working.mkdir()
            status = 17 if index == 0 else 0
            command = ["bash", str(wrapper), sys.executable, "-I", str(Path(__file__).resolve()),
                       "--child", str(working), address, str(port), str(status),
                       json.dumps(expected), ARGUMENT]
            process = subprocess.Popen(command, env=environment, stdout=subprocess.PIPE,
                                       stderr=subprocess.STDOUT, text=True)
            processes.append((process, working, status))
        deadline = time.monotonic() + 12
        for process, working, _ in processes:
            while not (working / "ready.json").exists():
                if process.poll() is not None:
                    raise AssertionError(process.communicate()[0])
                assert time.monotonic() < deadline, "guarded child never became ready"
                time.sleep(0.02)
        # This is the CI control-plane assertion. The old UID selector rejects this
        # supervisor's socket even though the supervisor never entered the wrapper.
        if expect_control:
            exchange(address, port)
        else:
            refused(address, port)
        gids = [json.loads((working / "ready.json").read_text())["gid"]
                for _, working, _ in processes]
        assert len(set(gids)) == len(gids)
        assert os.getgid() not in gids
    finally:
        for _, working, _ in processes:
            (working / "release").touch()
        for process, _, status in processes:
            try:
                output = process.communicate(timeout=10)[0]
            except subprocess.TimeoutExpired:
                process.terminate()
                output = process.communicate(timeout=10)[0]
                raise AssertionError("wrapper did not finish: " + output)
            assert process.returncode == status, output
        assert owned_resources() == before, "wrapper left firewall rules or groups behind"


def setup_failures(wrappers, directory):
    before = owned_resources()
    shim = directory / "shim"
    shim.mkdir()
    real_sudo = shutil.which("sudo")
    for stage in ("groupadd", "ip6tables"):
        (shim / "sudo").write_text(
            '#!/bin/bash\nif [[ "$1" == "' + stage + '"' +
            (' && "$3" == -N' if stage == "ip6tables" else '') +
            ' ]]; then exit 73; fi\nexec ' + real_sudo + ' "$@"\n')
        (shim / "sudo").chmod(0o755)
        for wrapper in wrappers:
            marker = directory / "must-not-run"
            result = subprocess.run(["bash", str(wrapper), "touch", str(marker)],
                                    env=dict(os.environ, PATH=str(shim) + ":" + os.environ["PATH"]),
                                    capture_output=True, text=True, timeout=15)
            assert result.returncode == 73, result.stdout + result.stderr
            assert not marker.exists(), "setup failure ran an unguarded command"
            assert owned_resources() == before, "partial setup leaked resources"
    for memory in (False, True):
        wrapper = extracted_wrapper(directory / ("broken-memory" if memory else "broken-root"),
                                    memory=memory, broken_preflight=True)
        marker = directory / "must-not-run"
        result = subprocess.run(["bash", str(wrapper), "touch", str(marker)],
                                capture_output=True, text=True, timeout=15)
        assert result.returncode == 74, result.stdout + result.stderr
        assert not marker.exists(), "failed guarded preflight ran the command"
        assert owned_resources() == before, "failed guarded preflight leaked resources"


class Fixture(socketserver.TCPServer):
    allow_reuse_address = True


class Handler(socketserver.BaseRequestHandler):
    def handle(self):
        self.request.sendall(b"scope")


def main():
    assert sys.platform == "linux", "Linux scope proof requires Linux, sudo and iptables"
    addresses = json.loads(subprocess.check_output(
        ["ip", "-j", "-4", "address", "show", "scope", "global"], text=True))
    address = next(info["local"] for interface in addresses for info in interface["addr_info"]
                   if info["family"] == "inet")
    with tempfile.TemporaryDirectory(prefix="network-scope-") as temporary, Fixture(("0.0.0.0", 0), Handler) as server:
        directory = Path(temporary)
        thread = threading.Thread(target=server.serve_forever)
        thread.start()
        try:
            port = server.server_address[1]
            exchange(address, port)
            wrappers = [ROOT / "scripts/test-network-isolation.sh",
                        extracted_wrapper(directory / "standalone-memory", memory=True)]
            parallel = directory / "parallel"
            parallel.mkdir()
            exercise(wrappers, parallel, address, port)
            print("PASS: both concurrent entrypoints isolate descendants and preserve supervisor/UID/environment/status", flush=True)
            negative = directory / "negative"
            negative.mkdir()
            old = extracted_wrapper(directory / "old-selector", old_selector=True)
            exercise([old], negative, address, port, expect_control=False)
            print("PASS: changing only the selector to UID blocks the supervisor", flush=True)
            setup_failures(wrappers, directory)
            print("PASS: setup, partial firewall and guarded-preflight failures leave no command or resources", flush=True)
        finally:
            server.shutdown()
            thread.join(timeout=3)
            assert not thread.is_alive()


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--child":
        raise SystemExit(child(sys.argv[2:]))
    if len(sys.argv) > 1 and sys.argv[1] == "--nested":
        assert os.getgid() == int(sys.argv[4])
        refused(sys.argv[2], int(sys.argv[3]))
    else:
        main()
