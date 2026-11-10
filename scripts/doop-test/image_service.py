"""Owned disposable image runtime. The TCP relay is local test infrastructure."""
import json
import os
from pathlib import Path
import secrets
import select
import socket
import tempfile
import threading
import time
import urllib.request
import urllib.parse


class Relay:
    def __init__(self, listener, tls_context=None):
        self.listener = listener
        self.tls_context = tls_context
        self.listener.settimeout(0.2)
        self.target = None
        self.stopping = threading.Event()
        self.capacity = threading.BoundedSemaphore(16)
        self.lock = threading.Lock()
        self.workers = set()
        self.streams = set()
        self.thread = threading.Thread(target=self.accept)

    def start(self):
        self.thread.start()

    def accept(self):
        while not self.stopping.is_set():
            try:
                client, _ = self.listener.accept()
            except socket.timeout:
                continue
            except OSError:
                return
            if not self.capacity.acquire(blocking=False):
                client.close()
                continue
            worker = threading.Thread(target=self.forward, args=(client,))
            with self.lock:
                self.workers.add(worker)
            try:
                worker.start()
            except BaseException:
                with self.lock:
                    self.workers.discard(worker)
                self.capacity.release()
                client.close()
                raise

    def forward(self, client):
        backend = None
        try:
            target = self.target
            if target is None:
                return
            with self.lock:
                self.streams.add(client)
            if self.tls_context is not None:
                client.settimeout(5)
                secured = self.tls_context.wrap_socket(client, server_side=True, do_handshake_on_connect=False)
                with self.lock:
                    self.streams.discard(client)
                    self.streams.add(secured)
                client = secured
                client.do_handshake()
            backend = socket.create_connection(target, timeout=5)
            for stream in (client, backend):
                stream.settimeout(1)
            with self.lock:
                self.streams.update((client, backend))
            deadline = time.monotonic() + 180
            activity = time.monotonic()
            transferred = 0
            while not self.stopping.is_set() and time.monotonic() < deadline:
                ready, _, _ = select.select([client, backend], [], [], 0.2)
                if self.tls_context is not None and client.pending() and client not in ready:
                    ready.append(client)
                if not ready and time.monotonic() - activity > 10:
                    return
                for stream in ready:
                    data = stream.recv(65536)
                    if not data:
                        return
                    transferred += len(data)
                    if transferred > 4 * 1024 * 1024:
                        return
                    (backend if stream is client else client).sendall(data)
                    activity = time.monotonic()
        except OSError:
            pass  # Closing an owned tunnel is the bounded failure signal.
        finally:
            with self.lock:
                self.streams.discard(client)
                self.streams.discard(backend)
                self.workers.discard(threading.current_thread())
            client.close()
            if backend is not None:
                backend.close()
            self.capacity.release()

    def close(self):
        self.stopping.set()
        self.target = None
        if self.thread.ident is not None:
            self.thread.join(timeout=2)
        with self.lock:
            streams = list(self.streams)
            workers = list(self.workers)
        for stream in streams:
            try:
                stream.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
        deadline = time.monotonic() + 6
        for worker in workers:
            worker.join(timeout=max(0, deadline - time.monotonic()))
        if self.thread.is_alive() or any(worker.is_alive() for worker in workers):
            raise RuntimeError("Owned image relay did not stop")


class ImageService:
    def __init__(self, source, runtime, environment, listener, output, image, execute):
        self.source = source
        self.runtime = runtime
        self.environment = environment
        self.output = output
        self.image = image
        self.execute = execute
        self.owner = secrets.token_hex(16)
        self.name = "openbot-doop-test-" + self.owner
        self.volume = self.name + "-data"
        self.relay = Relay(listener)
        self.last_control_error = ""
        self.network = None
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def command(self, *arguments, timeout=15):
        with tempfile.TemporaryFile() as captured:
            try:
                self.execute(["docker", *arguments], timeout=timeout, stdout=captured, stderr=captured)
            except BaseException:
                captured.seek(0)
                self.last_control_error = captured.read(16384).decode("utf-8", errors="replace")
                raise
            captured.seek(0)
            result = captured.read(65537)
        if len(result) > 65536:
            raise ValueError("Docker test control output exceeds its bound")
        return result.decode("utf-8")

    def owned(self, kind, field):
        values = self.command(kind, "ls", *(["--all"] if kind == "container" else []),
            "--filter", "label=io.openbot.test-run=" + self.owner, "--format", "{{." + field + "}}").splitlines()
        if len(values) > 1:
            raise RuntimeError("Unexpected duplicate owned image resource")
        return values

    def prepare(self):
        self.relay.start()
        self.command("volume", "create", "--label", "io.openbot.test-run=" + self.owner, self.volume)

    def start(self, number):
        allowed = {"PORT", "BETTER_AUTH_URL", "BETTER_AUTH_SECRET", "MANAGED_COMPANY_WORKSPACES",
            "REQUIRE_EMAIL_VERIFICATION", "RESIDENT_TASK_LIMIT", "DO_NOT_TRACK", "DATABASE_URL",
            "BUCKET", "ACCESS_KEY_ID", "SECRET_ACCESS_KEY", "ENDPOINT", "REGION"}
        settings = {key: value for key, value in self.environment.items() if key in allowed}
        settings["NODE_ENV"] = "production"
        # Same key and private data volume across both controlled restarts.
        settings["SECRETS_ENCRYPTION_KEYS"] = self.environment["DOOP_TEST_IMAGE_ENCRYPTION_KEYS"]
        settings.update({"DOOP_TEST_SOURCE":"/app", "DOOP_TEST_SEED_METADATA":"/app/data/.openbot-test-seed.json",
            "DOOP_TEST_OBSERVE_ONLY":"1"})
        envfile = self.runtime / "container.env"
        with os.fdopen(os.open(envfile, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600), "w") as stream:
            stream.write("".join(key + "=" + value + "\n" for key, value in settings.items()))
        try:
            port = settings["PORT"]
            self.command("run", "--pull=never", "--detach", "--init", "--name", self.name,
                *(["--network", self.network] if self.network is not None else []),
                "--label", "io.openbot.test-run=" + self.owner,
                "--read-only", "--cap-drop=ALL", "--security-opt=no-new-privileges",
                "--security-opt=seccomp=" + str(self.source / "deploy/chromium-seccomp.json"),
                "--pids-limit=256", "--memory=2g", "--cpus=2", "--shm-size=256m",
                "--tmpfs=/tmp:rw,nosuid,noexec,size=256m",
                "--mount", "type=volume,src=" + self.volume + ",dst=/app/data",
                "--mount", "type=bind,src=" + str(Path(__file__).with_name('loopback.mjs')) + ",dst=/test/observe.mjs,readonly",
                "--publish", "127.0.0.1::" + port, "--env-file", str(envfile), self.image["image_id"],
                "node", "--import", "tsx", "--import", "/test/observe.mjs", "server/index.ts")
            bindings = json.loads(self.command("container", "inspect", "--format",
                '{{json (index .NetworkSettings.Ports "' + port + '/tcp")}}', self.name))
            if len(bindings) != 1 or bindings[0]["HostIp"] != "127.0.0.1":
                raise RuntimeError("Image test port is not loopback-only")
            self.relay.target = ("127.0.0.1", int(bindings[0]["HostPort"]))
            deadline = time.monotonic() + 90
            while time.monotonic() < deadline:
                if self.command("container", "inspect", "--format", "{{.State.Running}}", self.name).strip() != "true":
                    raise RuntimeError("Local Doop image exited before readiness")
                try:
                    with self.opener.open(settings["BETTER_AUTH_URL"] + "/healthz", timeout=1) as response:
                        if json.load(response) == {"ok": True}:
                            metadata = self.command("exec", self.name, "node", "--input-type=module", "-e",
                                "import fs from 'node:fs'; for(let n=0;n<50&&!fs.existsSync('/app/data/.openbot-test-seed.json');n++) await new Promise(r=>setTimeout(r,100)); process.stdout.write(fs.readFileSync('/app/data/.openbot-test-seed.json')); ")
                            value = json.loads(metadata)
                            if set(value) != {"ownerlessCanvasIds"} or not isinstance(value["ownerlessCanvasIds"], list) or len(value["ownerlessCanvasIds"]) > 128:
                                raise ValueError("Invalid read-only seed metadata")
                            (self.runtime / "seed.json").write_text(json.dumps(value))
                            if number == 0:
                                self.runtime_check()
                            return
                except (OSError, ValueError):
                    pass
                time.sleep(0.2)
            raise TimeoutError("Local Doop image readiness exceeded 90 seconds")
        except BaseException as original:
            try:
                try:
                    if isinstance(original, (RuntimeError, TimeoutError)):
                        diagnostic = self.command("logs", "--tail=80", self.name)[-16384:] + "\n" + self.last_control_error
                        sensitive = [settings.get(key) for key in ("BETTER_AUTH_SECRET", "SECRETS_ENCRYPTION_KEYS",
                            "DATABASE_URL", "ACCESS_KEY_ID", "SECRET_ACCESS_KEY")]
                        if settings.get("DATABASE_URL"):
                            sensitive.append(urllib.parse.urlsplit(settings["DATABASE_URL"]).password)
                        for value in sensitive:
                            if value:
                                diagnostic = diagnostic.replace(value, "[redacted]")
                        (self.output / ("image-startup-" + str(number) + ".txt")).write_text(diagnostic)
                finally:
                    self.stop()
            except BaseException as cleanup:
                raise original from cleanup
            raise

    def runtime_check(self):
        script = r"""
import fs from 'node:fs';
import { spawnSync } from 'node:child_process';
import { launchBrowser } from './server/screenshot.ts';
const status = Object.fromEntries(fs.readFileSync('/proc/self/status','utf8').split('\n').filter(line=>/^(CapEff|NoNewPrivs|Seccomp):/.test(line)).map(line=>line.split(/:\s*/)));
if(process.getuid()===0 || !/^0+$/.test(status.CapEff) || status.NoNewPrivs.trim()!=='1' || status.Seccomp.trim()!=='2') throw new Error('runtime privileges differ');
if(!fs.existsSync('/usr/sbin/chroot')) throw new Error('parent chroot diagnostic unavailable');
const parentChroot=spawnSync('/usr/sbin/chroot',['/','/usr/bin/true'],{encoding:'utf8',timeout:2000});
if(parentChroot.error || parentChroot.status!==125 || !/Operation not permitted/.test(parentChroot.stderr)) throw new Error('parent chroot was not denied');
let writable=false;
try { fs.writeFileSync('/app/.openbot-write-check','x',{flag:'wx'}); writable=true; fs.unlinkSync('/app/.openbot-write-check'); } catch {}
if(writable) throw new Error('application directory is writable');
fs.writeFileSync('/app/data/.runtime-write-check','x'); fs.unlinkSync('/app/data/.runtime-write-check');
const browser=await launchBrowser();
let result;
try {
  const page=await browser.newPage(); await page.goto('chrome://sandbox');
  const sandbox=await page.evaluate(()=>document.body.innerText);
  if(!/PID namespaces\s+Yes/.test(sandbox)||!/Network namespaces\s+Yes/.test(sandbox)||!/Seccomp-BPF sandbox\s+Yes/.test(sandbox)) throw new Error('Chromium sandbox is unavailable');
  result={uid:process.getuid(),status,parentChrootDenied:true,applicationWritable:writable,sandbox};
} finally { await browser.close(); }
// Service imports can own periodic timers. This one-off diagnostic owns its
// entire process, has finished every assertion and closed its real browser.
// Flush the receipt before ending it; do not wait on unrelated service timers.
await new Promise((resolve,reject)=>process.stdout.write(JSON.stringify({...result,
  resourcesAfterBrowserClose:process.getActiveResourcesInfo()})+'\n',error=>error?reject(error):resolve()));
process.exit(0);
"""
        try:
            raw = self.command("exec", self.name, "node", "--import", "tsx", "--input-type=module", "-e", script)
            result = json.loads(raw)
            (self.output / "image-runtime.json").write_text(json.dumps(result, indent=2) + "\n")
        except Exception as error:
            raise RuntimeError("Image privilege/renderer diagnostics failed") from error

    def stop(self):
        self.relay.target = None
        for identifier in self.owned("container", "ID"):
            self.command("stop", "--time=10", identifier)
            self.command("rm", identifier)

    def close(self):
        try:
            try:
                self.stop()
            finally:
                for name in self.owned("volume", "Name"):
                    if name != self.volume:
                        raise RuntimeError("Image volume identity differs")
                    self.command("volume", "rm", name)
        finally:
            self.relay.close()
