# p06 — Operations and verification (memory-service)

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

- Phase ID: p06. Workflow: implement-opus-reviewed-2_2. Record root:
  /Users/mac03/.codex/worktrees/60a0/openbot/plan/memory-service/phases/p06/docs/
- Root: session 6fedfb2e-40aa-486c-bd0b-051b5a3add7b, Opus 5.5 / xhigh (user-confirmed), author.
  Reviewer B: abdd866fb428eea05 (Opus/xhigh), continuing from p05.
- Worktree /Users/mac03/.codex/worktrees/60a0/openbot, detached HEAD a6bed48a (no commits authorized).
  Starting artifact: accepted p05 final tree 7388d1c2 (digest 88808f5a…b0a1).
- Snapshot: `bash /Users/mac03/.claude/skills/implement-opus-reviewed-2_2/scripts/snapshot.sh tmp plan/memory-service/phases/p05/docs plan/memory-service/phases/p06/docs plan/memory-service/COORDINATION.md`
- Coordination: plan/memory-service/COORDINATION.md revision 4 (I001 no commits/pushes/deploys/resets;
  I002 never localhost:5432, never .env; I003 r1–r4 product decisions U1–U7). Checked 2026-11-10.
- Resources: private PostgreSQL 18 + pgvector cluster 127.0.0.1:55432 (tmp/memory-service-run/pg-cluster.sh);
  test DB open_bots_root_test; destroy at the end.

## Requirements
plan/memory-service/06-operations-and-verification.md (06A.1–5, 06B.1–4, 06C, completion criteria);
implementation-decisions.md carries marked 06A (retention lines 1071-1082; P6b n2/purge 1137-1142;
provisioner rotation/managed_by reassignment 1198-1206; 06A conditions 1902-1912; credential list
bound 1929; settled-intent retention 1988); p05 carries: B6-F1 (CI service toolchain pin), B6-F2
(socket-style admin URLs in scripts/memory-integration.sh), B6-F3 (operator token on curl argv),
CI execution of the `memory-integration` job and browser step, earlier deferred F10/F16 (p05 records).

## Batches (proposed; split at stable contracts)
| Batch | Scope | Status |
| --- | --- | --- |
| p06-A1 | Service retention + purge + forgetting (idempotency, audit, tombstones/deletion barriers, generation inputs already done in P6b), tenant `managed_by` reassignment, credential list pagination; cancel inference before cleanup; no resurrection (P6b n2). Architecture gate (deletion/retention protocols, schema) | pending |
| p06-A2 | Observability: exported metrics (readiness, queue age/depth, indexing failures, retries, terminal/unknown generation, lease loss, fairness, provider usage, context omissions; existing in-process counters), request correlation ids, no prompts/secrets in logs; OpenBot side metrics | pending |
| p06-A3 | OpenBot retention: settled memory_deliveries (after the 7-day window), settled lifecycle rows + index + threshold metric, managed_by divergence conditions; B6-F1–F3; F10/F16 | pending |
| p06-A4 | Ops docs (rotation incl. reassignment, provider outage + auth circuit breaker, recovery, restore without replay, backup test on disposable DBs), image/model/runtime pinning, non-root + network-denied startup, resource budgets with early CI failure, stack bounds; `memory_operations_` | pending |
| p06-B | 06B.1–4: extracted standalone build + generic client journey; separately built OpenBot + service e2e incl. the 6/8/9 level matrix; separate-host topology; real competing-claimant cases (`memory_standalone_`, `memory_e2e_`) | pending |
| p06-C | 06C final full-suite gate for both projects | pending |

## Next action
Gates passed: A1 (R3, revisions 1–3, optional F9 taken: 403), A3 (R3), A2 (R2, F6 taken R3).
Implement A1 (service), then A3 and A2 (OpenBot and service), then the A1–A3 batch review. Meanwhile A3 OpenBot-only work that does not depend on A1 contracts.

## p06-A3 progress (carries done early, reviewed with A3)
- B6-F1: `memory-integration` CI job installs the service's pinned 1.95.0 explicitly
  (`rustup toolchain install 1.95.0 --profile minimal`) after the stable install.
- B6-F2: `scripts/memory-integration.sh` checks `MEMORY_INTEGRATION_ADMIN_URL` before anything
  connects: a socket-style URL (no TCP host) exits 2 ("must name a TCP host"); its query string
  (e.g. `sslmode`) is carried to the service URL. Verified: socket-style → exit 2; live run with
  `?sslmode=disable` → 2 passed, nothing left (`logs/MEM-P06-A3-live-carries.txt`).
- B6-F3: the operator token reaches curl on stdin (`-H @-`), not argv.

- F16 (MEM-P05-B2-F16): `complete_delivery` runs in a transaction with `SET LOCAL lock_timeout =
  '2s'`; a 55P03 maps to `DeliveryCompletion::Lost` (warn log), leaving the intent `claimed` for
  lease reclaim. `MemoryDeliveryWorker::pass` returns `Ok(false)` when the stop token is cancelled
  after the store mutex is acquired, before any claim. Tests (DB, private cluster):
  `memory_integration_a_completion_behind_a_row_lock_gives_up_and_is_reclaimed` (another
  connection holds the intent `FOR UPDATE`; completion is `Lost` within 5 s, bounded by a 10 s
  test timeout so a regression fails instead of hanging; the row stays `claimed`; after the lease
  is expired a pass delivers it) and `memory_integration_a_stopped_worker_claims_nothing` (state
  `pending`, `transient_retries` 0, no `observations/save` call). `cargo test --lib --
  memory_integration_a_` 4 passed (`logs/MEM-P06-A3-F16-tests.txt`); `memory_integration_` +
  `memory_capture_` 69 passed, 2 ignored (the live tests).
- F10 stays deferred to 06C (isolation of the full gate), as recorded in p05.

## p06-A1 architecture gate (revision 1)
Investigation: the service schema (`services/memory/migrations/0001_init.sql`), deletion paths
(`adapters/postgres/mod.rs` `change_tenant_once` 453-548, `delete_namespace` 1061-1123,
`cancel_operations` 360-446), observation delete/correct (`postgres/memory.rs` 947-1085), the
maintenance task (`generation.rs` `reap_once` 1186-1260, `sweep_once` 1262-1358), settlement
(`settle_once` 947+, `recheck_authority` 175-268), idempotency (`postgres/memory.rs` 148-310,
`application/memory.rs` 97-135) and `list_credentials` (`mod.rs` 1134-1166). Today nothing removes
content of a deleted tenant or namespace, expired observations, idempotency records, audit events
or spend rows; purge answers 501; `managed_by` is never changed; credential listing is capped at
1,000 unpaginated rows. Stored idempotency results hold ids, revisions, levels, states and opaque
external ids only (no memory text), and audit `details` hold ids and counts.

Decisions (record: D3 "the sweeper removes its content in bounded batches; tombstoned ids can never
be recreated"; D9 retention; D5; D13 lock order; P6b n2; E1/E12 06A conditions):

A1-1 Tombstones and content removal. Tenant and namespace rows stay forever (opaque ids). New
  columns `tenants.content_removed_at`, `namespaces.content_removed_at` (CHECK: only on
  `state = 'deleted'`), partial index on namespaces `WHERE state = 'deleted' AND content_removed_at
  IS NULL`. Tenant delete now also tombstones every active namespace of the tenant and deletes its
  grants in the same transaction (after the tenant `FOR NO KEY UPDATE`, which already excludes
  every writer, claim and settle because they take the tenant `FOR SHARE` first), so one predicate
  drives removal. A new maintenance category removes a deleted namespace's content only once
  **no operation of that namespace is unfinished** (cancel inference before cleanup): delete/purge
  cancel accepted/claimed and flag dispatched rows (existing); a flagged dispatch cannot renew its
  lease, so it settles `cancelled` (output discarded) or the reaper settles it after lease + 30 s.
  Then, per tick, ≤500 rows per step in FK order, each step `… FOR UPDATE SKIP LOCKED` on the
  candidates and a second statement that re-checks the predicate: observations (parts and links
  cascade), `generation_operation_sources` + finished operations no observation references,
  sources no link/operation references, sessions nothing references; finally namespaces with no
  remaining rows get `content_removed_at`. A deleted tenant whose namespaces are all removed loses
  its grants, idempotency records, spend days and schedule row, then gets `content_removed_at`.
  Credential rows of deleted tenants are kept (verifier hash, opaque label, timestamps), so an old
  token still gets the uniform `credential_revoked` rather than changing to `unauthenticated`.
  Audit events are not removed by tenant deletion (A1-4).
A1-2 Client purge (`namespaces.purge`, implemented; 501 removed). Authorization: the `purge` grant
  on the namespace (a write, as today). Lock order (D13): tenant `FOR SHARE`, namespace `FOR NO KEY
  UPDATE`, calling credential `FOR SHARE`, idempotency claim, then operations in id order. Effect:
  the admin namespace delete's effect (grants of every credential removed, operations cancelled
  with `namespace_deleted`, tombstone) plus audit `namespace.purge` (actor client); content goes
  through A1-1. Response `{namespace, purged: true}`. Reads and writes stop at commit (they already
  require `state = 'active'` under the namespace lock or in their snapshot). Replay: the purge
  removed the caller's own grant, so a retry fails authorization; only for this operation, an
  authorization refusal first looks up `(tenant, 'namespaces.purge', key)` executed by the same
  credential with an identical request hash and replays it (`replayed: true`); any other caller or
  hash gets the ordinary refusal. Recreation stays refused (409 `namespace_deleted`).
A1-3 No resurrection (P6b n2 and stale workers). `recheck_authority` returning `None` (a missing
  tenant, namespace or credential row) becomes a stop `cancelled(authority_missing)` for a `Saved`
  settlement instead of saving. Generation saves already re-check tenant/namespace under `FOR SHARE`
  (a purge either commits first → stop, or waits and then sweeps what was saved). The indexer's
  commit returns `Stale` for a removed row; finished operations are frozen by the existing trigger.
  Expired-observation removal re-checks `expires_at` in the deleting statement so a correct that
  lands between scan and delete is not lost.
A1-4 Retention windows (06A.1), one row each in the README table:
  - canonical memories: until deleted, corrected (no history) or `expires_at` → **new**: expired
    observations are deleted (index `observations_expiry`), ≤500 per tick;
  - raw evidence and generation inputs: unchanged (P6b, 30 days; nulled, metadata kept for
    provenance until the namespace is removed);
  - idempotency results: **new** sweep of records with `created_at < now() - 7 days` and
    `request_expires_at < now()` (D5), index `idempotency_retention`. Generation keys: keep
    `UNIQUE (tenant_id, idempotency_key)` on operations and map its violation (a key reused with a
    new deadline after the record was swept) to 409 `idempotency_conflict`, so one key still gives
    at most one operation while that operation exists;
  - audit: **new** sweep of events older than 400 days (`audit_retention`), and a `BEFORE DELETE`
    trigger refusing to delete a younger event, so retention is enforced in the database;
  - spend days: **new** sweep after 400 days (index on `day`);
  - tombstones and finished operation rows: kept until the namespace is removed (tombstones forever).
  All new categories join the maintenance tick with the existing bounds (500 per category, 100 ms
  after a full category, else 5 s, error backoff).
A1-5 Tenant reassignment (provisioner rotation, E1). New operator-only admin operation
  `admin.tenants.reassign {from, to, limit?: 1–500 (default 500)}` → `{moved, remaining}`: moves
  `managed_by` from admin credential `from` (may be revoked: the compromise case) to `to`, which
  must be an active, unexpired **provisioner** credential (else 409 `invalid_target`), for tenants
  in any state including tombstones, in id order `FOR NO KEY UPDATE` after locking both admin
  credential rows `FOR SHARE`. One audit row per moved tenant (`tenant.reassign`, from/to). A
  request the old provisioner already holds a tenant lock for finishes first; its next request is
  `not_found`. Repeating the call moves the rest (naturally idempotent, no key).
A1-6 Credential listing (E1 m5/r2-m4). `admin.credentials.list` gains `label?` (exact match),
  `active_only` (default false: unrevoked and unexpired only when true), `after?` (credential id
  cursor) and `limit` (1–1000, default 1000); the response adds `next_after` (the last id when the
  page is full). OpenBot's lifecycle (A3) lists with its `ob:<namespace>` label and `active_only`,
  following `next_after`; `credential_list_truncated` then only arises from an unbounded page loop
  guard.
A1-7 Documentation: README retention table, purge and forgetting (correct overwrites in place,
  delete removes vectors and links at once, purge tombstones then removes content after inference
  settles), and the limits of deletion: text already returned to a client, copied into prompts,
  transcripts or frozen preparations, or sent to the provider (no zero-data-retention, D12) is
  outside the service's reach; OpenBot revalidates frozen packs on resume (B4 R2) so deleted
  memory is not re-served from a frozen preparation.

Failure modes and invariants: no content row of a removed namespace survives and none is written
after removal starts; no operation is deleted unfinished; a crash between steps leaves a state the
next tick resumes (each step is its own transaction, idempotent on what remains); competing sweepers
skip each other (`SKIP LOCKED`); the content steps lock only content rows; marking a namespace or
tenant removed is its own transaction that locks only that parent row (revision 2, F1), so purge
and delete wait at most for that short statement.

Affected contracts: `openapi.json` (purge implemented, credentials list fields, reassign op);
OpenBot adapter (`credentials/list` paging in A3; the contract fake mirrors the new fields);
`memory_contract_` snapshot; README; `docs/deploy.md` rotation (A4).

Verification plan (`memory_operations_`, service DB tests, competing claimants on separate
connections where a protocol races):
- purge: cancels accepted/claimed and flags dispatched; a late `Saved` settlement is cancelled; no
  content removed while an operation is unfinished; after the sweep no observation, part, link,
  source, session, op-source or operation row of the namespace remains, the tombstone does, and
  recreate is 409; replay by the same credential, refusal for another; purge racing a save on two
  connections (save first → swept; purge first → refused);
- tenant delete tombstones namespaces; full tenant removal; old token still `credential_revoked`;
- n2: a `Saved` settlement with a missing credential row is `cancelled(authority_missing)`;
- stale indexer commit after removal → `Stale`, no parts;
- expired observation removal, and a correct racing it;
- idempotency sweep boundary (7 days and deadline), retry after it → 410, generation key reuse → 409;
- audit sweep at 400 days and the trigger refusing a younger delete;
- two sweepers on separate connections never double-delete or deadlock (counts add up);
- reassign: moved/remaining, old provisioner `not_found`, new one manages, provisioner caller 403,
  revoked/operator target 409, audit rows;
- credentials list: label, active_only, cursor pages cover every row exactly once.
Focused: `scripts/focused.sh memory_operations_`, `memory_contract_`, `memory_authority_`,
`memory_generation_`, `memory_storage_` (affected callers); mutation probes for A1-1's unfinished
-operation guard, A1-2's replay restriction and A1-3's stop.

### A1 revision 2 (review MEM-P06-A1-R1)
- F1 (lock order): every content step runs without any parent lock. Marking is a separate
  transaction: `SELECT … FROM namespaces WHERE id = $1 AND state = 'deleted' AND
  content_removed_at IS NULL FOR NO KEY UPDATE`, then `UPDATE … SET content_removed_at = now()`
  only `WHERE NOT EXISTS` any observation, operation, op-source, source or session of it. No child
  row can appear meanwhile: writers, settles and indexer commits take the namespace `FOR SHARE`
  and refuse a deleted one (n2 stop for settles). The tenant step is the same: child deletions
  (grants, idempotency records, spend days, schedule, label blanking) in their own bounded
  transactions; then a marking transaction that locks only the tenant row, with `NOT EXISTS` any
  unremoved namespace or child row. The invariant above is corrected.
- F2 (races):
  - Purge replay: purge's transaction claims the idempotency record right after its locks (the D5
    order) and *before* re-checking the namespace state and grant. A concurrent identical purge
    that waited on the namespace lock therefore finds the winner's committed record and replays.
    The pre-lock authorization refusal (a retry after the commit) consults the record the same way,
    for the same credential and identical hash only.
  - Tests on separate connections:
    `memory_operations_identical_purges_race_to_one_execution_and_one_replay`;
    `memory_operations_reassign_waits_for_an_in_flight_provisioner_request_then_refuses_it` (one
    connection holds an old-provisioner request's tenant lock while reassign runs: reassign
    completes after it, and the old provisioner's next call is `not_found`);
    `memory_operations_reassign_races_old_provisioner_writes` (concurrent issue/disable calls by
    the old provisioner: each either commits before the move or is `not_found` after it; no
    tenant ends half-moved).
- F3 (limits of deletion):
  - Copies in other namespaces: an observation in namespace B that cites a parent in A keeps its
    own text and label when A is deleted or purged; only the parent link goes (cascade). This is
    the established decision (implementation-decisions.md 762-768: "a copy keeps its own label");
    B's memory was written under B's write authority, and A's deletion must not reach into B.
    It is documented as a limit, and tested:
    `memory_operations_purge_leaves_a_cross_namespace_copy_and_drops_its_parent_link`.
  - Restore: restoring a service backup older than a deletion brings the deleted content back,
    and the restored audit log does not record that deletion. So a restore must be followed by
    re-applying deletions before serving clients.
    - For OpenBot tenants, the record is OpenBot's lifecycle rows settled `absent` after the
      backup time. A3-2 keeps them 30 days, so a backup older than 30 days cannot be reconciled
      from OpenBot and must not be restored without an operator-held deletion list.
    - Carried to A4: the restore procedure (keep the service closed to clients, re-drive OpenBot's
      absent lifecycles updated since the backup, then open), and a disposable-database test of it.
- F4 (credential labels): labels are free text, so at tenant removal every credential's label is
  replaced by `removed` (the verifier hash and timestamps stay, so old tokens still answer
  `credential_revoked`). Credentials get their own row in the README retention table: kept for
  the tenant's life (revoked and expired included; A1-6 lists them with `active_only`), labels
  blanked at tenant removal, rows kept forever with the tombstone.
- F5 (metadata growth; taken): a finished operation that no observation references is deleted with
  its op-source rows 30 days after it finished (its request is nulled then already). A source
  whose content was nulled 30 or more days ago and that no link or op-source references is
  deleted. Consequence, documented: re-recording a removed revision creates a fresh source row
  (revision immutability holds while anything references the revision). OpenBot's capture refs
  are per exchange and never re-recorded. Sessions stay (one row per session, no content).
  `GET generations/{id}` for a removed operation is `not_found` (its idempotency record is gone
  after 7 days anyway).
- F6 (taken):
  - `remaining` is a bool everywhere;
  - `from = to` is 409 `invalid_target`;
  - the two admin credential rows are locked in id order;
  - a new index `credentials (tenant_id, label, id)`;
  - `list_credentials` loads the grants of a page in one query (`credential_id = ANY($2)`) instead
    of one per credential.

### A1 revision 3 (review MEM-P06-A1-R2)
- F7: label blanking is keyed on the tenant's state, not on its mark: the blanking step's
  candidates are credentials of any `deleted` tenant whose label is not `removed` (≤500 per
  tick, `FOR NO KEY UPDATE SKIP LOCKED`), whether or not the tenant is marked. The tenant mark's
  `NOT EXISTS` adds "no credential of the tenant with `label <> 'removed'`". Test:
  `memory_operations_tenant_removal_blanks_every_credential_label_beyond_one_batch` (1,201
  credentials: three ticks blank them all, the tenant is marked only after the last batch).
- F8 (taken):
  - (a) A concurrent identical purge by **another** credential is refused 403 `forbidden`, exactly
    as a fresh execution by that credential would be (`denied`, application/memory.rs:36-45, and
    the in-transaction namespace re-check; review A1-R3 F9 corrected `not_found`): purge's replay
    authority is "the caller holds the `purge` grant on the namespace now", and the winner's
    commit removed every grant. The same credential replays. Test: two credentials with purge
    grants race identical purges (same key, same body); one is `purged`, the other 403, and the
    record shows one executing credential.
  - (b) Carried to A4's restore procedure: a restore also undoes every observation delete and
    correction any client made after the backup, and the service cannot re-apply those (clients
    hold that knowledge). The procedure says so, and documents that OpenBot issues no per-memory
    deletes or corrections today.
  - (c) The implementation record (implementation-decisions.md, a new 06A "As implemented"
    section) states that F5 amends D9's sentence "Identity, revision, level and digest stay for
    provenance": they stay while anything references the source, then go 30 days after its
    content was nulled.

## p06-A3 architecture gate (revision 1)
Investigation: `memory_deliveries` / `memory_lifecycles` schema (`migrations/20260817000000_init_schema.sql`
14980-15087), save enqueue (`persistence/memory/deliveries.rs` `freeze_save_on`, callers
`workflow/memory_steps.rs::park_on` and `memory_tool_checkpoint.rs::save_on`, both under the run
lock; a key that exists replays), capture enqueue (once per accepted exchange), the effect-wait
wake (schema 187-194 reads the intent through `wait_id`), the lifecycle worker
(`application/memory/lifecycle.rs` `pass` 246-281, `tenant_absent` 386-396, `namespace_absent`
444-466, `revoke_labelled` 481-523) and the existing storage sweep pattern with monitoring counters
(`application/workflow/storage.rs::sweep`). Today settled intents and settled lifecycle rows are
kept forever; the claim's expired-lease arm and the stuck query are served only by the partial
`pending` index; a namespace absent step accepts a 404 that, once A1-5 exists, can mean "managed by
another provisioner"; rotation leaves every lifecycle row `provisioner_mismatch` forever.

A3-1 Settled intent retention. A delivery intent is removed when it is terminal (`delivered`,
  `failed`, `expired`, `cancelled`), `created_at < now() - 7 days` (past the service's 7-day
  idempotency window, so its key cannot execute again there) and it can no longer be replayed or
  read here: a `generation` intent (enqueued once, at exchange acceptance) or a `save` whose run is
  terminal (`succeeded`/`failed`/`cancelled`) or already retired. A save of a live run is kept,
  because a re-run of its parked step replays the intent by key and a waiting step reads its
  receipt through `wait_id`. Bounded: ≤500 rows per pass, candidates `FOR UPDATE SKIP LOCKED` in
  id order (the E9 order), deleted in a second statement that re-checks the predicate. New index
  `memory_deliveries_settled (created_at, id) WHERE state IN (terminal states)`.
A3-2 Settled lifecycle retention and indexes. A lifecycle row is removed when `state = 'settled'`,
  `desired = observed = 'absent'`, `updated_at < now() - 30 days`, and its owner is gone (a
  namespace row with no `memory_namespaces` row; a tenant row with no `memory_tenants` row).
  Namespace ids are never reused after removal, and the service tombstone (409
  `namespace_deleted`) still refuses any re-creation, so removing the row loses no barrier.
  `failed` rows are kept (operator-visible). New indexes: `memory_lifecycles_leased (lease_until,
  id) WHERE state = 'claimed'` (the claim's expired-lease arm and the expiry sweep) and
  `memory_lifecycles_settled (updated_at, id) WHERE state = 'settled' AND desired = 'absent'`.
A3-3 Maintenance driver and threshold metric. One supervised `MemoryMaintenance` task (infra,
  when the memory service is configured) runs every 5 min: the two sweeps (A3-1, A3-2), repeating
  a full batch up to 8 times per run, then records gauges `memory_deliveries_rows{state}`,
  `memory_lifecycles_rows{state}` and `memory_retention_backlog_rows{table}` (rows eligible but not
  yet removed) and counters `memory_retention_deleted_total{table}`; errors count
  `memory_retention_errors_total` and back off like the storage sweep. A backlog above 10,000 rows
  logs a `warn` once per run (the early signal for the bound; the batch limits are constants,
  tested at the boundary).
A3-4 Provisioner rotation adoption (needs A1-5). When a claimed row's recorded provisioner differs
  from the configured one, the worker probes `tenants/create` with the configured credential: a 200
  (any state) proves the configured credential manages the tenant (after an operator reassigned
  it), so the company's `memory_tenants.provisioner_credential_id` and all its lifecycle rows are
  re-recorded to the configured id in one transaction (company lock order) and the step proceeds;
  a 409/404 keeps today's `provisioner_mismatch` postponement (5 min). No row ever moves to an id
  the service has not confirmed.
A3-5 Absent steps never complete on "not ours" (E1 06A condition). `namespace_absent`:
  - a create 404 is not taken as absent: the worker probes `tenants/create`; 200 → `Wait(0)`
    (the next pass's create is definite: 200, 403 or 409); 409/404 → `Fail("tenant_unmanageable")`
    (definite, surfaced; an operator reassigns, then the owner's "Retry failed provisioning");
  - with the tenant disabled (create 403), a delete 200 completes; a delete 404 no longer completes
    (a reassignment or an earlier create still in flight cannot be excluded without the quiescence
    deadline the persistence guide requires): it waits (`Wait("tenant_disabled", TENANT_WAIT)`)
    until the tenant is enabled (then create + delete is definite) or deleted (its tombstone
    answers 409, which completes). Nothing is reachable meanwhile: a disabled tenant serves no
    client request.
  - `revoke_labelled` (today a listing 404/409 returns "nothing left", lifecycle.rs 488-492) keeps
    that only on the absent path, which now reaches it solely after a definite tombstone (create
    409 or delete 200: the namespace's grants are gone, so no credential of it can serve); on the
    present path (create answered 200, so the tenant was ours) a listing 404 becomes
    `Fail("tenant_unmanageable")` instead of settling `present` with unseen strays.
  `tenant_absent` already fails a 404 (`tenant_unmanageable`).
A3-6 Credential listing (needs A1-6): `revoke_labelled` lists with `label = ob:<namespace>` and
  `active_only`, following `next_after` for at most 4 pages per pass; more → `Wait("revoking_strays",
  0)` while it revoked something, else `Retry("credential_list_truncated")` as today.
A3-7 Carries: B6-F1–F3 and F16 done (above); F10 deferred to 06C. Docs: `docs/deploy.md` rotation
  (issue the new provisioner, `tenants/reassign` until `remaining = false`, switch OpenBot's token,
  rows adopt on their next pass, revoke the old one) and the retention windows of both tables.

Invariants: a removed intent can never be re-enqueued or read by a waiting step; no lifecycle row
whose removal could let a namespace be recreated is removed; no absent completion rests on a 404
that can mean "managed by someone else"; every sweep is bounded and leaves a resumable state.

Verification plan (OpenBot DB tests, prefix `memory_operations_`): intent retention boundaries (6
vs 8 days; live vs terminal vs retired run; generation kind; a wait still parked keeps its intent);
two sweepers on separate connections remove each row once; lifecycle retention (owner present
kept; failed kept; 29 vs 31 days); adoption with the contract fake managing the tenant under the
new id (rows re-recorded, step proceeds) and refusing it (postponed, ids unchanged); namespace
absent 404 → probe → definite paths, disabled + 404 waits, tombstone completes; listing pages with
label/active_only; gauges and counters recorded via the monitoring stand-in; `EXPLAIN` checks that
the claim's lease arm and the sweeps use the new indexes. Mutation probes: drop the run-terminal
condition (A3-1), drop the owner-gone condition (A3-2), restore the 404-as-absent (A3-5).

### A3 revision 2 (review MEM-P06-A3-R1)
- F1 (control retry): a control retry can reopen a `failed` run with the same execution while
  `run.deadline > now()`, which would replay `ob.s.<execution>`. A3-1's predicate for `save`
  intents becomes: the run is retired, or it is terminal **and** `run.deadline < now()`. A retry
  needs a future deadline, so after the deadline no path can re-enqueue or read the intent. Tests:
  a failed run with a future deadline keeps its intent; the same run after its deadline loses it.
  Mutation probe: drop the deadline condition.
- F2 (rotation order): the documented order becomes:
  1. an operator issues the new provisioner credential;
  2. OpenBot's token is switched to it (restart);
  3. the operator runs `tenants/reassign` from the old to the new credential until
     `remaining = false`;
  4. rows adopt on their next pass (≤ 5 min);
  5. the old credential is revoked.
  Rotation adds two rules:
  - every "not ours" answer for a tenant that OpenBot recorded (a probe 409/404 in A3-4 and
    A3-5, and a namespace-present create 404) postpones `provisioner_mismatch` (5 min, spends no
    attempt), instead of a definite failure. That also covers a namespace row created in the
    window under the new id;
  - `tenant_unmanageable` stays definite only for the tenant steps' 404 after a successful
    create probe in the same step.
  The A3-3 task gauges `memory_lifecycle_postponed_rows{outcome}`, so a rotation that never
  completes is visible; settings already show the mismatch. Test: the full sequence against the
  contract fake (switch → postponed → reassign → adopted → steps complete; no `failed` row).
- F3 (taken):
  - the sweep orders by `(created_at, id)` to match its index (`SKIP LOCKED` never waits, so the
    E9 id order is not needed);
  - gauges count only open rows (`pending`, `claimed`, served by the partial indexes), plus the
    backlog;
  - `EXPLAIN` checks run with `enable_seqscan = off`.
- F4 (documented in `docs/deploy.md`):
  - an absent step on a disabled tenant waits until enable or delete;
  - configuring an operator token as the provisioner makes adoption match every tenant (never do
    it: the provisioner must be a provisioner credential);
  - a probe 200 can mean the probe itself created the tenant, which is correct for adoption;
  - `failed` rows need "Retry failed provisioning".

### A3 revision 3 (review MEM-P06-A3-R2)
- F5: `tenants/create` answers 409 for every tombstoned tenant, ours included
  (`postgres/mod.rs:904-905`), so it cannot prove management of a deleted tenant. Instead, one
  `manages_tenant` probe serves A3-4 and A3-5:
  1. it calls `credentials/list {tenant, limit: 1}` (A1-6). That goes through `lock_tenant`
     without an active-state requirement, so a 200 proves the configured credential manages the
     tenant, in any state, tombstones included;
  2. only on its 404 (not ours, or never created) does it fall back to `tenants/create`. A 200
     there proves management, including a tenant the probe just created;
  3. a 409 or 404 then means not ours: postpone `provisioner_mismatch`.
  Adoption therefore re-records rows of a deleted company whose deletion is still in progress, and
  their absent steps then complete. Test:
  `memory_operations_rotation_mid_deletion_adopts_and_completes_pending_absent_rows` (a deleted
  company with pending absent namespace rows recorded under the old id; switch, reassign, adopt;
  every row settles `absent`, none stays postponed).

## p06-A2 architecture gate (revision 1)
Investigation: the service has no metrics export. It keeps in-process counters
(`SEMANTIC_PARTIAL_TOTAL` at `postgres/search.rs:36-42`, `USAGE_EXCEEDED` at
`postgres/generation.rs:41-48`, `Maintenance::ticks`). Its request ids are UUIDv7 minted per request
(`http/mod.rs:81`, `mcp/mod.rs:85`); they appear only in error bodies and in `audit_events.request_id`,
and no inbound correlation is read. OpenBot has `MonitoringService` (counters, gauges, histograms;
`src/domain/monitoring.rs:60-72`) and an operator-only `/metrics` route
(`adapters/http/routes/monitoring.rs`). The OpenBot memory adapter sends no correlation id.

A2-1 Service metrics. A new operation `GET /admin/v1/metrics` requires an **operator** credential
  (provisioners get 403, clients 401) and returns Prometheus text. Labels come only from closed sets
  (operation id, error code, state, category), never from tenant or namespace ids. A small
  hand-rolled registry of atomics is used, with no new crate.
  - Counters: requests by operation and outcome code; generation settlements by state and
    outcome; dispatches; retries; lease losses (renewal refused, settle `Stale`); provider tokens
    (input, output, cache read, cache write); usage above the reservation; index commits,
    failures and stale commits; semantic partial; context packs and omitted items (budget);
    retention rows by category; maintenance errors.
  - Gauges, from one bounded aggregate query at scrape (statement timeout 2 s; a timeout returns
    the counters plus `memory_metrics_query_ok 0`): readiness; accepted and dispatched operation
    counts; oldest accepted age; `unknown` operations finished in the last 24 h; tenants waiting;
    the largest per-tenant oldest-wait (fairness); indexing pending count, oldest pending age and
    failed count; seconds since the last maintenance tick.
A2-2 Correlation.
  - A client may send `x-correlation-id`: 1–128 bytes of `[A-Za-z0-9._:-]`, opaque and non-secret.
    A malformed value is `400 invalid_request`, so a bad id is never silently dropped.
  - The service echoes its own `x-request-id` (always server-minted, never taken from the client)
    on every response, success included.
  - It logs `request_id`, `correlation_id`, `operation` and the outcome code for every request at
    `info`.
  - It stores the correlation id in a new `audit_events.correlation_id` column (CHECK on the same
    shape) and on the operation at generation acceptance (`generation_operations.correlation_id`).
    Worker logs (claim, dispatch, settle, reap) carry `operation_id` and `correlation_id`.
  - OpenBot sends the delivery intent's idempotency key (`ob.s.…`, `ob.g.<exchange>`) on
    deliveries. On recall and tool reads it sends `ob.r.<run>.<execution>`. It logs the same id
    beside run, exchange and intent ids.
  - So a capture can be followed by one id: from OpenBot's exchange acceptance, through the
    service's generation operation and the saved observations, to a later recall.
A2-3 Log hygiene.
  - No prompt, memory text, event text, query text, token or credential is logged.
  - Provider error bodies are reduced to type and code before logging.
  - A test runs save, search, pack, generation (fake provider) and a refused request with a
    marker string in every text field. It captures the service's tracing output and asserts that
    the marker and the bearer token never appear.
  - OpenBot gets the same kind of test over a delivery, a recall and a capture.
A2-4 OpenBot metrics, through `MonitoringService`:
  - `memory_deliveries_settled_total{kind,state}`;
  - `memory_delivery_completions_lost_total`;
  - `memory_delivery_transient_retries_total`;
  - `memory_recall_total{outcome}`;
  - `memory_recall_omitted_total` (the pack's `omitted.budget`);
  - `memory_capture_skipped_total{reason}`;
  - `memory_lifecycle_failed_rows` and `memory_lifecycle_stuck_rows` gauges, from A3-3's task;
  - the A3-3 retention gauges and counters.
  All of these are already-known events at existing call sites; no new queue is introduced.

Invariants:
- No tenant- or content-derived label or log field.
- A metrics scrape cannot block writers. Its query is read-only, under a statement timeout,
  and takes no row locks.
- A correlation id is never authority: it is not hashed into idempotency identity, never compared
  for replay, and never trusted as a request id.

Verification:
- `memory_operations_` tests check metrics auth (operator 200, provisioner 403, client 401).
- After scripted work, counters change by the expected amounts.
- A gauge query that times out still answers, with `memory_metrics_query_ok 0`.
- Correlation tests cover: echo; a malformed id gives 400; the id is in the audit row and on the
  operation; worker logs carry it (captured tracing).
- The hygiene marker test runs on both projects.
- OpenBot tests check the header on deliveries and recall (contract fake records headers) and
  the counters recorded through the monitoring stand-in.

### A2 revision 2 (review MEM-P06-A2-R1)
- F1 (hygiene test): the service test installs a process-wide subscriber
  (`tracing::subscriber::set_global_default` in a dedicated test binary target, or
  `#[tokio::test(flavor = "current_thread")]` with the worker driven on the same thread).
  - Positive control: before asserting absence, it asserts that a request line, an operation
    line and a worker settle line were captured, each carrying the correlation id.
  - The marker goes into every text field. It also goes into a refused request where serde quotes
    the offending value in its error (an unknown field and a wrong type, service.rs:204).
  - OpenBot's test follows the same pattern, with a positive control.
- F2 (affected contracts):
  - OpenAPI: the `x-correlation-id` request header parameter on every operation; the
    `x-request-id` response header; the metrics route (`text/plain`, outside the operation
    registry and with no MCP tool).
  - The `memory_contract_` snapshot.
  - Service schema:
    - `audit_events.correlation_id`;
    - `generation_operations.correlation_id`, which joins `generation_requests_are_immutable`'s
      frozen columns.
  - OpenBot port and fake:
    - `MemoryClient` methods (ports.rs:394-438) gain a `Correlation` argument
      (`Correlation::Delivery(key)` / `Correlation::Recall { run, execution }`), and every
      caller is updated;
    - the contract fake records headers per call.
  - `/mcp`: the header on the HTTP request applies to the JSON-RPC call it carries (stateless mode
    is one call per request); a malformed one is a JSON-RPC error with the 400 code.
  - Capacity-middleware refusals also carry `x-request-id` and count as
    `memory_requests_total{operation="unrouted",code="unavailable"}`.
- F3: README states that counters are per process (sum across instances), while gauges are
  database-wide (read from any one instance).
- F4: lease losses are counted by reason: `renewal_cancel_requested`, `renewal_lost` and
  `settle_stale`.
- F5: the capture-to-recall trace is stated as a join path:
  - OpenBot exchange → intent key (= correlation id) → service operation (`correlation_id`);
  - → observations (`generation_operation_id`) → a later recall's items (observation ids)
    under that recall's own correlation id.

### A2 revision 3 (review MEM-P06-A2-R2, optional F6 taken)
- F6: capacity refusals count under their real code:
  `memory_requests_total{operation="unrouted",code="capacity_exceeded"}`, matching the 429
  response.

## p06-A1 implementation (gate revisions 1–3 passed)
Changed (services/memory):
- Schema (`migrations/0001_init.sql`, init migration edited in place, no compatibility):
  - removal markers: `tenants.content_removed_at`, `namespaces.content_removed_at`, each CHECKed
    to deleted rows only;
  - new indexes: `namespaces_pending_removal`, `tenants_managed_by`, `credentials_by_label`,
    `generation_by_namespace`, `sources_removal`, `observation_links_source` (it also serves the
    FK check when a source is deleted), `tenant_spend_retention`;
  - the operations key constraint is now named `generation_operations_one_per_key`;
  - `audit_retained`, a `BEFORE DELETE` trigger refusing to delete events younger than 400 days.
- `adapters/postgres/retention.rs` (new):
  - `remove_deleted_once` removes the content of deleted namespaces only once no operation is
    unfinished: observations, then operations no observation names (with their op-sources),
    then sources, then sessions, each with no remaining references;
  - each step is its own transaction, ≤ limit rows, `FOR UPDATE SKIP LOCKED`;
  - marking a namespace or tenant removed locks only the parent rows; the candidates already
    have no children, and the update re-checks;
  - label blanking runs for every deleted tenant (F7); the tenant's rows go once all its
    namespaces are removed; the tenant mark requires that no unblanked label remains;
  - `sweep_aged_once` removes expired observations (re-checked), idempotency records (7 days
    and past their deadline), audit events and spend rows older than 400 days, and unreferenced
    finished operations and nulled sources older than 30 days (F5); the last three re-check
    their predicate in a fresh snapshot.
- `postgres/mod.rs`:
  - `tombstone_namespace` is shared by admin namespace delete and client purge;
  - tenant delete tombstones every active namespace in id order and removes all grants;
  - `reassign_once`: the admin rows are taken `FOR SHARE` in id order, then the tenants
    `FOR NO KEY UPDATE` in id order without skipping, with one audit row per moved tenant;
  - `list_credentials` filters by label and `active_only`, pages by `after`/`limit` and
    returns `next_after`; `grants_of_page` loads a page's grants in one query.
- `postgres/memory.rs`:
  - `purge_once`, in D13 order: tenant `FOR SHARE`, namespace `FOR NO KEY UPDATE`, credential
    `FOR SHARE`, then the claim; after the claim, the namespace state and the caller's `purge`
    grant are re-checked;
  - another credential's record replays only while the caller holds the grant, so it gets 403;
  - `purge_replay` serves the pre-lock path;
  - `lock_writer` is split into `require_tenant` and `require_credential`.
- `postgres/generation.rs`:
  - a `Saved` settlement with authority `None` becomes `cancelled(authority_missing)`;
  - a key-constraint violation maps to 409 `idempotency_conflict`.
- Application layer:
  - contract types `CredentialListRequest`, `CredentialList.next_after`, and
    `TenantReassignRequest`/`TenantReassignResponse`;
  - the operation `admin.tenants.reassign` (operator only); purge is now implemented;
  - `service.rs`: the bounds `MAX_CREDENTIAL_PAGE` (1,000) and `MAX_REASSIGN_BATCH` (500),
    and `invalid_target`;
  - `memory.rs`: purge dispatch with the replay-on-refusal path;
  - ports: `CredentialPage`, `Removed`, `Aged`, `remove_deleted`, `sweep_aged`,
    `purge_namespace`, `purge_replay`, `reassign_tenants`;
  - `MaintenancePass`: the tick now runs reaper → input sweep → removal → age sweep;
  - `domain/retention.rs` holds the windows.
- README: "Deletion, purge and forgetting", the retention table, and "Provisioner rotation and
  listing". `openapi.json` regenerated; `.sqlx` regenerated with sqlx-cli 0.8.6 against
  `mem_p06_prep`.
Deviation from the gate: the reassignment target refusal is `409 conflict` with
`details.reason = "invalid_target"` instead of a new error code (no taxonomy growth). The A1-4
"correct racing expiry" test was not written: a correction cannot change `expires_at`
(`CorrectRequest` has none), so there is no race. The expiry delete still re-checks.
Defect found by the tests and fixed: `remove_sessions` lacked the namespace-level
unfinished-operation guard and removed a session while a flagged call was still running.
Tests (`postgres/operations_tests.rs`, all DB-backed, private cluster), 13 `memory_operations_`:
- purge: `purge_cancels_inference_then_removes_every_content_row`;
  `identical_purges_race_to_one_execution_and_one_replay` (same credential: one execution, one
  replay; two credentials: one success, one 403; post-commit replay and refusal);
  `purge_leaves_a_cross_namespace_copy_and_drops_its_parent_link`;
- removal and no resurrection: `tenant_removal_blanks_every_credential_label_beyond_one_batch`
  (1,201 credentials: labels 500/500/201, marked only after the last);
  `a_saved_settlement_without_its_credential_row_is_a_stop` (the row is removed under
  `session_replication_role = replica`); `a_stale_indexer_commit_after_removal_writes_nothing`;
- age-based retention: `expired_observations_are_removed_once_by_competing_sweepers` (4
  sweepers, 1,200 rows, each removed once);
  `idempotency_records_age_out_and_keys_never_execute_twice` (6 vs 8 days, then 410; the
  generation key is 409); `audit_and_spend_rows_age_out_at_400_days` (the trigger refuses a
  younger delete); `unreferenced_operations_and_nulled_sources_age_out`;
- reassignment: `reassign_moves_tenants_in_batches_and_refuses_bad_targets`;
  `reassign_waits_for_in_flight_requests_and_races_old_writes`;
- listing: `credential_listing_pages_by_label_and_activity`.
Updated: `contract_tests.rs` (purge is implemented, every client op has an example, none is
unimplemented, the purge HTTP/MCP calls reach the use case); `generation_tests.rs` (`tick()`
returns a `MaintenancePass`).
Checks:
- `cargo test --locked --offline --lib memory_operations_`: 15 passed (13 new, plus 2
  existing contract/MCP tests whose names contain the prefix);
- the full service suite, `cargo test --locked --offline --all-targets`: 166 passed, 0 failed
  (`logs/MEM-P06-A1-service-suite-2.txt`);
- `cargo clippy --all-targets -D warnings` clean; `cargo fmt --check` clean;
  `scripts/boundary-check.sh` ok;
- not run yet: `scripts/verify.sh` (the full service gate, with the network-isolated runner),
  which belongs to 06C;
- mutation probes, in an isolated copy (tmp/memory-service-run/mut-p06a1, its own
  CARGO_TARGET_DIR, deleted afterwards; patches and outputs in `logs/MEM-P06-A1-mutation-*`).
  Each was killed by its decisive test:
  - MA1, removal ignores unfinished inference →
    `purge_cancels_inference_then_removes_every_content_row`;
  - MA2, purge replays for any credential →
    `identical_purges_race_to_one_execution_and_one_replay`;
  - MA3, a missing authority saves → `a_saved_settlement_without_its_credential_row_is_a_stop`;
  - MA4, a tenant is marked with labels left (the candidate and the re-check condition both
    removed) → `tenant_removal_blanks_every_credential_label_beyond_one_batch`.

## p06-A3 implementation (gate revisions 1–3 passed)
Changed (OpenBot):
- Schema (`migrations/20260817000000_init_schema.sql`):
  - `memory_deliveries_settled (created_at, id)` over the terminal states;
  - `memory_lifecycles_leased (lease_until, id) WHERE claimed`;
  - `memory_lifecycles_settled (updated_at, id) WHERE settled AND desired = absent`.
- `persistence/memory/retention.rs` (new), `MemoryMaintenanceStore`:
  - `sweep_settled_deliveries` removes an intent that is terminal, more than 7 days old, and
    either a capture or a save whose run is retired or terminal with `deadline < now()` (F1);
  - `sweep_settled_lifecycles` removes a row that is settled absent/absent, more than 30 days
    old, and whose owner row is gone;
  - each sweep is one CTE statement, `FOR UPDATE SKIP LOCKED`, in index order (F3);
  - `memory_row_counts` returns the open-row counts, stuck rows (12 or more transient retries),
    `provisioner_mismatch` rows, and both backlogs.
- `application/memory/maintenance.rs` (new): `MemoryMaintenance` runs every 5 min. It does up to
  8 batches of 500 per table and records:
  - gauges `memory_deliveries_rows{state}`, `memory_lifecycles_rows{state}`,
    `memory_lifecycle_failed_rows`, `memory_lifecycle_stuck_rows`,
    `memory_lifecycle_postponed_rows{outcome}` and `memory_retention_backlog_rows{table}`;
  - counters `memory_retention_deleted_total{table}` and `memory_retention_errors_total`;
  - a warning when the backlog exceeds 10,000.
  It is spawned in `infra/workflow.rs` beside the lifecycle worker, only when memory is
  configured.
- Lifecycle (`application/memory/lifecycle.rs`):
  - `manages_tenant` probe: `credentials/list {limit: 1}`, then `tenants/create` on its 404
    (F5);
  - `pass()`: on a mismatch, a confirming probe calls `adopt_provisioner`, otherwise the row is
    postponed with `provisioner_mismatch`;
  - `not_ours()`: postpones (`provisioner_mismatch` for 5 min, or `tenant_recheck` when the
    probe confirms); used for a namespace-create 404 on both the present and the absent paths,
    and for a present-path listing 404;
  - absent with a disabled tenant: a delete 404 now waits `tenant_disabled`;
  - `revoke_labelled`: label plus `active_only`, at most 4 pages of 1,000, with an `absent`
    flag.
- `adopt_provisioner` (`persistence/memory/lifecycle.rs`): locks the company's `memory_tenants`
  row, then its lifecycle rows in id order; it re-records only differing ids. The claim and
  expiry SQL became `CLAIM` and `EXPIRE_LEASES` constants, so the EXPLAIN test checks the real
  statements.
- Port: `CredentialQuery`, `CredentialPage`, and `list_client_credentials(tenant, query)`. The
  adapter sends `label`, `active_only`, `after` and `limit`, and reads `next_after`.
- Contract fake:
  - `credentials/list` filters by label and `active_only`, pages with `next_after`, and answers
    for any tenant state its manager manages;
  - new `accept_admin(token)` and `reassign(from, to)` (the operator's call).
- Docs (`docs/deploy.md`): the outcome list (404 rule, the disabled wait, the 4,000-credential
  bound); the retention and gauges paragraph; a new "Rotating the provisioner credential"
  section with the order and edges (F2, F4).
- Carried to A4: the "Restoring the memory service" procedure in the operations guide, which
  deploy.md now references, with the 30-day reconcile window.
Tests:
- Rewritten for real rotated tokens (the old ones paired a credential id with a token that does
  not carry it): `memory_scope_reconciler_refuses_a_changed_provisioner_credential_id` (now the
  whole rotation sequence: postponed with no destructive call, the operator reassigns, rows
  adopt, the old credential is refused) and
  `memory_scope_reconciler_mismatch_refuses_tenant_and_namespace_deletion` (only probe calls are
  made under a foreign credential).
- New in `lifecycle_tests`:
  `memory_operations_rotation_mid_deletion_adopts_and_completes_pending_absent_rows` and
  `memory_operations_absent_steps_never_complete_on_a_404_that_may_mean_not_ours` (disabled + 404
  waits; reassigned elsewhere postpones; reassigned back completes).
- New in `retention_tests`:
  - `memory_operations_settled_intents_and_lifecycle_rows_age_out_at_their_bounds` (6 vs 8
    days, open rows kept, capture kind; lifecycle 29 vs 31 days, failed rows and rows with an
    owner kept);
  - `memory_operations_competing_retention_sweepers_remove_each_row_once` (3 sweepers, 1,200
    rows);
  - `memory_operations_maintenance_records_gauges_and_the_backlog` (3 batches in one run;
    gauges, counter, and a backlog of 7 beyond 8×500);
  - `memory_operations_retention_and_lease_queries_use_their_indexes` (EXPLAIN with
    `enable_seqscan = off`: the settled indexes, `memory_lifecycles_leased` for the expiry scan
    and the claim, and `memory_lifecycles_due`).
- New in `memory_steps_tests`:
  `memory_operations_settled_save_intents_outlive_every_run_that_can_replay_them` (F1: a live
  run, then a terminal run before its deadline, are both kept; after the deadline the intent is
  removed).
Checks:
- `cargo test --lib -- lifecycle_tests memory_service::tests`: 59 passed.
- `retention_tests lifecycle_tests`: 51 passed.
- The broader memory filters run in the batch check below.
- Mutation probes, in an isolated OpenBot copy (tmp/memory-service-run/mut-p06a3, its own
  CARGO_TARGET_DIR, deleted afterwards; patches and outputs in `logs/MEM-P06-A3-mutation-*`).
  Each was killed by its decisive test:
  - MB1, deadline guard dropped →
    `memory_operations_settled_save_intents_outlive_every_run_that_can_replay_them`;
  - MB2, owner guard dropped →
    `memory_operations_settled_intents_and_lifecycle_rows_age_out_at_their_bounds`;
  - MB3, a 404 counts as absent →
    `memory_operations_absent_steps_never_complete_on_a_404_that_may_mean_not_ours`;
  - MB4, adoption without confirmation → both rewritten mismatch tests.

## p06-A2 implementation (gate R2 passed, F6 taken at R3)
Service (services/memory):
- `domain/correlation.rs`: `CorrelationId` (Copy, validated), plus a unit test.
- `RequestIds {request, correlation}` (`service.rs`), with `From<Uuid>` so existing callers stay
  unchanged. `App::execute(…, impl Into<RequestIds>)` carries it into `ClientWrite.correlation`
  and `Actor.correlation`.
- Schema:
  - `audit_events.correlation_id` and `generation_operations.correlation_id`, both CHECKed to
    the id alphabet; the operation column joins the immutability trigger;
  - `observations_index_failed`, a partial index for the gauge.
- Writes: client and admin audit rows record the correlation; acceptance stores it on the
  operation; the settle audit row copies it from the operation.
- `GenerationJob.correlation` (read at claim) carries it into the worker's log lines for
  dispatch, lease loss and settle.
- `application/metrics.rs`: a closed-label counter registry (`METRICS`), Prometheus rendering,
  `Gauges`, `LAST_MAINTENANCE_UNIX`.
- Counters: requests by operation and code (HTTP and capacity), settlements by state and
  outcome, dispatches, retries, lease losses by reason, reaped by kind, provider tokens by kind,
  usage above the reservation, index commits by result, semantic partial, packs and omitted
  candidates, retention rows by category, and maintenance errors.
- HTTP (`http/mod.rs`):
  - `correlation()` parses the header; a malformed one is 400;
  - `with_request_id` sets `x-request-id` on every answer;
  - one info line per request carries ids and codes only;
  - `GET /admin/v1/metrics` is operator-only, with gauges from `Store::metrics_gauges` (a
    read-only transaction under a 2 s statement timeout); a failed query is
    `memory_metrics_query_ok 0`.
- MCP parses the header from the HTTP request parts; a malformed one is a structured
  `invalid_request`.
- OpenAPI: every operation gets the `x-correlation-id` parameter and the `x-request-id` response
  header; the metrics path is added. `openapi.json` regenerated; README "Observability".
OpenBot:
- Correlation: a task-local scope (`application/memory/ports.rs` `with_correlation`,
  `current_correlation`, `read_correlation`); the adapter's `post` sends the header.
  - Deliveries scope their intent key (`FrozenRequest::key`).
  - Recall (`MemoryRecall::step` and `validate`) and the memory tools (`call_memory_tool`)
    scope `ob.r.<run>.<execution>`.
  - Delivery logs carry `correlation_id`.
- Metrics:
  - `MemoryDeliveryWorker::with_monitoring` counts `memory_deliveries_settled_total{kind,state}`,
    `memory_delivery_completions_lost_total` and `memory_delivery_transient_retries_total`;
  - `MemoryRecall::with_monitoring` counts `memory_recall_total{outcome}` and
    `memory_recall_omitted_total` (`MemoryPack.omitted`, parsed from `omitted.budget`);
  - both are wired in `infra/workflow.rs`;
  - the lifecycle failed and stuck gauges come from A3-3.
- Contract fake: records `(operation, x-correlation-id)` per call (`correlations()`).
- Docs: deploy.md correlation and metrics paragraph.
Deviations from the gate (for the batch review):
- (a) OpenBot passes the correlation through a task-local scope instead of a new argument on
  every `MemoryClient` method. The header contract is exactly as gated; only the internal
  plumbing differs. It keeps 14 struct sites and about 22 call sites unchanged, and the
  adapter's concurrent joins run on the calling task, so they see it.
- (b) Capture skips are counted from a structured log line (`metric="memory_capture_skipped_total"`,
  `reason`) rather than through `MonitoringService`, because the acceptance checkpoint is in
  persistence, which holds no monitoring handle.
- (c) The fairness gauge is the largest per-tenant accepted queue: the largest per-tenant
  oldest wait would equal the global oldest wait, which is already exported.
Tests:
- Service (`postgres/observability_tests.rs`):
  - `memory_operations_metrics_export_is_operator_only_and_counts_requests`: operator 200,
    provisioner 403, client 401; counter, ready and gauge lines; no tenant label; with the
    operations table locked elsewhere the export answers within 5 s with
    `memory_metrics_query_ok 0` and no stale gauges;
  - `memory_operations_correlation_ids_reach_audit_and_operations`: a malformed header is 400
    with `x-request-id`; a valid one reaches the acceptance audit row (with the matching request
    id), the operation and the settle audit row; MCP with a malformed header is
    `invalid_request`;
  - `memory_operations_logs_never_carry_content_or_secrets`: a process-wide capture subscriber
    (F1). The positive control requires that this test's own save, pack, generation-request and
    worker-settle lines carry its correlation id. The marker is in every text field and in serde
    refusals (an unknown field and a wrong type); no marker and no token appear in the logs.
  - Unit: `memory_operations_correlation_ids_accept_only_the_id_alphabet`.
- OpenBot:
  - `memory_operations_capture_and_delivery_logs_carry_ids_not_content` (e2e): the capture and
    delivery lines are captured, the delivery line carries the intent key, the fake saw that
    key as `x-correlation-id`, the delivery counter is 1, and the marker is absent;
  - the recall test `memory_preparation_recall_freezes_…` now asserts both packs carried
    `ob.r.<run>.<execution>`.
Checks:
- Service: the full suite, `cargo test --all-targets`, 170 passed
  (`logs/MEM-P06-A2-service-suite.txt`); clippy clean; fmt clean; boundary ok; `.sqlx`
  regenerated.
- OpenBot: `cargo test --lib -- memory_` 161 passed, 3 ignored (the live tests). Two failed only
  because the shared test DB carried the old init-migration checksum; after resetting
  `open_bots_root_test` (no compatibility) they pass (`logs/MEM-P06-A123-openbot-memory.txt`).
  Clippy `--all-targets -D warnings` is clean; fmt is clean.

## p06-A4 architecture gate (revision 1)
Investigation:
- Already in place from earlier phases: `memory-service.yml` builds the image from pinned
  digests and runs it as non-root (`USER memory`, uid 10001), network-denied and read-only
  (`version`, `model-check` with a peak-RSS budget, and startup without a database). The
  model, runtime and toolchain are pinned (`assets/model-manifest.json`, `rust-toolchain.toml`).
- Provider outcomes (`ports.rs` `ProviderError`): `NotSent`, `Rejected` (retried up to 3
  dispatches), `Terminal` (incl. `authentication_error` and `permission_error`), `Ambiguous`.
  Today a revoked or wrong key fails every queued operation `failed(authentication_error)`,
  one by one.
- A restore brings back unfinished operations that may have run after the backup.
Decisions:
A4-1 Provider authentication circuit breaker (P6b carry):
  - Trigger: `authentication_error` or `permission_error` from the provider. It is a definite
    rejection, so no inference ran.
  - The operation goes back to `accepted` with the dispatch not counted, its reservation
    released and `next_attempt_at` set to the breaker's reopening time.
  - The worker pool's shared breaker opens for 5 min, doubling to 1 h. While it is open, no
    worker claims. It closes on the next settled call that is not an authentication failure.
  - Visibility: an error log once per opening, the counter
    `memory_provider_auth_failures_total`, and the gauge `memory_provider_auth_breaker_open` in
    the metrics export.
  - Deadlines still apply: accepted operations past `expires_at` are expired by the reaper, so
    nothing waits forever.
  - The breaker is per process: each instance learns from at most one failed call per window.
  - Other terminal errors are unchanged.
A4-2 Restore without replay. A new CLI command, `memory-service admin after-restore`, is run on
  a restored database before `serve`, with the service closed to clients. In one transaction it:
  - finishes every unfinished operation (`accepted`, `claimed`, `dispatched`) as
    `cancelled(restored)`, releasing reservations and zeroing `reserved_tokens`. Whether a
    dispatched call ran after the backup is unknowable, and its output is not in the restored
    database;
  - writes one system audit row (`service.after_restore`, with the count);
  - is idempotent.
  A worker afterwards finds nothing to send, so restore or restart is never authority to replay
  inference. New inference needs a new request.
A4-3 Backup check on disposable databases.
  - An `#[ignore]` DB-backed test, `memory_operations_backup_restore_keeps_content_and_never_replays`,
    run by `scripts/backup-restore-check.sh` (it requires `MEMORY_PG_BIN` with pg_dump,
    pg_restore and psql matching the server, and refuses a zero-test run). The test:
    - seeds tenants, observations with vectors, links, sources, and operations in every state;
    - `pg_dump -Fc`s the test database, then restores it into a second disposable database;
    - runs after-restore and checks that content counts, search results and labels match, every
      unfinished operation is `cancelled(restored)`, a worker pass is idle, and the counted
      provider saw no call.
  - CI: a `memory-service.yml` step installs `postgresql-client-18` from the PGDG apt
    repository and runs the script. It is unverifiable locally (no Docker or Actions), so it is
    reported as unverified until CI runs it.
A4-4 Operations documentation (services/memory README "Operations" and the OpenBot deploy.md
  "Restoring the memory service"):
  - Credential rotation:
    - admin credentials: issue, reassign, revoke;
    - client credentials, including the P6b carry: a rotated write-up credential cannot reuse
      revisions above its read grant (the uniform 409), so the client records new revisions.
  - Provider outage: retries, `unknown` outcomes, the breaker, quota and cap.
  - Explicit recovery: `reindex-failed`, re-driving OpenBot lifecycles, and that a finished
    operation is never revived.
  - Restore:
    1. close traffic (OpenBot without `MEMORY_SERVICE_URL`, or the network blocked);
    2. restore;
    3. run `migrate` and `admin after-restore`;
    4. in OpenBot, re-drive every lifecycle settled `absent` after the backup time (SQL given),
       which re-applies deletions; a backup older than OpenBot's 30-day lifecycle retention
       cannot be reconciled this way;
    5. reopen.
  - The limits of a restore: memories saved, generated or corrected after the backup are lost
    (OpenBot does not resend settled intents, which would be a new execution under a lost
    idempotency record); other clients' deletes and corrections after the backup are undone
    (A1 F8b).
  - OpenBot test: `memory_operations_a_redriven_absent_lifecycle_deletes_a_restored_namespace_again`
    (the fake holds the namespace again; the re-driven row deletes it and settles).
A4-5 Overload and budgets:
  - Test `memory_operations_overload_is_refused_fast_and_recovers`: with `max_in_flight = 2`, 12
    concurrent saves held about 400 ms each (the `hold-` key hook). The rest answer 429
    `capacity_exceeded` with `Retry-After`; there is no 5xx; every refused save then succeeds on
    retry; `memory_requests_total{operation="unrouted",code="capacity_exceeded"}` counts them.
  - Tenant fairness and worker budgets are already covered by the `memory_generation_` fairness,
    reaper and heartbeat tests; the model RSS budget by `model-check` in CI.
  - The "agreed representative load" is not agreed anywhere in the record. An `#[ignore]`
    measurement test (4 tenants × 100 saves and 200 searches, recording p50/p95 in its output)
    is run once locally and reported. The figures inform rather than gate, and the agreed load
    and its limits remain a user decision.
A4-6 Paid-provider smoke check: its budget is still open (D12). Not run; reported as an open
  user decision, and CI makes no paid calls.
A4-7 Stack bounds: the service keeps its own `.cargo/config.toml` and runtime settings. A4 checks
  that their test-thread and process stack values move together, as the root AGENTS.md
  requires, and documents them; it changes nothing unless they diverge.
Invariants: no automatic path re-sends inference after a restore, a restart or an
  authentication failure; a breaker or after-restore never turns a definite outcome into a
  different one; every new path is bounded (breaker ≤ 1 h, request deadlines) and visible
  (metrics and logs).
Verification: the tests named above (prefix `memory_operations_`), and mutation probes for the
  breaker (counting the dispatch, or not opening) and for after-restore (skipping dispatched
  operations).
- A4 work held during the B1 review, to keep the frozen artifact unchanged:
  `memory_operations_overload_is_refused_fast_and_recovers` (A4-5) was written and passed once
  (1 passed, 4.85 s), then parked at
  /private/tmp/claude-501/-Users-mac03--codex-worktrees-60a0-openbot/6fedfb2e-40aa-486c-bd0b-051b5a3add7b/scratchpad/observability_tests.with-overload.rs.
  The worktree file was restored from the frozen tree 5ae80918, and the digest was rechecked
  (ab43fef6…0e38). Reapply after B1 and the A4 gate.

### p06-B1 corrections (review MEM-P06-B1-R1)
- F1 (fixed): `tenant_absent` treats a delete 404 as definite (`tenant_unmanageable`) only
  after a create that succeeded in the same step. After a refused create (409/404) it postpones
  through `not_ours` (`provisioner_mismatch`). The test
  `memory_scope_reconciler_foreign_managed_tenant_never_completes_deletion` now asserts the
  postponement, then the definite failure after a successful create plus a faulted delete 404.
  `cargo test --lib -- lifecycle_tests`: 47 passed.
- F2 (taken):
  - (a) `memory_lifecycle_postponed_rows{outcome}` now covers `provisioner_mismatch`,
    `tenant_disabled` and `tenant_recheck`.
  - (b) provider error types outside the documented set become `provider_error` (`outcome_kind`),
    so the outcome code and the metric label are a closed set; the anthropic test adds an
    unknown, malformed type.
  - (c) `retryable` also re-runs a transaction on `RowNotFound`, covering a source removed
    between `ON CONFLICT DO NOTHING` and the read of the conflicting row.
  - (d) tenant delete locks its namespaces before cancelling operations.
  - (e) the CI toolchain step installs `--component rustfmt,clippy`.
  - (f) not taken: OpenBot's hygiene test covers the delivery and capture paths. Recall
    requests go through the same adapter `post` (covered), recall logs carry outcomes only,
    and the recall test asserts its correlation header.
  Service: `memory_observation_anthropic memory_operations_ memory_generation_revocation`
  22 passed. OpenBot: `retention_tests` 4 passed.

## p06-A4 revision 2 (review MEM-P06-A4-R1)
- F1 (restore undoes more than deletions):
  - A4-4 step 4 becomes: re-drive **every** lifecycle row (tenant and namespace) updated after
    the backup time, in any settled state (SQL in deploy.md). This works because:
    - tenant rows re-apply disable/enable;
    - namespace rows re-apply deletions, and their present step re-runs `revoke_labelled`, so
      client credentials OpenBot revoked after the backup are revoked again.
  - New A4-8, a stored credential is verified. `namespace_present` checks that the stored
    credential id is in the namespace's live listing (the label and `active_only` pages it
    already reads). If it is missing and the listing is complete, the stored credential is
    cleared and a new one is issued and stored under the fence. A credential the restored
    service never had is therefore replaced, not trusted.
  - New A4-9, a delivery 401 on a credential OpenBot holds as live:
    - it is no longer terminal: the intent retries transiently, with no attempt spent and
      bounded by its own `expires_at`;
    - the namespace's lifecycle is re-pended with a new generation (`credential_unverified`),
      and its present step then verifies or re-issues;
    - recall and tools still answer `memory.forbidden` for that call, and trigger the same
      re-pend;
    - the re-pend is a compare-and-set: only a `settled`/`present` row whose stored credential
      id is still the one that got the 401 moves, so concurrent 401s on one namespace bump the
      generation once, and an `absent` row or a row already re-issued is left alone;
    - 403 (grant missing) stays terminal: it is a classification or scope decision, not a lost
      credential;
    - this also covers a credential revoked by hand: OpenBot replaces it. To stop OpenBot's
      access, the operator disables the tenant (or disables memory in OpenBot), not the client
      credential. This is documented in deploy.md.
  - Admin credentials, tenant settings and caps changed after the backup: the operator re-revokes
    admin credentials and re-applies `tenants/settings` before reopening. The service cannot know
    them, so operators keep those records outside the service database. This is documented.
  - The open generation intents of OpenBot are cancelled at restore (F3c). The SQL sets
    `cancelled/service_restored` and nulls the request, so a capture that may have run after the
    backup never runs a second time. Open saves are left to resend: a save is no inference, and
    resending one re-creates the memory the restore lost.
  - Tests:
    - `memory_operations_a_restored_service_without_the_stored_credential_gets_a_new_one`: the
      fake lacks the stored credential; the re-driven present row issues and stores a new one,
      and a delivery then succeeds;
    - `memory_operations_a_delivery_401_reverifies_the_namespace_credential`;
    - `memory_operations_a_redriven_absent_lifecycle_deletes_a_restored_namespace_again`.
- F2 (verification):
  - `memory_operations_auth_breaker_pauses_competing_workers_and_closes_on_success`: 3 workers
    and a provider that answers 401, then succeeds. Exactly one call per window (the half-open
    probe); `attempts` unchanged; no claim while open; the window doubles (with a test-only
    short base); it closes after a success, and every operation is then sent once.
  - `memory_operations_after_restore_finishes_every_unfinished_operation`, in the regular DB
    suite: operations in each unfinished state become `cancelled(restored)` with zero
    reservation; finished ones are untouched; a second run is a no-op; a worker pass is idle;
    one audit row. The ignored backup test runs the same function after a real dump and
    restore.
  - The 06A focused-check map (below).
- F3 (taken):
  - (a) `billing_error` joins `authentication_error` and `permission_error` as breaker triggers.
  - (b) half-open: after the window, a single worker per process holds the probe permit; the
    others treat the breaker as open until the probe settles.
  - (c) as above.

06A focused-check map (06-operations-and-verification.md line 31):

| Check | Test(s) |
| --- | --- |
| retention | `memory_operations_expired_observations_…`, `…_idempotency_records_age_out_…`, `…_audit_and_spend_rows_…`, `…_unreferenced_operations_…`, `memory_generation_retention_is_bounded_…` (P6b), OpenBot `memory_operations_settled_…` (3) |
| in-flight deletion | `memory_operations_purge_cancels_inference_then_removes_every_content_row`, `…_a_saved_settlement_without_its_credential_row_is_a_stop`, `…_a_stale_indexer_commit_…`, `memory_generation_revocation_expiry_and_deletion_stop_operations` |
| rotation | `memory_operations_reassign_…` (2), OpenBot `memory_scope_reconciler_refuses_a_changed_provisioner_credential_id`, `memory_operations_rotation_mid_deletion_…` |
| restore | `memory_operations_after_restore_…` (new), `memory_operations_backup_restore_keeps_content_and_never_replays` (ignored, script), the OpenBot restore tests above |
| quota/auth recovery | `memory_generation_spend_cap_is_terminal_and_overshoot_is_counted`, `memory_operations_auth_breaker_…` (new) |
| expired requests | `memory_operations_idempotency_records_age_out_and_keys_never_execute_twice`, `memory_generation_revocation_expiry_…` (request expiry) |
| missing model assets | `memory_startup_missing_model_fails_explicitly` (existing) and the CI `model-check` budget |
| controlled overload, budgets | `memory_operations_overload_is_refused_fast_and_recovers` (parked), the `memory_generation_` fairness and reaper tests, the CI `model-check` RSS budget |

### p06-B1 correction 2 (review MEM-P06-B1-R2)
- F3 (fixed): the F1 test holds the fake's `MutexGuard` in a block that ends before
  `due(&f).await`, so `clippy::await_holding_lock` no longer applies. Clippy result below.

## p06-A4 revision 3 (review MEM-P06-A4-R2)
- F4 (re-issue without leaving `ready`): A4-8 never clears the stored credential. When the stored
  id is missing from a complete live listing:
  - the step issues a new credential first;
  - one fenced completion then swaps `credential_id` and `credential_envelope` on the row and
    settles `present`. The UPDATE is guarded by `state = 'ready' AND credential_id = <old id>`,
    so a concurrent change refuses it and the step re-runs. The namespace stays `ready`
    throughout, so the delivery claim never settles its intents `namespace_unavailable`;
  - the old id is not revoked: the service does not have it;
  - a truncated listing (more than `MAX_CREDENTIAL_PAGES`) cannot prove the id missing, so the
    stored credential is kept and the step settles as now;
  - until the swap commits, a delivery that gets a 401 retries transiently (A4-9), and the claim
    after the swap reads the new envelope.

  The test `memory_operations_a_delivery_pending_through_a_reissue_is_delivered`:
  - a pending save intent; the fake drops the stored credential;
  - the delivery gets a 401 and stays pending without spending an attempt; the lifecycle is
    re-pended;
  - the present step issues and swaps the credential while the namespace stays `ready`;
  - the intent is then delivered with the new credential, and its text was never dropped.

  A4-8's restore test, `…_without_the_stored_credential_gets_a_new_one`, also asserts the row
  never left `ready`.
- F5 (taken, documented): the service answers 401 for every credential of a tenant deleted out
  of band. OpenBot's intents then retry transiently until their `expires_at`, and the re-pended
  present step fails `tenant_conflict`, which settings show at tenant level. That is bounded by
  the intents' deadlines and visible. deploy.md lists it under the restore and rotation edges as
  an unsupported operator action. The tenant id is the company id, so a tombstoned tenant cannot
  be recreated, and memory for that company stays failed until the service database is
  restored or reset. OpenBot offers no in-product recovery for it.
- B1-F3 check: `cargo clippy --all-targets --offline -- -D warnings` (OpenBot, SQLX_OFFLINE) exit
  0, logged in `logs/MEM-P06-B1-R2-clippy-2.txt`. Service clippy exit 0, logged in
  `logs/MEM-P06-B1-R2-clippy.txt`.

## p06-A4 implementation (gate revisions 1–3 passed, MEM-P06-A4-R3)
Service (`services/memory`):
- A4-1 and F3 (breaker):
  - `application/provider_breaker.rs` (new): `ProviderBreaker`, which is closed, open, or
    half-open with one probe permit. The window is 5 min, doubling to 1 h (`GenerationSettings`
    `breaker_base`/`breaker_max`, shortened only in tests). `AUTH_KINDS` = authentication,
    permission and billing errors. The gauge `AUTH_BREAKER_OPEN` is per process.
  - The worker (`generation_worker.rs`):
    - `Pass::Paused` when the breaker is open; a probe's permit is released when it ends without
      a provider answer;
    - a call already in flight when the breaker opened does not re-open or double it;
    - an auth-kind answer becomes `Settlement::Paused(kind, delay)`; any other provider answer
      closes the breaker (`provider_answered`).
  - `settle_once` (`postgres/generation.rs`) handles `Paused`: back to `accepted`, `attempts - 1`,
    reservation released, `next_attempt_at = now() + delay`, and the audit outcome `paused`.
    With a cancel requested it is a cancel charged 0.
  - The counter `memory_provider_auth_failures_total{kind}` and the gauge
    `memory_provider_auth_breaker_open` (`/admin/v1/metrics`).
  - `startup.rs`: one breaker shared by a process's workers.
- A4-2 `memory-service admin after-restore` (`main.rs`) runs `Store::after_restore`: in one
  transaction every unfinished operation becomes `cancelled(restored)` with its reservation
  released, and a `service.after_restore` audit row is written only when something changed
  (deviation: a no-op run writes nothing).
- A4-3:
  - `scripts/backup-restore-check.sh` checks that client and server majors match and refuses a
    run that selects no test;
  - the ignored test `memory_operations_backup_restore_keeps_content_and_never_replays`;
  - `memory-service.yml` installs `postgresql-client-18` from PGDG and runs the script.
  Locally the test ran for real (PostgreSQL 18 client tools are installed): 1 passed. The CI step
  stays unverified until CI runs it.
- A4-4: README "Operations": rotating credentials, provider outage/quota/credentials, explicit
  recovery, backup and restore with its limits, stack and memory budgets. Other README
  changes: the generation paragraph, the metrics list, `MEMORY_ANTHROPIC_API_KEY`, the
  Commands section.
- The contract doc (`GenerationState::Failed`/`Cancelled`) names `invalid_request_error`,
  pausing and `restored`. `openapi.json` is regenerated.
- A4-5:
  - The parked overload test is reapplied, unchanged.
  - The measurement `memory_operations_representative_load_is_measured` is ignored and uses the
    real model with `MEMORY_MODEL_DIR`. One release run on this machine with the real model:
    400 concurrent saves p50 0.68 ms / p95 0.89 ms; indexing 400 observations 1.77 s; 800
    searches p50 10.95 ms / p95 13.33 ms (`logs/MEM-P06-A4-load-measurement.txt`).
  - It informs and does not gate: the agreed load is still a user decision.
- A4-7: the service stack bounds are consistent (2 MiB both); documented, nothing changed.
- Changed existing test: `memory_generation_ambiguity_retries_and_terminal_errors_never_resend`
  used `authentication_error` as its terminal example, and A4-1 deliberately makes that pause.
  It now uses `invalid_request_error`; the auth case is covered by the breaker tests.

OpenBot:
- A4-8:
  - `revoke_labelled` returns whether the stored id is among the live credentials; `Ok` always
    means a complete listing, and a truncated one is still a retry.
  - `namespace_present` issues and then swaps when the stored id is missing:
    `Step::Store(credential, replaces)` and `store_credential(.., replaces)`. The UPDATE is
    guarded by `credential_id = <old>` with the state `provisioning` or `ready`, so a ready
    namespace stays ready.
- A4-9:
  - `application/memory/reverify.rs` (new): `ReverifyingClient` wraps every `MemoryClient` call
    and is composed in `infra/workflow.rs` `with_memory`. On a 401 it calls
    `CredentialReverification::credential_refused(tenant, id)`, which is implemented in
    `persistence/memory/lifecycle.rs`.
  - That call is one compare-and-set UPDATE that moves a settled `present` row of the namespace
    storing that id, with outcome `credential_unverified`.
  - `ClientCredential` gains `id`, which the delivery claim and `namespace_on` read.
  - `delivery_result`: 401 → `Retry("credential_unverified")`; 403 stays `Failed("authority")`.
  - Design detail beyond the gate text: the hook is one client wrapper, so delivery, recall,
    tools and steps share it. A multi-namespace search or pack re-verifies each of its
    namespaces, because a 401 there does not name the credential.
- A4-4: deploy.md:
  - the delivery 401 behaviour and how to stop OpenBot's access;
  - the postponed-rows gauge with all three outcomes;
  - "Restoring the memory service": the steps, the marked restore SQL, and the limits,
    including F5.
  The tests run that SQL verbatim from the doc (`restore_sql` in `restore_tests.rs`).
- Contract fake: issued client tokens now authenticate, revoked ones stop, and
  `forget_credential` simulates a restore.
- Tests in `persistence/memory/restore_tests.rs` (a child of `lifecycle_tests`):
  - `memory_operations_a_restored_service_without_the_stored_credential_gets_a_new_one`:
    - the restore SQL re-drives the after-backup namespace row and leaves the pre-backup tenant
      row alone;
    - it cancels the open capture (`service_restored`, request nulled) and keeps the open save;
    - the namespace stays `ready`, a new credential is swapped in, and the save is delivered.
  - `memory_operations_a_delivery_pending_through_a_reissue_is_delivered`:
    - 401 → pending, 0 attempts, request kept, lifecycle `credential_unverified`, still ready;
    - the reissue swaps in a new id, and the old id no longer moves anything;
    - the save is then delivered.
  - `memory_operations_a_delivery_401_reverifies_the_namespace_credential`:
    - a 403 re-pends nothing;
    - three concurrent 401s bump the generation exactly once.
    The gate's delivery-401 behaviour itself is asserted in the previous test.
  - `memory_operations_a_redriven_absent_lifecycle_deletes_a_restored_namespace_again`.

Checks (A4):
- Service:
  - `cargo test --offline --all-targets`: 174 passed, 2 ignored, the ignored ones being the
    backup test (run by its script) and the load measurement
    (`logs/MEM-P06-A4-service-suite.txt`);
  - `scripts/backup-restore-check.sh` with `MEMORY_PG_BIN=/opt/homebrew/opt/postgresql@18/bin`:
    1 passed;
  - the breaker and after-restore tests repeated 5×: all passed;
  - clippy `-D warnings` exit 0, fmt clean; `.sqlx` regenerated (prepare database
    `mem_p06_prep`).
- OpenBot:
  - `cargo test --lib -- memory`: 167 passed, 3 ignored, the ignored ones existing before
    (`logs/MEM-P06-A4-openbot-memory.txt`);
  - clippy `--all-targets -D warnings` exit 0 (`logs/MEM-P06-A4-clippy.txt`), fmt clean.
- Mutation probes, in isolated copies with their own CARGO_TARGET_DIR, deleted afterwards
  (`logs/MEM-P06-A4-mutation-*`). Every one was killed:
  - MC1, the paused dispatch counted: killed by `…auth_breaker_…`;
  - MC2, the breaker never opens: killed by `…auth_breaker_…`;
  - MC3, many half-open probes: killed by the unit test and `…auth_breaker_…`;
  - MC4, after-restore skips dispatched operations: killed by `…after_restore_…`;
  - MD1, the stored credential is trusted without the listing: killed by two restore tests;
  - MD2, a delivery 401 is terminal: killed by `…pending_through_a_reissue…`;
  - MD3, the re-pend also moves a running row: killed by `…delivery_401_reverifies…`.
  MC1 and MC4 change `query!` SQL, so they ran with live macro checks against `mem_p06_prep`.
- Not run here, as planned for 06C: the service `scripts/verify.sh`, the full OpenBot suite, the
  stack budget, the live integration script and the browser test.

## p06-B plan (06B.1–06B.4), drafted while B2 is in review
No architecture gate: 06B adds tests, scripts and harnesses, with no schema, protocol or
contract decision. Reviewed as one batch (B3).
- 06B.1 `memory_standalone_`:
  - a service-owned script `services/memory/scripts/standalone-journey.sh` extracts the tracked
    service files to a temp directory outside the repo, builds there with its own lockfile and
    `.sqlx`, provisions a throwaway database and role with `provision-db.sh`, bootstraps an
    operator and serves on loopback with a service-owned Messages API stand-in (no OpenBot file
    is used);
  - a generic stdlib-only client (`python3 -I scripts/standalone_client.py`) walks the journey
    over HTTP and MCP: provision tenant, namespace and client key; save; search; fetch; context
    pack and validate; generation through the stand-in, polled to completion; correct; delete;
    purge; tenant delete, then a `401` for the old token;
  - each step prints `memory_standalone_<step> ok`, and the script requires every step.
  Payloads are the service's own contract; nothing OpenBot-shaped.
- 06B.2 `memory_e2e_`: ignored live tests next to `memory_integration_live_`, which
  `scripts/memory-integration.sh` also runs (it requires a nonzero count for each prefix). The
  service runs under its own database role; OpenBot uses its own test database and the
  provisioner credential.
  - The level matrix: readers at 6, 8 and 9 recall only levels 0–6; a context consuming level 4
    saves at 4; one consuming level 6 saves at 6.
  - A level-3 reader cannot retrieve the level-4 save through HTTP search, fetch, timeline,
    context or MCP, or through source metadata.
  - Also: an audience downgrade; a public latest recall after private history; tampered labels
    refused; missing precise consumption under ceiling 6 saves at 6; missing trusted authority or
    level-7 content fails safely.
  - Revocation and replay: a revoked client credential re-verifies (A4-9) against the real
    service, and a replayed save keeps its receipt.
- 06B.3 topology:
  - separate processes, each with its own database and role on the one local cluster, and only
    endpoint and credential configuration changed;
  - a SQL check that the service role cannot read OpenBot's tables and the reverse;
  - the extracted build needs no sibling source (`extracted-build.sh`, `boundary-check.sh`).
  - Unavailable here: a second host and Docker, which CI's `image` job covers for the non-root
    image. Reported as unavailable.
- 06B.4 competing claimants: map the existing concurrency tests per case (saves, job claims,
  delivery claims, stale completions, deletion, revocation, recall/save accounting, dispatch and
  commit crash/timeout boundaries) and add the missing ones. Likely missing: concurrent corrects
  of one observation, a correct racing a delete, and a revocation racing an in-flight save
  commit.
Focused checks: the two scripts (nonzero step and test counts), `memory_e2e_`, and the new
competing tests. The final gate is 06C.

### p06-B2 accepted; optional MEM-P06-B2-F1 taken into B3
- (a) The breaker test asserts only from recorded call times, as lower bounds on the gaps between
  provider calls, instead of sampling at wall-clock points. A loaded runner only lengthens the
  gaps, so it cannot make the test fail.
- (b) The gauge reads the process's shared breaker, which the App holds; the static is removed.

### User decisions U8 and U9 (COORDINATION I003 r5, 2026-11-10)
- U8, load option C:
  - the ignored measurement seeds about 10,000 memories per namespace (4 tenants, indexed with
    the real model), then measures concurrent saves and searches, p50/p95;
  - it is reported in TASK and gates nothing;
  - CI's non-latency budgets stay as they are: the model RSS budget, the overload test, fairness.
- U9, paid smoke:
  - budget 50,000 tokens, set as the smoke tenant's `generation_daily_token_cap`, so the service
    refuses beyond it (`spend_cap_reached`);
  - vehicle: the 06B.1 standalone journey with the real provider, run by
    `scripts/memory-paid-smoke.sh`, which reads only `MEMORY_SMOKE_ANTHROPIC_API_KEY` from the
    git-ignored root `.env` and never prints it;
  - `.env` was created holding only that key, empty (`.env` did not exist before), and the key
    is documented in `.env.example`;
  - root asks the user to confirm before the paid run. CI never runs it.

### U8 measurement (option C), recorded
One release run on this machine, with the real model, 4 tenants, each namespace seeded with 10,000
varied memories (`logs/MEM-P06-B3-load-measurement-10k.txt`):
- seeding 40,000 memories (200 per save) took 4.9 s; indexing them took 419.5 s, about 95 memories
  per second on one indexer;
- then 400 concurrent saves: p50 1.04 ms, p95 1.79 ms; indexing those 400 took 3.33 s;
- 800 concurrent searches: p50 45.98 ms, p95 72.59 ms. At 100 per namespace they were
  10.95 / 13.33 ms, so search grows with namespace size, and a large backfill is bound by the
  indexer.
It is reported only and gates nothing (U8).

## p06-B3 implementation (06B.1–06B.4, plus B2-F1 and U8/U9)
- B2-F1:
  - (a) `CountedProvider::started()` records each call's start. The breaker test asserts only
    lower bounds: call 1 is at least `base` after call 0, and call 2 at least `2 × base` after
    call 1. It no longer samples DB state, and the base is now 200 ms. Repeated 3×: all passed.
  - (b) `App::with_breaker` and `App::generation_paused()`; the metrics gauge reads the
    process's shared breaker (`startup.rs`); `AUTH_BREAKER_OPEN` is removed. The metrics test
    asserts `memory_provider_auth_breaker_open 0`.
- 06B.1 `memory_standalone_`:
  - `services/memory/scripts/standalone-journey.sh`:
    - copies the service's own files and its model into a temp directory outside any checkout
      (it refuses a copy inside one) and runs `boundary-check.sh` there;
    - builds the copy with its own lockfile and `.sqlx` (`--locked --offline`);
    - provisions a throwaway database and role (`provision-db.sh`), bootstraps an operator and
      serves on loopback, with the Messages API stand-in;
    - drops everything on exit.
  - `scripts/standalone_client.py` is stdlib only and uses only the published contract. Its
    13 steps: provision, save (plus replay), search (a query sharing no word with the note, so
    only the semantic leg finds it), classification (a read_max-2 key finds and fetches
    nothing), fetch, timeline, context pack and validate, MCP tools/list and memory_search,
    generation polled to `completed/saved`, correct (revision 2), delete, purge, tenant delete
    (then 401).
  - The Messages API stand-in moved to `services/memory/scripts/fake-messages.py`, which
    OpenBot's `memory-integration.sh` now uses, so the service needs no sibling file.
  - CI: a "Standalone journey" step in `memory-service.yml`.
  - Run locally (`MEMORY_EXTRACT_UNTRACKED=1`, since the service is untracked before its first
    commit): 13 steps passed, and no database, role or temp directory was left
    (`logs/MEM-P06-B3-standalone-journey.txt`).
- U9: `scripts/memory-paid-smoke.sh` runs the same journey with `MEMORY_JOURNEY_PROVIDER=anthropic`
  and `MEMORY_JOURNEY_CAP=50000`, reading only `MEMORY_SMOKE_ANTHROPIC_API_KEY` from `.env`. In that
  mode the generation step accepts `saved` or `nothing_to_save`, and the journey prints the
  provider token counters. Not run yet: it waits for the user's key and confirmation.
- 06B.2 `memory_e2e_`, ignored and run by `scripts/memory-integration.sh`, which now requires a
  passing test of each prefix:
  - `memory_e2e_thread_readers_6_8_9_load_only_levels_0_to_6_from_the_real_service`
    (`workflow/memory_steps_e2e_tests.rs`):
    - real admission of a thread run with readers 6, 8 and 9, then the real `memory.load`
      handler against the real service, with 10 memories at levels 0–9 written by an
      independent client;
    - the load returns exactly 0–6; after a level-3 reader joins (the audience downgrade) it
      returns 0–3;
    - with content 4 that drop refuses the load before any read (no consumption row).
  - `memory_e2e_labels_follow_consumption_and_no_lower_reader_sees_them`:
    - agent scope: exact 4 under ceiling 6 saves 4; consumed 6 saves 6; inexact under ceiling
      6 saves 6; company scope saves 6 (U2);
    - recall at 6 never shows an independent level-7 memory (search, pack);
    - a level-3 reader learns nothing of the level-4 save, whether OpenBot's credential at
      ceiling 3 or an independent read_max-3 key. Search and pack answers never mention it, and
      fetch, timeline and MCP memory_fetch answer exactly as for an id that never existed (once
      the per-request ids are blanked). The probe does find it at ceiling 6.
  - `memory_e2e_tampered_labels_and_missing_authority_save_nothing`:
    - an intent edited after its freeze to claim consumed 7 under ceiling 6, or to drop its
      ceiling, is refused by the service, the intent fails and nothing is stored;
    - a known level-7 source under ceiling 6 is refused by OpenBot's freeze, so no intent exists.
  - `memory_e2e_a_revoked_credential_is_replaced_and_the_save_delivered_once`: the operator
    revokes OpenBot's credential; the save's 401 is transient (0 attempts, 1 transient retry);
    the reconciler swaps in a new credential for the same namespace, and the save is delivered
    at 4.
  - The "public latest recall after private history" case runs at the freeze level here (consumed
    6 saves 6). The run-level version is the fake-backed `memory_access_level_` suite (E12 table).
  - Run: `scripts/memory-integration.sh`: 6 passed, the 4 new and the 2 existing live tests
    (`logs/MEM-P06-B3-memory-integration.txt`).
- 06B.3 topology:
  - the service and OpenBot run as separately built processes, each with its own database;
    the service under its own non-superuser role, and only URL and credential configuration
    connect them;
  - `memory-integration.sh` now checks the isolation both ways. The service role querying
    OpenBot's `companies` gets permission denied, and a non-superuser OpenBot-side role cannot
    connect to the service database (permission denied); the probe role is dropped. Passed.
  - Builds need no sibling source: the standalone journey builds from the extracted copy.
  - **Unavailable here:** a second host and Docker. CI's `image` job runs the non-root image
    network-denied. The separate-host run is therefore not executed and is not claimed.
- 06B.4 competing claimants (real PostgreSQL, competing tasks), mapped:
  - saves: `memory_idempotency_concurrent_identical_saves_create_one_observation`,
    `memory_integration_a_replacement_claim_fences_the_stale_saver_and_replays_one_intent`;
  - job claims: `memory_generation_competing_claimants_keep_one_in_flight_per_tenant_fairly`,
    `memory_scope_reconciler_competing_claimants_get_distinct_rows`,
    `memory_storage_competing_indexers_claim_disjoint_rows_and_index_each_once`;
  - delivery claims: `memory_integration_delivery_fences_competing_claimants_and_bounds_attempts`,
    `memory_capture_competing_delivery_claimants_send_once`, and against the real service
    `memory_integration_live_competing_delivery_workers_deliver_each_intent_once`;
  - stale completions: `memory_generation_lease_loss_aborts_the_call_and_stale_workers_cannot_commit`,
    `memory_storage_index_fences_reject_stale_commits_and_expired_leases`,
    `memory_operations_a_stale_indexer_commit_after_removal_writes_nothing`,
    `memory_scope_reconciler_completion_racing_company_delete_is_lost`,
    `memory_integration_run_cancel_during_an_in_flight_delivery_loses_its_completion`;
  - corrections: `memory_storage_concurrent_corrections_serialize_on_the_revision`;
  - deletion: `memory_storage_delete_races_writes_without_resurrection` (also covers corrections
    and copies), `memory_operations_identical_purges_race_to_one_execution_and_one_replay`,
    `memory_authority_namespace_delete_races_grant_issue_and_leaves_no_grant`, and the
    `memory_scope_competing_*` deletes;
  - revocation: `memory_authority_admin_revocation_races_competing_admin_writes`, and the **new**
    `memory_authority_client_revocation_races_competing_saves`: 6×8 saves by a principal
    authenticated before the revoke race it; each commit is counted and audited, each refusal is
    `credential_revoked`, and the stale principal saves nothing afterwards. Repeated 3×, all
    passed; the first run committed 33 and refused 15.
  - recall/save accounting: `memory_access_level_competing_recall_and_save_freeze_preserve_the_watermark`,
    `…competing_rise_and_parked_save…`, `…competing_rise_and_tool_save…`;
  - dispatch and commit boundaries: `memory_generation_lease_loss_…`, `…shutdown_aborts_in_flight_calls_and_returns_claims`,
    `…reaper_frees_a_dispatched_slot_only_after_lease_and_grace`,
    `…reaper_racing_live_heartbeats_never_reaps_a_held_lease`, and generation_tests.rs:967 (a
    worker dying after dispatch leaves an `unknown` that is never re-sent).
- Checks (`logs/MEM-P06-B3-checks.txt`):
  - service `cargo test --offline --all-targets`: 175 passed, 2 ignored; clippy exit 0, fmt
    clean;
  - OpenBot `cargo test --lib -- memory`: 167 passed, 7 ignored (the live tests, which pass under
    the live script); clippy exit 0, fmt clean;
  - the standalone journey and the live script as above.

### Push authorization (COORDINATION I001 r2) and the integration plan
1. Wait for B's pass on B3.
2. Back up the other session's untracked organization-plan copies under tmp/. They are stale:
   every file is identical to `origin/main` or to an older `main` blob.
3. On branch `memory-service` at a6bed48a, commit this plan's paths only.
4. Merge `origin/main` (fetched 3752ea35) and resolve the conflicts. Expect `src/main.rs` (the
   startup notes list), the init migration, CI and the overlapping workflow files.
5. Run 06C, the full gates for both projects, on the merged tree.
6. Get B's review of the merge resolution and integration effects, plus the 06C evidence.
7. Re-fetch: if `main` moved again, merge again and rerun the affected checks.
8. Push `memory-service:main` as a fast-forward.
The paid smoke runs when the user sets the key. If it is still unset at push time, it is
reported as the one open item.

### p06-B3 corrections (review MEM-P06-B3-R1)
- F1 (fixed): the isolation check no longer depends on OpenBot's schema.
  - The admin creates a throwaway table `memory_isolation_probe_<suffix>` with no grants in the
    `TEST_DATABASE_URL` database (same server as the admin URL, now documented in the script
    header). The service role must get "permission denied" reading it, and cleanup drops it.
  - Rerun on a database created just before the run and never migrated
    (`open_bots_fresh_probe`): the isolation check passed both ways and all 6 live tests passed.
    No probe table was left, and the database was then dropped
    (`logs/MEM-P06-B3-R2-memory-integration-fresh-db.txt`).
- F2 (fixed):
  - (a) Real-service provenance. `memory_integration_live_provisioning_save_recall_capture_and_disable`
    now reads the generated memories through the service's own fetch with OpenBot's credential.
    Each cites exactly two distinct `source` refs at revision `1`: the captured window's input
    and output parts. Passed in the rerun above.
  - (b) Correction: no journey runs against the production image. CI's `image` job runs only
    `version`, `model-check` and a refused startup without a database. "CI's image job covers" in
    the B3 implementation record overstated this. The image journey is **unavailable** here (no
    Docker) and is carried to 06C as an unavailable check. It is not claimed.
- F3 (optional), taken:
  - (a) The race test now proves both outcomes. The revoke starts only after every writer has
    committed one save (a barrier), and each writer then saves until refused, up to 500 rounds.
    Asserted: committed ≥ 6 and refused = 6, besides committed = stored = audited. Repeated 3×,
    all passed.
  - (b) The tampered-label e2e also writes a level-7 external source into an intent after its
    freeze; the service refuses it and nothing is stored. Passed.
  - (c) deploy.md "Memory service" now requires the service's own database and role, and
    OpenBot's own role to be a non-superuser (a superuser reads every database). The probe stays
    a generic non-superuser role, which matches that requirement.
  - (d) The provider key reaches only `serve`. The journey reads
    `MEMORY_JOURNEY_ANTHROPIC_API_KEY` into an unexported variable and unsets it at once. `serve`
    alone gets `MEMORY_ANTHROPIC_API_KEY` (and the stand-in base URL) as shell assignments, never
    argv. The paid wrapper passes the renamed variable. Journey rerun: 13 steps passed, with the
    stand-in counters printed (`logs/MEM-P06-B3-R2-standalone-journey.txt`).
  - (e) Recorded: automatic recall is exercised against the real service only through its client
    calls (search, context pack and validate, in the live tests). The recall step itself, with
    budgets, omissions and the frozen preparation, is covered by the fake-backed
    `memory_recall_` and `memory_preparation_` suites.
- Checks: service and OpenBot clippy `-D warnings` exit 0, fmt clean
  (`logs/MEM-P06-B3-R2-checks.txt`); the live script and the journey as above; the race test 3×.
