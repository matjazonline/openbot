"""Actual-image, native PostgreSQL and private storage test deployment."""
import importlib.util
import json
import os
from pathlib import Path
import re
import secrets
import signal
import shutil
import socket
import subprocess
import tempfile
import time
import urllib.request


def sibling(name):
    spec = importlib.util.spec_from_file_location("doop_private_" + name, Path(__file__).with_name(name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


IMAGE = sibling("image_service")
INGRESS = sibling("ingress")
OWNED = sibling("ownership")
RESTORE = sibling("restore")
POSTGRES = "postgres:18.6-alpine@sha256:123b17998ccaf47e53b502e8c6a1e79afd8f579c6f3eec83c563da026f957395"


def validate_private_report(result, mode, additional_required=(), *, suite="all"):
    if suite not in ("all", "e2e") or result.get("suite") != suite:
        raise RuntimeError("Private acceptance report has the wrong probe family")
    required = {
        "live Doop bot-key rotation revokes both old keys",
        "live frozen Openbot publications enforce current keys, disable and disconnect",
        "Openbot disconnect retains remote company documents and private assets",
        "same-schema owned database restore retains private assets and revoked keys",
        "private image byte and metadata isolation",
        "alternate loopback backend cannot bypass private image authorization or warm caches",
        "actual HTTPS held upload denies publication after its parent deletion completes",
        "actual image materialization binds a draft to its current parent and preserves canonical source",
        "actual image screenshot and multipage PDF contain private pixels and the real embedded font",
        "actual MCP dense previews and full downloads retain private current-session authority",
        "actual normal browser paints private image and font in an opaque editor frame",
        "actual browser PDF export control downloads selected private pages with embedded fonts",
        "actual foreign and signed-out human browsers cannot open the private canvas",
        "an explicitly invited human opens and edits a private canvas in the normal browser",
        "human membership removal clears the warm editor and denies browser revalidation",
        "known private media and export URLs deny after membership and key revocation",
        "owner deletion invalidates warm private bytes and the actual open editor",
        "private acceptance browser cleanup",
    }
    if suite == "e2e":
        required = {
            "live two-company Openbot document workflows and current approvals",
            "live worker restart reuses known creates and parks a lost receipt",
            "live document edits persist and bot keys reopen and export",
            "live ordinary humans reopen their generated private documents",
            "live human PDF controls export generated private documents",
            "live generated document IDs and private media deny foreign and signed-out access",
            "private acceptance browser cleanup",
        }
    if mode == "bucket":
        required.add("actual private bucket origin rejects unsigned known files")
    elif mode != "disk":
        raise ValueError("Unknown private deployment storage mode")
    required.update(additional_required)
    checks = result.get("checks")
    if not isinstance(checks, list) or not 1 <= len(checks) <= 64:
        raise RuntimeError("Private acceptance report is missing its bounded executed checks")
    names = [check.get("name") for check in checks if isinstance(check, dict)]
    if len(names) != len(checks) or any(not isinstance(name, str) for name in names) or len(set(names)) != len(names):
        raise RuntimeError("Private acceptance report has invalid or duplicate checks")
    passed = {check["name"] for check in checks if check.get("status") == "PASS"}
    if required - passed or any(check.get("status") != "PASS" for check in checks):
        raise RuntimeError("Required actual-image private acceptance was not successfully exercised")
    for name, maximum in (("requests", 160), ("privateRequests", 96)):
        count = result.get(name)
        if type(count) is not int or not 1 <= count <= maximum:
            raise RuntimeError("Private acceptance request count is outside its fixed budget")


def private_json(path, value):
    with os.fdopen(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "w") as stream:
        json.dump(value, stream)
        stream.flush()
        os.fsync(stream.fileno())


def replace_private_json(path, value):
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", dir=path.parent, prefix=".private-state-", delete=False) as stream:
            temporary = Path(stream.name)
            json.dump(value, stream)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def read_private_json(path):
    if path.is_symlink() or not path.is_file() or path.stat().st_size > 8192:
        raise ValueError("Invalid private deployment recovery record")
    value = json.loads(path.read_text())
    if not isinstance(value, dict):
        raise ValueError("Invalid private deployment recovery record")
    return value


def stop_retained_process(runtime, role, owner, grace=5):
    if role not in ("probe", "browser"):
        raise ValueError("Invalid retained process role")
    path = runtime / (role + "-process.json")
    if not path.exists():
        return
    record = read_private_json(path)
    if set(record) != {"version", "phase", "pid", "owner"} or record["version"] != 1 or record["owner"] != owner:
        raise ValueError("Invalid retained process identity")
    if record["phase"] == "removed":
        return
    if record["phase"] not in ("pending", "created"):
        raise ValueError("Invalid retained process state")
    pid = record["pid"]
    if not isinstance(pid, int) or not 1 < pid < 2 ** 31:
        raise RuntimeError("Local process creation is unconfirmed; recovery state retained")
    marker = "--doop-test-owner=" + owner if role == "probe" else "--user-data-dir=" + str(runtime / "ui-profile")
    def identity():
        result = subprocess.run(["ps", "-p", str(pid), "-o", "command="], capture_output=True, timeout=5)
        if result.returncode == 1:
            return False
        if result.returncode or len(result.stdout) > 8192:
            raise RuntimeError("Retained process identity is unavailable")
        command = result.stdout.decode("utf-8", errors="replace")
        if not re.search(r"(?<!\S)" + re.escape(marker) + r"(?!\S)", command) or (role == "probe" and str(Path(__file__).with_name("probe.mjs")) not in command):
            raise RuntimeError("Retained PID belongs to another invocation; no process was stopped")
        try:
            group = os.getpgid(pid)
        except ProcessLookupError:
            return False
        if group != pid:
            raise RuntimeError("Retained process group ownership is unconfirmed")
        return True
    if identity():
        try:
            os.killpg(pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
    deadline = time.monotonic() + grace
    while True:
        try:
            os.killpg(pid, 0)
        except ProcessLookupError:
            replace_private_json(path, {**record, "phase": "removed"})
            return
        except PermissionError:
            # Darwin can briefly report EPERM for a zombie-only group. Keep
            # waiting for actual reaping, never turn it into a cleanup receipt.
            if time.monotonic() >= deadline:
                raise RuntimeError("Retained process group cannot be inspected") from None
            time.sleep(0.02)
            continue
        if time.monotonic() >= deadline:
            break
        time.sleep(0.02)
    if not identity():
        raise RuntimeError("A retained group outlived its leader; recovery state retained")
    try:
        os.killpg(pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        try:
            os.killpg(pid, 0)
        except ProcessLookupError:
            replace_private_json(path, {**record, "phase": "removed"})
            return
        except PermissionError:
            pass  # Receipt still requires an actual absent group.
        time.sleep(0.02)
    raise RuntimeError("Local process cleanup remains unconfirmed")


def recover(runtime, output, runner):
    """Explicit reconciliation; never infer pending creation from an empty list."""
    import hashlib
    runtime = Path(runtime).absolute()
    if runtime.is_symlink() or not re.fullmatch(r"\.private-run-[a-z0-9_]{8}", runtime.name):
        raise ValueError("Recovery requires a run directory created by this runner")
    info = runtime.stat()
    if info.st_uid != os.getuid() or info.st_mode & 0o077:
        raise ValueError("Recovery runtime is not protected by its original user")
    record = read_private_json(runtime / "recovery.json")
    required = {"version", "mode", "source", "storage_helper_sha256", "owner", "runner_pid"}
    if set(record) != required or record["version"] != 2 or record["mode"] not in ("disk", "bucket"):
        raise ValueError("Invalid private deployment recovery identity")
    source = Path(record["source"])
    if source.name != "managed-source" or source.is_symlink() or runtime.parent.resolve() != source.parent.resolve():
        raise ValueError("Retained run does not belong to its verified setup")
    if runtime.resolve() in output.resolve().parents or runtime.resolve() == output.resolve():
        raise ValueError("Recovery output must be outside the private runtime")
    pid = record["runner_pid"]
    if not isinstance(pid, int) or pid <= 1:
        raise ValueError("Invalid original runner process identity")
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        pass
    else:
        raise RuntimeError("Original runner PID is active; recovery refused")
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    command_env = runner.clean_environment(runtime / "home")
    command_env["DOCKER_CONFIG"] = os.environ.get("DOCKER_CONFIG", str(Path.home() / ".docker"))
    for key in ("DOCKER_HOST", "DOCKER_CONTEXT"):
        if key in os.environ:
            command_env[key] = os.environ[key]
    def execute(command, **kwargs):
        return runner.execute(command, env=command_env, **kwargs)
    def control(*arguments, timeout=15):
        with tempfile.TemporaryFile() as captured:
            execute(["docker", *arguments], stdout=captured, stderr=captured, timeout=timeout)
            captured.seek(0)
            result = captured.read(65537)
        if len(result) > 65536:
            raise ValueError("Recovery Docker output exceeds its bound")
        return result.decode()
    resources = OWNED.OwnedResources(runtime, control, recover=True)
    if resources.state["owner"] != record["owner"]:
        raise ValueError("Recovery owner differs from the resource ledger")
    try:
        for role in ("probe", "browser"):
            stop_retained_process(runtime, role, record["owner"])
        resources.remove("app")
        garage = runtime / "garage"
        if (garage / "state.json").is_file():
            helper = source / "scripts/private-storage-fixture.py"
            if helper.is_symlink() or hashlib.sha256(helper.read_bytes()).hexdigest() != record["storage_helper_sha256"]:
                raise ValueError("Original storage cleanup helper changed")
            with tempfile.TemporaryFile() as captured:
                execute(["python3", str(helper), "stop", "--runtime-dir", str(garage)],
                    stdout=captured, stderr=captured, timeout=30)
        resources.close()
        (output / "recovery.json").write_text(json.dumps({"confirmed": True, "owner": record["owner"],
            "engine": resources.state["engine"], "resources": resources.state["resources"]}, indent=2) + "\n")
        shutil.rmtree(runtime)
        print("Private deployment cleanup confirmed", flush=True)
        return 0
    except BaseException:
        (output / "recovery.json").write_text(json.dumps({"confirmed": False, "runtime": str(runtime),
            "owner": record["owner"], "resources": resources.state["resources"]}, indent=2) + "\n")
        raise


def private_environment(path, settings):
    with os.fdopen(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "w") as stream:
        for key, value in settings.items():
            if "\n" in value or "\r" in value:
                raise ValueError("A generated test environment value contains a newline")
            stream.write(key + "=" + value + "\n")


def close_private_run(runner, probe, service, runtime, primary):
    # Quiesce each producer before cleaning its dependent resources. A missing
    # creation receipt cannot establish that a process will not appear later.
    try:
        runner.cleanup_run(probe, None, None, primary)
        record_path = runtime / "probe-process.json"
        if record_path.exists():
            record = read_private_json(record_path)
            if probe is None and record["phase"] == "pending":
                raise RuntimeError("Local probe creation is unconfirmed")
            replace_private_json(record_path, {**record, "phase": "removed"})
    except BaseException as error:
        return [("private probe process", error)]
    if service is not None:
        try:
            stop_retained_process(runtime, "browser", service.owner)
        except BaseException as error:
            return [("private browser process", error)]
        try:
            service.close()
        except BaseException as error:
            return [("private deployment resources", error)]
    return []


class PrivateImageService(IMAGE.ImageService):
    def __init__(self, source, runtime, environment, listener, output, image, execute, mode):
        if mode not in ("disk", "bucket"):
            raise ValueError("Private deployment storage mode must be disk or bucket")
        super().__init__(source, runtime, environment, listener, output, image, execute)
        self.mode = mode
        self.tls = INGRESS.LocalTLS(runtime / "tls", execute)
        self.relay = IMAGE.Relay(listener, self.tls.server)
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}),
            urllib.request.HTTPSHandler(context=self.tls.client))
        self.resources = OWNED.OwnedResources(runtime, self.raw_command, owner=self.owner)
        self.network = self.name + "-network"
        self.pg_name = self.name + "-postgres"
        self.pg_volume = self.name + "-postgres-data"
        self.pg_password = secrets.token_urlsafe(32)
        self.garage = runtime / "garage"
        self.bucket_origin = None
        self.sensitive = [self.pg_password]
        self.restore_started = False
        self.helper = source / "scripts/private-storage-fixture.py"
        import hashlib
        private_json(runtime / "recovery.json", {"version": 2, "mode": mode, "source": str(source),
            "storage_helper_sha256": hashlib.sha256(self.helper.read_bytes()).hexdigest(), "owner": self.owner,
            "runner_pid": os.getpid()})

    def raw_command(self, *arguments, timeout=15):
        return super().command(*arguments, timeout=timeout)

    def create(self, key, kind, name, arguments, label=OWNED.LABEL, timeout=15):
        self.resources.begin(key, kind, name, label)
        receipt = self.raw_command(*arguments, timeout=timeout)
        self.resources.created(key, receipt)
        return receipt

    def command(self, *arguments, timeout=15):
        if arguments[:2] == ("volume", "create") and arguments[-1] == self.volume:
            return self.create("app-data", "volume", self.volume, arguments, "io.openbot.test-run", timeout)
        if arguments[0] == "run" and "--name" in arguments and arguments[arguments.index("--name") + 1] == self.name:
            return self.create("app", "container", self.name, arguments, "io.openbot.test-run", timeout)
        return self.raw_command(*arguments, timeout=timeout)

    def prepare(self):
        self.create("network", "network", self.network,
            ("network", "create", "--label", OWNED.LABEL + "=" + self.owner, self.network))
        self.create("postgres-data", "volume", self.pg_volume,
            ("volume", "create", "--label", OWNED.LABEL + "=" + self.owner, self.pg_volume))
        envfile = self.runtime / "postgres.env"
        private_environment(envfile, {"POSTGRES_USER": "doop_fixture", "POSTGRES_PASSWORD": self.pg_password,
            "POSTGRES_DB": "doop"})
        self.create("postgres", "container", self.pg_name,
            ("run", "--pull=never", "--detach", "--name", self.pg_name, "--label", OWNED.LABEL + "=" + self.owner,
                "--network", self.network, "--user", "postgres", "--read-only", "--cap-drop=ALL",
                "--security-opt=no-new-privileges", "--pids-limit=128", "--memory=512m", "--cpus=1",
                "--tmpfs=/tmp:rw,nosuid,noexec,size=32m,mode=1777",
                "--tmpfs=/var/run/postgresql:rw,nosuid,noexec,size=1m,mode=1777", "--env-file", str(envfile),
                "--mount", "type=volume,src=" + self.pg_volume + ",dst=/var/lib/postgresql",
                POSTGRES, "postgres", "-c", "log_statement=none"))
        deadline = time.monotonic() + 45
        while True:
            running = self.raw_command("container", "inspect", "--format", "{{.State.Running}}", self.pg_name)
            if running.strip() != "true":
                raise RuntimeError("Owned native PostgreSQL exited before readiness")
            try:
                self.raw_command("exec", self.pg_name, "pg_isready", "-h", "127.0.0.1", "-U", "doop_fixture", "-d", "doop")
                break
            except Exception:
                if time.monotonic() >= deadline:
                    raise TimeoutError("Owned native PostgreSQL readiness exceeded45seconds")
                time.sleep(0.1)
        version = self.raw_command("exec", self.pg_name, "psql", "-h", "/var/run/postgresql", "-U", "doop_fixture", "-d", "doop",
            "-At", "-c", "SHOW server_version")
        uid = self.raw_command("exec", self.pg_name, "id", "-u")
        if not version.startswith("18.6") or uid.strip() == "0":
            raise RuntimeError("Owned PostgreSQL runtime version/identity differs")
        self.environment["DATABASE_URL"] = "postgresql://doop_fixture:" + self.pg_password + "@" + self.pg_name + ":5432/doop"
        if self.mode == "bucket":
            with tempfile.TemporaryFile() as output:
                self.execute(["python3", str(self.helper), "start", "--runtime-dir", str(self.garage)],
                    stdout=output, stderr=output, timeout=90)
            state = json.loads((self.garage / "state.json").read_text())
            settings = json.loads((self.garage / "sdk.json").read_text())
            self.raw_command("network", "connect", self.network, state["name"])
            template = '{{ (index .NetworkSettings.Networks "' + self.network + '").IPAddress }}'
            address = self.raw_command("container", "inspect", "--format", template, state["name"]).strip()
            import ipaddress
            if not ipaddress.ip_address(address).is_private:
                raise RuntimeError("Owned storage endpoint is not on a private network")
            self.environment.update(BUCKET=settings["bucket"], ACCESS_KEY_ID=settings["key"],
                SECRET_ACCESS_KEY=settings["secret"], ENDPOINT="http://" + address + ":3900", REGION="garage")
            self.sensitive.extend([settings["key"], settings["secret"]])
            self.bucket_origin = settings["endpoint"] + "/" + settings["bucket"]
        (self.output / "postgres-runtime.json").write_text(json.dumps({"version": version.strip(), "uid": uid.strip(),
            "public_port": False, "database": "fresh native PostgreSQL with persistent owned volume"}, indent=2) + "\n")
        super().prepare()

    def stop(self):
        self.relay.target = None
        self.resources.remove("app")

    def restore_database(self):
        return RESTORE.restore(self)

    def close(self):
        try:
            # If app creation is still uncertain, retain its dependencies and
            # recovery state rather than allowing a late create to adopt new data.
            self.stop()
            if (self.garage / "state.json").is_file():
                with tempfile.TemporaryFile() as output:
                    self.execute(["python3", str(self.helper), "stop", "--runtime-dir", str(self.garage)],
                        stdout=output, stderr=output, timeout=30)
            self.resources.close()
        finally:
            self.relay.close()


def run(directory, output, mode, image_path, openbot_binary, openbot_database, ui_chrome, runner, suite="all"):
    if suite not in ("all", "e2e"):
        raise ValueError("Unknown private probe family")
    if openbot_binary is None or openbot_database is None:
        raise ValueError("Private lifecycle acceptance requires the explicit Openbot helper and owned PostgreSQL")
    source, manifest, identity = runner.select_source(directory, None, None, True)
    if openbot_binary is not None and not openbot_binary.is_file():
        raise ValueError("Openbot acceptance helper is unavailable")
    helper_digest = runner.digest(openbot_binary) if openbot_binary is not None else None
    if ui_chrome is None:
        ui_chrome = next((Path(path) for path in (
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "/usr/bin/google-chrome",
            "/usr/bin/chromium", "/usr/bin/chromium-browser") if Path(path).is_file()), None)
    if ui_chrome is None or not ui_chrome.is_file() or not os.access(ui_chrome, os.X_OK):
        raise ValueError("Private deployment requires installed Chrome/Chromium for human acceptance")
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    # The selected setup is a deliberately shared local Docker path. macOS's
    # default /var/folders temp directory need not be visible inside its VM.
    # Keep credentials outside the source checkout, under a fresh mode0700 child.
    runtime = Path(tempfile.mkdtemp(prefix=".private-run-", dir=directory))
    (runtime / "home/tmp").mkdir(parents=True)
    command_env = runner.clean_environment(runtime / "home")
    command_env["DOCKER_CONFIG"] = os.environ.get("DOCKER_CONFIG", str(Path.home() / ".docker"))
    for key in ("DOCKER_HOST", "DOCKER_CONTEXT"):
        if key in os.environ:
            command_env[key] = os.environ[key]
    def execute(command, **kwargs):
        return runner.execute(command, env=command_env, **kwargs)
    service = None
    probe = None
    primary = None
    number = 0
    image = None
    try:
        image = runner.image_identity(image_path, identity, runtime)
        for required in [POSTGRES] + (["dxflrs/garage:v2.5.0@sha256:677e5b758e4425a325b811c425c026bf35a4bf42f7ddbaa773de3f4fd779a5a5"] if mode == "bucket" else []):
            # Provision these pinned images before the network-restricted run.
            execute(["docker", "image", "inspect", "--format", "{{.Id}}", required],
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10)
        environment = runner.clean_environment(runtime / "home")
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            listener.listen()
            origin = "https://127.0.0.1:" + str(listener.getsockname()[1])
            import base64
            environment.update(PORT=str(listener.getsockname()[1]), BETTER_AUTH_URL=origin,
                BETTER_AUTH_SECRET=secrets.token_urlsafe(48),
                DOOP_TEST_IMAGE_ENCRYPTION_KEYS="test:" + base64.b64encode(secrets.token_bytes(32)).decode(),
                DOOP_TEST_SOURCE=str(source), DOOP_TEST_SOURCE_REVISION=runner.REVISION,
                DOOP_TEST_SOURCE_FINGERPRINT=identity["fingerprint"],
                DOOP_TEST_SEED_METADATA=str(runtime / "seed.json"),
                DOOP_TEST_WS_MODULE=str(source / "node_modules/ws/wrapper.mjs"),
                DOOP_TEST_PRIVATE_DEPLOYMENT=mode, DOOP_TEST_UI_CHROME=str(ui_chrome.resolve()),
                DOOP_TEST_PRIVATE_RUNTIME=str(runtime))
            if openbot_binary is not None:
                environment.update(DOOP_TEST_OPENBOT_BINARY=str(openbot_binary.resolve()),
                    DOOP_TEST_OPENBOT_DATABASE=openbot_database)
            service = PrivateImageService(source, runtime, environment, listener, output, image, execute, mode)
            environment.update(NODE_EXTRA_CA_CERTS=str(service.tls.ca), DOOP_TEST_TLS_SPKI=service.tls.spki,
                DOOP_TEST_PRIVATE_OWNER=service.owner, DOOP_TEST_OPENBOT_CA=str(service.tls.ca))
            service.prepare()
            if service.bucket_origin is not None:
                environment["DOOP_TEST_PRIVATE_BUCKET_ORIGIN"] = service.bucket_origin
            service.start(0)
            replace_private_json(runtime / "backend.json", {"version": 1, "generation": number,
                "origin": "http://127.0.0.1:" + str(service.relay.target[1])})
            def start_generation():
                nonlocal number
                number += 1
                service.start(number)
                replace_private_json(runtime / "backend.json", {"version": 1, "generation": number,
                    "origin": "http://127.0.0.1:" + str(service.relay.target[1])})
            def restart(bindings):
                service.stop()
                if bindings is not None:
                    environment["MANAGED_COMPANY_WORKSPACES"] = json.dumps(bindings, separators=(",", ":"))
                start_generation()
            def restore_database():
                record = service.restore_database()
                start_generation()
                private_json(output / "database-restore.json", record)
            with (runtime / "probe.log").open("wb") as log:
                process_record = {"version": 1, "phase": "pending", "pid": None, "owner": service.owner}
                private_json(runtime / "probe-process.json", process_record)
                probe = subprocess.Popen(["node", str(runner.SCRIPTS / "probe.mjs"), origin, str(output), suite,
                    "--doop-test-owner=" + service.owner],
                    cwd=runtime, env=environment, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                    stderr=log, start_new_session=True)
                replace_private_json(runtime / "probe-process.json", {**process_record, "phase": "created", "pid": probe.pid})
                code = runner.drive_probe(probe, restart, restore_database)
            report_path = output / "report.json"
            if not report_path.is_file():
                raise RuntimeError("Private deployment produced no acceptance report")
            result = json.loads(report_path.read_text())
            runner.reviewed_source(source, identity["fingerprint"], manifest)
            if openbot_binary is not None:
                if runner.digest(openbot_binary) != helper_digest:
                    raise ValueError("Openbot helper binary changed during the private run")
                result["openbot_helper_sha256"] = helper_digest
            result.update(setup=manifest, source_identity=identity, storage_mode=mode, restarts=number,
                deployment="disposable verified image/nativePostgreSQL/localTLS", image={key: image[key] for key in
                    ("image_id", "manifest_digest", "archive_sha256", "user")},
                tls={"native_client": "run-local CA/hostname verification", "browser": "exact generated server SPKI",
                    "openbot_helper": "run-local CA through test-only EndpointPolicy root" if openbot_binary is not None else "not selected",
                    "public_certificate_or_production_origin": False})
            report_path.write_text(json.dumps(result, indent=2) + "\n")
            if code != 0:
                return code
            validate_private_report(result, mode,
                ("live Openbot company connections and encrypted custody",) if suite == "all" else (), suite=suite)
            return code
    except BaseException as error:
        primary = error
        raise
    finally:
        failures = close_private_run(runner, probe, service, runtime, primary)
        if failures:
            (output / "cleanup-pending.json").write_text(json.dumps({"confirmed": False, "runtime": str(runtime),
                "owners": [name for name, _error in failures]}, indent=2) + "\n")
            cleanup = runner.CleanupFailure(failures)
            if primary is not None:
                raise primary from cleanup
            raise cleanup
        shutil.rmtree(runtime)
        private_json(output / "cleanup-receipt.json", {"version": 1, "confirmed": True, "runtime_removed": True})
