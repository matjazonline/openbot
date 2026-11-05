#!/usr/bin/env python3
"""Verify artifact preparation without model downloads or native inference."""
import hashlib
import importlib.util
import io
import json
import tarfile
import tempfile
import unittest
from pathlib import Path

SOURCE = Path(__file__).resolve().parents[1] / "prepare-knowledge-model.py"
spec = importlib.util.spec_from_file_location("prepare", SOURCE)
prepare = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prepare)


def item(path, data):
    return {"path": path, "size": len(data), "sha256": hashlib.sha256(data).hexdigest()}


class PreparationTests(unittest.TestCase):
    def test_validated_atomic_publish_cache_and_corruption(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            model = root / "cache/models/multilingual-e5-small-revision/model.onnx"
            model.parent.mkdir(parents=True)
            model.write_bytes(b"model fixture")
            archive = root / "cache/archives/runtime.tgz"
            archive.parent.mkdir()
            with tarfile.open(archive, "w:gz") as bundle:
                record = tarfile.TarInfo("root/lib/runtime.so")
                record.size = 7
                bundle.addfile(record, io.BytesIO(b"runtime"))
            archive_bytes = archive.read_bytes()
            artifact = item("runtime/runtime.so", b"runtime")
            artifact["archive_path"] = "lib/runtime.so"
            runtime = {"platform": "linux-x64", "archive_url": "https://invalid.test/runtime.tgz", "archive_size": len(archive_bytes), "archive_sha256": hashlib.sha256(archive_bytes).hexdigest(), "files": [artifact]}
            manifest = root / "model-manifest.json"
            manifest.write_text(json.dumps({"model_revision": "revision", "files": [item("model.onnx", model.read_bytes())], "runtimes": [runtime], "bundled_files": [item(name,b"fixture") for name in ["MODEL-LICENSE.txt","MODEL-NOTICE.txt"]]}))
            for name in ["MODEL-LICENSE.txt", "MODEL-NOTICE.txt"]:
                (root / name).write_text("fixture")
            original = prepare.MANIFEST
            prepare.MANIFEST = manifest
            try:
                output = root / "published"
                prepare.prepare(output, "linux-x64", root / "cache")
                self.assertEqual((output / "model.onnx").read_bytes(), b"model fixture")
                self.assertEqual((output / "runtime/runtime.so").read_bytes(), b"runtime")
                prepare.prepare(output, "linux-x64", root / "cache")
                (output / "model.onnx").chmod(0o644)
                (output / "model.onnx").write_bytes(b"tampered")
                with self.assertRaisesRegex(ValueError, "Existing model directory is invalid"):
                    prepare.prepare(output, "linux-x64", root / "cache")
                self.assertFalse(any(root.glob(".knowledge-model-*")))
            finally:
                prepare.MANIFEST = original

    def test_runtime_archive_rejects_links_duplicates_wrong_size_and_missing_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            cache = root / "cache/archives"
            cache.mkdir(parents=True)
            for case in ["link", "duplicate", "wrong_size", "missing"]:
                archive = cache / "runtime.tgz"
                with tarfile.open(archive, "w:gz") as bundle:
                    if case != "missing":
                        entry = tarfile.TarInfo("root/lib/runtime.so")
                        entry.size = 8 if case == "wrong_size" else 7
                        if case == "link":
                            entry.type = tarfile.SYMTYPE
                            entry.linkname = "/etc/passwd"
                        bundle.addfile(entry, io.BytesIO(b"runtime!") if case == "wrong_size" else io.BytesIO(b"runtime"))
                        if case == "duplicate":
                            bundle.addfile(entry, io.BytesIO(b"runtime"))
                data = archive.read_bytes()
                artifact = item("runtime/runtime.so", b"runtime")
                artifact["archive_path"] = "lib/runtime.so"
                runtime = {"archive_url": "https://invalid.test/runtime.tgz", "archive_size": len(data), "archive_sha256": hashlib.sha256(data).hexdigest(), "files": [artifact]}
                stage = root / case
                stage.mkdir()
                with self.assertRaises(ValueError, msg=case):
                    prepare.stage_runtime(stage, runtime, root / "cache")
                self.assertFalse((root / "passwd").exists())


if __name__ == "__main__":
    unittest.main()
