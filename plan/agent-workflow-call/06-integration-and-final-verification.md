# 06 — Integration and final verification

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Depends on: 01–05. Outcome: demonstrated end-to-end behavior and a recorded complete verification
gate, without concealing missing database, browser, or runtime checks.

## Batch A — Behavioral and concurrency proof

Use the real persistence/runtime with scripted model/provider adapters. No live paid model is
needed. Required scenarios:

1. A parent `agent.run` calls native channel B, waits, consumes B's response, calls channel C with
   different content/configuration, and produces its final answer in the same parent execution.
   Neither B nor C has a workflow-tool registration. Verify the model sees the actual approved
   response bodies, not just a canned final answer or a saved-result row.
2. A target workflow invokes its own agent and a static/registered workflow child. Capture agents,
   skills, resources, and versions correctly across dependency scopes. Republish/repin while
   waiting; the existing call stays pinned and a later distinct call selects current configuration.
3. Reviewed response, edited approval, rejection, review expiry, no-reply, workflow failure, human
   decision, source reply, and bounded timeout all settle the caller correctly. Parent response
   suppression remains independent. No ordinary ingress fan-out or automatic external final reply.
4. Two competing launchers create one invocation/child/source; two competing resumers consume one
   logical result. Race launch against cancellation, target replacement, revocation, and lease loss;
   race approval against cancellation and response consumption against caller-access loss.
5. Restart after snapshot/launch commit, child completion, review settlement, result persistence,
   and parent reopen. Preserve exact call identity, contents, child count, and retry accounting.
6. Direct self-call and A → B → A reject without unbounded work; depth/count/deadline and response
   size limits fail early. A nested scope cannot reset a tree budget. Separate sequential calls
   remain legal. A poisoned response/wakeup cannot monopolize a poll batch.
7. Foreign/private channels, non-operations caller, forged source/agent/call IDs, broader target
   `run_as`, resource revocation, restrictive knowledge audience, and hidden history all fail
   without leaking content or leaving orphan work. Include fully authorized positive controls.
8. Cancellation, retention, and company teardown remove or retain the appropriate invocation,
   source, manifest, response, review, and wait records without dangling ownership.
9. Static `workflow.call`, registered workflow tools, normal channel ingress/review/delivery,
   message tools, simulations, and agent-history continuation preserve their existing contracts.

Focused commands: list then execute the new `channel_call_` tests with an isolated database; run
affected `workflow_tool_`, `autonomous_agent_children_`, `autonomous_agent_storage_manifest_`,
`channel_reply_`, and history/privacy filters only where changed owners warrant them. Do not use
sequential mocks as the sole proof of any concurrent state transition.

## Batch B — Final gate, once after integration

1. Finish schema edits in the init migration. Use fresh disposable databases and regenerate `.sqlx`
   metadata with `SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets`. Do not reset a shared
   developer database or hide checksum errors with compatibility migrations.
2. Run the repository's local full gate with `DATABASE_URL` explicitly set to the disposable
   schema database: `scripts/check.sh`. It creates a separate test database. Record each gate's
   result; do not stop reporting after the first failure.
3. Compare that script with the current `.github/workflows/ci.yml` and applicable agent-history
   workflow. At planning time the script omits some CI gates, notably lease-budget and browser
   fixture protocols. Complete the omitted required gates explicitly with an isolated test URL:
   - `scripts/test-network-isolation.sh scripts/lease-budget.sh --offline`;
   - `python3 scripts/tests/workflow-sample-boundary.py` and
     `python3 scripts/workflow-sample-boundary.py`;
   - CI's generated-fixture browser checks for agent authoring, workflow agent selection, channel
     responses, decisions, run inspection, and affected history views; follow the actual workflow
     environment variables/commands instead of running browsers against missing fixtures;
   - remaining current CI gates, including image/transport/knowledge checks, where required by
     the checkout's CI definition. Record unavailable Docker, browser, model-artifact, or service
     prerequisites instead of marking their tests passed.
4. The final evidence must explicitly include all of these, whether supplied by the script or
   separately: formatting, patch whitespace, locked offline all-target compilation, clippy,
   fresh migrations, SQLx metadata check, source-module reachability, database-backed all-target
   suite, network isolation, stack budget, lease budget, and applicable browser/sample checks.
5. Keep existing stack and lease thresholds. If a changed bound is unavoidable, document the
   reason and add/retain an early CI failure signal for approaching it. Never raise a limit just
   to turn this feature's failing regression green.
6. After verification fixes, rerun affected checks; repeat the full gate only when impact warrants
   it. Refresh the context graph with `graft build` after the substantive implementation and
   ensure generated evidence/source anchors match the final code.

## Completion record

Replace the pending entries during implementation. Include exact commands, nonzero test counts,
exit status, relevant environment/prerequisites without credentials, and evidence paths.

| Evidence | Status |
| --- | --- |
| Focused phase checks | Done. Each phase's focused checks are in its TASK.md. p06 Batch A: `cargo test --offline --locked --lib -- channel_call_` 120 passed (`phases/p06/docs/logs/a2-channel-call-tests.txt`); accepted at `c7fba340…7431` (Fable PASS AWC-P06-A-R2-N1). |
| Native and custom channel round trips | Done, p06 G1–G2: `channel_call_integration_*`, including two desks in one execution and nested no-reply keeping the parent response (`phases/p06/docs/TASK.md`, Batch A). |
| Competing dispatch/resume/review and crash recovery | Done in p03–p04 plus p06 G3–G6 (competing claimants, pinned while waiting, retention). In the full suite, `agent_history_e2e_process_crash_before_successor…` failed once under the 2 MiB load run and passed in every other run (main run, alone, and 7 runs at 2 MiB). It is kept as an **unresolved intermittent gap** (TASK.md B2.4). |
| Authority, history, knowledge, and response isolation | Done: p06 G7–G8 (member caller, unavailable desks, demoted admin refused before model I/O, teardown). AWC-C005: in the full run, `memory_scope` 80/80, `run_scope` 4/4, `agent_history` 158/159. The one failure is the pre-existing version-7 test. |
| Formatting, offline compilation, clippy, and reachability | Formatting, whitespace and offline compilation pass. Clippy with the scoped flags passes on 1.99.1 and 1.95.0; the CI command on stable **fails** on 9 pre-existing `redundant_field_names` errors in files this plan never changed (`phases/p06/docs/logs/b2-clippy.txt`). Reachability: `source_modules` 8 passed, `--bins` 6 passed (`phases/p06/docs/logs/b3-source-modules-bins.txt`). |
| Fresh migrations and SQLx metadata | PASS on `check.sh`'s own fresh database: migrations, plus "SQLx metadata matches migrated schema" (`phases/p06/docs/logs/b-check-sh.txt`). |
| Database-backed full suite and network isolation | Run under network isolation: 5146 passed, **4 failed**, 30 ignored (`phases/p06/docs/logs/b-check-sh.txt`). The four failures: `desk_wording` was a regression from this plan; it is fixed and now passes; The version-7 test is pre-existing (the base was already at 8); Two Doop tests timed out under load and pass when run alone. The Linux runner-control-plane isolation self-test is **unavailable** on macOS. |
| Stack and lease regression budgets | Lease budget PASS (139). Stack budget run: 5147 passed, 3 failed, with no overflow. The failures are `desk_wording` (since fixed), the version-7 test and the intermittent crash test. Thresholds unchanged. |
| Browser, samples, and other applicable CI gates | PASS: browser protocols (19), frontend protocols, sample boundary, transport boundary (rerun with `rg`), memory extracted service and journey (rerun on Python 3.13), memory integration. The calls and response browser checks pass after the rename. **Unavailable:** Docker images and managed Doop (no Docker); plugin package (needs Node v24.21.0 exactly); query statistics (the server does not preload `pg_stat_statements`); attribution extracted service (no `jsonschema` module). **Failing, unrelated by code path:** attribution SSO browser cookies. |
| Operations docs, startup plan paths, and graph refresh | Done. `docs/workflow-operations.md` "Channel calls" section (p05; heading name corrected in p06). `trail build` exit 0 (`phases/p06/docs/logs/b2-trail-build.txt`). |

Do not declare implementation complete while a required check is unavailable or an acceptance
scenario remains unimplemented. State the precise blocker and retained evidence instead.

**Status (2026-11-24):** implemented, reviewed and accepted in phase. Batch B review: Fable PASS AWC-P06-B-R2-N1 at digest
`bc8b9841…764f`, manifest `phases/p06/docs/logs/AWC-P06-B-R2.evidence.json`. **Not declared complete** under the
rule above. These checks remain:
- **Unavailable on this machine:** Docker images, managed Doop, the plugin package, query statistics, the
  attribution extracted service, and the Linux isolation self-test.
- **Unresolved intermittent gap:** the agent-history crash test under load.

Pre-existing failures are recorded separately:
- the version-7 test;
- clippy on stable;
- attribution SSO.

Running the unavailable gates in CI or on a machine with Docker closes the first group.
