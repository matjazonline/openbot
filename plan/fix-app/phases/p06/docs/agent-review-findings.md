# P06 reviewer findings

G1 entries are architecture findings. WB1 entries review the frozen implementation batch; later disposition sections supersede original statuses without erasing history.

## FIXAPP-P06-G1-F1 — Retention has no deletion path for workflow jobs

- Date: 2026-11-04. Workflow: `implement-astra-reviewed-2-2`; phase p06; stage architecture gate; round [FIXAPP-P06-G1-R1](agent-review-notes.md#fixapp-p06-g1-r1).
- Reviewer B `/root/p06_reviewer`, requested Astra/high in independent context. Actual model/effort metadata and compaction threshold are not exposed; no setting change is claimed.
- Worktree `/Users/mac02/dev/openbot-recovery/.worktrees/fix-app-p06`, branch `fix-app/p06`, review-base/HEAD `9c205daad5c029229f0547dc137a9936c6b2b49c`.
- Artifact: [G1 r1 D4](logs/G1-r1-decisions.md), SHA256 `ebe4bee36e2f15bfc1e5f1a10691cf9c7c34f911ef18ba68a7d9a44aae157c94`, lines 129–145.
- Classification: **actionable, high, architecture/correctness**. Status: **open**.

D4 archives usage, deletes all tree runs in one statement, and changes `background_tasks_workflow_execution_fk` to deferred `NO ACTION`, classifying it among internal cascade-round edges. This does not remove the workflow jobs. The run→execution edge cascades (`migrations/20260817000000_init_schema.sql:10531`), but the job→execution edge only refuses a missing execution (`:9217`). Background tasks cascade from company/channel, neither of which retention deletes (`:9193–9201`). There is no execution DELETE trigger to remove jobs, confirmed by the live catalog in [probe output](logs/G1-r1-review-probes.json). Every real admission creates an execution and its background task (`src/adapters/persistence/workflow/admission_write.rs:163–176`). Deferring the key moves the failure to commit; it does not make the surviving job disappear.

Consequently a normal terminal workflow tree with its job cannot be retired by the stated sequence. The archive/delete transaction rolls back, preserving correctness but defeating retention. The isolated PostgreSQL 18.6 reproduction uses exactly the relevant run→execution cascade and job→execution deferred `NO ACTION` shape and reproduces this commit failure. It is a relational protocol probe, not a test of an implemented p06 writer.

Required action: revise D4 to state the complete atomic lifetime/deletion protocol for the tree's workflow jobs and dependent attempt/checkpoint facts, including ordering against usage archival and append-only effect guards. Demonstrate that a real admitted terminal run with a job can commit complete removal, while unrelated jobs and protected runs remain. Preserve all tenant/ownership constraints. This asks the author to choose the correction; no replacement design is supplied.

Instruction lesson: an inbound FK inventory alone is not a deletion-closure proof. Candidate guidance for retention reviews: distinguish a deferred reference that vanishes through an independent cascade from a referencing row that has no delete owner. Supported by this finding; no instructions were edited.

Resolution history: 2026-11-04 opened against G1 r1; no correction reviewed.

## FIXAPP-P06-G1-F2 — Bundle conflict reuse does not yet establish the claimed GC serialization

- Date: 2026-11-04. Workflow: `implement-astra-reviewed-2-2`; phase p06; stage architecture gate; round [FIXAPP-P06-G1-R1](agent-review-notes.md#fixapp-p06-g1-r1).
- Reviewer/worktree/settings/base: as F1 above.
- Artifact: [G1 r1 D2](logs/G1-r1-decisions.md), SHA256 `ebe4bee36e2f15bfc1e5f1a10691cf9c7c34f911ef18ba68a7d9a44aae157c94`, lines 38–53.
- Classification: **actionable, medium, concurrency/availability**. Status: **open**.

D2 says admission inserts with conflict reuse, then inserts a run using an initially deferred FK, and relies on “Admission's unique-row/FK locks” to serialize with unused-bundle GC. The reused-row case needs a concrete locking/retry decision: a uniqueness check that takes the usual `ON CONFLICT DO NOTHING` path does not hold a row lock protecting the existing bundle until transaction end, and the initially deferred FK does not acquire its reference protection before the run is inserted/checked. Thus the stated steps do not by themselves establish the claimed serialization.

[The isolated competing-transaction probe](logs/G1-r1-review-probes.json) demonstrates this on the assigned PostgreSQL 18.6 instance: after admission has completed conflict reuse of an unused bundle, the observed `PgSleep` barrier lets GC lock it with `FOR UPDATE SKIP LOCKED`, confirm no run reference, and commit deletion. Admission then inserts its run and fails its deferred FK at commit. The FK prevents corruption, but maintenance causes a valid admission to fail. This probe does not imply all possible conflict-reuse implementations fail; it identifies the consequential protocol choice missing from the gate text.

Required action: specify how existing-row reuse is protected through run creation/commit, including what happens when GC wins before that protection is obtained. Keep the immutable payload and exact-content checks. Extend the promised GC/admission verification to prove controlled progress in both orderings with an existing, unused bundle; “no dangling rows” alone also passes when admission rolls back, so assert the intended admission result. The author owns the choice of lock/retry protocol.

Instruction lesson: when a gate asserts lock serialization, identify the conflict/reuse path as well as the fresh insert path. Candidate scoped guidance: test GC against a reused unreferenced object, not only a newly inserted object. Supported by this finding; no instructions were edited.

Resolution history: 2026-11-04 opened against G1 r1; no correction reviewed.

## FIXAPP-P06-G1-R2 — Resolution updates, 2026-11-04

These updates preserve the original R1 findings and change their current disposition.
Reviewer/workflow/worktree/base/settings are unchanged. Reviewed decision [G1 r2](logs/G1-r2-decisions.md), SHA256 `c4f6f73fced9a8482e8eb62b39a564ad6715900e085dd8e78ca8cb57f57c1401`. See [R2 note](agent-review-notes.md#fixapp-p06-g1-r2).

- **FIXAPP-P06-G1-F1: CLOSED at architecture gate.** D4 now captures and locks exact company/job/execution identities before deleting executions, archives usage, deletes the complete tree, and explicitly removes captured workflow jobs afterward in the same transaction. This satisfies the deferred job FK while retaining absent-run ownership for append-only dependent cascades. Expected removal identities are checked and failures roll back both deletion and archival. Author relational probe was inspected and independently rerun: jobs, attempts, checkpoints and guarded facts disappear for the selected owner, unrelated rows remain, and archived/live amounts remain 50/80. D5 now requires actual admitted-tree coverage and a failure between tree and job deletion. Those production tests have not yet run; this closure accepts the corrected architecture, not implemented behavior.
- **FIXAPP-P06-G1-F2: CLOSED at architecture gate.** D2 explicitly uses `ON CONFLICT ... DO UPDATE ... WHERE false` to hold existing-row ownership without a row update, then reads and compares exact bytes before inserting the run. It addresses both ownership orders and does not rely on the deferred FK for early serialization. The inspected and independently rerun probe observes GC skipping the admission-owned reused row; in the reverse order it observes admission waiting on the GC transaction, then reinserting the exact payload and committing a valid run. The rejecting row-level immutable UPDATE trigger stays active. D5 requires both existing-unused-bundle orderings in production-writer tests, including successful admission assertions. No protocol finding remains open.

Evidence: [independent rerun script](logs/G1-r2-reviewer-probes.py), [independent rerun output](logs/G1-r2-reviewer-probes.json), [author delta](logs/G1-r2-decision-delta.diff). Reviewer copy changes only scratch schema prefix and output destination; assertions and protocol SQL are unchanged. Scratch schema removed, public application schema unchanged.


## FIXAPP-P06-WB1-F1-MAC02 — Activation limit does not bound admitted tree cardinality

- Round: [FIXAPP-P06-WB1-R1-MAC02](agent-review-notes.md#fixapp-p06-wb1-r1-mac02), 2026-11-04. Classification: **actionable, medium, correctness/retention progress**. Status: **OPEN**.
- Source: `src/adapters/persistence/workflow/storage_retention.rs:31-39`; accepted G1 r2 D4 discovery premise at `logs/G1-r2-decisions.md:117-120`.
- Frozen source diff SHA256 `a23e85b0fb6675374168f0e0f0feeb7902550c7f43409c38bba477a9713f2502`; tree SHA256 `0e1955101cac3d0ba0c99a3bb63445ee76e156715071999042d79d7bba60d93e`, 1813 files, sole exclusion phase docs. Reviewer/settings/base/authority are recorded in the round note.

The new collector rejects every tree with more than 100000 runs as invalid, using the activation ceiling as a run-count ceiling. Admission does not charge a child activation: `children.rs:219-315` creates a queued child and its job, and `activation.rs:74-88` charges only after resolving its inputs. A normally admitted child can fail before that charge (`pending_io_tests.rs:132-176`). Ordinary activated workflow.call steps each admit a child; the root is additional to those calls. Sequential call graphs and failing unactivated leaves can therefore accumulate more admitted runs than consumed activations. The [source trace and construction](logs/WB1-r1-review-cardinality.md) explains a 100000-call/100001-run case without relying on a graph cycle or repeated-round allowance. A 100001-run production fixture was not executed; the underlying native admission/pre-activation premises were independently checked.

An otherwise eligible old tree above the imposed limit will fail on every cursor cycle and never be reclaimed, contrary to global retention and D4's requirement not to exempt valid trees with an invented smaller cap. Logging and advancing the cursor avoid starvation of other roots but do not fix this tree's permanent exclusion.

Required action: **reopen the consequential D4 cardinality/bounded-work decision**. Establish a defensible admitted-tree bound over all actual creation paths, including unactivated children, repeats and workflow tool calls, or otherwise handle valid large trees without this exclusion while retaining atomicity and bounded resources. Do not change runtime budget semantics solely to fit the collector. Add meaningful boundary/premise verification for the chosen rule, including admitted runs that have consumed no activation. The author owns the corrected design; a numeric increase alone is not proof.

Instruction lesson: do not equate a work-consumption budget with persistent entity cardinality unless every creation path charges that resource before commit. Accepted architecture remains open to correction when code review exposes the missing invariant.

## FIXAPP-P06-WB1-F2-MAC02 — Required production retry versus retention race is absent

- Round/settings/identity: same [WB1 r1 note](agent-review-notes.md#fixapp-p06-wb1-r1-mac02) and frozen diff/tree above. Classification: **actionable, medium, concurrency verification**. Status: **OPEN**.
- Evidence: G1 r2 D5 `logs/G1-r2-decisions.md:193-198`; `storage_tests.rs:225-238`; exhaustive [retire_tree call sites](logs/WB1-r1-retire-callers.txt).

D5 requires actual competing retry and retention transactions with a barrier. The frozen tests race two collectors and exercise both reused-bundle GC/admission orders, but no test races a production retry writer against whole-tree retention. The collector now deletes run/execution/job/attempt/reconciliation state under a new lock/deferred-FK protocol, while retry can make an old failed run live again. Sequential eligibility checks and collector-versus-collector tests cannot demonstrate that this transition is fenced or that either ordering preserves the intended retry outcome. The author confirmed this promised case is absent; reduced G1 relational probes do not cover it.

Required action: add a focused native-writer test with observable competing transactions for retry versus retention, covering the relevant ownership orders. Assert a coherent committed result, protection of revived/live work, no dangling job/attempt state and no partial usage archival. Keep production guards enabled. Re-run the new case and affected retry/retention checks. This finding requests evidence for the implemented protocol; it does not assert an observed race bug.

Instruction lesson: concurrency acceptance must exercise the state-changing competitor named in the design; another collector is not equivalent to a retry writer.

## FIXAPP-P06-WB1-F3-MAC02 — Real harness checkpoint deletion and rollback remain unverified

- Round/settings/identity: same [WB1 r1 note](agent-review-notes.md#fixapp-p06-wb1-r1-mac02) and frozen diff/tree above. Classification: **actionable, medium, lifecycle verification**. Status: **OPEN**.
- Evidence: G1 r2 D5 `logs/G1-r2-decisions.md:199-204`; `storage_tests.rs:101-162`; G1 r2 reduced relational probes.

The mandatory real admitted tree with jobs, attempts and harness checkpoints is not present. The complete-tree and between-run/job-delete rollback tests use a data.map chain; they do not create or assert a `task_harness_runs` checkpoint. Checkpoint rows lie on the job-owned cleanup path whose append-only guards require the run owner to have disappeared first. This exact ordering motivated G1-F1. The reduced relational probe demonstrates the intended pattern but cannot establish actual production checkpoint creation, FK/trigger closure or rollback. The author confirmed the frozen fixture has no harness checkpoint.

Required action: extend focused coverage through the actual native harness/checkpoint writer for an admitted terminal workflow tree. Assert the checkpoint and its owned dependent state disappear with the correct jobs at commit while unrelated/protected rows remain; inject failure between run deletion and job cleanup and assert the checkpoint, original tree and usage all survive with no history copy. Re-run the affected lifecycle/retention cases with native guards active. Existing data.map rollback coverage remains useful but does not close this obligation.

Instruction lesson: a cascade inventory or minimized schema probe does not replace a lifecycle fixture containing each guarded dependent class promised by the gate.


## FIXAPP-P06-G1-R3 — Selective cardinality gate resolution, 2026-11-04

Reviewed immutable [G1 r3](logs/G1-r3-decisions.md), SHA256 `d611a10527f94097b560e1a563a1d4acfa3f839b7844ceaeba9bbc3f1e73a8da`. Same independent reviewer/settings/worktree/base as [R3 note](agent-review-notes.md#fixapp-p06-g1-r3). Source remains the original frozen WB1 r1 artifact; this is an architecture correction, not a code correction.

- **WB1-F1-MAC02: architecture portion RESOLVED; implementation finding remains OPEN pending code/evidence recheck.** R3 withdraws the false activation-to-admitted-run cardinality premise and the numeric cutoff, without changing admission or runtime budget semantics. Scalar locking aggregates, root-link relational scope and transaction-local exact job capture replace client-sized ID collections. The author retains complete-tree ownership, unchanged eligibility, archive/delete atomicity, owner-before-job order, count comparisons and bounded operation lifetime. Count mismatch rolls back; it does not categorically classify a large valid tree as invalid. Reviewed invariant: owned runs cannot disappear while locked, and their budget identities cannot change or be removed while owners exist; therefore a later superset with equal cardinality is the same set. Reduced independent probes confirm held row ownership beyond 100000, rollback on a newly committed member, guarded deletion closure and temporary relation cleanup/reuse in the same session. Native-schema boundary, actual unactivated-child and corrected production-writer checks are explicitly required before closing the implementation finding.
- **WB1-F2-MAC02: OPEN.** R3 explicitly requires actual retry/retention competitors in both ownership orders with observed barriers; no production race test has been added or credited at this gate.
- **WB1-F3-MAC02: OPEN.** R3 explicitly requires actual WorkflowCheckpointStore open/reserve lifecycle, protected/unrelated preservation and between-run/job failure rollback; reduced relational checks do not close this finding.
- Original **G1-F1/G1-F2 remain CLOSED at architecture gate**; R3 preserves their explicit job deletion and reused-bundle ownership protocols. No new actionable or optional gate finding.

Gate outcome: **PASS for FIXAPP-P06-G1-R3**, exact decision hash above. This authorizes dependent correction implementation within the selected workflow; it does not accept frozen WB1 r1 or close its three code findings. Evidence: [independent scaling/cascade rerun](logs/G1-r3-reviewer-probes.json), [membership and same-session lifecycle checks](logs/G1-r3-reviewer-membership-probes.json), [freshness/source identity](logs/G1-r3-review-freshness.json).


## FIXAPP-P06-WB1-R2-MAC02 — Correction closure, 2026-11-04

Reviewed exact whole-phase source diff SHA256 `156523e0d100fb2bc7ef94d4bc4409a884701b09f080b84052874230733ed885`, protected tree SHA256 `2d1157f2b618248ab0c6a4513f0bc913330ce1ff6f0233b5f6b35ab972c61d19` (1816 files; phase docs only excluded). Same independent Reviewer B; settings/worktree/base/authority and reused evidence in the [R2 review note](agent-review-notes.md#fixapp-p06-wb1-r2-mac02).

- **FIXAPP-P06-WB1-F1-MAC02: CLOSED.** `storage_retention.rs:31-130` implements accepted G1 r3: no activation-derived cardinality rejection or client-sized ID vector/array; ordered run/execution locking aggregates; exact temporary company/job/execution capture; relational attempt/usage locking, eligibility and archival; materialized run deletion with owned-count equality before exact captured-job cleanup. Source inspection confirms same transaction, owner-before-job ordering and rollback on count mismatch. The actual admitted chain now asserts a terminal unactivated child and fewer activation debits than runs. The native-schema boundary test commits 100001 guarded members, invokes the production writer, reaches the exact first-DELETE failure marker, and verifies owner/job/live-usage preservation with no history copy. Independently passed on the exact frozen source. This does not claim 100001 runtime admissions or full purge performance. G1 r3's reduced ownership/membership/temp lifecycle evidence remains applicable to the implemented protocol.
- **FIXAPP-P06-WB1-F2-MAC02: CLOSED.** `storage_retry_tests.rs:4-105` uses actual production retry and retention writers with observable database barriers in both ownership orders. Retry-first holds the run transition, retention skips it, and retry commits live state with live usage retained and no archive. Retention-first holds the job-delete stage after owner removal; a competing retry is observed waiting, then returns NotFound after atomic retirement. Tests assert usage/archive counts, no retained owner/job on successful retirement and no dangling execution-linked jobs. All guards are enabled during the races. Both independently passed; the native archive exact-match guard and existing report-equality coverage remain intact.
- **FIXAPP-P06-WB1-F3-MAC02: CLOSED.** `storage_checkpoint_tests.rs:4-169` invokes actual WorkflowCheckpointStore open, execution reserve and model reserve writers. It creates a separate live checkpoint in the same company/database, cancels/ages the selected native owner, and checks exact snapshot restoration across runs, executions, jobs, attempts, checkpoints, calls and history after the between-run/job-delete fault. Subsequent success deletes the selected checkpoint and owner/job/usage while leaving the protected checkpoint byte-for-byte unchanged. Both direct success and fault/rollback/retry cases independently passed with native guards active.

Outcome: **PASS for FIXAPP-P06-WB1-R2-MAC02**. All WB1 implementation findings are closed for this artifact; no new actionable or optional findings. Earlier architecture findings remain closed. [Independent exact-source checks: 42 PASS](logs/WB1-r2-review-storage.log); [final source/authority recheck](logs/WB1-r2-review-final-identity.json). This review does not perform root acceptance, commit or integration.
