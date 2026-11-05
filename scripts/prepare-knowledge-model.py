#!/usr/bin/env python3
"""Build/development only: prepare exactly verified local model/runtime artifacts."""
import argparse
import hashlib
import json
import os
import platform
import shutil
import tarfile
import tempfile
import time
import urllib.request
from pathlib import Path

MANIFEST = Path(__file__).resolve().parent.parent / "assets/knowledge/model-manifest.json"


def verified(path, item):
    if not path.is_file() or path.is_symlink() or path.stat().st_size != item["size"]:
        return False
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest() == item["sha256"]


def download(url, destination, size, digest):
    item = {"size": size, "sha256": digest}
    for attempt in range(3):
        deadline = time.monotonic() + 600
        try:
            request = urllib.request.Request(url, headers={"User-Agent": "openbot-artifact-prepare/1"})
            with urllib.request.urlopen(request, timeout=30) as source, destination.open("wb") as target:
                read = 0
                while chunk := source.read(1024 * 1024):
                    read += len(chunk)
                    if read > size or time.monotonic() >= deadline:
                        raise ValueError("Artifact exceeded its size or download deadline")
                    target.write(chunk)
            if not verified(destination, item):
                raise ValueError("Artifact checksum/size mismatch: " + destination.name)
            return
        except (OSError, ValueError):
            destination.unlink(missing_ok=True)
            if attempt == 2:
                raise
            time.sleep(attempt + 1)


def stage_file(stage, item, cache_path=None):
    destination = stage / item["path"]
    destination.parent.mkdir(parents=True, exist_ok=True)
    if cache_path and verified(cache_path, item):
        shutil.copyfile(cache_path, destination)
    else:
        download(item["url"], destination, item["size"], item["sha256"])


def stage_runtime(stage, runtime, cache):
    archive = stage / "runtime.tgz"
    source = cache / "archives" / runtime["archive_url"].rsplit("/", 1)[-1] if cache else None
    archive_item = {"size": runtime["archive_size"], "sha256": runtime["archive_sha256"]}
    if source and verified(source, archive_item):
        shutil.copyfile(source, archive)
    else:
        download(runtime["archive_url"], archive, archive_item["size"], archive_item["sha256"])
    expected = {item["archive_path"]: item for item in runtime["files"]}
    found = set()
    with tarfile.open(archive, "r:gz") as bundle:
        for entry in bundle:
            # Only exact manifest-listed regular files. Never follow links or extract paths.
            relative = entry.name.partition("/")[2]
            item = expected.get(relative)
            if not item:
                continue
            if relative in found or not entry.isfile() or entry.size != item["size"]:
                raise ValueError("Invalid runtime archive member: " + relative)
            found.add(relative)
            destination = stage / item["path"]
            destination.parent.mkdir(parents=True, exist_ok=True)
            with bundle.extractfile(entry) as source_file, destination.open("wb") as output:
                shutil.copyfileobj(source_file, output, 64 * 1024)
            if not verified(destination, item):
                raise ValueError("Runtime artifact checksum mismatch: " + relative)
    archive.unlink()
    if found != set(expected):
        raise ValueError("Runtime archive is missing required files")


def default_platform():
    system, machine = platform.system(), platform.machine().lower()
    if system == "Darwin" and machine == "arm64":
        return "osx-arm64"
    if system == "Linux" and machine in ("aarch64", "arm64"):
        return "linux-aarch64"
    if system == "Linux" and machine in ("x86_64", "amd64"):
        return "linux-x64"
    raise ValueError("Unsupported model runtime platform")


def prepare(destination, target, cache):
    manifest = json.loads(MANIFEST.read_text())
    runtime = next(item for item in manifest["runtimes"] if item["platform"] == target)
    artifacts = manifest["files"] + runtime["files"] + manifest["bundled_files"]
    if destination.exists():
        if all(verified(destination / item["path"], item) for item in artifacts):
            return
        raise ValueError("Existing model directory is invalid; remove it before preparing again: " + str(destination))
    destination.parent.mkdir(parents=True, exist_ok=True)
    stage = Path(tempfile.mkdtemp(prefix=".knowledge-model-", dir=destination.parent))
    try:
        for item in manifest["files"]:
            source = cache / "models" / ("multilingual-e5-small-" + manifest["model_revision"]) / item["path"] if cache else None
            stage_file(stage, item, source)
        stage_runtime(stage, runtime, cache)
        for item in manifest["bundled_files"]:
            source=MANIFEST.parent / item["path"]
            if not verified(source,item):
                raise ValueError("Baked license/notice checksum mismatch: "+item["path"])
            shutil.copyfile(source,stage / item["path"])
        for item in artifacts:
            if not verified(stage / item["path"], item):
                raise ValueError("Prepared artifact verification failed: " + item["path"])
        for file in stage.rglob("*"):
            if file.is_file():
                file.chmod(0o444)
        os.rename(stage, destination)
    finally:
        if stage.exists():
            shutil.rmtree(stage)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--destination", type=Path, default=Path("target/knowledge-model"))
    parser.add_argument("--platform", choices=("osx-arm64", "linux-aarch64", "linux-x64"))
    parser.add_argument("--cache", type=Path, help="Optional verified development artifact cache")
    args = parser.parse_args()
    prepare(args.destination.resolve(), args.platform or default_platform(), args.cache)
    print("Verified local knowledge artifacts: " + str(args.destination.resolve()))


if __name__ == "__main__":
    main()
