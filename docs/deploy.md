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

### 2. Application

```sh
fly apps create mail-agents-server
fly ips allocate-v4          # dedicated IPv4, required for SMTP on port 25

fly secrets set \
  DATABASE_URL="postgres://mail_agents:<POSTGRES_PASSWORD>@mail-agents-db.internal:5432/mail_agents" \
  JWT_SECRET="<random-64-chars>" \
  SMTP_USERNAME="<relay-user>" \
  SMTP_PASSWORD="<relay-password>" \
  OPENAI_API_KEY="<key>"

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
| `OPENAI_API_KEY` | Or `ANTHROPIC_API_KEY` / `GROQ_API_KEY` / `GEMINI_API_KEY`, per the agent's configured provider |
| `GCS_SERVICE_ACCOUNT_JSON_BASE64` | The Cloud Storage service account key, base64-encoded — see [Picture uploads](#picture-uploads) |

Non-secret settings live in the `[env]` block of `fly.toml`. `.env.example`
documents the full set.

### Picture uploads

Avatars are picked from disk and stored in a Google Cloud Storage bucket, so two
settings travel together — `GCS_BUCKET` (non-secret, `fly.toml`) and
`GCS_SERVICE_ACCOUNT_JSON_BASE64` (a secret). Setting one without the other
fails the boot rather than starting with uploads quietly disabled.

```sh
# A service account with Storage Object Creator on the bucket, nothing more.
gcloud iam service-accounts keys create key.json \
  --iam-account uploader@<project>.iam.gserviceaccount.com

fly secrets set GCS_SERVICE_ACCOUNT_JSON_BASE64="$(base64 -i key.json | tr -d '\n')"
rm key.json
```

The bucket has to be readable without credentials for a browser to load what it
stores (`gcloud storage buckets add-iam-policy-binding gs://<bucket>
--member=allUsers --role=roles/storage.objectViewer`). Objects are named by a
fresh UUID and written with a one-year immutable `Cache-Control`, so a URL's
bytes never change. Put a CDN in front by setting `GCS_PUBLIC_BASE_URL`.

With no bucket configured the app runs normally and the picture pickers report
that uploads are not configured; avatars already stored keep rendering.

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
