#!/usr/bin/env python3
"""Required offline/non-root image, startup integrity and early resident-memory gate."""
import argparse
import json
import re
import subprocess
import tempfile
import uuid
from pathlib import Path

MEMORY_MIB = 2048
PEAK_RSS_BUDGET_MIB = 1536
MODEL = "/app/models/multilingual-e5-small"
ROOT = Path(__file__).resolve().parent.parent


def docker(*args, timeout=120, success=True):
    result = subprocess.run(["docker", *args], text=True, capture_output=True, timeout=timeout)
    if success and result.returncode:
        raise RuntimeError("Docker check failed: " + result.stderr[-4096:])
    return result


def run(image, command, *options, success=True):
    name="knowledge-image-probe-"+uuid.uuid4().hex
    try:
        return docker("run", "--name", name, "--rm", "--network", "none", "--read-only", "--memory", f"{MEMORY_MIB}m", "--memory-swap", f"{MEMORY_MIB}m", "--cpus", "2", *options, image, *command, success=success)
    finally:
        # A killed Docker client can leave its native call running; remove only our unique probe.
        docker("rm","--force",name,timeout=10,success=False)


def failed_startup(image, source, mount_target):
    options = ("--mount", f"type=bind,src={source},dst={mount_target},readonly")
    for command in [["/app/open_bots", "knowledge", "model-check"], ["/app/open_bots"]]:
        result = run(image, command, *options,
            "--env", "JWT_SECRET=knowledge-image-probe-jwt-secret-0001",
            "--env", "AUTH_ATTEMPT_SECRET=knowledge-image-probe-independent-attempt-secret-0002",
            "--env", "APP_DOMAIN_NAME=example.test",
            "--env", "DATABASE_URL=postgres://invalid:invalid@127.0.0.1:1/unreachable",
            success=False)
        output = result.stdout + result.stderr
        if result.returncode == 0 or "Knowledge artifact missing or invalid at " not in output or mount_target not in output:
            raise RuntimeError("Missing/tampered artifact did not fail at the model boundary: " + output[-4096:])
        if "error communicating with database" in output.lower():
            raise RuntimeError("Serve reached the database before validating model artifacts")


def check(image):
    fly=(ROOT / "fly.toml").read_text()
    if not re.search(r'^\s*memory\s*=\s*"'+str(MEMORY_MIB//1024)+r'gb"\s*$',fly,re.MULTILINE):
        raise RuntimeError("Fly memory and the required model container bound must move together")
    metadata = json.loads(docker("image", "inspect", image).stdout)[0]
    user = metadata["Config"]["User"]
    if not user or user in ("root", "0", "0:0"):
        raise RuntimeError("Production image must name a non-root user")
    run(image, ["/bin/sh", "-ec", f'test "$(id -u)" != 0; test ! -w {MODEL}/onnx/model.onnx; test "$(stat -c %u {MODEL}/onnx/model.onnx)" = 0; test -r {MODEL}/MODEL-LICENSE.txt; test -r {MODEL}/runtime/LICENSE; test -r {MODEL}/runtime/ThirdPartyNotices.txt'])
    installed_app_bytes=int(run(image,["du","-sb","/app"]).stdout.split()[0])
    result = run(image, ["/app/open_bots", "knowledge", "model-check"])
    probe = json.loads(result.stdout)
    manifest = json.loads((ROOT / "assets/knowledge/model-manifest.json").read_text())
    if probe["model_id"] != manifest["model_id"] or probe["dimensions"] != 384 or abs(probe["norm"] - 1) > 0.001:
        raise RuntimeError("Image model identity/shape/norm mismatch")
    if probe["batch_size"] != 4 or probe["batch_tokens"] != 512:
        raise RuntimeError("Image did not exercise the maximum native batch")
    if probe["chunk_count"]<2 or probe["largest_chunk_tokens"]>512 or not probe["oversized_query_and_batch_refused"]:
        raise RuntimeError("Image did not enforce tokenizer-aware chunk/query/batch limits")
    if probe["peak_rss_bytes"] > PEAK_RSS_BUDGET_MIB * 1024 * 1024:
        raise RuntimeError(f'Model peak RSS exceeds the early {PEAK_RSS_BUDGET_MIB} MiB budget: {probe["peak_rss_bytes"]}')
    if probe["translation_score"] < probe["unrelated_score"] + 0.02:
        raise RuntimeError("Image failed multilingual relevance")
    fixture_root=ROOT / "target"
    fixture_root.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="knowledge-image-",dir=fixture_root) as temporary:
        root = Path(temporary)
        empty = root / "empty"
        empty.mkdir(mode=0o755)
        tampered = root / "config.json"
        tampered.write_bytes(b"x" * 655)
        tampered.chmod(0o644)
        failed_startup(image, empty, MODEL)
        failed_startup(image, tampered, MODEL + "/config.json")
    return {"image": image, "image_id": metadata["Id"], "docker_reported_image_size_bytes": metadata["Size"], "installed_app_bytes": installed_app_bytes, "user": user, "network": "none", "read_only": True, "container_memory_mib": MEMORY_MIB, "peak_rss_budget_mib": PEAK_RSS_BUDGET_MIB, "probe": probe, "missing_and_tampered_model_check_and_serve": "PASS"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", required=True)
    args = parser.parse_args()
    print(json.dumps(check(args.image), indent=2))


if __name__ == "__main__":
    main()
