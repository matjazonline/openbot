"""Run-local TLS material; no host trust-store or authentication changes."""
import base64
import hashlib
from pathlib import Path
import secrets
import ssl
import subprocess
import tempfile


class LocalTLS:
    def __init__(self, directory, execute):
        directory = Path(directory)
        directory.mkdir(mode=0o700)
        self.ca = directory / "ca.pem"
        ca_key = directory / "ca-key.pem"
        key = directory / "server-key.pem"
        certificate = directory / "server.pem"
        request = directory / "server.csr"
        extensions = directory / "server-extensions.cnf"
        extensions.write_text("basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\n"
            "extendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost,IP:127.0.0.1\n")
        commands = [
            ["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-sha256", "-days", "2",
                "-keyout", str(ca_key), "-out", str(self.ca), "-subj", "/CN=Doop local fixture " + secrets.token_hex(8),
                "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign"],
            ["openssl", "req", "-new", "-newkey", "rsa:2048", "-nodes", "-sha256", "-keyout", str(key),
                "-out", str(request), "-subj", "/CN=localhost"],
            ["openssl", "x509", "-req", "-in", str(request), "-CA", str(self.ca), "-CAkey", str(ca_key),
                "-CAcreateserial", "-sha256", "-days", "2", "-extfile", str(extensions), "-out", str(certificate)],
        ]
        with tempfile.TemporaryFile() as output:
            for command in commands:
                execute(command, stdout=output, stderr=output, timeout=15, umask=0o077)
        for private in (key, ca_key):
            private.chmod(0o600)
        self.server = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        self.server.minimum_version = ssl.TLSVersion.TLSv1_2
        self.server.load_cert_chain(certificate, key)
        self.client = ssl.create_default_context(cafile=str(self.ca))
        public = subprocess.check_output(["openssl", "x509", "-in", str(certificate), "-pubkey", "-noout"], timeout=5)
        encoded = subprocess.run(["openssl", "pkey", "-pubin", "-outform", "DER"], input=public,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=5, check=True).stdout
        self.spki = base64.b64encode(hashlib.sha256(encoded).digest()).decode("ascii")
