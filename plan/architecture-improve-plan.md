# Architecture Remediation: Remaining Work

## Summary

What is left of the critical and high remediation. Ingress security, SMTP admission, the browser
delivery hardening, and the task fencing work are all done (see below); four pieces remain:

1. **Execution bounds and shutdown** — the last of the task-execution work, and the only item
   with a live correctness gap.
2. **Mail transport** — the `MailTransport` port and the plaintext gate.
3. **HTML sinks** — the escaping audit for the legacy page shell.
4. **Deployment** — the one missing CI step.

Ordered by exposure: (1) leaves a provider call running after the process has been told to stop,
while (3) is confined to pages behind authentication.

## Already done

Recorded so this plan is read against the right tree. The full history of the earlier
remediation is in git — `plan/architecture_plan.md`, removed once its remaining work moved here,
was last current at commit `2a21d3c`.

- Every inline `on*` handler, `hx-on` expression and inline `<script>` is gone; the strict
  `script-src 'self'` CSP no longer silently breaks the legacy pages. Two regression tests hold
  the line (`rendered_markup_carries_no_inline_javascript`,
  `every_delegated_action_has_a_branch_in_the_bundle`).
- A failed agent run commits nothing — no reply body, no thread message, no outbox row — and
  `TaskExecutionOutcome` replaces `Result<(), String>`. The `"Agent execution failed:"` prefix
  protocol is gone.
- `background_tasks.execution_generation`, a per-status lease CHECK, `TaskLeaseRef`, pending-only
  claims, and `reap_expired_task_leases`.
- `commit_agent_dispatch`: reply messages, outbox row and task payload in one lease-fenced
  transaction.
- Approval suspension is fenced through `TaskSuspension`.

---

## 1. Bound and cancel task execution, and own it through shutdown

### 1.1 Remove the detached spawn

`src/application/services/agent_runner.rs:1085` still does `tokio::spawn(task.run()).await`.
`while_leased` (`adapters/persistence/task.rs`) is a correct `tokio::select!` supervisor that drops
the pinned work future on lease loss — but dropping a `JoinHandle` does not cancel the task behind
it, so today the provider future keeps running after lease loss, shutdown or timeout. This is the
single line that defeats the whole supervisor.

Await `task.run()` directly. The comment at `:1084` justifies the spawn as keeping the caller's
stack shallow; if that is a real constraint, the fix is `Box::pin` on the future or a larger stack
via `tokio::runtime::Builder`, not a task the supervisor cannot reach.

**Test:** lease loss drops the provider future. Assert on a guard type whose `Drop` sets a flag,
not merely that the call returned — the current bug is invisible to a return-value assertion.

### 1.2 `AGENT_RUN_TIMEOUT_SECS`

No timeout wraps a task execution. Add the key (default 300) to `src/infra/config.rs` following
`parse_task_worker_concurrency` (`:23-36`) exactly, including range validation and the
`#[should_panic]` test alongside it. Wrap the `while_leased` call in
`execute_single_task_with_lease`; a timeout yields `TaskExecutionOutcome::RetryableFailure` and
therefore consumes an attempt.

Only meaningful once 1.1 lands: with the detached spawn in place a timeout would abandon a
still-running provider call rather than cancel it.

### 1.3 Fair company rounds

Bounding is done — `run_bounded_task_loop` uses a `JoinSet`, claims only free capacity, and drains
on shutdown, with `TASK_WORKER_CONCURRENCY` validated in config. Only fairness is missing: the
claim orders by `run_at ASC, created_at ASC, id ASC` (`adapters/persistence/task.rs:1611`), so one
tenant with a backlog takes an entire batch.

Add a round number and order by it before age:

```sql
ROW_NUMBER() OVER (PARTITION BY company_id ORDER BY run_at, created_at, id) AS company_round
...
ORDER BY company_round, run_at, created_at, id
```

Check the plan on `background_tasks_pending_ready_idx` before committing to this; a window
function over the pending set may want the partial index extended with `company_id`.

**Test:** one company with more pending tasks than the batch size cannot fill a batch that another
company also has work in.

### 1.4 Track background work through shutdown

`src/main.rs` drops three handles on the floor — task worker (`:70`), SMTP listener (`:79`),
mailbox listener (`:83`). Only `runtime_sampler_handle` (`:55`) and `memory_worker_handle` (`:71`)
are joined (`:125-142`). All three already receive the shutdown broadcast, so cancellation is
signalled; what is missing is anyone awaiting them.

Bind all three and join them within `DRAIN_GRACE` using the existing `timeout`-then-`abort` shape
at `:125-133`. SMTP already aborts its own non-durable connections correctly
(`adapters/smtp/server.rs:171-183`); `main` only needs to sequence it.

### 1.5 Record interrupted work as retryable

On shutdown `run_bounded_task_loop` awaits in-flight tasks unbounded and records nothing, so a
deploy strands them for a full lease period before the reaper picks them up.

Note the tension to resolve first: `poll_until_shutdown`'s doc
(`services/task_worker.rs:88-95`) states that not propagating cancellation is *deliberate*,
because "an agent run cut off midway through writing its result is worse than one that finishes".
**That reasoning no longer holds.** `commit_agent_dispatch` makes the result atomic and
lease-fenced, so a cut-off run can no longer leave a half-written result — the case the comment
was protecting against is now unrepresentable. Update the comment along with the behaviour, or
deliberately keep the current stance and say why; do not leave the two disagreeing.

Either way, shutdown should mark still-leased durable tasks retryable rather than let their leases
lapse, so a deploy costs one attempt immediately instead of a lease period of silence.

**Tests:** no new work is claimed after cancellation; in-flight durable tasks end retryable or
safely leased for the reaper; every tracked handle finishes inside `DRAIN_GRACE`.

---

## 2. Put Lettre behind a `MailTransport` port

### 2.1 The port

`smtp_transport()` (`services/outbound_dispatcher.rs:552`) builds a fresh `AsyncSmtpTransport` on
**every** send — three call sites, `:206`, `:392`, `:527` — so each message pays a new TCP, TLS and
AUTH handshake. `src/adapters/smtp/AGENTS.md:38-39` forbids exactly this. It also sits in the
*application* layer importing `lettre` directly, against the dependency-direction rule.

Define `MailTransport` in the application layer, implement it in an adapter over `lettre`, and
build one shared, timeout-configured instance at startup (keep the existing 30s timeout).

Callers to move off the concrete static: `use_cases/thread/dispatch.rs:693,695`,
`adapters/protocols/email/egress.rs:59,64`, `services/task_worker.rs:676,1287`,
`use_cases/thread/ingest.rs:559`. `ConfirmationCodeSender` (`use_cases/user.rs:260`) is an existing
narrow port over the same machinery and should end up implemented in terms of the new one.

### 2.2 The plaintext gate

TLS currently holds only by accident: `relay()` defaults to a TLS wrapper and `Cargo.toml:29`
enables `tokio1-rustls-tls` with no dangerous features. Nothing asserts it, and the local-relay
escape hatch is a string compare — `config.smtp_host != "localhost"` at `:391` and `:526` — so
`127.0.0.1` and `[::1]` are treated as remote.

Add `SMTP_ALLOW_PLAINTEXT_LOCAL` (default false). Permit plaintext only when it is true, the host
*resolves* to loopback, and no credentials are configured. Fail startup when plaintext is
configured together with credentials. `is_local_domain` (`infra/config.rs:325`) is a near-miss
helper used for the app domain — extend or mirror it rather than adding a third notion of "local".

**Tests:** remote credentials can only reach a TLS relay; plaintext with credentials fails startup;
`127.0.0.1` and `[::1]` are accepted as loopback under the flag; one transport is shared across
sends.

---

## 3. Finish the HTML sink audit

`escape_html_text` (`pages/layout.rs`) escapes `& < > " '`, so it is already correct for both text
and quoted-attribute contexts, and the `/ui` pages use it heavily. The gap is the legacy shell,
where values are interpolated raw. Escape exactly once at each — do not double-escape values that
already pass through the helper.

Worst first, by what an attacker controls:

- **`pages/approvals.rs`** — zero uses of the helper. `title`, `action_title`, `approver_email`,
  `action_type`, `action_summary` are raw at `:53,57,58,59,60` and again at `:89,92,95,96,140`.
  `title` also reaches `<title>` raw via `base_layout`. These are the plan's "confirmation
  prompts", and the approval page is reachable by anyone holding a token.
- **`pages/agents.rs`** — zero uses. Agent `name`, `slug`, `provider`, `model`,
  `system_prompt_display` and the JSON `config_display` render raw; `name` also lands in an
  **attribute**, `hx-confirm="…delete agent '{name}'?"`.
- **`pages/channels.rs`**, **`pages/companies.rs`**, **`pages/tasks.rs`**,
  **`pages/simulation.rs`** — partial coverage; audit each remaining `{…}` against the helper.

Extend the hostile-value tests in `pages/tests.rs` (models at `:303`, `:3196`, `:3468`, `:4246`) to
cover what the plan named and current tests miss: confirmation prompts, the agent config JSON
display, `hx-confirm` attribute context, message IDs, and `data-*` attribute values.

Consider closing this class off rather than case by case: a newtype whose `Display` escapes, so an
unescaped value cannot reach a template by omission. Worth costing before doing the sweep by hand.

---

## 4. Close the deployment gap

One item. `package.json:10` already defines
`check:css` (`npm run build:css && git diff --exit-code -- assets/app.css`), but no CI job runs it,
so a stale committed `assets/app.css` ships silently. Add `actions/setup-node`, `npm ci` and
`npm run check:css` to `.github/workflows/ci.yml`, which today covers fmt (`:33`), locked offline
compile (`:35`), migrations (`:37`) and the DB-backed suite (`:43`).

Then document the keys added by sections 1 and 2 — `AGENT_RUN_TIMEOUT_SECS`,
`SMTP_ALLOW_PLAINTEXT_LOCAL` — in `.env.example`, `fly.toml` and `docs/deploy.md`, matching how
`TASK_WORKER_CONCURRENCY` is documented (`.env.example:6-7`, `fly.toml:22-23`, `README.md:62`).
While there, reconcile `.env.example:7` setting `TASK_WORKER_CONCURRENCY=16` against a code default
of 4.

Already complete, no action: `ai-agents` pinned (`Cargo.toml:28` + `Cargo.lock:54-56`), the
non-root image (`Dockerfile:32-42`, `EXPOSE 3001 2525`, no `setcap`, port 25 mapped externally at
`fly.toml:55-63`), and the `.sqlx` cache — which needs no regeneration for task/schedule SQL, since
those modules use runtime `sqlx::query()` rather than the macros.

---

## Working notes

- `src/AGENTS.md` is binding, and several of these items are the rules it already states
  ("Own background work through shutdown", "Preserve dependency direction", "Bound work at every
  external boundary"). Touched code must not extend an existing violation.
- The single migration is edited in place, so any schema change means a local
  `dropdb && createdb && cargo sqlx migrate run`. Tests use a derived `_test` database and never
  the development one; `TEST_DATABASE_URL` overrides the derived name.
- Regenerate offline metadata after SQL changes:
  `DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents" cargo sqlx prepare -- --all-targets`
- DB-backed tests run in parallel against one database, and sweeps like the reapers are global.
  Claim a specific task by id rather than through a batch claim, and assert on the row's resulting
  state rather than on a sweep's return count, or the test is flaky by construction.

## Final verification

Migration reset, `cargo sqlx prepare`, `SQLX_OFFLINE=true cargo check --locked --all-targets`, the
full DB-backed suite, `npm run check:css`, a Docker build, and a non-root runtime smoke test.

## Deferred

Medium and lower findings, unchanged: approval expiry, remaining notification durability,
retention, prompt-token budgeting, rate limits and quotas, session revocation and login hardening,
metrics/readiness/correlation IDs, index work supported by production query plans, and broad
module/port cleanup.
