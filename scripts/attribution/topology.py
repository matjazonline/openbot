#!/usr/bin/env python3
"""Owned local containers; real Openbot/attribution HTTPS integration, no production data."""
import base64
import contextlib
import http.server
import json
import os
import re
from pathlib import Path
import socket
import ssl
import subprocess
import tempfile
import threading
import time
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[2]
OPENBOT = os.environ.get("ATTRIBUTION_OPENBOT_TEST_IMAGE", "as-openbot-integration:local")
ATTRIBUTION = os.environ.get("ATTRIBUTION_TEST_IMAGE", "as-attribution-production:local")
POSTGRES = "postgres:18.3-bookworm"
NGINX = "nginxinc/nginx-unprivileged:1.28.0-bookworm"
# Public, deliberately reusable test credential, generated with Argon2id.
PASSWORD = "attribution-topology-test-password"
PASSWORD_HASH = "$argon2id$v=19$m=65536,t=3,p=4$154Hq7fy0UuUSMQq4omXog$K9bGbbPo2N1+lH+4gol401N//rLvsfPslChSikKtvVg"


def run(*args, input=None, check=True, timeout=180):
    r = subprocess.run(args, input=input, capture_output=True, text=True, timeout=timeout)
    if check and r.returncode:
        # Container args/config/logs can contain credentials. Never print them here.
        raise RuntimeError(f"fixture command failed: {args[0]} {args[1]} (exit {r.returncode})")
    return r


def docker(*args, **kwargs):
    return run("docker", *args, **kwargs).stdout.strip()


def secret():
    return base64.urlsafe_b64encode(os.urandom(32)).decode().rstrip("=")


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class Fixture:
    def __init__(self, directory, mode, images, ca, pin):
        self.directory, self.mode, self.images, self.ca, self.pin = directory, mode, images, ca, pin
        self.name = "as_topology_" + uuid.uuid4().hex
        self.containers, self.networks = [], []
        self.secrets = []
        self.port = free_port()
        self.origin = f"https://attribution.fixture.test:{self.port}"
        self.openbot = f"https://openbot.fixture.test:{self.port}"
        self.user, self.company = str(uuid.uuid4()), str(uuid.uuid4())
        self.username = "topology-" + uuid.uuid4().hex
        self.client, self.lifecycle, self.key = secret(), secret(), secret()
        self.passwords = {app: uuid.uuid4().hex for app in ("ob", "as")}
        self.control_secret = secret()
        self.stage = "initialization"

    def network(self, suffix, internal=False):
        name = self.name + "_" + suffix
        docker("network", "create", *( ["--internal"] if internal else []), name)
        self.networks.append(name)
        return name

    def container(self, suffix, image, *args, command=()):
        name = self.name + "_" + suffix
        docker("create", "--name", name, *args, image, *command)
        self.containers.append(name)
        return name

    def ip(self, container, network):
        state = json.loads(docker("inspect", container))[0]
        return state["NetworkSettings"]["Networks"][network]["IPAddress"]

    def envfile(self, app, values):
        path = self.directory / (self.name + app + ".env")
        self.secrets.extend(str(v) for v in values.values() if len(str(v)) >= 16)
        path.write_text("".join(f"{k}={v}\n" for k, v in values.items()))
        path.chmod(0o600)
        return str(path)

    def sql(self, app, statement, database=None):
        return docker("exec", "-i", self.db[app], "psql", "-U", "postgres", "-d", database or app, "-X", "-At", "-v", "ON_ERROR_STOP=1", input=statement)

    def wait_http(self, host, path, expected=200):
        context = ssl.create_default_context(cafile=str(self.ca))
        context.verify_flags |= ssl.VERIFY_X509_STRICT
        until = time.monotonic() + 80
        while time.monotonic() < until:
            app=self.apps["as" if host.startswith("attribution.") else "ob"]
            if docker("inspect",app,"--format","{{.State.Status}}") == "exited":
                raise RuntimeError("application exited before readiness")
            try:
                req = urllib.request.Request(f"https://127.0.0.1:{self.port}{path}", headers={"Host": host})
                with urllib.request.urlopen(req, context=context, timeout=3) as r:
                    if r.status == expected:
                        return
            except (OSError, urllib.error.URLError):
                pass
            time.sleep(.2)
        raise RuntimeError("readiness deadline: " + self.stage)

    def start(self):
        self.stage = "networks and private databases"
        self.net = {app: self.network(app, internal=self.mode == "separate-networks") for app in ("ob", "as")}
        self.edge = self.network("edge")
        self.db = {}
        for app in ("ob", "as"):
            env = self.envfile(app + "db", {"POSTGRES_PASSWORD": self.passwords[app]})
            self.db[app] = self.container(app + "db", POSTGRES, "--network", self.net[app], "--network-alias", app + "db", "--env-file", env)
            docker("start", self.db[app])
            for _ in range(100):
                ready = run("docker", "exec", self.db[app], "pg_isready", "-h", "127.0.0.1", "-U", "postgres", check=False)
                if ready.returncode == 0:
                    break
                time.sleep(.2)
            else:
                raise RuntimeError("fixture PostgreSQL did not start")
            self.sql(app, f"CREATE ROLE \"{app}\" LOGIN PASSWORD '{self.passwords[app]}' NOSUPERUSER NOCREATEDB NOCREATEROLE; CREATE DATABASE \"{app}\" OWNER \"{app}\"; REVOKE ALL ON DATABASE \"{app}\" FROM PUBLIC;", "postgres")
        self.gateway = self.container("gateway", NGINX, "--network", self.edge, "--publish", f"127.0.0.1:{self.port}:{self.port}", "--entrypoint", "nginx", command=("-g", "daemon off;"))
        for net in self.net.values():
            docker("network", "connect", net, self.gateway)
        docker("start", self.gateway)
        self.sql("ob", "BEGIN; ALTER ROLE ob SUPERUSER; SET LOCAL ROLE ob; CREATE EXTENSION IF NOT EXISTS pg_stat_statements; RESET ROLE; ALTER ROLE ob NOSUPERUSER; COMMIT;")
        self.apps = {}
        for app in ("ob", "as"):
            address = self.ip(self.gateway, self.edge if self.mode == "containers" else self.net[app])
            values = {
                "DATABASE_URL": f"postgres://ob:{self.passwords['ob']}@obdb/ob", "JWT_SECRET": secret(), "AUTH_ATTEMPT_SECRET": secret(),
                "APP_DOMAIN_NAME": self.openbot.removeprefix("https://"), "COOKIE_SECURE": "true", "INCOMING_SMTP_ENABLED": "false",
                "CREDENTIAL_ENCRYPTION_KEYS": "1:" + base64.b64encode(os.urandom(32)).decode(), "CREDENTIAL_ENCRYPTION_ACTIVE_VERSION": "1",
                "OPENBOT_ATTRIBUTION_ORIGIN": self.origin, "OPENBOT_ATTRIBUTION_CLIENT_SECRET": self.client, "OPENBOT_ATTRIBUTION_LIFECYCLE_SECRET": self.lifecycle,
            } if app == "ob" else {
                "ATTRIBUTION_DATABASE_URL": f"postgres://as:{self.passwords['as']}@asdb/as", "ATTRIBUTION_BIND_ADDR": "0.0.0.0:8091",
                "ATTRIBUTION_PUBLIC_ORIGIN": self.origin, "ATTRIBUTION_OPENBOT_ORIGIN": self.openbot,
                "ATTRIBUTION_CLIENT_SECRET": self.client, "ATTRIBUTION_LIFECYCLE_SECRET": self.lifecycle,
                "ATTRIBUTION_SESSION_KEYS": json.dumps({"active": 1, "keys": [{"version": 1, "key": self.key}]}),
                "ATTRIBUTION_EVENT_MAX_AGE_DAYS": 7, "ATTRIBUTION_REFUND_WINDOW_DAYS": 365,
                "ATTRIBUTION_CLICK_RETENTION_DAYS": 38, "ATTRIBUTION_OBSERVATION_RETENTION_DAYS": 30, "ATTRIBUTION_FINANCIAL_RETENTION_DAYS": 400,
                "ATTRIBUTION_EVENT_RETENTION_DAYS": 8, "ATTRIBUTION_AUDIT_RETENTION_DAYS": 90,
            }
            self.apps[app] = self.container(app, self.images[app], "--network", self.net[app], "--network-alias", app + "app",
                "--add-host", "openbot.fixture.test:" + address, "--add-host", "attribution.fixture.test:" + address,
                "--env-file", self.envfile(app, values), "--read-only", "--tmpfs", "/tmp:rw,noexec,nosuid,size=64m", "--cap-drop", "ALL", "--security-opt", "no-new-privileges")
            if self.mode == "containers":
                docker("network", "connect", self.edge, self.apps[app])
        for app in self.apps.values():
            docker("start", app)
        self.configure_gateway()
        docker("exec", self.gateway, "nginx", "-s", "reload")
        self.stage = "actual applications and TLS readiness"
        self.wait_http(self.openbot.removeprefix("https://"), "/login")
        self.wait_http(self.origin.removeprefix("https://"), "/readyz", 204)
        self.sql("ob", f"BEGIN; INSERT INTO users(id,username,email,password_hash,email_verified_at) VALUES('{self.user}','{self.username}','{self.username}@example.test','{PASSWORD_HASH}',clock_timestamp()); INSERT INTO user_login_methods(user_id,provider) VALUES('{self.user}','password'); INSERT INTO companies(id,user_id,name,slug) VALUES('{self.company}','{self.user}','Topology fixture','fixture-{uuid.uuid4().hex}'); INSERT INTO company_members(id,company_id,user_id,role,access_level) VALUES(gen_random_uuid(),'{self.company}','{self.user}','owner',9); COMMIT;")

    def configure_gateway(self):
        source_net = self.edge if self.mode == "containers" else None
        attr = self.ip(self.apps["as"], source_net or self.net["as"])
        ob = self.ip(self.apps["ob"], source_net or self.net["ob"])
        config = f'''pid /tmp/nginx.pid;
events {{ worker_connections 128; }}
http {{
 access_log off; error_log /dev/stderr warn;
 client_body_temp_path /tmp/client_temp;
 proxy_temp_path /tmp/proxy_temp;
 map "$remote_addr:$uri" $blocked_ob {{ default 0; ~^{attr}:/api/attribution/v1/ 0; ~^{attr}: 1; }}
 map "$remote_addr:$uri" $blocked_as {{ default 0; ~^{ob}:/internal/v1/company-lifecycle$ 0; ~^{ob}: 1; }}
 server {{ listen {self.port} ssl; server_name openbot.fixture.test;
  ssl_certificate /etc/nginx/fixture.crt; ssl_certificate_key /etc/nginx/fixture.key;
  if ($blocked_ob) {{ return 403; }}
  location / {{ proxy_pass http://obapp:3001; proxy_set_header Host $http_host; proxy_read_timeout 15s; }}
 }}
 server {{ listen {self.port} ssl; server_name attribution.fixture.test;
  ssl_certificate /etc/nginx/fixture.crt; ssl_certificate_key /etc/nginx/fixture.key;
  if ($blocked_as) {{ return 403; }}
  location / {{ proxy_pass http://asapp:8091; proxy_set_header Host $http_host; proxy_read_timeout 15s; }}
 }}
}}
'''
        conf = self.directory / (self.name + ".conf")
        conf.write_text(config)
        docker("cp", str(conf), self.gateway + ":/etc/nginx/nginx.conf")
        docker("cp", str(self.directory / "fixture.crt"), self.gateway + ":/etc/nginx/fixture.crt")
        docker("cp", str(self.directory / "fixture.key"), self.gateway + ":/etc/nginx/fixture.key")

    def boundaries(self):
        for source, target in [("as", "ob"), ("ob", "as")]:
            ports=[(self.db[target],self.net[target],5432)]
            if self.mode=="separate-networks":
                ports.append((self.apps[target],self.net[target],3001 if target=="ob" else 8091))
            for container,network,port in ports:
                ip=self.ip(container,network)
                r=run("docker","run","--rm","--network","container:"+self.apps[source],"--entrypoint","bash",POSTGRES,"-c",f"timeout 2 bash -c 'exec 3<>/dev/tcp/{ip}/{port}'",check=False)
                if r.returncode==0: raise RuntimeError("private network boundary allowed a connection")
            state=json.loads(docker("inspect",self.apps[source]))[0]
            assert state["HostConfig"]["ReadonlyRootfs"] and not state["Mounts"]
        if self.mode=="separate-networks":
            for source,host,path in [("as","openbot.fixture.test","/login"),("ob","attribution.fixture.test","/readyz")]:
                address=self.ip(self.gateway,self.net[source])
                response=run("docker","run","--rm","--interactive","--network","container:"+self.apps[source],"--entrypoint","openssl",self.images[source],"s_client","-quiet","-connect",f"{address}:{self.port}","-servername",host,"-verify_hostname",host,"-verify_return_error",input=f"GET {path} HTTP/1.1\r\nHost: {host}:{self.port}\r\nConnection: close\r\n\r\n",check=False,timeout=15)
                assert "HTTP/1.1 403" in response.stdout,"gateway must deny non-bridge traffic from each application namespace"
        address=self.ip(self.gateway,self.edge if self.mode=="containers" else self.net["as"])
        r=run("docker","run","--rm","--network","container:"+self.apps["as"],"--entrypoint","openssl",ATTRIBUTION,"s_client","-connect",f"{address}:{self.port}","-servername","openbot.fixture.test","-verify_hostname","openbot.fixture.test","-verify_return_error",input="",check=False)
        assert r.returncode!=0,"unknown CA must fail TLS verification"
        assert "certificate verify failed" in r.stderr or "verify error" in r.stderr
        print("PASS network namespaces: private database ports denied; no filesystem mounts; untrusted TLS denied" + ("; cross-application plaintext ports and non-bridge HTTPS paths denied" if self.mode=="separate-networks" else ""),flush=True)

    def action(self, body):
        action = body.get("action")
        if action == "commerce-clock":
            # Fixture chronology follows the database that timestamps clicks;
            # host and container clocks can differ after a VM restart.
            return {"now": self.sql("as", "SELECT to_char(clock_timestamp() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"');")}
        elif action == "verify-fixture-domain":
            domain = str(uuid.UUID(body["id"]))
            self.sql("as", f"UPDATE attribution_domains SET verified_until=clock_timestamp()+interval '1 day' WHERE id='{domain}';")
        elif action == "authority-outage":
            docker("stop", "--time", "25", self.apps["ob"])
        elif action == "expire-status":
            self.sql("as", "UPDATE attribution_tenants SET status_valid_until=clock_timestamp()-interval '1 second';")
        elif action == "cold-restart":
            docker("restart", "--time", "8", self.apps["as"])
            self.wait_http(self.origin.removeprefix("https://"), "/healthz", 204)
        elif action == "authority-recover":
            docker("start", self.apps["ob"])
            self.wait_http(self.openbot.removeprefix("https://"), "/login")
        else:
            raise RuntimeError("unknown fixture control")
        return {"ok": True}

    def browser(self):
        fixture = self
        class Control(http.server.BaseHTTPRequestHandler):
            def do_POST(self):
                if self.path != "/" + fixture.control_secret:
                    self.send_error(404); return
                try:
                    size = int(self.headers.get("Content-Length", "0"))
                    if not 0 < size < 2048:
                        raise ValueError()
                    result = fixture.action(json.loads(self.rfile.read(size)))
                    payload = json.dumps(result).encode()
                    self.send_response(200); self.end_headers(); self.wfile.write(payload)
                except Exception:
                    self.send_error(500, "fixture control unavailable")
            def log_message(self, *_): pass
        server = http.server.HTTPServer(("127.0.0.1", 0), Control)
        thread = threading.Thread(target=server.serve_forever, daemon=True); thread.start()
        try:
            env = dict(os.environ, ATTRIBUTION_PUBLIC_ORIGIN=self.origin, ATTRIBUTION_FIXTURE_OPENBOT_ORIGIN=self.openbot,
                ATTRIBUTION_FIXTURE_USERNAME=self.username, ATTRIBUTION_FIXTURE_PASSWORD=PASSWORD, ATTRIBUTION_FIXTURE_USER_ID=self.user,
                ATTRIBUTION_FIXTURE_COMPANY=self.company, ATTRIBUTION_FIXTURE_CERT_PIN=self.pin, ATTRIBUTION_FIXTURE_CA=str(self.ca),
                ATTRIBUTION_FIXTURE_CONTROL=f"http://127.0.0.1:{server.server_port}/{self.control_secret}")
            result = subprocess.run(["node", str(ROOT / "services/attribution/browser/topology.mjs")], env=env, capture_output=True, text=True, timeout=180)
            print(result.stdout, end="", flush=True)
            if result.returncode:
                # Browser script deliberately reports only a fixed stage name.
                raise RuntimeError(result.stderr.strip())
        finally:
            server.shutdown(); server.server_close(); thread.join()

    def diagnose(self):
        for container in self.containers:
            logs=run("docker","logs","--tail","12",container,check=False)
            value=logs.stdout+logs.stderr
            for item in sorted(self.secrets,key=len,reverse=True): value=value.replace(item,"[redacted]")
            value=re.sub(r"(?:postgres(?:ql)?|https?)://[^\s]+","[url]",value)
            value=re.sub(r"notes.*?\n", "[plan note list omitted]\n", value)
            print("Fixture diagnostic "+container.rsplit("_",1)[-1]+": "+value,flush=True)

    def close(self):
        for container in reversed(self.containers):
            run("docker", "rm", "--force", "--volumes", container, check=False)
        for network in reversed(self.networks):
            run("docker", "network", "rm", network, check=False)


def main():
    with tempfile.TemporaryDirectory(prefix="as-topology-") as temp:
        directory = Path(temp)
        ca = directory / "ca.crt"
        run("openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", str(directory / "ca.key"), "-out", str(ca), "-days", "1", "-subj", "/CN=attribution topology root", "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign", "-addext", "subjectKeyIdentifier=hash")
        run("openssl", "req", "-new", "-newkey", "rsa:2048", "-nodes", "-keyout", str(directory / "fixture.key"), "-out", str(directory / "fixture.csr"), "-subj", "/CN=openbot.fixture.test")
        extensions=directory / "extensions.conf"
        extensions.write_text("basicConstraints=critical,CA:FALSE\nsubjectKeyIdentifier=hash\nauthorityKeyIdentifier=keyid:always,issuer\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:openbot.fixture.test,DNS:attribution.fixture.test,IP:127.0.0.1\n")
        run("openssl", "x509", "-req", "-in", str(directory / "fixture.csr"), "-CA", str(ca), "-CAkey", str(directory / "ca.key"), "-CAcreateserial", "-out", str(directory / "fixture.crt"), "-days", "1", "-extfile", str(extensions))
        # This is an ephemeral fixture key inside a private temporary directory.
        (directory / "fixture.key").chmod(0o644)
        public = run("openssl", "x509", "-in", str(directory / "fixture.crt"), "-pubkey", "-noout").stdout
        der = subprocess.run(["openssl", "pkey", "-pubin", "-outform", "DER"], input=public.encode(), capture_output=True, check=True).stdout
        import hashlib
        pin = base64.b64encode(hashlib.sha256(der).digest()).decode()
        images = {}
        try:
            for app, base, user in [("ob", OPENBOT, "mailagents"), ("as", ATTRIBUTION, "attribution:attribution")]:
                tag = "as-topology-trust-" + uuid.uuid4().hex
                images[app] = tag
                dockerfile = directory / (app + ".Dockerfile")
                dockerfile.write_text(f"FROM {base}\nUSER root\nCOPY ca.crt /usr/local/share/ca-certificates/fixture.crt\nRUN update-ca-certificates\nUSER {user}\n")
                docker("build", "--tag", tag, "--file", str(dockerfile), str(directory))
                print(json.dumps({"application": app, "base_image": docker("image", "inspect", base, "--format", "{{.Id}}"), "trusted_image": docker("image", "inspect", tag, "--format", "{{.Id}}")}), flush=True)
            for mode in ["containers", "separate-networks"]:
                f = Fixture(directory, mode, images, ca, pin)
                try:
                    print("Topology: " + mode, flush=True)
                    f.start(); f.boundaries(); f.browser()
                except Exception:
                    f.diagnose()
                    raise
                finally:
                    f.close()
        finally:
            for image in images.values():
                run("docker", "image", "rm", image, check=False)


if __name__ == "__main__":
    main()
