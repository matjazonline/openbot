# Workflow release verification

The phase10 acceptance gate runs against PostgreSQL 18.6, fresh migrations and isolated databases
for unscoped queue operations. Scripted model and HTTP endpoints are loopback-only; the test network
guard denies external egress. The reviewed-email scenario uses the real delivery worker and frozen
email renderer with a scripted accepting transport boundary. It does not establish SMTP provider
readiness. The [cutover pilot](workflow-operations.md#deliberate-fresh-cutover) must establish that on
the selected deployment.

This is an executable test map, not a claim that naming a test means it passed. Commands, results,
artifact identities and independent findings are archived at release commit `19bab48` in
[phase10 TASK](https://github.com/matjazonline/openbot/blob/19bab48c28c76a929f20a19f46e44bfb75c24cae/plan/workflow-foundation/phases/p10/docs/TASK.md) and
[REVIEW](https://github.com/matjazonline/openbot/blob/19bab48c28c76a929f20a19f46e44bfb75c24cae/plan/workflow-foundation/phases/p10/docs/REVIEW.md). Final acceptance is recorded there; the completed plan directory was subsequently removed.

| Plan10 scenario | Regression evidence |
| --- | --- |
| Reviewed response | `workflow_cutover_reviewed_email_delivers_exact_accepted_artifact`: real email admission, classifier/context and model output visible in the human proposal, stale artifact rejected, comment does not advance, duplicate acceptance replays, exact edited text and source threading accepted once by the delivery worker. |
| Agent capability defaults | `workflow_capabilities_agent_receives_exactly_its_saved_tools_and_skills_in_order`, `workflow_capabilities_agent_without_saved_capabilities_receives_none`, `workflow_capabilities_step_and_workflow_narrowing_inputs_are_rejected`. Per 06-D5, profiles/overrides are excluded; explicit empty/invalid fields cannot broaden into defaults. |
| Human feedback | `reviewer_comments_revises_reads_next_draft_and_accepts_exact_artifact`, `workflow_repeat_feedback_reaches_next_round_and_last_round_accept_is_done`; new reviewed-email journey retains original proposal, comments and accepted data. |
| Alternate route | `agent_model_step_tests.rs`, `decision_tests.rs`, registry route/output validation and batch completion tests: declared label/choice enforcement and exactly one eligible successor. |
| Concurrent messages | `message_ingress_end_to_end_each_message_gets_its_own_run_and_reply`, `workflow_wait_same_thread_messages_remain_independent`: separate admissions/replies and independent waits in one thread. |
| Immutable execution | `workflow_admit_sql_exact_snapshot_job_and_replay_preserves_deadline`, `workflow_admit_sql_stale_selection_conflicts_and_saved_replay_survives_removal`, agent checkpoint/publication and authority suites. |
| Duplicate ingress | `workflow_admit_sql_competing_same_key_and_canonical_source`, message intent replay tests. |
| Competing workers | `workflow_io_real_competing_claims_lost_ack_crash_backoff_and_frozen_retry`, `workflow_message_publish_competing_dispatchers_publish_once`, action competing-entry tests, new stale-evidence integration. |
| Human races | `workflow_decision_two_reviewers_compete_for_one_successor`, `workflow_decision_submit_competes_with_timeout`, `workflow_decision_submit_competes_with_cancel`. |
| Recovery | Lease, batch, pending, agent, message and reconciliation recovery suites inject failures at durable boundaries and reconstruct workers/services; assertions retain committed outputs, saved calls, receipts and pending continuations. This is not an OS-kill test of every possible instruction boundary. |
| External uncertainty | `workflow_cutover_http_historical_evidence_never_authorizes_an_extra_effect`: actual lost response after effect, service reconstruction, replay/stale proof rejection, competing claimants, counted effects and exact durable evidence. See the [reviewed historical diagnostic disposition](workflow-reconciliation-safety.md); the exact public historical INSERT diagnostic remains unexecuted. |
| MCP over HTTP | `mcp_*tests.rs` in persistence/workflow and adapter transport suites: explicit calls, JSON/SSE, bounded schemas, revocation, cancellation, uncertain outcomes and shared recovery. |
| Partial delivery | `workflow_message_receipt_failed_delivery_takes_final_error_and_never_resends`, delivery worker multipart/recipient tests, message control-retry recovery: accepted destinations/parts stay accepted when siblings fail. |
| Workflow tools | `workflow_tool_resumes_the_pending_call_after_a_child_human_decision`, `workflow_tool_dispatch_starts_one_child_per_call_and_one_resumer_continues`, `workflow_tool_parent_cancel_cascades_and_a_restarted_worker_resumes_the_call`. |
| Bounded repetition | `workflow_repeat_feedback_reaches_next_round_and_last_round_accept_is_done`, `workflow_repeat_exhaustion_takes_its_route_and_never_approves`, budget and cancellation tests. |
| Authorization | `workflow_decision_revoked_reviewer_and_foreign_company_are_hidden`, HTTP/MCP authority tests, run/decision route visibility tests, revoked-delivery actor tests. |
| Sample run | Sample persistence/HTTP/worker tests and `scripts/workflow-sample-boundary.py`: sealed sample policy, no production delivery or external action entry. Browser sample form tests preserve validation and navigation behavior. |

Wait maintenance is covered by `wait_deadline_tests.rs`, decision timeout/race tests,
`child_*tests.rs`, `message_tests.rs` (unknown/deadline/poison receipt), and reconciliation/budget
recovery suites. Fairness/capacity uses real competing workers in
`workflow_fairness_real_loop_mixes_full_unsupported_poison_wait_hung_and_refill`,
`workflow_fairness_sustained_refill_dispatches_quiet_handlers_in_bounded_turns`, complementary-worker
and full-page tests. Metrics tests check real owner-created state, read lock timeouts and recovery,
clearing values, unavailable samples and cancellation of pending reads.

CI retains formatting, locked offline compilation, Clippy, fresh migrations, SQLx prepare checking,
database-backed tests with network denial, transport/sample boundaries, browser protocols, lease
budget and the stock 2 MiB stack suite. CSS generation scans only `src/**/*.rs`; `npm run check:css`
builds to a temporary file and compares bytes without mutating the tracked asset. Dependency locks
and pinned git revisions remain in force, and the production container remains non-root.

The context graph could not be refreshed in this workspace: `graft`, indexed graph files and a
graft connector were unavailable. Phase10 used the scoped source/owner inventory archived with its review records instead;
run `graft build` in an environment with the repository's graph tool before relying on graph spans.


## Channel response verification

Use disposable PostgreSQL18 databases and the loopback network guard. The focused response groups
are `channel_reply_e2e_`, `channel_reply_authoring_`, `channel_reply_views_` and
`channel_reply_controls_`:

```sh
scripts/test-network-isolation.sh cargo test --locked --offline --lib channel_reply_e2e_
scripts/test-network-isolation.sh cargo test --locked --offline --lib channel_reply_authoring_
scripts/test-network-isolation.sh cargo test --locked --offline --lib channel_reply_views_
scripts/test-network-isolation.sh cargo test --locked --offline --lib channel_reply_controls_
```

The end-to-end group uses the production workflow composition, real PostgreSQL and delivery
worker, and a counted scripted provider. It checks independent automatic replies with observers,
silent completion and explicit replies, safe retry without another model call, quarantined
uncertainty, human rounds and concurrent waits, child continuation, frozen contracts, exact final
approval, quiet ingress and in-app publication. Recovery cases reconstruct the engine with a
committed final intent and interrupt a provider after remote acceptance before its receipt returns.
These checks establish the stored identities and scripted effects, not live provider readiness.
Existing focused database races and transaction fault tests retain coverage of finalization,
publication acknowledgements, cancellation, current authority and decision settlement.

Authoring coverage includes settings authorization/CAS, invalid projection, preserved and escaped
stale input, native create replay after explicit clear, competing create claimants, rollback of
invalid custom terminal routes, edited approval/rejection, all five compiling/executing examples,
and sealed sample response identity. Views cover independent execution/publication/transport states,
source identity outside the message window, response pagination, current classification/tenant
visibility and real transaction-only delivery wakeups. Controls retain accepted/unknown truth and
provide no blind resend or completed-business retry button.
Nested local questions and formal reviews wake the actual root even while intermediate runs
remain parked for their children; their notifications carry identifiers and state only. Readers
recheck current visibility and reread the stored question/review before rendering it.

Affected checks also include native provisioning/onboarding, template/fixture composition, registry
terminal routes, authoring/sample/run/conversation/decision browser protocols, capability-boundary
checks, `npm run check:css`, formatting, locked offline compilation/Clippy, fresh migrations and
actual `cargo sqlx prepare` followed by `cargo sqlx prepare --check -- --all-targets`. Set owned
`DATABASE_URL` and `TEST_DATABASE_URL`; never use a development/production reset for this purpose.
Keep stock2MiB test stacks for focused runtime evidence. Run only affected tests during this task,
including final integration, per the explicit targeted-only instruction; do not claim a full-suite
pass. Exact executed commands, nonzero selected counts, failures and independent review identities
belong in plan03 phase records. A test name in this map alone is not passing evidence.

## Autonomous agent history verification

History verification uses fresh PostgreSQL fixtures and targeted test filters. Naming a test here
is not a claim of release acceptance; current commands, failures, rechecks and immutable evidence
are in `plan/04-autonomous-agent-history/phases/`. No full-suite run is implied by these focused checks.

- `agent_history_views_`: native pages reach the actual last exchange, one selected source is
  outside the first 16-item page, private other-admin access matches current Inspect, restricted
  projection omits payload/source/revision/omission data, and the actual API query handler encodes
  only the projected resource. Frozen owning context survives selected-source payload expiry.
- `agent_history_level_history_reader_`: one-shot pauses after the actual payload fetch allow
  member-level demotion and current in-flight classification rise before final projection.
- `agent_history_retention_`: real competing collectors, real 127-call approach to a 128-record
  final-slot race, replay at capacity, JSON-null output proof, truthful hole/prefix boundaries,
  source cascades, transaction rollback and deferred commit abort. Clock setup is confined to each
  OwnDatabase and restores native guards before testing expiry/retirement. Strict model scripts
  stay within their existing per-scenario bound; no test/runtime limit was raised.
- The other actual storage limits (2048 company payloads, 4096/32768 metadata and 2048
  conversations) have competing native boundary checks. Historical cancelled prefixes keep real
  FKs/CHECKs and the actual policy constants; USER lifecycle triggers are disabled only while
  loading that prefix in an OwnDatabase transaction, then restored before genuine producers.
  These are count/pressure fixtures, not a claim of 32768 live end-to-end model journeys. Sparse
  streams exercise pressure expiry down to the real 1536 soft ceiling. Empty identity fixtures
  use real native agents/scopes with all guards enabled and protect live resolved identities.
- Native owner-race checks pause actual preparation, expiry, retry and human submission transactions
  inside their OwnDatabase. Advisory-gate arrival and a conflicting company NOWAIT claim prove
  the production owner holds the shared mutex. They exercise both selection orderings and exact
  human replay after a selected source expires. A counted65-call recent-window case verifies that
  produced output in a failed own job remains pinned before its original deadline, native safe
  retry changes no preparation/output/order/charged call, and real job completion removes the pin.
- `agent_history_e2e_`: actual child processes run the production workflow worker against a
  parent-owned disposable database and counted loopback provider. Parents verify SIGKILL and
  recover through native polling/lease expiry at enqueue, in-flight acquisition, frozen
  preparation, settled model response, final checkpoint, and before successor wakeup. The
  acquisition transaction is deliberately uncommitted; these checks do not claim that a remote
  provider invocation is exactly once under an unknown outcome. Existing lost-COMMIT and
  unknown-effect cases separately retain their reconciliation assertions.
- The real knowledge journey uploads `0_sales.md` and `5_sales.md`, uses native search, then adds
  a customer CC and removes that participant before a third call. Actual requests, delivery
  payloads, frozen seeds and the current restricted inspection projection are asserted together.
  Its transport is counted test delivery, not proof of external mailbox receipt.
- `.github/workflows/agent-history.yml` runs the native journey and emits actual inspection
  markup for the browser protocol in a fresh PostgreSQL 18.6 job. CI execution is distinct from
  local execution of those commands. The ignored child entry is a technical process driver:
  required parents invoke it explicitly with an owned fixture, never as a standalone release test.
- Existing history continuation/retry/channel/source-retention tests retain their discovered names.
  Direct history deletion is now refused: old pruning fixtures use native payload expiry followed
  by eligible source-tree retirement, preserving the original replay and rollback assertions.

The actual native pagination test can emit browser fixtures without a live production account:

```sh
AGENT_HISTORY_BROWSER_FIXTURES=/tmp/openbot-history-browser \
  scripts/test-network-isolation.sh cargo test --locked --offline --lib \
  agent_history_views_maximum_page_reaches_last_source_outside_page_and_keeps_exact_frozen_copy_after_expiry
AGENT_HISTORY_BROWSER_FIXTURES=/tmp/openbot-history-browser \
  node --test scripts/tests/agent-history-browser.mjs
```

Set the existing test database environment before running database tests. If Playwright's bundled
Chromium is unavailable, `CHROME_PATH` selects an installed browser executable. The browser protocol
uses actual Rust-rendered native fixtures, vendored htmx and shared request progress/error scripts;
it checks both themes, keyboard pagination through the real last record, exact escaped input,
independent Produced/workflow status, competing reads completing out of order and failed-read
recovery. Its local response harness does not establish live server/session deployment readiness.

Keep formatting, actual SQLx prepare/check after SQL changes, locked offline compilation, fresh
migrations, source module discovery and the affected native/runtime/route/browser checks with
release evidence. Cleanup uses owned databases for global discovery/sweeps; preserve competing
claimants and post-read authority races when changing its protocol.
