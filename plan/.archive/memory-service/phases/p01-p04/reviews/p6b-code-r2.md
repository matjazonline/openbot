# Review p6b-code-r2: P6b recheck (M1, m1, n1, n3–n6), reviewer B gen 3

## Artifact identity

- Reviewer: role B gen 3, nonce `run=08e8e605 role=B gen=3`, agent `afbba0fc2ed0e068e`.
- `frozen/p6b-code-r2.diff`: sha256 `652c69a5…4899`, matches.
- `frozen/p6b-r2-incremental-from-r1.diff`: sha256 `8ebae747…8b61`, matches (14 files).
- Worktree tree (temporary index, `tmp/` excluded): `42604f1e7d8c999a7ea9877bb052d5d94d674df2`. It equals the frozen tree before and after my runs.
- Read: the whole r1→r2 incremental diff (production code, the five new tests and the changed shutdown test), the P6b notes' round-1 resolutions, 01B and the access-level contract, `startup.rs` shutdown timing, the `observation_links` foreign keys, and the `insert_links` call sites.

## Checks I ran

All runs used root's private cluster and the shared target only.

- `verify.sh`: **all gates passed, 152/152** under network isolation. Log `logs/b3-p6b-r2-verify.log`.
- `focused.sh memory_generation_ --network-isolated`: five runs, 16/16 each (3.8 s). Logs `logs/b3-p6b-r2-generation-run{1..5}.log`.
- The same suite as three concurrent processes: 16/16 each. Logs `logs/b3-p6b-r2-generation-parallel{1..3}.log`.
- No flakiness seen.
- Mutation probes on a scratch copy of the crate, built into the shared target.
  - The copy and its scratch database `b3mut` (migrated, used only for the SQLx macros) were removed afterwards. No project file was edited.
  - Logs `logs/b3-p6b-r2-mut{A,C,D}.log` and `logs/b3-p6b-r2-mutAD-{1,2,3}.log`.

| Mutation | Effect | Result |
| --- | --- | --- |
| A | local deadline = `lease + margin`, i.e. after the DB `lease_until` (both in `pass` and in the heartbeat update) | **survives**: stalled-heartbeat and reaper-race tests pass |
| C | a successful renewal no longer extends the local deadline | **caught**: the reaper-race test fails |
| D | the reaper frees a dispatched slot when `lease_until − 0.5 s + grace < now()` | **survives**: all 16 tests pass |
| A + D | a 1 s window, under test settings, in which the slot is free while the first call can still be live | **survives: 16/16, three runs out of three** |

## Recheck

| Item | Status |
| --- | --- |
| M1: stalled heartbeat vs slow provider | **Partly resolved; see M1 below.** The test now runs the failed-renewal branch (the renewals' `lock_timeout` against a held row lock, with `pg_stat_activity` showing the lock waits) and the local-deadline branch (`dropped() == 1` while 30 s remain on the provider). But the same row lock also blocks the reaper (`SKIP LOCKED`), so the second call cannot start before the holder commits, whatever the deadline or the reaper threshold. Its "never doubles" assertion cannot fail on the property it names: A, D and A+D survive. |
| M1: reaper vs heartbeat | **Met but coarse.** Two reapers tick every 10 ms at grace 0 against a 2.5 s call with a 1 s lease and 150 ms renewals: never reaped, never dropped, saved once. It catches a renewal that stops extending the deadline (C). It does not catch a reaper acting up to about 0.85 s early (D), because renewals keep that much lease ahead. |
| M1: competing sweepers | **Met.** Four sweepers over 1,200 rows in batches of 50: the counts sum exactly, there are no errors, and every batch is bounded. |
| m1 sweep starvation | **Fixed.** The candidate scan uses the null-out predicate (`generation.rs:1295-1312`) and the UPDATE still re-checks. The test has 600 reused old rows ahead of an expired one; only the expired one is nulled. |
| n1 | **Fixed.** A failing candidate is logged and skipped; the error is returned only when nothing was claimed. Untested (it needs fault injection); acceptable. |
| n3 shutdown | **Sound.** See below. |
| n4 | **Fixed.** Claim-time stops (authority, expiry, `spend_cap_reached`) write the system audit row. |
| n5 | **Holds.** See below. |
| n6 | **Fixed as stated.** The residual existence signal is acceptable; see below and m2. |
| n2 | Deferred to the 06A purge, as recorded. |

### n3 shutdown (80/20)

- **Timing.** The worker's drain timer starts at the shutdown request (`drained`: `cancelled()` then sleep 0.8 × grace). The join deadline is also measured from the request (`startup.rs:206-207`), so 0.2 × grace is left to settle `unknown(shutdown)`.
- **No new work.** The heartbeat keeps the lease during the drain, so no reaper on another instance can act. No new claim starts: `run` checks shutdown, and a claim taken while stopping is released.
- **Commit racing the abort.** A call that answers just before 0.8 × grace settles in one transaction. If the join aborts it mid-transaction, the transaction rolls back and the operation stays `dispatched`. Any instance's reaper then records `unknown(lease_expired)` after `lease_until + grace`. Never a partial commit, never a re-send. The output is lost but the loss is surfaced.
- **Coverage.** The test covers a call that finishes within the drain; the r1 test still covers the abort after it.

### n5 links and locks

- The only link path changed is `save_output`'s window-source links (`Link::Source`). Generated items have no parent links.
- The `observation_links (tenant_id, source_id, source_level) → sources` foreign key takes `FOR KEY SHARE` on the cited rows, after the observations are inserted, so the order matches D13.
- What actually protects the evidence is not that lock: `FOR NO KEY UPDATE` does not conflict with `KEY SHARE`. It is that the sweep never touches an unfinished operation's sources, together with immutable revisions. The code comment says exactly that, so the claim holds.
- The client save and correct paths (`memory.rs:912, 1013`) are unchanged.

### n6 under 01B

01B's filtering rules govern reads (results, snippets, counts, omissions). Here a caller writes an identifier it chose, `(ref, revision)`, into a namespace it holds a write grant for. A collision with an unreadable record is inherent to unique identifiers within one namespace, and the uniform 409 removes the content-confirmation oracle. The remaining "something exists here" signal is acceptable and recorded in the P6b notes.

## Findings

### Major

- **M1 (residual), the paid-call bound is still not falsifiable**
  - Code: `generation_worker.rs:166, 329` (local deadline), `adapters/postgres/generation.rs:1205` (reaper threshold).
  - Test: `generation_tests.rs`, `memory_generation_stalled_heartbeat_with_a_slow_provider_never_doubles_calls`.
  - **Why it does not count yet.** The row lock that makes renewals fail also blocks the reaper. The test therefore cannot observe the slot being freed while the first call is live. A deadline after `lease_until` (A), early reaping (D), and both together (A+D: a real overlap) all pass 16/16.
  - **Required:**
    - Make renewals fail *without* locking the row, so the reaper is free. For example, give the stalled worker a `Store` wrapper whose `renew_generation` returns `Err` (or stalls), with everything else delegated to `PgStore`. Keep the reaper at grace 0, plus a second worker with a second same-tenant operation.
    - Assert `drops_at_start == [0, 1]` and `max_in_flight == 1`.
    - Assert that when the drop is observed, the database still has `lease_until > clock_timestamp()` (the deadline falls before the lease).
    - Add a reaper boundary check that would catch D: a dispatched row with `lease_until` slightly ahead is not reaped; one with a grace > 0 is not reaped before `lease_until + grace`.
    - Re-run A, D and A+D: each must fail.

### Minor

- **m2, undocumented side effect of n6** (`adapters/postgres/generation.rs:359-365`, `adapters/postgres/memory.rs:730-736`).
  - A write-up credential (`write_max > read_max`) now gets 409 for *its own* revisions above its `read_max`. That covers record-then-generate with identical text, overlapping or sliding windows that reuse revisions, and a re-record under a new key.
  - This is a plausible capture pattern at levels above the reader's ceiling. The README and D3/D9 mention only the guess-confirmation fix.
  - Required: document the limitation (README conflict text, D9 window storage, D3 write-up grants), so clients can send such evidence only through fresh revisions; or decide otherwise.

### Nits

None new.

## Remaining checks

1. Fix residual M1 (and re-run the mutation probes); document m2. Then focused `memory_generation_` (repeat a few times) and `verify.sh`.
2. Carried, CI only: the first `memory-service.yml` run and the image job.
3. Carried to 06B: the black-box serve run against the fake Messages API.
4. Carried to 06A: the idempotency-key decision, n2 under purge, the authentication circuit breaker, the paid smoke budget.

## Verdict

**Not passed, on residual M1 (major, test strength).**

- Fixed and sound: m1, n1, n3, n4, n5, n6. The production code is unchanged in behaviour where M1 matters, and still reads correct.
- Evidence: verify.sh 152/152, and `memory_generation_` 16/16 across 5 sequential and 3 concurrent runs.
- The open point: the new stalled-heartbeat test cannot fail when the local deadline and the reaper threshold overlap, as mutation A+D shows.
