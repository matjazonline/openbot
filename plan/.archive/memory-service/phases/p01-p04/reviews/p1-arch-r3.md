# Review p1-arch-r3 — P1 limited recheck of r2 n1–n5 (reviewer B gen 1)

## Artifact identity

- Reviewer: role B gen 1, nonce `run=e127dfad role=B gen=1`, agent `a636617799682ea85`.
- `tmp/memory-service-run/frozen/p1-arch-r3.diff`: sha256 `1b0ef1c036fb745efcdc5ff9fd99fb8bffe95a375537a75222532187e81dcf38` (recomputed, matches); base `a6bed48a`.
  The working-tree record is byte-identical to the file this diff adds, and `src/main.rs` registers it once.
- I compared it line by line with the r2 record. Changes are limited to D1 (config check and probe), D2 (`tenants.disabled_by`, schedule-row
  insertion), D3 (grant removal on namespace delete, provisioner enable rule), D9 (sweeper/source retention), D10
  (`memory_generation_` row), D11 (batch split), D13 (lock order and per-path rules) and the verification record.
  I did not re-review unchanged areas.

## n1–n5 recheck

| r2 | Status | Evidence |
| --- | --- | --- |
| n1 split `memory.save` | Resolved | D11 lines 493–499. The split is deterministic: greedy, ≤ 3.5 MiB or 256 items per batch. Keys `<base>.<i>-<n>` use only the allowed charset. The base key is ≤ 112 bytes, and the suffix is at most 8 bytes (`.255-256`). Every batch carries the identical frozen context, so all batches get the same label and a retry rebuilds identical requests. A single fact (≤ 64 KiB) always fits a batch. |
| n2 D13 residue | Resolved (one cross-effect, R2) | D13 lines 536–553: the per-path orders for revoke, purge and admin namespace delete are stated. The reaper locks the operation and then its spend row, which is downward. The sweeper uses `ctid … FOR UPDATE SKIP LOCKED` (`FOR NO KEY UPDATE` for null-outs), and its cascades descend. The schedule row is inserted in the tenant-creation transaction (D2 line 110). The sweeper against indexer commit and corrections has no cycle: they wait at the observation row while holding only parent `FOR SHARE` locks, and the sweeper skips locked rows. |
| n3 source retention race | Resolved | D9 lines 431–435. Acceptance locks the recorded sources `FOR SHARE` and refuses one already nulled. A null-out re-checks for non-terminal references under its row lock, and its `SKIP LOCKED` skips sources an in-flight acceptance holds. A missing source at dispatch fails `source_unavailable` with nothing sent. Under READ COMMITTED, an acceptance blocked behind a null-out re-reads the row and sees the null. A test is listed (D10 line 460). |
| n4 credential endpoint and provisioner enable | Resolved, with a bypass (R1) and a grant race (R2) | D3 lines 162–163 and 174–176, D2 line 109. |
| n5 parent cargo config | Meets the requirement as a detection control, with one gap (R3) | D1 lines 43–48 and 85–89. |

### n5 judgement: is detection enough for independence?

Yes, as long as the check is exact.

- 01A.1 requires that the service "cannot depend on … root configuration". 02A.4 requires "a boundary check for parent … configuration".
- Cargo joins array-valued config keys across the config hierarchy, so the service cannot reliably cancel a parent's
  `build.rustflags` from its own config.
- A check that refuses to proceed whenever a parent config contributes something the service does not control means no gated
  build or test runs with parent influence. That is the property that matters.
- The extracted-copy CI run has no parents, so it stays the positive proof.
- Leaving `$CARGO_HOME` out of scope is reasonable: it is the developer machine's own state, and it is empty in CI.

The gap is the exemption rule "fails on any key the service config does not set itself" (see R3).

## Findings

**R1 (minor) A provisioner can bypass the operator-disable rule by re-disabling first.**
Location: D3 lines 174–176, D2 line 109.
Problem: `disabled_by` records whoever disabled the tenant. If a provisioner "disables" a tenant the operator already disabled,
and that overwrites `disabled_by`, the provisioner can then enable it. This defeats the n4 bound.
Required:
- Disabling an already-disabled tenant is a no-op that keeps `disabled_by`, or operator scope is sticky: once an operator
  disables a tenant, only an operator can change `disabled_by`.
- `memory_authority_` covers a provisioner disable followed by an enable on an operator-disabled tenant.

**R2 (minor) Credential issue can race namespace delete and leave a grant on a deleted namespace.**
Location: D3 lines 162–163, D13 lines 543–546.
Problem: Namespace delete takes the namespace row `FOR NO KEY UPDATE` and removes its grants. Inserting a grant only takes the
foreign key's `FOR KEY SHARE` on that row, which does not conflict with `FOR NO KEY UPDATE`. A concurrent credential issue (or
grant change) that read the namespace as active can therefore commit a grant after the delete. `GET /v1/credential` would then
list the deleted namespace, which is the tombstone exposure n4 was meant to prevent. D13 states no order for issue or grant
changes.
Required:
- Credential issue and grant changes take tenant `FOR SHARE` → namespace `FOR SHARE` (re-checking `active`) → then insert the
  credential and grants.
- Add this path to D13's per-path list.
- Test issue racing namespace delete.

**R3 (minor) The n5 check must not accept array-valued keys just because the service sets them too.**
Location: D1 lines 44–48.
Problem: "Fails on any table or key … the service config does not set itself" exempts a parent key whenever the service sets the
same key. That is right for scalars and `[env]` entries, where the nearer config wins. It is wrong for array-valued keys
(`build.rustflags`, `build.rustdocflags`, `target.<triple>.rustflags`, `target.<cfg>.rustflags`, …), which Cargo joins: the
parent's flags still reach the build. The author's own rationale rests on this concatenation. Separately, the check runs only in
`verify.sh`, but `focused.sh` results are recorded as batch evidence, so they can be produced under undetected parent
influence.
Required:
- A parent definition of any array-valued key always fails, whatever the service sets. Name the key classes the check treats as
  overriding (scalars, `[env]` entries).
- `focused.sh` runs the same check before building.
- Optionally, the probe sets `build.rustflags` in both parent and service and asserts the check still fails.

## Observations (no action required)

- n1: the batch base key can itself contain `.` and `-`. In theory, base `x.0-2` (unsplit) and base `x` (batch 0 of 2) collide.
  The adapter generates opaque bases (D11 delivery intents), so this only needs a one-line rule if bases ever come from callers:
  for example, a hex base or no `.` in the base.
- n5 probe: once `build.rustflags` is removed, the `cfg(memory_poison)` assertion checks nothing, because no other route
  carries that cfg. It is harmless, and it becomes meaningful if R3's optional both-sides case is added.

## Verdict

**Findings to resolve: three minor (R1–R3).**
- n1, n2 and n3 are resolved.
- n4 is resolved apart from the R1 bypass and the R2 race it exposed.
- n5's detection approach satisfies the independence requirement once R3 tightens its exemption rule.
- None of the findings affects the architecture. Each is a one-sentence correction to the record plus a test entry.
