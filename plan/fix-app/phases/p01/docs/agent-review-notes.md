# Agent review notes — fix-app p01

## FIXAPP-P01-G1-R1 — architecture gate G1, decisions D1–D6 revision r1

### Identity

- Date: 2026-10-31. Workflow: `implement-opus-reviewed-2_2`. Stage: architecture gate G1 for batch B1
  (whole plan 01, its batches 1–4). Task record [TASK.md](TASK.md); review record [REVIEW.md](REVIEW.md);
  findings [agent-review-findings.md](agent-review-findings.md).
- Reviewer: B, Opus 5.5, effort xhigh (as assigned by root); no sub-agents used.
- Phase `p01`, worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p01`, branch `fix-app/p01`.
- Phase start `e6b0a51`, review base `e6b0a51`, HEAD `e6b0a510ec27a636bf76c9aa9312605ca3e5c8cb` (no code
  changes yet). Artifact digest with exclusion exactly `plan/fix-app/phases/p01/docs`:
  `9d5bde9263086a6007787848a407759f6e8a3ca7b391addfaef755df2fee4c66` — confirmed at start and again before
  returning (see Freshness).
- Gated decisions reviewed: D1–D6, revision **G1 r1**, immutable text `logs/G1-r1-decisions.md`, sha256
  `89fc9a88462bfe3d9b5437604556b177a7682399fdf485e282ddb4e659d7e2a8` (verified; byte-identical to the TASK.md
  "G1" section).
- Coordination: authoritative `/Users/mac03/dev/openbot/plan/fix-app/COORDINATION.md`, revision 1, sha256
  `758532a69a450cac10c533d4906059a9bdeed900ba4a5c5904ed751829811c2a`, identical to the immutable copy
  `logs/COORDINATION-r1.md`. Applicable plan-wide instructions fix-app-I001 r1 (repo/plan rules; in-place
  edits of named init-schema statements; final gate reset-db + check.sh), I002 r1 (isolation/resources),
  I003 r1 (commits). No cross-phase notices target p01. TASK.md acknowledges all three.

### Coverage and evidence

- Requirements derived from the originals: `plan/fix-app/01-identity-and-ownership.md` (Outcome, What
  happens today 1–5, Decisions, Batches 1–4 with their tests), `plan/fix-app/README.md` (concurrency rules,
  known overlaps with 05, verification), root `AGENTS.md`, `src/AGENTS.md` (headings),
  `src/application/AGENTS.md`, `src/adapters/persistence/AGENTS.md`; plan 05 overlap sections (member
  removal, notification recipient FK) checked for conflicts with D2/D3 — none found.
- Code inspected at e6b0a51 (spans re-checked; plan spans had drifted slightly):
  `participant.rs` (`create_person_principal_on` L260-368, `resolve_or_create_external_identity_on`
  L190-252, access contexts L453-538); `user.rs` (`update_profile` L310, `confirm_pending_account_change`
  L668, Email UPDATE L723; only two email writers exist); `use_cases/user.rs` bypass L558-562, L854-905;
  `company.rs` create L107-146 (users `FOR UPDATE`) and `delete_company_with_cleanup`; `company_invite.rs`
  persistence (create/update/accept L228-304, `remove_member` L415-444, `update_member_role` already owner
  guarded) and use case (create/update); `agent.rs` delete L360-378 and `use_cases/agent.rs` delete_agent;
  `channel.rs` allowlist write L218-258, delete L474-479, read-back L92-115, `channel_access` L186-216;
  `domain/entities/channel.rs` participant/viewer access and `preferred_approver`; `thread/message.rs`
  `resolve_author`/`resolve_participants`; `thread/views.rs` author joins and reply contexts;
  `thread/mod.rs` `THREAD_SELECT`; workflow readers of `messages.author_principal_id` (`reply_waits.rs`,
  `provisioning.rs requester_on`, `message_intents.rs`, `thread/inbound.rs` intent insert); every FK
  referencing principals / identities / agents / messages / company_members in the init schema; triggers on
  the affected tables.
- Fixture surface enumerated with `graft grep`: raw company inserts (3), `UPDATE companies SET user_id`
  (7 test sites), owner/role updates on `company_members` (~18 test sites), `DELETE FROM company_members`
  (22) and `DELETE FROM principals` (5) in tests.
- **Tested (scratch DB, not the project tests):** private cluster 127.0.0.1:55401, database
  `p01_review_scratch` created from `migrations/20260817000000_init_schema.sql`@e6b0a51 plus an emulation of
  D2/D3 (`logs/G1-R1-reviewer-apply_g1.sql`), fixture `logs/G1-R1-reviewer-fixture.sql`, scenarios
  `logs/G1-R1-reviewer-scenarios.sql`, output `logs/G1-R1-reviewer-scratch.log`; database dropped afterwards.
  Results:
  - S1/S2 owner demotion or `companies.user_id` change alone → 23514 `companies_owner_member_agreement`;
    S3 non-member owner → 23503 `companies_owner_member_fk`; S5 second owner → 23505
    `company_members_one_owner_idx`; S14 the D2 transfer-helper order commits; production create order
    (company row before member row) commits.
  - S6 D5 upsert `... DO UPDATE ... WHERE company_members.role <> 'owner'` returns no row and leaves `owner`
    (so the result must `LEFT JOIN` membership, as D5 says).
  - S8 identity move and message-author move as separate statements → 23503
    `messages_authored_identity_author_fk`; S9 one data-modifying CTE commits (forward); S9b the reverse
    upsert (`ON CONFLICT ... DO UPDATE SET principal_id`) plus authorship move in one statement commits.
    Independently confirms the author's experiment.
  - S10 plain agent delete with a principal → 23503 `principals_company_agent_fk`; S11 D4 lock/demote/delete
    commits, agent-authored message kept with an `external` author.
  - S12 deleting a principal with authored messages → 23503 `messages_author_principal_fk`; S4 deleting a
    message author's member row fails at statement end on the same FK (D3 × fixtures).
  - S15 company delete with retired agent principal, persons, externals, identities, messages and
    participants removes everything (NO ACTION checks at statement end + circular deferred owner FK).
  - S16 counterfactual: with `participant_identities_principal_fk` also NO ACTION the company delete fails
    (identities have no company FK); restored to CASCADE it succeeds — D3's interpretation is required.
  - S17 a single trigger function using `CASE ... NEW.company_id` fails when fired on `companies`.
  - `model_usage_calls` FK name confirmed as `model_usage_calls_company_id_agent_id_fkey`.
- **By inspection only (not executed):** D1 lock ordering and deadlock behaviour against inbound ingestion
  (ingestion takes several advisory locks in message order — a deadlock with the re-key is possible and is
  resolved by PostgreSQL aborting one transaction; same pre-existing property between two ingestions);
  D1 vs `remove_member` race (outcome equals `remove_member`'s own end state, no trust leak); D2 absence of
  write skew under READ COMMITTED deferred checks; D4 serialization against `create_agent_principal_on`
  (insert branch blocks on the FK's `KEY SHARE`, `DO UPDATE` branch on the principal row lock); SMTP trust
  TOCTOU (`observe_access_context` resolves in its own transaction before the message insert) — one message
  in flight during an email change may be judged with pre-change trust; negligible.
- **Unavailable / not run:** no project tests were run (gate precedes implementation; nothing to test). No
  competing-claimant execution of D1/D4 (requested as F2).
- **Assumptions challenged:**
  - D3 "three cascades" reading → confirmed and strengthened (S16).
  - D1 authorship-follows-address → the only option that keeps `messages_authored_identity_author_fk` and
    human-completion reply routing (`latest_replyable_email_context` inner-joins the authored identity;
    nulling `authored_identity_id` would make those threads unreplyable); does not affect workflow requester
    derivation (application-origin messages have no authored identity). Accepted; consequences to document
    (F4).
  - D1 reverse adoption → defect (F1): the outsider's grants and thread rows are stranded.
  - D1 bypass path → plan-mandated; new adoption capability flagged as a user-level choice (F3).
  - D4 "no database cause for bound to a channel" → confirmed: only `agent_skills` (CASCADE), `principals`,
    `task_harness_runs` and `model_usage_calls` reference `agents`; the old 23514 has no remaining source.
  - D5 owner guards → `update_member_role` and `remove_member` already exclude the owner; with D5 and D2 no
    invite or member write can change the owner row.
  - D6 → correct for production today; latent read-back limitation recorded (F6).
- **Lightly reviewed:** D5/D6 code-level details (error messages, trait mock fan-out of the new
  `is_company_owner_address` port) are left to the batch review; the full list of fixtures the new
  invariants invalidate was enumerated by grep, not by running the suite.

### Outcome

- **Changes required** for decision revision G1 r1.
- Actionable: [FIXAPP-P01-G1-F1](agent-review-findings.md#fixapp-p01-g1-f1--d1-reverse-adoption-strands-the-outsiders-grants-and-thread-rows-on-an-identity-less-principal)
  (medium, D1 reverse adoption strands grants/thread rows; allowlist read-back, access and mode widening),
  [FIXAPP-P01-G1-F2](agent-review-findings.md#fixapp-p01-g1-f2--verification-plan-has-no-competing-claimant-tests-for-the-new-locking-protocols)
  (low, competing-claimant tests for the D1/D4 locking protocols missing from the verification plan).
- Optional: F3 (bypass adoption — user-level choice), F4 (D1 rationale/consequences), F5 (fixture-impact
  list; S16 supports D3), F6 (D6 latent read-back), F7 (D2 trigger branching, D4 constraint names/tests).
- D2, D3, D4, D5, D6 pass as decided (subject to the optional notes). D1 passes except for the reverse
  adoption gap (F1). A recheck needs a G1 revision addressing F1 and F2; S1–S17 evidence can be reused while
  the schema emulation still matches the revised decisions.

### Freshness

- Start: branch `fix-app/p01`, HEAD e6b0a51, digest `9d5bde92…4c66`, COORDINATION r1 sha256 `758532a6…1c2a`.
- Before returning: rechecked (recorded in REVIEW.md "Freshness at return").

### Process and instruction feedback

- Batch size for the gate was right: six decisions, one schema file, one review was thorough. The gate was
  warranted (FK/trigger schema, ownership invariant, identity re-key).
- Useful in the author's record: the explicit constraint names, the lock reasoning and the flagged D3
  interpretation; the author's NO ACTION end-of-statement experiment was reproducible.
- Missing: analysis of every reader that joins principal → identity when an identity changes principal
  (led to F1); the AGENTS.md competing-claimant requirement was not carried into the verification plan
  (F2).
- Candidate instruction (scope: architecture-gate decisions that move rows between owners/principals):
  "For each row that changes owner, list the queries that join through that ownership and state each one's
  result after the move; include a competing-claimant test for every new lock." Evidence: F1, F2. Single
  occurrence; recorded for later analysis only.
- Defects are in the gated decision text (D1, verification plan), not in implementation.

## FIXAPP-P01-G1-R2 — correction recheck of architecture gate G1, decision revision r2

### Identity

- Date: 2026-10-31. Workflow `implement-opus-reviewed-2_2`. Stage: correction recheck of G1 (batch B1).
  Requested by the phase root (A) for findings FIXAPP-P01-G1-F1/F2 (actionable) and F3–F7 (optional).
- Reviewer: B, Opus 5.5, effort xhigh; no sub-agents.
- Phase `p01`, worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p01`, branch `fix-app/p01`; phase start,
  review base and HEAD `e6b0a510ec27a636bf76c9aa9312605ca3e5c8cb`; digest (exclusion exactly
  `plan/fix-app/phases/p01/docs`) `9d5bde9263086a6007787848a407759f6e8a3ca7b391addfaef755df2fee4c66` at
  start of this round. **Before returning the digest had changed** to
  `4fd6c2fca3227aceb81542dc23baba152c5289db3a5d219f89d3db16d7af58fa`: uncommitted edits appeared in
  `migrations/20260817000000_init_schema.sql` and `src/adapters/persistence/channel.rs` (94 insertions, 5 deletions; D2 owner FK
  and constraint triggers, D3 NO ACTION FKs, D6 `DO NOTHING`), i.e. dependent implementation edits begun
  while this recheck was in progress, before B's pass. HEAD unchanged. Those edits were only identified (diff
  glanced for which statements changed), **not reviewed**; they belong to the B1 batch review. The gate
  verdict below is for the decision text G1 r2, whose hash is unchanged. A further check read digest
  `89f97c592b18793dbe8143703bf4b2fbc882d7660838a710b9457fd7c43b7e94` (edits still in progress).
- Decision revision reviewed: **G1 r2**, `logs/G1-r2-decisions.md` sha256
  `b85869d6a676837b84ec5cb34e8f8361e385a642410f9c4e3d3d08e1a5e454ac`; verified equal to TASK.md "G1" +
  "Verification plan" sections; r1 text still preserved (`logs/G1-r1-decisions.md`, sha256 `89fc9a88…e2a8`).
- Coordination: `/Users/mac03/dev/openbot/plan/fix-app/COORDINATION.md` still revision 1, sha256
  `758532a6…1c2a` (= `logs/COORDINATION-r1.md`); I001/I002/I003 r1; no p01 notices.

### Coverage and evidence

- Reviewed the full r1→r2 diff (D1 reply-routing/adopter text, D1 "Adopting an outsider's address", D1
  bypass product note, D6 limitation, "Implementation notes", "Verification plan" with three
  competing-claimant tests). D2–D5 schema text unchanged, so R1 scratch evidence S1–S17
  (`logs/G1-R1-reviewer-scratch.log`) is reused unchanged; no new DB experiment was needed for r2 (the grant
  and thread-row copy is plain `INSERT ... SELECT ... ON CONFLICT DO NOTHING` + `DELETE`).
- Checked against code at e6b0a51 (inspection): PKs that the `ON CONFLICT` relies on
  (`channel_principal_grants_pkey (company_id, channel_id, principal_id, capability)`,
  `thread_principals_pkey (company_id, channel_id, thread_id, principal_id, role)`); identity writers
  (no writer attaches a second identity to an existing external principal, so "X has no identity left" is
  the steady-state case and is stable under the advisory lock); the grant trigger
  `notification_channel_grant_recheck` → `notification_recheck_channel_access` (`channels ... FOR UPDATE`);
  channel save order (`UPDATE channels` → grant delete → advisory locks in `insert_email_allowlist_grants`);
  `Channel::participant_access`/`viewer_access` and `access_context_for_user` grant-authority withholding
  for unverified accounts.
- Challenged: (a) removed-member X (from `remove_member`) adopted by another member who later takes that
  address — r2 moves X's grants/thread rows to the adopter; this is narrower than what the existing
  `create_person_principal_on` promotion already transfers (the whole principal incl. UI-authored messages),
  so it is consistent with the plan's "adopts it, as it adopts any outsider"; accepted. (b) bypass adoption
  of grants — UI view stays withheld for unverified accounts; mail authorization equals what X had;
  accepted. (c) in-flight workflow destinations that name X by principal id become unreachable after the
  move — same class as the accepted in-flight reply-wait consequence; observation only. (d) lock order of
  the new grant writes vs channel save — possible detected deadlock; recorded as optional F8.
- Not run: no project tests (nothing implemented). The competing-claimant tests are plan items; their
  execution and assertions are to be checked in the B1 batch review.

### Outcome

- **PASS for decision revision G1 r2** (D1–D6 with the r2 verification plan; text sha256
  `b85869d6…54ac`), reviewed against code at `e6b0a51` / digest
  `9d5bde9263086a6007787848a407759f6e8a3ca7b391addfaef755df2fee4c66`. The worktree has since moved to digest
  `4fd6c2fc…58fa` through unreviewed implementation edits (see Identity); that does not change the gated
  decision but is a process deviation (dependent edits before the gate pass) to report to root.
- FIXAPP-P01-G1-F1 fixed; FIXAPP-P01-G1-F2 fixed; F4, F5, F7 fixed; F3 and F6 dispositioned as accepted
  (recorded in r2). New optional FIXAPP-P01-G1-F8 (lock-order inversion between adoption's grant writes and a
  channel save; deadlock detected and rolled back; not blocking). Details in
  [agent-review-findings.md](agent-review-findings.md).
- Reused from R1: all scratch evidence and the D2–D5 analysis (unchanged text, unchanged code). Broadened only
  to the new D1 adoption step and the new verification items.

### Process and instruction feedback

- The correction was delivered in one round, each change tagged with the finding it answers, and r1 text was
  preserved — made the recheck cheap and traceable.
- F8 shows corrections can introduce new lock interactions through triggers; candidate guidance (scope:
  gate corrections that add writes): "re-run the lock-order list including trigger-taken locks for any newly
  written table." Single occurrence; recorded only.
- Process deviation observed: implementation edits to the init schema and `channel.rs` started during the G1
  recheck, before B's explicit pass (SKILL.md: "obtain B's explicit pass for the current decision revision
  before dependent edits"). Impact here is low because the edited statements implement D2/D3/D6, whose text
  did not change between r1 and r2, but the reviewed artifact moved under the reviewer.


## FIXAPP-P01-B1-R1 — batch review of B1 (plan 01 batches 1–4) against G1 r2

Status: complete — **changes required** (see Outcome below). Identity:

- Date: 2026-11-01. Workflow `implement-opus-reviewed-2_2`; stage batch review B1; reviewer B, Opus 5.5 /
  xhigh; no sub-agents. Phase `p01`, worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p01`, branch
  `fix-app/p01`; phase start = review base = HEAD `e6b0a51`; all changes uncommitted (incl. 5 new source
  files, 4 new `.sqlx` entries). Digest at start (exclusion `plan/fix-app/phases/p01/docs`):
  `b01b09456e7ea3e98d6af9c47bd04cfb52fe2bc2e02f639b31eb00171247f7de` — matches the request.
- Gated decisions: G1 r2 (`logs/G1-r2-decisions.md` sha256 `b85869d6…54ac`) plus the F8 disposition added to
  TASK.md D6 (accepted retryable deadlock).
- Coordination: `/Users/mac03/dev/openbot/plan/fix-app/COORDINATION.md` revision 2, sha256
  `0e39a556e26a04ca6a59e2c7bf092c881f6954907470b9fc133d0adcff740e93` (= `logs/COORDINATION-r2-latest.md`).
  New applicable instruction fix-app-I004 r1 (targeted tests only; no full suite / check.sh; fmt, compiling
  affected targets and `.sqlx` regeneration allowed; `CARGO_INCREMENTAL=0`). No notices target p01. Applied to
  reviewer checks.

### Working notes B1-R1 (interim, saved against compaction)

- Reviewed diffs: init schema (D2 function/triggers/FK; D3 three FKs), participant.rs (lock_identity_key,
  account_email_identity, upsert_account_identity_on CTE), account_identity.rs (rekey, detach, adopt, adopt
  places), user.rs (locked_account_email; both email writes in a tx + rekey), agent.rs (retire + constraint
  mapping), company_invite persistence + use case (D5), channel.rs (D6), test_support helpers, all workflow
  fixture rewrites.
- Reviewer run 1 (`logs/B1-R1-reviewer-tests-1.log`): frozen test binary
  `target/debug/deps/mail_agents-65122ec83e7c21fc` (built 21:52 after last source edit 21:46), scratch DB
  `p01_review_scratch`, filters account_email_change_ company_owner_ company_invite_ agent_retire_
  channel_allowlist_ post_marker_revoke… → 25 passed, 0 failed.
- CONFIRMED defect candidate: `workflow/action_dispatch_remote_tests.rs:66-94` still injects
  `DELETE FROM company_members WHERE company_id=$1` via `Intercept` with `.execute(pool).await?`; under D2 it
  fails at commit (`companies_owner_member_fk`, server.log captured in `logs/B1-R1-reviewer-remote-revoke.log`)
  and the `?` turns it into the dispatch error the test asserts — revocation case now vacuous.
- Other candidates: `taken_address_error` doc comment now prefixes `locked_account_email` (user.rs ~825);
  rekey parses `old` before checking for persons → an account whose stored address passes
  `is_plausible_address` but not `EmailIdentity::parse` can no longer change its address.
- Remaining unrewritten member/principal/role fixtures inspected: all `.unwrap()` (would panic) and target
  non-owners or principals without history; they ran in t-affected-3 and passed.

### Coverage and evidence (B1-R1, final)

- Requirements traced from the originals (plan 01 Outcome 1–4, Batches 1–4 and their named tests; README
  overlaps/verification; AGENTS.md concurrency-test and verification rules; persistence AGENTS.md row/sqlx/test
  isolation rules) and from G1 r2 D1–D6 plus the F8 disposition.
- Whole diff read (31 tracked files, 5 new source files, 4 new `.sqlx` entries): schema; `participant.rs`;
  `account_identity.rs`; `user.rs`; `agent.rs`; `company_invite.rs` (persistence and use case); `channel.rs`;
  `mod.rs`; `test_support.rs` helpers; every rewritten workflow fixture; the five new test files and the two
  run-history tests; both mocks of `CompanyInvitePersistence`.
- Callers and contracts checked: HTTP invite create/update handlers (`ui_team.rs`, error shown via `Display`),
  every `delete_agent`/`AgentPersistence::delete` caller (incl. library delete and onboarding rollback), every
  production writer of `company_members` (only company creation, acceptance, role update, `remove_member`),
  no SQL fixture files touch the new invariants, `.sqlx`: the 4 new entries are exactly the 4 new macro queries;
  no macro text changed otherwise (no stale entries).
- Invariant-touching fixtures left as raw SQL (`rg` over `src`): all `.unwrap()` their statements/commits and
  target non-owners or principals without history, except `action_dispatch_remote_tests.rs` (F1).
- **Tested by the reviewer** (I004 scope, frozen test binary `target/debug/deps/mail_agents-65122ec83e7c21fc`,
  built after the last source edit; scratch DB `p01_review_scratch` on 127.0.0.1:55401, created and dropped):
  - `logs/B1-R1-reviewer-tests-1.log`: new filters (account_email_change_, company_owner_, company_invite_,
    agent_retire_, channel_allowlist_, the remote revoke test) → 25 passed, 0 failed.
  - `logs/B1-R1-reviewer-remote-revoke.log`: the remote revoke test alone passes while the server log shows the
    injected owner-row delete refused by `companies_owner_member_fk` (F1).
  - `logs/B1-R1-reviewer-tests-2.log`: all 66 filters of `logs/affected-filters.txt` on a fresh scratch DB →
    1094 passed, 0 failed, 1 ignored (pre-existing) — reproduces the author's `t-affected-3.log` count.
  - `logs/B1-R1-reviewer-restrict-probe.log`: frozen schema with the three D3 FKs as RESTRICT, company delete still
    succeeds (F4). Probe DB `p01_restrict_probe` created and dropped.
- Not run (I004): full suite, `scripts/check.sh`, stack budget; fmt/clippy/sqlx-prepare evidence reused from the
  author's logs (`fmt-check.log`, `clippy.log`, `sqlx-prepare.log`) — inspected, not re-run.
- Assumptions challenged: D1 implementation against G1 r2 step by step (lock order, `FOR UPDATE OF principal,
  identity`, one-statement moves, grant/thread copy with original `created_at`, "no identity left" check) —
  conforms; D2 trigger handles UPDATE of `company_id` by checking both companies — conforms; D4 lock and constraint
  mapping (deferred FKs mapped at `COMMIT`) — conforms, both names tested; D5 acceptance `LEFT JOIN` replaced by
  dropping the membership join (data-modifying CTE still executes) — conforms, tested; D6 — conforms, tested.
- Lightly reviewed: the semantics of each rewritten workflow fixture beyond "same assertions, valid state" (the
  replacement owner added by `revoke_membership` was judged harmless because the old "owner without member row"
  state is no longer representable); HTTP rendering of the new `BadRequest` messages relied on the passing
  `ui_team`/`pages` tests.

### Outcome (B1-R1)

- **Changes required** for digest `b01b09456e7ea3e98d6af9c47bd04cfb52fe2bc2e02f639b31eb00171247f7de`.
- Actionable: [FIXAPP-P01-B1-F1](agent-review-findings.md) (medium — remote revoke test case vacuous under D2),
  FIXAPP-P01-B1-F2 (low — unparseable stored address can no longer be changed), FIXAPP-P01-B1-F3 (low — doc
  comment misattached in `user.rs`).
- Optional: FIXAPP-P01-B1-F4 (NO ACTION comment rationale inaccurate), F5 (email-change races lack a barrier),
  F6 (hollow principal after retiring a never-spoken agent; plan-mandated).
- G1-F8: dispositioned (accepted) — recorded in findings.
- Recheck scope: the F1–F3 correction delta, the remote dispatch tests and the email-change tests; the rest of
  the evidence above can be reused if the digest changes only by those edits.
- Freshness at return: HEAD `e6b0a51`, digest unchanged `b01b0945…f7de`, COORDINATION revision 2 sha256
  `0e39a556…0e93` unchanged, no p01 notices.

### Process and instruction feedback (B1-R1)

- One review of the whole plan was feasible; the batch was large (~750 changed lines + ~1,700 lines of new tests)
  but cohesive. The fixture rewrites were the hardest part to review exhaustively.
- Useful: the author's per-filter log, the explicit fixture-rewrite helper list and the frozen-artifact section
  made independent reproduction cheap (frozen binary reuse avoided a rebuild on a tight disk).
- Gap (F1): enumeration of invalidated fixtures by "what failed" misses fixtures whose errors are swallowed into
  an expected-failure path. Candidate guidance recorded in the F1 entry.
- Defects are implementation-level; the gated decisions held.

## FIXAPP-P01-B1-R2 — correction recheck of B1

- Date 2026-11-01; workflow `implement-opus-reviewed-2_2`; stage B1 correction recheck; reviewer B, Opus 5.5 /
  xhigh; no sub-agents. Phase `p01`, worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p01`, branch
  `fix-app/p01`; base = HEAD `e6b0a51`, uncommitted changes; digest (exclusion `plan/fix-app/phases/p01/docs`)
  `ca8cf08ecb8b98c92e23c2585a22823c1605209d746e7cdc22b36f41d55dc23d` — matched at start and before returning.
  Gated decisions G1 r2 (+ F8 disposition), unchanged. COORDINATION.md revision 2, sha256 `0e39a556…0e93`,
  unchanged; I004 scope applied; no p01 notices.
- Delta identified: the six files A listed are exactly the changed files modified after the B1-R1 freeze
  (mtimes 01:40:09–01:40:48; all other changed files and the 4 `.sqlx` entries unchanged since 21:46). Reviewed:
  `action_dispatch_remote_tests.rs` (F1), `account_identity.rs` + new test (F2), `user.rs` doc order (F3), three
  schema comments + `agent_retire_tests.rs` doc (F4); TASK.md "B1 corrections after FIXAPP-P01-B1-R1" (F5
  declined, F6 deferred).
- Tested (`logs/B1-R2-reviewer-tests.log`, rebuilt binary `mail_agents-65122ec83e7c21fc` from 01:47, after the
  last edit; scratch DB `p01_review_scratch`, created and dropped): the remote revoke test alone passes with no
  cluster ERROR lines (the revocation commits); filters account_email_change_ remote_tests action_dispatch
  agent_retire_ company_owner_ company_invite_ channel_allowlist_ persistence::user → 346 passed, 0 failed
  (matches A's `t-recheck-1.log`). Inspected, not re-run: `clippy-2.log` (exit 0), fmt check (A's record).
- Reused from B1-R1 (unaffected code and fixtures): review of D1–D6 implementation, the fixture-rewrite audit, the
  66-filter run (`logs/B1-R1-reviewer-tests-2.log`), `.sqlx` audit (no macro query changed in the delta).
- **Outcome: PASS for digest `ca8cf08ecb8b98c92e23c2585a22823c1605209d746e7cdc22b36f41d55dc23d`.** B1-F1, F2, F3
  fixed; F4 fixed; F5 declined and F6 deferred (both optional, accepted). No open acceptance-blocking findings for
  B1. Not run under I004: full suite, `scripts/check.sh`, stack budget — so this pass covers the affected scope
  only, not the whole suite.
- Process feedback: one correction round with every finding answered and an itemised delta; the change list
  matched the actual file changes, which kept the recheck small. F5's decline reason mis-states which protocols
  the barrier tests cover (recorded on the finding); no instruction change proposed.

