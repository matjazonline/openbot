# 07 — Integration, verification, and release

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [06](06-authoring-observability-and-operations.md) and every earlier phase. Prove the
complete receive/process/respond and HITL journeys, including contention and recovery. This file
defines future implementation checks; saving the plan does not mean those checks have run.

## Batch 07A — End-to-end acceptance

Add `channel_reply_e2e_` tests that use real persistence, workflow workers, response publication,
and the delivery worker with a counted scripted transport/provider. Include these journeys:

1. Incoming message starts its designated responder, produces typed output, and receives one
   correctly threaded automatic reply with no explicit messaging step. Multiple observers
   produce no extra automatic response. Independent messages in one thread each get their own.
2. Conditional `channel.no_reply` preserves the business result, creates an inspectable suppressed
   outcome, and sends nothing. Explicit intermediate replies survive; an explicit final reply
   followed by silent completion produces no automatic duplicate.
3. Business effect succeeds, final response commits, provider temporarily fails, then delivery
   retries successfully. The business effect/model call count stays unchanged. An ambiguous
   provider outcome remains uncertain instead of being resent automatically.
4. A workflow requests human input, sends a prompt, remains waiting, accepts an authorized response,
   resumes the same run, and ultimately sends one final reply. Exercise multiple human rounds,
   two live waits in one thread, early replies, duplicate input, and child HITL continuation.
5. An automatic final reply needs approval. Its exact content is reviewed before terminal success;
   approval resumes the same execution and publishes only the approved response. Comments,
   rejected/expired decisions, stale artifacts, and plain untrusted reply text cannot approve it.
6. Channel bindings/contracts change while a run waits. Continuation retains its original response
   owner and content contract; current revocation can still block sending. Scheduled, manual,
   observer, and child runs never acquire a reply destination accidentally.
7. Quiet/file-only ingress, internal messages, in-app-only delivery, foreign tenant IDs, restricted
   threads, and sealed samples retain their policies through automatic and HITL publication.
8. Crash/reconstruct at each durable seam: finalization before commit; committed intent before
   wakeup; publication before acknowledgement; decision accepted before continuation dispatch;
   provider accepted before receipt persistence. Assert exact stored identities and counted
   effects, not just eventual UI status.

Real competing-claimant coverage must include responder selection, finalization, publication,
delivery ownership, two human submissions, decision versus timeout/cancel, and completion versus
cancel. A sequential replay mock is insufficient. Reuse the current database race/fault-injection
fixtures; isolate tests that sweep queues with `own_database`.

## Focused verification during phases

All prefixes below are future test families required by this plan. List each filter first and
record a nonzero selected/executed count. Run each with
`cargo test --locked --offline --lib PREFIX`; include the smallest relevant existing regressions.

| Phase | New focused prefixes | Existing areas to retain |
| --- | --- | --- |
| 01 | `channel_reply_contract_`, `channel_reply_binding_` | Binding/schema/admission, quiet ingress, samples |
| 02 | `channel_reply_terminal_contract_`, `channel_reply_terminal_` | Registry/compiler, pure/I/O completion, child result propagation |
| 03 | `channel_reply_intent_schema_`, `channel_reply_completion_` | Completion/cancellation races, approval/evidence, recovery |
| 04 | `channel_reply_publication_`, `channel_reply_delivery_`, `channel_reply_recovery_` | Messaging authority, publication races, delivery uncertainty and isolation |
| 05 | `channel_reply_hitl_prompt_`, `channel_reply_final_approval_`, `channel_reply_hitl_resume_` | Decisions, reply ingress, waits, child continuation, approval histories |
| 06 | `channel_reply_authoring_`, `channel_reply_views_`, `channel_reply_controls_` | Authoring/sample routes, source visibility, run/conversation/decision browser protocols |
| 07 | `channel_reply_e2e_` | Reviewed-email and existing workflow release scenarios |

For every SQL-bearing batch, apply all migrations to a fresh disposable database, then run
`SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets` and retain the required `.sqlx/` changes.
Runtime SQL is not validated by an empty metadata diff: execute its database tests. Do not point
these commands or queue tests at development/production data, and do not enable the missing-DB
skip. Export `TEST_DATABASE_URL` explicitly so test support cannot select an unrelated database.

Local setup example, assuming an available PostgreSQL role matching the local user:

```sh
reply_plan_db="open_bots_channel_send_$(date +%s)_$$_test"
createdb "$reply_plan_db"
export DATABASE_URL="postgres://$(whoami)@localhost:5432/${reply_plan_db}"
export TEST_DATABASE_URL="$DATABASE_URL"
export SQLX_OFFLINE=true
SQLX_OFFLINE=false cargo sqlx migrate run
```

Use an isolated PostgreSQL 18.6 instance for the final gate, with CI's query-statistics settings.
Do not reconfigure a shared local server to mimic CI. Provision dependency caches, Node packages,
and Chromium before running offline/network-isolated checks. Clean up only the explicitly created
test database after recording results and ensuring no worker/test still owns it.

## Batch 07B — Final full-suite gate

Run once after all phases are integrated, before declaring implementation complete. Treat
[CI](../../.github/workflows/ci.yml) as the authoritative inventory and reconcile any additions
since this plan was written. The current local `scripts/check.sh` omits some CI gates and defaults
its migration URL to a development database; do not treat it alone as this release gate.

Record the revision and environment, then run:

```sh
cargo fmt --all -- --check
git diff --check
cargo check --locked --offline --all-targets
cargo clippy --locked --offline --all-targets -- -D warnings
SQLX_OFFLINE=false cargo sqlx migrate run
SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets
cargo test --locked --offline --all-targets --no-run
scripts/test-network-isolation.sh cargo test --locked --offline --all-targets
scripts/test-network-isolation.sh scripts/stack-budget.sh --offline
scripts/test-network-isolation.sh scripts/lease-budget.sh --offline
npm run check:css
npm run test:run-counts
node --test scripts/tests/sse-navigation.mjs scripts/tests/agent-runtime.mjs scripts/tests/workflow-runs.mjs scripts/tests/workflow-conversations.mjs scripts/tests/workflow-samples.mjs
scripts/tests/deploy.sh
scripts/tests/credential-key-rotation.sh
scripts/tests/transport-boundary.sh
scripts/transport-boundary-check.sh
python3 scripts/tests/workflow-sample-boundary.py
python3 scripts/workflow-sample-boundary.py
```

Also execute CI's query-statistics activation assertions and fixture-backed browser gates:

```sh
export HTML_INJECTION_BROWSER_FIXTURES="$(mktemp -d)"
export APP_JS_OUT="$HTML_INJECTION_BROWSER_FIXTURES/app.js"
cargo test --locked --offline --lib dump_html_injection_browser_fixtures -- --ignored
node --test scripts/tests/team-autocomplete-browser.mjs scripts/tests/html-injection-browser.mjs

export SESSION_REVOCATION_BROWSER_FIXTURES="$(mktemp -d)"
cargo test --locked --offline --lib dump_session_revocation_browser_fixtures -- --ignored
node --test scripts/tests/session-revocation-browser.mjs

export WORKFLOW_DECISION_BROWSER_FIXTURES="$(mktemp -d)"
cargo test --locked --offline --lib decision_browser_markup_and_closed_draft_recovery
cargo test --locked --offline --lib source_message_outside_window_is_exact_and_authorized
node --test scripts/tests/workflow-decisions.mjs
```

Add new channel-reply browser/structural gates to CI as needed; don't leave required verification
as an undocumented local command. Keep stack/lease budgets and production security checks. If a
limit must increase, document the reason and the CI check that still fails before exhaustion.

Record each batch/final command, result, selected and executed test count, relevant artifacts,
and any unavailable check. After a fix rerun affected checks; repeat the full gate when its
impact warrants it. Never claim full verification with a missing database, skipped suite,
unavailable browser, failed network isolation, or unexecuted required check.

## Release acceptance

- All seven phases and end-to-end cases pass; no unresolved duplicate-publication, missed-reply,
  approval-bypass, or cross-tenant finding remains.
- Source migrations, query metadata, sample definitions, templates, and docs agree. Reset an
  incompatible database rather than introducing old/new payload handling or staged data rollout.
- Final success atomically implies a durable response decision. Human prompts/approvals retain
  independent waits and deliveries. Delivery retry never reruns completed business effects.
- Provider acceptance/local publication is labeled accurately; automated scripted tests do not
  claim readiness of a live SMTP/provider deployment.
- Refresh the graph with `graft build` after the substantial implementation. Inspect the refresh
  and record availability/results. Keep every plan file registered in the startup warning's
  `NO_BACKWARD_COMPATIBILITY_NOTES` list.

## Planning-only verification record — 2026-11-03

- `cargo fmt --all -- --check`: passed.
- `git diff --check`: passed.
- A local Python document audit passed: seven consecutively numbered phase files, all 43 local
  Markdown links resolve, every file has the database-policy note, every file is registered once
  in `NO_BACKWARD_COMPATIBILITY_NOTES`, and new-file whitespace is clean.
- Source scope of this planning change is only the required startup-warning path list. Runtime,
  database, browser, and end-to-end suites were not run for the plan and path-list update; the
  implementation gates above remain future requirements.
