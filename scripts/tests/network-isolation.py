"""Prove the enclosing OS guard permits fixtures and rejects egress, without a live service."""
import errno
import socket

# An established local exchange proves that the guard is not just disabling all sockets.
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
                errno.ENETUNREACH, errno.EHOSTUNREACH,
            }, f"egress was not promptly rejected: {type(error).__name__}"
        else:
            raise AssertionError("external egress is permitted")
print("network guard: loopback allowed; IPv4 and IPv6 external destinations rejected")
