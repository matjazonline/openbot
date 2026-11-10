# p06 agent review notes

Reviewer B: claude-opus-5-5 / xhigh (abdd866fb428eea05), continuing from p05. These are the reviewer's working notes;
findings are in agent-review-findings.md, and outcomes are in REVIEW.md.

## MEM-P06-A1-R1 — architecture gate p06-A1 (revision 1): CHANGES REQUIRED

- 2026-11-10; workflow `implement-opus-reviewed-2_2`; stage architecture gate.
  - HEAD a6bed48a. Digest 7ce18038c31f60d29ebd6570f3b65c2868b4dc80cfc1f7ca7d10dc295cbe21e6, using TASK.md's snapshot
    command, which excludes the p05 and p06 docs and COORDINATION.md.
  - Gate text: TASK.md sha256 59505897…bead4, the A1, A3 and A2 sections.
  - Coordination r4 (I001–I003). No database used. 22 GiB free.
- Read: the originals and the code.
  - 06A.1, 06A.2 and the completion criteria.
  - implementation-decisions D3, D5, D9 retention, D11/D12, D13, P6b retention and n2, the E1 managed_by condition, the
    E1 06A conditions and r2-m4.
  - Service schema (0001_init.sql, whole).
  - mod.rs: `lock_actor`, `lock_tenant`, `cancel_operations`, `change_tenant_once`, `create_tenant`,
    `create_namespace`, `delete_namespace`, `list_credentials`, `revoke_credential`, `revoke_admin_credential`.
  - memory.rs: `lock_writer`, `claim`, `replay_authority`, `finish`, `resolve`.
  - generation.rs: `recheck_authority`, the dispatch recheck, `settle_once`, `save_output`, `reap_once`,
    `sweep_once`, `renew_generation`.
  - search.rs: `commit_index_once`, `claim_index_jobs`.
  - service.rs: client and admin dispatch, `label`.
  - contract.rs: purge.
  - OpenBot lifecycle.rs:440-540.
- What holds.
  - Lock order.
    - Every writer, settlement (for `Saved`), indexer commit, credential issue and namespace delete takes the
      tenant `FOR SHARE` before the namespace, so A1-1's tenant `FOR NO KEY UPDATE` excludes them before it
      tombstones namespaces.
    - The generation claim skips locked rows, and the indexer claim locks only observations.
    - Purge's order matches `lock_writer` (namespace before credential), so revoke and purge cannot cycle.
  - Cancellation before cleanup is sound.
    - Accepted and claimed operations are cancelled.
    - Dispatched ones are flagged. `renew_generation` refuses a flagged lease (generation.rs:1460-1466), and
      `settle_once` discards the output of a flagged operation. Otherwise the reaper settles it after lease + 30 s.
  - The removal is closed within one namespace: sources only from the target namespace (memory.rs:416),
    operation-source rows and generated observations in the same namespace. Only parent links cross
    namespaces, and those cascade.
  - n2 (A1-3) is correct. `recheck_authority` returns `None` only for a missing row on the non-skip path, and
    dispatch already fails closed on `None` (generation.rs:783-792).
  - The idempotency sweep matches D5. Mapping the operations' `UNIQUE (tenant_id, idempotency_key)` violation to
    409 keeps one key to at most one operation while that operation exists.
  - Reassignment.
    - `may_manage` makes `managed_by` the only authority binding: `disabled_by` is checked by scope, and
      `issued_by` is not an authority.
    - The `to` row is locked `FOR SHARE` against a racing revoke.
  - The replay restriction to the same credential is no existence oracle, because refusals are a uniform 403.
- Findings.
  - F1 (Minor): the marking statements lock a parent after its children; the invariant says they never do.
  - F2 (Minor): no competing tests for an identical purge or for reassign against an in-flight request, and the
    replay rule's trigger misses the in-transaction refusal.
  - F3 (Minor): the limits of deletion omit cross-namespace copies and restore from backup.
  - F4 (Minor): free-text credential labels are kept forever.
  - F5 (optional): operation and source metadata growth.
  - F6 (optional): `remaining` type, `from = to`, admin row order, label index and N+1 grants.
- 06A.1/06A.2 coverage beyond the findings.
  - Every 06A.1 category has a decision: canonical memories, raw evidence, generation inputs, idempotency
    results, audit records and deletion barriers. Credentials (F4) and operation metadata (F5) are missing.
  - 06A.2 "pending deliveries" is OpenBot's (p05 E9 settles them at delete and disable; A3-1 now sweeps them).
  - "Client snapshots" are covered by resume revalidation (B4 R2), which A1-7 cites.
- Outcome: **CHANGES REQUIRED** for p06-A1 revision 1: Minor F1–F4, optional F5 and F6, no Major. The decisions
  are right in shape (tombstones plus a deferred sweep, purge as delete plus audit, the n2 stop, retention
  windows, reassignment, the paged listing), so dependent A1 code can follow once F1–F4 are settled in the gate
  text.

## MEM-P06-A3-R1 — architecture gate p06-A3 (revision 1): CHANGES REQUIRED

- 2026-11-10. Same HEAD, digest and gate text as A1-R1.
- Read: E8's settled-intent line (implementation-decisions.md:1988), the E1 06A conditions (1902-1912), and
  persistence AGENTS.md:100-104.
  - Schema: memory_lifecycles and memory_deliveries (14980-15087), the effect-wait wake (187-194),
    `workflow_control_retry_safe` (2909-2944), `stamp_workflow_run_finished`.
  - Code: `park_on` (key `ob.s.<execution>`), the tool-checkpoint key `ob.t.<invocation>`, `control_retry::retry_on`,
    the admission deadline (24 h), the lifecycle claim and `tenant_ready`, `settle_lifecycle`.
  - application/memory/lifecycle.rs: `pass`, `step`, the tenant steps, `namespace_absent`, `revoke_labelled`,
    `retry`, `RetryBudget::of`.
  - The storage sweep pattern (storage.rs).
- What holds.
  - Waits read intents only for live runs; the wake requires `run.state='waiting'`.
  - The only readers of `memory_deliveries` are the step, polling and failure text.
  - A settled-absent namespace row has already deleted its `memory_namespaces` row (`settle_lifecycle`).
  - With A3-5, every settled-absent namespace rests on a remote tombstone: a namespace or tenant 409, or a delete
    200. That makes A3-2's "removing the row loses no barrier" true, and AGENTS.md's quiescence rule is met by
    waiting instead of accepting the 404.
  - `tenant_absent` already completes only on a delete 200.
  - The probe answers are as stated: `tenants/create` answers 200 for live tenants we manage and 409 for
    tombstones or other managers.
  - A3-6 is consistent with A1-6.
- Findings.
  - F1 (Minor): `failed` runs can be retried. The predicate relies on run deadlines (24 h) being far below 7 days
    and does not say so.
  - F2 (Minor): the documented rotation order (reassign before switching the token) fails every affected lifecycle
    pass definitively. The reverse order is absorbed by A3-4, except for new namespaces created in the window.
  - F3 (optional): full-table gauges, the sweep order against its index, EXPLAIN on small tables.
  - F4 (optional): document waits on a disabled tenant, an operator token as provisioner, probe-created tenants,
    and retrying failed rows.
- Outcome: **CHANGES REQUIRED** for p06-A3 revision 1: Minor F1 and F2, optional F3 and F4, no Major. A3-4 and
  A3-6 also wait on A1-5 and A1-6 (A1 open).

## MEM-P06-A2-R1 — architecture gate p06-A2 (revision 1): CHANGES REQUIRED

- 2026-11-10. Same HEAD, digest and gate text.
- Read: 06A.3, and the code below.
  - Service: http/mod.rs (`handle`, `limit`, `mcp_gate`), mcp/mod.rs `call_tool`, the in-process counters
    (search.rs:36-42, generation.rs:41-48), `renew_generation`, the schema's immutability trigger, and
    service.rs (admin principal: a client on the admin surface gets 401).
  - OpenBot: monitoring.rs (port 60-72), routes/monitoring.rs (operator-only), and the memory port read methods
    (ports.rs:394-438).
- What holds.
  - Every 06A.3 signal has a metric: readiness; queue age and depth; indexing failures; retries;
    terminal/unknown generation; lease loss; tenant fairness; provider usage; context omissions.
  - Labels come from closed sets.
  - The scrape is read-only under a 2 s statement timeout and degrades to `memory_metrics_query_ok 0`.
  - A correlation id is never authority.
  - Operator-only access matches OpenBot's `/metrics`.
- Findings.
  - F1 (Minor): the hygiene test needs a positive control, and a refused request whose marker serde would quote.
  - F2 (Minor): no affected-contract list, and three gaps: MCP correlation, immutability of the operation's
    `correlation_id`, and the requests counter and `x-request-id` for capacity refusals.
  - F3 (optional): per-process counters against database-wide gauges.
  - F4 (optional): lease-loss reasons.
  - F5 (optional): "one id" is a join path.
- Outcome: **CHANGES REQUIRED** for p06-A2 revision 1: Minor F1 and F2, optional F3–F5, no Major. A2-4's lifecycle
  gauges depend on A3-3.
- Return-time freshness (all three rounds): HEAD a6bed48a.
  - Digest recomputed: 7ce18038…21e6, MATCH.
  - TASK.md sha256 unchanged (59505897…bead4).
  - No database and no mutation copy used. Only these three record files were written.

## MEM-P06-A1-R2 — recheck of p06-A1 revision 2: CHANGES REQUIRED (F7)

- 2026-11-10; stage gate recheck by finding ID.
  - HEAD a6bed48a. Digest 7ce18038…21e6 recomputed: MATCH; the code is unchanged.
  - Gate text: TASK.md sha256 2c053242cc3c81b5…, with revision-2 subsections at 184, 326 and 436.
  - The coordinator says the revision-1 text changed only at line ~155 and in "Next action". I could not
    diff this (no copy of revision 1 was kept). The corrected invariant (155-157) reads as described.
- Per finding.
  - F1: FIXED. The marks are separate transactions that lock only the parent; the content steps hold no
    parent lock.
  - F2: FIXED. Purge claims before re-checking state and grant, so a waiting identical purge replays.
    Three tests on separate connections are named.
  - F3: FIXED. The copy decision is verified against implementation-decisions.md:763-768. The restore
    reasoning corrects my R1 suggestion: the restored audit cannot list later deletions.
  - F4: FIXED, apart from the mark predicate (F7).
  - F5: TAKEN.
  - F6: TAKEN.
- F5, checked hardest (removal of unreferenced finished operations and nulled sources).
  - What still references them after the change:
    - generated observations (FK) and links (FK `(source_id, source_level)`);
    - op-sources;
    - idempotency results, which are gone after 7 days, before the 30-day threshold.
  - Races.
    - Save resolves cited sources `FOR SHARE` (memory.rs:416-427), and acceptance locks the window's
      sources `FOR SHARE`.
    - The sweep locks candidates `FOR UPDATE SKIP LOCKED` and re-checks references in the delete. A
      racing citation therefore either keeps the row or finds it gone (`not_found`, or a fresh record
      at acceptance).
    - A unique `(ns, ref, revision)` insert waits for an uncommitted delete.
  - Immutability. It now holds only while something references the revision. That is not an access
    control property: a client can always record the same text under a new revision at any level within
    its `write_max`.
    - The rotated-credential 409 for others' revisions above the read grant becomes a 200 fresh row
      once the revision is removed. That reveals nothing new, because existence already showed as 409
      versus 200.
    - OpenBot's per-exchange refs are never re-recorded.
  - Result: acceptable as documented, provided the D9 amendment is recorded (F8c).
- New findings.
  - F7 (Minor): the tenant mark's `NOT EXISTS` cannot cover the kept credentials. A tenant marked after a
    partial label blanking (more than 500 credentials) is no longer a candidate, and its remaining
    labels stay.
  - F8 (optional): (a) a concurrent identical purge by another credential would replay through D5's
    `replay_authority`, while the text says it is refused; (b) the restore procedure should note
    other clients' deletes and corrections; (c) record the D9 amendment.
- Outcome: **CHANGES REQUIRED** for p06-A1 revision 2, on F7 (Minor) only. It is a one-clause fix to the
  tenant mark plus one test. Everything else passes.

## MEM-P06-A3-R2 — recheck of p06-A3 revision 2: CHANGES REQUIRED (F5)

- 2026-11-10; same HEAD, digest and gate text.
- Per finding.
  - F1: FIXED. The kept-run test is `run.deadline < now()` with a terminal state, or a retired run.
    Tests on both sides of the deadline and a mutation probe are named.
  - F2: FIXED.
  - F3: TAKEN.
  - F4: TAKEN.
- F2's new postponement rule, checked hardest.
  - Every "not ours" answer now postpones `provisioner_mismatch` without spending an attempt.
  - It never completes anything on a 404, which is the E1 condition. It covers namespaces created under
    the new id in the window.
  - It makes a rolling restart harmless. Old-token processes may adopt new-id rows back to the old id
    before the reassignment, and they settle once every process uses the new token.
  - The cost: a genuinely foreign tenant now waits indefinitely instead of failing definitely. That is
    visible through the new `memory_lifecycle_postponed_rows{outcome}` gauge and the settings
    mismatch. Acceptable.
  - The postponement reuses the code `provisioner_mismatch` even when the recorded and configured ids
    agree, as for a namespace-present 404. That is a naming nit only.
- New finding.
  - F5 (Minor): the adoption probe `tenants/create` answers 409 for every tombstone, managed by us or
    not (mod.rs:904-905), even after A1-5 moves tombstones.
  - Rows of a deletion still in progress at rotation are recorded under the old id. They are postponed
    every 5 minutes forever, where without rotation they would settle absent on the next pass.
  - Fix: probe with `admin.credentials.list` (`lock_tenant` without `require_active`), falling back to
    create on 404.
- Outcome: **CHANGES REQUIRED** for p06-A3 revision 2, on F5 (Minor) only.

## MEM-P06-A2-R2 — recheck of p06-A2 revision 2: PASS

- 2026-11-10; same HEAD, digest and gate text.
- Per finding.
  - F1: FIXED. A process-wide subscriber or an inline current-thread worker; a positive control with the
    correlation id on request, operation and settle lines; quoted-serde refusals.
  - F2: FIXED. Contracts listed, including the `MemoryClient` `Correlation` argument and fake headers.
    `/mcp` stateless mode is confirmed (http/mod.rs:205-206). `correlation_id` is frozen by the trigger.
    Capacity refusals carry `x-request-id`.
  - F3: TAKEN.
  - F4: TAKEN.
  - F5: TAKEN.
- New: F6 (optional). The capacity label `code="unavailable"` does not match the response's
  `capacity_exceeded` (429).
- Outcome: **PASS** for p06-A2 revision 2 (gate text TASK.md sha256 2c053242…, digest 7ce18038…21e6).
  Optional: F6. A2-4's lifecycle gauges still come from A3-3's task.
- Return-time freshness (all three rounds): HEAD a6bed48a.
  - Digest recomputed: 7ce18038…21e6, MATCH.
  - TASK.md sha256 unchanged at 2c053242….
  - No database and no mutation copy used.

## MEM-P06-A1-R3 — recheck of p06-A1 revision 3: PASS

- 2026-11-10; stage gate recheck by finding ID.
  - HEAD a6bed48a. Digest 7ce18038…21e6 recomputed: MATCH; the code is unchanged.
  - Gate text: TASK.md sha256 b5c3606b5c77c1d8…, 514 lines, with revision-3 subsections at 244, 386 and 511.
  - The file grew by 44 lines since R2 (470 lines): 23 + 16 + 4 in the new subsections and 1 in "Next action".
    That is consistent with "only those subsections and Next action changed".
- F7: FIXED.
  - Blanking candidates are keyed on the tenant's `deleted` state, so a tenant's labels keep being blanked after
    it is marked.
  - The mark also requires "no credential with `label <> 'removed'`". Either change alone closes the gap.
  - The test (1,201 credentials, three ticks, mark after the last) exercises the batch boundary.
  - Blanking before the namespaces are removed is harmless: a deleted tenant's credentials already answer
    `credential_revoked`, and OpenBot's absent path finds nothing to revoke (tenant 409 → revoke by label →
    none).
- F8: TAKEN.
  - (a) Refusal, with a race test. The code `not_found` contradicts "as a fresh execution would be": a fresh
    ungranted purge is 403 `forbidden` (D3, `denied`). That is new optional F9; it is no practical oracle,
    because exploiting it needs the other credential's exact key and deadline.
  - (b) Carried to A4.
  - (c) D9 amendment recorded.
- Outcome: **PASS for p06-A1** (revisions 1–3), gate text TASK.md sha256 b5c3606b…, digest 7ce18038…21e6.
  Optional: F9. All earlier A1 findings are resolved: F1–F4 and F7 fixed; F5, F6 and F8 taken.

## MEM-P06-A3-R3 — recheck of p06-A3 revision 3: PASS

- 2026-11-10; same HEAD, digest and gate text.
- F5: FIXED.
  - `manages_tenant` calls `credentials/list {limit: 1}` (A1-6) first. `list_credentials` runs `lock_actor`,
    then `lock_tenant` (Share) with `may_manage` and no `require_active` (mod.rs:1134-1166). So a 200 proves
    management in any state, including tombstones and tenants with no credentials; a revoked configured
    credential takes the outage path.
  - Only on a 404 does it fall back to `tenants/create`, where a 200 is proof, including a tenant the probe
    created. A 409 or 404 then postpones `provisioner_mismatch`.
  - A3-5's `namespace_absent` uses the same probe. A deleted tenant we manage still completes directly
    (namespace create 409). A reassign between the probe and the next call is absorbed by the postponement
    rule.
  - The rotation-mid-deletion test is named.
- Outcome: **PASS for p06-A3** (revisions 1–3), same gate text and digest. All earlier A3 findings are
  resolved: F1, F2 and F5 fixed; F3 and F4 taken. Dependent A3-4 and A3-6 code can follow A1's contracts,
  which passed in this round.

## MEM-P06-A2-R3 — confirmation of p06-A2 revision 3 (optional F6): PASS stands

- 2026-11-10; same HEAD, digest and gate text.
- F6: TAKEN. The capacity refusals' label becomes `code="capacity_exceeded"`, matching the 429 response
  (`ErrorCode::CapacityExceeded`, `snake_case`).
- Nothing else in A2 changed, so this reopens nothing. **PASS for p06-A2** stands, now at revision 3 (gate text
  TASK.md sha256 b5c3606b…, digest 7ce18038…21e6). No open A2 findings.
- Return-time freshness (all three rounds): HEAD a6bed48a.
  - Digest recomputed: 7ce18038…21e6, MATCH.
  - TASK.md sha256 b5c3606b…, unchanged during the review.
  - No database and no mutation copy used.

## MEM-P06-B1-R1 — batch review of p06-B1 (A1, A3, A2 implementations, B6 carries, F16): CHANGES REQUIRED (F1)

- 2026-11-10; stage batch review.
  - HEAD a6bed48a. Digest ab43fef6…0e38 recomputed at start: MATCH.
  - The frozen tree 5ae80918 equals the working tree; files were spot-checked by hash.
  - The incremental diff file's sha256 f0015ce2… matches.
  - The coordination change (two rows: p05 accepted, p06 started) is consistent with r4.
  - Free disk: 15 GiB at start and 11 GiB after building, below the 20 GiB floor. So **no mutation probes of
    my own**; the author's MA1–MA4 and MB1–MB4 are recorded as killed.
- Reviewed (frozen tree 5ae80918 against 7388d1c2).
  - Service:
    - retention.rs, whole;
    - the mod.rs diff: tombstone, tenant delete, reassign, paged listing, metrics gauges;
    - memory.rs purge (`purge_once`, `purge_replay`, the lock split);
    - application `memory.rs` purge dispatch;
    - generation.rs: `authority_missing`, the key constraint mapping, correlation, counters, lease-loss
      reasons;
    - http/mod.rs: correlation, `x-request-id`, the request line, metrics;
    - metrics.rs;
    - observability_tests (the hygiene test);
    - service.rs reassign validation.
  - OpenBot:
    - lifecycle.rs: `pass` adoption, `manages_tenant`, `not_ours`, the namespace paths, `revoke_labelled`;
    - persistence lifecycle.rs: `adopt_provisioner`, CLAIM and EXPIRE_LEASES;
    - retention.rs;
    - maintenance.rs;
    - delivery and ports correlation and metrics;
    - the contract fake;
    - the capture hygiene e2e test;
    - the rotation docs.
  - The B6 carries (script and CI).
- Challenged points.
  - Removal lock order.
    - Content steps lock content rows only (`FOR UPDATE … SKIP LOCKED`). Marks lock only parent rows and
      re-check NOT EXISTS under the lock. Label blanking is keyed on the tenant's state.
    - Every content predicate is monotone: a deleted namespace admits no writer, acceptance, save or
      indexer commit, and finished operations never gain observations. So no second look is needed.
    - The age sweeps re-check expiry or references in the delete, apart from operations, which are
      monotone.
    - The reaper runs before removal in the tick.
    - The tenant delete order nit is F2(d).
  - Resurrection.
    - The indexer commit is `Stale` for a deleted row and bumps for a deleted namespace.
    - Settles cancel on the flag or the namespace stop; `None` is now `authority_missing`.
    - A copy citing a purged parent is `not_found` (namespace check).
    - The source removal race with a citing save is closed by `FOR UPDATE` against the save's
      `FOR SHARE`/KEY SHARE and the re-check. A re-record race gives only a transient error (F2c).
  - Purge replay.
    - The claim comes after D13's locks and before the state and grant re-check.
    - The same credential replays, both concurrently and after commit (`purge_replay` on a pre-lock 403,
      same credential and hash only).
    - Another credential gets 403, so the A1-F9 suggestion is taken.
    - The in-transaction refusal rolls the inserted record back.
  - Adoption and "not ours".
    - `manages_tenant` calls list first, and create only on its 404.
    - `adopt_provisioner` locks `memory_tenants` and then lifecycle rows in id order, the same order as the
      tenant completion (`lock_resource` → `fence_on`). Namespace completions take no `memory_tenants`
      lock, so there is no cycle.
    - The fence is unchanged, so the step after adoption completes normally.
    - Namespace 404s on the present and absent paths and the present-path listing 404 postpone.
      `tenant_absent` deviates (F1).
  - Retention predicates against real readers.
    - The effect-wait wake (schema 187-194) needs `run.state='waiting'`.
    - `retry_on` needs `owner.deadline > now`, and the predicate removes a save only when its run is gone,
      or terminal with `deadline < now`.
    - The only readers of deliveries are the step, polling and failure text.
    - The lifecycle owner checks match `settle_lifecycle` (absent deletes the `memory_namespaces` row).
  - The fake's fidelity.
    - Listing, label, `active_only`, `next_after` on a full page, the tombstone 409 at create, and reassign
      moving any state all match the service.
    - Gaps that do not matter to safety: the fake does not tombstone namespaces at tenant delete or blank
      labels. OpenBot's absent path answers the same either way (create 409; revoking strays on a deleted
      tenant is harmless).
  - Hygiene positive control.
    - Service: a process-wide subscriber. The test requires its own request, pack, generation-request and
      worker-settle lines, each carrying its unique correlation id, before asserting the marker and the
      tokens are absent. A failed install leaves the capture empty, so the test fails.
    - OpenBot: a current-thread runtime, `set_default` and `rebuild_interest_cache`. It requires the capture
      and delivery lines (the delivery line carrying the key); recall is not covered (F2f).
  - Deviations.
    - Reassign `409 conflict` with `invalid_target`: acceptable.
    - No correct-versus-expiry test: verified that `CorrectRequest` has no `expires_at`.
    - The task-local correlation: the scope wraps the future, and joins run on the calling task.
    - Capture skips as a log line, and the fairness gauge as the largest queue: both acceptable.
  - B6 carries: socket-style URLs refused with the query carried over (script lines 20-26); the token passes
    through `-H @-` on stdin; the toolchain step is F2(e).
  - F16: the 2 s lock timeout makes the completion `Lost`, and the stop token is checked before a claim.
    Both tests pass in my run.
- Reviewer evidence. Private cluster 127.0.0.1:55432, DATABASE_URL unset, SQLX_OFFLINE, no `.env`.
  - Service full suite (`cargo test --locked --offline --all-targets`): 170 passed, 0 failed
    (logs/MEM-P06-B1-R1-B-service-suite.txt).
  - OpenBot `cargo test --locked --lib -- memory_` on my own fresh DB `open_bots_b1r1_test`: 163 passed,
    0 failed, 3 ignored (the live tests) (logs/MEM-P06-B1-R1-B-openbot-memory.txt). The database was dropped
    afterwards.
  - Not run: mutation probes (disk); scripts/verify.sh; the full OpenBot suite; stack budget; the live
    script; the browser test (06C).
- Findings: F1 (Minor); F2 (optional, a–f).
- Outcome: **CHANGES REQUIRED** for p06-B1, on F1 only (Minor). Everything else conforms to the passed gates.
- Return-time freshness:
  - Digest recomputed: ab43fef6…0e38, MATCH.
  - My DB dropped; no copies created.

## MEM-P06-A4-R1 — architecture gate p06-A4 (revision 1): CHANGES REQUIRED

- 2026-11-10. HEAD a6bed48a, digest ab43fef6…0e38 (unchanged), TASK.md sha256 149b2087…. Reviewed after B1, as
  asked.
- Read:
  - 06A.4, 06A.5 and the focused checks (06-operations-and-verification.md:25-33);
  - the P6b carries (implementation-decisions.md:1130-1142) and D9's worker settlement (1055-1066);
  - `ProviderError`, `classify` (`RETRYABLE` = rate limit, overloaded, api error);
  - the dispatch CAS (`attempts + 1` before the send), settle and reap;
  - the stack settings: `.cargo/config.toml` `RUST_MIN_STACK` = 2 MiB = `DEFAULT_RUNTIME_THREAD_STACK_BYTES`;
    the config bounds pin min = default = max; config.rs:405-408 asserts the equality. So A4-7 holds as
    stated;
  - OpenBot's lifecycle present path and its 401 handling.
- What holds.
  - The breaker returns definite, no-inference rejections to `accepted`, bounded by `expires_at` and a window
    of at most 1 h, and visible.
  - after-restore cancels every unfinished state, so restore is never authority to replay inference.
  - The backup check uses disposable databases with matching client binaries and refuses a zero-test run.
  - The overload test is fast-refusal with recovery.
  - The representative load and the paid smoke check are correctly reported as open user decisions, not
    passes.
  - The non-root, network-denied image and the model RSS budget are already in CI.
- Findings.
  - F1 (Minor): the restore re-drive covers absent rows only. Credentials issued after the backup break
    every client call: a 401 becomes `failed(authority)` and drops queued intents. Revocations, admin
    included, and disables after the backup are undone.
  - F2 (Minor): no named breaker test (competing workers) and no after-restore test in the regular suite;
    the 06A focused-check map is missing.
  - F3 (optional): `billing_error` in the breaker; a single probe when half-open; re-sent open intents as a
    restore limit.
- Outcome: **CHANGES REQUIRED** for p06-A4 revision 1: Minor F1 and F2, optional F3, no Major.
- Return-time freshness:
  - HEAD a6bed48a.
  - Digest recomputed: ab43fef6…0e38, MATCH.
  - TASK.md sha256 149b2087… unchanged during the review.

## MEM-P06-B1-R2 — correction recheck of p06-B1 (F1, F2a–f): CHANGES REQUIRED (F3, clippy)

- 2026-11-10; stage correction recheck by finding ID.
  - HEAD a6bed48a. Digest 541b35ad…9351 recomputed at start: MATCH.
  - Freeze tree 757148d9 is a tree object. The incremental diff's sha256 (ac88cbd6…) matches.
  - The change from 5ae80918 is confined to the 8 files named.
  - Coordination unchanged. 20–25 GiB free.
- Per finding.
  - F1: FIXED (`created` flag; a 404 after a refused create goes through `not_ours`). The test now asserts
    both branches.
  - F2: (a) TAKEN; (b) TAKEN (`outcome_kind`, plus a malformed-type test); (c) TAKEN; (d) TAKEN; (e) TAKEN;
    (f) NOT TAKEN, reason accepted.
  - F2(c) scope: `RowNotFound` is now retryable everywhere `with_retry` runs. That is safe, because every such
    closure re-runs whole transactions, and a persistent miss still ends as the same 503.
  - The idempotency-claim analogue (a record swept between `DO NOTHING` and the read) re-runs into a fresh
    claim that answers 410 for the old deadline, as D5 says.
- New: F3 (Minor). The author's clippy run fails `await_holding_lock` at lifecycle_tests.rs:331, the F1
  test's `MutexGuard` dropped by hand before an await. The author acknowledged it.
- Reviewer evidence. Private cluster; DATABASE_URL unset; my own DB `open_bots_b1r2_test`, dropped afterwards.
  - Service `cargo test --lib -- memory_observation_anthropic memory_operations_ memory_generation_`:
    38 passed (logs/MEM-P06-B1-R2-B-service-focused.txt).
  - OpenBot `cargo test --lib -- lifecycle_tests retention_tests memory_scope_ memory_operations_`: 82 passed
    (logs/MEM-P06-B1-R2-B-openbot-focused.txt).
  - My test build does not run clippy. I rely on the author's log for F3.
- Outcome: **CHANGES REQUIRED** for p06-B1, on F3 only (scope the guard and rerun clippy). F1 and F2 are
  resolved.

## MEM-P06-A4-R2 — recheck of p06-A4 decision revision 2: CHANGES REQUIRED (F4)

- 2026-11-10; same HEAD and digest. Gate text: TASK.md "p06-A4 revision 2".
- Per finding.
  - F1: RESOLVED IN SUBSTANCE.
    - Every lifecycle row updated since the backup is re-driven.
    - A4-8 verifies the stored credential.
    - A4-9 makes a delivery 401 transient and re-pends with compare-and-set; 403 stays terminal.
    - Admin re-revocation and settings are documented as operator records.
    - Open generation intents are cancelled `service_restored`.
    - The three OpenBot restore tests are named.
  - F2: FIXED (competing breaker test, after-restore in the regular suite, focused-check map).
  - F3: TAKEN.
- Checked hardest: A4-8 together with the delivery claim.
  - `memory_namespaces` cannot be `ready` without a credential. The claim fails a non-ready namespace's intent
    `namespace_unavailable`, terminally and with its text dropped.
  - Clearing before re-issuing therefore loses exactly the intents A4-9 keeps alive → new F4 (Minor): issue
    first and swap in one fenced completion, or make the claim wait.
  - The 401 also covers a tenant deleted out of band (the uniform `credential_revoked`) → new F5 (optional,
    document).
- Outcome: **CHANGES REQUIRED** for p06-A4 revision 2, on F4 (Minor). Optional: F5.
- Return-time freshness: HEAD a6bed48a; digest recomputed 541b35ad…9351, MATCH.

## MEM-P06-B1-R3 — recheck of p06-B1 F3: PASS

- 2026-11-10. HEAD a6bed48a. Digest 7e5ee98d…9b51 recomputed: MATCH. Freeze tree 5f080b3b is a tree object, and
  the diff's sha256 (641a1b5c…) matches.
- The only change since 757148d9 is `src/adapters/persistence/memory/lifecycle_tests.rs`: the guard is scoped in a
  block before the await.
- F3: FIXED. My `cargo clippy --locked --all-targets --offline -- -D warnings` (SQLX_OFFLINE) exits 0
  (logs/MEM-P06-B1-R3-B-clippy.txt), as in the author's clippy-2 log. Service clippy: the author's log, exit 0.
- Outcome: **PASS for digest 7e5ee98d00640cb0cb575e3815488266c7f708859bc8ad60fba77a6fa9729b51 (freeze tree 5f080b3b)
  for the whole batch p06-B1 (R1–R3)**.
  - F1 and F3 fixed. F2 (a)–(e) taken; (f) not taken, with an accepted reason.
  - No open B1 findings.
  - Not run (06C): `scripts/verify.sh`, the full OpenBot suite and the stack budget, the live script, the browser
    test, and my own mutation probes (disk was below 20 GiB in R1).

## MEM-P06-A4-R3 — recheck of p06-A4 decision revision 3 (F4, F5): PASS

- 2026-11-10. Same HEAD, digest and tree. Gate text: TASK.md "p06-A4 revision 3".
- F4: FIXED.
  - The credential is issued first, then swapped by one fenced UPDATE guarded by `ready` and the old id. The
    namespace never leaves `ready`.
  - A refused swap or a lost fence leaves a labelled stray for the next pass to revoke.
  - A truncated listing keeps the credential.
  - A 401 before the swap stays transient, and its compare-and-set does nothing after the swap.
  - Named test: `memory_operations_a_delivery_pending_through_a_reissue_is_delivered`. The restore test also
    asserts `ready` throughout.
- F5: TAKEN (documented as an unsupported out-of-band action; bounded and visible).
- Outcome: **PASS for p06-A4 (decision revisions 1–3)**, digest 7e5ee98d…9b51. No open A4 findings.
- Return-time freshness: HEAD a6bed48a; digest recomputed 7e5ee98d…9b51, MATCH.

## MEM-P06-B2-R1 — batch review of p06-B2 (A4 implementation): PASS

- 2026-11-10; stage batch review.
  - HEAD a6bed48a. Digest a83ffe84…afd1 recomputed at start: MATCH.
  - Freeze tree fb435320 is a tree object. The incremental diff's sha256 (aff497d0…) matches.
  - The change from 5f080b3b covers 33 files outside `.sqlx` and the phase docs.
  - Coordination unchanged.
  - 20–24 GiB free. No mutation probes of my own: the margin was at the floor, and the author's MC1–MC4 and
    MD1–MD3 are recorded as killed.
- Reviewed against the A4 gate revisions 1–3.
  - Service:
    - provider_breaker.rs (states, the single probe permit, an in-flight trip changing nothing, doubling
      bounded at 1 h, a release without an answer);
    - the worker's pass (`Paused` when open, the probe permit released after the pass, auth kinds →
      `Settlement::Paused`, any other provider answer closes);
    - settle `Paused` (back to `accepted`, `attempts - 1`, reservation released, `next_attempt_at` = the
      window, audit `paused`; with a cancel it is a cancel charged 0; expiry is left to the reaper);
    - after-restore (every unfinished operation locked in order, `finish_operation(cancelled, restored)`
      with the reservation released and nothing charged, audit only when something changed, idempotent);
    - the CLI;
    - the startup breaker shared per process;
    - the metrics counter and gauge;
    - backup-restore-check.sh (major-version check, refuses a zero-test run);
    - the `memory-service.yml` PGDG step (after the extracted gate; the job's network is not isolated);
    - the README Operations section (every key it names is read).
  - OpenBot:
    - reverify.rs, composed once in `with_memory` before recall, tools and the delivery worker;
    - the compare-and-set `credential_refused` (settled present rows whose namespace still stores that id;
      a single-row update, no lock cycle);
    - delivery 401 → `Retry("credential_unverified")`, 403 terminal;
    - A4-8: verify via `revoke_labelled`'s complete listing, issue, then a swap guarded by
      `credential_id = old` that keeps `ready`; a failed swap leaves a stray for the next pass;
    - the delivery claim and steps read `credential_id`;
    - the restore SQL in deploy.md (checked against every CHECK on both tables), run verbatim by
      restore_tests;
    - the contract fake (issued tokens now authenticate; revoke and forget remove them);
    - the deploy docs (401 behaviour, restore steps and limits, F5).
- Deviations.
  1. The single wrapper: accepted. A multi-namespace 401 re-verifies each namespace, and each verification
     is only a listing.
  2. `ClientCredential.id`: accepted.
  3. The doc-embedded SQL tested verbatim: good.
  4. The fake now authenticates issued tokens and stops revoked ones. That strengthens fidelity; my run of
     the OpenBot memory suite passes.
  5. The audit row only on change: accepted.
  6. The test using `invalid_request_error`: correct, given A4-1.
  7. The static gauge: acceptable for one breaker per process (F1b).
  8. The CI step: unverified, as stated.
- Reviewer evidence. Private cluster 127.0.0.1:55432, DATABASE_URL unset; `mem_p06_prep` untouched.
  - Service `cargo test --locked --offline --all-targets`: 174 passed, 0 failed, 2 ignored, the after-restore,
    breaker and overload tests included (logs/MEM-P06-B2-R1-B-service-suite.txt).
  - `scripts/backup-restore-check.sh` with `MEMORY_PG_BIN=/opt/homebrew/opt/postgresql@18/bin`: 1 passed, and
    no test database left (logs/MEM-P06-B2-R1-B-backup-check.txt).
  - OpenBot `cargo test --locked --lib -- memory` on my own `open_bots_b2r1_test`: 167 passed, 0 failed,
    3 ignored (logs/MEM-P06-B2-R1-B-openbot-memory.txt). The database was dropped.
  - Not run (06C): verify.sh, the full OpenBot suite and the stack budget, the live script, the browser test,
    and the load measurement (the author's figures are informational).
- Findings: F1 (optional: a, b).
- Outcome: **PASS for digest a83ffe8402206d2cbee9aa6856c4666fc130a5f500bb3dffa42e08bef8e9afd1 (freeze tree fb435320) for
  p06-B2**. Optional: F1. The representative load and the paid-provider smoke check remain open user decisions,
  not passes.
- Return-time freshness: HEAD a6bed48a; digest recomputed a83ffe84…afd1, MATCH.

## MEM-P06-B3-R1 — batch review of p06-B3 (06B.1–06B.4, B2-F1, U8/U9): CHANGES REQUIRED (F1, F2)

- 2026-11-10; stage batch review.
  - HEAD a6bed48a. Digest b86dc1e8…4b0e recomputed at start: MATCH.
  - Freeze tree 540efd56 is a tree object. The incremental diff's sha256 (0c0a7089…) matches.
  - The change from fb435320 covers 23 files outside `.sqlx` and the phase docs (+1399/−59).
  - About 22 GiB free.
- Reviewed against plan 06B.1–06B.4 (06-operations-and-verification.md:35-62), the TASK record, MEM-P06-B2-F1,
  COORDINATION I003 r5 (U8, U9) and D12 (implementation-decisions.md:622-625).
- 06B.1, the standalone journey (services/memory/scripts/standalone-journey.sh, standalone_client.py): accepted.
  - The copy is independent:
    - it holds only `git ls-files` of the service, in `mktemp -d`;
    - a copy that `git rev-parse` places inside a checkout is refused;
    - the model is copied in;
    - `boundary-check.sh`, then `cargo build --locked --offline`;
    - its own throwaway database and role, through the service's own `provision-db.sh`;
    - the Messages API stand-in is now inside the service (`scripts/fake-messages.py`);
    - the EXIT trap drops the database, role and directory.
  - Nothing in the service is configured to reach OpenBot. Parent and OpenBot access is "denied" by absence plus
    the boundary check, as accepted for `extracted-build.sh` in earlier phases.
  - The generic client is enough for 06B.1:
    - stdlib only, no OpenBot-shaped payload;
    - it provisions a tenant, namespace and key; saves, then replays; searches through the semantic leg only;
    - classification: a read_max-2 key sees nothing;
    - fetch, timeline, context pack and validate, MCP;
    - generation through the counted stand-in: the journey requires a `/v1/messages` call, and the provider
      counters are printed;
    - correct, delete, purge, and tenant delete (then 401).
  - The journey requires all 13 step lines.
- 06B.2, the `memory_e2e_` tests, against the real service:
  - The 6/8/9 matrix through the real `memory.load` (memory_steps_e2e_tests.rs): 0–6; a level-3 reader joining
    gives 0–3; with level-4 content the load is refused (`KNOWLEDGE_AUDIENCE_BELOW_CONTENT`) before any read, with
    no consumption row.
  - Labels follow consumption. Exact 4 saves at 4 and exact 6 at 6; inexact under ceiling 6 saves at 6; company
    scope saves at the ceiling (U2). An independent level-7 memory stays out at ceiling 6.
  - The `visible_anywhere` probe:
    - search, pack and MCP search must not contain the id;
    - fetch, timeline and MCP fetch must give an answer for the hidden id equal to the answer for a fresh absent
      id, once the request ids are blanked and the absent id is replaced by the hidden one;
    - it runs both with OpenBot's credential at ceiling 3 and with an own read_max-3 key;
    - positive control: the probe finds the memory at ceiling 6.
    - Sound. Indistinguishability from "does not exist" is the right property, and it is stronger than checking
      a status code.
    - Not covered, and not claimed: timing side channels.
  - Tampered labels: sound.
    - `jsonb_set` on the frozen request gives consumed 7 above ceiling 6, or a null ceiling.
    - The real service refuses both, the intents fail, and nothing is stored.
    - A level-7 external source is refused at the freeze, so no intent exists. The post-freeze variant is
      F3(b), optional.
  - The revoked credential:
    - 401, transient `(attempts, transient) = (0, 1)`;
    - A4-8 swaps in a new credential for the same namespace;
    - the save is delivered once, at 4.
  - The "public latest recall after private history" case is mapped: freeze level here, run level in the
    fake-backed E12 table. Accepted.
  - Gap: "validate source provenance" (F2a).
- 06B.3: separate processes and roles, and only URL and credential configuration between them. Accepted in shape.
  - The separate-host run is honestly reported as unavailable and not claimed.
  - The isolation check runs both ways, but the service-role half depends on OpenBot's schema existing in
    `TEST_DATABASE_URL`, and nothing creates it there. A fresh database, as in CI, fails before the tests (F1).
  - The reverse probe is a generic non-superuser role (F3c).
- 06B.4: every test named in the map exists.
  - The new `memory_authority_client_revocation_races_competing_saves`, 6×8 saves racing a revoke: committed =
    stored = audited; every refusal is `credential_revoked`; a save after the revoke is refused.
  - It does not assert that both outcomes occurred (F3a).
  - The dispatch/commit crash boundaries are mapped to existing generation tests. Accepted.
- B2-F1: both parts fixed.
  - (a) Lower bounds from recorded start times.
  - (b) The gauge reads `App::generation_paused()` from the one breaker startup hands both the App and every
    worker; the router is built after it.
- U8: a reported measurement, gating nothing, as the user decided. No review action; the numbers are
  informational.
- U9 and D12:
  - The named model is fixed by the service (`claude-sonnet-5-5`; the adapter refuses any other).
  - The budget is the tenant's daily cap of 50,000. The one generation request asks for an allowance of 40,000
    and at most 1,024 output tokens, so a retry would be refused at the cap.
  - The paid generation step accepts `nothing_to_save`.
  - Key handling is acceptable:
    - only the `MEMORY_SMOKE_ANTHROPIC_API_KEY=` line is read (the last one);
    - quotes are stripped and an empty key is refused;
    - nothing echoes the key;
    - `.env.example` documents it.
  - The key does reach every child process of the journey (F3d, optional).
  - I did not read `.env` and did not run the paid smoke.
- Reviewer evidence. Private cluster 127.0.0.1:55432, DATABASE_URL unset; `mem_p06_prep` untouched; no mutation
  probes.
  - `scripts/memory-integration.sh` against a freshly created `open_bots_b3r1_test`: exit 1 at the isolation
    check, `relation "companies" does not exist` (logs/MEM-P06-B3-R1-B-memory-integration.txt).
  - After applying the init migration by hand (logs/MEM-P06-B3-R1-B-schema-apply.txt): isolation passed both ways,
    and 6 passed (4 `memory_e2e_` and 2 `memory_integration_live_`), exit 0
    (logs/MEM-P06-B3-R1-B-memory-integration-migrated.txt).
  - The standalone journey (`MEMORY_EXTRACT_UNTRACKED=1`, ports 18195/18196): 13 steps passed. The stand-in was
    called (input 50 / output 40 tokens), exit 0, and no database, role or temp directory was left
    (logs/MEM-P06-B3-R1-B-standalone-journey.txt).
  - Service focused: the new revocation race and the reworked breaker test, 3× each, all passed
    (logs/MEM-P06-B3-R1-B-focused-service.txt).
  - Cleanup confirmed: no `memory_live_*`, `memory_journey_*`, `openbot_probe_*` or `open_bots_b3r1_test` database
    or role remains.
  - Not run (06C): verify.sh, the full OpenBot suite and stack budget, the load measurement, the image job, and
    the paid smoke (forbidden).
- Findings:
  - F1: Minor, actionable. The isolation check fails on a fresh database, CI's included.
  - F2: Minor, actionable. Provenance and the production image have no real-service evidence and are not
    recorded as uncovered.
  - F3: optional (a–e).
  - B2-F1: resolved.
- Outcome: **CHANGES REQUIRED for digest b86dc1e8992a694729b968f3b9b5511100f6cd4e8ea39f154ed55e33d0f64b0e (freeze tree
  540efd56) for p06-B3**: F1, F2. Optional: F3.
- Return-time freshness: HEAD a6bed48a; digest recomputed b86dc1e8…4b0e, MATCH.

## MEM-P06-B3-R2 — recheck of p06-B3 corrections (F1, F2, F3a–e): PASS

- 2026-11-10; stage correction recheck.
  - HEAD a6bed48a. Digest 3617afd0…9405 recomputed at start: MATCH.
  - Freeze tree 607df2cd is a tree object.
  - Diff sha256 values match: the full diff 458a0ba5…0ac315, and the incremental diff from my R1 tree 540efd56
    71973d11…7eb19d4.
  - The change from 540efd56 covers 8 files outside `.sqlx` and the phase docs (+125/−28), as stated.
  - About 22 GiB free.
- Coordination: COORDINATION I001 r2.
  - The user authorized a push to `main` once the plan is complete: after 06C passes and B's final pass on the
    integrated tree.
  - `origin/main` lacks this work's base, so root will merge `origin/main` on a branch, and 06C runs on the merged
    tree.
  - Deploys and production resets stay unauthorized.
  - Noted. The final pass on the integrated tree, merge resolution included, is a later round.
- F1 fixed.
  - The service-role probe reads a throwaway no-grant table that the admin creates in the OpenBot database and the
    cleanup trap drops.
  - The admin URL with the OpenBot database path is built from the admin authority, so no credential is printed.
  - The probe fails closed: `ON_ERROR_STOP` on the create, and any answer other than `permission denied` exits 1.
  - It is equivalent to OpenBot's real tables, because the init migration grants nothing.
  - Reviewer run on a database created just before the run (`open_bots_b3r2_test`, `companies` absent):
    "isolation: neither role can read the other's database", then 6 passed, exit 0. The probe table was gone and
    the database dropped (logs/MEM-P06-B3-R2-B-memory-integration-fresh-db.txt).
- F2 fixed.
  - (a) Raw-fetch provenance on the generated memories: two distinct `source` refs at revision 1, which the
    service's in-window rule makes the captured parts. Passed in my run.
  - (b) Corrected record: no image journey, carried to 06C as unavailable.
- F3 (a)–(e) taken.
  - (a) The barrier plus save-until-refused makes both outcomes certain; a revoke that did not stop authenticated
    principals now fails. 3× passed (logs/MEM-P06-B3-R2-B-focused-service.txt).
  - (b) A post-freeze level-7 external source is refused, and nothing is stored. Passed in my run.
  - (c) The deploy.md role requirements are accurate to `provision-db.sh`.
  - (d) The key is unexported and unset before any child process; only `serve` gets it, by assignment.
    - The stand-in's log records only path and body, so no key would reach it either.
    - Observation, not a finding: in `anthropic` mode, `serve` still inherits a `MEMORY_ANTHROPIC_BASE_URL` from
      the operator's own shell, if one is set. Config accepts only https or a loopback host.
  - (e) Recorded.
- Reviewer evidence. Private cluster 127.0.0.1:55432, DATABASE_URL unset, `mem_p06_prep` untouched, `.env` not
  read, paid smoke not run, no mutation probes.
  - The live script on a fresh database: above.
  - The standalone journey (stand-in; `MEMORY_EXTRACT_UNTRACKED=1`; ports 18195/18196; both key variables unset in
    the caller): 13 steps passed. The stand-in was called (input 50 / output 40), exit 0, and no database, role or
    temp directory was left (logs/MEM-P06-B3-R2-B-standalone-journey.txt).
  - The race test 3×: passed.
  - fmt `--check` on both crates, `bash -n` on the three scripts, and clippy `--all-targets -D warnings` on both
    crates: all exit 0 (logs/MEM-P06-B3-R2-B-fmt.txt).
  - Not run (06C): verify.sh, the full suites, the stack budget, the image job.
- Findings: none new. F1 and F2 resolved; F3 taken.
- Outcome: **PASS for digest 3617afd03fefe97f517200b0a4a266940ab3441f0e27c9f5b8b4d20f2c249405 (freeze tree 607df2cd) for
  p06-B3 (R1–R2)**. No open B3 findings.
- Return-time freshness: HEAD a6bed48a; digest recomputed 3617afd0…9405, MATCH.
