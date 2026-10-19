# Latest continuation supplement — 2026-10-19

The historical reviewer handle below is not live in the resumed session. Root was
reconfirmed Astra/high by the user. No code review has been requested for this unfinished
batch. Native owner/store remains next under accepted D4 revision2.

Added admission.rs root-reviewer validation and admission_token_tests.rs. Fresh children
read actual inherited root policy through workflow_run_budgets; no root run lock. Every
reviewer uses existing locked person/member owner/admin authority. Replays remain unchanged.
22 admission tests passed at stock2MiB/four threads; offline all-target and strict Clippy
passed. TASK and06B2b-admission-gates.log carry final fresh migration/SQLx status. Earlier
failed test expectations are retained in logs; no production guard was weakened.
All existing WIP/cluster data preserved. Main test DB still has prior checksum; focused
admission tests use isolated migrated databases. Disposable SQLx DB is script-owned only.
No user stop, commit or deployment. Continue remaining D4 implementation, then whole-batch
review and final phase gates; do not treat this supplement as acceptance.

---

# 06B2b in-progress handoff (not acceptance)

Root remains confirmed Astra/high author. Reviewer /root/reviewer_06 remains idle,
independent Astra/medium. D4 revision2 is accepted after06-R012. No further design
approval or user permission is pending. Continue implementation; no user stop request.

## Implemented WIP since accepted06B2a

- Shared RunCheckpoint retains actual usage above reservation, uses conservative
  max(reserved,actual) hard totals, and saves BudgetExceeded terminal state on hard
  overflow. Reserve totals must independently remain valid. Driver/open reject terminal
  budget state. SQL state enumeration extended. Unit and isolated DB proof added.
- RootBudgetLimits has optional TokenAllowance(max_tokens u32 bounded i32::MAX,
  1..16 unique reviewer UUIDs); ExecutionLimits is Clone, no longer Copy. Two admission
  callers and compiler test borrow limits. BudgetCharge.consumed_after borrows limits.
- Compiler parses tokens {max_tokens,reviewers:[{kind:user,id:UUID}]}, freezes optional
  policy in compiled.root_budget; compiler revision4 matches frozen_v4 SQL admission.
  Root table stores immutable token_policy JSON with valid_workflow_token_policy guard.
  Previous compiler3 sealed-revision test updated. SQL shape matrix covers malformed,
  duplicate, fractional/string and overbound cases. Monetary properties rejected.
- AgentSnapshot includes agent_id. Publication rejects nil and hashes UUID. Fixtures
  use deterministic UUID1. Existing connection reference is(company_id,provider), not
  a connection UUID; it already lives in frozen snapshot fields. See company model
  connection table/entity. Do not create a second identity or persist secrets.
- agent.run optional approval uses factored registry/actions.rs::approval helper,
  sharing action schema/static reviewer/InputCheck rules. No new approval runtime yet.

## Still required (do not claim operational token approval)

1. Validate live configured root reviewers before admission; descendants inherit root
   policy. Current config parser/SQL function only validate shape, not membership.
2. Extend existing task_harness_runs with disjoint native ownership, exact FKs/fence,
   immutable prepared state, active request owner and blocked proposal; preserve all
   other legacy auxiliary guards and avoid legacy invocation projection on native writes.
3. Native application-owned store and real WorkflowFence operations; same root usage
   lock for admission/settlement, shared allowance and typed irreversible refusal.
4. Token proposals/approval preserve exact request/wait/attempt; approval only reopens.
   Fresh claimed transaction reserves/debits and alone returns non-clone NewAdmission.
   No replay/ambiguous response permits provider entry; unknown calls stay charged.
5. Reuse single Rig drive loop through closed legacy/native bindings, frozen preamble,
   exact saved model/provider/agent/capabilities/context/schema/target identity and
   current authorization. Sequential ActionService effects; exact delivery wait resume.
6. D4 crash/race/SQL matrix, focused+broader gates, consolidated06B2b review. Then06B3,
   06C and final phase06 suite/integration review. No06B2b source review yet.

## Evidence/resources

Read latest TASK for exact commands/results. Checkpoint unit11/0 then isolated DB1/0;
config compiler1/0; budget27/0 at2MiB four threads; all-target check passed after UUID;
application203/0 after approval/UUID changes. Latest checkpoint follow-up log is
06B2b-checkpoint-unit-2.log; inspect completion before relying on it.
Historical failed compile logs retained (borrow and test str.as_ref corrections).
Fresh isolated test DBs apply updated migration; main workflow0410_test remains on
old checksum. No existing DB reset this segment. Cluster55450 system identifier
7697669855432439122 confirmed. Preserve presquash/postsquash databases. Serialize
source writes/builds/DB checks, CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4, stock2MiB.
No commit/deploy, no limits raised. Source is WIP, not frozen for review.

Latest completion: checkpoint follow-up12/0, affected isolated DB1/0, strict all-target
Clippy PASS54.51s, fmt/diff/graft PASS. No processes running except retained PostgreSQL.
WIP snapshot13705122bb2e844704b9ac0c4789177ca796c0a47b5d89d25865f8b52acc85df
on base5b4989ae0038a82f431db60a338b7f59d9bcf25c, sole docs exclusion as usual.
No source review/full phase gate/schema equality/SQLx regeneration yet.
