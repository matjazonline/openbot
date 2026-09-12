# Per-address tasks — implementation and verification

Phase 1 widens inbound commits to bounded task lists, keys message tasks by company/source/channel,
returns task ids in association order on redelivery, and derives dispatch identity from the lease.
The task-less dispatch path and its unused response-review wiring are removed.

Phase 2 groups answering channels by the addressed handle. Each `+` pipeline stays sequential;
separate addresses receive separate tasks and replies. Published replies are cross-filed as
`delegation` context into the source message's other threads. Drafts are cross-filed only on approval.

## Architecture details

- A shared pure validator rejects channels assigned to different tasks before inbound writes. The
  database adapter and in-memory double use the same check.
- Normal and reviewed publication share one transactional helper. It bounds the source-thread
  lookup, locks affected threads in UUID order, publishes the reply's own associations, and inserts
  sibling context without duplicating associations.
- Cross-filing advances thread activity timestamps only for newly inserted context. Timestamp
  updates use wall-clock update time without moving an existing timestamp backwards, including
  when a transaction waits behind another publication.
- Existing threads at ingress use the same lock order while preserving the request's association
  order. This avoids lock inversion between reply publication and a follow-up with reversed
  recipient order.
- Thread context does not enqueue work, send inter-channel mail, or satisfy the receiving task's
  already-replied guard. No schema column or compatibility migration was added.

## Regression coverage

| Plan cases | Coverage |
|---|---|
| Phase 1 persistence | `inbound_multi_task_tests.rs`: multiple tasks and target rows, ordered redelivery, atomic overlap rejection, per-channel enqueue deduplication. The concurrent inbound test now submits two pipelines. |
| Phase 1 child queries | The internal cancellation/completion race includes an earlier task on a different channel with the same source; collaboration chooses the intended child and cancellation preserves the sibling. |
| 1–7 | `address_grouping_tests.rs` and the pure `pipelines` regression in `ingest/commit.rs`: chaining, To/Cc separation, passive Cc filing, channel deduplication, dropped first steps, and message-wide quiet. |
| 8–12 | `external_reply_tests/per_address_tests.rs`: real task worker and email sender with local scripted models and recorded transport; independent senders/Cc/content, upstream context confined to a pipeline, task board correlation and owners, primary-thread activity, failed sibling retry and successful recovery, and redelivery. |
| 13–17 | `task/reply_publication_tests.rs`: conversation/delegation placement, filed-only channels, reply guard, approval timing, duplicate suppression, and sources with no siblings. |
| 18 | `another_agents_reply_is_labelled_as_delegation_context` in `agent_runner/prompt_tests.rs`. |
| Concurrency and refresh | Publication tests exercise competing sibling publishers, publication against reverse-order follow-up ingress, and a transaction begun earlier publishing after a newer thread update. |

Existing application assertions required only the `task_id` → `task_ids` shape update. The existing
`test_pipeline_address_chaining_execution` remains unchanged. The concurrent inbound test's task
count changes from one to two because its fixture now explicitly submits two task requests. The
internal cancellation test moves to an isolated database to assert the sibling remains untouched,
with cleanup owned by that database fixture.

The plan's worked example is checked automatically through the production worker, persistence, and
email egress with deterministic local providers. This substitutes for the live `:3001` manual
exercise; no external email is sent and no real model credentials are needed.

## Validation

Phase 1 passed offline all-target compilation, formatting, SQLx preparation, Clippy with warnings
denied, and the complete test suite (1,489 library tests plus five main-process tests; 22 existing
opt-in tests ignored).

Phase 2 passed:

- `cargo test --locked --offline --all-targets`: 1,507 library tests and five main-process tests
  passed; 22 existing opt-in tests ignored.
- `scripts/stack-budget.sh --offline`: all 1,507 library tests passed with the unchanged 2 MiB
  thread stack.
- Offline all-target compilation, Clippy with warnings denied, formatting, whitespace checks, and
  transport boundary checks.
- `cargo sqlx prepare -- --all-targets` against the fresh migrated schema. The offline cache has no
  diff, as expected for the runtime SQL queries changed here.
- `graft build` refreshed the local context graph.

The Mac's internal volume filled during validation and its local PostgreSQL stopped. Subsequent
checks use an isolated PostgreSQL cluster under ignored `target/handle-cc-pg` on the external
workspace volume, with `mail_agents` and `mail_agents_test` on port 55439, and temporary files under
`target/handle-cc-tmp`. Both fresh databases accepted all migrations. Product configuration and
resource limits were not changed. The original service on port 5432 recovered, the abandoned
isolated databases from the interrupted run were removed, and the temporary cluster was stopped
after verification.
