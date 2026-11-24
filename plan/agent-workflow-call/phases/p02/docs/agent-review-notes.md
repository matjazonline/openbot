# AWC-P02 reviewer notes

## AWC-P02-D1-R1-N1

**Identity.** 2026-11-22; workflow `implement-opus-reviewed-2_4`; task [TASK.md](TASK.md),
review [REVIEW.md](REVIEW.md); batch AWC-P02-B1 (02 Batches A and B), stage architecture gate;
reviewer B, Fable 5.2 (effort as assigned: medium); phase AWC-P02; worktree
`/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`, HEAD
`fe7791d3405f00335cbc05dd5029730cc9273416` with p01's accepted working tree. Decision
AWC-P02-D1 r1, [logs/AWC-P02-D1-r1-decision.md](logs/AWC-P02-D1-r1-decision.md), sha256
`09f3101e1d851569a10a86dba4e77b49c069d1b4ad725619b1bb806976c1fb46`. No digest was supplied
for this gate; B computed `29e2c86094099e616898e6d27f7f13f5463a11ed8bd8fd7b352265be1aae669c`
(exclusions: both phases' `docs` and `.trail/learnings`) at the end of the review. Governing
gate AWC-P01-D1 r3 with addendum A1. Coordination: agent-workflow-call r7, AWC-C001 r4
(applies), AWC-C002 r1 (targets p03–p05). TASK still cites r6.

**Coverage and evidence.** Inspection only; nothing executed. Requirements: 02 Batches A and
B and its acceptance, README, 03 for boundaries, root and `src` guides.

- Readers of `run_dependency_manifests.root_run_id`: every SQL use in the init schema (12841,
  12880, 14817, 16617, 17151, 17259, 17942, 20908) and every Rust use outside tests
  (`escalations.rs:124`, `reviewer_resolution.rs:42`, `reads.rs:244`, `role_views.rs:53,83`,
  `run_provenance/mod.rs:32,114,130`, `agent_manifest/storage.rs:26,50,56,88,118,249`,
  `follow_reviewers/mod.rs:79`). None reads it as the budget root. The decision's claim holds.
- Scope cases against writers: the only two inserts are `persist_on` (root, from admission)
  and `attach_child_on` (copy of the parent's row, from `children::insert_child_on`). Both
  satisfy (a) and (c). Budget rows come from `insert_workflow_budget_link` (init 765-779), so
  the parent link the trigger reads is database-written. A forged fresh scope needs an
  invocation, which the D1 guard ties to the child's actor, binding, desk and parent.
- Insert guard: every item reads rows present at commit; the settings comparison is sound
  while `channels FOR SHARE` is held. The call id is not proven against a saved call until
  p03's launch guard, as the decision says.
- Lock order: read `prelock_with_native`, `authorize_run_with_locks`,
  `authorize_person_with_locks`, `target_on`, `configure_reply_on`, `lock_settings_accounts`,
  `lock_account_on`. Channel locks: a call holds share, a configurer upgrades share to update
  and waits; no cycle with a call. Accounts: see F1.
- Teardown: cascade from the caller's execution, with the delete guard in the
  `guard_agent_manifest` pattern, works when the run row is already gone.

Not verified: bodies of every `workflow_runs` trigger (F5); retention's delete order with the
new non-cascading keys to bindings and the child run; `agent_origin::saved_call_on`; the
manifest walk itself.

**Outcome.** Changes required; no pass for AWC-P02-D1 r1. Actionable: F1, F2 (medium), F3,
F4 (low). Optional: F5. D1's table and D2's three cases are sound as designed. See
[agent-review-findings.md](agent-review-findings.md).

**Process and instruction feedback.** One gate for both batches was right; they cannot be
judged apart. The "Facts" section with anchors made checking fast and was accurate except for
the settings writers. Candidates are in F1 and F2.

**Follow-up to AWC-P02-D1-R1-N1 (2026-11-22).** The R1 review missed that r1 placed none of
AWC-P01-D1's steps 6–8 (intake, levels, spend cap). Reported against r2 as AWC-P02-D1-F6. The
R1 outcome is otherwise unchanged.

## AWC-P02-D1-R2-N1

**Identity.** 2026-11-22; workflow `implement-opus-reviewed-2_4`; task [TASK.md](TASK.md),
review [REVIEW.md](REVIEW.md); batch AWC-P02-B1, stage architecture gate recheck; reviewer B,
Fable 5.2 (effort as assigned: medium); phase AWC-P02; worktree
`/Users/mac02/orca/workspaces/openbot/plan08_ph1`, HEAD
`fe7791d3405f00335cbc05dd5029730cc9273416` with p01's accepted tree. Decision AWC-P02-D1 r2,
[logs/AWC-P02-D1-r2-decision.md](logs/AWC-P02-D1-r2-decision.md), sha256
`62e44a2554254d7f2c0240763971b4436811377391120d5d913798bd43ee87d3`, matching the hand-off and
unchanged at return; r1 unchanged (`09f3101e…fb46`). Coordination: agent-workflow-call r8,
AWC-C001 r4, AWC-C002 r1, AWC-C003 r1 (proposed).

**Coverage and evidence.** Inspection only. Read r2 as its full delta against r1 and
AWC-C003. Rechecked F1–F5 by ID. New material examined:

- Step 1 and step 5: the account union and `Conflict` close the `run_as` race, because a
  pinned revision's `run_as` is immutable and any settings change after the unlocked read is
  seen under the share lock. Checked against `prelock_with_native`
  (`action_account_hints.rs:361-392`).
- Step 4 locks: compared with `agent_preparation.rs:67-118` and its callers
  (`checkpoints.rs:379`, `agent_history/preparation.rs:212`). Same rows, same modes, same
  position relative to the company lock and the fence.
- D4: coherent. It keeps one child writer, satisfies `workflow_child_parent_wait`, and inherits
  delivery policy, correlation and budget lineage from the existing read of the parent. The
  batch is larger (schema, two capture refactors, target module, hint purpose, child identity,
  twelve test groups) but still one cohesive unit; splitting it would leave p02 unprovable.
- Verification plan: V0 and named-constraint negatives answer F2; V4 and V5 are competing
  tests as the root guide requires.

Not verified: writers of `channel_principal_grants` and agent principals against the step-4
locks beyond the existing precedent; retention order with the new keys; the manifest walk.

**Outcome.** Changes required; no pass for AWC-P02-D1 r2. F1–F4 fixed, F5 closed. New: F6
(medium, actionable), F7 (low, actionable). See
[agent-review-findings.md](agent-review-findings.md).

**Process and instruction feedback.** The disposition table again made the recheck quick. F6
is a gap B should have raised in R1; candidate instruction is in the finding.

## AWC-P02-D1-R3-N1

**Identity.** 2026-11-22; workflow `implement-opus-reviewed-2_4`; task [TASK.md](TASK.md),
review [REVIEW.md](REVIEW.md); batch AWC-P02-B1, stage architecture gate recheck; reviewer B,
Fable 5.2 (effort as assigned: medium); phase AWC-P02; worktree
`/Users/mac02/orca/workspaces/openbot/plan08_ph1`, HEAD
`fe7791d3405f00335cbc05dd5029730cc9273416` with p01's accepted tree. Decision AWC-P02-D1 r3,
[logs/AWC-P02-D1-r3-decision.md](logs/AWC-P02-D1-r3-decision.md), sha256
`7e1890315ee48b02fa62362fd4738bd1fd01fce75928f3f803e793d6db472125`, a delta on r2
(`62e44a25…87d3`, unchanged); both matched the hand-off and were unchanged at return.
Coordination: agent-workflow-call r9, AWC-C001 r4, AWC-C002 r1, AWC-C003 r1 (proposed; its r2
text is in the decision).

**Coverage and evidence.** Inspection only. Read r3 in full and rechecked F6 and F7.

- Savepoint: `ROLLBACK TO SAVEPOINT` reverts `set_config(…, true)` made inside it, discards
  the queued deferred-constraint events and notifications of the rolled-back rows, and
  releases row locks taken inside it. This rests on PostgreSQL's subtransaction semantics, not
  on a run here; V14 and V15 assert zero rows and a held fence, which is the observable part.
- Levels: the child's content level is the request's, which r3 defines as the calling run's
  current content level and exactness. That is p01 D1.4's stamp, it satisfies
  `content ≤ ceiling` because `release_request` ran first, and the return path
  (`absorb_content_on`, `GREATEST`) stays within the caller's ceiling.
- Optional `run_as`: compared with `prelock_with_native`'s mismatch branch
  (`action_account_hints.rs:372-392`). The base accounts must still all be locked; only the
  destination's account may be absent, and no account is locked after the company lock.
- Owner table against AWC-P01-D1 r3 D1.1 steps 3–8: every step now has a place and an owner.

Not verified: `unit_budgets::check_admission_on`'s body and preconditions; that no trigger
fired by the child rows has a non-transactional side effect beyond notifications.

**Outcome.** PASS for decision revision AWC-P02-D1 r3 (sha256
`7e1890315ee48b02fa62362fd4738bd1fd01fce75928f3f803e793d6db472125`, with r2's unreplaced
text). F6 and F7 fixed. New F8 (low, actionable, deferred to the batch, not blocking). See
[agent-review-findings.md](agent-review-findings.md).

**Process and instruction feedback.** Three rounds for this gate; the second and third came
from the governing gate's ordered checks not being walked step by step in r1, by the author
or by B. The candidate in F6 covers it. A decision published as a delta on its predecessor is
harder to read whole; a consolidated text would help the batch review.


## AWC-P02-B1-R1-N1

**Round.** 2026-11-22; batch review AWC-P02-B1-R1; reviewer B, Fable 5.2 (effort as assigned:
medium); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`, HEAD
`fe7791d3` (work uncommitted); artifact digest
`543ef9e9e262f537971b9bc5fd6abcb6038825c712893c811a7036ae97181777` (exclusions
`plan/agent-workflow-call/phases/p02/docs`, `.trail/learnings`), confirmed before and after.
Governing decision AWC-P02-D1 r3 on r2; coordination agent-workflow-call r10.

**Coverage and evidence.** Read the whole p02 delta: the migration diff
(`workflow_channel_calls`, its immutability and snapshot triggers, `run_dependency_scope`, the
replaced FK, the `guard_workflow_scope_seal` exemption and the whole seal function),
`channel_call_target.rs`, the `children.rs`, `action_account_hints.rs`, `agent_manifest/mod.rs`,
`run_scopes.rs`, `admission_write.rs`, `authority.rs`, `resources.rs`, `workflow_tool_calls.rs`
and `channel.rs` diffs, and all of `channel_call_snapshot_tests.rs`. Checked against gate r3
D3 steps 1–9 and D4, r2 D1/D2 and the verification table.

- Lock order matches D3: accounts then company (`prelock_channel_call_on`), fence, replay by
  key, desk facts under `channels FOR SHARE`, hint check, intake, levels, destination
  admission, child under a savepoint. `authorize_association_on` takes `channels FOR SHARE`
  (`management.rs:78`), so there is no share-to-update upgrade between two callers of one desk.
  Unit archive runs under the company lock (`department.rs:656`, `begin_department_write`), so
  the `FOR KEY SHARE` on the unit is enough. V5 is a real competing claimant for the account
  order; V4 for reconfiguration.
- Schema matches D1/D2: the three scope cases, deferred FKs, the commit-time snapshot check.
- Validated the evidence manifest (5 PASS, 4 FAIL, 5 NOT EXERCISED). The clippy failures are
  nine `redundant_field_names` errors in files this batch does not touch.
- Ran: `cargo test --locked --offline --lib channel_call_` → 36 passed; and
  `agent_history_recovery_parent_cancel_private_child…` alone → passed
  ([log](logs/AWC-P02-B1-R1-reviewer-focused.txt)). The whole suite was not run by B.

**Author-declared contract changes outside gate r3.** Neither reopens the gate.
1. The seal exemption: without it the seal's inherited branch refuses a call child, because
   it requires the parent's actor and an exact copy of the parent's scopes, which D4 forbids
   for a call child. The exemption applies only to a run recorded as that parent's call child;
   the record is immutable and checked at commit. It is the schema consequence of D4 item 3.
2. Record before `run_scopes::capture_on`: forced by 1 (the seal reads the record), inside
   the savepoint, with no observable difference.

**Not verified.** The failing tests the author reports as unrelated
(`cloudroom_e2e_cut_points…`, `doop_agent_current_preparation…`) were not rerun; the
statement-timeout failure under load of the history-cascade test was not reproduced or
explained, only its pass alone. `unit_budgets::check_admission_on`'s body. Whether unit caps
roll up to ancestor units (matters for how F1's fix is written).

**Outcome.** Changes required. Actionable: AWC-P02-B1-F1, F2 (medium), F3, F4 (low). Optional:
F5. AWC-P02-D1-F8 closed. No production-code defect found in the admission path; the medium
findings are missing proof for D1.2 and for the U2 allow case. See
[agent-review-findings.md](agent-review-findings.md).

**Process and instruction feedback.** The author declared most gaps in TASK, which made them
quick to judge. Two of them (F1's V0 half, F2) were gate-listed cases whose omission leaves a
user decision or a D-item without an allow/deny pair; declaring a gap does not make a
gate-listed case optional.


## AWC-P02-B1-R2-N1

**Round.** 2026-11-22; correction recheck AWC-P02-B1-R2; reviewer B, Fable 5.2 (effort as
assigned: medium); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch
`plan08_ph1`, HEAD `fe7791d3` (work uncommitted); artifact digest `0639c0f03d391ed087ddf434513e123753578451483602bd069fd989b3122112` (exclusions
`plan/agent-workflow-call/phases/p02/docs`, `.trail/learnings`), confirmed before and after.
The run was interrupted once by a usage limit; no partial record existed, and the digest was
recomputed on resume. Coordination agent-workflow-call r10.

**Coverage and evidence.** The stated delta only: the corrected parts of
`channel_call_snapshot_tests.rs` (fixture unit, `claim_from`, `write_unit`, `exhaust`,
`participate`, `forged`, `foreign_channel`, V0, V2, V3, V15 and the two new target-facts
tests) and the `Prelocked` change in `action_account_hints.rs`. Read TASK's R2 section and
the corrected mapping rows. Validated `logs/AWC-P02-B1-R2.evidence.json` (5 PASS, 2 FAIL,
5 NOT EXERCISED). Ran `cargo test --locked --offline --lib channel_call_` → 38 passed
([log](logs/AWC-P02-B1-R2-reviewer-focused.txt)).

**Not verified.** That nothing outside the two named files changed, beyond the digest and the
author's statement. The new load-sensitive failure
`workflow_action_reconciliation_bounds_128_prior_consuming_129_total_then_lost_refuses` was
not rerun by B; the `Prelocked` change it passes through is a pure rename of a tuple by
inspection, and the author reports 3/3 alone. The archive-race answer
(`company_level_authority.rs:114`) was taken from TASK's citation, not reopened. The whole
suite was not run by B; the final full-suite gate remains the author's.

**Outcome.** PASS for digest `0639c0f03d391ed087ddf434513e123753578451483602bd069fd989b3122112`. AWC-P02-B1-F1–F4 fixed and closed; F5 optional, not
taken; AWC-P02-D1-F8 closed in R1. No new findings.

**Process and instruction feedback.** Two rounds. Both medium findings were fixture-level
(equal values on both sides of a "not the caller's" claim; a deny-only authorization test);
a mapping-row check of "can this assertion fail under the opposite behaviour" by the author
before freezing would have saved the round.
