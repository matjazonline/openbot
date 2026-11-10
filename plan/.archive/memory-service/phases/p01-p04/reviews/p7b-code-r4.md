# P7b code review r4 (r3-m1 only) and overall P7b verdict — reviewer B generation 5

Nonce `run=e471aa3f role=B gen=5`, agent `a217255500990fff2`. Date 2026-11-08.

## Artifact identity

- `tmp/memory-service-run/frozen/p7b-code-r4.diff` sha256
  `c1717d208b7c6ed769276c8f30f28b63a7bd135eabb0a3ab14b628dd5128cc33` (verified).
- `p7b-r4-incremental-from-r3.diff` sha256
  `0da76dfddf71d20d9af16aded7660c0d80ccbe9bbda11d4a403b6ff2e9419324` (verified; 7 files).
- The live tree recomputes to `113801e442204b9277285d0be088f0492757d7a6`, equal to the frozen tree.

## r3-m1: resolved

- `namespace_present` (`application/memory/lifecycle.rs`, around `:408-418`):
  - only `Ok(Deleted)` or a 409 `namespace_deleted` gives `namespace_conflict`;
  - any other 409 gives terminal `tenant_conflict` on the namespace row. That outcome is in
    `DEFINITE_FAILURES`, so lazy provisioning never re-drives or replaces it; only an owner's
    explicit retry does.
- This matches the service: a deleted tenant answers 409 `conflict`
  (`services/memory/src/adapters/postgres/mod.rs:184-186`), and our namespace tombstone answers
  409 `namespace_deleted`. The fake does the same (`contract_fake.rs:260, 310`).
- Replacing a `namespace_conflict` scope now also waits out the 600 s cool-down
  (`namespaces.rs:133-140`). The cool-down test covers a fresh tombstone (not replaced) and an
  old one (replaced).
- Settings show a tenant-level refusal when a namespace failed with `tenant_conflict`
  (`pages/company_memory.rs:84-93`).
- The new test `memory_scope_reconciler_deleted_tenant_conflict_mints_no_new_namespaces`
  covers the service-side tenant deletion: across three ensure-and-drain cycles past the
  cool-down there is one namespace row, one namespace lifecycle row, the `tenant_conflict`
  outcome, and the settings notice.

The earlier r3 conclusions still hold:
- the replacement path runs only under owner and enabled-tenant `FOR SHARE` locks, so no
  resurrection and no orphaned data;
- the retry action stays owner/admin only and refuses a disabled tenant.

**Checks run:** private cluster, own scratch DB `b5_p7b_r4_test`, dropped afterwards. Unmodified
tree, shared target.
- `memory_scope_` 30 selected, 30 passed.
- `memory_integration_` 21 selected, 21 passed (`logs/b5-p7b-r4-memory_*.log`).
- `pages::tests::company` 6/6.

Clippy and fmt are the author's evidence, not re-run by me.

## Overall P7b verdict (05A.1–2 and the lifecycle parts of 05B.7)

**PASS.** Review history: r1 found M1–M4 and m1–m5 (`p7b-code-r1.md`); r2 found r2-m1..m4
(`p7b-code-r2.md`); r3 found r3-m1 (`p7b-code-r3.md`). All are resolved, with tests and the probes
recorded in those files. The deadlock probes were re-run against the production statements in r2.
The substep-1 findings were rechecked by the other reviewer (R-n1 resolved in r2).

What holds:
- **Tenant and namespace mapping.** Tenant external id = company UUID; namespace ids are random.
  A missing or foreign owner is refused (`OwnerUnavailable`/`NotEnabled`) and never widened.
- **Per-company opt-in (U1)**, owner/admin only, with a concealed denial, revision checks and
  CSRF enumeration. Disable bumps the epoch; enable and retry revive no epoch.
- **Lazy provisioning** in its own transaction.
- **Owner hooks** for company, agent, member and account deletion, under one lock order. It is
  verified by competing tests in both orders and by two-session probes.
- **The fenced reconciler:**
  - create-then-delete, where only a 200 settles;
  - 404 → terminal `tenant_unmanageable`;
  - mismatch refusal before any remote call, on every path;
  - outage retries unbounded but visible as stuck, and definite failures terminal;
  - a fresh budget per generation;
  - stray revocation;
  - credentials sealed under fence and generation;
  - supervised shutdown.
- **Operator surfaces:** the startup warning, the docs query and re-drive, and settings notices.

## Remaining checks to carry (not run in P7b)

- **Browser check of the company Settings memory section in both themes** (pages AGENTS), on
  computed styles. Include the busy states of the enable, disable and retry buttons.
- **Live-service tests** `memory_integration_live_` through `scripts/memory-integration.sh`
  against the built service (05 gate, E11). Not written yet.
- **Full OpenBot suite and `scripts/stack-budget.sh`** (05 gate and 06C). Also the
  transport-boundary pre-existing failure (`channel_reply_continuation_views_support.rs:59`), to be
  reported at the final gate.
- **P7c obligations:**
  - the caller of `ensure_namespace` stays outside the run transaction, with connection
    accounting;
  - disable fails pending intents, and owner hooks cancel intents (E9 test
    `memory_integration_disable_races_save_and_capture_then_reenable`);
  - permit contention between the delivery pool and lifecycle work.
- **06A carries:**
  - lifecycle-row retention (settled rows are kept forever);
  - the 1,000-credential ceiling, which needs a label-filtered or paginated service
    `credentials/list`;
  - `managed_by` reassignment, which must keep the recorded provisioner id consistent: otherwise
    the 404/409 settles in `namespace_absent` would falsely complete deletions;
  - provisioner rotation.
