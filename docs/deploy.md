# Deploying to Fly.io

The deployment is **two Fly apps in the same organization**:

| App | Config | Role |
| --- | --- | --- |
| `openbots` | `fly.toml` | axum API (3001) + inbound SMTP listener (2525) + workflow workers |
| `openbots-db` | `deploy/postgres/fly.toml` | self-hosted Postgres on a volume, no public IP |

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
fly apps create openbots-db
fly volumes create pgdata --app openbots-db --region fra --size 10
fly secrets set POSTGRES_PASSWORD='<generate-a-strong-one>' --app openbots-db
fly deploy -c deploy/postgres/fly.toml
```

The region must match `primary_region` in `deploy/postgres/fly.toml`, or the
machine cannot attach the volume.

Do **not** allocate a public IP for this app.

The database build derives from the pinned official PostgreSQL image only to install a checked
entrypoint. It always starts with `pg_stat_statements` preloaded, query IDs enabled, top-level
statement tracking, utility tracking disabled, and both bind-parameter log limits set to zero. The
baseline migration creates the extension.
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
fly apps create openbots
fly ips allocate-v4          # dedicated IPv4, required for SMTP on port 25

fly secrets set \
  DATABASE_URL="postgres://mail_agents:<POSTGRES_PASSWORD>@openbots-db.internal:5432/mail_agents" \
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
| `DATABASE_URL` | Points at `openbots-db.internal` |
| `JWT_SECRET` | Session signing key |
| `SMTP_USERNAME` / `SMTP_PASSWORD` | Outbound relay credentials |
| `SMTP_HOST` / `SMTP_FROM_ADDRESS` | Outbound relay and sender. When a usable SMTP transport is configured, password registration and address changes send a six-digit confirmation code; the account/address is verified only after the correct code is redeemed. Remote relay certificates are validated. A configured relay send failure does not bypass confirmation. |
| `GOOGLE_OAUTH_CLIENT_ID` / `GOOGLE_OAUTH_CLIENT_SECRET` | Optional Google registration/login. Set both and register `https://<APP_DOMAIN_NAME>/auth/google/callback` as an authorized redirect URI in Google Cloud. |
| `APPLE_OAUTH_CLIENT_ID` / `APPLE_OAUTH_TEAM_ID` / `APPLE_OAUTH_KEY_ID` / `APPLE_OAUTH_PRIVATE_KEY_BASE64` | Optional Sign in with Apple. Set all four, use the Services ID as the client ID, base64-encode the `.p8` private key, and register `https://<APP_DOMAIN_NAME>/auth/apple/callback` as the return URL. Apple requires a real HTTPS domain and does not accept localhost. Register the outbound mail domain and sender with Apple's private email relay so confirmation codes reach relay addresses. |
| `CREDENTIAL_ENCRYPTION_KEYS` | Versioned AES-256-GCM keys (`1:<base64-32-bytes>,2:<base64-32-bytes>`). This is the read key ring; keep prior versions during rotation. |
| `CREDENTIAL_ENCRYPTION_ACTIVE_VERSION` | Positive version in `CREDENTIAL_ENCRYPTION_KEYS` used for new writes. It is intentionally independent of the highest readable version. |
| `GCS_SERVICE_ACCOUNT_JSON_BASE64` | The Cloud Storage service account key, base64-encoded — see [Picture uploads](#picture-uploads) |

`ALLOW_UNVERIFIED_EMAIL_WITHOUT_SMTP` is a startup-read boolean, default `false` in
`fly.toml` and `.env.example`. With no usable outbound confirmation transport, password
registration and email address changes are unavailable by default. Set it to `true` only when
deliberately allowing those operations without SMTP: the resulting address remains unverified,
so email-based invitations, participant-restricted channels, and operator access stay unavailable.
The flag has no effect when outbound SMTP is configured, including when a send fails. A loopback
SMTP relay counts as configured only when `SMTP_ALLOW_PLAINTEXT_LOCAL=true` meets its local-only
requirements; the default localhost placeholder does not enable confirmation.

`AUTH_TRUSTED_PROXY_IPS` is an optional, non-secret comma-separated list of TCP peer IPs for
trusted ingress proxies. Startup rejects invalid entries. A trusted proxy must replace any
client-supplied `X-Forwarded-For` with exactly one client IP; untrusted peers' forwarded headers
are ignored. If the list is empty, clients behind one proxy share one auth rate bucket. The
current fixed 60-second admission budgets are 600 requests globally, 30 per source for login or
OAuth, and 12 per source for registration, confirmation, or credential changes. Auth request
bodies are capped at 8 KiB, except OAuth at 32 KiB, with a 10-second read deadline; at most four
Argon2 jobs run concurrently.
PostgreSQL applies five code guesses per registration address or account-change kind across
reissues in a fixed one-hour window, and five password-login attempts per normalized identifier
and resolved source in a fixed two-minute window. A blocked request does not extend its window;
successful login clears that identifier/source counter. Expired rows are removed in bounded batches
as authentication traffic continues. A correct code is spent once. Invalid, expired, and exhausted
codes return the same response; password-login exhaustion returns invalid credentials.
See [authentication operations](authentication-operations.md) for all fixed bounds, low-cardinality
telemetry and provider-outage triage.

Session validation and cleanup metrics use the separate `monitoring::counter` and
`monitoring::gauge` targets. The startup default and supplied `fly.toml`/`.env.example` filters
enable both at info. If overriding `RUST_LOG`, retain `monitoring::counter=info,monitoring::gauge=info`
to export these signals; the application-only `mail_agents` directive does not include them.
See [authentication operations](authentication-operations.md#security-changes-and-session-cleanup)
for session lifetime, revocation and backlog alerts.

### Request traces and ingress access logs

The application's HTTP span records a generated request ID, method, protocol version, and only a
static Axum matched-route template when one exists. The response event adds status and latency.
Use those fields, provider type, and an opaque event correlation ID for incident triage; do not
paste OAuth callback URLs or webhook URLs into tickets. The exact field policy and sensitive-route
inventory are in [request trace redaction](request-trace-redaction.md).

`fly.toml` sets `RUST_LOG` for application output but has no access-log redaction setting for an
upstream proxy. Fly's [security questionnaire](https://fly.io/legal/security-questionnaire/)
states that Fly Proxy records request paths on errors. An application span cannot remove a Resend
webhook token from a proxy error record made before the request reaches Rust. Restrict access to
platform logs and rotate a webhook token after suspected exposure. If another proxy, CDN, or log
shipper is added, configure it to omit path and query values before retaining or forwarding access
logs, and verify that configuration with marker requests.

Startup validates both credential-encryption settings but never mutates stored credentials. Use the
explicit operational commands below to inspect or rotate rows. After the first encrypted deployment,
rotate the credentials themselves with each model provider and save the replacement values through
the application; database encryption limits storage exposure but cannot invalidate provider keys
that may already have been copied from an older dump.

### Workflow cutover

Use the [workflow operations and cutover runbook](workflow-operations.md) for the complete release,
including inactive bindings, samples, controlled message/manual/schedule smoke tests and emergency
stop. The Rig pilot below is one part of that gate. No deployment target or database reset is
authorized by these instructions alone.

### Rig rollout on a fresh database

This release assumes a database reset. It does not require a staged conversion of existing agents.
Use one coordinated deployment and the squashed baseline migration, including company MCP
credentials/connections, durable continuations, structured responses and their constraints. Keep
the production image's non-root user and existing proxy/port configuration; Rig needs no additional
runtime privileges. This procedure describes the operator's reset; it is not a startup feature.

1. Stop external ingress and agent/configuration writes at the existing routing/deployment layer,
   stop schedules/workers, and wait for shutdown to settle before the operator resets the database.
   Keep the deployment closed while provisioning the fresh database and credentials. Do not leave
   a previous process attached to it.
2. Build/deploy the same release to every web/worker instance. Startup applies migrations under
   the existing advisory lock; verify completion before allowing company setup. The fresh database
   must have the baseline applied before MCP configuration is saved.
   Configure the existing encryption key ring and active version for company model and MCP secrets.
3. Every agent runs on Rig; there is no runtime selection. Verify successful registry
   construction at startup. Startup checks local configuration and coverage, not company
   credentials or remote models. Recreate a controlled company, its model connection and enabled
   model, and an agent using the [tested request](rig.md#configure-an-agent).
4. Keep access restricted to the pilot operators. Run the pilot below against controlled recipients,
   a captured delivery target and a controlled MCP endpoint. Capture release/model/run IDs and
   pass/fail evidence without secrets or candidate/schema contents in operational logs.
5. Open ordinary ingress and configuration access only after the release gates and pilot pass on
   the deployed release. There is no dedicated Rig or MCP admission feature flag:
   use the coordinated access restriction above.

| Pilot | Required evidence before opening access |
| --- | --- |
| Ordinary execution | Text completion; usage and elapsed duration visible; controlled timeout terminates within budget |
| Skill | A controlled `agent.run` step whose agent has an attached skill answers by the skill's instructions |
| Structured response | A model step with an `output_schema`: valid JSON; one/two repairs; an exhausted repair fails the step and delivers no reply |
| MCP | Save separate credentials, refresh/test and review grants; schema/revision change rejects stale grants. Run one workflow `mcp.call` step against the controlled endpoint and keep its receipt |

Use the deterministic scenarios in the [release verification matrix](rig-verification.md) for
invalid outputs, timeout and disconnect cases; live model prompts alone cannot reliably force them.
The deployed company/model authentication and controlled MCP checks are additional pilot evidence.
The recorded local fixture run is not a claim that this deployment's pilot has already passed.

Relevant deployment settings:

| Setting | Behavior |
| --- | --- |
| `MCP_INTERNAL_ENDPOINTS` | Optional comma-separated exact IP-literal URL exceptions to public HTTPS; startup validates syntax, not endpoint readiness |
| `RUNTIME_THREAD_STACK_BYTES` | Runtime threads, default 16 MiB; separate from libtest's `RUST_MIN_STACK` |

Workflow `http.request` steps reach public HTTPS origins only. Unlike MCP, there is no operator
exception list for private or internal addresses.

Model keys and enabled models belong in company model connections. Upstream Rig SDK example
environment keys do not configure this application's agent credentials. MCP bearer tokens are
company-owned encrypted credentials, using the existing key ring; agents store connection IDs only.
See [Rig configuration](rig.md) for supported providers, endpoint restrictions and exact limits.

#### Monitor and recover

Compare completion, suspension, retry, terminal failure, timeout, latency and usage by
provider/model on representative workloads. Start from the run (`GET
/api/companies/{company_id}/workflow-runs/{run_id}`); the paths below are under the same company
prefix. Track structured validation outcomes,
`repair_call_count`, `terminal_invalid_output_reason` and aggregate token usage (which includes
repairs); there is no separate repair-only token total. Estimated/mixed usage is not a bill.
System-prompt generation is not metered and does not appear in usage.
Use safe diagnostics, not prompts, schemas, candidates or tool payloads, for alerts.

- **Decision wait:** a `decision.human` step parks the run on a workflow decision. Settle it on
  the decision page or with `POST …/workflow-decisions/{decision_id}/submit`. Restart uses saved
  call IDs, arguments and results. Do not change configuration mid-run or delete receipts.
- **Provider timeout/crash:** let the job's retry and lease path recover the checkpoint.
  Unknown model requests remain charged. Diagnostics failures must not trigger manual paid-call
  replay. An exhausted budget is not repaired by clearing its ledger.
- **Indeterminate MCP effect:** the run waits on reconciliation. Retain its run, invocation and
  connection IDs, and inspect the controlled remote service's audit trail to determine whether the
  effect committed. `POST …/workflow-runs/{run_id}/reconciliations` records an operator note on
  the action; it verifies no evidence and resumes nothing. If the effect committed, reconcile the
  business result without issuing the call again; if it is confirmed absent, an operator may start
  fresh work after checking that doing so is safe. Never clear an invocation receipt to force
  redispatch; cancelling the run (`POST …/workflow-runs/{run_id}/cancel`) cannot undo an accepted
  remote request.
- **Invalid structured output:** inspect the bounded failure reason and repair count; no completed
  answer was published. Correct the agent, the model or the step's `output_schema` by publishing a
  new workflow version, then start fresh work if appropriate. Do not reset repair counters or
  publish the rejected candidate.

#### Roll back after new work exists

Keep access restricted while rolling back. Prefer a compatible release that still reads the current
Rig checkpoints, MCP receipts and response snapshots and enforces their contracts. Stop new
admissions through the routing/deployment controls and by deactivating bindings (`POST
…/workflow-bindings/{binding_id}/deactivate`), inventory queued, running and waiting runs (`GET
…/workflow-runs`), and drain, cancel or reconcile them. Retain applied migrations and receipts.

For structured-response rollback, keep enforcement for saved runs until they finish or are
reconciled. A step's contract is its published version's `output_schema`, and a run keeps the
version it started on, so publishing a new version does not change a run in flight. Do not deploy
workers that ignore snapshots or reset repair reservations. If no compatible
rollback artifact exists, keep affected work stopped and fix forward; this runbook does not promise
an incompatible binary can safely consume the new database.

### Stored credential formats

Two formats share the one key ring, and both are inventoried by `credentials status` and moved by
`credentials rotate`.

| Format | Column | Shape |
|---|---|---|
| `enc:v1` | `company_model_connections.api_key` | The key ring's key encrypts the credential directly, with empty associated data. Legacy; `plan/db_improve/improve-key-credentials.md` is the plan to retire it. |
| `enc:v2` | `integration_credentials.envelope` | Envelope encryption. A random per-credential data key encrypts the token; the key-ring key wraps only that data key. |

`enc:v2` is the format every new secret uses. Two properties follow from it:

- **The row is authenticated.** Both AES-256-GCM layers bind `(company, installation, transport,
  credential kind)` as associated data, so an envelope copied into another company's, another
  installation's, or the neighbouring credential kind's row fails to open instead of yielding the
  original token. A row that fails this check is an error at the read boundary, never a silent
  "no credential stored".
- **Rotation never handles plaintext.** Moving an `enc:v2` row to a new key version rewraps the
  48-byte data key and copies the ciphertext through untouched. `enc:v1` has no such option and is
  decrypted and re-encrypted, which is one more reason it is being retired.

**Threat model, and its accepted limitation.** The key-encryption key is an exportable value held in
Fly secrets and loaded into process memory — the "bounded launch alternative" of
`plan/db_improve/improve-key-credentials.md`, chosen so launch is not blocked on standing up a KMS.
It gives envelope structure, per-row context binding, and cheap rewrapping. It does **not** protect a
token from a compromised application process or a malicious deployment, because that process can
legitimately request the plaintext in order to call the provider. Moving the KEK behind a KMS is a
change to two functions in `src/adapters/persistence/credentials/envelope.rs` and does not change the
stored format; revisit it when the deployment holds credentials for workspaces it does not own.

### Multi-Machine credential-key rotation

Never replace bytes under an existing version number: authenticated decryption of data written with
the earlier bytes will fail. Escrow every key version in the deployment's designated secret manager;
Fly secret values cannot be retrieved later. Restore operations must restore the key versions needed
by the selected database backup.

For a version 1 to version 2 rotation, keep both versions through two separate rolling deployments:

1. Back up/escrow both key versions and verify the current database:

   ```sh
   fly ssh console --app openbots \
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
   fly ssh console --app openbots \
     --command "/app/mail_agents credentials rotate"
   fly ssh console --app openbots \
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
| `DATABASE_MAX_CONNECTIONS` | Database pool size, default 20. Startup refuses a pool smaller than `9 + 2 × WORKFLOW_WORKER_SLOTS`, naming both keys. |
| `WORKFLOW_WORKER_SLOTS` | Concurrent workflow workers per process; 1–16, default 2. Each slot needs two pool connections (its claim or handler transaction and its lease renewal). |
| `RUNTIME_THREAD_STACK_BYTES` | Stack reserved per async runtime thread; 2 MiB–256 MiB, default 16 MiB. A background job's chain of `async fn` frames can overrun Tokio's 2 MiB default in an unoptimized build; `scripts/stack-budget.sh` is what notices it growing. |
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
With no attachments bucket configured, mail still arrives and attachment metadata is recorded on
the canonical `messages` row — the files simply show in the mailbox as unavailable.

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
`min_machines_running = 1` are deliberate. The workflow workers' poll loops and the
SMTP listener have to keep running with zero HTTP traffic; letting Fly suspend
the machine on idle would silently stall both.

**Never scale the database past one machine.** `fly scale count 2` on
`openbots-db` gives you two independent Postgres instances on separate
volumes — not replicas. That is silent data divergence, not high availability.

**Backups are volume snapshots only**, taken daily with roughly five days of
retention. There is no replica and no point-in-time recovery. For a system
holding mail thread state, add a `pg_dump` to offsite storage before carrying
anything you care about.

**A running app survives a database outage; a booting one does not.** These are
two different behaviours, worth keeping straight:

- *Already running.* The process stays up. sqlx's pool reconnects when Postgres
  returns. Requests touching the database return 500s meanwhile, each background
  worker logs a warning and polls again on its own interval (a workflow worker
  after one second, `WorkflowWorker::run`), and the SMTP listener keeps accepting —
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
fly logs -a openbots-db                # database logs
fly ssh console                           # shell into the app machine
fly ssh console -a openbots-db -C "psql -U mail_agents mail_agents"
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


### TypeSafe Jev

`jev.evaluate` is available in the production workflow catalogue. Configure its
company-owned API key and allowed versioned models in company Settings, then bind
the `typesafe` workflow slot to that connection. There is no global TypeSafe API-key
or endpoint environment setting. The adapter uses the fixed HTTPS System One URL;
company forms cannot change it. Existing `CREDENTIAL_ENCRYPTION_KEYS` and
`CREDENTIAL_ENCRYPTION_ACTIVE_VERSION` encrypt TypeSafe keys and the credential
inventory/rotation commands include these rows. Startup validates encryption
configuration; it does not probe TypeSafe credentials or model availability.

Experimental image steps also need the private attachment storage already used by
this deployment; use the documented GCS settings above. Text steps need no storage.
Keep the enforced 64 KiB wire limit and image bounds described in the
[workflow language](workflow-language-v1.md#experimental-jev-images-and-box).
Use isolated scripted samples before a controlled live evaluation and inspect
run history and company model usage. Provider usage is billed by TypeSafe; local
estimates and configured prices are not invoices. Operational failure/recovery and
revocation semantics are in the [Jev runbook](workflow-operations.md#typesafe-jev-evaluation).
