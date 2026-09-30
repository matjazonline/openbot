# Bounded claim-budget SQL refusal/audit verification

Status: checkpointed implemented subset; SQLx prepare --check, independent actual-code
review and root acceptance pending. Root requested STOP at its context threshold.
This is V1 item2's refusal/audit subset, not complete item2, foundation or04.7 acceptance.
Original Execution6, BRIEF-04.7:194–240,261–326, accepted CLAIM-BUDGET contract
SHA2564ea6c4f8652cdd1889ead7ac77abea7702e89b04bb70fa080b393236e45a3938 and
AUDIT remain authoritative. No04.8 or V1items3–4 work.

Baseline: HEADc9cb4b434258b6d33ca26def4ff6b5728dd5a6bc plus inherited WIP;
preceding frozen `/private/tmp/workflow-04.7-claim-races-01a0f6e2/HANDOFF.md`.
Own delta: new `action_reconciliation_claim_sql_tests.rs`, three-line eligibility
module registration and this evidence file. No production or migration edit.
All53 applied migrations immutable; prior16 dependencies/5criteria hashes matched
the preceding frozen race baseline before work. Initial whole-worktree hashes and
diff, exact own delta, artifacts/dependencies/migration/SQLx hashes, preservation
checks and full command logs live in `/private/tmp/workflow-04.7-claim-sql-01a0f6fb/`.

## Criteria implemented

| Case | Discrimination and observation |
| --- | --- |
| Eligible fabricated refusal | Genuine scheduled episode; predicate explicitly true; exact episode and audit scoped references; fabricated exhaustion label fails23514 `invalid current workflow reconciliation budget refusal`. |
| Ineligible caller-labelled refusal | Real descendant claim/reservation exhausts shared allowance; predicate explicitly false; same valid references but no current retirement; exact refusal diagnostic. |
| Historical audit | Audit owner writes/commits a same-execution exhaustion-kind event in a prior transaction; ineligible episode remains pending; subsequent refusal fails exact guard. This historical event is independently labelled, not a historical genuine refusal. |
| Consumed episode | Actual claim installs next attempt; exact attempt increment and pending-episode NULL asserted; real sibling reservation exhausts budget; exact old-episode refusal fails guard. |
| Wrong-kind audit | Production pending retirement owner establishes current-xid, confirmed retirement and genuine linked exhaustion audit; second otherwise valid same-execution wrong-kind audit cannot replace it. Exact refusal diagnostic. |
| Same-transaction protected audit | Real pending retirement and valid refusal pass `SET CONSTRAINTS ALL IMMEDIATE` first; kind/execution/sequence/company/run/actor UPDATE, DELETE and primary-key UPSERT replacement each fail23514 exact `workflow claim budget retirement audit is immutable`. |
| Later protected audit and positive control | Actual `claim_io` commits one genuine refusal; existing `assert_refusal` verifies episode/scoped audit/job preservation plus unchanged attempts/usage; all eight later mutation/replacement/deletion attacks fail the exact audit guard. |

Each negative has an explicit rollback and exact equality of **all public tables**
before/after (including full run/job/revision/attempt/episode/refusal/audit/accounting
and existing action/evidence/proof/provider history). FK references are real accepted
owner history; no copied provenance, disabled trigger, rewritten history or production
guard bypass. Fixtures use existing isolated database owner and all migrations.
The same-transaction positive deferred check rules out an earlier masked FK failure.
The consumption attack demonstrates refusal rejection after a next attempt exists;
it does not independently isolate each conjunct of the refusal predicate.

## Explicitly pending within V1 item2

- New episode wrong-scoped-command, wrong retired ordinal, current-xid/deferred
  witness cases and duplicate candidate isolation. Genuine scheduling automatically
  inserts unique command/job/ordinal bindings; a fresh raw insert against existing
  history hits that uniqueness before the deferred episode guard. A separate genuine
  fixture/fault-injection design must retain valid composites and isolate the intended
  guard; do not copy or delete protected episode history to manufacture it.
- Exact wrong-execution audit using a second genuine execution in the same run,
  with its event composite FK valid. NULL execution mutation is covered above but
  does not substitute for this refusal-insert case.
- Historical same-execution audit substitution **with a genuine current retirement**
  isolated from event execution/kind uniqueness, and a historical genuine retirement
  witness/current-xid substitution. The implemented historical test covers absent
  current retirement only; it cannot certify these stronger cases.
- Deferred refusal revalidation independently discriminating a next-attempt install
  after initially valid refusal; V1items3–4 and all broader contract/original04.7 gates.

## Verification and resource record

Focused six tests PASS with explicit retained database URLs, SQLX_OFFLINE=true,
RUST_MIN_STACK=2097152, locked/offline. First compile failure (ActionScope versus
ActivationRequest) corrected; original log retained. Formatting PASS after final
reformat; first check failure retained. Affected action_dispatch_tests104PASS,
including eligibility/lifecycle/races and new SQL tests. A final test-only improvement
calls existing lease::lock_scope before pending retirement; final focused6PASS and
final strict all-target ClippyPASS cover that changed seam. Broader104 and offline
all-target check passed immediately before that helper addition; unchanged regression
evidence reused. Migration info/schema inspection/graftPASS. SQLx prepare-alltargets
PASS; root STOP prevented starting prepare --check, which remains unverified. Exact
commands/outcomes are in `commands.jsonl` and frozen HANDOFF.md.

Retained PG18system7691265791172745923, data `/private/tmp/workflow-admission-pg-e3aa`,
database workflow_admission/mac03, port55439/socket/private/tmp/max_connections200
verified before use. No retained reset/drop/checksum rewrite or limit raise.
Worker UUID01a0f6fb-af61-7db0-8e66-a9c66b7d0d6a; no children. Startup26,032/258,400
=10.07%; milestone94,335/258,400=36.51% at2026-10-01T10:30:01.721Z. Sources
token_usage_record.usage/task_started.model_context_window. Final sample/resource
quiescence is in frozen HANDOFF.md. Root alone owns acceptance.
