#!/usr/bin/env python3
"""Validate round evidence identity and Git retention; never execute recorded commands."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys


class InvalidEvidence(ValueError):
    pass


def git(repo, *args):
    result = subprocess.run(
        ["git", "-C", str(repo), *args], capture_output=True, check=False
    )
    if result.returncode not in (0, 1):
        raise InvalidEvidence(result.stderr.decode(errors="replace").strip())
    return result


def beneath(path, root):
    try:
        path.relative_to(root)
        return True
    except ValueError:
        return False


def retained(path, repo, require_tracked):
    relative = path.relative_to(repo).as_posix()
    tracked = git(repo, "ls-files", "--error-unmatch", "--", ":(literal)" + relative)
    if tracked.returncode == 0:
        return
    if require_tracked:
        raise InvalidEvidence("not tracked: " + relative)
    ignored = git(repo, "check-ignore", "--", relative)
    if ignored.returncode == 0:
        raise InvalidEvidence("ignored and untracked: " + relative)


def text_field(value, label):
    if not isinstance(value, str) or not value.strip():
        raise InvalidEvidence(label + " must be a nonempty string")


def hex_digest(value, label):
    if not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{64}", value):
        raise InvalidEvidence(label + " must be 64 lowercase hex digits")


def regular_file(path, records, repo, require_tracked):
    resolved = path.resolve()
    if not beneath(path, records) or not beneath(resolved, records):
        raise InvalidEvidence("path escapes phase records: " + str(path))
    if not path.is_file():
        raise InvalidEvidence("missing evidence file: " + str(path))
    # Git stores directory symlinks, not files underneath them.
    if any(parent.is_symlink() for parent in path.parents if beneath(parent, repo)):
        raise InvalidEvidence("symlinked evidence directory: " + str(path))
    retained(path, repo, require_tracked)
    if resolved != path:
        retained(resolved, repo, require_tracked)
    return resolved


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def validate(manifest, repo, require_tracked=False, expect_artifact=None):
    manifest = Path(manifest).absolute()
    repo = Path(repo).resolve()
    # Normalize aliases of the checkout root (macOS /var and /private/var), while
    # retaining directory symlinks inside the checkout for the containment checks.
    for parent in manifest.parents:
        if parent.resolve() == repo:
            manifest = repo / manifest.relative_to(parent)
            break
    else:
        raise InvalidEvidence("manifest must be inside the repository")
    if manifest.parent.name != "logs" or not manifest.name.endswith(".evidence.json"):
        raise InvalidEvidence("manifest must be logs/<round>.evidence.json")
    records = manifest.parent.parent
    if not beneath(records, repo) or records == repo or records.resolve() != records:
        raise InvalidEvidence("phase records must be a real directory inside the repository")
    regular_file(manifest, records, repo, require_tracked)
    if manifest.stat().st_size > 1024 * 1024:
        raise InvalidEvidence("manifest exceeds 1 MiB")
    try:
        data = json.loads(manifest.read_text())
    except (ValueError, UnicodeError) as error:
        raise InvalidEvidence("invalid JSON: " + str(error)) from error
    if not isinstance(data, dict) or type(data.get("version")) is not int or data["version"] != 1:
        raise InvalidEvidence("manifest version must be 1")
    text_field(data.get("round"), "round")
    hex_digest(data.get("artifact"), "artifact")
    if expect_artifact is not None and data["artifact"] != expect_artifact:
        raise InvalidEvidence("artifact does not match the reviewed snapshot")
    checks = data.get("checks")
    if not isinstance(checks, list) or not checks:
        raise InvalidEvidence("checks must be a nonempty list")
    results = {"PASS": 0, "FAIL": 0, "NOT EXERCISED": 0}
    for index, check in enumerate(checks):
        label = "check " + str(index + 1)
        if not isinstance(check, dict):
            raise InvalidEvidence(label + " must be an object")
        criteria = check.get("criteria")
        if not isinstance(criteria, list) or not criteria:
            raise InvalidEvidence(label + " needs criterion IDs")
        for criterion in criteria:
            text_field(criterion, label + " criterion")
        for field in ("command", "fixture"):
            text_field(check.get(field), label + " " + field)
        status = check.get("status")
        if not isinstance(status, str) or status not in results:
            raise InvalidEvidence(label + " has an invalid status")
        results[status] += 1
        files = check.get("files", [])
        if not isinstance(files, list):
            raise InvalidEvidence(label + " files must be a list")
        if status == "NOT EXERCISED":
            text_field(check.get("reason"), label + " reason")
        elif not files:
            raise InvalidEvidence(label + " " + status + " needs retained output")
        seen = set()
        for item in files:
            if not isinstance(item, dict):
                raise InvalidEvidence(label + " file must be an object")
            text_field(item.get("path"), label + " file path")
            relative = Path(item["path"])
            if relative.is_absolute() or ".." in relative.parts:
                raise InvalidEvidence(label + " file path must stay relative to phase records")
            if relative in seen:
                raise InvalidEvidence(label + " repeats an evidence path")
            seen.add(relative)
            hex_digest(item.get("sha256"), label + " file hash")
            path = regular_file(records / relative, records, repo, require_tracked)
            if path == manifest.resolve():
                raise InvalidEvidence("manifest cannot be its own evidence")
            if sha256(path) != item["sha256"]:
                raise InvalidEvidence("evidence hash mismatch: " + str(relative))
    return results


def manifests(repo, tracked_only):
    args = ["ls-files", "-z", "--cached"]
    if not tracked_only:
        args += ["--others", "--exclude-standard"]
    output = git(repo, *args).stdout.decode()
    return sorted({
        repo / name for name in output.split("\0")
        if name and Path(name).parent.name == "logs" and name.endswith(".evidence.json")
    })


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifests", nargs="*")
    parser.add_argument("--repo", default=".")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--tracked-manifests", action="store_true")
    mode.add_argument("--available-manifests", action="store_true")
    parser.add_argument("--require-tracked", action="store_true")
    parser.add_argument("--expect-artifact")
    args = parser.parse_args(argv)
    if bool(args.manifests) == bool(args.tracked_manifests or args.available_manifests):
        parser.error("supply manifest paths or one aggregate mode")
    try:
        root = git(Path(args.repo), "rev-parse", "--show-toplevel").stdout.decode().strip()
        if not root:
            raise InvalidEvidence("repository root unavailable")
        repo = Path(root).resolve()
        paths = ([Path(name) for name in args.manifests] if args.manifests else
                 manifests(repo, args.tracked_manifests))
        for path in paths:
            counts = validate(path, repo, args.require_tracked, args.expect_artifact)
            print(str(path) + ": retained evidence valid " + json.dumps(counts, sort_keys=True))
        print(str(len(paths)) + " evidence manifest(s) validated; declared results are not acceptance")
    except (InvalidEvidence, OSError) as error:
        print("evidence validation failed: " + str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
