These rules govern the public SMTP edge in addition to `src/AGENTS.md`. Assume every command,
address, header, and byte in `DATA` is controlled by an attacker.

# Authentication verdicts come only from the trusted verifier

The edge MTA's DNS-backed SPF, DKIM, and DMARC results are authoritative. Never overwrite them from
`Authentication-Results`, `Received-SPF`, or any other header inside the submitted message. A
trusted upstream may supply verdicts only behind explicit configuration that identifies and
authenticates that upstream; otherwise delete or ignore its asserted results.

Parse verifier output once into an `AuthVerdict`-style enum at ingress. Authorization matches the
enum exhaustively and fails closed for absent, neutral, soft-fail, temporary-error, permanent-error,
unknown, or newly introduced variants unless a documented policy explicitly accepts one. Do not
carry security verdicts inward as `Option<String>`.

# Enforce the SMTP limits you advertise

Reject over-limit input while reading it; never buffer first and check later. Enforce message and
line bytes, declared `MAIL FROM SIZE`, recipient count, command count, idle/per-command/session
timeouts, DNS lookup timeout, per-IP connections, and a global connection/task semaphore. Apply
equivalent limits to IPv4 and IPv6, including any reputation checks.

Test limits at the boundary and one unit beyond it, plus slow clients, missing terminators, excess
recipients, and distributed connections that bypass a per-IP-only cap. The server's `SIZE`
advertisement must equal the enforced limit and agree with other ingress paths where practical.

# Reject synchronously when the answer is definitive

After `DATA`, return a 5xx response for hard rejections the receiver can determine confidently,
such as an unknown destination or ACL denial. Do not answer 250 and rely on a best-effort bounce;
that claims delivery succeeded and creates backscatter risk. For deliberately asynchronous or
soft failures, enqueue a durable, retryable notification and record enqueue failure.

# Never send credentials over a dangerous transport

Remote SMTP with credentials uses authenticated TLS with certificate validation. A plaintext or
`builder_dangerous` transport is permitted only for an explicitly configured local test relay and
must never receive production credentials. Build and share a timeout-configured transport so every
message does not pay a new TCP/TLS/AUTH handshake.

Treat display names as untrusted header data: construct them through the mail library's typed APIs
or escape/validate them before parsing. Free-text channel names must not inject or break `From`.
