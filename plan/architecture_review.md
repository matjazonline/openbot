# Architecture Review — mail-agents-server

## Context

You asked for a systems-engineering read of the app to find weak points worth implementing better.
This is a **findings report** — you asked for the diagnosis, not the fixes, so no code changes are
proposed as work to do now. An implementation ordering is at the end for when you want it.

Scope covered: both ingress paths (SendGrid webhook, SMTP listener), the ingest/ACL layer, the
durable task queue and worker, the scheduler, the outbound dispatcher, the HTTP/tenancy surface,
the data layer, and the deploy shape on Fly.io.

**On verification depth:** every Critical and High finding below I confirmed by reading the code
directly, and the exploit path is stated. The Medium and Lower findings carry precise locations but
some were not individually re-derived — they're reliable enough to act on, not so reliable that I'd
skip a look before changing the code.

`plan/db_improvements.md` already documents the schema-level issues (tenant-consistency FKs,
plaintext API keys, attempt-ledger fencing, schedule-run durability). This report does **not**
re-derive those. Where a finding here overlaps, it's noted.

---

## What is genuinely strong

Worth stating plainly, because it shapes where the risk actually is — this is not a codebase with a
weak core.

- **Ports/adapters layering is real.** `src/domain/` imports nothing from `application` or
  `adapters`, and no `sqlx`/`axum` types leak into either. The exceptions are narrow (below).
- **The queue's claim/lease/fencing design is well built.** `FOR UPDATE SKIP LOCKED`, worker-id +
  live-lease guards on every state transition, and a lease re-check immediately before outbound
  dispatch (`thread/dispatch.rs:464-478`) so two workers can't both reply.
- **Outbound Message-IDs are idempotency-keyed** (`outbound_dispatcher.rs:226-239`, SHA-256 of a
  stable key), so a re-send is deduplicable rather than a straight duplicate.
- **Per-company scoping is otherwise disciplined** — `ui_tasks.rs:313-324` is the model, and the
  ownership re-check is consistently present across agent/channel/schedule/invite use cases. The
  one break is Critical-4.
- **The fail-open `.unwrap_or(false)` authorization pattern `src/AGENTS.md` warns about is mostly
  cleaned up** — `DirectoryCache::membership` propagates with `?`, the `is_trusted_participant`
  triplication is consolidated into `Channel::participant_access`.
- **The approval gate for agent tool use is the right shape** — defaults to requiring approval, fails
  closed on malformed config, classifies recipients against the directory rather than the model's word.
- **No connection is held across an external await** — the single biggest pool-exhaustion risk, avoided.
- **DB-backed integration tests exist** with a genuinely thought-through harness (`test_support.rs`).

The weaknesses are concentrated at the **edges**: ingress trust, admission control, concurrency, and
what happens when a process dies.

---

## Critical

### C-1 — Any inbound email can forge its own SPF/DKIM/DMARC verdict with one header

`smtp/server.rs:766-793`. The server does genuine DNS-backed verification via `mail-auth`
(`server.rs:479-548`) and passes the real verdicts into `parse_raw_mime_to_payload` as
`mut spf_status` / `mut dkim_status` / `mut dmarc_status` (`server.rs:724-732`). The header scan then
**overwrites them from the message body**:

```rust
if name_lower == "authentication-results" {
    if val_lower.contains("spf=pass") {
        spf_status = Some("pass".to_string());          // unconditional overwrite
    } else if val_lower.contains("spf=fail") && spf_status.is_none() {
        spf_status = Some("fail".to_string());          // only fills a hole
    }
}
```

The asymmetry is the bug: `pass` overwrites a real `Fail`, `fail` only applies when nothing was set.
Identical logic for `dkim` and `dmarc` (`server.rs:782-792`) and `received-spf` (`:773-779`).

This server is the edge MTA, so `Authentication-Results:` in the DATA payload is written by the
**sender**, not a trusted upstream. A spoofer whose SPF genuinely returns `Fail` adds one header line
and the verified verdict becomes `"pass"`.

**Exploit:** one extra header in a spoofed message defeats the entire email-authentication layer on
the path that was supposed to be the trustworthy one.

### C-2 — The SendGrid webhook is unauthenticated, and the caller supplies its own auth verdict

`webhooks/sendgrid.rs:20-22` registers the route; `routes/mod.rs:81-87` merges `webhooks::router()`
into the **public** router, outside the `require_auth` layer. There is no signature check, no shared
secret, no IP allowlist — no `SENDGRID_*` variable exists in config or `.env.example`.

The handler reads `from`, `spf`, `dkim`, `spam_score`, and `headers` verbatim from the request body
(`sendgrid.rs:78-107`), and those become the message's authentication fields
(`email_parser.rs:221-223`). So an unauthenticated `curl` controls:

- **who the sender is** — which is exactly what `Channel::participant_access` keys authorization on;
- **the SPF/DKIM verdicts** — or simply omits them, see C-3;
- **the spam score** the `>= max_spam_score` gate at `ingest.rs:686-697` compares against.

Replay protection is `Message-ID` dedup only (`ingest.rs:806-825`) — attacker-chosen, so a fresh
UUID replays freely.

Minor, same file: `#[instrument(skip(thread_use_cases, headers))]` at `sendgrid.rs:45` does not skip
`req`, so the full header map is recorded into the span.

### C-3 — Email authentication fails open

`thread/support.rs:165-181` rejects **only** on the literal string `"fail"`:

```rust
if status.is_some_and(|s| s.eq_ignore_ascii_case("fail")) { ... reject ... }
```

`None`, `""`, `"softfail"`, `"temperror"`, `"permerror"`, or any provider spelling change all pass.
Absent a value the check is a no-op — which is the state of every request on the C-2 path.

This is the concrete instance of the rule `src/AGENTS.md` already states: *"Statuses that arrive as
strings … should be parsed into an enum once at the adapter boundary, so the match is exhaustive."*

C-1, C-2 and C-3 compose: C-2 opens the door, C-3 means you don't even need to forge a `pass`, and
C-1 means closing C-2 still leaves the SMTP path bypassable.

*Scope note, so this isn't overstated:* the `is_inter_channel` bypass above this check is **not**
remotely reachable. `InternalChannelSource` is only constructed in-process (`thread/mod.rs:427-431`);
the inbound `X-MailAgents-Channel-ID` header cannot claim internal trust. That part is correct.

### C-4 — Cross-tenant thread disclosure in `/ui/schedules` (IDOR)

`list_messages_by_thread_id` takes only a thread id and applies no company/channel predicate
(`persistence/thread.rs:614-623`, `LIMIT 200`). Three call sites in `ui_schedules.rs` hand it a raw
caller-supplied id:

- **`ui_schedules.rs:373` + `397-400`** (`thread_pane`) — verifies the *schedule* and the *channel*
  belong to the caller's company, then **ignores both** and reads `Path(thread_id)` directly.
- **`ui_schedules.rs:288-292`** — `selected_thread_id` from `query.thread_id`, validated against nothing.
- **`ui_schedules.rs:436` + `472-475`/`505-508`** (`reply_in_thread`) — same, and the loaded history
  builds `in_reply_to`, so the victim thread's `Message-ID` leaks into an outbound message too.

**Exploit:** one request —
`GET /ui/schedules/thread/<victim_thread_uuid>?company_id=<yours>&schedule_id=<yours>` — renders up
to 200 messages of another tenant's thread.

The rest of the codebase does this correctly and is the model to copy: `ui_attachments.rs:83-95`,
`channel.rs:1035-1054` (`.filter(|thread| thread.channel_id == channel_id)`).

### C-5 — System mail sends the SMTP relay password in cleartext

`outbound_dispatcher.rs:547-559`:

```rust
if !config.smtp_host.is_empty() {
    let mut mailer_builder =
        AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.smtp_host)
```

`builder_dangerous` = no TLS, no certificate validation — and `SMTP_PASSWORD` is attached to it at
`:553-556`. The two sibling transports use `relay()` correctly (`:205`, `:402`).

Worse, the guard is `!config.smtp_host.is_empty()`, missing the `&& != "localhost"` used at `:400` —
so this path **fires against the real relay in production**. Reached by `send_system_reply` (the
`_help` auto-reply, `ingest.rs:548`) and the bounce path.

### C-6 — Stored XSS from inbound email content

`escape_html_text` (`layout.rs:514-521`) is correct and mailbox/outbox/profile use it thoroughly.
Several page modules skip it entirely. The worst is `pages/simulation.rs:805-822`, the inbound branch:

```rust
sender  = msg.sender,
msg_id  = msg.message_id,
subject = msg.subject,
body    = msg.clean_text_body,      // raw inbound email content, unescaped
```

The sibling agent branch three lines up (`simulation.rs:763`) **does** call `render_markdown`, which
sanitizes via `ammonia` — the two branches disagree, which makes this a slip rather than a policy.

`msg.subject` and `msg.clean_text_body` are attacker-controlled by anyone who can reach C-2 or the
SMTP port. Other unescaped sinks: `simulation.rs:551/786/1126/1143`, `tasks.rs:459` (inbound subject
into an unescaping `badge()`), `agents.rs:255/271/324` (`:271` is inside an `hx-confirm="..."`
**attribute**), `channels.rs:809/973-977`, `companies.rs:144/207/268/332/359/490`, `onboarding.rs:101/171`.

`ammonia` is used in exactly one place (`layout.rs:14`), for the markdown path only.

**This compounds with H-8** (no CSP, no CSRF token): stored XSS on an authenticated page with no CSP
backstop and cookie-only CSRF defence is session takeover across tenants.

---

## High

### H-1 — A task that kills the process retries forever, with no dead-letter escape

`persistence/task.rs:1449-1456`. Stealing an expired lease re-claims the row **without touching
`retry_count`** — only `mark_task_failed` increments it (`task.rs:1541`), and that only runs if the
worker survives long enough to report.

So any failure that kills the process rather than returning `Err` — OOM on the 1 GB VM, SIGKILL, a
panic outside the agent's catch — is an **unbounded retry loop**, re-claimed every ≤15 min forever,
re-charging the LLM each time. `max_retries` does not protect against it. This is the classic poison
pill.

Same hole on the close-out path: `mark_task_completed`/`mark_task_failed` both require
`lock_expires_at > CURRENT_TIMESTAMP` (`task.rs:1522`, `:1545`). If the heartbeat lost the lease, the
outcome is logged as ignored and the row stays `processing` with an expired lease → re-claimed, same
retry count. Loop.

Notably, the **outbox** already has the fix: lease expiry costs an attempt
(`reap_expired_outbox_leases`, `task.rs:1233`). `background_tasks` is missing it.

### H-2 — A lost lease orphans a still-running agent; two workers then run it concurrently

`agent_runner.rs:1026` is `tokio::spawn(task.run()).await`. `while_leased` (`task.rs:130-147`) signals
`Leased::Lost` by **dropping the work future** — but dropping the outer future drops only the
`JoinHandle`, and tokio does not cancel a spawned task.

So on lease loss the agent keeps running to completion: it can still write `email_outbox` rows, create
approvals, and mutate outreach state, while `claim_pending_tasks` hands the same task to another
worker. Two concurrent runs of the same agent, both with side effects, one unobserved.

This fires predictably after any approval/outreach suspension, which un-leases the task from inside
the agent run (`approval.rs:198-206`, `task.rs:737-741` set `worker_id = NULL`) — so the next
heartbeat necessarily fails and the running agent is orphaned.

### H-3 — A transient LLM error is emailed to the customer and permanently disables retries

`thread/dispatch.rs:267-275` turns an agent error into an output body:

```rust
let message = format!("Agent execution failed: {err}");
```

That flows through `combine_responses` → `deliver_agent_response` → `queue_agent_reply` — i.e. it is
**queued to `email_outbox` and mailed to the sender**, and saved to the thread — *before* the error
that fails the task is returned.

Then on retry, `task_worker.rs:757-765` finds that saved body and short-circuits:

```rust
if outbound.clean_text_body.starts_with("Agent execution failed:") { ... return Err(...) }
```

**The agent is never re-run.** A provider 429 or timeout therefore produces: an "Agent execution
failed: …" email to the customer, three instant no-op retries, then `dead_letter`. Retry is disabled
for exactly the class of failure retries exist for.

### H-4 — The scheduler can livelock and permanently stop firing

`persistence/schedule.rs:472-482` — the pending-run query in the new durable-schedule path:

```sql
SELECT id, scheduled_for, schedule_snapshot, thread_id, task_id
FROM schedule_runs WHERE task_id IS NULL
ORDER BY created_at, id LIMIT $1
```

No lease, no `SKIP LOCKED`, no attempt counter, no backoff column, no terminal state.
`record_run_error` (`schedule.rs:503-514`) writes `last_error` and leaves `task_id IS NULL`, so the row
stays in `schedule_runs_pending_idx` forever. `process_due_schedules` returns `due_runs.len()`
regardless of success, and a full batch makes `poll_until_shutdown` sleep `Duration::ZERO`
(`task_worker.rs:101`, `:250-253`).

**So 10 permanently-failing runs** — e.g. `"Channel not found for schedule execution"` after a channel
delete — turn the 2-second schedule loop into a hot spin against Postgres, *and* because the query is
`ORDER BY created_at LIMIT 10` with no tenant scoping, those same 10 broken rows are returned every
iteration. **No new scheduled run is ever materialised again**, for any tenant. Silent, permanent.

Cheap to reproduce: delete a channel that has an enabled schedule, then watch whether any other
schedule ever fires.

### H-5 — Inbound SMTP has no admission control

Port 25 is publicly exposed (`fly.toml [[services.ports]] port = 25`) into a 1 GB shared-cpu-1x VM
that also serves all HTTP traffic. `INCOMING_SMTP_ENABLED` defaults to **true** and
`INCOMING_SMTP_HOST` to **`0.0.0.0`** (`config.rs:272-281`), so this is the default posture.

- **The advertised size limit is not enforced.** EHLO advertises `SIZE 20971520`
  (`server.rs:325-328`), but `push_data_line` (`:650-658`) is an unbounded `data_buffer.push_str(...)`
  with no counter and no `552` path. `MAIL FROM … SIZE=` is not checked either. `read_line`
  (`:184-219`) has no line-length cap.
- **No timeouts of any kind.** No `tokio::time::timeout` anywhere in the file — no idle timeout, no
  per-command timeout, no session cap. RFC 5321 §4.5.3.2 requires these. A slowloris pins a task forever.
- **No `RCPT` count limit** (`:342-360` pushes unbounded), no command-count limit.
- **Unbounded task spawn per connection** (`:136`). The per-IP cap (30) is process-local and does not
  bound a distributed flood: 1000 IPs × 30 = 30 000 concurrent tasks.
- `check_dnsbl` (`:51-74`) does sequential `lookup_host` with **no timeout**, on the connection-admission
  path — and silently returns `None` for all IPv6 (`:61`), so DNSBL screening doesn't exist on IPv6.

Not an open relay, though — recipients are constrained by the channel lookup. That part is fine.

### H-6 — One agent run at a time, globally, with no time bound

`task_worker.rs:58` — `const TASK_CLAIM_BATCH: i64 = 1;` — and the run happens **inline on the poll
loop**. LLM concurrency is 1 for the whole platform, with `min_machines_running = 1`.

There is also no wall-clock bound: `ensure_config_fields` (`agent_runner.rs:453-539`) injects
`max_tokens`/`provider`/`model`/`api_key` but **not `timeout_seconds`**, nothing wraps `execute()` in
`tokio::time::timeout`, and `agent.chat` loops over tool calls with no deadline. Since the heartbeat
renews the lease indefinitely while work is in flight, **a stuck run blocks all task processing for as
long as it lasts and no other worker can take over**.

Under a 10 k-email burst: ingestion absorbs it, then it drains one run at a time — at 20 s/run, ~55
hours — with a globally FIFO queue and **no per-tenant fairness**, so one noisy tenant starves everyone.

### H-7 — Shutdown drops in-flight work and never releases leases

`main.rs:39-55` spawns the task worker, SMTP listener and event listener with `tokio::spawn` and
**never holds the `JoinHandle`s**. `poll_until_shutdown` only observes shutdown *between* iterations
(`task_worker.rs:109-117`), and `main` returns as soon as the server future resolves or `DRAIN_GRACE`
(20 s) elapses — so the runtime is dropped and the in-flight agent run is aborted mid-execution.

Nothing releases the lease on shutdown, so the row stays `processing` with `lock_expires_at` up to
**15 minutes in the future**. On Fly, **every deploy strands the in-flight task for up to 15 minutes**,
and per H-1 it then restarts from scratch with no retry accounting.

### H-8 — No security headers, and third-party CDN scripts with no SRI

`grep` for `Content-Security-Policy`, `Strict-Transport-Security`, `X-Frame-Options`,
`X-Content-Type-Options` across `src/` returns **zero** matches. `layout.rs:32-43` and
`mailbox.rs:483-488` pull `daisyui@5`, `@tailwindcss/browser@4` and `htmx.org@2.0.4` from jsdelivr and
unpkg with **no `integrity=` anywhere** (0 matches) and floating major versions.

`@tailwindcss/browser` is the in-browser *compiler* — a development tool executing arbitrary CDN
JavaScript on authenticated pages. A CDN compromise is full session takeover across every tenant, and
it's a hard third-party availability dependency in front of the UI.

CSRF rests entirely on `SameSite=Lax` — no token, no `Origin`/`Referer` check anywhere. `Lax` is
*site*-scoped, not origin-scoped. With C-6, the two compose.

---

## Medium

### M-1 — Rejected mail is accepted, then bounced best-effort

`smtp/server.rs:452-457` answers `250 2.0.0 Message processed (<reason>)` even when
`ingest.accepted` is false — the sending MTA records successful delivery. Telling the sender then
falls to a **detached, non-durable** `tokio::spawn` (`server.rs:447`, `sendgrid.rs:141`) calling
`handle_bounce_dispatch`, which is `let _ = adapter.dispatch_bounce(bounce).await`
(`thread/mod.rs:507-521`) — result discarded, not even logged.

A bounce that fails, or is in flight at SIGTERM, is silently lost while the sender believes the mail
was delivered. It's also backscatter: bouncing to a forged sender is how a relay gets blocklisted.
Under a spam flood it's one detached task and one fresh TCP+TLS handshake per rejected message.

**Your call on this one was "both, per reason"** — `550` at DATA for hard rejections the system is
certain about (unknown channel, ACL denial), durable outbox bounce for softer ones. Noted for
whenever this gets implemented; it's the right split.

Same fire-and-forget shape: stop notices (`task_worker.rs:1118`) and confirmation codes
(`outbound_dispatcher.rs:180-220`, sent inline in the HTTP request, no retry).

### M-2 — The outbox is only half a transactional outbox

The **approval** path does it right — task pause + outbox insert in one transaction
(`approval.rs:142-232`). The **agent reply** path does not: `enqueue_outbound_send` (`task.rs:1145`),
`save_outbound_messages` (`dispatch.rs:705`) and `update_claimed_task_payload` (`task.rs:1370`) are
three separate transactions.

Crash after the outbox insert but before the thread write → the reply *is* delivered, but no outbound
message exists, so the H-3 guard sees nothing, **re-runs the agent (second LLM charge)**, and
`ON CONFLICT (idempotency_key) DO NOTHING` then swallows the second reply. The email the customer
received and the answer stored in the thread are **different texts under the same Message-ID**.

Separately, SMTP send and `mark_outbox_email_sent` are two operations (`task_worker.rs:541-555`) — die
in between and the reaper re-sends. The stable Message-ID means MUAs often collapse it, but an
external MTA will deliver two copies. **Net: at-least-once everywhere.** A retry cannot normally
re-send an email; it *can* re-charge an LLM call.

### M-3 — Tasks parked in `pending_approval` are stuck forever

`expire_pending_approval` (`approval.rs:443`) is only reached lazily when a human clicks an
already-expired link. There is no sweeper — `run_maintenance` (`task_worker.rs:282-296`) only reaps
outbox leases and quorum timeouts. `background_tasks.wait_expires_at` and its index
(`init_schema.sql:375-377`) are **written in seven places and read in none** — the index is dead, and
`task.rs:1002` sets the column to `NULL` on entering `pending_approval` anyway. A task whose approver
never clicks sits there indefinitely, holding its thread and outreach state.

### M-4 — Missing indexes on the two hottest paths

- **`email_messages(message_id)`** — thread resolution runs on every inbound email
  (`thread.rs:411-418`) and binds `t.channel_id = $1 AND em.message_id = ANY($2)`. The only covering
  index is `UNIQUE (company_id, message_id)` with `company_id` leading, which can't serve it. Cost
  grows linearly with channel history on the single hottest path in the product.
- **`task_attempts(started_at)`** — the table has only a PK and `UNIQUE (task_id, attempt_number)`,
  while three dashboard panels filter on `started_at` (`dashboard.rs:156-172`, `:236-250`) and the
  code's own comment (`:218`) notes the dashboard re-reads *every five seconds for every connected tab*.

Also unindexable by construction: the Outlook `Thread-Index` fallback puts the column on the *pattern*
side — `$2 LIKE em.thread_index || '%'` (`thread.rs:441-448`) — plus an `ORDER BY length(...)`
expression sort. Full scan of the channel's messages whenever that fallback is reached.

### M-5 — `background_tasks` is never pruned, and two dashboard aggregates have no time bound

`dashboard.rs:43-50` and `:67-74` aggregate `background_tasks` and `email_outbox` **for all time**,
with no window and no LIMIT, every 5 s per connected tab — and for the operator view, across all
tenants. `QUEUE_DEPTH_BODY`'s own doc comment (`:186`) says it out loud: *"which is only possible
because `background_tasks` is never pruned."*

There is no retention job, no partitioning, no archival anywhere in the repo. This is the biggest
long-term scaling liability in the data layer.

### M-6 — LLM prompt history is unbounded by tokens

`thread.rs:614-623` caps at 200 *messages*; `agent_runner.rs:1085-1098` then concatenates **all** of
them into one string with no token budget, truncation, or summarisation. A long thread will exceed
context and cost real money. Compounding it, `MESSAGE_SELECT` pulls `raw_text_body`, `raw_html_body`
and the `attachments` JSONB for all 200 rows — none of which the prompt uses.

Confirmed N+1 alongside it: `dispatch.rs:161-165` reloads the full history *inside* the per-matched-channel
loop. `DirectoryCache` memoises companies/channels/agents/memberships but not thread history.

### M-7 — Blocking and unbounded work in the ingress request path

`protocols/email/ingress.rs:28-38` uploads attachments to GCS **synchronously**, inside the webhook
handler and inside the SMTP `DATA` transaction, with a 30 s client timeout per request
(`storage/gcs.rs:45`) and potentially several attachments. That's past SendGrid's webhook timeout —
which makes SendGrid retry, and re-ingest.

Inconsistently, the webhook sets no `DefaultBodyLimit` (only `ui_uploads.rs:50` does), so axum's 2 MB
default applies and real mail with attachments silently 413s — while SMTP advertises 20 MB. The two
ingress paths disagree and neither limit is the enforced one.

### M-8 — Secrets: LLM keys plaintext at rest, and recorded into tracing spans

Covered at rest in `plan/db_improvements.md`. Two things that document doesn't cover:

- **Logged.** `AgentWrite` derives `Debug` and carries `api_key` (`use_cases/agent.rs:30-43`).
  `#[instrument(skip(self))]` on `create_agent` (`:135`) and `update_agent` (`:276`) skips only
  `self`, so `write` — including the raw key — becomes a span field. Same shape for
  `ChannelWrite.api_key` (`channel.rs:35`, `:224`, `:352`). `.env.example` ships
  `RUST_LOG="mail_agents=debug,..."`.
- **Interpolated into YAML.** `llm_guardrail.rs:75-80` does `format!("...api_key: {}...")` — a key
  containing a newline injects YAML.

Also PII: `ingest.rs:233-236` logs `From:` and the full `To:` list at `info`.

### M-9 — Session and login gaps

The signing side is solid — HS256, startup-enforced 32-byte minimum, `exp` required, `HttpOnly` +
`SameSite=Lax` + derived `Secure`, Argon2id at the OWASP baseline, constant-time verification. Gaps:

- **No revocation.** Stateless JWTs, 30-day default TTL, no `jti`/`token_version` in `SessionClaims`.
  A stolen token stays valid for the full TTL; changing a password does not invalidate sessions.
- **Username-enumeration timing oracle.** `login` (`user.rs:507-536`) returns `InvalidCredentials` at
  `:515` for an unknown identifier, *before* the Argon2 verify at `:526`. Unknown user answers in
  microseconds; existing one pays a full hash. No dummy-hash path.
- **No rate limiting on authentication.** No limiter in `Cargo.toml` or `infra/app.rs:36-51`.
  `/login` and both JSON login routes are unthrottled.
- Approval links are emailed to external approvers but `approval::router()` is merged **inside**
  `protected` (`routes/mod.rs:66`), so `/approvals/{token}` requires a session — likely a functional
  break for non-user approvers. The URLs are also built as **`http://`** (`approval.rs:74-82`), so a
  bearer credential authorizing outbound mail travels in the clear.

### M-10 — Prompt injection: the only guardrail is off by default and is an 8-phrase blocklist

Inbound email becomes the prompt with no delimiting or untrusted-data framing
(`thread/support.rs:263-283`). The approval gate is the real control and it's well built — but:

- **The approver is chosen loosely.** `dispatch.rs:315-333` picks the *first* non-`@public`
  participant, else the *first* entry from `list_company_team_emails(...).await.unwrap_or_default()`.
  That `unwrap_or_default()` is precisely the pattern `src/AGENTS.md` prohibits on authorization
  paths — it happens to fail closed, by luck rather than design. "First element of an unordered list"
  is not a defensible way to name the human who authorizes outbound mail.
- **The guardrail is disabled by default** (`ENABLE_LLM_SPAM_GUARDRAIL = false`, `config.rs:326`) and
  `static_pattern_check` (`llm_guardrail.rs:18-38`) is a `contains` scan for 8 literal English
  phrases — defeated by paraphrase, translation, or a zero-width space.

### M-11 — Essentially no per-tenant limits

- HTTP: no rate-limit layer at all.
- Inbound: one per-**thread** cap (`MAX_THREAD_MESSAGES_PER_HOUR = 60`). No per-company, per-channel
  or per-sender cap — an attacker on the C-2 path opens a new thread per message and never touches it.
- LLM spend: **uncapped**. Token counters are recorded but nothing reads them as a budget; no quota
  exists in config or the `companies` table. Every inbound message spends the tenant's key.
- Outbound: bounded only by `max_targets = 50` per outreach call, with no per-hour or per-day ceiling.

---

## Lower

- **`/metrics` leaks across tenants.** `routes/monitoring.rs:9` is inside `protected` but has **no
  operator check**, unlike `ui_dashboard.rs:137`. Any authenticated user of any company reads global
  AI token spend, task counts and SMTP rejection counts. It's also `Json`, not Prometheus exposition
  format, so nothing can scrape it; `increment_counter` drops labels entirely; `record_histogram`
  degrades to a counter so there are **no percentiles anywhere**; and all counters are in-process
  `AtomicU64` that reset on every deploy and partition across instances.
- **No correlation ID crosses webhook → task → agent → outbox.** `infra/app.rs:41-50` mints a fresh
  UUID per HTTP request, doesn't read `X-Request-Id`/`Fly-Request-Id`, doesn't echo it, and the span
  dies at the handler. Answering *"why did this email not get a reply"* means manually chaining
  `message_id` → `task.id` → `outbox_id` across four unstructured log lines. Worker logging is
  string-interpolated (`task_worker.rs:270` and five siblings), so the JSON layer emits one opaque
  `message` field you cannot filter on.
- **Nothing is alertable.** No dead-letter counter, no export; `record_task_metric` reports `Failed`
  identically for a retry and a dead-letter. Nothing at all is emitted for a *stuck* task — which,
  per H-1/H-4/M-3, is the failure mode you actually have.
- **Startup logs are lost.** `init_tracing()` is called inside `create_app` (`app.rs:14`), which runs
  *after* `init_app_state()` (`main.rs:22` vs `:60`) — migrations, config and GCS validation all log
  to a subscriber that doesn't exist yet, and a config `panic!` produces no structured log.
- **`File::create("app.log")` in the container** (`setup.rs:130`): no rotation, unbounded growth,
  ephemeral disk, invisible to `fly logs`, and `expect` panics the boot on a read-only FS. The
  `EnvFilter` fallback is `"axum_trainer=debug"` (`:120`) — a stale crate name; with `RUST_LOG` unset
  the app emits nothing from `mail_agents`.
- **No readiness endpoint.** `/health` returns 200 unconditionally (deliberate, documented, correct
  for liveness) — but a rolling deploy will route traffic to a machine that cannot reach Postgres.
- **Dockerfile runs as root** — no `USER` directive in the runtime stage, while listening on public
  port 25. Otherwise clean.
- **Pool: 10 connections, one permanently held.** `infra/db.rs:14-22` sets only `max_connections`
  (default 10) and `acquire_timeout`; no `statement_timeout`, no `idle_in_transaction_session_timeout`,
  no `max_lifetime`. `PgListener` holds one for the process lifetime → 9 usable, shared by four poll
  loops, HTTP, and every 5 s dashboard tick. `DATABASE_MAX_CONNECTIONS` isn't set in `fly.toml`.
- **Untimed `reqwest` in auth paths** — `routes/user.rs:371`, `routes/apple_auth.rs:311/370` use
  `Client::new()` with no timeout. GCS (30 s), spam scanner (5 s) and HydraDB are all correctly bounded.
- **A fresh SMTP transport per outbound email** (`outbound_dispatcher.rs:399-413`). `lettre`'s
  transport holds the connection pool and is meant to be built once and shared; as written every
  email pays a new TCP + TLS + AUTH handshake, up to 20/sec from the outbox. No `.timeout()` set either.
- **Unescaped channel name in the `From:` header** — `format!("\"{name}\" <{addr}>")`
  (`outbound_dispatcher.rs:330-334`), then parsed as a `Mailbox`. Only slugs are validated
  (`channel.rs:549`); `name` is free user text. A `"` either breaks the parse — `AppError::Internal`,
  retried to dead-letter, so that channel can never send — or injects header structure.
- **Missed scheduler slots are collapsed to one run** (`schedule.rs:449-461`) — deliberate and tested,
  but an hourly digest that misses six hours produces one digest, with no backfill and no record that
  slots were skipped. The round-trip through local wall time also means a sub-daily interval crossing
  a DST boundary lands on an ambiguous or non-existent local timestamp and the cadence shifts.
- **`schedule_runs` CHECK vs `ON DELETE SET NULL`** (uncommitted migration, `:6-7,14-15`): deleting a
  `threads` row fires `SET NULL` on `thread_id`, which is an UPDATE, which re-evaluates
  `CHECK (task_id IS NULL OR thread_id IS NOT NULL)`. If `task_id` is still set the CHECK fails and
  the parent DELETE aborts. Also `record_run_task` (`schedule.rs:489-501`) ignores `rows_affected`,
  unlike every other write in that file.
- **Compile-time SQL checking is ~10% real.** 23 `query!`-family call sites vs **199** runtime
  `sqlx::query(...)`, 31 of them assembled with `format!`. `docs/deploy.md` claims the persistence
  layer is compile-time validated; `SQLX_OFFLINE=true` in the Dockerfile therefore validates almost
  nothing.
- **`docs/deploy.md` tells you to set `OPENAI_API_KEY`, and nothing reads it.** Keys resolve only
  from `agents → channels → companies → config_json` (`agent_runner.rs:348-377`), failing at the
  first email. Similarly `setup.rs:38-42` claims an unreachable bucket "should stop the boot", but
  `GcsFileStorage::from_config` only decodes the key — no network probe.
- **No CI at all** (no `.github/`) against 463 tests, while `src/AGENTS.md` requires regenerating
  `.sqlx/` after any query change — a stale cache either fails the Fly build or silently builds the
  old query shape. There is also **no test that runs two claimants concurrently against the same
  rows**, which is exactly the gap H-4 and H-2 fall into.
- **`ai-agents` is an unpinned git dependency** (`Cargo.toml:29`, no `rev`/`tag`). `Cargo.lock` pins a
  commit and the Dockerfile builds `--locked`, so builds are reproducible today — but any
  `cargo update` moves the whole LLM orchestration layer to a third party's HEAD, and repo deletion
  makes the project unbuildable.
- **CORS defaults to `http://localhost:5173`** with `allow_credentials(true)` — harmless if
  overridden in deploy, a footgun if not.
- **README drift** — states a 20-messages/hour thread limit (code: 60, `thread/mod.rs:58`) and "three
  independent loops" (code: four). Outbound mail is `TEXT_PLAIN` only, no `multipart/alternative`,
  despite the README's premise of blending into human threads.

---

## Cross-cutting themes

Four patterns explain most of the individual findings:

1. **Trust is decided from attacker-controlled strings.** C-1, C-2, C-3 are all the same root cause:
   authentication verdicts travel as `Option<String>` compared with `== "fail"`, sourced from data
   the sender controls. `src/AGENTS.md` already prescribes the fix (parse to an enum at the adapter
   boundary, exhaustive match) — it just hasn't been applied here. `ingest_status` matching rejection
   *strings* (`server.rs:691-707`, whose own comment admits arms are miscategorized) is the same
   problem waiting to become security-relevant.

2. **Durability stops at the outbox boundary.** Inside it, the design is careful. Outside it —
   bounces, stop notices, confirmation codes, schedule-run materialisation, the reply/thread/payload
   write split — work is fire-and-forget or split across transactions, and process death loses it.

3. **Nothing is bounded.** No message size, no session timeout, no connection cap, no agent
   wall-clock limit, no concurrency limit, no per-tenant quota, no LLM budget, no data retention.
   Individually minor; together they mean the failure mode under load is always "the single 1 GB
   machine falls over" rather than "requests get shed."

4. **Ports are ~70% honoured.** Four traits — `TaskPersistence` (`persistence/task.rs:308`),
   `ApprovalPersistence`, `SchedulePersistence`, `DashboardPersistence` — are defined **inside the
   adapter they abstract**, so the application layer imports upward (`agent_runner.rs:1`,
   `task_worker.rs:9`, and four more). The mail provider isn't behind a port at all —
   `outbound_dispatcher.rs:1-11` imports `lettre` directly in the application layer, bypassing the
   `ProtocolEgressAdapter` that exists for it. It's a file-move fix, but the dependency arrow is
   inverted today. Related: 12 port methods have silently-safe default bodies to keep the 38-method
   trait mockable — including `renew_task_lease() -> Ok(true)`, a default of "the lease always
   renewed" on the method whose entire job is detecting that it didn't.

Also worth noting against the project's own rules: **23 files exceed the 1,000-line limit** in
`src/AGENTS.md:126` (largest: `thread/tests.rs` 4621, `pages/tests.rs` 4070, `persistence/task.rs`
2991), and **11 inline test modules exceed the 500-line limit** — with `MockTaskPersistence` /
`MockThreadPersistence` re-declared independently in nine places, exactly the duplication that rule
was written to prevent.

---

## If and when you implement: suggested order

Roughly by (risk × cheapness):

1. **C-1** — delete the `Authentication-Results`/`Received-SPF` header override in
   `smtp/server.rs:766-793`. A trusted upstream verdict would need an explicit trust-boundary
   config; today there is no upstream, so the fix is deletion. Smallest diff, largest risk reduction.
2. **C-3** — `AuthVerdict` enum parsed once at the adapter boundary, exhaustive match, fail closed.
3. **C-2** — required `INBOUND_WEBHOOK_SECRET`, validated at startup the way `JWT_SECRET` is
   (`config.rs:224-229`), constant-time compare, 401 before parsing.
4. **C-4** — scope the three `ui_schedules.rs` reads through the channel, copying
   `ui_attachments.rs:83-95`.
5. **C-5** — `relay()` instead of `builder_dangerous`, and match the `!= "localhost"` guard at `:400`.
6. **C-6** — `escape_html_text` on the `simulation.rs` inbound branch and the other listed sinks;
   **H-8** CSP as the backstop, plus SRI or vendoring the three CDN tags through the existing
   `include_bytes!` asset router.
7. **H-1** — count a lease steal as an attempt (the outbox already does this at `task.rs:1233`).
8. **H-3** — don't let an agent error become an outbound body; distinguish "failed" from "replied
   with a failure message" so retries work.
9. **H-4** — lease the `schedule_runs` pending claim, add attempt/backoff/terminal columns, and scope
   the query per tenant. Worth doing **before** the uncommitted migration ships.
10. **H-6 / H-2 / H-5 / H-7** — bounded worker pool + `tokio::time::timeout` on the agent run;
    cancellation token instead of dropping the future; SMTP size/timeout/connection caps; join the
    worker handles on shutdown and release leases.
11. **M-4** — two indexes, two lines, the largest measurable performance win available.
12. Then the observability and retention work (correlation IDs, Prometheus, `background_tasks` pruning).

Two things worth reproducing empirically first, because both are silent and cheap to trigger:
`kill -9` mid-agent-run and watch `retry_count` stay at 0 across re-claims (H-1); and delete a channel
that has an enabled schedule, then watch whether any other schedule ever fires again (H-4).
