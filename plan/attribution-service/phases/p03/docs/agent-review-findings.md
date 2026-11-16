# Attribution p03 reviewer findings

## AS-P03-G1-F1 — Clock rollback can extend the status authorization budget

- Date: 2026-11-16. Workflow: implement-astra-reviewed-2-3. Phase p03,
  architecture gate AS-P03-G1, note AS-P03-G1-N1.
- Reviewer `/root/attribution_reviewer`; Astra/high requested, actual telemetry
  unavailable. Worktree `/Users/mac02/orca/workspaces/openbot/attribution-service`.
- Reviewed decision AS-P03-D1/r1, SHA256
  `d139936c19eb01c044b924d7a3971b41777f93f87b68c82c99bbba67cf7c8b0f`;
  source digest `f4421ac9eb35f50acf92d710948bf4e3ca777104821000a4c0e296d1ac53c10d`.
- Classification: actionable architecture/authorization defect, **medium (P2)**.
  Origin is the gated decision, before implementation.
- Status: **open; blocks this decision's acceptance**.

The accepted contract, `services/attribution/contract/README.md:123–130`, requires
at most five minutes from successful request start and explicitly says local clocks
and HTTP deadlines must not extend that budget. AS-P03-D1/r1:137–142 instead selects
the service DB clock for the durable observation/deadline and limits monotonic time
to the HTTP request. Neither that section nor the recovery/verification sections
define a rollback-safe age bound for subsequent uses of stored status.

Concrete counterexample: a successful active observation starts at database time
12:00 and stores validity until 12:05. The authority then becomes unreachable and
a suspension notification is missed. At four real minutes the DB clock steps back
two minutes. At six real minutes its time is 12:04, so the proposed deadline still
authorizes tenant operations even though the approved maximum age has elapsed.
The two-second monotonic HTTP timeout does not participate in that cached read and
cannot prevent this. Reading/checking the same DB clock removes inter-host skew,
but not backward-clock extension. Delayed responses and version ordering guards
address different failure paths.

Required correction: revise the architecture to specify how the maximum elapsed
status age remains bounded despite backward wall-clock changes, including what
happens on restart or another service process reading an existing observation.
Map this to deterministic rollback/elapsed-age and recovery tests, in addition to
the existing request-start, delayed-response and stale-version cases. The author
owns the mechanism; this finding does not prescribe an implementation.

Instruction lesson: the accepted contract already states the invariant. A generic
new instruction is not supported by this one architecture review; verify elapsed
authorization budgets across the entire observation lifetime, not only transport.

Resolution history: opened 2026-11-16 against r1. No correction reviewed yet.

### Resolution — 2026-11-16 — AS-P03-G1-N2

**Resolved at the architecture-decision level** in AS-P03-D1/r2, SHA256
`6968a1aa538ec5817939a92198ff2f111da8a581ac7bd17a1d2529645430b722`.
Source remains `f4421ac9eb35f50acf92d710948bf4e3ca777104821000a4c0e296d1ac53c10d`.

The revised decision requires a process-local monotonic ticket captured before
database/network waits, in addition to persisted state/version/deadline. Tickets
cannot be reconstructed from stored timestamps or inherited by another process;
restart, expiry and eviction require a successful trusted refresh, failing closed
on outage. Writers recheck the monotonic deadline after obtaining the tenant lock.
Database sequence ordering replaces wall-time response ordering; lifecycle pushes
cannot mint tickets, and current persisted version/state still gates every use.
The correction therefore prevents the original backward-clock counterexample
without weakening lifecycle invalidation or live user-authority checks.

Verification criteria now explicitly cover rollback plus elapsed monotonic expiry,
no write after expiry, restart and second instances with apparently fresh persisted
timestamps, late responses, stale sequence delivery, eviction, and lock waits past
expiry. Inspection only: these are required implementation assertions, not tests
executed in this gate. No remaining design issue under F1; runtime proof is due in
the implementation batch review.

## AS-P03-B1-F1 — Logout retries fail instead of completing the public logout contract

- Date 2026-11-16; workflow implement-astra-reviewed-2-3; phase p03, batch B1,
  implementation review R1, note AS-P03-B1-N1. Reviewer `/root/attribution_reviewer`,
  Astra/high requested, actual telemetry unavailable; same worktree/base as REVIEW.
- Artifact `45fc0ad49012ab1f6f58a619c06508430011b0c3a127683196c79ea906c71897`.
- Actionable correctness/contract defect, **P2 (medium)**; **open**.

`IdentityService::logout` (`services/attribution/src/application/identity.rs:313–320`)
uses the live-only `SessionStore::session` lookup before revoking. Once the first
logout commits, an identical retry is rejected at the lookup, before CSRF validation
or cookie clearing. The accepted README:194–196 requires idempotent targeted logout.
The route additionally returns 204 (`adapters/identity/http.rs:282–291`), whereas
the accepted attribution OpenAPI `/auth/logout` success response is 302.

The reviewer standalone-server probe created a real local session using a bounded
HTTP authority fixture, performed logout, then replayed the original cookie/CSRF as
if the first response were lost: first response 204, retry 401, no clearing cookie.
The user's JS consequently reports sign-out failure and cannot complete a lost-response
retry. Evidence: [probe source](logs/AS-P03-B1-R1-reviewer-session-probe.py),
[results](logs/AS-P03-B1-R1-reviewer-session-probe.txt). This is actual service code
with an invocation-owned real PostgreSQL database; the authority is a stand-in.

Correct the targeted logout recovery while preserving cookie/CSRF validation and
local-first behavior during upstream outage. Align the success status and redirect
with the published contract, or make an explicit reviewed contract change rather
than silently diverging. Test same-target retry after local commit/lost response,
including cookie removal; preserve rejection of invalid CSRF. The existing logout
test asserts only the first 204 and therefore codifies the contract mismatch.

Instruction lesson: existing contract-to-assertion guidance applies; a successful
first mutation does not establish its promised ambiguous-response recovery. No new
global instruction proposed. Resolution history: opened against B1-R1; no fix reviewed.

### F1 resolution — 2026-11-16 — AS-P03-B1-N2

**Fixed** at `fd40f4807955c2e19c00ab9d864c2a0ce84566088e99743b84f9338a7ce7d30c`.
Logout now uses a narrowly named retained-session lookup to recover encrypted CSRF
material for the targeted session, while authentication retains its live-only
lookup. Revocation remains local-first; repeat CSRF is still validated. The route
returns the published 302 to `/` and clears the cookie on the retry too.
Inspected and reran the focused logout test, including invalid-CSRF replay and
upstream outage. The independent standalone probe now observes first302/retry302,
same redirect and cookie clearing; see R2 reviewer-runtime-probe source/output.
No public contract change or weakened authentication boundary was required.

## AS-P03-B1-F2 — Expired or revoked browser sessions strand the HTML entry point

- Same date/workflow/reviewer/worktree/base/artifact as F1; note AS-P03-B1-N1.
- Actionable browser correctness defect, **P2 (medium)**; **open**.

`services/attribution/src/adapters/identity/http.rs:370–393` renders the sign-in
page only when parsing the cookie reports Unauthorized. A correctly shaped cookie
whose session is expired, revoked or no longer known proceeds to `authenticate` or
`company`, and its Unauthorized result becomes a JSON 401. There is no sign-in link,
redirect or clearing cookie. The browser cookie lasts up to eight hours, whereas
the local idle limit is thirty minutes, so ordinary idle expiry reaches this case.

The standalone runtime probe set a real local session's idle deadline in the past:
GET `/` with the existing cookie returned 401, no `/auth/login` link and no cookie
removal; the same URL without the cookie returned the usable 200 sign-in shell.
The same dead end follows the lost-logout response in F1. Evidence is the session
probe linked above. API 401 behavior is appropriate and should remain distinct.

Make HTML entry/settings navigation recover to a usable sign-in flow when the
local session or upstream identity is definitively unauthorized, clearing the
unusable local cookie as appropriate. Preserve retryable authority-outage behavior
instead of treating 503 as logout. Add a real browser or HTTP-shell regression for
idle expiry and upstream revocation, and prove API endpoints still return 401.

Instruction lesson: trace expiry/revocation through the browser consumer, beyond
the persistence/API assertion. Resolution history: opened against B1-R1; no fix reviewed.

### F2 resolution — 2026-11-16 — AS-P03-B1-N2

**Fixed** at the R2 digest above. HTML shell loading now catches only Unauthorized
and renders the sign-in page with local cookie removal; API401 and authority503
remain distinct. Inspected/reran tests covering `/` and `/settings` after real local
idle expiry and stubbed upstream revoke, plus outage preserving the cookie/session.
The independent standalone probe observes200, sign-in link and clearing cookie for
the original idle-expiry case. Inspected the current real-browser eight-check proof:
upstream logout is followed by page reload, visible sign-in and absent local cookie.
Evidence: R2 reviewer-service/runtime-probe and AS-P03-B1-R2-browser logs.

## AS-P03-B1-F3 — A valid maximum company page exceeds the authority client's body cap

- Same date/workflow/reviewer/worktree/base/artifact as F1; note AS-P03-B1-N1.
- Actionable interoperability/bounds defect, **P2 (medium)**; **open**.

`services/attribution/src/adapters/identity/authority.rs:6,28–52` rejects response
bodies above 65,536 bytes. Both published CompanyPage schemas permit 100 companies
with 120-character names, and the service exposes `limit=100`. The Openbot producer
uses PostgreSQL `left(company.name,120)` (characters) and normal UTF-8 JSON;
`CompanyWrite::normalize` allows such Unicode names.

A compact 100-item page with 120 four-byte Unicode characters in each name is
73,530 bytes, before any non-null cursor. It validates against the accepted Openbot
schema, yet the actual service `/v1/companies?limit=100` returned 503 when the
authority fixture supplied it. Evidence: [schema validation](logs/AS-P03-B1-R1-reviewer-page-schema.txt),
[runtime probe](logs/AS-P03-B1-R1-reviewer-page-probe.py),
[results](logs/AS-P03-B1-R1-reviewer-page-probe.txt). This is a valid payload
rejected by transport bounds, not an oversized-contract input.

Reconcile the response budget with all permitted serialized pages while retaining
a hard response-size limit. Add an enduring maximum-page/Unicode boundary test
through HttpAuthority and retain an over-limit rejection test. Per repository rules,
raising this bound must leave a CI guard that fails as its budget is approached.

Instruction lesson: existing verification guidance explicitly asks for maximum
caller page size; character limits and UTF-8/wire-byte budgets must be reconciled.
Resolution history: opened against B1-R1; no fix reviewed.

### F3 resolution — 2026-11-16 — AS-P03-B1-N2

**Fixed** at the R2 digest above. HttpAuthority uses a hard128KiB response budget.
The new regression derives page/name/cursor bounds from the accepted schema,
exercises100 maximum names using four-byte Unicode and six-byte JSON control
escapes, and fails early at80% of the transport cap. Valid JSON padded past the cap
must still be rejected. Inspected the decisive assertions and independently reran
the test. The original73,530-byte runtime probe now returns200. CI's full service
test selection includes this guard. The scoped l-20f04c5f learning records the
finding and guard; no accepted page-size contract was reduced.

## AS-P03-B1-F4 — Lifecycle failures are stored but never surfaced as operational alerts

- Same date/workflow/reviewer/worktree/base/artifact as F1; note AS-P03-B1-N1.
- Actionable requirements/observability gap, **P2 (medium)**; **open**.

Original phase03 batch03C item4 requires alerts for undelivered lifecycle changes
and expired status, using durable evidence rather than only process-local counters.
The implementation stores delivery_state/attempts/last_failure and tenant freshness/
last_reconcile_failure, but no production reader surfaces that persistent state.
The delivery worker logs only claim or completion-storage failures; an HTTP failure
that is successfully recorded produces no alert, even at exhausted attempts.
Reconciliation likewise silently records ordinary authority failures.

Evidence: `src/adapters/http/attribution/worker.rs:15–48`, persistence outbox
`:4–55`, service worker `:14–43`, and service tenants `:114–125`; exhaustive Trail
references to `attribution_lifecycle_outbox` and `last_reconcile_failure` lead only
to persistence operations/tests. README:145–153 defers operational views, but that
does not disposition this phase's explicit alert requirement. Durable rows make
recovery possible; by themselves they do not surface a failed revocation channel.

Add a bounded operational signal/probe backed by persisted undelivered/failed work
and expired tenant status, with appropriate operator scope. A broad phase09 UI is
not necessary. Test that outstanding state remains visible after worker/process
restart and that successful repair resolves the signal. Alternatively obtain an
explicit scoped requirement disposition before claiming AC5 complete.

Instruction lesson: the G1 decision promised persistent backlog/alert evidence but
left the actual alert consumer underspecified; N1/N2 gate coverage did not establish
that rows would be surfaced. Link this implementation finding to that earlier
assumption without rewriting the architecture pass. Resolution: open against B1-R1.

### F4 resolution — 2026-11-16 — AS-P03-B1-N2

**Fixed** at the R2 digest above. Both supervised workers now read persisted health
through a two-second bounded probe, at startup and on a30-second cadence. Structured
operator events expose undelivered/failed lifecycle work and expired/failed tenant
status, with WARN on outstanding state/unavailable observation and INFO recovery.
No global tenant-facing endpoint was added. README documents fields, cadence and
the external deployment's responsibility for hosted alert routing.
Reviewed the production query/worker/log paths and tests that reconstruct service/
bridge instances over durable state, capture warning fields, repair work and capture
healthy fields. This fulfills the narrow alert-signal requirement without requiring
the later operational UI. Evidence: R2 root/service test logs and reviewer reruns.

## AS-P03-B1-F5 — New Openbot membership SQL lacks the required real authorization regressions

- Same date/workflow/reviewer/worktree/base/artifact as F1; note AS-P03-B1-N1.
- Actionable verification gap, **P2 (medium)**; **open**.

The new bridge implements membership/permissions directly in its PostgreSQL joins
and role mapping (`src/adapters/persistence/attribution/mod.rs:53–75,157–177,183–215`).
Original phase03B and TASK AC3 require multi-company users, lost membership during
a session, role downgrade and another tenant's valid IDs. The sole new root access
test (`tests.rs:136–168`) creates one owner company, queries a random nonexistent
UUID, lists one item, then changes the password. It never changes membership/role,
creates a real foreign company, or exercises multi-company pagination. The root
HTTP lifecycle test only exercises an owner. These are new producer paths, not
calls through the existing CompanyUseCases whose regressions were run.

The service access test toggles a stub response and uses a random missing UUID
(`services/attribution/src/adapters/identity/http_tests.rs:231–268`); it proves the
consumer reacts to upstream permissions but cannot prove the new Openbot SQL
produces them correctly. Browser evidence has a single owner/company. Therefore
the TASK PASS summary overstates producer-side AC3 evidence, although source
inspection did not establish a current authorization leak.

Add real PostgreSQL/bridge tests using a retained live grant while membership is
downgraded and removed, owner/admin/member permissions (including status writes),
a valid inaccessible company ID, and a multi-company user's scoped list/access.
Keep the consumer stand-in tests and label their proof separately in the mapping.
Use actual fixture/production mutations rather than substituting another fixed
authority response.

Instruction lesson: the existing verification-contract instructions already require
producer/consumer tracing and explicit stand-in limits. Resolution history: opened
against B1-R1; no correction reviewed.

### F5 resolution — 2026-11-16 — AS-P03-B1-N2

**Fixed** at the R2 digest above. New real PostgreSQL/HTTP bridge fixtures create
owner/admin/member and genuinely foreign companies, obtain one real code/grant,
and reuse that grant/session through production membership downgrade/removal use
cases. Assertions cover exact permission arrays, permitted owner/admin status
writes, refused member/foreign/removed writes, unaffected own-company access, and
keyset traversal of a multi-company user's list before/after removal. These exercise
the actual new producer SQL independently of the retained consumer stub tests.
TASK now separates the original evidence gap from the correction's producer proof.
Evidence: access_tests.rs and AS-P03-B1-R2-root-1/reviewer-root logs.
