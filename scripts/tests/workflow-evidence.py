#!/usr/bin/env python3
"""Retention failures are exercised in temporary Git repositories."""

import contextlib
import io
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    "check_evidence", Path(__file__).resolve().parents[1] / "check-workflow-evidence.py"
)
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)


class EvidenceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name).resolve()
        self.run_git("init", "-q")
        self.records = self.repo / "plan/topic/phases/p01/docs"
        self.logs = self.records / "logs"
        self.logs.mkdir(parents=True)
        self.output = self.logs / "tests.txt"
        self.output.write_text("one exercised assertion passed\n")
        self.manifest = self.logs / "R1.evidence.json"
        self.data = {
            "version": 1, "round": "R1", "artifact": "a" * 64,
            "checks": [{
                "criteria": ["AC1"], "command": "cargo test named_test",
                "fixture": "owned-test-database", "status": "PASS",
                "files": [{"path": "logs/tests.txt", "sha256": self.hash(self.output)}],
            }],
        }
        self.save()

    def run_git(self, *args):
        return subprocess.run(
            ["git", "-C", str(self.repo), *args], check=True, capture_output=True
        )

    @staticmethod
    def hash(path):
        return hashlib.sha256(path.read_bytes()).hexdigest()

    def save(self):
        self.manifest.write_text(json.dumps(self.data))

    def rejected(self, message):
        self.save()
        with self.assertRaisesRegex(CHECK.InvalidEvidence, message):
            CHECK.validate(self.manifest, self.repo)

    def test_local_untracked_and_strict_tracked_evidence(self):
        self.assertEqual(CHECK.validate(self.manifest, self.repo)["PASS"], 1)
        with self.assertRaisesRegex(CHECK.InvalidEvidence, "not tracked"):
            CHECK.validate(self.manifest, self.repo, True)
        self.run_git("add", ".")
        self.assertEqual(CHECK.validate(self.manifest, self.repo, True)["PASS"], 1)

    def test_ignored_output_requires_actual_tracking(self):
        (self.repo / ".gitignore").write_text("*.log\n")
        ignored = self.logs / "tests.log"
        self.output.rename(ignored)
        self.data["checks"][0]["files"][0]["path"] = "logs/tests.log"
        self.rejected("ignored and untracked")
        self.run_git("add", "-f", str(ignored))
        self.assertEqual(CHECK.validate(self.manifest, self.repo)["PASS"], 1)

    def test_manifest_itself_cannot_be_ignored(self):
        (self.repo / ".gitignore").write_text("*.evidence.json\n")
        self.rejected("ignored and untracked")

    def test_missing_and_changed_evidence(self):
        self.output.unlink()
        self.rejected("missing evidence")
        self.output.write_text("different run")
        self.rejected("hash mismatch")

    def test_paths_cannot_escape_records(self):
        for path in ("../outside.txt", str(self.repo / "outside.txt")):
            with self.subTest(path=path):
                self.data["checks"][0]["files"][0]["path"] = path
                self.rejected("relative to phase")

    def test_symlink_escape_and_ignored_internal_target(self):
        outside = self.repo / "outside.txt"
        outside.write_text("outside")
        self.output.unlink()
        self.output.symlink_to(outside)
        self.rejected("escapes phase")
        self.output.unlink()
        target = self.logs / "ignored.log"
        target.write_text("target")
        self.output.symlink_to(target)
        self.data["checks"][0]["files"][0]["sha256"] = self.hash(target)
        (self.repo / ".gitignore").write_text("*.log\n")
        self.rejected("ignored and untracked")

    def test_not_exercised_is_never_counted_as_pass(self):
        entry = self.data["checks"][0]
        entry.update(status="NOT EXERCISED", reason="fixture unavailable", files=[])
        self.save()
        self.assertEqual(CHECK.validate(self.manifest, self.repo),
                         {"PASS": 0, "FAIL": 0, "NOT EXERCISED": 1})
        del entry["reason"]
        self.rejected("reason")

    def test_executed_results_need_output_and_failures_remain_failures(self):
        entry = self.data["checks"][0]
        entry["status"] = "FAIL"
        self.save()
        self.assertEqual(CHECK.validate(self.manifest, self.repo)["FAIL"], 1)
        for status in ("FAIL", "PASS"):
            entry.update(status=status, files=[])
            self.rejected("needs retained output")

    def test_artifact_and_invalid_shapes(self):
        with self.assertRaisesRegex(CHECK.InvalidEvidence, "artifact does not match"):
            CHECK.validate(self.manifest, self.repo, expect_artifact="b" * 64)
        for key, value in (("version", True), ("checks", []), ("checks", [None])):
            with self.subTest(key=key, value=value):
                original = self.data[key]
                self.data[key] = value
                self.rejected("version|checks|object")
                self.data[key] = original
        self.data["checks"][0]["status"] = ["PASS"]
        self.rejected("invalid status")

    def test_aggregate_modes_and_cli_exit_status(self):
        self.assertEqual(CHECK.manifests(self.repo, False), [self.manifest])
        self.assertEqual(CHECK.manifests(self.repo, True), [])
        self.run_git("add", ".")
        self.assertEqual(CHECK.manifests(self.repo, True), [self.manifest])
        with contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(CHECK.main(["--repo", str(self.repo), "--tracked-manifests",
                                         "--require-tracked"]), 0)
        self.assertIn('"PASS": 1', output.getvalue())
        self.output.write_text("stale")
        with contextlib.redirect_stderr(io.StringIO()) as errors:
            self.assertEqual(CHECK.main(["--repo", str(self.repo), "--tracked-manifests"]), 1)
        self.assertIn("hash mismatch", errors.getvalue())

    def test_archival_move_keeps_relative_evidence_valid(self):
        destination = self.repo / "plan/.archived/topic"
        destination.parent.mkdir()
        (self.repo / "plan/topic").rename(destination)
        manifest = destination / "phases/p01/docs/logs/R1.evidence.json"
        self.assertEqual(CHECK.validate(manifest, self.repo)["PASS"], 1)

    def test_checkout_alias_is_supported_but_internal_directory_links_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            alias = Path(temporary) / "checkout"
            alias.symlink_to(self.repo, target_is_directory=True)
            manifest = alias / self.manifest.relative_to(self.repo)
            self.assertEqual(CHECK.validate(manifest, self.repo)["PASS"], 1)
        link = self.records / "linked-logs"
        link.symlink_to(self.logs, target_is_directory=True)
        self.data["checks"][0]["files"][0]["path"] = "linked-logs/tests.txt"
        self.rejected("symlinked evidence directory")


if __name__ == "__main__":
    unittest.main()
