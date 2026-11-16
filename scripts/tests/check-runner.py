#!/usr/bin/env python3
"""Check suite registration and runner failures without launching real gates or databases."""
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[2]
RUNNERS = ("scripts/check.sh", "scripts/check-support.sh", "scripts/check-doop.py",
           "scripts/test-network-isolation.sh", "services/memory/scripts/container-smoke.py")
MANUAL = {
    "run-list-query-plan.py": "On-demand EXPLAIN measurements against a selected migrated database.",
    "workflow-authoring-browser.cjs": "Manual journey against an explicitly selected running application.",
    "workflow-runs-browser.cjs": "Manual journey against an explicitly selected running application.",
}


def unregistered(root, runner_source):
    runner_source = re.sub(r"(?m)^\s*#.*$", "", runner_source)
    return sorted(str(path.relative_to(root)) for path in (root / "scripts/tests").iterdir()
                  if path.is_file() and path.suffix in (".py", ".sh", ".mjs", ".cjs")
                  and path.name not in MANUAL and str(path.relative_to(root)) not in runner_source)


def function(source, name):
    match = re.search(r"^" + re.escape(name) + r"\(\) \{\n.*?^\}", source, re.M | re.S)
    if not match:
        raise AssertionError("missing shell function " + name)
    return match.group()


class CheckRunnerTests(unittest.TestCase):
    def test_all_script_suites_are_registered(self):
        source = "\n".join((ROOT / path).read_text() for path in RUNNERS)
        self.assertEqual(unregistered(ROOT, source), [])
        main = (ROOT / "scripts/check.sh").read_text()
        self.assertIn("source scripts/check-support.sh", main)
        helpers = re.findall(r"^(check_\w+)\(\)", (ROOT / "scripts/check-support.sh").read_text(), re.M)
        for name in helpers:
            self.assertRegex(main, r'gate "[^"\n]+" ' + name + r"\b", name)
        for name, reason in MANUAL.items():
            self.assertTrue((ROOT / "scripts/tests" / name).is_file(), name)
            self.assertTrue(reason)
        ci = "\n".join(path.read_text() for path in (ROOT / ".github/workflows").glob("*.yml"))
        for path in set(re.findall(r"scripts/tests/[\w.-]+\.(?:py|sh|mjs|cjs)", ci)):
            self.assertIn(path, source, "CI suite omitted from check.sh: " + path)
            self.assertNotIn(Path(path).name, MANUAL, "required CI suite cannot be marked manual")

    def test_new_or_removed_registration_is_detected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "scripts/tests").mkdir(parents=True)
            (root / "scripts/tests/new-browser.mjs").write_text("// new suite\n")
            self.assertEqual(unregistered(root, ""), ["scripts/tests/new-browser.mjs"])
            self.assertEqual(unregistered(root, "# node --test scripts/tests/new-browser.mjs"),
                             ["scripts/tests/new-browser.mjs"])
            self.assertEqual(unregistered(root, "node --test scripts/tests/new-browser.mjs"), [])

    def test_service_and_budget_gates_remain_required(self):
        main = (ROOT / "scripts/check.sh").read_text()
        for command in ("scripts/stack-budget.sh --offline", "scripts/lease-budget.sh --offline",
                        "services/memory/scripts/extracted-build.sh", "scripts/memory-integration.sh",
                        "check_attribution_service", "check_attribution_browser", "check_attribution_image",
                        "check_memory_backup", "check_memory_image", "check_knowledge_image",
                        "check_plugin", "check_plugin_package", "python3 scripts/check-doop.py"):
            self.assertIn(command, main)
        self.assertNotIn("--test source_modules", main)
        self.assertIn("cargo test --locked --offline --all-targets\n", main)

    def test_attribution_is_required_when_present_or_declared_by_ci(self):
        main = (ROOT / "scripts/check.sh").read_text()
        block = re.search(r"^if \[\[ -d services/attribution .*?^fi$", main, re.M | re.S).group()
        for service, workflow in ((False, False), (True, False), (False, True), (True, True)):
            with self.subTest(service=service, workflow=workflow), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                if service:
                    (root / "services/attribution").mkdir(parents=True)
                if workflow:
                    (root / ".github/workflows").mkdir(parents=True)
                    (root / ".github/workflows/attribution-service.yml").write_text("name: attribution\n")
                result = subprocess.run(["bash", "-c", 'gate() { printf "%s\\n" "$1"; }\n' + block],
                                        cwd=root, capture_output=True, text=True, timeout=10)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(len(result.stdout.splitlines()), 3 if service or workflow else 0)

    def test_group_failure_stops_its_commands_and_records_failure(self):
        for fail_fast in (False, True):
            with self.subTest(fail_fast=fail_fast), tempfile.TemporaryDirectory() as temporary:
                script = "set -uo pipefail\nnames=(); results=(); failed=false\n"
                script += "FAIL_FAST=" + str(fail_fast).lower() + "\n"
                script += function((ROOT / "scripts/check.sh").read_text(), "gate")
                script += '''
broken() (set -e; false; touch "$PROBE_DIR/incorrectly-continued")
next() { touch "$PROBE_DIR/next"; }
gate "broken group" broken
gate "next gate" next
[[ "$failed" == true && "${results[0]}" == FAIL* ]]
'''
                result = subprocess.run(["bash", "-c", script], capture_output=True, text=True,
                                        env=dict(os.environ, PROBE_DIR=temporary), timeout=10)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertFalse((Path(temporary) / "incorrectly-continued").exists())
                self.assertEqual((Path(temporary) / "next").exists(), not fail_fast)

    def test_cleanup_only_drops_a_database_created_by_this_invocation(self):
        source = (ROOT / "scripts/check.sh").read_text()
        for succeeds in (False, True):
            with self.subTest(creation_succeeds=succeeds), tempfile.TemporaryDirectory() as temporary:
                script = "set -uo pipefail\n" + function(source, "create_check_database") + "\n"
                script += function(source, "cleanup_check")
                script += '''
check_database=openbot_check_fixture; check_admin_url=postgres://fixture/postgres
check_database_created=false; check_work="$PROBE_DIR/work"; mkdir "$check_work"
check_knowledge_image=fixture-knowledge; check_memory_image=fixture-memory; check_attribution_image=fixture-attribution
psql() {
  case "$*" in
    *CREATE*) [[ "$CREATION_SUCCEEDS" == true ]] ;;
    *DROP*) touch "$PROBE_DIR/dropped" ;;
  esac
}
docker() { :; }
create_check_database
cleanup_check
'''
                result = subprocess.run(["bash", "-c", script], capture_output=True, text=True, timeout=10,
                                        env=dict(os.environ, PROBE_DIR=temporary, CREATION_SUCCEEDS=str(succeeds).lower()))
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual((Path(temporary) / "dropped").exists(), succeeds)


class DoopRunnerTests(unittest.TestCase):
    def module(self):
        spec = importlib.util.spec_from_file_location("check_doop", ROOT / "scripts/check-doop.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module

    def test_service_gate_uses_clean_environment_and_stops_on_failure(self):
        module = self.module()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            runner = Mock()
            runner.select_source.return_value = (root / "service", {}, {"fingerprint": "reviewed"})
            runner.clean_environment.return_value = {"PATH": "/usr/bin"}
            calls = []

            def failing_run(*args, **kwargs):
                calls.append((args, kwargs))
                if "typecheck" in args:
                    raise subprocess.CalledProcessError(1, args)

            with patch.dict(os.environ, {"UNRELATED_PRIVATE_TOKEN": "must-not-reach-service"}), \
                    patch.object(module, "run", side_effect=failing_run):
                with self.assertRaises(subprocess.CalledProcessError):
                    module.service_gate(runner, root / "setup", root, "/chrome", "postgres://fixture/owned")
            self.assertEqual(len(calls), 3)
            for _, arguments in calls:
                self.assertNotIn("UNRELATED_PRIVATE_TOKEN", arguments["env"])
                self.assertEqual(arguments["env"]["DOOP_TEST_DATABASE_URL"], "postgres://fixture/owned")
            runner.reviewed_source.assert_not_called()

    def test_helper_requires_one_current_binary_and_all_acceptance_families(self):
        module = self.module()
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            binary = output / "compiled-helper"
            binary.write_bytes(b"fixture executable")
            message = {"reason": "compiler-artifact", "executable": str(binary),
                       "target": {"name": "open_bots", "kind": ["lib"]}, "profile": {"test": True}}

            def build(*args, **kwargs):
                kwargs["stdout"].write(json.dumps(message) + "\n")

            with patch.object(module, "run", side_effect=build), \
                    patch.object(module.subprocess, "check_output", return_value="unrelated: test\n"):
                with self.assertRaisesRegex(RuntimeError, "missing Doop acceptance"):
                    module.compile_helper(output)
            self.assertFalse((output / "libtest-sha256.txt").exists())
            # A different crate's test binary cannot stand in for the OpenBot helper.
            message["target"]["name"] = "another_crate"
            with patch.object(module, "run", side_effect=build):
                with self.assertRaisesRegex(RuntimeError, "exactly one current"):
                    module.compile_helper(output)


class MemoryImageTests(unittest.TestCase):
    def test_model_bounds_and_database_refusal(self):
        spec = importlib.util.spec_from_file_location("memory_image", ROOT / "services/memory/scripts/container-smoke.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        model = {"dimensions": 384, "sessions": 2, "oversized_input_refused": True, "peak_rss_bytes": 100}
        for changes, startup, accepted in [
            ({}, (1, "cannot connect to the memory database"), True),
            ({"dimensions": 1}, (1, "cannot connect to the memory database"), False),
            ({"peak_rss_bytes": 2560 * 1024 * 1024 + 1}, (1, "cannot connect to the memory database"), False),
            ({"oversized_input_refused": False}, (1, "cannot connect to the memory database"), False),
            ({}, (0, "cannot connect to the memory database"), False),
            ({}, (1, "unrelated startup failure"), False),
        ]:
            with self.subTest(changes=changes, startup=startup):
                def docker(*args, **kwargs):
                    output, code = "", 0
                    if args[:2] == ("image", "inspect"):
                        output = "memory:memory"
                    elif args[-1] == "model-check":
                        output = json.dumps({**model, **changes})
                    elif args[-1] == "serve":
                        code, output = startup
                    return subprocess.CompletedProcess(args, code, output, "")
                with patch.object(module, "docker", side_effect=docker):
                    if accepted:
                        module.check("fixture")
                    else:
                        with self.assertRaises(RuntimeError):
                            module.check("fixture")


if __name__ == "__main__":
    unittest.main()
