#!/usr/bin/env python3
"""Service-owned model artifacts (D1/D7): the same pinned multilingual-e5-small files and ONNX
Runtime as OpenBot, recorded in assets/model-manifest.json and placed in the gitignored
assets/model/. Serving validates local files and never downloads.
  --check-manifest   validate the manifest itself (no files needed)
  --verify           check assets/model against the manifest (sizes and SHA-256)
  --download         fetch missing files over HTTPS and verify them (build network only)
"""
import hashlib, json, pathlib, platform, sys, tarfile, tempfile, urllib.request

ROOT = pathlib.Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "assets" / "model-manifest.json"
TARGET = ROOT / "assets" / "model"
PLATFORMS = {("Darwin", "arm64"): "osx-arm64", ("Linux", "aarch64"): "linux-aarch64", ("Linux", "x86_64"): "linux-x64"}


def manifest():
    data = json.loads(MANIFEST.read_text())
    assert data["model_id"].endswith("/fp32/mean/384"), "the service expects 384-d mean-pooled fp32"
    for artifact in data["files"] + data["bundled_files"]:
        assert artifact["path"] and artifact["size"] > 0 and len(artifact["sha256"]) == 64
    for artifact in data["bundled_files"]:
        assert digest(ROOT / "assets" / "bundled" / artifact["path"]) == artifact["sha256"], artifact["path"]
    for runtime in data["runtimes"]:
        assert runtime["library"] in [f["path"] for f in runtime["files"]]
    return data


def digest(path):
    hash = hashlib.sha256()
    with open(path, "rb") as handle:
        for block in iter(lambda: handle.read(1 << 16), b""):
            hash.update(block)
    return hash.hexdigest()


def wanted(data):
    name = PLATFORMS.get((platform.system(), platform.machine()))
    runtime = next((r for r in data["runtimes"] if r["platform"] == name), None)
    if runtime is None:
        sys.exit(f"no pinned ONNX Runtime for {platform.system()} {platform.machine()}")
    return data["files"] + data["bundled_files"], runtime


def verify(data):
    files, runtime = wanted(data)
    bad = []
    for artifact in files + runtime["files"]:
        path = TARGET / artifact["path"]
        if path.is_symlink() or not path.is_file() or path.stat().st_size != artifact["size"] or digest(path) != artifact["sha256"]:
            bad.append(artifact["path"])
    return bad


def download(data):
    files, runtime = wanted(data)
    for artifact in files:
        path = TARGET / artifact["path"]
        if "url" in artifact and not (path.is_file() and digest(path) == artifact["sha256"]):
            path.parent.mkdir(parents=True, exist_ok=True)
            urllib.request.urlretrieve(artifact["url"], path)
    for artifact in data["bundled_files"]:
        path = TARGET / artifact["path"]
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes((ROOT / "assets" / "bundled" / artifact["path"]).read_bytes())
    with tempfile.TemporaryDirectory() as tmp:
        archive = pathlib.Path(tmp) / "runtime.tgz"
        urllib.request.urlretrieve(runtime["archive_url"], archive)
        if digest(archive) != runtime["archive_sha256"]:
            sys.exit("ONNX Runtime archive digest mismatch")
        with tarfile.open(archive) as tar:
            members = {m.name.split("/", 1)[-1]: m for m in tar.getmembers()}
            for artifact in runtime["files"]:
                source = tar.extractfile(members[artifact["archive_path"]])
                path = TARGET / artifact["path"]
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(source.read())


def main():
    data = manifest()
    if "--check-manifest" in sys.argv:
        print(f"manifest ok: {data['model_id']}")
        return 0
    if "--download" in sys.argv:
        download(data)
    bad = verify(data)
    for path in bad:
        print(f"model artifact missing or invalid: assets/model/{path}", file=sys.stderr)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
