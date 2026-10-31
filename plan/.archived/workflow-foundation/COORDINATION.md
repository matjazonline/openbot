# Plan coordination: workflow-foundation

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

- Authority: /Users/mac02/dev/openbot/plan/workflow-foundation/COORDINATION.md
- Writer: phase p10 root session of 2026-10-25 (Codex; user confirms Astra/high),
  worktree /Users/mac02/dev/openbot. Same root transfers ownership from completed p09 to p10;
  earlier phase records/resources are preserved.
- Revision: 16 (r16: 2026-10-25, p10 accepted locally after final R005 and full gates; status closure only, notice requirements unchanged; r15: 2026-10-25, p10 registered and ownership transferred; p08/p09 committed in 5d4aab1; existing requirements/notices unchanged; r14: 2026-10-25, p09 final acceptance/status closure only; requirements and notice revisions unchanged; r13: 2026-10-25, stopped-session ownership transferred to resumed p09 root;
  requirements and notices unchanged; r12: 2026-10-25, p09 response to WF-C003 r1: admin UI accepted in p09 B1b, and
  the reuse wording is read against P07-D2 rev 4 §D2.2; r11: 2026-10-24, writer handoff to p09;
  p09 registered; p09 acknowledgements of WF-C002 r1, WF-C003 r1, WF-C004 r2 and WF-C005 r1)

Ownership handoff (r2): the p07 work moved from `/Users/mac03/dev/openbot` to
`/Users/mac02/dev/openbot` (user commit `2d608f2` carries r1 and the p07 records). The p07 root
session of 2026-10-22 resumed p07 and took over as writer; the earlier session is stopped.

## Plan-wide instructions

None beyond the repository `AGENTS.md` files and the plan README. Phases run sequentially in the
primary worktree; earlier phases (01–06) were executed under the 2.1 workflows with records in
`plan/workflow-foundation/docs/` (removed by the user's retrospective cleanup; retrievable from git
history at `921079d`).

## Phase directory
| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |
| p07 | root session 2026-10-22 (implement-opus-reviewed-2_2) — complete, committed `bd2ef36` | /Users/mac02/dev/openbot (branch `workflows`) | plan/workflow-foundation/phases/p07/docs/TASK.md, HANDOFF.md |
| p08 | root session 2026-10-22 (implement-opus-reviewed-2_2), started at `da86871`; every batch accepted in phase, phase gate checks owed (user deferred), not committed | /Users/mac02/dev/openbot (branch `workflows`) | plan/workflow-foundation/phases/p08/docs/TASK.md, HANDOFF.md |
| p09 | resumed root session 2026-10-25 (implement-astra-reviewed-2.2), started at `da86871` + p08 artifact `00ec454f…6829`; complete and accepted in phase, final R020 PASS, not committed | /Users/mac02/dev/openbot (branch `workflows`) | plan/workflow-foundation/phases/p09/docs/TASK.md, HANDOFF.md |
| p10 | root session 2026-10-25 (implement-astra-reviewed-2.2), starting `5d4aab1`; complete and accepted locally, final R005 PASS, uncommitted | /Users/mac02/dev/openbot (branch `workflows`) | plan/workflow-foundation/phases/p10/docs/TASK.md, HANDOFF.md |

## Cross-phase notices

### WF-C001 / r2
- State: active (r3: activated 2026-10-22 after B PASS WF-P07-R004; r4: revised to r2 after B
  PASS WF-P07-R010 for P07-D2 rev 4)
- Origin: p07, decision P07-D1 rev 2 §D1.2a, §D1.7, §D1.1 (`phases/p07/docs/logs/decision-D1-r2.md`);
  findings WF-P07-F003, WF-P07-F004.
- Targets: p08
- Instruction/change: `TriggerSource::Child` is not admissible through `WorkflowService::admit`.
  Delegation or other runtime-created children use the runtime child writer
  (`children::create_on`) semantics or a separately reviewed path that satisfies D1.2a (frozen
  pinned graph, inherited deadline, parent wait consuming the wakeup). Manual start/rerun entry
  points expose `AdmitWorkflowRequest.rerun_of`. Every production `DefinitionService`
  composition includes `ChildPublicationDirectory`, or parents with child steps cannot publish.
  r2 (P07-D2 rev 4 §D2.8): `ChildPublicationDirectory` is nested **outside**
  `AgentPublicationDirectory` (it collects pins from captured workflow-tool snapshots too), and
  every production `AgentActionTools` composition includes the workflow tool runtime.
- Dependencies/order: effective when p07 B1 is accepted; blocks p08 entry-point and delegation
  work that admits runs or composes publication.
- Evidence/review: P07-D1 rev 4 (`phases/p07/docs/logs/decision-D1-r4.md`, sha256 5c076606…adc4), B PASS WF-P07-R004.
- Verification: p08 tests show refusal of child admission, rerun link exposure, and publication
  of a parent with a `workflow.call` step through the production composition.
- Responses: p08 → acknowledged r2 (2026-10-22, p08 TASK "Coordination"): production composition in p08 B1; manual start/rerun (`rerun_of`) in p08 B2; routes in p08 B3 (r6 corrects the batch placement); verification pending.

### WF-C002 / r1
- State: active (r3: activated 2026-10-22 after B PASS WF-P07-R004)
- Origin: p07, decision P07-D1 rev 2 §D1.11; finding WF-P07-F007 item 4.
- Targets: p09
- Instruction/change: runtime child runs carry the parent's `binding_id` for provenance only.
  Binding run lists filter `parent_run_id IS NULL` or label children. Run inspection shows
  lineage (parent run/execution, round ordinal) and `rerun_of`.
- Dependencies/order: effective when p07 B1 is accepted; applies to p09 operations UI.
- Evidence/review: P07-D1 rev 4 (`phases/p07/docs/logs/decision-D1-r4.md`, sha256 5c076606…adc4), B PASS WF-P07-R004.
- Verification: p09 run lists and inspection tests with a runtime child and a rerun.
- Responses: p09 → acknowledged r1 (2026-10-24, p09 TASK "Coordination"): run lists and inspection in p09 B2; verification pending.

### WF-C003 / r1
- State: active (r4: activated 2026-10-22 after B PASS WF-P07-R010)
- Origin: p07, decision P07-D2 rev 4 §D2.0, §D2.8 (`phases/p07/docs/logs/decision-D2-r4.md`,
  sha256 ebf212db…dfab7); user decisions P07-Q1 (named workflow tool registrations) and P07-Q2
  (revoke stops, re-pin/archive don't); findings WF-P07-F023 item 4.
- Targets: p08, p09
- Instruction/change: p08 exposes company-scoped HTTP routes over the `WorkflowTools` commands
  (register with expected revision, revoke, list) and over agent tool grants
  (`agents.workflow_tools`, which no route sets today), with the semantics of P07-Q2. p09 builds
  the admin UI for both. It warns when a revoked or re-pointed tool name is reused, because the
  old pins stay refused. It shows tool-call children in run inspection (the parent's saved call
  id) as it does step children (WF-C002), and covers workflow-tool children in sample execution
  ("zero production effects, including through direct agent tools and children").
- Dependencies/order: effective when p07 B2 is accepted; p08 routes before p09 UI.
- Evidence/review: P07-D2 rev 4, B PASS WF-P07-R010.
- Verification: p08 route tests (authority, revision conflict, revoke) and p09 UI/inspection and
  sample-execution tests with a workflow-tool child.
- Responses: p08 → acknowledged r1 (2026-10-22; routes in p08 B3 — r6 corrects the batch placement); verification done r9 (2026-10-23): p08 B3b-1 accepted in phase after B PASS WF-P08-R015 (digest `57566f92…dd33`). The routes are `GET /api/companies/{c}/workflow-tools`, `PUT|DELETE …/workflow-tools/{name}` (expected revision; revoke 204) and `PUT /api/companies/{c}/agents/{a}/workflow-tools` (`{expected, tools}`; a grant may not add provisioning names); run listings label children with `parent_run_id`. p09 → acknowledged r1 (2026-10-24, p09 TASK "Coordination"): admin UI in p09 B1b, inspection in B2, sample execution in B4; verification pending.
  p09 → r12 (2026-10-25): the admin UI is **accepted in p09** (B1b, B PASS WF-P09-R007, digest `89d23e56…0eb9`). It covers the workflow tools pane with revoke, "Use as a workflow tool" with a two-step replace of a taken name, and the agent pane's grants (expected list; provisioning names refused). The warnings state P07-D2 rev 4 §D2.2 and §D2.7 as the code enforces them:
    - revoking, or pointing a name at another workflow, stops every published version that froze it: no new runs, no configuring or activating its bindings, and its agents' next calls are refused;
    - a call already in progress still returns its result;
    - re-registering the name for the **same** workflow restores those versions.
  So this notice's "the old pins stay refused" holds only when the name is reused for a different workflow (p09 DV-P09-7, confirmed by B). No requirement changes. Inspection (B2) and samples (B4) are still pending.

### WF-C004 / r1
- State: active (r7: published and activated 2026-10-23 by p08 after B PASS WF-P08-R006)
- Origin: p08 B1, finding WF-P08-F029 (root cause) and F030 (`phases/p08/docs/TASK.md` "B1
  corrections for WF-P08-R005"; `agent-review-findings.md`).
- Targets: all current and future phases (p08, p09, later).
- Instruction/change: sqlx-postgres 0.8.6 `begin()` cancelled after `BEGIN` reached the server
  returns its connection to the pool **inside an open transaction** (the depth counter is
  incremented only after the reply, and the drop guard does nothing at depth 0); later
  "autocommit" statements on it stay uncommitted. Every Postgres pool built in production code or
  tests must be built from `crate::infra::db::discard_leaked_transactions(PgPoolOptions::new())`,
  whose `after_release` hook closes a connection released inside a transaction. Do not hold a
  `PoolConnection` across a cancelled `begin` and keep using it (the hook only sees releases).
  Lazy pools that never connect are exempt. The regression test
  `infra::db::tests::a_cancelled_begin_never_leaks_its_transaction` fails, saying so, once sqlx
  fixes the bug; then the hook may be removed together with that test.
- Dependencies/order: effective now (p08 B1 accepted at digest `b90a7f9c…a468`, uncommitted
  working tree on branch `workflows`).
- Evidence/review: B PASS WF-P08-R006 (`phases/p08/docs/agent-review-notes.md`); sqlx-core 0.8.6
  release-path trace in that note.
- Verification: `grep -rn "PgPoolOptions::new()" src` shows only guarded constructions and the
  regression test's deliberate bare pool; p08 B2 routes the remaining test-only pools (F030)
  through the guard.
- Responses: p08 → acknowledged r1 (origin; F030 in p08 B2); p09 → superseded by r2 (see r2).

### WF-C004 / r2
- State: active (r8: 2026-10-23, supersedes r1; same origin and targets)
- Change from r1: one more exemption. A test that proves what a pinned backend did by reading
  its last statement (`pg_stat_activity.query`) may use a bare pool, because the guard runs a
  statement on every release; the construction must say so and the test must not cancel a
  `begin`. Today that is `workflow_lease_resolution_read_only_validate_rolls_back_under_stalled_renewal`
  (`src/adapters/persistence/workflow/lease_resolution_tests.rs`). Everything else in r1 stands.
- Verification: `grep -rn "PgPoolOptions::new()" src` shows guarded constructions, the
  regression test's deliberate bare pool, that documented test pool, and lazy pools.
- Responses: p08 → acknowledged r2 (origin; F030 done in B2 under r2); p09 → acknowledged r2 (2026-10-24, p09 TASK "Coordination"): applies to any pool p09 builds; none planned.

### WF-C005 / r1
- State: active (r10: published 2026-10-23 by p08 after B PASS WF-P08-R018)
- Origin: p08 B3b, gate P08-D3 rev 2 (`phases/p08/docs/logs/decision-D3-r2.md`, sha256
  `0cacf996…558c`, B PASS WF-P08-R017), findings WF-P08-F052, F064, F070 and F071.
- Targets: p09 (the admin UI over the p08 workflow routes).
- Instruction/change: p09's UI follows the contracts of the p08 routes:
  - **Listings** answer `{items, next}`. Page with `?after=<next>&limit=1..=200`. A page may
    hold fewer items than its size (rows the viewer cannot see are omitted) and still have a
    `next`; a stale cursor is 400. Run detail has the newest 200 executions, waits and actions,
    each with a `*_truncated` flag.
  - **Action reconciliation** (`POST /api/companies/{c}/workflow-runs/{r}/reconciliations`) is
    an **annotation**: "record what happened" with an unknown note. It is **not a resume**,
    because production has no evidence verifier. A run waiting on reconciliation recovers by
    cancel and then rerun; label the controls accordingly.
  - **Client rules:**
    - a recorded outcome is 200; `revision_conflict`, `idempotency_conflict`, `blocked` and
      `evidence_conflict` are 409, with the outcome in the body; after a 409, re-read the run
      and use a new key;
    - after a 503 (timeout), retry the same request with the same key: the settlement may
      already have committed;
    - every call, a replay included, needs current access and a ready resource, or it is 404 and
      nothing is recorded;
    - the run detail's `actions[].reconcilable` and `destinations[].reconcilable` are hints, and
      the command decides.
  - **Child runs** are controlled through their parent: cancel and retry on a child answer 409,
    naming the parent (`parent_run_id` labels them in lists).
  - **The decision page** `/ui/workflow-decisions/{id}` is the link target of review notices.
- Dependencies/order: effective now. The p08 routes are accepted in phase (B3b-1 R015, B3b-2
  R018).
- Evidence/review: P08-D3 rev 2 (R017); B3b-1 and B3b-2 batch reviews.
- Verification: p09 UI tests for paging continuation over partly hidden pages, the annotation
  labelling, and the 409/503 client handling.
- Responses: p09 → acknowledged r1 (2026-10-24, p09 TASK "Coordination"): p09 B2 (runs, controls) and B3 (decision page); verification pending.

## p09 final disposition — r14 (2026-10-25)

Administrative closure only; all notice requirements and revisions above remain unchanged.
Historical pending responses above are superseded for p09 by these completed dispositions:

- WF-C002 r1: run-list child labels/filtering, parent/execution/round lineage and rerun links
  verified in B2a R010 and final R020.
- WF-C003 r1: admin registrations/grants accepted B1b R007; tool-child inspection accepted B2a
  R010; frozen workflow-tool child sample execution accepted B4 R019, final R020. The r12
  interpretation of same-workflow name reuse remains unchanged.
- WF-C004 r2: guarded-pool contract retained; no new exemption. Full database suite passed,
  including the leak regression.
- WF-C005 r1: partly hidden pagination, annotation wording, command identity/retry behavior,
  parent-owned child controls and decision pages verified B2/B3 and final R020.

p09 accepted at artifact `76d100c63dd8735808ebd7b0c84ee0cbbb4e7932818e28575dc55f0e3030c3ea`,
HEAD `da868710d3b235d49856e3e481f8f3735f6b3e56`; see p09 REVIEW/HANDOFF and final evidence.
Full required Rust/database/stack/lease gates passed; pre-existing CSS drift DV-P09-8 retained.
This does not change p08's recorded status or declare whole-plan integration complete.

## p10 startup — r15

p08/p09 implementation and records are committed/pushed in `5d4aab1`. This corrects only
commit status in the historical directory rows, not independent phase acceptance status.
p10 acknowledges WF-C004 r2 and preserves WF-C001r2/C002r1/C003r1/C005r1 contracts while
verifying combined integration. No production deployment or database reset target is authorized.

## p10 completion — r16

p10 accepted in the current worktree at artifact
`37915f1301f66f04c5fff3f2a2ea28bc1fb98649eeb7997f39e00b61b4661ecb`, base/HEAD `5d4aab1`.
Independent final review WF-P10-R005 PASS; all F001–F003 closed. Full integrated library gate
passed 2,311 tests at stock 2 MiB (three existing developer/optional DNS ignores); binary5,
lease130+timing, browser17, migrations/SQLx/offline/Clippy/fmt/CSS/security boundaries passed.
This supplies the combined verification of the integrated phases01–10 implementation; prior
phase-specific historical records remain unchanged. p10 is not committed/pushed or deployed.

WF-C004r2 remains enforced with no exemption; guarded-pool regression and DB-backed suite pass.
Inherited WF-C001r2/C002r1/C003r1/C005r1 interfaces remain covered by the full integration suites;
no contracts or notice revisions changed. Metrics add one accounted pool connection, not a new
workflow authority. Exact historical public episode INSERT diagnostic remains unexecuted under
the independently reviewed P10-D1r1 obstruction; C adversarial public-trigger and behavioral
replay/effect integration coverage remain. Graft refresh unavailable in this environment.
Only phase10-owned PG55500 and browser fixtures were stopped/removed after review. See p10
TASK/REVIEW/HANDOFF for exact commands, failures/corrections, resources and limitations.
