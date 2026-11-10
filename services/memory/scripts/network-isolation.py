"""Prove the enclosing network guard admits loopback fixtures and rejects egress, before any test
runs under it (adapted from OpenBot's scripts/tests/network-isolation.py). When
MEMORY_TEST_DATABASE_URL is set, its PostgreSQL server must also be reachable through the guard:
on CI it is a loopback-published container that the Linux rules admit by conntrack original
destination, and a guard that blocked it would otherwise surface only as unrelated test failures.
"""
import errno
import os
import socket
import urllib.parse

# An established local exchange proves the guard is not just disabling all sockets.
with socket.socket() as listener:
    listener.bind(("127.0.0.1", 0))
    listener.listen(1)
    listener.settimeout(2)
    with socket.create_connection(listener.getsockname(), timeout=2) as client:
        with listener.accept()[0] as server:
            client.sendall(b"fixture")
            server.settimeout(2)
            assert server.recv(7) == b"fixture"

# RFC 5737/3849 documentation addresses: no provider, auth header, DNS or HTTP request.
for family, destination in [
    (socket.AF_INET, ("192.0.2.1", 443)),
    (socket.AF_INET6, ("2001:db8::1", 443)),
]:
    with socket.socket(family) as client:
        client.settimeout(2)
        try:
            client.connect(destination)
        except OSError as error:
            assert error.errno in {
                errno.EPERM, errno.EACCES, errno.ECONNREFUSED,
                errno.ENETUNREACH, errno.EHOSTUNREACH, errno.EADDRNOTAVAIL,
            }, f"egress was not promptly rejected: {type(error).__name__} {error.errno}"
        else:
            raise AssertionError("external egress is permitted")

database = os.environ.get("MEMORY_TEST_DATABASE_URL", "")
reached = ""
if database:
    url = urllib.parse.urlsplit(database)
    host, port = url.hostname or "localhost", url.port or 5432
    # Only the TCP handshake: no credentials or SQL are sent.
    with socket.create_connection((host, port), timeout=5):
        reached = f"; test PostgreSQL at {host}:{port} reachable"
print(f"network guard: loopback allowed; IPv4 and IPv6 external destinations rejected{reached}")
