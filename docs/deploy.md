# Deploying to Fly.io

The deployment is **two Fly apps in the same organization**:

| App | Config | Role |
| --- | --- | --- |
| `mail-agents-server` | `fly.toml` | axum API (3001) + inbound SMTP listener (2525) + task worker |
| `mail-agents-db` | `deploy/postgres/fly.toml` | self-hosted Postgres on a volume, no public IP |

They talk over Fly's private 6PN network, so the database is never exposed to
the internet.

## Prerequisites

- `flyctl` installed and authenticated (`fly auth login`).
- A dedicated IPv4 address for the app (see [Inbound mail](#inbound-mail)).
- A domain you control, for `APP_DOMAIN_NAME` and its MX record.

## Build prerequisite: `.sqlx`

The persistence layer uses `sqlx::query!` macros, which are validated against a
real Postgres **at compile time**. The Docker build has no database, so it
relies on the checked-in `.sqlx/` directory with `SQLX_OFFLINE=true`.

**Whenever you add or change a SQL query, regenerate it and commit the result:**

```sh
DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents" \
  cargo sqlx prepare -- --all-targets
```

Needs `cargo install sqlx-cli --no-default-features --features postgres,rustls`.

Note that the `DATABASE_URL` in `.env` omits the username; sqlx-cli 0.9 defaults
a missing username to a role called `anonymous` and fails with
`role "anonymous" does not exist`, hence the explicit `$(whoami)` above.

Verify the offline build the same way CI or Fly would:

```sh
env -u DATABASE_URL SQLX_OFFLINE=true cargo check --locked
```

If this fails, `fly deploy` will fail the same way.

## First deploy

`scripts/deploy.sh --bootstrap` automates both apps' first-time setup (app
creation, volume, dedicated IPv4, and prompting for secrets), then deploys:

```sh
scripts/deploy.sh --bootstrap        # db, then app
scripts/deploy.sh --bootstrap db     # db only
scripts/deploy.sh --bootstrap app    # app only
```

It's a no-op (skipped with a message) for any app that already exists, so
it's safe to leave `--bootstrap` on for routine deploys. Edit the non-secret
values in `fly.toml` (`APP_DOMAIN_NAME`, `CORS_ALLOWED_ORIGINS`,
`primary_region`) before the app's first deploy — the script prints a
reminder but doesn't edit it for you.

The equivalent manual steps, if you'd rather run them by hand:

### 1. Database

```sh
fly apps create mail-agents-db
fly volumes create pgdata --app mail-agents-db --region fra --size 10
fly secrets set POSTGRES_PASSWORD='<generate-a-strong-one>' --app mail-agents-db
fly deploy -c deploy/postgres/fly.toml
```

The region must match `primary_region` in `deploy/postgres/fly.toml`, or the
machine cannot attach the volume.

Do **not** allocate a public IP for this app.

The database build derives from the pinned official PostgreSQL image only to install a checked
entrypoint. It always starts with `pg_stat_statements` preloaded, query IDs enabled, top-level
statement tracking, utility tracking disabled, and both bind-parameter log limits set to zero. The
additive migration creates the extension.
After the database deploy and application migration, verify activation rather than inferring it
from the image configuration:

```sql
SHOW shared_preload_libraries;
SHOW compute_query_id;
SHOW pg_stat_statements.track;
SHOW pg_stat_statements.track_utility;
SELECT stats_reset, dealloc FROM pg_stat_statements_info;
```

Optional slow-query logs are controlled on the **database app** by
`DATABASE_SLOW_QUERY_LOGGING_ENABLED`, which accepts only `true` or `false` and defaults to
`false`. `true` adds a 200 ms threshold; both PostgreSQL bind-parameter log limits remain zero in
either mode. Any other value stops database startup. Enable it only for a bounded investigation
window and restart the database machine for the change to take effect.

Local PostgreSQL must likewise include `pg_stat_statements` in `shared_preload_libraries` and be
restarted. Creating the extension without that restart is not activation; the operator dashboard
will correctly report the extension as unavailable.

### 2. Application

```sh
fly apps create mail-agents-server
fly ips allocate-v4          # dedicated IPv4, required for SMTP on port 25

fly secrets set \
  DATABASE_URL="postgres://mail_agents:<POSTGRES_PASSWORD>@mail-agents-db.internal:5432/mail_agents" \
  JWT_SECRET="<random-64-chars>" \
  SMTP_USERNAME="<relay-user>" \
  SMTP_PASSWORD="<relay-password>" \
  CREDENTIAL_ENCRYPTION_KEYS="1:<base64-32-byte-key>" \
  CREDENTIAL_ENCRYPTION_ACTIVE_VERSION="1"

fly deploy
```

Schema migrations run automatically at startup from `src/infra/db.rs`, under a
Postgres advisory lock, so they are safe even with several machines booting at
once.

## Configuration

Secrets — set with `fly secrets set`, never in `fly.toml`:

| Secret | Notes |
| --- | --- |
| `DATABASE_URL` | Points at `mail-agents-db.internal` |
| `JWT_SECRET` | Session signing key |
| `SMTP_USERNAME` / `SMTP_PASSWORD` | Outbound relay credentials |
| `SMTP_HOST` / `SMTP_FROM_ADDRESS` | Outbound TLS relay and sender; when both are configured with a non-local host, new accounts must confirm a six-digit code sent by email. Remote relay certificates are validated. If outbound SMTP is not configured, registration skips confirmation. |
| `GOOGLE_OAUTH_CLIENT_ID` / `GOOGLE_OAUTH_CLIENT_SECRET` | Optional Google registration/login. Set both and register `https://<APP_DOMAIN_NAME>/auth/google/callback` as an authorized redirect URI in Google Cloud. |
| `APPLE_OAUTH_CLIENT_ID` / `APPLE_OAUTH_TEAM_ID` / `APPLE_OAUTH_KEY_ID` / `APPLE_OAUTH_PRIVATE_KEY_BASE64` | Optional Sign in with Apple. Set all four, use the Services ID as the client ID, base64-encode the `.p8` private key, and register `https://<APP_DOMAIN_NAME>/auth/apple/callback` as the return URL. Apple requires a real HTTPS domain and does not accept localhost. Register the outbound mail domain and sender with Apple's private email relay so confirmation codes reach relay addresses. |
| `CREDENTIAL_ENCRYPTION_KEYS` | Versioned AES-256-GCM keys (`1:<base64-32-bytes>,2:<base64-32-bytes>`). This is the read key ring; keep prior versions during rotation. |
| `CREDENTIAL_ENCRYPTION_ACTIVE_VERSION` | Positive version in `CREDENTIAL_ENCRYPTION_KEYS` used for new writes. It is intentionally independent of the highest readable version. |
| `HYDRA_DB_API_KEY` | Optional deployment-wide HydraDB credential. Set with all three `HYDRA_DB_*` settings below or leave all four absent. |
| `HINDSIGHT_API_KEY` | Optional deployment-wide Hindsight credential. Set with all three `HINDSIGHT_*` settings below or leave all four absent. |
| `GCS_SERVICE_ACCOUNT_JSON_BASE64` | The Cloud Storage service account key, base64-encoded — see [Picture uploads](#picture-uploads) |

Startup validates both credential-encryption settings but never mutates stored credentials. Use the
explicit operational commands below to inspect or rotate rows. After the first encrypted deployment,
rotate the credentials themselves with each model provider and save the replacement values through
the application; database encryption limits storage exposure but cannot invalidate provider keys
that may already have been copied from an older dump.

### Multi-Machine credential-key rotation

Never replace bytes under an existing version number: authenticated decryption of data written with
the earlier bytes will fail. Escrow every key version in the deployment's designated secret manager;
Fly secret values cannot be retrieved later. Restore operations must restore the key versions needed
by the selected database backup.

For a version 1 to version 2 rotation, keep both versions through two separate rolling deployments:

1. Back up/escrow both key versions and verify the current database:

   ```sh
   fly ssh console --app mail-agents-server \
     --command "/app/mail_agents credentials status --require-version 1"
   ```

2. **Distribute** version 2 without activating it. Set
   `CREDENTIAL_ENCRYPTION_KEYS=1:OLD,2:NEW` and keep
   `CREDENTIAL_ENCRYPTION_ACTIVE_VERSION=1`, then finish the rolling deployment. Abort if any
   Machine is unhealthy or remains on the earlier release.
3. **Activate** version 2 in a second rolling deployment, without removing version 1. Wait until no
   active-version-1 Machine remains.
4. **Converge** and require database-backed proof:

   ```sh
   fly ssh console --app mail-agents-server \
     --command "/app/mail_agents credentials rotate"
   fly ssh console --app mail-agents-server \
     --command "/app/mail_agents credentials status --require-version 2"
   ```

   Output is JSON containing versions and counts only. `status` exits nonzero for malformed or
   unavailable envelopes or failed `--require-version`; `rotate` exits nonzero on lock contention,
   invalid data, database errors, or incomplete convergence.
5. Retain version 1 for the documented backup/recovery window and record the decision. Before
   retirement, repeat status and confirm that no old Machine, old-version row, malformed envelope,
   unavailable version, or backup requiring version 1 remains.
6. Remove version 1, complete the rolling deployment, and run status once more.

`scripts/credential-key-rotation.sh` performs these checks and rollouts in order. It requires a
key ring containing both versions in `CREDENTIAL_ENCRYPTION_KEYS` and uses `jq` to verify every
Machine is started, healthy, and on the same image:

```sh
CREDENTIAL_ENCRYPTION_KEYS='1:OLD,2:NEW' \
  scripts/credential-key-rotation.sh 1 2
```

After the retention window, retirement is a separate invocation and requires both the retained
key ring and an explicit recovery decision:

```sh
RETAINED_CREDENTIAL_ENCRYPTION_KEYS='2:NEW' \
  scripts/credential-key-rotation.sh 1 2 --retire --backup-retention-confirmed
```

Non-secret settings live in the `[env]` block of `fly.toml`. `.env.example`
documents the full set.

| Setting | Requirement |
| --- | --- |
| `TASK_WORKER_CONCURRENCY` | Concurrent durable agent tasks per process; 1–64, default 4 |
| `AGENT_RUN_TIMEOUT_SECS` | Fallback wall-clock deadline for one agent/provider run; 1–3600 seconds, default 300. An agent's optional `run_timeout_secs` overrides it; a timeout consumes an attempt and cancels the provider future. |
| `RUNTIME_THREAD_STACK_BYTES` | Stack reserved per async runtime thread; 2 MiB–256 MiB, default 16 MiB. The task-worker → dispatch → agent-runner chain of `async fn` frames overruns Tokio's 2 MiB default in an unoptimized build. |
| `SMTP_ALLOW_PLAINTEXT_LOCAL` | Development-only, default `false`; plaintext is accepted only with no credentials and a host that resolves exclusively to loopback. Never enable it for a deployed relay. |

`RUNTIME_THREAD_STACK_BYTES` does not reach threads the process did not spawn itself, which means
libtest's. `.cargo/config.toml` sets `RUST_MIN_STACK` to the same value for the test suite; keep the
two in step. Because that also raises the ceiling at which CI would notice the chain growing,
`scripts/stack-budget.sh` re-runs the suite at the stock 2 MiB thread stack and fails if it no
longer fits — that is the regression alarm, and CI runs it. `scripts/stack-frames.sh` then reports
which frames grew (arm64/macOS only).

### Query-statistics investigations

The operator-only System dashboard is the routine monitoring surface. For the detailed current
database snapshot, use `scripts/db-stats.sh --local` with `DATABASE_URL`, or `scripts/db-stats.sh`
to run through Fly SSH. Its normalized SQL output is operationally sensitive; sanitize it before
saving or sharing it.

Capture a plan only for a reviewed `SELECT`, preferably against representative staging data. If a
production capture is necessary, use a read-only transaction with short statement and lock
timeouts and a bounded operational window. The baseline is:

```sql
BEGIN READ ONLY;
SET LOCAL statement_timeout = '5s';
SET LOCAL lock_timeout = '1s';
EXPLAIN (ANALYZE, BUFFERS, WAL, SETTINGS, TIMING OFF)
SELECT ...;
ROLLBACK;
```

Record the PostgreSQL version, statistics reset time, parameter classes (never sensitive values),
row counts/selectivity, and candidate index definition with the plan. Capture multiple executions
to distinguish cold and warm cache behavior.

### Long-term memory providers

Two memory backends are supported, HydraDB and Hindsight. Each is disabled unless all four of its
settings are present. A deployment may configure either, both, or neither; company owners pick one
per company in company settings, and switching retires the connection to the provider being left
so its remote data is torn down rather than orphaned.

#### HydraDB

HydraDB is disabled unless all four settings below are present. Startup validates the group; it
does not make a network call or promise that the credential is accepted. Company owners select
HydraDB in company settings, after which a durable worker provisions the remote database. Channel
memory controls remain unavailable until that connection reports `ready`.

| Setting | Requirement |
| --- | --- |
| `HYDRA_DB_API_KEY` | Secret bearer credential; never stored in PostgreSQL or logged |
| `HYDRA_DB_BASE_URL` | Absolute `http` or `https` API base URL |
| `HYDRA_DB_FAST_TIMEOUT_SECS` | Request timeout for fast calls; 1–110 seconds |
| `HYDRA_DB_THINKING_TIMEOUT_SECS` | Request timeout for thinking calls; 1–110 seconds and at least the fast timeout |

#### Hindsight

Hindsight partitions memory only by *bank*, so each company scope gets its own bank named
`{remote_database_id}--{scope}`. Provisioning creates the company bank; the agent and user banks
are created on first write, because their names are not knowable until a message arrives. Recall
issues one call per enabled scope and ranks the results by the scope weights the channel
configured. Retain runs asynchronously — Hindsight extracts facts with an LLM pass — so a memory
becomes recallable shortly after the reply rather than at once.

| Setting | Requirement |
| --- | --- |
| `HINDSIGHT_API_KEY` | Secret bearer credential; never stored in PostgreSQL or logged |
| `HINDSIGHT_BASE_URL` | Absolute `http` or `https` base URL **including the API version and organization path segment** — `https://api.hindsight.vectorize.io/v1/default` for Hindsight Cloud, `http://localhost:8888/v1/default` self-hosted |
| `HINDSIGHT_FAST_TIMEOUT_SECS` | Request timeout for fast calls; 1–110 seconds |
| `HINDSIGHT_THINKING_TIMEOUT_SECS` | Request timeout for thinking calls; 1–110 seconds and at least the fast timeout |

Scope-specific persistence is passed to Hindsight as per-item extraction context rather than as an
enforced filter, because its own extraction instructions are bank-level while the channel's
persistence mode can change between messages.

Memory traffic also has fixed, provider-neutral application bounds. They are intentionally not
deployment settings, so increasing a provider timeout or changing a deployment cannot remove the
safety envelope:

| Boundary | Limit and behavior |
| --- | --- |
| Connect / request / worker operation | 10 seconds maximum connect time; configured 1–110 second request time; 120 second total background-provider operation |
| Recall query / additional context | 16,000 / 512 Unicode characters; safely truncated with an explicit marker |
| Persistence user context / assistant answer | 32,000 Unicode characters each; safely truncated with an explicit marker |
| Multi-channel upstream context | 24,000 Unicode characters across all preceding steps; safely truncated with an explicit marker |
| Target collections | At most 3 per operation; larger operations are rejected |
| Request / successful response body | 384 KiB total request envelope (376 KiB body plus 8 KiB reserved for validated URL, credential, and headers) / 512 KiB response; checked before send and while streaming before JSON parsing |
| Provider URL / credential | 2,048 / 4,096 UTF-8 bytes; oversized startup configuration is rejected without logging the credential |
| Recall results | At most the requested count and never more than 20 rows |
| Chunk / final formatted memory context | 16,000 Unicode characters, including truncation marker and final context framing |

Metrics `memory_truncations_total` and `memory_bound_rejections_total` report the operation and
boundary without recording query, conversation, or response content.

Provider selection, provisioning state, retry attempts, and cleanup jobs are durable. Disabling
memory is suspension: runtime recall returns no memory, persistence is skipped immediately, and
the connection plus channel memory choices are retained. Re-enabling a previously ready connection
queues a readiness check before runtime memory resumes. If provider configuration disappears from
a deployment, runtime memory degrades to the same safe no-memory behavior and emits structured
state metrics. Deleting a company is different: it queues remote cleanup before the company row is
removed, and cleanup jobs deliberately survive the company cascade.

Each channel's Company, Agent, and User recall and persistence choices are independent. Missing
agent or sender identities skip only the unavailable scope; they never fall back to Company.
Company recall is available only to company members and system runs, while selected Agent and User
recall can still serve external channel participants. User collections use a stable digest of the
normalized sender address rather than embedding the address in collection names or logs.

The default `audience_only` persistence mode sends the same conversation to each selected target
for normal provider inference. The optional `scope_specific_facts` mode supplies fixed extraction
instructions per target: durable user-attributable facts for User, reusable agent/workflow lessons
for Agent, and durable organization-wide policies and processes for Company. Empty extraction is a
successful no-op.

### Picture uploads

Avatars are picked from disk and stored in a Google Cloud Storage bucket, so two
settings travel together — `GCS_AVATAR_BUCKET_PUBLIC` (non-secret, `fly.toml`) and
`GCS_SERVICE_ACCOUNT_JSON_BASE64` (a secret). Setting one without the other
fails the boot rather than starting with uploads quietly disabled.

```sh
# A service account with Storage Object Creator on the bucket, nothing more.
gcloud iam service-accounts keys create key.json \
  --iam-account uploader@<project>.iam.gserviceaccount.com

fly secrets set GCS_SERVICE_ACCOUNT_JSON_BASE64="$(base64 -i key.json | tr -d '\n')"
rm key.json
```

The avatar bucket has to be readable without credentials for a browser to load what it
stores (`gcloud storage buckets add-iam-policy-binding gs://<bucket>
--member=allUsers --role=roles/storage.objectViewer`). Objects are named by a
fresh UUID and written with a one-year immutable `Cache-Control`, so a URL's
bytes never change. Put a CDN in front by setting `GCS_PUBLIC_BASE_URL`.

With no bucket configured the app runs normally and the picture pickers report
that uploads are not configured; avatars already stored keep rendering.

### Mail attachments

Attachments arriving on a channel are stored in a **second bucket with no public access**
(`GCS_ATTACHMENTS_BUCKET_PRIVATE`), never the avatar bucket. They are downloaded through the app
(`/ui/threads/{thread}/attachments/{sha256}`), which authorizes each request the same way opening
the thread is authorized, and serves the bytes with `Content-Disposition: attachment` and
`X-Content-Type-Options: nosniff` so nothing emailed to us ever renders as a page on this origin.

There is no signed or public URL anywhere in that path, so nothing is forwardable and access ends
the moment someone leaves the channel. The same service account key covers both buckets; grant it
Storage Object Creator **and** Storage Object Viewer on the private one.

Objects are named by the SHA-256 of their contents, so the same file arriving twice is stored once.
With no attachments bucket configured, mail still arrives and its attachments are recorded in
`email_messages.attachments` — they simply show in the mailbox as files we do not have.

### Who can read what

Sessions are HS256 tokens signed with `JWT_SECRET` (cookie `session`, `HttpOnly`, `SameSite=Lax`,
`Secure` off localhost). Deploying this invalidates every existing session once: the old cookie was
an unsigned user id, and it is no longer believed.

Reads are scoped by the channel a message arrived on, not by the company alone:

| Channel's participant list | Who may read its threads and files |
| --- | --- |
| absent or empty | the company team (owner + `company_members`) |
| contains `@public` | the company team — `@public` admits *mail*, never readers |
| specific addresses | those addresses, plus the company owner |

Administration (channels, agents, company settings) stays owner-only regardless.

### Do not use `sslmode=require`

The official Postgres image ships without a TLS certificate. sqlx defaults to
`sslmode=prefer` and falls back to plaintext, which is fine because 6PN traffic
is already WireGuard-encrypted. Forcing `sslmode=require` will break the
connection.

### CORS

`CORS_ALLOWED_ORIGINS` is a comma-separated list of browser origins. Because the
API sends credentials, a wildcard is not possible — every frontend origin must
be listed explicitly. An unparseable value panics at startup, by design: a
silently-wrong CORS config is worse than a failed boot.

## Inbound mail

The SMTP listener binds `0.0.0.0:2525` inside the machine, and `fly.toml` maps
public port 25 to it.

**Port 25 requires a dedicated IPv4** (`fly ips allocate-v4`, ~$2/month). Fly's
shared IPv4 only forwards 80 and 443, so on a shared IP the listener is
unreachable no matter what the config says.

DNS to set up on the domain in `APP_DOMAIN_NAME`:

- `MX` → the app's hostname, resolving to the dedicated IPv4.
- `SPF` and `DKIM` for the outbound relay.
- `PTR` (reverse DNS) on the dedicated IP — ask Fly support; without it most
  large providers will reject or spam-folder your outbound mail.

## Operational notes

**Machines must not auto-stop.** `auto_stop_machines = false` and
`min_machines_running = 1` are deliberate. The task worker poll loop and the
SMTP listener have to keep running with zero HTTP traffic; letting Fly suspend
the machine on idle would silently stall both.

**Never scale the database past one machine.** `fly scale count 2` on
`mail-agents-db` gives you two independent Postgres instances on separate
volumes — not replicas. That is silent data divergence, not high availability.

**Backups are volume snapshots only**, taken daily with roughly five days of
retention. There is no replica and no point-in-time recovery. For a system
holding mail thread state, add a `pg_dump` to offsite storage before carrying
anything you care about.

**A running app survives a database outage; a booting one does not.** These are
two different behaviours, worth keeping straight:

- *Already running.* The process stays up. sqlx's pool reconnects when Postgres
  returns. Requests touching the database return 500s meanwhile, each of the
  task worker's loops logs a warning and backs off for 5s before polling again
  (`ERROR_BACKOFF` in `task_worker.rs`), and the SMTP listener keeps accepting —
  failed messages are logged and remote senders retry, so inbound mail is
  deferred rather than lost. No intervention needed.
- *Booting.* `init_db` connects and runs migrations once at startup, and `main`
  returns `Err` on failure, so a machine that starts while Postgres is
  unreachable exits and is restarted by Fly until the database is back. You hit
  this by deploying both apps at once, or if the app machine happens to restart
  during database downtime.

`/health` always returns HTTP 200 while the process is up, so Fly will not
recycle a running machine during a database outage — which is the intent.

**`/health` reports database status but never fails on it.** The HTTP status
code is unconditionally 200 as long as the process is serving; the response
body separately reports `"database": "up"` or `"down"` from a `SELECT 1`
probe bounded to 1 second, well under the platform check's own timeout. A
database blip shows up in the body for observability without causing Fly to
recycle a process that is otherwise serving traffic.

```json
{"status":"ok","version":"0.1.0","database":"up"}
```

## Routine tasks

```sh
fly logs                                  # app logs
fly logs -a mail-agents-db                # database logs
fly ssh console                           # shell into the app machine
fly ssh console -a mail-agents-db -C "psql -U mail_agents mail_agents"
fly status                                # machine health
fly secrets list                          # names only, never values
```

Deploying again is just `fly deploy` (app) or
`fly deploy -c deploy/postgres/fly.toml` (database).

Redeploying the database replaces its machine, so Postgres is down for roughly
10-30 seconds. With a single machine on a single volume there is no replica to
fail over to, so that is real downtime: the app stays up but returns errors for
any request that needs the database, and recovers by itself. Avoid deploying
both apps simultaneously — that is the one case where the app crash-loops
instead of degrading, per the note above.
