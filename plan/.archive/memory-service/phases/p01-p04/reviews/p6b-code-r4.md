# Review p6b-code-r4: P6b recheck limited to F1, reviewer B gen 3

## Artifact identity

- Reviewer: role B gen 3, nonce `run=08e8e605 role=B gen=3`, agent `afbba0fc2ed0e068e`.
- `frozen/p6b-code-r4.diff`: sha256 `330c93ca…3209`, matches.
- `frozen/p6b-r4-incremental-from-r3.diff`: sha256 `e81055e5…e9f`, matches (3 files).
- Worktree tree (temporary index, `tmp/` excluded): `f31d2e7d1c986b2c5b7c6c3d487eaf5fcb50285a`. It equals the frozen tree before and after my runs.

## F1 fix

**Verified, and the test is not weakened.**

- **Own fault flag.** The faulty store is now `PgStore { renew_fault: Arc::new(AtomicBool::new(true)), ..f.db.store.clone() }`. It shares the pool but not the flag, so the competitor's store (`f.app.store()`) renews normally.
- **Start order.** The competitor (polling every 5 ms) and the reaper (grace 0, every 5 ms) start once `calls() == 1`, i.e. once the stalled worker has dispatched.
- **Why nothing is lost.** The stalled call runs on until its local deadline (dispatch + 2 s − 0.5 s), so both competitors are live for the whole window in which an early free or a late drop could double the calls. All assertions are unchanged:
  - drop before the database `lease_until`, by `clock_timestamp()`;
  - `Abandoned("deadline")` and `unknown(deadline)`;
  - `max_in_flight == 1` and `drops_at_start == [0, 1]`.
- **Mutation probes**, on a scratch copy with a **separate `CARGO_TARGET_DIR`** and a scratch database `b3mut4`; both removed and dropped afterwards. Logs `logs/b3-p6b-r4-mut{A,D}.log`; each compiled from the copy.
  - A (local deadline = `lease + margin`): **caught**, the stalled-heartbeat test fails.
  - D (reaper 0.5 s early): **caught**, both the reaper-boundary and stalled-heartbeat tests fail.

## Repeats on the real tree

Shared target, built from the real sources.

| Mode | Result |
| --- | --- |
| `focused.sh memory_generation_ --network-isolated` | 17/17 |
| 25 sequential `cargo test --lib memory_generation_` | 25/25 passed; the stalled-heartbeat test never failed |
| 7 rounds × 4 concurrent processes (rounds 1–3 also ran alongside the mutation build in the separate target) | 26/28 passed. **The stalled-heartbeat test passed all 28.** The 2 failures were both the reaper-boundary test (below). |

Logs `logs/b3-p6b-r4-{focused,seq*,par*}.log`.

## Findings

### F2 (major, gate reliability): the reaper-boundary test has a load-sensitive tick count

- **Where:** `adapters/postgres/generation_tests.rs:1472`, in `memory_generation_reaper_frees_a_dispatched_slot_only_after_lease_and_grace`.
- **The assertion:** `quiet_ticks > 50`. It requires the tick, a `clock_timestamp()` query and a 10 ms sleep to average under about 34 ms across the 1.7 s before the boundary.
- **Observed failures:**
  - `ticks raced the boundary: 36`, while the mutation build ran in the separate target;
  - `ticks raced the boundary: 49`, with nothing else running (load average about 12).
  - That is 2 of 28 concurrent runs, against 0 of 26 sequential.
- **Why it matters.** CI runners are usually slower than this machine and run the whole suite in parallel. This is the same class of problem as F1: the gate must not depend on machine speed.
- **What the test actually needs** is a tick close enough to the boundary to see an early free. Mutation D frees 0.5 s early.
- **Fix:** replace the count with that requirement. Assert that the last quiet tick ended within, say, 300 ms before `boundary` (`last_quiet_after >= boundary - 300 ms`), or that at least one quiet tick ended in `[boundary - 400 ms, boundary)`. Then re-run mutation D in a separate target and repeat the suite under 4× concurrency.

## Remaining checks

1. Fix F2. Then run `memory_generation_` about 20 times sequentially and several rounds of 4× concurrency, re-run mutation D (separate target), then `verify.sh`.
2. Carried, CI only: the first `memory-service.yml` run and the image job.
3. Carried to 06B: the black-box serve run against the fake Messages API.
4. Carried to 06A: the idempotency-key decision, n2 under purge, the authentication circuit breaker, rotation docs, the paid smoke budget.

## Verdict

**F1 is fixed. P6b is not passed, on F2.**

- **F1 holds up:** the stalled-heartbeat test passed 53 of 53 runs here, and still catches mutations A and D in a separate target.
- **F2 is new:** the reaper-boundary test's `quiet_ticks > 50` fails under concurrent load (2 of 28 runs). The fix is to assert tick proximity to the boundary instead of a count.
