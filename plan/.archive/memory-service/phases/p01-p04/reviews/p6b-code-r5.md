# Review p6b-code-r5: P6b recheck limited to F2 and the reaper-vs-heartbeat timing assertion, reviewer B gen 3

## Artifact identity

- Reviewer: role B gen 3, nonce `run=08e8e605 role=B gen=3`, agent `afbba0fc2ed0e068e`.
- `frozen/p6b-code-r5.diff`: sha256 `d90994b5…df2f`, matches.
- `frozen/p6b-r5-incremental-from-r4.diff`: sha256 `c99c4397…d4ab`, matches. It touches 3 files; the only code change is in `generation_tests.rs`.
- Worktree tree (temporary index, `tmp/` excluded): `7043063f6d704e1d9bbc5dbfd3c87584e884e4c2`. It equals the frozen tree before and after my runs.

## Reaper-boundary test (`memory_generation_reaper_frees_a_dispatched_slot_only_after_lease_and_grace`)

**Does the retry loop let an early reaper pass?** No.

- **The early-free check runs in every attempt.** Each attempt ticks the reaper until it frees a fresh dispatched operation (1 s lease, 0.7 s grace). The freeing tick must end with the database clock at or after `lease_until + grace`, or the test panics at once. A retry cannot skip that check.
- **What makes an attempt count.** Its last quiet tick must have *started* (database clock, read just before the tick) at or after `boundary − 450 ms`. The tick's transaction, and so its `now()`, comes after that instant.
- **Why a 0.5 s-early reaper always fails.** Such a reaper would free the slot on that very tick, so the tick is never quiet. Under that mutation an attempt either:
  - fails "freed early", or
  - frees on a tick long enough to end past the boundary, and so does not count.
- After 5 attempts that do not count, `qualified` is false and the test fails. So any reaper at least 450 ms early fails deterministically, whatever the load. Smaller errors are caught whenever the freeing tick ends before the boundary, which is the tick-resolution bound the test states.
- **Load affects only how fast an attempt counts.** Each retry uses a fresh operation; the previous one is `unknown(lease_expired)`, so the tenant slot is free. Slow ticks delay qualification but never make the test pass wrongly.

## Reaper-vs-heartbeat test

The `ticks > 20` count is replaced by a requirement that at least one reaper tick ran entirely after `dispatched_at + 1 s` and before `finished_at`, by the database clock.

- `dispatched_at` and the initial `lease_until` come from the same statement, so `dispatched_at + 1 s` is the initial `lease_until`.
- With grace 0, an unrenewed lease would be reaped by such a tick. The `(released, abandoned) == (0, 0)` assertion therefore now provably covers the window where renewal is what keeps the lease alive. That is exactly the claim, with no dependence on how many ticks ran.

## Mutation probes

Each probe ran on a scratch copy with a separate `CARGO_TARGET_DIR` and a scratch database `b3mut5`, migrated for the SQLx macros. Both were removed and dropped afterwards (no `b3mut*` database remains). Logs `logs/b3-p6b-r5-mut{D-1,D-2,R}.log`.

| Mutation | Change | Result |
| --- | --- | --- |
| D | reaper frees a dispatched slot when `lease_until − 0.5 s + grace < now()` | **caught twice**: reaper-boundary test "freed early" by about 0.5 s, plus the stalled-heartbeat test in run 1 |
| R | a renewal no longer extends the database lease (`SET lease_until = lease_until + 0 * …`) | **caught**: reaper-vs-heartbeat test, `(released, abandoned) = (0, 1)` |

(The first attempt at R failed to compile on a type error in my mutation; it was redone.)

## Repeats on the real tree

Shared target, built from the real sources.

| Mode | Result |
| --- | --- |
| `focused.sh memory_generation_ --network-isolated` | 17/17 |
| 20 sequential `cargo test --lib memory_generation_` | 20/20 |
| 8 rounds × 4 concurrent processes (load average about 9; several rounds overlapped the mutation build) | **32/32** |
| `verify.sh` | **all gates passed, 153/153** under network isolation (`logs/b3-p6b-r5-verify.log`) |

Logs `logs/b3-p6b-r5-{focused,seq*,par*}.log`. No failure in 53 suite runs, including conditions heavier than those that broke r4.

## Findings

None.

## Remaining checks

1. Carried, CI only: the first `memory-service.yml` run and the image job.
2. Carried to 06B: the black-box serve run against the fake Messages API.
3. Carried to 06A: the idempotency-key decision, n2 under purge, the authentication circuit breaker, rotation docs, the paid smoke budget.

## Verdict

**Pass: F2 and the second timing assertion are resolved.**

- **Evidence:**
  - Both lease tests now assert against the database clock, not tick counts.
  - The retry loop cannot let an early reaper pass.
  - Mutations D and R are caught in a separate target.
  - `memory_generation_` passed 53/53, including 32 runs under 4× concurrency.
  - `verify.sh` passed 153/153.
- **P6b as a whole passes:** P6b's earlier findings were all resolved in rounds 1–4.
