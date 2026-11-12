# Review p1-arch-r2: architecture gate P1, recheck (reviewer B gen 1)

## Artifact identity

- Reviewer: role B gen 1, nonce `run=e127dfad role=B gen=1`, agent `a636617799682ea85`, root session `6fedfb2e-40aa-486c-bd0b-051b5a3add7b`.
- Frozen artifact: `tmp/memory-service-run/frozen/p1-arch-r2.diff`, sha256 `dd90dd6e615f6dda7da4007056bd70219a796d5fbedb5a0a9c6b1c3abda6d7cf` (recomputed, matches).
  Base is `a6bed48a`. The working-tree `plan/memory-service/implementation-decisions.md` (547 lines) is byte-identical to the
  file the diff adds. `src/main.rs` still registers the path exactly once.
- Compared against p1-arch-r1 (414 lines; 455 changed lines) and my r1 record `tmp/memory-service-run/reviews/p1-arch-r1.md`.
- I applied the 2026-11-08 user decisions as fixed: (a) no provider-level gate, cap default 0; (b) no ZDR/region requirement,
  documented; (c) one provisioner credential with its authority stated and bounded.

## Coverage

I re-read the whole revised record. I rechecked each r1 finding against the actual text, not against the author's summary. I
then traced D13's lock order through every transaction the record describes: save, correct, observation delete, generation
accept, cancel, claim, dispatch CAS, heartbeat, commit, indexer claim/renew/commit, reaper, sweeper, disable/enable, tenant
delete, namespace purge, revoke, and provisioning (including the implicit FK `KEY SHARE` locks). I also checked the cancel/lease
interplay, `GET /v1/credential`, the provisioner bound, and the split `memory.save` batches.

## Recheck of r1 findings

| r1 | Status | Evidence in r2 |
| --- | --- | --- |
| M1 vector leg vs `max_scan_tuples` | Resolved | D7 lines 280–292: short means fewer than `leg_candidates` observations, for any cause. A scan cut off by `max_scan_tuples` is inconclusive and goes to the exact scan, which first counts eligible parts (`LIMIT 100001`) and otherwise reports `partial`. `max_scan_tuples` and `scan_mem_multiplier` are set explicitly. The D10 test has more than 20,000 nearer foreign and above-ceiling parts (line 444). |
| M2 lock order | Resolved (residue in n2) | D13 lines 519–534 define one global order. `tenant_generation_schedule` (line 104) replaces the cursor on the tenant row. Claim (lines 347–354), dispatch (365–366) and commit (392) take the parent rows `FOR SHARE` first. `40P01`/`40001` errors are retried. Races are tested. |
| M3 lease vs live call | Resolved | D9 lines 365–377: the local monotonic deadline is the last renewal send + 75 s, inside the 90 s lease. The heartbeat CAS requires `cancel_requested_at IS NULL`. The reaper frees the slot only after `lease_until + 30 s`. Cancel of a dispatched operation is defined (lines 387–391). Tested (line 447). |
| M4 disable/enable revival | Resolved | D3 lines 162–167: disable, delete and revoke cancel `accepted`/`claimed` operations atomically and mark `dispatched` ones. Enable revives nothing. Tested (line 438). |
| M5 parent config leakage | Resolved (residue in n5) | D1 lines 40–49 and 79–83: the service has its own `.cargo/config.toml` (`RUST_MIN_STACK`, unforced `SQLX_OFFLINE=true`), reads only `MEMORY_*` keys, and passes `--no-dotenv` with an explicit URL. A poisoned-parent probe checks this. Stack bounds are 2 MiB and move together. The macros check the `SQLX_OFFLINE` environment variable before any `.env` fallback (sqlx-macros-core 0.8.6 `query/mod.rs:120`), so the service-set value wins. |
| M6 verify DB target | Resolved | D10 lines 450–456: a disposable `memverify_*` database with an owner role, explicit `--no-dotenv --database-url`, a refusal when only `DATABASE_URL` is set, and a trap cleanup. |
| m1 seam facts | Resolved (new issue n1) | D11 lines 478–485, plus `GET /v1/credential` (D3 line 155). |
| m2 spend ledger | Resolved | D9 lines 333–335 and 355–364: the day row is upserted, settlement targets `spend_day`, an excess metric exists, and checks happen at acceptance. |
| m3 provider schema | Resolved | D9 lines 402–410: the provider-safe schema plus a stricter local `jsonschema`. |
| m4 5xx retry | Resolved | D9 lines 378–386: only Anthropic's error envelope with a `request-id` makes a retry safe. |
| m5 canonical form | Resolved | D4 lines 181–182 and 198–201, D5 lines 209–222: explicit revisions, nothing taken from mutable state, and the order authorize → lock → idempotency → expiry → derive. |
| m6 existence oracle | Resolved (residue in n4) | D3 lines 158–159, D4 line 202, D6 lines 250–254. |
| m7 omission/defaults | Resolved | D8 lines 308–315. |
| m8 test DB role path | Resolved, not merely accepted | D2 lines 124–132: migrations and tests run as an owner role, the extension is created by the superuser, `Drop` runs on a dedicated thread, and stale-pid databases are swept. |
| m9 retention | Resolved (new issue n3) | D9 lines 420–423 and D11 lines 471–473. |
| m10 provisioner | Resolved per decision (c) (residue in n4) | D3 lines 146–152 and D11 lines 490–493. |
| m11 CI/extraction | Resolved | D1 lines 31–39 and 83–86, D10 lines 460–467. |
| m12 MCP hosts | Resolved | D6 lines 239–242. |
| m13 indexer | Resolved | D7 lines 295–299, D6 line 264, D10 line 442. |
| m14 pin wording | Resolved | D1 lines 65–67. |

User decisions are applied correctly. `provider_max_level` is gone and generation stays off through the cap (D9 lines
336–340). The ZDR/region limitation is documented (D9 line 338, D12). The provisioner's transitive authority is stated honestly
and bounded to the tenants it manages: no spend caps, no admin credentials, audited.

## New findings (one pass)

All minor. No blocker or major remains.

**n1 (minor) The split `memory.save` keys are invalid, and each batch's label inputs are not pinned.**
Location: D11 line 480 vs D5 line 206.
Problem: The key form `<key>:<i>/<n>` contains `/`, which is outside the key charset `[A-Za-z0-9._:-]`. A base key near 128
bytes plus the suffix also overflows the 128-byte limit. The record does not require the split to be deterministic or every
batch to carry one frozen context. Under D4/D5 a retry with a different split, or with context recomputed after the run's
`content_level` rose, would hash differently: that is a 409 at best, and a differently labelled batch if the first attempt never
landed.
Required:
- Use a charset-valid suffix (e.g. `<key>.<i>-<n>`) with a stated base-key budget.
- Make the split a deterministic function of the frozen fact list.
- Every batch of one `memory.save` carries the identical frozen `producing_ceiling`, `consumed` and source set, so all batches
  get the same label.

**n2 (minor) D13's exemptions and per-path order are not exact.**
Location: D13 lines 523–529, D9 lines 418–419.
Problems:
- The sweeper's `DELETE … WHERE ctid IN (SELECT … LIMIT 500)` has no `SKIP LOCKED`. The D10 test plan runs competing sweepers
  (line 447), and two such statements over overlapping rows in different orders can deadlock. The statement is therefore not
  "exempt". Cascades also lock parts and links.
- The reaper does not just update one row: it settles or releases `tenant_spend_days`. That is still consistent with the order,
  but the exemption text is wrong.
- Revoke and client purge must take their higher-order parents `FOR SHARE` before their own `FOR NO KEY UPDATE`: the tenant
  row for revoke, and the tenant row for purge (then the namespace `FOR NO KEY UPDATE`, then the calling credential
  `FOR SHARE`).
- The claim locks `tenant_generation_schedule` with `FOR UPDATE SKIP LOCKED`, so that row must be created with the tenant (or
  upserted).

Required:
- Use `SELECT ctid … FOR UPDATE SKIP LOCKED LIMIT 500`, or one advisory-lock-owned sweeper.
- Correct the exemption list.
- State the parent `FOR SHARE` prefix for revoke and purge.
- Create the schedule row in tenant creation.

**n3 (minor) Source retention can race a live generation's window.**
Location: D9 lines 420–423, with acceptance at lines 327–335 and dispatch at 365–366.
Problem: Unreferenced evidence is nulled 30 days after recording. An operation accepted near that boundary that references
recorded source revisions can lose content between acceptance and dispatch. Neither rule says the sweeper re-checks for
non-terminal referencing operations under lock, nor that dispatch refuses a window with nulled content rather than sending an
incomplete window (04A.2/04A.3).
Required:
- Acceptance locks the referenced sources `FOR SHARE` (content level of D13).
- The sweeper's null-out excludes sources referenced by any non-terminal operation.
- The dispatch or load step fails the operation (`source_unavailable`, nothing sent) if any window content is missing.

**n4 (minor) `GET /v1/credential` and the provisioner bound leave two gaps.**
Location: D3 lines 146–159.
Problems:
- If grants on a deleted namespace remain listed, `GET /v1/credential` shows clients a tombstone, against "tombstones are
  visible only to admin create". It is also a client use case and therefore an MCP tool (D6 line 236).
- A provisioner can `enable` any tenant it manages, including one an operator disabled. Under decision (c) the provisioner's
  bound should not let it override an operator safety action.

Required:
- Namespace delete removes or hides its grants, so the endpoint lists only active granted namespaces.
- Record who disabled a tenant. A provisioner may not enable a tenant an operator disabled, and this is tested in
  `memory_authority_`.

**n5 (minor) The configuration-isolation check covers only `[env]`.**
Location: D1 lines 42–44 and 79–83.
Problem: The boundary check fails only on uncovered root `[env]` keys. A parent `.cargo/config.toml` with `[build]` (`rustflags`,
`target-dir`), `[target]`, `[net]` or `[alias]` would still change in-tree service builds unnoticed. The root has only `[env]`
today, so this is a latent leak, not a current one.
Required: the check fails on any table or key in a parent `.cargo/config.toml` that the service does not explicitly set. The
poisoned-parent probe adds one non-`[env]` key (e.g. `build.rustflags = ["--cfg", "memory_poison"]`) and asserts
`cfg(memory_poison)` is absent.

## Cross-point checks with no finding

- D13 with implicit FK locks: content inserts take `FOR KEY SHARE` on tenant and namespace rows, which is compatible with the
  `FOR NO KEY UPDATE` that disable, delete and purge use. No cycle.
- Two claimers for one tenant: the schedule row's `SKIP LOCKED` keeps a second claimer from reaching the operation, so the
  partial-unique wait between claimers cannot join a cycle.
- Commit vs delete: commit holds the parent `FOR SHARE` before the operation, so delete waits on the tenant row. No inversion.
- The cancel request on a `dispatched` operation halts heartbeat renewal, so `lease_until` stops advancing. The worker aborts
  within 30 s, or the reaper acts after the grace period. Commit refuses when `cancel_requested_at` is set. The slot is freed
  only after a local abort or the grace period, so the ≤ 1 in-flight per tenant property holds.
- Local deadline vs database lease: the deadline is measured from the renewal *send*, and `lease_until` is set at the later
  database `now()`. With a 15 s margin and a 30 s grace period, ordinary drift is covered.
- Authorization before idempotency replay: after a namespace is deleted, a late delivery gets 403 and never recreates content.
  The phase-05 adapter must treat that as terminal. This is already implied by D11 lines 486–487; no change needed.

## Claims (incremental)

- Verified: sqlx macros check the `SQLX_OFFLINE` environment variable before `.env` (`sqlx-macros-core-0.8.6/src/query/mod.rs:120–126`).
- Verified: cargo `[env]` from a deeper config overrides the root value, and an unforced value does not override a
  caller-exported variable. This matches D1's `prepare` override.
- r1 verifications stand (pgvector GUCs and untrusted status, rmcp 1.8.0 config and `allowed_hosts` default, Anthropic API facts
  via the skill cache).
- Unverifiable: whether a client-aborted non-streaming Messages request is still billed. The design charges the reservation, so
  it is safe either way.

## Verdict

**Findings to resolve (minor only).** All six r1 majors and all fourteen r1 minors are resolved in the record. Five new minor
findings (n1–n5) need record corrections. None changes the architecture or blocks dependent phase 01–04 implementation, except
that n1 must be fixed before the phase-05 adapter and n2/n3 before the 02B/04B workers are coded. A recheck can be limited to
those edits.
