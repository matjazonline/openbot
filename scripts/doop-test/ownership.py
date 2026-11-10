"""Durable intent and exact Docker ownership for disposable deployment resources."""
import json
import os
from pathlib import Path
import re
import secrets
import tempfile


LABEL = "io.openbot.private-deployment"
LABELS = {LABEL, "io.openbot.test-run"}


class OwnedResources:
    def __init__(self, runtime, control, owner=None, recover=False):
        self.runtime = Path(runtime)
        self.control = control
        if self.runtime.is_symlink() or not self.runtime.is_dir():
            raise ValueError("Ownership runtime must be an existing private directory")
        info = self.runtime.stat()
        if info.st_uid != os.getuid() or info.st_mode & 0o077:
            raise ValueError("Ownership runtime must belong to this user with mode0700")
        self.path = self.runtime / "ownership.json"
        if recover:
            if self.path.is_symlink() or self.path.stat().st_size > 32768:
                raise ValueError("Invalid retained ownership state")
            self.state = json.loads(self.path.read_text())
            self.validate()
            self.check_engine()
        else:
            if self.path.exists() or self.path.is_symlink():
                raise ValueError("Refusing to adopt existing ownership state")
            self.state = {"version": 1, "owner": owner or secrets.token_hex(16), "engine": None, "resources": {}}
            self.save()
            self.state["engine"] = self.engine()
            self.save()

    def validate(self):
        if not isinstance(self.state, dict) or set(self.state) != {"version", "owner", "engine", "resources"} or self.state["version"] != 1:
            raise ValueError("Invalid retained ownership state")
        if not isinstance(self.state["owner"], str) or not re.fullmatch(r"[0-9a-f]{24,64}", self.state["owner"]):
            raise ValueError("Invalid retained ownership identity")
        resources = self.state["resources"]
        if not isinstance(resources, dict) or len(resources) > 16:
            raise ValueError("Invalid retained ownership resource count")
        names = set()
        for key, item in resources.items():
            if not re.fullmatch(r"[a-z][a-z0-9-]{0,31}", key):
                raise ValueError("Invalid retained ownership key")
            if not isinstance(item, dict) or set(item) != {"kind", "name", "label", "owner", "phase", "id"}:
                raise ValueError("Invalid retained ownership resource")
            if item["kind"] not in {"container", "volume", "network"} or item["label"] not in LABELS:
                raise ValueError("Invalid retained ownership resource kind")
            if not re.fullmatch(r"[a-z][a-z0-9_.-]{0,127}", item["name"]) or item["owner"] != self.state["owner"]:
                raise ValueError("Invalid retained ownership resource identity")
            if item["phase"] not in {"pending", "created", "removed"}:
                raise ValueError("Invalid retained ownership phase")
            if item["phase"] == "created" and item["id"] is None:
                raise ValueError("Created resource lacks its receipt")
            if item["id"] is not None and not (
                item["id"] == item["name"] if item["kind"] == "volume" else re.fullmatch(r"[0-9a-f]{64}", item["id"])
            ):
                raise ValueError("Invalid retained ownership resource receipt")
            identity = (item["kind"], item["name"])
            if identity in names:
                raise ValueError("Duplicate retained ownership resource")
            names.add(identity)

    def save(self):
        self.validate()
        temporary = None
        try:
            with tempfile.NamedTemporaryFile(mode="w", dir=self.runtime, prefix=".ownership-", delete=False) as stream:
                temporary = Path(stream.name)
                json.dump(self.state, stream)
                stream.flush()
                os.fsync(stream.fileno())
            os.replace(temporary, self.path)
        finally:
            if temporary is not None:
                temporary.unlink(missing_ok=True)

    def engine(self):
        identity = self.control("info", "--format", "{{.ID}}", timeout=10).strip()
        if not re.fullmatch(r"[A-Za-z0-9:_-]{1,128}", identity):
            raise RuntimeError("Docker engine identity is unavailable")
        return identity

    def check_engine(self):
        if self.state["engine"] != self.engine():
            raise RuntimeError("Docker engine changed; retained resources were not touched")

    def begin(self, key, kind, name, label=LABEL):
        old = self.state["resources"].get(key)
        if old is not None and old["phase"] != "removed":
            raise RuntimeError("A resource creation is already outstanding")
        if old is None and len(self.state["resources"]) >= 16:
            raise ValueError("Owned resource count exceeds its bound")
        self.state["resources"][key] = {"kind": kind, "name": name, "label": label,
            "owner": self.state["owner"], "phase": "pending", "id": None}
        try:
            self.save()
        except BaseException:
            if old is None:
                del self.state["resources"][key]
            else:
                self.state["resources"][key] = old
            raise

    def created(self, key, receipt):
        item = self.state["resources"][key]
        if item["phase"] != "pending":
            raise RuntimeError("Resource creation receipt has no pending intent")
        # Persist a malformed/unconfirmed receipt as pending, never as success.
        receipt = receipt.strip()
        valid = receipt == item["name"] if item["kind"] == "volume" else re.fullmatch(r"[0-9a-f]{64}", receipt)
        if not valid:
            raise RuntimeError("Resource creation outcome is unconfirmed")
        item.update(phase="created", id=receipt)
        self.save()

    def locate(self, item):
        kind = item["kind"]
        listing = [kind, "ls"] + (["--all", "--no-trunc"] if kind == "container" else ["--no-trunc"] if kind == "network" else [])
        listing += ["--filter", "label=" + item["label"] + "=" + item["owner"], "--format", "{{.Name}}" if kind == "volume" else "{{.ID}}"]
        candidates = self.control(*listing).splitlines()
        if len(candidates) > 16:
            raise RuntimeError("Owned Docker listing exceeds its bound")
        matches = []
        for candidate in candidates:
            labels = ".Config.Labels" if kind == "container" else ".Labels"
            identifier = ".Name" if kind == "volume" else ".Id"
            template = '{"name":{{json .Name}},"id":{{json ' + identifier + '}},"owner":{{json (index ' + labels + ' "' + item["label"] + '")}}}'
            actual = json.loads(self.control(kind, "inspect", "--format", template, candidate))
            name = actual["name"].removeprefix("/")
            if actual["id"] == item["id"] and name != item["name"]:
                raise RuntimeError("Owned resource was renamed; cleanup retained")
            if name != item["name"]:
                continue
            if actual["owner"] != item["owner"] or (item["id"] is not None and actual["id"] != item["id"]):
                raise RuntimeError("Resource ownership changed; cleanup retained")
            matches.append(actual["id"])
        if len(matches) > 1:
            raise RuntimeError("Duplicate resource ownership; cleanup retained")
        return matches[0] if matches else None

    def remove(self, key):
        item = self.state["resources"].get(key)
        if item is None or item["phase"] == "removed":
            return
        self.check_engine()
        identifier = self.locate(item)
        if identifier is None and item["phase"] == "pending":
            raise RuntimeError("Creation outcome unconfirmed; cleanup state retained")
        if identifier is not None:
            if item["kind"] == "container":
                self.control("stop", "--time=5", identifier)
                self.control("rm", identifier)
            else:
                self.control(item["kind"], "rm", identifier)
            if self.locate(item) is not None:
                raise RuntimeError("Resource cleanup unconfirmed; state retained")
        item["phase"] = "removed"
        self.save()

    def close(self):
        # Later resources may depend on earlier ones. An uncertain app creation
        # must not lose its volume/network while the daemon could still start it.
        for key in reversed(self.state["resources"]):
            self.remove(key)
