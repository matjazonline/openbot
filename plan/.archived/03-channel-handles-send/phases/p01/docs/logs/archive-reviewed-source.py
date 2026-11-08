#!/usr/bin/env python3
"""Retain exact R1 bytes and the correction delta, verified by its frozen manifest."""
import difflib
import hashlib
import io
import json
from pathlib import Path
import tarfile

logs = Path(__file__).resolve().parent
root = logs.parents[5]
manifest = json.loads((logs / "CHS-P01-B1-R1-manifest.json").read_text())
before = {}
delta = []
for name, expected in manifest["source_files"].items():
    current = (root / name).read_bytes()
    original = current
    if name.endswith("/workflow/channel_reply.rs"):
        addition = b"    association::authorize_association(\n        tx,\n        request.company,\n        configuration.run_as(),\n        RelatedAssociation::Channel(request.channel),\n    )\n    .await?;\n"
        assert original.count(addition) == 1
        original = original.replace(addition, b"")
    elif name == "migrations/20260817000000_init_schema.sql":
        replacement = b"            AND binding.thread_id IS NULL AND configuration.message_trigger IS NOT NULL;"
        assert original.count(replacement) == 1
        original = original.replace(replacement, b"            AND binding.active AND binding.thread_id IS NULL AND configuration.message_trigger IS NOT NULL;")
    elif name.endswith("/channel_reply_binding_integrity_tests.rs"):
        first = original.index(b"#[tokio::test]\nasync fn channel_reply_binding_rechecks_")
        retained = original.index(b"#[tokio::test]\nasync fn channel_reply_binding_competing_ingress_", first)
        original = original[:first] + original[retained:]
    assert hashlib.sha256(original).hexdigest() == expected, name
    before[name] = original
    if original != current:
        delta.extend(difflib.unified_diff(original.decode().splitlines(keepends=True),
                     current.decode().splitlines(keepends=True), fromfile="a/"+name, tofile="b/"+name))
with tarfile.open(logs / "CHS-P01-B1-R1-source.tar.gz", "w:gz") as archive:
    for name, content in before.items():
        metadata = tarfile.TarInfo(name)
        metadata.size = len(content)
        metadata.mode = 0o644
        archive.addfile(metadata, io.BytesIO(content))
(logs / "CHS-P01-B1-R1-to-correction.diff").write_text("".join(delta))
receipt = {"source_manifest": "CHS-P01-B1-R1-manifest.json", "files_verified": len(before),
           "method": "inverse of the three recorded correction edits, each exact original byte hash verified",
           "archive_sha256": hashlib.sha256((logs / "CHS-P01-B1-R1-source.tar.gz").read_bytes()).hexdigest()}
(logs / "CHS-P01-B1-R1-source-archive.json").write_text(json.dumps(receipt, indent=2)+"\n")
print(json.dumps(receipt))
