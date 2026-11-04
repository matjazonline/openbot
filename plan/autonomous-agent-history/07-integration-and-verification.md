# 07 — Integration, verification, and release

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [06](06-history-views-retention-and-operations.md) and all preceding phases. Prove
default agent continuity across separate calls, its isolation and recovery guarantees, and the
absence of duplicated effects. Saving this directory is planning work; the implementation checks
below remain future requirements until explicitly executed and recorded.

## Batch 07A — End-to-end acceptance

Add `agent_history_e2e_` tests with real persistence/workers and counted scripted providers that
capture the actual model request. Include:

1. Two root workflow runs use the same agent and authorized conversation without `context.history`.
   The second provider request contains the first input/output pair and its own input exactly
   once. Original logical input records contain no recursively copied seed history.
2. Same agent in another conversation/tenant/private scope cannot read those exchanges. Different
   agents remain separate. Authorized standalone session reuse works; default standalone and
   scheduled runs without a trusted session start independently. Samples remain sealed.
3. Concurrent activations in one conversation get deterministic durable order and execute one
   at a time; another conversation progresses independently. The successor sees its predecessor's
   accepted output, and queued cancellation does not strand the next eligible turn.
4. Crash/reconstruct after enqueue, acquisition, frozen preparation, model settlement, final
   checkpoint commit, and before successor wakeup. Assert stable exchange/selection identities,
   exact retained output, and counted provider/tool calls; ambiguous outcomes remain honest.
5. An agent requests approval/human input, parks, resumes twice under the same exchange, and
   completes once. The authorized response bypasses ordinary new-turn queueing. A parent using
   the same agent through a child private branch completes without deadlock or transcript leakage.
6. Invalid output is repaired inside one exchange; terminal invalid output/failure leaves an input
   and diagnostic state but no accepted answer. An accepted output remains available after a later
   workflow failure. Model-only classify/decision steps do not acquire persistent conversation state.
7. A new agent revision/model or different output schema is used on a later call. Old pairs remain
   data under their original provenance; current instructions/schema stay authoritative. Retried
   calls retain their old frozen preparation. Historical tool IDs never cause replayed effects.
8. Long history respects model/request/checkpoint bounds, preserves complete pairs, and reports
   omissions. Cleanup racing preparation/resume preserves pinned material; retained-history limits
   and queue admission limits fail predictably before resource exhaustion.
9. Where channel auto-response is implemented, transformed publication, `channel.no_reply`, and
   failed/retried delivery leave the agent's saved output and exchange count unchanged. Otherwise
   exercise existing explicit message publication and list the pending channel-plan integration
   cases explicitly; do not label them tested or make them a hidden runtime prerequisite.
10. Retiring an agent, deleting a session/source, or revoking rights cannot expose old private
    history, break an active preparation silently, or authorize a resumed call under stale access.
11. Access levels, in one thread with an agent granted `search_knowledge` and `0_sales.md` and
    `5_sales.md` uploaded:
    - a level-5 member asks about the discount floor, and the exchange is stamped 5;
    - the member CCs a customer and asks "summarise this thread for them". The second provider
      request contains neither the level-5 exchange nor any marker of it, and nothing at level 5
      is delivered;
    - a third call after the customer is removed sees the level-5 exchange again;
    - a level-5 exchange parked on human input is refused on resume after a level-0 CC, with no
      provider call;
    - a child branch inherits the ceiling;
    - a lower-level session reuse loads only its part of the session;
    - a level-0 teammate sees "Restricted exchange" in history views.

    Run this journey once the knowledge plan's 01 and 03 are integrated. Until its search tool
    (04) exists, raise the content level with a test fixture and record that substitution.

Actual competing claimants must cover conversation creation/turn allocation, ownership, final
output acceptance, cancellation versus completion, lease replacement, human submission,
cleanup versus selection, and content-level rise versus output acceptance (02B). Sequential replay mocks alone do not prove these protocols.

## Focused checks during implementation

All prefixes are required future test families. List each with
`cargo test --locked --offline --lib PREFIX -- --list`, verify a nonzero intended selection, then
run `cargo test --locked --offline --lib PREFIX`. Record selected/executed counts and results.

| Phase | New focused prefixes | Existing regressions |
| --- | --- | --- |
| 01 | `agent_history_identity_`, `agent_history_contract_`, `agent_history_level_` | Agent preparation, registry, tenant authority, `access_level_` |
| 02 | `agent_history_storage_`, `agent_history_completion_`, `agent_history_level_` | Checkpoints, output repair, recovery, model usage, `knowledge_run_level_` |
| 03 | `agent_history_ordering_`, `agent_history_recovery_` | Claims, leases, cancellation, deadlines, child waits |
| 04 | `agent_history_selection_`, `agent_history_preparation_`, `agent_history_protocol_`, `agent_history_level_` | Provider replay/repair, input budgets, checkpoint matching |
| 05 | `agent_history_callers_`, `agent_history_children_`, `agent_history_samples_`, `agent_history_continuation_`, `agent_history_channel_`, `agent_history_level_` | Admission, provisioning, decisions, reply ingress, sample boundaries |
| 06 | `agent_history_views_`, `agent_history_retention_`, `agent_history_controls_`, `agent_history_level_` | Runtime/run UI, lifecycle, query plans, cleanup |
| 07 | `agent_history_e2e_` | Existing workflow and agent release scenarios |

Broaden focused checks early at shared admission/checkpoint/protocol seams. Do not run the full
suite after every batch. Use `own_database` for unscoped claims, cleanup, or assertions on global
state. Never enable missing-database skipping and never point queue tests at development data.

For each SQL-bearing batch, migrate a fresh disposable database and run
`SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets`; retain required `.sqlx/` changes. Runtime
SQL needs database execution even when metadata does not change. Schema changes follow the reset
policy; do not add history backfills or old/new checkpoint decoders.

Local setup example, assuming an available PostgreSQL role matching the local user:

```sh
history_plan_db="open_bots_agent_history_$(date +%s)_$$_test"
createdb "$history_plan_db"
export DATABASE_URL="postgres://$(whoami)@localhost:5432/${history_plan_db}"
export TEST_DATABASE_URL="$DATABASE_URL"
export SQLX_OFFLINE=true
SQLX_OFFLINE=false cargo sqlx migrate run
```

Use isolated PostgreSQL 18.6 with CI's query-statistics settings for the final gate. Do not
reconfigure a shared server. Provision locked Rust/Node dependencies, SQLx CLI, and Chromium
before offline/network-isolated checks. Clean up only the explicitly created disposable database
after all workers/tests stop and evidence is recorded.

## Batch 07B — Final full-suite gate

Run once after the whole implementation is integrated, before declaring completion. Reconcile
the list against [CI](../../.github/workflows/ci.yml) at implementation time; the current local
`scripts/check.sh` is not a substitute for all CI gates. Record revision, environment, commands,
results, and executed test counts.

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

Execute CI's query-statistics activation assertions as well: the isolated database must expose
the probe's query ID, `pg_stat_statements.track=top`, `track_utility=off`, and both query-parameter
logging length settings at zero. Preserve CI's production/security gates and add new history
boundary checks there if needed; do not leave required protocols as local-only scripts.

Run the fixture-backed browser gates:

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

After fixes, rerun affected checks; repeat the full suite when the impact warrants it. Report
unavailable database/browser/network-isolation checks and pending related-plan integrations
explicitly. A missing database, zero-match filter, skipped suite, or failed required gate is not
full verification. If a resource limit rises, retain and document the CI signal that fails before
the new limit is exhausted; preserve stack and lease budget checks.

## Release acceptance

- All seven phases are integrated. Every ordinary agent call records its input and accepted
  output and automatically loads the authorized retained history without workflow assembly.
- No unresolved cross-scope disclosure, duplicate exchange/effect, stale-owner write, mutable
  retry context, parent/child deadlock, or stranded queue head remains.
- Current instructions and access policy are authoritative. History is ordinary data; prior
  output never grants capabilities, approves actions, or promises external delivery.
- Runtime, schema, offline metadata, examples, samples, and documentation agree. Independent
  agent-revision and channel-response integrations are reported accurately for the checked-out
  feature set. Workflow-supplied conversations and summarization are explicitly future work.
- Refresh the graph with `graft build` after substantial implementation and inspect the refresh.
  Keep every file in this directory registered in `NO_BACKWARD_COMPATIBILITY_NOTES` in `src/main.rs`.

## Planning-only verification record — 2026-11-03 (access-level integration)

- Access levels from `plan/knowledge-access-levels/` were integrated into phases 01–07. This was
  a documentation-only change: no Rust, database or browser suites were run.

## Planning-only verification record — 2026-11-03

- `cargo fmt --all -- --check`: passed.
- `git diff --check`: passed.
- A local Python document audit passed: seven consecutively numbered phase files, all 42 local
  Markdown links resolve, every file carries the database-policy note and verification guidance,
  every file is registered exactly once in `NO_BACKWARD_COMPATIBILITY_NOTES`, and new-file
  whitespace is clean.
- Source scope of this planning change is the seven required startup-warning paths. Existing
  autonomous-agent and channel-handles-send plans and their registrations were preserved.
- No agent history implementation or schema reset was performed. Runtime, compilation,
  database-backed, and browser suites were not run for this documentation/path-list change;
  the implementation gates above remain future requirements.
