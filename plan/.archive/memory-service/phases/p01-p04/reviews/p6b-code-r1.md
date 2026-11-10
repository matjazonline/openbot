# Review p6b-code-r1: P6b = P6a-r2 m1 + n1–n5 and batch 04B (reviewer B gen 3)

## Artifact identity

- Reviewer: role B gen 3, nonce `run=08e8e605 role=B gen=3`, agent `afbba0fc2ed0e068e`.
- Full diff `frozen/p6b-code-r1.diff`: sha256 recomputed `a93941dd…a513`, matches.
- Incremental from P6a, `frozen/p6b-incremental-from-p6a.diff`: sha256 `097c217c…29d9`, matches (76 files).
- Worktree tree (temporary index, `tmp/` excluded): `41668d9eb81330d0a33f4bed550e5bfce0648f94`, the same as the frozen tree.
- Originals read:
  - 04B in full and its focused-check list, plus 06A.1 and 06B.
  - D3 (cancellation on state changes), D5, D9, D10 (the `memory_generation_` row), D13.
  - The P6b implementation notes.
- Code read in full:
  - `adapters/postgres/generation.rs`
  - `application/generation_worker.rs`
  - `cancel_operations` (`adapters/postgres/mod.rs`)
  - the migration diff
  - the new parts of `application/generation.rs` (request/status/cancel, canonical identity, `restore`)
  - the startup and config diffs
  - the redaction patterns
  - `generation_tests.rs`: acceptance, claimants, lease loss, ambiguity, spend, revocation and deletion, restart, both retention tests
  - the counted provider's in-flight tracking

## Checks I ran

All runs used root's private cluster at 127.0.0.1:55432 and the shared target only.

- `scripts/verify.sh`: **all gates passed, 147/147** under macOS network isolation. Log `logs/b3-p6b-verify.log`.
- `scripts/focused.sh memory_generation_ --network-isolated`, six runs back to back: 11/11 each, 2.9–3.3 s. Logs `logs/b3-p6b-generation-run{1..6}.log`.
- The same suite as three concurrent `cargo test` processes, for CPU and DB contention: 11/11 each. Logs `logs/b3-p6b-generation-parallel{1..3}.log`.
- No flakiness seen.

## P6a-r2 recheck

| Item | Status |
| --- | --- |
| m1 prefixed keys | **Fixed** (`domain/generation.rs`). `(?:[a-z0-9]+[_-])*` key prefix and `secret[_-]access[_-]key`; URL passwords (`scheme://user:pass@`) redacted. Marker-skip keeps both passes idempotent. Tests cover `DB_PASSWORD=`, `refresh_token=`, `postgres://user:…@`. |
| n1 prose | **Fixed.** A token cannot end in `.`/`-`. Bearer/Basic (≥ 16) and `Authorization:` (≥ 8) need a digit or `+ / =`. The three prose cases are tested. |
| n2–n5 | **Fixed:** identifier docs; quoted value absorbs its tail; any model other than `claude-sonnet-5-5` refused at construction (tested); secret-shaped revisions tested in both paths. |

## 04B coverage

| 04B item | Status |
| --- | --- |
| 1. Atomic acceptance, competing claimants, leases, fences, supervision, stale workers | **Met in code.** See below for how far tests cover it. |
| 2. Spend reservation, logical identity, rechecks, capacity | **Met.** |
| 3. Safe retry vs ambiguous paid request | **Met.** |
| 4. Terminal stops, no revival, explicit recovery | **Met.** |
| 5. One fenced commit at the frozen level | **Met.** |
| 6. Expiry, retention, back-off, outcomes | **Met**, except m1. |

Details:

- **Acceptance (D5 order).** The order is:
  1. writer locks;
  2. idempotency claim (a new execution past its deadline gets 410; a replay is still returned);
  3. cap check (`daily_cap_zero` → 403; a reservation above the whole cap → 429);
  4. session;
  5. window revisions in `(ref, revision)` order, reused under `FOR SHARE`, refused when nulled or expired;
  6. the operation, its links, the result and audit — all in one transaction.
  - The identity hashes the prepared, filtered request.
  - 8 concurrent identical requests give one operation and zero provider calls (tested).
- **Immutability.** The trigger freezes label, reservation and request, and refuses to revive a finished row. A classification change on retry is a 409 and cannot be represented in the row (tested).
- **Claim.**
  - Candidates are read without locks: one per tenant with nothing in flight, ordered by `last_claimed_at NULLS FIRST`.
  - Per candidate, in this order: tenant/namespace/credential `FOR SHARE SKIP LOCKED`, the grant, the schedule row and the operation `FOR UPDATE SKIP LOCKED`; then the claim update (fence + 1; a 23505 from the partial unique index gives up the candidate); then the schedule; then spend last.
  - That order follows D13. I traced it against disable/delete/revoke (tenant `FOR NO KEY UPDATE` → ops by id → spend), cancel (writer locks → op → idempotency → spend), the reaper (ops `SKIP LOCKED` → spend in (tenant, day) order), settle (parents only when saving → op → content → spend) and the sweeper (`SKIP LOCKED` on its own rows only). I found no wait cycle.
  - 8 concurrent claimants over 2 tenants yield exactly one job per tenant; sequential claims alternate tenants (tested).
- **Dispatch and lease.**
  - `dispatched` and `attempts + 1` are committed before the send, after a blocking re-check of authority and expiry and loading the window.
  - The local deadline is the instant captured *before* the dispatch/renew statement plus lease − margin, so it always falls before the DB's `lease_until` with no clock skew involved.
  - A failed renewal leaves the deadline in place. A renewal refused (fence, owner, cancel requested, lease already expired) marks the lease lost.
  - The reaper frees a slot only after `lease_until + grace`.
  - The heartbeat is aborted with the pass, so a dead worker never keeps renewing.
  - I found no path that allows two live calls per tenant.
- **Outcomes.**
  - Only an envelope with a `request-id` and a retryable type, or `not_sent`, goes back to `accepted`: reservation released, 30 s × 2ⁿ⁻¹ back-off, at most 3 dispatches.
  - Everything else after the send is `unknown`, charged at the reservation and never re-sent.
  - Terminal envelopes become `failed(<type>)`.
  - Disable/enable, the reaper and a new day revive nothing; recovery is a new request (tested, with provider call counts).
- **Spend.** The UTC day is fixed at claim and settlement targets it. Usage above the reservation is recorded and counted; spend-cap failure is terminal (tested).
- **Commit.** One transaction, fenced on state, fence and owner; parents re-checked when saving; expiry re-checked. Observations come from the operation's frozen label columns with `(operation, ordinal)` identity, links come from the window's sources, and the rows enter indexing as pending. A late settle after the reaper is `Stale` with no observations (tested).
- **Retention vs live operations (D9 n3).** The source sweep locks `FOR NO KEY UPDATE SKIP LOCKED`, then re-checks in a fresh-snapshot UPDATE. I traced all three interleavings with an acceptance: the acceptance holding `FOR SHARE`, the acceptance committing between scan and lock, and the acceptance arriving after the sweep. All are safe (tested with 40 racing acceptances).
- **Status and cancel.**
  - Status needs `generation_status` and a `read_ceiling`. An operation above the ceiling, or in an unnamed or deleted namespace, is `not_found`. Observation ids are filtered by ceiling and expiry; outcomes are bounded codes and never provider text.
  - Cancel needs `generate` and a `write_max` at the operation's level, otherwise `not_found`. It locks the operation before its idempotency record.
- **Config.** `MEMORY_ANTHROPIC_API_KEY`, `_BASE_URL` (https, or http to loopback; no userinfo/query/fragment), `_EFFORT` and `MEMORY_GENERATION_WORKERS` (1–4) are read and shape-validated. The README names exactly these and says startup never contacts the provider. Correct.

## Findings

### Major

- **M1, the D10-listed concurrency checks for the lease protocol are missing.** D10's `memory_generation_` row names three checks that no test exercises:
  - "a stalled heartbeat against a slow provider never yields two in-flight calls per tenant";
  - "reaper vs heartbeat";
  - "competing sweepers".
  - **What is untested.** The local-deadline branch (`lost()` → `"deadline"`, `generation_worker.rs:288-300`) never runs in any test, nor does the failed-renewal branch (`:327-328`). Lease expiry is only simulated with a sequential `UPDATE … lease_until = now() - 1s` (`generation_tests.rs:625, 644`). Retention has one sweeper against acceptances, never two sweepers against each other.
  - **Why it is major.** The code reads correct, but this is the invariant that bounds paid calls. AGENTS.md requires competing-claimant tests for concurrency protocols, and the plan says mocked sequential transitions are not proof.
  - **Required tests:**
    1. A store wrapper whose `renew_generation` errs or stalls, a provider slower than the lease, the reaper ticking with grace 0, and a second worker plus a second same-tenant operation. Assert `max_in_flight(tenant) ≤ 1`, that the first call was dropped (`dropped()`) before the second was sent, and `unknown(deadline)`.
    2. Heartbeat renewals racing reaper ticks on a live lease: the operation is never reaped while renewals succeed.
    3. Two or more concurrent `sweep_retention` calls over the same candidates: counts sum exactly and nothing errors.

### Minor

- **m1, the retention source sweep can starve** (`adapters/postgres/generation.rs:1274-1286` against `:1306-1313`).
  - **Cause.** The candidate SELECT omits the UPDATE's "no operation using it finished in the last 30 days" condition. Rows recorded more than 30 days ago but reused by a recently finished operation are therefore selected and locked (`ORDER BY recorded_at LIMIT 500`), then never nulled.
  - **Effect.** With ≥ 500 such rows ahead of the eligible ones, every tick picks the same rows. Every other eligible row waits, including content past its own expiry, until those age out: up to 30 days, and longer if reuse continues. "Full" never triggers, so it does not hot-spin, but retention silently stalls (06A.1: bounded cleanup that does not make deletion impossible).
  - **Fix.** Use the same predicate in the candidate scan, and add a test with more than one batch of such rows ahead of an expired one.

### Nits

- **n1** (`generation.rs:1393`): `claim_generation` returns on the first non-deadlock error. The failing candidate rolls back with `last_claimed_at` unchanged, so every worker tries it first on every pass. One poison candidate stalls all tenants, behind the back-off. Log it and continue with the next candidate.
- **n2** (`generation.rs:945-957, 1052`): a `Saved` settlement whose parent rows are missing (`recheck_authority` → `None`) skips the stop check and saves. Today this is unreachable (tenants and namespaces are tombstoned); treat it as a stop before 06A purge removes rows.
- **n3** (`generation_worker.rs:204`, `startup.rs`): shutdown aborts in-flight calls at once as `unknown(shutdown)`, so every deploy turns up to `workers` paid calls into unknowns. Consider: stop claiming at shutdown, and abort in-flight calls only near the end of the grace.
- **n4** (`generation.rs:647-735`): the claim-time stops (authority, expiry, `spend_cap_reached`) write no `system_audit` row; dispatch-time stops do.
- **n5** (`generation.rs:1108-1125`): `save_output` locks sources `FOR SHARE` before inserting observations, the reverse of D13's content order. It cannot deadlock, because the sweeper only uses `SKIP LOCKED`, but align either the code or the D13 text.
- **n6** (observation, existing before this point): a revision whose digest matches is reused and a mismatch is a 409. A credential whose `write_max` exceeds its `read_max` could use that to confirm guesses about a stored revision's content at a level it cannot read. This applies to `sources.record` and now to acceptance. Raise it only if D3 allows such write-up grants.

## Author's open issues

- **Black-box serve test against the fake Messages API.**
  - **Not required by 04B:** its focused checks call for a counted test provider, and the adapter is already tested against a loopback fake.
  - **Required by 06B.1.** Its standalone flow "generates observations through a test provider", and the production binary contains no test provider. The only way is the fake Messages API through `MEMORY_ANTHROPIC_BASE_URL`, which config allows over http to loopback.
  - The startup wiring (config → provider → `workers` → settle) is not exercised by any test today. A cheap serve test now would de-risk 06B, but it is not a blocker.
- **`UNIQUE (tenant_id, idempotency_key)` on `generation_operations`.**
  - **No correctness problem before 06A:**
    - nothing deletes idempotency records yet;
    - the record's scope `(tenant, 'generations.request', key)` equals this constraint;
    - the record is claimed first, so the constraint cannot fire on any legitimate path.
  - **06A must decide:** once records are swept after 7 days while operation rows remain, reusing the key would hit an unmapped 23505. Either drop the constraint (the record is the identity) or map it to 409 and document that keys are not reusable.
- **Authentication failure fails queued operations one by one.** This is acceptable under 04B.4: each stops terminally and none revives. Each costs one unbilled 401. A circuit breaker belongs with 06A's provider-outage handling.
- **Metrics and purge belong to 06A.**
- **Declared deviations:** no text-less events; matching on digest, level and session; `read_ceiling` on status; replay returns the acceptance result. All are coherent with D5/D9 and recorded.

## Speculative scope

None found.

## Remaining checks

1. Add the M1 tests; fix m1 and test it (n1–n5 optional). Then run focused `memory_generation_` (repeat a few times) and `verify.sh`.
2. Carried, CI only: the first `memory-service.yml` run and the image job (`aws-lc-sys`).
3. Carried to 06B: the black-box serve run against the fake Messages API.
4. Carried to 06A: the idempotency-key decision above, and the paid smoke budget.

## Verdict

**Not passed, on M1 (major, a test gap).** Everything else in 04B reads correct:
- the protocol, lock order, fencing, spend, commit, retention races, authorization and config;
- the P6a-r2 fixes.

Evidence: verify.sh 147/147, and `memory_generation_` 11/11 across 6 sequential and 3 concurrent runs. The source-retention starvation (m1) should be fixed in the same round.
