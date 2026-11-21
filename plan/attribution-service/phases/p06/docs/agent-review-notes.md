# p06 reviewer notes

## AS-P06-G1-N1 — architecture gate — 2026-11-16

Identity and exact seven exclusions: [REVIEW](REVIEW.md). B is
`/root/attribution_reviewer`, requested Astra/high with independent context; actual
model/effort and context occupancy are not exposed. Workflow
`implement-astra-reviewed-2-3`, batch p06/B1, architecture stage. Worktree
`/Users/mac02/orca/workspaces/openbot/attribution-service`, branch
`attribution-service`, start/base/HEAD `2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`.
Reviewed source digest `a03abfeaf183952cbbf5c178939823e441b9355fcc2e151618527ee2d18d1a4d`;
current decision AS-P06-D1/r2 SHA256
`4dbef90e5d7e2534198e4cb61d2f8b4f8f2f2e18f9d3433c460bad28f689492a`.

### Coverage and evidence

Read original phase 06 website/checkout requirements and service wire contract,
Observation/Checkout schemas and response reasons, and repository/service rules.
Reviewed all nine decision items and planned AC1–AC8 mapping in
[immutable r2](logs/AS-P06-D1-r2.md), with original r1 retained. Independently traced
existing producers and authority boundaries through Trail and exact source spans:
`adapters/setup/postgres.rs:437–464` public installation/tenant/domain scope;
`adapters/setup/merchant.rs:109–137` current key/store/scope and atomic last use;
`adapters/catalog/postgres.rs:321–358` server click and frozen dimensions;
`adapters/http.rs:195–228` existing security wrapper. All paths here are relative
to `services/attribution/src`. Examined callers and indexed click references to
check that p06 adds consumers of accepted p04/p05 owners; p07 ledger and p08
report readers do not yet exist.

The gate covers durable observation identity, timestamp horizon/uniqueness retention,
active installation and exact origin checks, image acknowledgement/HEAD and commit
uncertainty, admission, merchant pre-body authorization, immutable checkout identity,
selection timing/ties/classification, snapshot ownership, consent and bounded SDK
storage/retries/visibility, and representative merchant server/browser boundaries.
Pixel timestamp amendment is explicit in the decision and AS-C005/r1; subsequent
OpenAPI/docs/examples must change together. Existing nullable click/link fields
still require at least one related reference. r2 clarifies that consent alone on a
visit without captured/retained click produces no landing observation, and that an
approved frozen destination is not collected visitor-page URL data. These are
resolved wording questions, not established defects. The inspected r1/r2 delta
contains only those clarifications and revision label.

Coordination authority is `/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md`,
writer `/root`. Fully re-read revision 14 at completion and byte-compared it with
[immutable pin](logs/AS-P06-D1-coordination-r14.md). AS-C001/r1 API-first/dedicated
login, AS-C002/r1 consent/no IP/window/freshness/503, AS-C003/r1 shared setup/key
boundaries, AS-C004/r1 frozen clicks/principal identity are preserved. Proposed
AS-C005/r1 captures p06 protocol and downstream revalidation/retention obligations.
Downstream phases require accepted p06 implementation, not this gate.

Executed: SHA256 of both immutable decisions; r1/r2 diff; coordination authority/pin
comparison; branch/HEAD; start/end snapshot comparison under identical exclusions;
`git diff --check`. All PASS. See [identity/check log](logs/AS-P06-G1-reviewer-identity.txt)
and [end snapshot](logs/AS-P06-G1-reviewer-end.snapshot.txt).

AC1–AC8 are a verification design, not executed evidence: real HTTP/PostgreSQL
scoping and durable responses, competing dedupe/freeze and pruning, actual COMMIT
acknowledgement loss, exact time selection, browser consent/navigation/storage/tab
races and visibility, merchant fixture, affected shared/contract/build/migration
checks must be inspected with implementation. No test runner or runtime environment
was exercised here and no implementation manifest is due at an architecture gate.
Full-plan verification and deployment/pilot decisions remain later work; this pass
does not waive repository final-suite requirements. No area of the proposed gate
was deliberately omitted, but implementation correctness remains unverified.

### Outcome

**PASS AS-P06-D1/r2.** No concrete findings or unresolved blocking questions.
No findings log is needed for a clean gate. Source remained accepted p05 throughout.
Root may accept the decision and begin dependent p06 implementation. No source,
phase or runtime acceptance is claimed.

### Process and instruction feedback

One cohesive gate was appropriate for the new persistent event/checkout identities,
retry/pruning protocol and consent/cross-tab boundary. The explicit decision revision
and retained delta resolved two wording ambiguities without inventing defects or
requiring implementation evidence prematurely. No instruction change is supported
by this round alone. This note preserves review conclusions, not private reasoning.

## AS-P06-B1-N1 — implementation review — 2026-11-16

Identity: round AS-P06-B1-R1, whole p06/B1, workflow `implement-astra-reviewed-2-3`.
Reviewer `/root/attribution_reviewer`, requested Astra/high independent context;
actual model/effort/context occupancy unavailable. No implementation authoring or
delegation. Worktree `/Users/mac02/orca/workspaces/openbot/attribution-service`,
branch `attribution-service`, phase start/base/HEAD
`2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`. Starting artifact
`0f0023424964d9a62ee5a6439de67d1193a7eba606b19d2efd7c500cac914747`;
predecessor p05 `a03abfeaf183952cbbf5c178939823e441b9355fcc2e151618527ee2d18d1a4d`.
Gated D1/r2 remains AS-P06-G1-N1. Exact seven exclusions in REVIEW unchanged.
Authoritative coordination `/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md`,
writer `/root`, revision16 and active AS-C001–C005/r1 re-read and matched to
`logs/AS-P06-B1-coordination-r16.md` at review boundaries.

### Coverage and evidence

Re-read original06A/B, root/service AGENTS, gated decision, current TASK acceptance/
consumer mapping, contracts and review references. Reviewed application/website,
website HTTP/PostgreSQL, reset schema, SDK, merchant browser fixture and all new
HTTP/PG tests/browser driver. Reviewed shared catalog snapshot extraction/router
composition, SRI/pixel contract tests, OpenAPI shapes/descriptions, WEBSITE/CI.
Trail callers confirm public_installation, begin_merchant and link_dimensions owners
and consumers; click producer and SQL readers inspected. Existing p01–p05 work is
preserved; shared integration checks were inspected, without restarting whole-plan
review. No portion of new p06 production implementation was deliberately omitted.

AC1: typed/unknown/duplicate rejection, sizes, exact active scope/origin/related IDs,
observation labels and no sale route. AC2/3: normalized fingerprints, original-time
horizons, pixel/HEAD, durable acknowledgement and real COMMIT-loss relay, competing
receipts, admission, installation-disable serialization in both orders. AC4/5:
pre-body key scope, current transaction/revocation/last-use, immutable checkout
receipts/evidence, server time and inclusive30d/nanosecond/tie/reason behavior,
ineligible-latest fallback and freeze. AC6/7: actual SDK URL capture, consent,
history/expiry/withdrawal/retries, visibility, merchant-controlled identity/cookie/
server-only key and privacy assertions. AC8: schema, shared owners, contracts,
documentation and CI composition. Findings expose gaps in AC3/6/7 despite green
existing checks.

Executed independently from service cwd with Rust1.95, `/tmp/as-attribution-target`,
local PostgreSQL maintenance role mac02 and invocation-owned UUID fixtures:

- `cargo test --locked --offline --lib website_tests -- --nocapture`:11 PASS,
  browser1 ignored; [output](logs/AS-P06-B1-reviewer-website-tests.txt).
- `cargo test --locked --offline --lib attribution_checkout_handoff_browser_ -- --ignored --nocapture`:
  1 PASS/11 emitted attempts with pinned external Playwright Chromium module;
  [output](logs/AS-P06-B1-reviewer-browser-tests.txt).
- Two actual Chromium tabs with unmodified SDK and controlled CDP interleaving:
  F1 reproduced; retained probe/output linked from finding. Separate VM interleaving
  confirms the same loss. The probe204 endpoint is a declared stand-in; the full
  existing browser journey independently uses production service and PostgreSQL.
- `check-evidence.py .../AS-P06-B1-R1.evidence.json --expect-artifact 0f0023424964d9a62ee5a6439de67d1193a7eba606b19d2efd7c500cac914747`:
  valid8 PASS entries; retention validation alone does not prove coverage.
- Snapshot start/end, coordination pin and `git diff --check`: coordination/check
  PASS; source snapshot MISMATCH described below.

Inspected author retained outputs: catalog14 PASS; identity/setup35 PASS with unchanged
setup browser ignored; foundation12+2 PASS including fresh migrations, formatting,
offline all-target compilation/metadata and source boundary10; contract15 PASS;
syntax/diff/Trail checks. No test moves/deletions; test helper visibility preserves
discovery. Asserted behavior traced through actual production paths: the wire relay
observes database COMMIT before suppressing acknowledgement; competing disable
fixtures observe real PG blocking; timestamp case distinguishes nanosecond rounding.
Earlier author build/fixture failures remain documented with later corrective
results in TASK. The current manifest summarizes acceptance checks only.

The browser hidden-document case overrides visibilityState to exercise that branch;
it is not an actual OS occlusion test. Representative merchant fixture uses simulated
identity authority and local HTTPS. Paid-order revalidation remains p07 and retention
operation p09; native platforms, production deployment and full-plan/container
acceptance remain later work. No repository verification requirement is waived.

### Outcome and artifact mismatch

**CHANGES REQUIRED**, AS-P06-B1-F1 and F2, both P2 OPEN, persisted together in
[findings](agent-review-findings.md). F1 is independently reproduced; F2 is concrete
control-flow inspection with no separate runtime reproduction claim. All owned
fixture/probe processes completed and cleaned up.

End snapshot changed to `965ec148d90defe65da73aba25159cd1ee9c1478b838029ca712a1b67db72a35`;
file list newly includes `scripts/check.sh` and `scripts/check-support.sh`. B edited
only authorized review/log paths and notified root. Starting identity matches the
assigned artifact; changed current identity requires ownership reconciliation and
refreeze, not expanded exclusions. No current-artifact acceptance is claimed.

### Process and instruction feedback

Batch size remains appropriate. Architecture invariants remain valid: F1 arises from
cleanup implementation, not approval of destructive cleanup at G1. Per-click keys
plus sequential tab tests failed to establish race safety; exercise the actual
read/cleanup overlap and next durable consumer (scoped candidate lesson in F1).
F2 requires testing an existing criteria intersection, not a new broad instruction.

Root subsequently confirmed the two runner-file changes are external to this task,
will preserve/reconcile them and refreeze including them before affected recheck.
No additional reviewer acceptance is implied by that acknowledgement.

## AS-P06-B1-N2 — correction recheck — 2026-11-16

Round AS-P06-B1-R2, whole p06/B1 correction delta, workflow
`implement-astra-reviewed-2-3`; independent B `/root/attribution_reviewer`, requested
Astra/high, actual model/effort/context telemetry unavailable. No authoring/delegation.
Active worktree and p06 record authority moved to
`/var/folders/y1/v99y__1j741fh8z9fqd8rvlc0000gn/T/as-attribution-author-7fo_yda4/worktree`,
detached HEAD, phase start/base/current commit
`2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`. Current digest
`c5f2ac256934858ba3b656f9158657d3160436b2188ee2027c555a52cdd438e6`.
Exact seven relative exclusions retained; no runner/source exclusion was added.
Main coordination authority remains
`/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md`,
writer/root, revision17; AS-C001–C005/r1 unchanged. Authority, isolated copy and
`logs/AS-P06-B1-R2-coordination-r17.md` match at both boundaries. Transfer manifest
inspected; isolated runner paths match HEAD. Main task's external edits preserved;
combined final integration must reconcile them. Gated decision D1/r2 unchanged.

### Coverage and evidence

Reused original06/D1 requirements and unaffected R1 review coverage. Inspected
correction SDK historyEntries, all HTTP admission/CORS branches and common outer
request boundary, new actual HTTP competing-limit regression, browser CDP regression,
SRI/WEBSITE statements and new scoped Trail learning. No new architecture decision
or changed acceptance criterion required. Prior catalog/setup/foundation evidence
is reused for unchanged source, not claimed re-executed in this round.

F2 correction independently passes: origin validation is under existing outer32
admission/deadline; body/ingestion remains behind public16 permit; verified-origin
public rejection passes through CORS. Invalid/unknown origin and unavailable lookup
fail closed. The real HTTP fixture holds16 requests at body reads, confirmed by
100-continue, and tests known-origin429/Retry-After/ACAO/Vary, no credentials,
unknown-origin403, no writes and recovery. Global pre-origin exhaustion cannot
establish origin and remains outside this narrower response-policy promise; explicit
TASK disposition inspected without broadening ingestion limits.

F1 correction independently passes the original after-read/before-cleanup interleaving
and keeps returned history bounded32. However, fresh concurrent writes before storage
read are still deleted by a cached `now` in both validity and retained-selection
predicates. Reproduced with real Chromium tabs and current production SDK, pausing
A after clock capture then granting B. This leaves the same F1 OPEN; full details,
probe and durable navigation assertions are in the findings follow-up. No new finding
ID or new requirement used. The changed production browser regression only schedules
the later cleanup boundary and therefore passes despite this remaining loss.

Independent checks, invocation-owned UUID PostgreSQL fixtures, Rust1.95 and shared
service target cache, external pinned Playwright module:

- `cargo test --locked --offline --lib website_tests -- --nocapture`:12 PASS,
  browser1 ignored; `logs/AS-P06-B1-R2-reviewer-website-tests.txt`.
- `cargo test --locked --offline --lib attribution_checkout_handoff_browser_ -- --ignored --nocapture`:
  1 PASS; `logs/AS-P06-B1-R2-reviewer-browser-tests.txt`.
- Real two-tab earlier-clock probe: remaining F1 reproduced; retained
  `logs/AS-P06-B1-R2-reviewer-browser-clock-race.{mjs,txt}`. Endpoint204 is a stand-in;
  SDK/storage/scheduling are real. Existing full journey separately exercises PG.
- R2 evidence manifest validator:9 PASS entries retained/valid. Inspected current
  contract15/fmt/offline all-target results and retained failure corrections. This
  is evidence retention/inspection, not independent reruns of those commands.
- R2 start/end source snapshots byte-match exact c5f2ac artifact; main coordination
  pin comparison and HEAD PASS; `git diff --check` PASS.

All owned fixtures/probes completed and cleaned. No required check reported
unavailable. Full-plan/root/container integration remains p10; no requirement waived.

### Outcome and process feedback

**CHANGES REQUIRED: F1 OPEN/P2, F2 FIXED.** No phase/implementation acceptance.
The isolated worktree resolves R1's artifact-identity problem without hiding source.
F1's narrowly scheduled regression proved the original failure was corrected but
not the entire cross-tab invariant; reader clock capture is another concurrency
boundary. Scoped lesson appended to F1. Preserve prior findings/notes and the
original successful architecture gate; this is an implementation recheck failure.

## AS-P06-B1-N3 — final correction recheck — 2026-11-16

Identity: AS-P06-B1-R3, p06/B1 correction stage, workflow
`implement-astra-reviewed-2-3`; reviewer `/root/attribution_reviewer`, requested
Astra/high independent context; actual model/effort/context occupancy unavailable.
No implementation authoring or delegation. Active worktree/phase records:
`/var/folders/y1/v99y__1j741fh8z9fqd8rvlc0000gn/T/as-attribution-author-7fo_yda4/worktree`.
Detached HEAD, phase start/base/current commit
`2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`; exact whole-source digest
`fd2a235453db086980456eaf541657997df46111ddd476632bba09c8cd3a2991`.
Exact seven relative exclusions remain p01–p06 docs and plan COORDINATION as listed
in REVIEW; no implementation path excluded. Gated AS-P06-D1/r2 unchanged.

Coordination authority `/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md`,
writer/root, revision17, active AS-C001–C005/r1 unchanged. Byte-compared main authority
to `logs/AS-P06-B1-R3-coordination-r17.md` at start and end. Isolated source stayed
at its frozen base despite main's independent test-fix commit6e55258c; combined
integration is explicitly not included in this review.

### Coverage and evidence

Reviewed F1 completion against original06B.3/5, AC6/7 and gated candidate-preservation
invariant. Trail used before source inspection. Inspected the entire current SDK
read/validation/retention/eviction sequence and the changed browser assertions,
expiry/future-input/bound behavior, plus WEBSITE SRI. Storage input validation now
uses a clock after each value read, and retention uses a clock after the whole scan;
sampled-key eviction still prevents deletion of unseen writes. No policy, schema,
HTTP or authorization change. F2's N2 closure and unaffected R1 whole-batch coverage
are reused rather than re-reviewing unchanged code.

Actual browser regression schedules both competing cases in distinct contexts:
B writes before A enumerates/validates and B writes after A scans/before cleanup.
Each then navigates B and checks both retained IDs from storage. The first case
separates time by30ms, so it discriminates the R2 stale-clock bug. Original expiry,
bound and withdrawal checks remain, and a one-day-future stored entry must be
excluded and removed. These assertions exercise actual SDK/service/PG and local
HTTPS merchant paths, with existing simulated identity authority limitation.

Executed independently in isolated service cwd, Rust1.95, shared target cache,
owned UUID PostgreSQL fixtures and pinned Playwright Chromium:

- `cargo test --locked --offline --lib attribution_checkout_handoff_browser_ -- --ignored --nocapture`:
  1 PASS, including both schedules/expiry/future/bound and prior merchant journey;
  `logs/AS-P06-B1-R3-reviewer-browser-tests.txt`.
- `/tmp/as-p02-contract-clean/bin/python services/attribution/contract/test_contracts.py`:
  15 PASS including current pinned SDK SRI;
  `logs/AS-P06-B1-R3-reviewer-contract-tests.txt`.
- `node --check` for SDK/browser and `git diff --check`: PASS.
- R3 evidence validator:9 retained PASS entries valid. Inspected author browser-6/
  contract-3 outputs. Unchanged R2 website12, fmt/offline all-target and prior shared
  catalog/identity/setup/foundation evidence reused; not claimed rerun in R3.
- R3 reviewer start/end snapshots byte-match the exact assigned digest; isolated
  HEAD unchanged; main coordination pin comparison PASS.

No test disappeared; expanded cases remain in the existing discoverable browser
test. All owned fixtures/processes completed and cleaned. No required affected
check unavailable. Full-plan/root/container gates remain p10, not waived.

### Outcome and process feedback

**PASS fd2a235453db086980456eaf541657997df46111ddd476632bba09c8cd3a2991**,
combining R1 whole-batch review with R2/R3 corrections. F1 independently FIXED;
F2 remains FIXED. No unresolved actionable findings; root may accept p06 at this
identity. No combined-main, full-plan or deployment acceptance claimed.

The two-schedule regression now tests the persistence invariant across both read
validation and cleanup boundaries. Retained F1 lesson/failure history remains useful;
no further broad instruction change supported. Batch/recheck scopes were appropriate,
and isolated record/source authority prevented the external main task from silently
changing the reviewed artifact.
