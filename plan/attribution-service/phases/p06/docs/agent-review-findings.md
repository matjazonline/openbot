# p06 independent reviewer findings

Round identity for both findings: 2026-11-16, AS-P06-B1-R1, note AS-P06-B1-N1,
workflow `implement-astra-reviewed-2-3`, whole p06/B1 implementation review.
Reviewer `/root/attribution_reviewer`, requested Astra/high independent context;
actual model/effort telemetry unavailable. Worktree
`/Users/mac02/orca/workspaces/openbot/attribution-service`, branch
`attribution-service`, base/HEAD `2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`.
Reviewed starting artifact `0f0023424964d9a62ee5a6439de67d1193a7eba606b19d2efd7c500cac914747`,
gated decision AS-P06-D1/r2. End-snapshot mismatch is documented in REVIEW.

## AS-P06-B1-F1 — P2: candidate cleanup deletes a concurrent tab's new click

Classification: actionable correctness/concurrency defect. Source:
`services/attribution/assets/sdk-1.0.0.js:45` (read/selection at lines30–44),
existing sequential-tab fixture `services/attribution/browser/website.mjs:121`.

`historyEntries()` reads stored candidates into its own memory, selects retained
entries, then enumerates storage again and deletes every key absent from that memory.
If tab B writes its distinct click between A's read and the second enumeration,
A deletes B's new entry. This happens with only two valid unexpired candidates,
well below the 32-candidate cap. B's current memory can hide the deletion until it
navigates; its next visit then loses the latest eligible click and may attribute to
A's earlier click. This violates original06B.3/5, AC6/7 and D1/r2 item7's cross-tab
preservation invariant. Per-click keys alone do not protect cleanup.

Independently reproduced with the unmodified production SDK in two actual Chromium
tabs. CDP pauses A just before line45, B grants consent and writes its click, then A
resumes and removes it. B's return visit lacks the candidate. See
[real-browser probe](logs/AS-P06-B1-reviewer-browser-race.mjs) and
[output](logs/AS-P06-B1-reviewer-browser-race.txt). The probe service returns a204
stand-in; the demonstrated defect concerns actual browser shared localStorage, not
server acceptance. A second actual-SDK VM interleaving probe is retained under
`logs/AS-P06-B1-reviewer-storage-probe.{mjs,txt}`.

Required correction: preserve concurrently added valid candidates during cleanup
while retaining documented bounds/expiry/withdrawal. Add an enduring competing
two-tab regression interleaving write and cleanup, and verify persistence after
navigation, not just the running tab's memory. The existing browser fixture awaits
each grant sequentially and passes despite this defect.

Instruction lesson: root AGENTS already requires competing claimants for a new
concurrency protocol. Scoped candidate improvement: when shared-storage cleanup
uses a prior read, exercise a writer adding data after that read and assert the
next consumer's durable view. This is an implementation/test gap; the architecture
gate required preservation and did not approve destructive cleanup.

Resolution: **OPEN**. No correction reviewed.

## AS-P06-B1-F2 — P2: public overload skips approved-origin CORS

Classification: actionable protocol/verification defect. Source:
`services/attribution/src/adapters/website/http.rs:147–150`, versus CORS wrapper
lines63–80 and its final application at line221.

When all16 public observation permits are occupied, `observe` returns
`Failure::RateLimited.into_response()` before reading Origin, bypassing `cors`.
An otherwise approved browser's actual POST receives429 without ACAO, Vary:Origin
or exposed Retry-After. Fetch reports a CORS/network error instead of allowing the
client to inspect the retryable status/header. The SDK retries that as a generic
network failure, hiding the documented overload signal/header policy. D1/r2 item4
requires successful and failed known-origin responses to carry exact ACAO/Vary;
item8 and AC3/6 require bounded retry with readable Retry-After. Original06A.3
requires approved-origin CORS.

The overload test (`http_tests.rs:602–654`) sends only Origin-less requests, so its
green429 assertions do not cover this browser contract. Finding established by
direct control-flow inspection; no independent runtime overload+CORS reproduction
is claimed.

Required correction: make the bounded admission error path return the exact permitted
CORS headers for an approved origin, without wildcard/credentials or weakening
admission. Add a competing HTTP/browser case with approved Origin asserting readable
429/Retry-After, retaining unknown-origin rejection. Check other early-error paths
against the same promised response contract.

Instruction lesson: no broad new instruction supported. Existing AC3/6 require the
intersection of CORS and overload, not only separate successful-origin and
Origin-less-overload tests.

Resolution: **OPEN**. No correction reviewed.

## AS-P06-B1-F1 follow-up — AS-P06-B1-R2 / AS-P06-B1-N2 — 2026-11-16

**Still OPEN, P2; correction is partial.** Current artifact
`c5f2ac256934858ba3b656f9158657d3160436b2188ee2027c555a52cdd438e6`, detached
HEAD `2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`, active worktree/records
`/var/folders/y1/v99y__1j741fh8z9fqd8rvlc0000gn/T/as-attribution-author-7fo_yda4/worktree`.
Same independent B/requested Astra-high settings and workflow as original finding.

The sampled-key cleanup fixes the originally reproduced after-read/before-eviction
interleaving. Its new actual-browser regression passes, including navigation.
However, `historyEntries` captures `now` at SDK line29 and subsequently deletes any
entry whose `captured_at > now` at line37. If B writes after A captures that clock
but before A reads storage, B's valid newer click is misclassified as a future
entry and deleted. The retained-selection predicate at line44 also uses that
stale clock, so addressing only the first comparison is insufficient.

Independently reproduced in two real Chromium tabs using the unchanged current SDK:
pause A at line30 after clock capture, wait30ms, grant B and verify its click, resume
A, then navigate B. B's stored click is deleted and its return-visit handoff lacks
it. Only two valid unexpired candidates exist. This is another interleaving of the
same cross-tab lost-update defect, not a new requirement or architectural gate.

Evidence: [probe](logs/AS-P06-B1-R2-reviewer-browser-clock-race.mjs) and
[output](logs/AS-P06-B1-R2-reviewer-browser-clock-race.txt). CDP controls scheduling;
production SDK/localStorage are real, observation endpoint is a204 stand-in, same
scope limitation as R1's focused reproduction. The full existing browser/service/PG
journey independently passes but tests only the later cleanup boundary.

Required completion: preserve legitimate writes made after a reader's clock sample
through both validation and retention/eviction, while retaining expiry and future
input handling. Add this before-read competing-tab case alongside the existing
before-cleanup case, and verify persisted history after navigation. Review the
whole read/validation/eviction sequence rather than just the final enumeration.
The R2 Trail learning accurately describes the fixed original interleaving, but
should not be interpreted as proof that all concurrent-write deletion is resolved.

Instruction lesson follow-up: extend F1's scoped concurrency lesson to include the
reader's captured time and validation phase, not only cleanup's key snapshot.

## AS-P06-B1-F2 resolution — AS-P06-B1-R2 / AS-P06-B1-N2 — 2026-11-16

**FIXED** in `c5f2ac256934858ba3b656f9158657d3160436b2188ee2027c555a52cdd438e6`,
same R2 worktree/identity above. `observe` and preflight retain the permit result,
validate the supplied origin, then enforce admission before body/ingestion inside
the exact-origin CORS wrapper. Approved origins receive429, Vary and exposed
Retry-After; unknown/invalid origins fail without ACAO. Public ingestion16, tenant8,
shared outer admission32/deadline remain intact; no limit raised.

Reviewed and independently executed new
`attribution_website_event_competing_browser_overload_exposes_cors_and_retry_after`:
16 actual HTTP requests hold public slots;100-continue confirms each handler reached
body polling. Known-origin POST/HEAD/OPTIONS429 headers, unknown-origin403/no ACAO,
zero writes and recovery are asserted. Invalid preflight method gets exact-origin
403. See [12-test independent output](logs/AS-P06-B1-R2-reviewer-website-tests.txt).

The outer global admission/deadline acts before origin policy is established and
may fail without ACAO; this inherited fail-closed envelope is explicitly documented
in TASK and remains bounded. This resolution covers the narrower public ingestion
admission defect, not a claim that CORS can be established during global exhaustion
or unavailable origin validation. No remaining defect established for F2.

## AS-P06-B1-F1 resolution — AS-P06-B1-R3 / AS-P06-B1-N3 — 2026-11-16

**FIXED** in exact artifact
`fd2a235453db086980456eaf541657997df46111ddd476632bba09c8cd3a2991`.
Active worktree `/var/folders/y1/v99y__1j741fh8z9fqd8rvlc0000gn/T/as-attribution-author-7fo_yda4/worktree`,
detached HEAD/base `2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`;
independent B/requested Astra-high and workflow unchanged.

Reviewed the full `historyEntries` sequence: each persisted value is validated
against a timestamp read after fetching that value; retention uses a timestamp
read after the storage scan; eviction remains restricted to sampled valid keys.
Thus a legitimate write before its read is no longer rejected by an earlier clock,
and a write after the scan is not removed by a fresh enumeration. Existing expiry,
future-input rejection and32-candidate handoff/eviction behavior remain intact.

Independently executed the expanded production service/PostgreSQL/Chromium journey:
pausing before enumeration/validation and separately before cleanup, granting the
other tab, resuming and navigating that tab preserves both persisted candidates.
The before-read case includes30ms separation to distinguish old clock sampling.
Existing31-day expiry/32-candidate assertions plus new one-day-future rejection and
removal assertions pass. This verifies the subsequent durable consumer, not only
running-tab memory. See [independent browser output](logs/AS-P06-B1-R3-reviewer-browser-tests.txt).
Independent15 contract checks including updated SDK SRI also PASS.

No residual defect established for F1. F2 remains FIXED from N2, unaffected here.
Original failure and partial-correction history are preserved above.
