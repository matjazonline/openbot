# p06 review record

| Round | Stage | Batch | Reviewer | Artifact digest | Outcome | Notes / findings |
| --- | --- | --- | --- | --- | --- | --- |
| MEM-P06-A1-R1 | architecture gate (revision 1) | p06-A1 (service retention, purge, forgetting, reassignment, credential listing) | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | 7ce18038c31f60d29ebd6570f3b65c2868b4dc80cfc1f7ca7d10dc295cbe21e6 (gate text TASK.md sha256 59505897…bead4) | CHANGES REQUIRED: 4 Minor actionable (F1–F4), 2 optional (F5, F6); no Major. The decisions are right in shape | [notes](agent-review-notes.md) MEM-P06-A1-R1; [findings](agent-review-findings.md) |
| MEM-P06-A3-R1 | architecture gate (revision 1) | p06-A3 (OpenBot retention, maintenance driver, rotation adoption, absent steps, credential paging) | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | 7ce18038c31f60d29ebd6570f3b65c2868b4dc80cfc1f7ca7d10dc295cbe21e6 (same gate text) | CHANGES REQUIRED: 2 Minor actionable (F1, F2), 2 optional (F3, F4); no Major. A3-4 and A3-6 also wait on A1 | [notes](agent-review-notes.md) MEM-P06-A3-R1; [findings](agent-review-findings.md) |
| MEM-P06-A2-R1 | architecture gate (revision 1) | p06-A2 (service metrics, correlation, log hygiene, OpenBot metrics) | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | 7ce18038c31f60d29ebd6570f3b65c2868b4dc80cfc1f7ca7d10dc295cbe21e6 (same gate text) | CHANGES REQUIRED: 2 Minor actionable (F1, F2), 3 optional (F3–F5); no Major | [notes](agent-review-notes.md) MEM-P06-A2-R1; [findings](agent-review-findings.md) |
| MEM-P06-A1-R2 | gate recheck (revision 2) | p06-A1 | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | 7ce18038c31f60d29ebd6570f3b65c2868b4dc80cfc1f7ca7d10dc295cbe21e6 (gate text TASK.md sha256 2c053242cc3c81b5…) | CHANGES REQUIRED: F1–F4 fixed, F5 and F6 taken; new F7 (Minor, actionable: the tenant mark does not wait for label blanking), new F8 optional | [notes](agent-review-notes.md) MEM-P06-A1-R2; [findings](agent-review-findings.md) |
| MEM-P06-A3-R2 | gate recheck (revision 2) | p06-A3 | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | 7ce18038c31f60d29ebd6570f3b65c2868b4dc80cfc1f7ca7d10dc295cbe21e6 (gate text TASK.md sha256 2c053242cc3c81b5…) | CHANGES REQUIRED: F1 and F2 fixed, F3 and F4 taken; new F5 (Minor, actionable: the adoption probe cannot confirm a tombstoned tenant) | [notes](agent-review-notes.md) MEM-P06-A3-R2; [findings](agent-review-findings.md) |
| MEM-P06-A2-R2 | gate recheck (revision 2) | p06-A2 | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | 7ce18038c31f60d29ebd6570f3b65c2868b4dc80cfc1f7ca7d10dc295cbe21e6 (gate text TASK.md sha256 2c053242cc3c81b5…) | **PASS** for p06-A2 revision 2: F1 and F2 fixed, F3–F5 taken; new F6 optional (capacity label code) | [notes](agent-review-notes.md) MEM-P06-A2-R2; [findings](agent-review-findings.md) |
| MEM-P06-A1-R3 | gate recheck (revision 3) | p06-A1 | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | 7ce18038c31f60d29ebd6570f3b65c2868b4dc80cfc1f7ca7d10dc295cbe21e6 (gate text TASK.md sha256 b5c3606b5c77c1d8…) | **PASS** for p06-A1 (R1–R3): F7 fixed, F8 taken; new F9 optional (refusal code in the other-credential race); no open actionable findings | [notes](agent-review-notes.md) MEM-P06-A1-R3; [findings](agent-review-findings.md) |
| MEM-P06-A3-R3 | gate recheck (revision 3) | p06-A3 | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | 7ce18038c31f60d29ebd6570f3b65c2868b4dc80cfc1f7ca7d10dc295cbe21e6 (gate text TASK.md sha256 b5c3606b5c77c1d8…) | **PASS** for p06-A3 (R1–R3): F5 fixed (one `manages_tenant` probe: credentials/list first, create on its 404); no open findings | [notes](agent-review-notes.md) MEM-P06-A3-R3; [findings](agent-review-findings.md) |
| MEM-P06-A2-R3 | confirmation (revision 3, optional F6) | p06-A2 | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | 7ce18038c31f60d29ebd6570f3b65c2868b4dc80cfc1f7ca7d10dc295cbe21e6 (gate text TASK.md sha256 b5c3606b5c77c1d8…) | **PASS** stands: F6 taken (label `capacity_exceeded`), nothing reopened; no open findings | [notes](agent-review-notes.md) MEM-P06-A2-R3; [findings](agent-review-findings.md) |
| MEM-P06-B1-R1 | batch review | p06-B1 (A1 service retention/purge/reassign/listing, A3 OpenBot retention and rotation, A2 observability, B6 carries, F16) | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | ab43fef61423e106ebd010a9c2e90ec720dc1c72216931797360d69b8da80e38 (frozen tree 5ae80918) | CHANGES REQUIRED: 1 Minor actionable (F1: `tenant_absent` fails on a 404 after an unsuccessful create, against A3 rev2's rule), 1 optional (F2 a–f). Service suite 170/0; OpenBot `memory_` 163/0/3; no reviewer mutation probes (11–15 GiB free) | [notes](agent-review-notes.md) MEM-P06-B1-R1; [findings](agent-review-findings.md) |
| MEM-P06-A4-R1 | architecture gate (revision 1) | p06-A4 (auth breaker, restore without replay, backup check, ops docs, overload/budgets, smoke check, stack bounds) | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | ab43fef61423e106ebd010a9c2e90ec720dc1c72216931797360d69b8da80e38 (gate text TASK.md sha256 149b20877be108b5…) | CHANGES REQUIRED: 2 Minor actionable (F1 restore reconciles absent rows only; F2 breaker and after-restore tests, 06A focused-check map), 1 optional (F3); no Major | [notes](agent-review-notes.md) MEM-P06-A4-R1; [findings](agent-review-findings.md) |
| MEM-P06-B1-R2 | correction recheck (F1, F2a–f) | p06-B1 | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | 541b35adc32d0ea9ad97823cc8bbc0aa7326d3eef9d0a0f04a9da4afd7bb9351 (freeze tree 757148d9) | CHANGES REQUIRED: F1 fixed; F2 (a)–(e) taken, (f) not taken (reason accepted); new F3 (Minor: the F1 test fails clippy `await_holding_lock`). Focused: service 38/0, OpenBot 82/0 | [notes](agent-review-notes.md) MEM-P06-B1-R2; [findings](agent-review-findings.md) |
| MEM-P06-A4-R2 | gate recheck (decision revision 2) | p06-A4 | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | 541b35adc32d0ea9ad97823cc8bbc0aa7326d3eef9d0a0f04a9da4afd7bb9351 (freeze tree 757148d9) | CHANGES REQUIRED: F1 resolved in substance, F2 fixed, F3 taken; new F4 (Minor: A4-8 clears before re-issuing, and the delivery claim then fails the intents A4-9 retries), new F5 optional | [notes](agent-review-notes.md) MEM-P06-A4-R2; [findings](agent-review-findings.md) |
| MEM-P06-B1-R3 | correction recheck (F3) | p06-B1 | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | 7e5ee98d00640cb0cb575e3815488266c7f708859bc8ad60fba77a6fa9729b51 (freeze tree 5f080b3b) | **PASS** for the whole batch p06-B1 (R1–R3): F3 fixed (reviewer clippy `-D warnings` exit 0); no open B1 findings | [notes](agent-review-notes.md) MEM-P06-B1-R3; [findings](agent-review-findings.md) |
| MEM-P06-A4-R3 | gate recheck (decision revision 3: F4, F5) | p06-A4 | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | 7e5ee98d00640cb0cb575e3815488266c7f708859bc8ad60fba77a6fa9729b51 (freeze tree 5f080b3b) | **PASS** for p06-A4 (revisions 1–3): F4 fixed (issue first, fenced swap, stays `ready`), F5 taken; no open A4 findings | [notes](agent-review-notes.md) MEM-P06-A4-R3; [findings](agent-review-findings.md) |
| MEM-P06-B2-R1 | batch review | p06-B2 (A4: auth breaker, after-restore, backup check, operations docs, overload/load, OpenBot A4-8/A4-9 and restore SQL) | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | a83ffe8402206d2cbee9aa6856c4666fc130a5f500bb3dffa42e08bef8e9afd1 (freeze tree fb435320) | **PASS** for p06-B2; optional F1 (breaker test timing, static gauge). Service 174/0/2, backup check 1/0, OpenBot `memory` 167/0/3; no reviewer mutation probes (disk at the floor) | [notes](agent-review-notes.md) MEM-P06-B2-R1; [findings](agent-review-findings.md) |
| MEM-P06-B3-R1 | batch review | p06-B3 (06B.1 standalone journey, 06B.2 `memory_e2e_`, 06B.3 topology and isolation, 06B.4 competing claimants, B2-F1, U8/U9 paid smoke) | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | b86dc1e8992a694729b968f3b9b5511100f6cd4e8ea39f154ed55e33d0f64b0e (freeze tree 540efd56) | CHANGES REQUIRED: 2 Minor actionable (F1 isolation check fails on a fresh, unmigrated OpenBot database, as in CI; F2 provenance and production-image coverage), 1 optional (F3 a–e); B2-F1 resolved. Journey 13/13; live script 6/0 only after a manual migration; focused service tests 3× ok | [notes](agent-review-notes.md) MEM-P06-B3-R1; [findings](agent-review-findings.md) |
| MEM-P06-B3-R2 | correction recheck (F1, F2, F3a–e) | p06-B3 | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | 3617afd03fefe97f517200b0a4a266940ab3441f0e27c9f5b8b4d20f2c249405 (freeze tree 607df2cd) | **PASS** for the whole batch p06-B3 (R1–R2): F1 fixed (no-grant probe table; reviewer live script on a never-migrated database 6/0), F2 fixed (raw-fetch provenance; image journey recorded unavailable, carried to 06C), F3 a–e taken. Journey 13/13; race test 3×; clippy/fmt clean | [notes](agent-review-notes.md) MEM-P06-B3-R2; [findings](agent-review-findings.md) |

## MEM-P06-B3-R2

- **PASS** for p06-B3 (digest 3617afd0…9405, freeze tree 607df2cd).
- [MEM-P06-B3-F1](agent-review-findings.md#mem-p06-b3-f1--the-isolation-check-needs-openbots-schema-which-nothing-creates-so-cis-live-job-fails-first) and [MEM-P06-B3-F2](agent-review-findings.md#mem-p06-b3-f2--two-06b-items-have-no-real-service-evidence-and-are-not-recorded-as-uncovered): fixed.
- [MEM-P06-B3-F3](agent-review-findings.md#mem-p06-b3-f3--optional-strengthening): (a)–(e) taken.
- The image journey and the separate-host run are recorded as unavailable and carried to 06C; neither is claimed.

## MEM-P06-B3-R1

- **CHANGES REQUIRED** for p06-B3 (digest b86dc1e8…4b0e).
- Actionable:
  - [MEM-P06-B3-F1](agent-review-findings.md#mem-p06-b3-f1--the-isolation-check-needs-openbots-schema-which-nothing-creates-so-cis-live-job-fails-first) — the service-role isolation probe queries OpenBot's `companies`, which a fresh test database (CI's) lacks, so `memory-integration.sh` exits 1 before any live test.
  - [MEM-P06-B3-F2](agent-review-findings.md#mem-p06-b3-f2--two-06b-items-have-no-real-service-evidence-and-are-not-recorded-as-uncovered) — source provenance and the production image: add the evidence or record them as uncovered.
- Optional: [MEM-P06-B3-F3](agent-review-findings.md#mem-p06-b3-f3--optional-strengthening) (a–e).
- [MEM-P06-B2-F1](agent-review-findings.md#mem-p06-b2-f1--two-robustness-notes-on-the-breakers-tests-and-gauge): resolved.

## MEM-P06-B2-R1

- **PASS** for p06-B2 (digest a83ffe84…afd1).
- Optional: [MEM-P06-B2-F1](agent-review-findings.md#mem-p06-b2-f1--two-robustness-notes-on-the-breakers-tests-and-gauge) — make the breaker test independent of wall-clock timing; read the gauge from the shared breaker instance.

## MEM-P06-A4-R3

- **PASS** for p06-A4. Fixed: [MEM-P06-A4-F4](agent-review-findings.md#mem-p06-a4-f4--a4-8-clears-the-stored-credential-before-re-issuing-so-the-delivery-claim-fails-the-very-intents-a4-9-keeps-retrying). Taken: [MEM-P06-A4-F5](agent-review-findings.md#mem-p06-a4-f5--a-401-also-means-a-tenant-deleted-out-of-band).

## MEM-P06-B1-R3

- **PASS** for p06-B1 (R1–R3). Fixed: [MEM-P06-B1-F3](agent-review-findings.md#mem-p06-b1-f3--the-f1-test-fails-clippy---all-targets--d-warnings).

## MEM-P06-A4-R2

- Resolved: [MEM-P06-A4-F1](agent-review-findings.md#mem-p06-a4-f1--the-restore-procedure-leaves-credentials-and-tenant-state-from-after-the-backup-unreconciled) (in substance), [MEM-P06-A4-F2](agent-review-findings.md#mem-p06-a4-f2--the-verification-names-no-test-for-the-breaker-or-for-after-restore-in-the-regular-suite-and-does-not-map-the-06a-focused-checks), [MEM-P06-A4-F3](agent-review-findings.md#mem-p06-a4-f3--three-points-to-state-in-the-breaker-and-restore-decisions) (taken).
- Actionable (Minor, new): [MEM-P06-A4-F4](agent-review-findings.md#mem-p06-a4-f4--a4-8-clears-the-stored-credential-before-re-issuing-so-the-delivery-claim-fails-the-very-intents-a4-9-keeps-retrying) — issue the new credential first and swap it in one fenced completion (or make the claim wait), so no retrying intent fails `namespace_unavailable`.
- Optional (new): [MEM-P06-A4-F5](agent-review-findings.md#mem-p06-a4-f5--a-401-also-means-a-tenant-deleted-out-of-band) — document that a tenant deleted out of band is a 401 too.

## MEM-P06-B1-R2

- Resolved: [MEM-P06-B1-F1](agent-review-findings.md#mem-p06-b1-f1--tenant_absent-still-fails-definitively-on-a-delete-404-after-a-create-that-did-not-succeed) (fixed), [MEM-P06-B1-F2](agent-review-findings.md#mem-p06-b1-f2--small-conformance-and-robustness-points) (a)–(e) taken, (f) not taken with an accepted reason.
- Actionable (Minor, new): [MEM-P06-B1-F3](agent-review-findings.md#mem-p06-b1-f3--the-f1-test-fails-clippy---all-targets--d-warnings) — scope the F1 test's `MutexGuard` and rerun clippy `--all-targets -D warnings`.

## MEM-P06-A4-R1

Actionable (Minor):
- [MEM-P06-A4-F1](agent-review-findings.md#mem-p06-a4-f1--the-restore-procedure-leaves-credentials-and-tenant-state-from-after-the-backup-unreconciled) — credentials issued after the backup break every client call after the restore (a 401 becomes `failed(authority)`, which drops queued intents). Revocations (admin included) and disables after the backup are undone. Re-drive every lifecycle row updated since the backup, and verify or re-issue stored credentials before deliveries resume.
- [MEM-P06-A4-F2](agent-review-findings.md#mem-p06-a4-f2--the-verification-names-no-test-for-the-breaker-or-for-after-restore-in-the-regular-suite-and-does-not-map-the-06a-focused-checks) — name a competing-worker breaker test and a regular-suite after-restore test, and map the 06A focused checks to tests.

Optional:
- [MEM-P06-A4-F3](agent-review-findings.md#mem-p06-a4-f3--three-points-to-state-in-the-breaker-and-restore-decisions) — `billing_error` in the breaker; a single probe when half-open; re-sent open intents as a restore limit.

## MEM-P06-B1-R1

Actionable (Minor):
- [MEM-P06-B1-F1](agent-review-findings.md#mem-p06-b1-f1--tenant_absent-still-fails-definitively-on-a-delete-404-after-a-create-that-did-not-succeed) — `tenant_absent` maps any delete 404 to `tenant_unmanageable`, even after a create 409. Either conform to A3 revision 2 (postpone through `not_ours`) or record the deviation, with a test either way.

Optional:
- [MEM-P06-B1-F2](agent-review-findings.md#mem-p06-b1-f2--small-conformance-and-robustness-points) — (a) gauge postponed rows by outcome; (b) map provider error kinds to a closed set; (c) the re-record race after source removal; (d) the tenant delete lock order; (e) the CI toolchain components; (f) recall in OpenBot's hygiene test.

## MEM-P06-A2-R3

- **PASS** stands. Taken: [MEM-P06-A2-F6](agent-review-findings.md#mem-p06-a2-f6--the-capacity-refusal-label-names-a-code-the-response-does-not-carry); nothing reopened.

## MEM-P06-A3-R3

- **PASS** for p06-A3. Fixed: [MEM-P06-A3-F5](agent-review-findings.md#mem-p06-a3-f5--the-adoption-probe-cannot-confirm-a-tombstoned-tenant-so-its-rows-stay-postponed-after-rotation).

## MEM-P06-A1-R3

- **PASS** for p06-A1. Fixed: [MEM-P06-A1-F7](agent-review-findings.md#mem-p06-a1-f7--the-tenant-mark-does-not-wait-for-label-blanking-so-a-large-tenant-can-keep-labels). Taken: [MEM-P06-A1-F8](agent-review-findings.md#mem-p06-a1-f8--three-small-points-left-open-by-revision-2).
- Optional (new): [MEM-P06-A1-F9](agent-review-findings.md#mem-p06-a1-f9--another-credentials-refusal-code-differs-from-what-a-fresh-execution-gets) — the other-credential race should answer 403 `forbidden`, as a fresh execution does, not `not_found`.

## MEM-P06-A2-R2

- **PASS** for p06-A2 revision 2. Fixed: [MEM-P06-A2-F1](agent-review-findings.md#mem-p06-a2-f1--the-log-hygiene-test-can-pass-without-capturing-anything), [MEM-P06-A2-F2](agent-review-findings.md#mem-p06-a2-f2--the-affected-contracts-are-not-listed-and-three-of-them-are-underspecified). Taken: F3, F4, F5.
- Optional (new): [MEM-P06-A2-F6](agent-review-findings.md#mem-p06-a2-f6--the-capacity-refusal-label-names-a-code-the-response-does-not-carry) — the capacity refusals' label should read `capacity_exceeded`, not `unavailable`.

## MEM-P06-A3-R2

- Fixed: [MEM-P06-A3-F1](agent-review-findings.md#mem-p06-a3-f1--failed-is-not-a-final-run-state-the-retention-of-a-save-rests-on-an-unstated-run-deadline-bound), [MEM-P06-A3-F2](agent-review-findings.md#mem-p06-a3-f2--the-documented-rotation-order-makes-lifecycle-passes-in-the-window-fail-definitively). Taken: F3, F4.
- Actionable (Minor, new): [MEM-P06-A3-F5](agent-review-findings.md#mem-p06-a3-f5--the-adoption-probe-cannot-confirm-a-tombstoned-tenant-so-its-rows-stay-postponed-after-rotation) — `tenants/create` answers 409 for every tombstone, so rows of a deletion in progress at rotation are postponed forever. Probe with `credentials/list` instead, falling back to create on 404.

## MEM-P06-A1-R2

- Fixed: [MEM-P06-A1-F1](agent-review-findings.md#mem-p06-a1-f1--the-removals-marking-steps-lock-a-parent-after-its-children-and-the-stated-invariant-says-they-never-do), [MEM-P06-A1-F2](agent-review-findings.md#mem-p06-a1-f2--two-new-races-have-no-competing-claimant-test-and-one-has-no-rule), [MEM-P06-A1-F3](agent-review-findings.md#mem-p06-a1-f3--the-limits-of-deletion-omit-copies-in-other-namespaces-and-restore-from-backup), [MEM-P06-A1-F4](agent-review-findings.md#mem-p06-a1-f4--free-text-credential-labels-are-kept-forever-as-if-they-were-opaque-ids). Taken: F5 (checked closely; acceptable once the D9 amendment is recorded), F6.
- Actionable (Minor, new): [MEM-P06-A1-F7](agent-review-findings.md#mem-p06-a1-f7--the-tenant-mark-does-not-wait-for-label-blanking-so-a-large-tenant-can-keep-labels) — the tenant mark's `NOT EXISTS` must include "no unblanked label", or a tenant with more than 500 credentials can keep labels.
- Optional (new): [MEM-P06-A1-F8](agent-review-findings.md#mem-p06-a1-f8--three-small-points-left-open-by-revision-2) — another credential's concurrent identical purge, other clients' deletes after a restore, the D9 amendment.

## MEM-P06-A2-R1

Actionable (Minor):
- [MEM-P06-A2-F1](agent-review-findings.md#mem-p06-a2-f1--the-log-hygiene-test-can-pass-without-capturing-anything) — the hygiene test needs a positive control (the expected request, operation and worker lines are captured) and a refused request whose marker serde would quote.
- [MEM-P06-A2-F2](agent-review-findings.md#mem-p06-a2-f2--the-affected-contracts-are-not-listed-and-three-of-them-are-underspecified) — the affected contracts are not listed (OpenAPI, snapshot, schema, OpenBot's port, the fake), and three are underspecified: correlation on `/mcp`, immutability of the operation's `correlation_id`, and the counter and `x-request-id` for capacity refusals.

Optional:
- [MEM-P06-A2-F3](agent-review-findings.md#mem-p06-a2-f3--per-process-counters-sit-beside-database-wide-gauges) — counters are per process, but the gauges are database-wide.
- [MEM-P06-A2-F4](agent-review-findings.md#mem-p06-a2-f4--lease-losses-would-count-cancel-requested-renewals) — a cancel-requested renewal would count as a lease loss.
- [MEM-P06-A2-F5](agent-review-findings.md#mem-p06-a2-f5--one-id-overstates-the-link-to-a-later-recall) — "one id" is really a join path.

## MEM-P06-A3-R1

Actionable (Minor):
- [MEM-P06-A3-F1](agent-review-findings.md#mem-p06-a3-f1--failed-is-not-a-final-run-state-the-retention-of-a-save-rests-on-an-unstated-run-deadline-bound) — a `failed` run can be retried with the same execution key. A3-1's safety relies on the 24 h run deadline and does not say so; put it in the predicate and test it.
- [MEM-P06-A3-F2](agent-review-findings.md#mem-p06-a3-f2--the-documented-rotation-order-makes-lifecycle-passes-in-the-window-fail-definitively) — "reassign, then switch the token" makes every affected lifecycle pass fail definitively. Document "switch, then reassign", and handle new namespaces created in the window.

Optional:
- [MEM-P06-A3-F3](agent-review-findings.md#mem-p06-a3-f3--monitoring-counts-scan-whole-tables-and-the-sweep-order-defeats-its-index) — full-table gauges, the sweep order against its index, EXPLAIN on small tables.
- [MEM-P06-A3-F4](agent-review-findings.md#mem-p06-a3-f4--document-the-open-ended-waits-and-the-adoption-edges) — document waits on a disabled tenant, an operator token as provisioner, probe-created tenants, and retrying failed rows.

## MEM-P06-A1-R1

Actionable (Minor):
- [MEM-P06-A1-F1](agent-review-findings.md#mem-p06-a1-f1--the-removals-marking-steps-lock-a-parent-after-its-children-and-the-stated-invariant-says-they-never-do) — marking a namespace or tenant removed locks the parent after its children, against D13; the invariant at TASK.md:155 is false as written.
- [MEM-P06-A1-F2](agent-review-findings.md#mem-p06-a1-f2--two-new-races-have-no-competing-claimant-test-and-one-has-no-rule) — two identical purges (the loser is refused inside its transaction, so the replay rule misses it) and reassign against an in-flight request have no competing tests.
- [MEM-P06-A1-F3](agent-review-findings.md#mem-p06-a1-f3--the-limits-of-deletion-omit-copies-in-other-namespaces-and-restore-from-backup) — the limits of deletion omit copies in other namespaces and restore from backup.
- [MEM-P06-A1-F4](agent-review-findings.md#mem-p06-a1-f4--free-text-credential-labels-are-kept-forever-as-if-they-were-opaque-ids) — free-text credential labels are kept forever; credentials have no row in the retention table.

Optional:
- [MEM-P06-A1-F5](agent-review-findings.md#mem-p06-a1-f5--finished-operations-and-source-metadata-of-a-live-namespace-grow-without-a-bound) — operation and source metadata grow for a namespace's whole life.
- [MEM-P06-A1-F6](agent-review-findings.md#mem-p06-a1-f6--small-contract-gaps-in-reassignment-and-listing) — the type of `remaining`, `from = to`, admin row order, a label index, N+1 grant queries.
