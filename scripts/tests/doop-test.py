#!/usr/bin/env python3
"""Resource and failure boundaries for the disposable Doop service runner."""
import importlib.util
import hashlib
from contextlib import redirect_stderr
import io
import json
import os
from pathlib import Path
import select
import signal
import shutil
import socket
import socketserver
import ssl
import subprocess
import sys
import tempfile
import time
import threading
from types import SimpleNamespace
import tarfile
import unittest
from unittest.mock import Mock, patch
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("doop_test", ROOT / "scripts/doop-test.py")
RUNNER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUNNER)
IMAGE_SPEC = importlib.util.spec_from_file_location("doop_image", RUNNER.SCRIPTS / "image_service.py")
IMAGE = importlib.util.module_from_spec(IMAGE_SPEC)
IMAGE_SPEC.loader.exec_module(IMAGE)
TLS_SPEC = importlib.util.spec_from_file_location("doop_ingress", RUNNER.SCRIPTS / "ingress.py")
TLS = importlib.util.module_from_spec(TLS_SPEC)
TLS_SPEC.loader.exec_module(TLS)
OWNED_SPEC = importlib.util.spec_from_file_location("doop_ownership", RUNNER.SCRIPTS / "ownership.py")
OWNED = importlib.util.module_from_spec(OWNED_SPEC)
OWNED_SPEC.loader.exec_module(OWNED)
PRIVATE_SPEC = importlib.util.spec_from_file_location("doop_private_deployment", RUNNER.SCRIPTS / "private_deployment.py")
PRIVATE = importlib.util.module_from_spec(PRIVATE_SPEC)
PRIVATE_SPEC.loader.exec_module(PRIVATE)


class CompleteAcceptanceTests(unittest.TestCase):
    """Controlled orchestration only; actual-image behavioral reports have their own validator."""

    def fixture_run(self, output, fault=None):
        calls = []
        def case(args):
            calls.append((args.suite, args.private_deployment))
            args.output.mkdir(mode=0o700)
            if fault == "exit" and len(calls) == 1:
                return 1
            report = {"suite": args.suite, "storage_mode": args.private_deployment,
                "source_identity": {"fingerprint": "a" * 64}, "image": {"image_id": "fixture-image"},
                "openbot_helper_sha256": "b" * 64, "checks": [{"name": "controlled orchestration fixture", "status": "PASS"}]}
            cleanup = {"version": 1, "confirmed": True, "runtime_removed": True}
            if len(calls) == 2:
                if fault == "identity": report["openbot_helper_sha256"] = "c" * 64
                if fault == "family": report["suite"] = "contract"
                if fault == "cleanup": cleanup["confirmed"] = False
                if fault == "integer-confirmation": cleanup["confirmed"] = 1
                if fault == "pending": (args.output / "cleanup-pending.json").write_text("{}")
            (args.output / "report.json").write_text(json.dumps(report))
            (args.output / "cleanup-receipt.json").write_text(json.dumps(cleanup))
            return 0
        return SimpleNamespace(output=output), calls, case

    def test_complete_release_requires_all_four_fresh_runs_and_one_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "evidence"
            args, calls, case = self.fixture_run(output)
            with patch.object(RUNNER, "run_private_deployment", side_effect=case):
                self.assertEqual(RUNNER.run_acceptance(args), 0)
            self.assertEqual(calls, [("all", "disk"), ("all", "bucket"), ("e2e", "disk"), ("e2e", "bucket")])
            report = json.loads((output / "acceptance.json").read_text())
            self.assertEqual(report["status"], "PASS")
            self.assertEqual(len(report["runs"]), 4)
            self.assertTrue(all(len(item["report_sha256"]) == 64 for item in report["runs"]))

    def test_failed_uncertain_wrong_family_or_changed_helper_prevents_the_next_run(self):
        for fault in ("exit", "identity", "family", "cleanup", "integer-confirmation", "pending"):
            with self.subTest(fault=fault), tempfile.TemporaryDirectory() as directory:
                output = Path(directory) / "evidence"
                args, calls, case = self.fixture_run(output, fault)
                with patch.object(RUNNER, "run_private_deployment", side_effect=case):
                    with self.assertRaises(RuntimeError): RUNNER.run_acceptance(args)
                self.assertEqual(len(calls), 1 if fault == "exit" else 2)
                self.assertFalse((output / "e2e-disk").exists())
                self.assertEqual(json.loads((output / "acceptance.json").read_text())["status"], "FAIL")

    def test_e2e_cannot_run_as_an_unmanaged_native_diagnostic(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(RUNNER, "run") as native:
            arguments = ["doop-test.py", "run", "--directory", directory, "--output", directory + "/unused", "--suite", "e2e"]
            with patch.object(sys, "argv", arguments), redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit) as error: RUNNER.main()
            self.assertEqual(error.exception.code, 2)
            native.assert_not_called()


class DockerControlFixture:
    """Controlled daemon outcomes, not proof of native Docker cleanup."""
    def __init__(self):
        self.engine = "fixture-engine"
        self.objects = {}
        self.calls = []
        self.inspected_owner = None

    def __call__(self, *args, **kwargs):
        self.calls.append(args)
        if args[0] == "info":
            return self.engine
        if args[1] == "ls":
            owner = args[args.index("--filter") + 1].split("=", 2)[2]
            return "\n".join(key for key, value in self.objects.items()
                if value["kind"] == args[0] and value["owner"] == owner)
        if args[1] == "inspect":
            actual = self.objects[args[-1]]
            return json.dumps({"id": args[-1], "name": actual["name"],
                "owner": self.inspected_owner or actual["owner"]})
        if args[0] == "stop":
            return args[-1]
        if args[0] == "rm" or args[1] == "rm":
            del self.objects[args[-1]]
            return args[-1]
        raise AssertionError(args)


class PrivateAcceptanceGateTests(unittest.TestCase):
    def test_e2e_groups_are_independently_required_executed_and_cannot_pass_as_all(self):
        names = (
            "live two-company Openbot document workflows and current approvals",
            "live worker restart reuses known creates and parks a lost receipt",
            "live document edits persist and bot keys reopen and export",
            "live ordinary humans reopen their generated private documents",
            "live human PDF controls export generated private documents",
            "live generated document IDs and private media deny foreign and signed-out access",
            "private acceptance browser cleanup",
        )
        report = {"suite": "e2e", "requests": 10, "privateRequests": 10,
            "checks": [{"name": name, "status": "PASS"} for name in names]}
        PRIVATE.validate_private_report(report, "disk", suite="e2e")
        with self.assertRaisesRegex(RuntimeError, "wrong probe family"):
            PRIVATE.validate_private_report(report, "disk")
        for name in names:
            for status in (None, "NOT EXERCISED", "FAIL"):
                changed = json.loads(json.dumps(report))
                changed["checks"] = [check for check in changed["checks"] if check["name"] != name]
                if status: changed["checks"].append({"name": name, "status": status})
                with self.assertRaisesRegex(RuntimeError, "not successfully exercised"):
                    PRIVATE.validate_private_report(changed, "disk", suite="e2e")

    def test_explicit_live_helper_cannot_be_counted_as_optional(self):
        report = json.loads((ROOT / "src/adapters/mcp/fixtures/doop/private-acceptance-report.json").read_text())
        name = "live Openbot company connections and encrypted custody"
        report["checks"] = [check for check in report["checks"] if check["name"] != name]
        report["checks"].append({"name": name, "status": "NOT EXERCISED"})
        with self.assertRaisesRegex(RuntimeError, "not successfully exercised"):
            PRIVATE.validate_private_report(report, "bucket")
        with self.assertRaisesRegex(RuntimeError, "not successfully exercised"):
            PRIVATE.validate_private_report(report, "bucket", (name,))

    def test_private_families_cannot_substitute_each_other_or_omit_the_family(self):
        report = json.loads((ROOT / "src/adapters/mcp/fixtures/doop/private-acceptance-report.json").read_text())
        for family in (None, "contract", "e2e"):
            with self.assertRaisesRegex(RuntimeError, "wrong probe family"):
                PRIVATE.validate_private_report({**report, "suite": family}, "bucket")
        with self.assertRaisesRegex(RuntimeError, "not successfully exercised"):
            PRIVATE.validate_private_report({**report, "suite": "e2e"}, "bucket", suite="e2e")

    def test_actual_image_report_requires_each_exercised_private_path(self):
        report = json.loads((ROOT / "src/adapters/mcp/fixtures/doop/private-acceptance-report.json").read_text())
        PRIVATE.validate_private_report(report, "bucket")
        for name in (
            "live Doop bot-key rotation revokes both old keys",
            "live frozen Openbot publications enforce current keys, disable and disconnect",
            "Openbot disconnect retains remote company documents and private assets",
            "same-schema owned database restore retains private assets and revoked keys",
            "private image byte and metadata isolation",
            "actual MCP dense previews and full downloads retain private current-session authority",
            "actual browser PDF export control downloads selected private pages with embedded fonts",
            "actual foreign and signed-out human browsers cannot open the private canvas",
            "an explicitly invited human opens and edits a private canvas in the normal browser",
            "human membership removal clears the warm editor and denies browser revalidation",
            "actual HTTPS held upload denies publication after its parent deletion completes",
            "actual private bucket origin rejects unsigned known files",
        ):
            with self.subTest(name=name):
                changed = {**report, "checks": [check for check in report["checks"] if check["name"] != name]}
                with self.assertRaisesRegex(RuntimeError, "not successfully exercised"):
                    PRIVATE.validate_private_report(changed, "bucket")
                changed["checks"].append({"name": name, "status": "NOT EXERCISED"})
                with self.assertRaisesRegex(RuntimeError, "not successfully exercised"):
                    PRIVATE.validate_private_report(changed, "bucket")

    def test_actual_image_report_cannot_hide_duplicate_or_out_of_budget_checks(self):
        report = json.loads((ROOT / "src/adapters/mcp/fixtures/doop/private-acceptance-report.json").read_text())
        changed = {**report, "checks": [*report["checks"], report["checks"][0]]}
        with self.assertRaisesRegex(RuntimeError, "duplicate"):
            PRIVATE.validate_private_report(changed, "bucket")
        for field, value in (("requests", 0), ("requests", 161), ("privateRequests", 97), ("privateRequests", True)):
            with self.subTest(field=field, value=value):
                with self.assertRaisesRegex(RuntimeError, "fixed budget"):
                    PRIVATE.validate_private_report({**report, field: value}, "bucket")


class DeploymentOwnershipTests(unittest.TestCase):
    def test_uncertain_local_process_keeps_all_deployment_dependencies(self):
        with tempfile.TemporaryDirectory() as directory:
            runtime = Path(directory)
            service = Mock(owner="fixture-owner")
            runner = Mock()
            runner.cleanup_run.side_effect = RuntimeError("probe quiescence unknown")
            with patch.object(PRIVATE, "stop_retained_process") as stop:
                failed = PRIVATE.close_private_run(runner, None, service, runtime, None)
                self.assertEqual([name for name, error in failed], ["private probe process"])
                stop.assert_not_called()
                service.close.assert_not_called()
            runner.cleanup_run.side_effect = None
            PRIVATE.private_json(runtime / "browser-process.json", {
                "version": 1, "phase": "pending", "pid": None, "owner": service.owner,
            })
            failed = PRIVATE.close_private_run(runner, None, service, runtime, None)
            self.assertEqual([name for name, error in failed], ["private browser process"])
            service.close.assert_not_called()
            self.assertEqual(PRIVATE.read_private_json(runtime / "browser-process.json")["phase"], "pending")

    def test_recovery_stops_only_the_recorded_positive_process_group(self):
        with tempfile.TemporaryDirectory() as temporary:
            runtime = Path(temporary)
            owner = "a" * 32
            child = subprocess.Popen([sys.executable, "-c", "import time;time.sleep(60)",
                "--user-data-dir=" + str(runtime / "ui-profile")], start_new_session=True)
            reaper = threading.Thread(target=child.wait)
            reaper.start()
            try:
                PRIVATE.private_json(runtime / "browser-process.json", {"version": 1, "phase": "created",
                    "pid": child.pid, "owner": owner})
                PRIVATE.stop_retained_process(runtime, "browser", owner, grace=0.5)
                reaper.join(timeout=2)
                self.assertFalse(reaper.is_alive())
                self.assertEqual(json.loads((runtime / "browser-process.json").read_text())["phase"], "removed")
            finally:
                if child.poll() is None:
                    child.kill()
                child.wait(timeout=2)
                reaper.join(timeout=2)

    def test_recovery_rejects_a_pid_from_another_invocation_without_stopping_it(self):
        for prefix_match in (False, True):
            with self.subTest(prefix_match=prefix_match), tempfile.TemporaryDirectory() as temporary:
                runtime = Path(temporary)
                marker = "--user-data-dir=" + str(runtime / "ui-profile") + "-other" if prefix_match else "other-owner"
                child = subprocess.Popen([sys.executable, "-c", "import time;time.sleep(60)", marker], start_new_session=True)
                try:
                    PRIVATE.private_json(runtime / "browser-process.json", {"version": 1, "phase": "created",
                        "pid": child.pid, "owner": "a" * 32})
                    with self.assertRaisesRegex(RuntimeError, "another invocation"):
                        PRIVATE.stop_retained_process(runtime, "browser", "a" * 32, grace=0.1)
                    self.assertIsNone(child.poll())
                    self.assertEqual(json.loads((runtime / "browser-process.json").read_text())["phase"], "created")
                finally:
                    os.killpg(child.pid, signal.SIGTERM)
                    child.wait(timeout=2)

    def test_unconfirmed_process_creation_is_retained_without_any_signal(self):
        with tempfile.TemporaryDirectory() as temporary:
            runtime = Path(temporary)
            record = {"version": 1, "phase": "pending", "pid": None, "owner": "a" * 32}
            PRIVATE.private_json(runtime / "probe-process.json", record)
            with patch.object(PRIVATE.os, "killpg") as send:
                with self.assertRaisesRegex(RuntimeError, "unconfirmed"):
                    PRIVATE.stop_retained_process(runtime, "probe", "a" * 32)
                send.assert_not_called()
            self.assertEqual(json.loads((runtime / "probe-process.json").read_text()), record)

    def test_recovery_refuses_an_active_runner_before_touching_docker_or_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            setup = Path(temporary)
            source = setup / "managed-source"
            source.mkdir()
            runtime = setup / ".private-run-abcdefgh"
            runtime.mkdir(mode=0o700)
            PRIVATE.private_json(runtime / "recovery.json", {"version": 2, "mode": "disk", "source": str(source),
                "storage_helper_sha256": "0" * 64, "owner": "a" * 32, "runner_pid": os.getpid()})
            output = setup / "evidence"
            with self.assertRaisesRegex(RuntimeError, "PID is active"):
                PRIVATE.recover(runtime, output, Mock())
            self.assertFalse(output.exists())
            self.assertTrue(runtime.exists())

    def test_pending_creation_retains_its_dependencies_until_late_resource_cleanup(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            control = DockerControlFixture()
            owned = OWNED.OwnedResources(root, control)
            owner = owned.state["owner"]
            owned.begin("network", "network", "fixture-network")
            self.assertEqual(json.loads(owned.path.read_text())["resources"]["network"]["phase"], "pending")
            control.objects["a" * 64] = {"kind": "network", "name": "fixture-network", "owner": owner}
            owned.created("network", "a" * 64)
            owned.begin("app", "container", "fixture-app")
            with self.assertRaisesRegex(RuntimeError, "already outstanding"):
                owned.begin("app", "container", "fixture-app")
            with self.assertRaisesRegex(RuntimeError, "unconfirmed"):
                owned.close()
            self.assertIn("a" * 64, control.objects)
            self.assertEqual(json.loads(owned.path.read_text())["resources"]["app"]["phase"], "pending")
            # The daemon's lost create response can still complete after timeout.
            control.objects["b" * 64] = {"kind": "container", "name": "fixture-app", "owner": owner}
            control.objects["c" * 64] = {"kind": "network", "name": "other-network", "owner": "0" * 32}
            recovered = OWNED.OwnedResources(root, control, recover=True)
            recovered.close()
            self.assertEqual(set(control.objects), {"c" * 64})
            self.assertTrue(all(x["phase"] == "removed" for x in recovered.state["resources"].values()))
            removals = [x[-1] for x in control.calls if x[0] == "rm" or x[1] == "rm"]
            self.assertEqual(removals, ["b" * 64, "a" * 64])

    def test_engine_identity_and_exact_resource_receipt_precede_destructive_cleanup(self):
        for failure in ("engine", "owner", "rename", "receipt"):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as temporary:
                control = DockerControlFixture()
                owned = OWNED.OwnedResources(Path(temporary), control)
                owner = owned.state["owner"]
                owned.begin("app", "container", "fixture-app")
                control.objects["a" * 64] = {"kind": "container", "name": "fixture-app", "owner": owner}
                owned.created("app", "a" * 64)
                if failure == "engine":
                    control.engine = "different-engine"
                elif failure == "owner":
                    control.inspected_owner = "0" * 32
                elif failure == "rename":
                    control.objects["a" * 64]["name"] = "renamed-app"
                else:
                    del control.objects["a" * 64]
                    control.objects["b" * 64] = {"kind": "container", "name": "fixture-app", "owner": owner}
                with self.assertRaises(RuntimeError):
                    owned.close()
                self.assertFalse(any(x[0] in ("stop", "rm") or x[1] == "rm" for x in control.calls))
                self.assertEqual(json.loads(owned.path.read_text())["resources"]["app"]["phase"], "created")

    def test_confirmed_removal_allows_one_new_generation_and_keeps_fixed_resource_bound(self):
        with tempfile.TemporaryDirectory() as temporary:
            control = DockerControlFixture()
            owned = OWNED.OwnedResources(Path(temporary), control)
            for generation in ("a", "b"):
                owned.begin("app", "container", "fixture-app")
                control.objects[generation * 64] = {"kind": "container", "name": "fixture-app", "owner": owned.state["owner"]}
                owned.created("app", generation * 64)
                owned.remove("app")
            for index in range(15):
                owned.begin("resource-" + str(index), "volume", "fixture-volume-" + str(index))
            with self.assertRaisesRegex(ValueError, "exceeds its bound"):
                owned.begin("overflow", "volume", "fixture-overflow")
            self.assertEqual(len(owned.state["resources"]), 16)


class ImageAndPackageTests(unittest.TestCase):
    def test_package_requires_exact_pinned_identity_and_patch_bytes(self):
        identity = json.loads((RUNNER.PACKAGE / "source.json").read_text())
        manifest = {key: value for key, value in identity.items() if key not in ("fingerprint", "patch_sha256")}
        self.assertEqual(RUNNER.package_identity(manifest), identity)
        for field in ("revision", "sdk", "bun"):
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, "differs"):
                RUNNER.package_identity({**manifest, field: "different"})
        self.assertEqual(RUNNER.package_identity({**manifest, "lock_sha256": "a" * 64}), identity)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "source.json").write_text(json.dumps(identity))
            (root / "managed.patch").write_text("tampered")
            with patch.object(RUNNER, "PACKAGE", root), self.assertRaisesRegex(ValueError, "digest differs"):
                RUNNER.package_identity(manifest)

    def managed_fixture(self, root):
        source = root / "source"
        source.mkdir()
        subprocess.run(["git", "init", "--quiet", str(source)], check=True)
        (source / ".gitignore").write_text("node_modules/\n.env\n")
        (source / "bun.lock").write_text("upstream lock\n")
        subprocess.run(["git", "-C", str(source), "add", "."], check=True)
        subprocess.run(["git", "-C", str(source), "-c", "user.name=Test", "-c", "user.email=test@example.invalid",
            "commit", "--quiet", "-m", "fixture"], check=True)
        revision = RUNNER.source_fingerprint(source)[0]
        sdk = source / "node_modules/@modelcontextprotocol/sdk"
        sdk.mkdir(parents=True)
        (sdk / "package.json").write_text(json.dumps({"version": "fixture-sdk"}))
        manifest = {"version": 1, "revision": revision, "source": RUNNER.SOURCE,
            "bun": RUNNER.BUN_VERSION, "lock_sha256": RUNNER.digest(source / "bun.lock"), "sdk": "fixture-sdk"}
        (root / "ready.json").write_text(json.dumps(manifest))
        package = root / "package"
        package.mkdir()
        (package / "managed.patch").write_text("diff --git a/bun.lock b/bun.lock\n--- a/bun.lock\n+++ b/bun.lock\n@@ -1 +1 @@\n-upstream lock\n+managed lock\n")
        reference = root / "reference"
        subprocess.run(["git", "clone", "--quiet", str(source), str(reference)], check=True)
        (reference / "bun.lock").write_text("managed lock\n")
        identity = {**manifest, "lock_sha256": RUNNER.digest(reference / "bun.lock"),
            "patch_sha256": RUNNER.digest(package / "managed.patch"),
            "fingerprint": RUNNER.source_fingerprint(reference)[1]}
        (package / "source.json").write_text(json.dumps(identity))
        bun = root / "tools/node_modules/.bin/bun"
        bun.parent.mkdir(parents=True)
        bun.write_text("#!/bin/sh\nprintf '" + RUNNER.BUN_VERSION + "\\n'\n")
        bun.chmod(0o755)
        return source, package, revision, identity, bun

    def test_managed_changed_lock_installs_own_cache_and_reuses_it_without_installing(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source, package, revision, identity, bun = self.managed_fixture(root)
            original = RUNNER.execute
            installs = []
            def execute(command, **kwargs):
                if command[0] != str(bun):
                    return original(command, **kwargs)
                self.assertEqual(command[1:], ["install", "--frozen-lockfile", "--ignore-scripts"])
                self.assertEqual(kwargs["cwd"], root / "managed-source")
                self.assertNotIn("NODE_OPTIONS", kwargs["env"])
                self.assertNotIn("DATABASE_URL", kwargs["env"])
                installs.append(command)
                sdk = kwargs["cwd"] / "node_modules/@modelcontextprotocol/sdk"
                sdk.mkdir(parents=True)
                (sdk / "package.json").write_text(json.dumps({"version": "fixture-sdk"}))
            with patch.object(RUNNER, "PACKAGE", package), patch.object(RUNNER, "REVISION", revision), \
                 patch.object(RUNNER, "execute", side_effect=execute):
                target = RUNNER.prepare_managed(root)
                self.assertEqual(len(installs), 1)
                self.assertFalse((target / "node_modules").is_symlink())
                self.assertEqual((source / "bun.lock").read_text(), "upstream lock\n")
                self.assertEqual((target / "bun.lock").read_text(), "managed lock\n")
                self.assertEqual(json.loads((root / "managed.json").read_text()), identity)
                with patch.object(RUNNER, "execute", side_effect=AssertionError("repeat installs")):
                    self.assertEqual(RUNNER.prepare_managed(root), target)
                    selected, manifest, actual = RUNNER.select_source(root, None, None, True)
                self.assertEqual(selected, target)
                self.assertEqual(manifest["lock_sha256"], identity["lock_sha256"])
                self.assertEqual(actual["fingerprint"], identity["fingerprint"])
                self.assertEqual(actual["kind"], "packaged managed source")
                shutil.rmtree(target / "node_modules")
                (target / "node_modules").symlink_to(source / "node_modules", target_is_directory=True)
                with self.assertRaisesRegex(ValueError, "separate installed directory"):
                    RUNNER.select_source(root, None, None, True)

    def test_managed_lock_tamper_fails_before_install_and_partial_cache_is_never_adopted(self):
        for failure in ("lock", "install"):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                _source, package, revision, identity, bun = self.managed_fixture(root)
                if failure == "lock":
                    identity["lock_sha256"] = "0" * 64
                    (package / "source.json").write_text(json.dumps(identity))
                original = RUNNER.execute
                installs = []
                def execute(command, **kwargs):
                    if command[0] != str(bun):
                        return original(command, **kwargs)
                    installs.append(command)
                    raise subprocess.CalledProcessError(7, ["fixture frozen install"])
                with patch.object(RUNNER, "PACKAGE", package), patch.object(RUNNER, "REVISION", revision), \
                     patch.object(RUNNER, "execute", side_effect=execute):
                    error = ValueError if failure == "lock" else subprocess.CalledProcessError
                    with self.assertRaises(error):
                        RUNNER.prepare_managed(root)
                    self.assertEqual(len(installs), 0 if failure == "lock" else 1)
                    self.assertFalse((root / "managed.json").exists())
                    with self.assertRaisesRegex(ValueError, "incomplete"):
                        RUNNER.prepare_managed(root)
                    with self.assertRaisesRegex(ValueError, "incomplete"):
                        RUNNER.select_source(root, None, None, True)

    def test_explicit_package_reconstructs_changed_lock_binary_empty_files_and_modes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source, package, revision, _identity, _bun = self.managed_fixture(root)
            actual = root / "reference"
            sdk = actual / "node_modules/@modelcontextprotocol/sdk"
            sdk.mkdir(parents=True)
            (sdk / "package.json").write_text(json.dumps({"version": "fixture-sdk"}))
            (actual / "new.bin").write_bytes(bytes(range(256)))
            (actual / "empty.txt").touch()
            (actual / ".env").write_text("never package this sentinel")
            (actual / ".gitignore").chmod(0o755)
            expected = RUNNER.source_fingerprint(actual)[1]
            with patch.object(RUNNER, "PACKAGE", package), patch.object(RUNNER, "REVISION", revision):
                self.assertEqual(RUNNER.package_source(root, actual, expected), 0)
                manifest = json.loads((root / "ready.json").read_text())
                identity = RUNNER.package_identity(manifest)
                self.assertEqual(identity["fingerprint"], expected)
                self.assertNotEqual(identity["lock_sha256"], manifest["lock_sha256"])
                self.assertNotIn(b"never package this sentinel", (package / "managed.patch").read_bytes())
                rebuilt = root / "rebuilt"
                subprocess.run(["git", "clone", "--quiet", str(source), str(rebuilt)], check=True)
                subprocess.run(["git", "apply", "--binary", "--whitespace=error", str(package / "managed.patch")],
                    cwd=rebuilt, check=True)
                self.assertEqual(RUNNER.source_fingerprint(rebuilt)[1], expected)
                self.assertEqual((rebuilt / "new.bin").read_bytes(), bytes(range(256)))
                self.assertEqual((rebuilt / "empty.txt").read_bytes(), b"")
                old = (package / "managed.patch").read_bytes()
                (actual / "new.bin").write_bytes(b"changed after approval")
                with self.assertRaisesRegex(ValueError, "fingerprint"):
                    RUNNER.package_source(root, actual, expected)
                self.assertEqual((package / "managed.patch").read_bytes(), old)

    def test_archive_excludes_ignored_secrets_dependencies_and_normalizes_metadata(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source"
            source.mkdir()
            subprocess.run(["git", "init", "--quiet", str(source)], check=True)
            (source / ".gitignore").write_text(".env\nnode_modules/\n")
            (source / "source.ts").write_text("reviewed")
            (source / ".env").write_text("secret sentinel")
            subprocess.run(["git", "-C", str(source), "add", "."], check=True)
            subprocess.run(["git", "-C", str(source), "-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "--quiet", "-m", "fixture"], check=True)
            expected = RUNNER.source_fingerprint(source)[1]
            archive = root / "context.tar"
            RUNNER.source_archive(source, archive, expected)
            with tarfile.open(archive) as content:
                self.assertEqual(content.getnames(), [".gitignore", "source.ts"])
                self.assertTrue(all(item.uid == item.gid == item.mtime == 0 for item in content.getmembers()))
            (source / "source.ts").write_text("changed")
            with self.assertRaisesRegex(ValueError, "fingerprint differs"):
                RUNNER.source_archive(source, archive, expected)

    def test_image_mismatch_fails_before_any_docker_execution(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "image.tar"
            archive.write_bytes(b"original archive")
            identity = {"source_identity": {"fingerprint": "a" * 64}, "user": "node", "image_id": "sha256:" + "b" * 64, "archive_sha256": hashlib.sha256(archive.read_bytes()).hexdigest()}
            path = root / "image.json"
            path.write_text(json.dumps(identity))
            with patch.object(RUNNER, "execute") as execute:
                with self.assertRaisesRegex(ValueError, "source identity differs"):
                    RUNNER.image_identity(path, {"fingerprint": "c" * 64}, root)
                archive.write_bytes(b"tampered archive")
                with self.assertRaisesRegex(ValueError, "archive digest differs"):
                    RUNNER.image_identity(path, identity["source_identity"], root)
                execute.assert_not_called()

    def test_relay_preserves_bytes_and_closes_only_its_own_tunnels(self):
        class Echo(socketserver.BaseRequestHandler):
            def handle(self):
                while data := self.request.recv(65536):
                    self.request.sendall(data)
        backend = socketserver.ThreadingTCPServer(("127.0.0.1", 0), Echo)
        backend.daemon_threads = True
        server = threading.Thread(target=backend.serve_forever)
        server.start()
        listener = socket.socket()
        listener.bind(("127.0.0.1", 0))
        listener.listen()
        relay = IMAGE.Relay(listener)
        relay.target = backend.server_address
        relay.start()
        try:
            with socket.create_connection(listener.getsockname(), timeout=2) as client:
                client.sendall(b"HTTP/1.1\r\n\x00websocket\xff")
                self.assertEqual(client.recv(65536), b"HTTP/1.1\r\n\x00websocket\xff")
                relay.close()
                self.assertEqual(client.recv(1), b"")
            with socket.create_connection(backend.server_address, timeout=2) as unrelated:
                unrelated.sendall(b"other owner")
                self.assertEqual(unrelated.recv(20), b"other owner")
        finally:
            relay.close()
            listener.close()
            backend.shutdown()
            backend.server_close()
            server.join(timeout=2)

    def test_tls_ingress_validates_local_ca_and_hostname_and_preserves_actual_bytes(self):
        class Echo(socketserver.BaseRequestHandler):
            def handle(self):
                while data := self.request.recv(65536):
                    self.request.sendall(data)
        with tempfile.TemporaryDirectory() as temporary:
            tls = TLS.LocalTLS(Path(temporary) / "tls", RUNNER.execute)
            self.assertEqual(len(tls.spki), 44)
            self.assertEqual((Path(temporary) / "tls/server-key.pem").stat().st_mode & 0o777, 0o600)
            backend = socketserver.ThreadingTCPServer(("127.0.0.1", 0), Echo)
            backend.daemon_threads = True
            server = threading.Thread(target=backend.serve_forever)
            server.start()
            listener = socket.socket()
            listener.bind(("127.0.0.1", 0))
            listener.listen()
            relay = IMAGE.Relay(listener, tls.server)
            relay.target = backend.server_address
            relay.start()
            try:
                for context, hostname in [(ssl.create_default_context(), "localhost"), (tls.client, "foreign.invalid")]:
                    with socket.create_connection(listener.getsockname(), timeout=2) as raw:
                        with self.assertRaises(ssl.SSLCertVerificationError):
                            context.wrap_socket(raw, server_hostname=hostname)
                with socket.create_connection(listener.getsockname(), timeout=2) as raw:
                    with tls.client.wrap_socket(raw, server_hostname="localhost") as secure:
                        data = b"HTTP/1.1\r\n\x00websocket\xff"
                        secure.sendall(data)
                        self.assertEqual(secure.recv(65536), data)
            finally:
                relay.close()
                listener.close()
                backend.shutdown()
                backend.server_close()
                server.join(timeout=2)

    def test_tls_handshakes_keep_their_sixteen_slots_until_actual_shutdown(self):
        with tempfile.TemporaryDirectory() as temporary:
            tls = TLS.LocalTLS(Path(temporary) / "tls", RUNNER.execute)
            listener = socket.socket()
            listener.bind(("127.0.0.1", 0))
            listener.listen()
            relay = IMAGE.Relay(listener, tls.server)
            relay.target = ("127.0.0.1", 1)  # No backend connects before a handshake.
            relay.start()
            clients = []
            try:
                clients = [socket.create_connection(listener.getsockname(), timeout=2) for _ in range(16)]
                deadline = time.monotonic() + 2
                while time.monotonic() < deadline:
                    with relay.lock:
                        if len(relay.streams) == 16:
                            break
                    time.sleep(0.005)
                with relay.lock:
                    self.assertEqual(len(relay.streams), 16)
                with socket.create_connection(listener.getsockname(), timeout=2) as overflow:
                    self.assertEqual(overflow.recv(1), b"")
                started = time.monotonic()
                relay.close()
                self.assertLess(time.monotonic() - started, 2)
                with relay.lock:
                    self.assertEqual(len(relay.workers), 0)
                    self.assertEqual(len(relay.streams), 0)
            finally:
                relay.close()
                for client in clients:
                    client.close()
                listener.close()

    def test_image_cleanup_attempts_volume_and_relay_after_container_failure(self):
        listener = socket.socket()
        try:
            service = IMAGE.ImageService(Path("source"), Path("runtime"), {}, listener, Path("output"), {}, Mock())
            service.stop = Mock(side_effect=RuntimeError("container stop failed"))
            service.owned = Mock(return_value=[service.volume])
            service.command = Mock()
            service.relay = Mock()
            with self.assertRaisesRegex(RuntimeError, "container stop failed"):
                service.close()
            service.command.assert_called_once_with("volume", "rm", service.volume)
            service.relay.close.assert_called_once()
        finally:
            listener.close()


class DatabaseRestoreTests(unittest.TestCase):
    def service(self, root):
        control = DockerControlFixture()
        owned = OWNED.OwnedResources(Path(root), control)
        owner = owned.state["owner"]
        owned.begin("postgres", "container", "fixture-postgres")
        control.objects["a" * 64] = {"kind": "container", "name": "fixture-postgres", "owner": owner}
        owned.created("postgres", "a" * 64)
        events = []
        service = SimpleNamespace(resources=owned, owner=owner, pg_name="fixture-postgres",
            mode="bucket", image={"image_id": "sha256:" + "b" * 64}, restore_started=False,
            stop=Mock(side_effect=lambda: events.append("stop")),
            execute=Mock(side_effect=lambda *a, **k: events.append(a[0][3])),
            raw_command=Mock(side_effect=lambda *a: "1024 600" if "stat" in a else "c" * 64 + "  backup"))
        return service, control, events

    def test_owned_database_restore_quiesces_before_single_use_container_id_commands(self):
        with tempfile.TemporaryDirectory() as root:
            service, _, events = self.service(root)
            record = PRIVATE.RESTORE.restore(service)
            self.assertEqual(events, ["stop", "sh", "dropdb", "createdb", "pg_restore", "rm"])
            self.assertEqual(record["postgres_container"], "a" * 64)
            self.assertEqual(record["private_asset_store"], "retained")
            self.assertEqual(record["backup_permissions"], "0600")
            for args, kwargs in service.execute.call_args_list:
                self.assertEqual(args[0][:3], ["docker", "exec", "a" * 64])
                self.assertEqual(kwargs["stdout"], subprocess.DEVNULL)
                self.assertEqual(kwargs["stderr"], subprocess.DEVNULL)
                self.assertNotIn("--force", args[0])
            self.assertIn("--exit-on-error", service.execute.call_args_list[3].args[0])
            with self.assertRaisesRegex(RuntimeError, "single-use"):
                PRIVATE.RESTORE.restore(service)
            self.assertEqual(service.stop.call_count, 1)

    def test_wrong_engine_label_owner_name_and_receipt_refuse_before_quiescence_or_data_tools(self):
        for failure in ("engine", "label", "owner", "rename", "receipt"):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as root:
                service, control, _ = self.service(root)
                if failure == "engine":
                    control.engine = "other-engine"
                elif failure == "label":
                    service.resources.state["resources"]["postgres"]["label"] = "foreign-label"
                elif failure == "owner":
                    control.inspected_owner = "0" * 32
                elif failure == "rename":
                    control.objects["a" * 64]["name"] = "foreign-postgres"
                else:
                    service.resources.state["resources"]["postgres"]["id"] = "b" * 64
                with self.assertRaises(RuntimeError):
                    PRIVATE.RESTORE.restore(service)
                service.stop.assert_not_called()
                service.execute.assert_not_called()

    def test_ownership_changed_during_quiescence_refuses_before_dump(self):
        with tempfile.TemporaryDirectory() as root:
            service, control, _ = self.service(root)
            service.stop.side_effect = lambda: setattr(control, "engine", "other-engine")
            with self.assertRaises(RuntimeError):
                PRIVATE.RESTORE.restore(service)
            service.execute.assert_not_called()

    def test_oversized_empty_or_exposed_dump_refuses_before_reset(self):
        for metadata in ("0 600", "16777217 600", "1024 644", "invalid"):
            with self.subTest(metadata=metadata), tempfile.TemporaryDirectory() as root:
                service, _, events = self.service(root)
                service.raw_command.return_value = metadata
                service.raw_command.side_effect = None
                with self.assertRaises((ValueError, RuntimeError)):
                    PRIVATE.RESTORE.restore(service)
                self.assertEqual(events, ["stop", "sh"])

    def test_database_tool_failure_is_secret_free_and_never_retried(self):
        with tempfile.TemporaryDirectory() as root:
            service, _, _ = self.service(root)
            service.execute.side_effect = subprocess.CalledProcessError(1, ["SQL secret"], stderr="COPY private-data")
            with self.assertRaisesRegex(RuntimeError, "dump failed") as caught:
                PRIVATE.RESTORE.restore(service)
            self.assertNotIn("secret", str(caught.exception))
            self.assertNotIn("private-data", str(caught.exception))
            self.assertEqual(service.execute.call_count, 1)
            self.assertEqual(service.last_control_error, "owned-database-restore.dump.failed")
            self.assertTrue(service.restore_started)

    def test_failed_restore_callback_never_acknowledges_readiness(self):
        with tempfile.TemporaryDirectory() as root:
            receipt = Path(root) / "acknowledgements.json"
            code = """
import json,select,sys
from pathlib import Path
controls=[{"kind":"configure","bindings":[{"workspaceId":"workspace","botUserId":"bot"}]},{"kind":"restore"}]
replies=[]
for control in controls:
    print('DOOP_TEST_CONTROL '+json.dumps(control),flush=True)
    replies.append(sys.stdin.readline().strip() if select.select([sys.stdin],[],[],1)[0] else 'timeout')
Path(sys.argv[1]).write_text(json.dumps(replies))
"""
            process = subprocess.Popen([sys.executable, "-c", code, str(receipt)],
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, start_new_session=True)
            restore = Mock(side_effect=RuntimeError("fixed restore stage failed"))
            try:
                with self.assertRaisesRegex(RuntimeError, "fixed restore stage failed"):
                    RUNNER.drive_probe(process, lambda _: None, restore)
                process.wait(timeout=3)
                restore.assert_called_once_with()
                self.assertEqual(json.loads(receipt.read_text()), ["ready", "timeout"])
            finally:
                RUNNER.stop(process, grace=0.1)
                process.stdin.close()
                process.stdout.close()

    def test_restore_control_requires_configuration_and_callback_and_cannot_repeat_or_follow_restart(self):
        configure = {"kind": "configure", "bindings": [{"workspaceId": "workspace", "botUserId": "bot"}]}
        restore = {"kind": "restore"}
        for controls, callback, valid in (
            ([configure, restore, {"kind": "restart"}], True, True),
            ([restore], True, False), ([configure, restore], False, False),
            ([configure, restore, restore], True, False),
            ([configure, {"kind": "restart"}, restore], True, False),
            ([configure, {"kind": "restore", "database": "foreign"}], True, False),
        ):
            with self.subTest(controls=controls, callback=callback):
                code = "import json,sys; [(print('DOOP_TEST_CONTROL '+json.dumps(c),flush=True),sys.stdin.readline()) for c in json.loads(sys.argv[1])]"
                process = subprocess.Popen([sys.executable, "-c", code, json.dumps(controls)],
                    stdin=subprocess.PIPE, stdout=subprocess.PIPE, start_new_session=True)
                calls = []
                try:
                    if valid:
                        self.assertEqual(RUNNER.drive_probe(process, calls.append, lambda: calls.append("restore")), 0)
                        self.assertEqual(calls, [configure["bindings"], "restore", None])
                    else:
                        with self.assertRaisesRegex(ValueError, "Unexpected"):
                            RUNNER.drive_probe(process, calls.append, (lambda: calls.append("restore")) if callback else None)
                finally:
                    RUNNER.stop(process, grace=0.1)
                    process.stdin.close()
                    process.stdout.close()


class RunnerTests(unittest.TestCase):
    def test_cli_reports_primary_and_cleanup_failures_without_cleanup_payloads(self):
        error = ValueError("primary failure")
        error.__cause__ = RUNNER.CleanupFailure([("probe process", RuntimeError("private cleanup payload"))])
        output = io.StringIO()
        with patch.object(sys, "argv", ["doop-test.py", "run", "--directory", "/unused", "--output", "/unused-output"]), \
             patch.object(RUNNER.shutil, "which", return_value="/bin/true"), \
             patch.object(RUNNER, "run", side_effect=error), redirect_stderr(output):
            self.assertEqual(RUNNER.main(), 1)
        self.assertIn("primary failure", output.getvalue())
        self.assertIn("probe process (RuntimeError)", output.getvalue())
        self.assertNotIn("private cleanup payload", output.getvalue())

    def test_outer_cleanup_finalizes_all_owners_and_preserves_primary_failure(self):
        real_stop = RUNNER.stop
        for image_mode in (False, True):
            for failed in ("stop", "stdin", "stdout", "interrupt"):
                for primary_failure in (False, True):
                    with self.subTest(image=image_mode, failed=failed, primary=primary_failure), tempfile.TemporaryDirectory() as temporary:
                        root = Path(temporary)
                        output = root / "output"
                        service = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"], start_new_session=True)
                        other = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"], start_new_session=True)
                        probe = Mock(stdin=Mock(), stdout=Mock())
                        container = Mock()
                        container.close.side_effect = lambda: real_stop(service, grace=0.1)
                        if failed in ("stdin", "stdout"):
                            getattr(probe, failed).close.side_effect = RuntimeError("pipe close failed")
                        original = ValueError("primary probe failure")
                        def drive(*args):
                            if primary_failure:
                                raise original
                            (output / "report.json").write_text(json.dumps({"checks": []}))
                            return 0
                        def stop(process, *args, **kwargs):
                            if process is probe:
                                if failed == "interrupt":
                                    raise KeyboardInterrupt("cleanup interrupted")
                                if failed == "stop":
                                    raise RuntimeError("probe stop failed")
                                return
                            return real_stop(process, *args, **kwargs)
                        try:
                            with patch.object(RUNNER, "select_source", return_value=(root, {}, {})), \
                                 patch.object(RUNNER, "image_identity", return_value={key: "fixture" for key in ("image_id", "manifest_digest", "archive_sha256", "user")}), \
                                 patch.object(RUNNER, "image_service", return_value=container), \
                                 patch.object(RUNNER, "start_server", return_value=service), \
                                 patch.object(RUNNER.subprocess, "Popen", return_value=probe), \
                                 patch.object(RUNNER, "drive_probe", side_effect=drive), \
                                 patch.object(RUNNER, "stop", side_effect=stop):
                                expected = ValueError if primary_failure else RUNNER.CleanupFailure
                                with self.assertRaises(expected) as raised:
                                    RUNNER.run(root, output, "all", image=root / "image.json" if image_mode else None)
                            if primary_failure:
                                self.assertIs(raised.exception, original)
                                self.assertIsInstance(original.__cause__, RUNNER.CleanupFailure)
                            self.assertIsNotNone(service.poll(), "independent service leaked")
                            self.assertIsNone(other.poll(), "cleanup crossed ownership")
                            probe.stdin.close.assert_called_once()
                            probe.stdout.close.assert_called_once()
                            if image_mode:
                                container.close.assert_called_once()
                        finally:
                            real_stop(service, grace=0.1)
                            real_stop(other, grace=0.1)

    @unittest.skipUnless(shutil.which("node") and os.name == "posix", "requires Node and POSIX signals")
    def test_sigint_during_initial_and_restart_readiness_cleans_owned_children(self):
        for stage in (0, 1):
            with self.subTest(stage=stage), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                source = root / "source"
                (source / "server").mkdir(parents=True)
                (source / "node_modules/tsx/dist").mkdir(parents=True)
                (source / "node_modules/tsx/dist/loader.mjs").write_text("")
                entry = source / "server/owned.mjs"
                entry.write_text("setInterval(() => {}, 1000);\n")
                (source / "server/index.ts").symlink_to(entry)
                owned = []
                other = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"], start_new_session=True)
                def readiness(process, origin):
                    owned.append(process)
                    time.sleep(0.15)
                    self.assertIsNone(process.poll(), "real child must be alive before interruption")
                    if len(owned) == stage + 1:
                        os.kill(os.getpid(), signal.SIGINT)
                def drive(probe, restart):
                    restart([{"workspaceId": "space", "botUserId": "bot"}])
                    self.fail("interrupted restart must propagate")
                try:
                    with patch.object(RUNNER, "validate", return_value=(source, {})), \
                         patch.object(RUNNER, "wait_ready", side_effect=readiness), \
                         patch.object(RUNNER, "drive_probe", side_effect=drive):
                        with self.assertRaises(KeyboardInterrupt):
                            RUNNER.run(root / "setup", root / "output", "all")
                    self.assertEqual(len(owned), stage + 1)
                    for process in owned:
                        self.assertIsNotNone(process.poll(), "startup/restart child leaked after SIGINT")
                    self.assertIsNone(other.poll(), "cleanup crossed process-group ownership")
                finally:
                    for process in owned:
                        RUNNER.stop(process, grace=0.1)
                    RUNNER.stop(other, grace=0.1)

    def test_source_identity_covers_content_modes_deletions_and_rejects_symlinks(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source.ts"
            source.write_text("first")
            original = RUNNER.fingerprint_paths(root, "base", [b"source.ts"])
            source.write_text("second")
            self.assertNotEqual(original, RUNNER.fingerprint_paths(root, "base", [b"source.ts"]))
            source.write_text("first")
            source.chmod(0o700)
            self.assertNotEqual(original, RUNNER.fingerprint_paths(root, "base", [b"source.ts"]))
            source.unlink()
            self.assertNotEqual(original, RUNNER.fingerprint_paths(root, "base", [b"source.ts"]))
            source.symlink_to(root / "missing")
            with self.assertRaisesRegex(ValueError, "symlinks"):
                RUNNER.fingerprint_paths(root, "base", [b"source.ts"])

    def test_control_protocol_permits_configure_then_restart_and_rejects_repetition(self):
        binding = [{"workspaceId": "workspace", "botUserId": "bot"}]
        for controls, expected in [
            ([{"kind": "configure", "bindings": binding}, {"kind": "restart"}], [binding, None]),
            ([{"kind": "restart"}], None),
            ([{"kind": "configure", "bindings": binding}] * 2, None),
        ]:
            code = (
                "import json,sys; controls=json.loads(sys.argv[1]); "
                "[(print('DOOP_TEST_CONTROL '+json.dumps(c),flush=True), "
                "sys.stdin.readline()) for c in controls]"
            )
            process = subprocess.Popen([sys.executable, "-c", code, json.dumps(controls)],
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, start_new_session=True)
            calls = []
            try:
                if expected is None:
                    with self.assertRaisesRegex(ValueError, "Unexpected"):
                        RUNNER.drive_probe(process, calls.append)
                else:
                    self.assertEqual(RUNNER.drive_probe(process, calls.append), 0)
                    self.assertEqual(calls, expected)
            finally:
                RUNNER.stop(process, grace=0.1)
                process.stdin.close()
                process.stdout.close()

    def test_source_and_control_budgets_fail_before_processing_unbounded_input(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with self.assertRaisesRegex(ValueError, "file bound"):
                RUNNER.fingerprint_paths(root, "base", [b"missing"] * 8193)
            large = root / "large"
            with large.open("wb") as stream:
                stream.truncate(32 * 1024 * 1024 + 1)
            with self.assertRaisesRegex(ValueError, "oversized"):
                RUNNER.fingerprint_paths(root, "base", [b"large"])
        process = subprocess.Popen([sys.executable, "-c", "print('x'*65537,flush=True)"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, start_new_session=True)
        try:
            with self.assertRaisesRegex(ValueError, "exceeds its bound"):
                RUNNER.drive_probe(process, lambda _: self.fail("unexpected restart"))
        finally:
            RUNNER.stop(process, grace=0.1)
            process.stdin.close()
            process.stdout.close()

    def test_provider_secrets_and_database_configuration_are_not_inherited(self):
        previous = dict(os.environ)
        try:
            os.environ.update({
                "OPENAI_API_KEY": "sentinel-provider", "DATABASE_URL": "sentinel-database",
                "NODE_OPTIONS": "--require=untrusted", "HTTPS_PROXY": "http://untrusted",
                "BETTER_AUTH_SECRET": "sentinel-parent", "STRIPE_SECRET_KEY": "sentinel-billing",
            })
            environment = RUNNER.clean_environment(Path("/owned/test/home"))
            encoded = json.dumps(environment)
            for sentinel in ("sentinel-provider", "sentinel-database", "untrusted", "sentinel-parent", "sentinel-billing"):
                self.assertNotIn(sentinel, encoded)
            self.assertNotIn("DATABASE_URL", environment)
            self.assertEqual(environment["RESIDENT_TASK_LIMIT"], "0")
        finally:
            os.environ.clear()
            os.environ.update(previous)

    def test_setup_refuses_to_overwrite_an_existing_directory(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            marker = root / "keep"
            marker.write_text("existing work")
            with self.assertRaises(FileExistsError):
                RUNNER.setup(root)
            self.assertEqual(marker.read_text(), "existing work")
            self.assertEqual(list(root.iterdir()), [marker])

    def test_changed_setup_identity_is_refused_before_running_source(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "ready.json").write_text(json.dumps({
                "version": 1, "revision": "unreviewed-source", "source": RUNNER.SOURCE,
                "bun": RUNNER.BUN_VERSION,
            }))
            with self.assertRaisesRegex(ValueError, "Setup identity differs"):
                RUNNER.validate(root)

    def test_setup_command_failure_preserves_its_exit_status(self):
        with self.assertRaises(subprocess.CalledProcessError) as failure:
            RUNNER.execute(["false"])
        self.assertEqual(failure.exception.returncode, 1)

    @unittest.skipUnless(shutil.which("node") and os.name == "posix", "requires Node and POSIX sockets")
    def test_reserved_socket_is_loopback_only_and_independent_between_two_servers(self):
        processes = []
        listeners = []
        try:
            for _ in range(2):
                listener = socket.socket()
                listener.bind(("127.0.0.1", 0))
                listener.listen()
                listeners.append(listener)
                environment = dict(os.environ, DOOP_TEST_SOCKET_FD=str(listener.fileno()), PORT=str(listener.getsockname()[1]))
                process = subprocess.Popen([
                    "node", "--import", str(RUNNER.SCRIPTS / "loopback.mjs"), "--input-type=module", "-e",
                    "import http from 'node:http'; const server=http.createServer((q,r)=>r.end('owned')); "
                    "server.listen(Number(process.env.PORT),()=>console.log(JSON.stringify(server.address())));",
                ], env=environment, pass_fds=(listener.fileno(),), start_new_session=True,
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
                processes.append(process)
            addresses = []
            for process in processes:
                self.assertTrue(select.select([process.stdout], [], [], 5)[0], "owned server readiness timeout")
                addresses.append(json.loads(process.stdout.readline()))
            self.assertNotEqual(addresses[0]["port"], addresses[1]["port"])
            opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
            for address in addresses:
                self.assertEqual(address["address"], "127.0.0.1")
                with opener.open("http://127.0.0.1:" + str(address["port"]), timeout=5) as response:
                    self.assertEqual(response.read(), b"owned")
            RUNNER.stop(processes[0])
            self.assertIsNotNone(processes[0].poll())
            self.assertIsNone(processes[1].poll(), "cleanup must not stop the other runner")
        finally:
            for process in processes:
                RUNNER.stop(process)
                process.communicate(timeout=5)
            for listener in listeners:
                listener.close()

    def test_readiness_fails_immediately_if_owned_process_exits(self):
        process = subprocess.Popen(["true"], start_new_session=True)
        process.wait(timeout=5)
        with self.assertRaisesRegex(RuntimeError, "exited before readiness"):
            RUNNER.wait_ready(process, "http://127.0.0.1:1")

    def test_cleanup_kills_a_term_ignoring_descendant_and_preserves_another_group(self):
        with tempfile.TemporaryDirectory() as temporary:
            ready = Path(temporary) / "child.json"
            child_code = (
                "import json,os,pathlib,signal,socket,sys,time; "
                "signal.signal(signal.SIGTERM,signal.SIG_IGN); "
                "server=socket.socket(); server.bind(('127.0.0.1',0)); server.listen(); "
                "ready=pathlib.Path(sys.argv[1]); staged=ready.with_suffix('.new'); "
                "staged.write_text(json.dumps({'port':server.getsockname()[1]})); staged.replace(ready); "
                "time.sleep(60)"
            )
            parent_code = (
                "import subprocess,sys,time; "
                "subprocess.Popen([sys.executable,'-c',sys.argv[1],sys.argv[2]]); time.sleep(60)"
            )
            leader = subprocess.Popen([sys.executable, "-c", parent_code, child_code, str(ready)], start_new_session=True)
            other = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"], start_new_session=True)
            try:
                deadline = time.monotonic() + 5
                while not ready.exists() and time.monotonic() < deadline:
                    time.sleep(0.02)
                self.assertTrue(ready.exists(), "owned child startup timeout")
                port = json.loads(ready.read_text())["port"]
                with socket.create_connection(("127.0.0.1", port), timeout=1):
                    pass
                RUNNER.stop(leader, grace=0.1)
                self.assertIsNotNone(leader.poll())
                self.assertIsNone(other.poll(), "cleanup crossed process-group ownership")
                deadline = time.monotonic() + 2
                closed = False
                while time.monotonic() < deadline:
                    try:
                        socket.create_connection(("127.0.0.1", port), timeout=0.1).close()
                    except OSError:
                        closed = True
                        break
                    time.sleep(0.02)
                self.assertTrue(closed, "owned descendant is still serving after its leader exited")
            finally:
                RUNNER.stop(leader, grace=0.1)
                RUNNER.stop(other, grace=0.1)


if __name__ == "__main__":
    unittest.main()
