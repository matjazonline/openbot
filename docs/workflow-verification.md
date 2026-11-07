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
