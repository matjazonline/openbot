# Attribution p10 — final handoff

Status: **accepted and integrated for the verified API-first attribution scope**.
All ten authorized phases are implemented and sequentially accepted. Main workspace
is the delivery and record authority. Attribution implementation is committed as e61d384e; the user's pushed test fixes
are preserved. Publication integration with current main is independently accepted.

- Source: `55ecc5eabdcb9493aa9c5ecead843cb075554aa686be101994280bb1b38efb97`.
- Original acceptance HEAD: `6e55258cbbdc7633940805f366fb17fe240f02aa`.
- Publication review base: `e61d384ed0d8859e04b8e5fc01b335f6c19d91c2`; clean merge of
  origin/main `8fdb892b6e279ed0c09150e2336bbdc7462f877d` is accepted. Frozen merge
  digest: `3688ab5d0c47cb02398dd3504190f7dfb7a25a8f2781a84d3c9a7d218823b26d`.
- Root acceptance: **AS-P10-ACCEPT-R1**; independent Astra review: **AS-P10-B1-N2 PASS**.
  F1 is closed. Root/A is user-confirmed Sol 6.1/xhigh; B is retained Astra/high.
- Workflow: `implement-astra-reviewed-2-3`. Exact snapshot exclusions remain each
  p01–p10 `docs` directory individually plus plan `COORDINATION.md`.
- Coordination: main [COORDINATION](../../../COORDINATION.md), revision30;
  C001–C008/r1 satisfied for the agreed scope. Canonical p06–p10 records are now main.

The service has independent storage/deployment/UI, Openbot sign-in and company
access, merchant setup/keys, durable redirects, consent-gated tracking and checkout,
paid/refund reconciliation, immutable attribution, reports/CSV and operations.
Native store adapters, real-store pilot, production region/retention/SLO decisions
and production deployment remain deferred.

Attribution verification passed:120 ordinary service tests,3 separate browser
journeys,15 contract and11 boundary cases;12 newly added ordinary Openbot tests in
normal and stock-stack runs; actual SSO8checks; both strict-TLS production topologies;
production images, migrations/SQLx metadata, offline compilation/Clippy, restore,
concurrency and bounded-load cases. Main source identity exactly matches the
reviewed worktree after the guarded23-file transfer. All28 imported historical
manifests validate; final root acceptance evidence is separately validated.

**Whole-repository verification remains non-green.** Original normal tests were
4210pass/20fail/27ignored; stock-stack4207pass/23fail/27ignored with no overflow;
lease134passed. AS-P10-VG1 department-directory lifecycleConflict remains causally
unresolved: do not call it proven expiry or confirmed unrelated. Original root
workflow/markup/MCP/Clippy failures and browser stall remain recorded. Corrected
memory checks passed backup1/journey13steps/live integration6, but one extracted
memory test failed PostgreSQL shared-memory allocation (174pass/1fail/2ignored).
Plugin archives reproduce, but clean runtime installation fails lock synchronization.
Managed Doop has2701passing tests but private-S3 setup fails under its sanitized
Docker environment; dependent build/image/private-acceptance stages were not
exercised. The knowledge image checker passed after correcting its fixture mount.
No assertion was weakened or unrelated source repaired to clear these gates.

Original acceptance resources were cleaned up. Owned native PostgreSQL was stopped/removed;
owned Docker PostgreSQL and anonymous volumes removed; task Colima VM stopped.
Prepared baseline, managed-Doop and pinned-node temporary fixtures were removed.
Source, canonical evidence, isolated author worktree, dependency/build caches and
cold image cache are preserved. The small knowledge mount fixture is retained with
the cold VM cache. Full resource disposition is in the cleanup JSON.

Evidence and history:

- [Review](REVIEW.md) and [findings/verification gap](agent-review-findings.md).
- [Task record](TASK.md) and [historical handoff](logs/AS-P10-HANDOFF-history.md).
- [Root acceptance evidence](logs/AS-P10-ACCEPT-R1.evidence.json).
- [Original full-run summary](logs/AS-P10-root-full-summary-1.txt).
- [Transfer receipt](logs/AS-P10-main-transfer.json) and [final source identity](logs/AS-P10-main-final-snapshot.txt).
- [Cleanup](logs/AS-P10-owned-resource-cleanup.json).

No attribution implementation work remains under the accepted scope. A follow-up
that changes source must preserve the unresolved verification gaps and obtain
focused verification/review for its actual changes. Implementation commit e61d384e is preserved in the accepted main integration.
Independent **AS-P10-PUSH-R1-N1 PASS** and root **AS-P10-PUSH-ACCEPT-R1** accept
merged source3688ab5d with incoming main8fdb892b. All143 service files remain
identical to the previously accepted tree. Combined fmt, offline all-target
compilation, fresh migration, service build,25focused root tests and actual
SSO1test/8checks pass. The initial linker disk-full failure is retained; the
retry uses debug0/incremental0 with unchanged code and assertions. See
[publication evidence](logs/AS-P10-PUSH-R1.evidence.json). Whole-repository gaps
above remain; no fresh global-green claim is made.

Owned publication PostgreSQL is stopped/removed, and no check/reviewer remains
active. User explicitly authorized the merge commit and non-force push to main.
The delivery merge contains this handoff; production deployment remains deferred.
