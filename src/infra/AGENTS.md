These rules govern application construction, configuration, database pools, event listeners, and
runtime lifecycle in addition to `src/AGENTS.md`.

# Initialize observability before fallible setup

Install tracing before loading/validating external configuration, applying migrations, creating
storage clients, or probing dependencies. Startup failures must reach structured platform logs.
Do not create an unbounded local log file in the container; use stdout/stderr or a configured
rotating sink, and ensure the default filter names this crate.

Liveness stays cheap and unconditional. Provide a separate readiness check for required service
dependencies such as Postgres, so traffic is not routed to a process that cannot serve it.

# Bound shared resources and external waits

Every client and pool has explicit connect/acquire/request/statement timeouts and lifecycle bounds.
Account for dedicated connections such as listeners when sizing the database pool. Set idle
transaction protection and choose maximum lifetime/idle settings compatible with the deployment
proxy. Reuse clients and transports rather than constructing a fresh pool per operation.

Configuration defaults exposed to the network are production-safe. Validate required secrets,
public URLs, limits, and mutually dependent settings at startup, but do not claim a dependency was
validated unless setup actually contacted or probed it.

# Supervise the process lifecycle

The owner that spawns HTTP, SMTP, event, scheduler, and worker loops retains their handles. On
SIGTERM, stop admission, broadcast cancellation, let in-flight work either finish within the drain
budget or release/cancel safely, and join every loop before returning from `main`. A timeout may
force shutdown, but it must emit which components remained and leave durable work recoverable.

Cancellation must reach nested operations, not merely the outer polling loop. Add lifecycle tests
for shutdown between iterations and during active work, including lease disposition and absence of
orphaned tasks.
