# Review p6b-code-r3: P6b recheck limited to M1 (residual) and m2, reviewer B gen 3

## Artifact identity

- Reviewer: role B gen 3, nonce `run=08e8e605 role=B gen=3`, agent `afbba0fc2ed0e068e`.
- `frozen/p6b-code-r3.diff`: sha256 `92876843…9d4d`, matches.
- `frozen/p6b-r3-incremental-from-r2.diff`: sha256 `8ab7e91a…5f73`, matches (12 files).
- Worktree tree (temporary index, `tmp/` excluded): `482af7df752bd76c7384d5f03fa323703bee7028`. It equals the frozen tree before and after my runs.
- Read: the whole r2→r3 incremental diff (`renew_fault`, `recorded_by`, both reuse rules, the rewritten stalled-heartbeat test, the reaper-boundary test, the m2 tests, and the D3, P6b-notes and README changes).

## Invalidated evidence from my round 2 (please record)

A crate copied to another directory gets the **same** test-binary hash in a shared target (`deps/memory_service-ae31e76356c4111f`). Its relative dep-info also made cargo treat the real crate as fresh.

- **Round 2's mutation runs are valid.** Each compiled from the mutated copy.
- **Round 2's flakiness runs after the mutations are invalid** (`b3-p6b-r2-generation-run{1..5}`, `-parallel{1..3}`). They ran the A+D mutant binary. Round 2's `verify.sh` (152/152) ran before the mutations and stands.
- **This round, the same contamination happened.** My first five `focused.sh` runs after the mutations failed 15/17 on the stale A+D binary.
  - I ran `cargo clean -p memory-service` in `services/memory`. It removed about 19 GiB from the shared target, so the next build recompiles from scratch.
  - Every run reported below came after that rebuild.
- **For future probes:** use a separate `CARGO_TARGET_DIR`, never the shared one.

## Checks I ran

All runs used root's private cluster.

- `verify.sh` (started before any mutation build): **all gates passed, 153/153**. Log `logs/b3-p6b-r3-verify.log`.
- **My own mutations against this tree.** Scratch copy plus a scratch database `b3mut3`, migrated, for the SQLx macros; both removed afterwards. Logs `logs/b3-p6b-r3-mut{A,D,AD}.log`.

| Mutation | Effect | Result |
| --- | --- | --- |
| A | local deadline = `lease + margin` (in `pass` and in the heartbeat) | **caught**: stalled-heartbeat fails |
| D | reaper frees a dispatched slot when `lease_until − 0.5 s + grace < now()` | **caught**: reaper-boundary and stalled-heartbeat fail |
| A + D | both together | **caught**: both fail |

- **Repeats on the real tree, after the rebuild:**
  - `focused.sh memory_generation_ --network-isolated` ×5: 4 passed 17/17; **run 5 failed** the stalled-heartbeat test.
  - `cargo test --lib memory_generation_` ×10: 9 passed 17/17; **run 4 failed** the same test (`logs/b3-p6b-r3-generation-x10*.log`).
  - Four concurrent suites: 17/17 each.
  - The stalled-heartbeat test alone ×20: 20/20.
  - The reaper-boundary test passed in every run (19 suites, including four concurrent at about 12 s each), so it is not timing-flaky at this load.

## M1 residual

**The substance is resolved.**

- **`renew_fault` is test-only.** The field, its initialisation and the check in `renew_generation` are all `#[cfg(test)]`. It does not exist in release or non-test builds, and no configuration can set it.
- **The rewritten stalled-heartbeat test** fails renewals without a row lock, leaving the reaper (grace 0, every 5 ms) free. It asserts:
  - the drop happens before the database's `lease_until`, by `clock_timestamp()`;
  - `Abandoned("deadline")` and `unknown(deadline)`;
  - `max_in_flight == 1` and `drops_at_start == [0, 1]`.
- **The new reaper-boundary test** (0.7 s grace, 10 ms ticks, more than 50 quiet ticks) checks the reaper's threshold against the database clock.
- A, D and A+D now fail, as the M1 fix required.

**New problem: the stalled-heartbeat test is flaky.** See F1.

## m2

**Fixed and not abusable.**

- **The rule.** `sources.recorded_by` is set only on insert, to the inserting credential's id, and frozen by the immutability trigger. Reuse above the caller's `read_max` needs `recorded_by = caller` *and* identical content, level and session. Every other caller gets the uniform 409.
- **Credential identity.** Ids are server-generated (UUIDv7 at issue), never client-chosen. A reissued or rotated credential has a new id, so it fails safe to 409. A revoked credential cannot authenticate. Credentials are never deleted (no `DELETE FROM credentials`), and the foreign key keeps `recorded_by` from dangling. Id reuse is therefore impossible.
- **No information gain.** The recording credential can only confirm content it wrote itself.
- **No leak.** `recorded_by` appears only in the two `INSERT`s and two comparison `SELECT`s. There is no wildcard or JSON row select of `sources`, and it is absent from `openapi.json`, the contract examples and every read path.
- **Tests cover** re-record under a new key, record-then-generate, overlapping windows, and a second write-up credential probing with the right and a wrong guess. The limitation is documented in D3, the P6b notes and the README.

## Findings

### F1 (major, gate reliability): the stalled-heartbeat test fails intermittently

- **Where:** `adapters/postgres/generation_tests.rs:1318-1407`, and `adapters/postgres/mod.rs:71-86`.
- **Cause.**
  - `PgStore` derives `Clone` and `renew_fault` is an `Arc<AtomicBool>`. Setting it on `f.db.store.clone()` therefore also sets it on the store the competitor uses (`f.app.store()`). The second worker is *not* "free": its renewals fail too.
  - The competitor is spawned right after the stalled worker, so it sometimes claims the first (30 s) operation itself. The stalled worker's pass then returns `Idle` and the assertion at line 1390 fails (`left: Idle, right: Abandoned("deadline", …)`).
- **Observed:** 2 of 19 full-suite runs. 0 of 20 when the test runs alone. A flaky check would break the CI gate this point depends on.
- **Fix:**
  - Give the faulty store its own flag, e.g. `PgStore { renew_fault: Arc::new(AtomicBool::new(true)), ..f.db.store.clone() }`.
  - Spawn the competitor and the reaper only after the stalled worker has dispatched (`provider.calls() == 1`).
  - Repeat the suite about 20 times, and re-run mutation A in a separate target.

### Nit

- **n1, rotation.** A rotated write-up capture credential loses reuse of revisions its predecessor recorded above its `read_max`; it fails safe with 409. Mention this in 06A.4's credential-rotation documentation.

## Remaining checks

1. Fix F1. Then run focused `memory_generation_` about 20 times (and concurrently), followed by `verify.sh`.
2. Carried, CI only: the first `memory-service.yml` run and the image job.
3. Carried to 06B: the black-box serve run against the fake Messages API.
4. Carried to 06A: the idempotency-key decision, n2 under purge, the authentication circuit breaker, rotation docs (n1), the paid smoke budget.

## Verdict

**Not passed, on F1** (a flaky core concurrency test; the fix is test-only and small).

- **M1's substance is resolved:** A, D and A+D are all caught on this tree, and `renew_fault` cannot reach a release build.
- **m2 is fixed and not abusable.**
- **Gates:** `verify.sh` 153/153. The reaper-boundary test is stable.
