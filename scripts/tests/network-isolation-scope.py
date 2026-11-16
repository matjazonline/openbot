"""Linux socket proof: isolate test descendants without cutting off their supervisor.

Uses a local non-loopback TCP fixture, never a public endpoint. Also runs a temporary
UID-selector counterexample so an ineffective firewall cannot satisfy this regression.
"""
import errno
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import socketserver
import subprocess
import sys
import tempfile
import threading
import time
import traceback


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
              if "OB_TEST_" in line or "RIG_TEST_" in line or "MEMORY_TEST_" in line] for rule in rules])


def extracted_wrapper(directory, memory=False, old_selector=False, broken_preflight=False, inherited_signals=False):
    directory.mkdir()
    scripts = directory / "scripts"
    scripts.mkdir()
    source = ROOT / ("services/memory" if memory else "") / "scripts/test-network-isolation.sh"
    wrapper = scripts / "test-network-isolation.sh"
    text = source.read_text()
    wrapper.write_text(text)
    helper_relative = (Path("scripts/network-isolation-linux.sh") if memory else
                       Path("services/memory/scripts/network-isolation-linux.sh"))
    helper = directory / helper_relative
    helper.parent.mkdir(parents=True, exist_ok=True)
    mechanism = (ROOT / "services/memory/scripts/network-isolation-linux.sh").read_text()
    if old_selector:
        assert '--gid-owner "$test_gid"' in mechanism
        mechanism = mechanism.replace('--gid-owner "$test_gid"', '--uid-owner "$test_uid"')
    if inherited_signals:
        assert "env --default-signal=INT,TERM " in mechanism
        mechanism = mechanism.replace("env --default-signal=INT,TERM ", "")
    helper.write_text(mechanism)
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


def sudo_shim(directory, mode, stage):
    shim = directory / "shim"
    shim.mkdir(exist_ok=True)
    # Run the actual privileged operation first for signal/deadline cases. The
    # signal handshake holds a successful creator before its shell can publish it.
    (shim / "sudo").write_text("#!" + sys.executable + "\n" +
        "import os, signal, subprocess, sys, time\nfrom pathlib import Path\n" +
        "mode, stage, real, gate = " + repr((mode, stage, shutil.which("sudo"), str(directory))) + "\n" +
        "args = sys.argv[1:]\n" +
        "target = stage in args and (stage != 'iptables' and stage != 'ip6tables' or '-N' in args)\n" +
        "if mode == 'failure' and target: raise SystemExit(73)\n" +
        "if mode == 'slow' and target:\n" +
        "    position = args.index('groupdel')\n" +
        "    group = args[position + 1]\n" +
        "    script = 'groupdel \"$1\"; echo \"$$\" > \"$2\"; sleep 30; touch \"$3\"'\n" +
        "    args[position:] = ['sh', '-c', script, 'cleanup-fixture', group, str(Path(gate, 'privileged-pid')), str(Path(gate, 'late-effect'))]\n" +
        "result = subprocess.run([real, *args]).returncode\n" +
        "if result == 0 and target and mode == 'signal':\n" +
        "    Path(gate, 'created').touch()\n" +
        "    deadline = time.monotonic() + 10\n" +
        "    while not Path(gate, 'return').exists():\n" +
        "        assert time.monotonic() < deadline\n        time.sleep(0.01)\n" +
        "raise SystemExit(result)\n")
    (shim / "sudo").chmod(0o755)
    return dict(os.environ, PATH=str(shim) + ":" + os.environ["PATH"])


def setup_failures(wrappers, directory):
    before = owned_resources()
    for stage in ("groupadd", "ip6tables"):
        environment = sudo_shim(directory, "failure", stage)
        for wrapper in wrappers:
            marker = directory / "must-not-run"
            result = subprocess.run(["bash", str(wrapper), "touch", str(marker)],
                                    env=environment, capture_output=True, text=True, timeout=15)
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


def interrupted_creators(wrappers, directory):
    before = owned_resources()
    for index, wrapper in enumerate(wrappers):
        for stage in ("groupadd", "iptables"):
            for interrupted in (signal.SIGINT, signal.SIGTERM):
                gate = directory / f"signal-{index}-{stage}-{interrupted}"
                gate.mkdir()
                environment = sudo_shim(gate, "signal", stage)
                marker = gate / "must-not-run"
                process = subprocess.Popen(["bash", str(wrapper), "touch", str(marker)],
                                           env=environment, stdout=subprocess.PIPE,
                                           stderr=subprocess.STDOUT, text=True)
                try:
                    deadline = time.monotonic() + 8
                    while not (gate / "created").exists():
                        if process.poll() is not None:
                            raise AssertionError(process.communicate()[0])
                        assert time.monotonic() < deadline, "creator did not reach successful-operation pause"
                        time.sleep(0.01)
                    process.send_signal(interrupted)
                finally:
                    (gate / "return").touch()
                output = process.communicate(timeout=12)[0]
                assert process.returncode == 128 + interrupted, output
                assert not marker.exists(), "interrupted setup ran the command"
                assert owned_resources() == before, "signal before ownership publication leaked resources"


def cleanup_deadline(wrappers, directory):
    before = owned_resources()
    for index, wrapper in enumerate(wrappers):
        gate = directory / f"deadline-{index}"
        gate.mkdir()
        # Real group deletion completes, then a privileged child stops responding. The
        # native wrapper deadline must fire and report failure, even though the
        # fixture deliberately leaves no system resource behind.
        environment = sudo_shim(gate, "slow", "groupdel")
        started = time.monotonic()
        result = subprocess.run(["bash", str(wrapper), "true"], env=environment,
                                capture_output=True, text=True, timeout=15)
        elapsed = time.monotonic() - started
        assert result.returncode != 0, "successful command hid cleanup timeout"
        assert "network isolation cleanup failed" in result.stderr, result.stderr
        assert elapsed < 12, f"cleanup did not honor its own deadline: {elapsed}"
        assert owned_resources() == before
        child_pid = int((gate / "privileged-pid").read_text())
        try:
            deadline = time.monotonic() + 2
            while True:
                state = subprocess.run(["ps", "-p", str(child_pid), "-o", "stat="],
                                       capture_output=True, text=True, timeout=3).stdout.strip()
                if not state or state.startswith("Z"):
                    break
                assert time.monotonic() < deadline, "privileged cleanup child survived the wrapper deadline"
                time.sleep(0.02)
            assert not (gate / "late-effect").exists(), "cleanup continued after its deadline"
        finally:
            # A deliberately broken counterexample must not leave privileged work alive.
            command = subprocess.run(["ps", "-p", str(child_pid), "-o", "args="],
                                     capture_output=True, text=True, timeout=3).stdout
            if str(gate / "privileged-pid") in command:
                children = subprocess.run(["ps", "-o", "pid=", "--ppid", str(child_pid)],
                                          capture_output=True, text=True, timeout=3).stdout.split()
                subprocess.run(["sudo", "-n", "kill", "-KILL", *children, str(child_pid)],
                               capture_output=True, timeout=3)
        print(f"PASS: cleanup deadline and visible failure for entrypoint{index} ({elapsed:.2f}s)", flush=True)


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
            interrupted_creators(wrappers, directory)
            print("PASS: INT/TERM after actual group/chain creation preserve status and clean both entrypoints", flush=True)
            cleanup_deadline(wrappers, directory)
            inherited = extracted_wrapper(directory / "inherited-signal-wrapper", inherited_signals=True)
            inherited_case = directory / "inherited-signal-case"
            inherited_case.mkdir()
            try:
                cleanup_deadline([inherited], inherited_case)
            except AssertionError as error:
                assert str(error) == "privileged cleanup child survived the wrapper deadline", str(error)
            else:
                raise AssertionError("inherited ignored-signal counterexample did not expose the surviving child")
            print("PASS: ignored owner signals propagate without explicit child reset; privileged work survives", flush=True)
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
        try:
            main()
        except Exception:
            if os.environ.get("GITHUB_ACTIONS") == "true":
                message = traceback.format_exc().replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")
                print("::error::" + message, flush=True)
            raise
