# Remaining execution expansion

Original numbered plans and their nested requirements remain authoritative. This compact queue
adds implementation seams and checks without replacing those requirements. 01 and 02.1–02.6
library scope are accepted; preserve their uncommitted work and evidence. Resume authorized 2026-09-27.

## Resume reconciliation (2026-09-27)

Re-read original 01–10 criteria and BRIEF-02.6; the queue below covers the remaining selected
scope. Current skill topology supersedes older resume notes: root Astra/low coordinates and
accepts; implementer Astra/medium owns code; its independent Astra/medium child reviews actual
changes while implementation stops. No production/runtime completion is inferred from library tests.

02.7 adds lifecycle commands around the existing draft, publication and binding owners. Save
bounded invalid source without pretending validation succeeded; edits use expected revisions.
Validate/publish share decoder/compiler/freeze diagnostics. Publication command identity is stable,
replay returns the original result only for an equivalent request and after current authorization.
Archive prevents new selection while retaining immutable history and admitted snapshots. Binding
configuration creates a revision; activation checks current scoped readiness and an expected
lifecycle revision, deactivation stops future admission without cancelling runs. Atomic ports must
close read/write races and reject stale state without partial changes. 03.1 owns normalized SQL
records/adapters and real competing publication/configuration/activation/admission transactions;
until those pass, production activation and CAS remain explicitly unverified. Phase04 owns actual
provider-use secret resolution/revocation. Do not introduce an alternate persistence owner.

No unresolved cross-point architecture decision was found. Local port shapes are reconciled at
02.7 implementation; original nested criteria remain mandatory. Cutover 10.4–10.8 remains blocked
on an identified target and operational authorization. No commit/reset/deployment is authorized.

## Shared contracts and checks

- Ownership and existing replacement sites: REPLACEMENT-MAP.md. Domain decisions stay pure;
  application owns narrow required ports; SQL/provider/HTTP implementations stay in adapters.
- Versions own immutable bundles, bindings own revisioned runtime selections, runs own snapshots,
  executions own activation/results, jobs own fenced attempts, actions own effect truth, decisions
  own human settlement, deliveries own provider receipts. Never create a second state owner.
- A frozen dependency is content, not permission. Current membership, resource authorization,
  readiness and revocation must still be checked. No secrets in bundles, prompts or traces.
- Each point: self-audit, formatting/whitespace, locked offline all-target check and Clippy,
  focused tests at stock 2 MiB, independent Astra/medium review. SQL changes additionally require
  isolated migrations, SQLx metadata and real competing-claimant tests. Phase boundaries run
  original broader acceptance gates; reuse unaffected evidence. No reset/commit/deploy authorized.
- Ordered dependencies follow the PROGRESS queue. Later implementation details are reconciled at
  point start; do not invent production persistence contracts ahead of their owning point.

## Phase 02 (after accepted 02.1–02.3)

| Point | Deliverable / seams | Acceptance and decisions |
| --- | --- | --- |
| 02.4 Publication item 2 | New application `workflow/publication` immutable bundle builder around registry/compiler; agent/skill/tool snapshots, profiles, approved action policies, pinned child bundles, dependency hashes, resource declarations | Derive compiler facts from captured content (no independent conflicting facts). Bounded deterministic snapshot/hash. Reject missing, duplicate, cross-company, recursive or incompatible dependencies. Freeze supplied finite agent catalogue for dynamic selectors; selected agents must resolve within it. Children are already-built immutable bundles; derive schemas/closure from them. Resource slots freeze kind/contract, not credentials. Mutation of original facts or later publication cannot change prior content/hash. Persistence/lifecycle commands remain 02.7/03. |
| 02.5 Publication item 3 | Binding revision types/validation in application workflow, resource-readiness port | One version, validated params, exact declared slots; current tenant/kind/contract/provider readiness checked on activation. Revision update affects future admissions. Test missing/extra/foreign/incompatible/revoked resources and competing revisions with owning adapter. Depends 02.4. |
| 02.6 Publication item 4 | Admission snapshots in workflow contracts/service/ports | Pin binding revision/params/bundle; credentials remain references. Use-time secret/revocation port contract, no credential copying. Test post-admission edits cannot mutate snapshots; effects enforced later by 04. Depends 02.5. |
| 02.7 Publication item 5 | Save/validate/publish/archive and binding lifecycle commands, cohesive persistence ports; adapters with owning schema work | Expected revisions, idempotent publication, reauthorization on replay, archive blocks future selection only; real race/rollback evidence at persistence integration. No HTTP business logic duplication. Depends 02.4–02.6. |
| 02.8 Publication item 6 | Representative source/frozen-fact fixtures and compiler tests; docs | Reviewed, autonomous (omitted classifier/profile), routing, repeated review and MCP typed downstream references. Fixtures compile, reject specified invalid variants, reused by later handlers. Phase02 immutable publication and source diagnostic gate. Depends 02.7. |

## Phase 03 — application workflow runtime, persistence/workflow, migrations and workers

Each row depends on earlier rows; normalized records and FK/scoped uniqueness own identity.

| Point | Deliverable and acceptance |
| --- | --- |
| 03.1 Admission/context | Atomic source+logical binding admission, immutable input/history boundary, first job; independent message runs; dedup survives binding updates; real concurrent admission/rollback tests. |
| 03.2 Execution item 1 | ID-only jobs; activation inputs frozen once independently of attempt; retry/crash tests. |
| 03.3 item 2 | Bounded pure-step transaction batches and continuation; trace each route/output, budget yields work. |
| 03.4 item 3 | Fenced leases/heartbeat; external I/O outside transactions; cancel actual future; competing claims/stale commits. |
| 03.5 item 4 | Atomic result/run/audit/successor commit; unique activation; crash injection on both sides. |
| 03.6 item 5 | Atomic wait+notification and event consumption+continuation; duplicate/event-before-park tests. |
| 03.7 item 6 | Run-first lock order; parent wakeup events; concurrent child completion/cancel deadlock tests. |
| 03.8 item 7 | PostgreSQL polling correctness, optional wakeups; lost notification/restart tests. |
| 03.9 Failure/recovery | Classified bounded retry/deadlines/sweepers/root budgets; explicit safe retry/cancel commands; poison batch, fairness, size/activation limits and full phase03 DB race/crash gate. |

03.1 reconciliation and production capability matrix: BRIEF-03.1.md. Reuse existing job/attempt
owners with audited discriminator exclusion; retain authenticated actor through prepared admission.
Persist versioned bounded exact publication content, failing restore on identity/compiler drift.
Keep command-key conflicts plus binding/source uniqueness; pin committed history membership,
not timestamp/allocated-sequence cutoffs. Actual resource authorities are mandatory; missing
HTTP/policy/profile/skill owners fail closed pending04/06, with no invented parallel grant store.
This limits supported production capability claims, not SQL atomicity/race acceptance obligations.
Initial03.1 SQL fragment adds Postgres definition/version/event persistence, transactional actor
reauthorization, draft/archive CAS, immutable publication replay and real lifecycle races. Evidence
and partial reviewer result are in BRIEF-03.1; binding/admission/history/job integration remains
pending, so03.1 has not passed its acceptance gate. Use explicit discriminator predicates across
legacy job readers/claims/controls/recovery rather than a second view/table owner.
Third03.1fragment (UNREVIEWED due agent thread capacity): crate-level test composition wiring
fixes the dependency guard; admission binding port now carries TriggerRef for canonical-source
aliases; normalized binding heads/revisions/events schema and FK integrity tests added.
Production binding lifecycle/readiness and all SQL admission/job/history work remain pending.
BRIEF-03.1 records exact evidence and immutable applied migration20260927213000.

## Phase 04 — shared action service and provider adapters

### Concrete04.5 receipt/replay reconciliation (ROOTaccepted2026-09-30 12:57Z)

[BRIEF-04.5.md](BRIEF-04.5.md) specifies the affected contracts and acceptance matrix.
Successful dispatch exposes only a committed bounded receipt; restart/model/step replay
uses that receipt under current access without transport. Immutable first marker proof
permits actual supported replay only in a new existing task attempt, with one append-only
entry per attempt, current exact provider proof/authority and retention since first marker.
No new job/attempt/lease/recovery owner. The necessary03 recovery dependency examines all
dispatches, refuses unknown siblings/malformed/expired firstproof, and keeps current proof
checks at actual I/O; queue eligibility never establishes effect result. Additive remote
receipt/entry schema only; applied migrations through20260930120000 remain immutable.
Additive retirement/reopen guards enforce the action-aware safety predicate at the DB
boundary and recheck durable facts/retention instead of trusting an old safe label;
direct-SQL failures and a Rust/SQL equivalence matrix are mandatory.
First marker stores immutable bounded canonical tool+target bytes in an additive nullable
column; SQL compares decoded
subject to the frozen intent and SHA256(bytes) to the first descriptor operation digest.
Legacy markers lacking that first binding evidence remain unknown, with no backfill shim.
Late receipts retain truth without cancelled advancement.04.6 parking/audit,04.7 evidence,
04.8 full cancel matrix,04.9/10 providers,06 checkpoints and08 bypass remain open.
Affected expansion corrected independent Astra PASS/root ACCEPTED before implementation.

### 2026-09-30 reconciliation (04.1–10, light pass)

Accepted phases01–03 supersede the historical partial03.1 notes above. Existing05–10
rows retain every original nested criterion; no new cutover authorization is inferred.
Current execution topology is implement-sol-astra (Sol6.1/medium root, Sol6.1/high
implementer, its Astra/medium reviewer). Existing phase04 rows expand as follows:

- One application-owned action service and normalized workflow action owner accept both
  workflow-step and direct-agent calls. Both ingress methods converge before validation and
  persistence. Existing harness/MCP journals remain legacy callers until their owning06/08
  replacement gates; no new bypass or compatibility bridge. Provider adapters join04.9–04.11.
- Identity is company/run/logical execution + bounded operation key + canonical argument
  digest; job/attempt/lease generation are ownership evidence only. Frozen operation includes
  scoped target/connection, schemas, capability, approved operation/recovery policy, arguments,
  policy context and deterministic idempotency key. Different arguments produce a different
  invocation; an existing model tool-call mapping cannot change invocation/digest. Model-call
  identity is scoped to its logical agent execution, never global or attempt-scoped.
- 04.1 persists immutable intent with an unevaluated policy decision and durable model-call
  linkage. No provider dispatch capability exists yet.04.2 resolves current authorization;
  frozen policy content/ceilings and future approval evidence never substitute for it.
  Approval identity is invocation+digest; protected actions stay undispatched through04
  pending05 human settlement. Replayed preparation is not authorization or effect success.
- 04.2 loads saved intent and authoritative run actor/bundle/resources; caller ceilings only
  narrow frozen run tool capabilities. Current company/principal/resource access and trusted
  current policy are mandatory, errors propagate, unsupported owners fail closed. Its decision
  is an observation, not a reusable grant:04.3 repeats checks in fenced dispatch transaction.
  Current policy may strengthen approval, never weaken frozen protection. BRIEF-04.2 records
  concrete checks and provider/direct-agent integration limits.
- 04.3 uses existing run-first locks and current task_attempts fence, with action dispatch
  records referencing that sole attempt owner. It does not create another lease/job queue.
  Intent/permission/possible-dispatch/receipt are distinct facts. Persist possible dispatch
  before remote I/O;04.4 applies only explicitly approved replay contracts.04.5 commits
  authoritative action receipt before shared completion or model output; checkpoints contain
  receipt references/copies only. Same-DB effects and receipts commit together.
- 04.6–04.8 preserve03 unknown-effect parking, add authorized evidence settlement, and retain
  late receipt audit despite cancellation. Unknown never becomes safely retryable via job
  retry, output failure, timeout, budget exhaustion, or cancellation.04.11 deliveries own
  each provider receipt; action aggregates required acceptances without sibling resend.
- Current concrete seams: domain workflow IDs/causality; application workflow publication
  ApprovedActionPolicy/ToolContract and compiler Schema; persistence workflow activation,
  run-first completion and lease guard. New04.1 action modules/schema reuse these seams.
  HTTP/MCP connection readiness remains fail closed pending actual resource owners04.9/04.10.
- Each owning point exercises its negative/race cases;04.1 adds canonical/schema bounds,
  tenant/FK integrity, concurrent same-intent preparation, immutable model-call mapping,
  changed-argument identity and restart replay.04.3 adds competing dispatchers/stale fences;
 04.11 runs original combined phase04 acceptance matrix including all provider criteria.

No blocking cross-point contract remains for04.1. Current authorization, remote dispatch,
provider contracts, human approval settlement and production agent/ingress replacement remain
explicit later-point deliverables, rather than claims established by frozen records.

### 2026-09-30 concrete04.3 dispatch transaction contract

Selected remaining scope04.3–04.11 and05–10 was reconciled against original plans, not
only their summary rows. Accepted04.1/04.2 andphase03 evidence remains unchanged.
Implementer owns this expansion/code/BRIEF-04.3; root owns PROGRESS/RESUME acceptance.
This section requires independent expansion review and root acceptance before code.

04.3 deliverables: application `actions/dispatch` cohesive port/service entry for a saved
invocation subject plus WorkflowFence; persistence `workflow/action_dispatch` transaction
writer and additive dispatch/receipt migration; isolated SQL race/crash tests. Reuse
`lease::{lock_fence,live_window}`, `authority::authorize_company`, and fact restoration
from `action_authority`; share04.2 pure frozen/current checks rather than duplicating rules.
`background_tasks/task_attempts` remain sole lease/attempt owners; new action records
reference the existing attempt identity/generation and never claim jobs.

- Lock run first, execution/job and exact current task attempt, then current company/principal
  and selected authority rows, then invocation. Prove exact company/run/execution/job/fence,
  live lease/deadline and incomplete execution. Restore saved actor/bundle/resources/operation;
  validate frozen capability/contract/slot and current resource/policy. Recheck time after
  waits and immediately before commit; deferred SQL commit guard rejects crossing lease/run
  deadlines. Wrong/stale scopes cause no action writes or local effects.
- ResourceDirectory/ActionPolicyDirectory observations cannot fence revocation. The SQL
  composition instead requires a trusted transaction-aware adapter collaborator to read and
  lock current resource/policy authority in the same SQL transaction. Its SQL-specific hook
  belongs to adapter composition, not an application dependency; no provider/network I/O is
  allowed inside it. It must propagate errors, preserve exact identity/current readiness,
  and hold conflicting revocation locks through commit. Unsupported production authority
  owners fail closed;04.9/10 supply the actual resource/policy adapters without an invented
  parallel grant store. Tests use real SQL authority rows and competing revokers.
- Protected frozen or strengthened current approval returns undispatched;05 alone may add
  exact invocation/digest human settlement. A04.2 observation is never input permission.
  Dispatch records retain the matched current policy facts separately from immutable intent.
- Registered same-database effects execute using that transaction/connection; bounded,
  schema-valid authoritative result receipt and effect commit together. Any local failure,
  invalid result, insertion fault or commit fault rolls back effect, dispatch and receipt.
  This adapter composition hook is explicitly local SQL only; remote side effects cannot
  masquerade as transactional handlers. A committed receipt prevents executing the local
  effect again; broader receipt lookup/replay/completion/checkpoint integration belongs04.5/06.
- Remote reservation commits a durable possible-dispatch record before provider I/O; only
  a first successful reservation may enter transport.04.3 owns provider-neutral dispatch
  orchestration through an application RemoteAction port, tested with scripted transport;
  actual HTTP/MCP security/transport adapters remain04.9/10. The service retains an internal,
  non-cloneable/non-serializable single-use reservation containing saved operation/fence and
  conservative monotonic lease/run deadline. It never returns this as a caller-owned grant. Public ActionDispatch exposes local and
  provider-neutral remote orchestration; crate-private RemoteDispatch owns reservation/entry/
  ownership checks, with final entry consuming the reservation exactly once.
  Immediately before first provider polling, repeat transactional current authority and exact
  marker/fence checks, then check the monotonic window with no intervening awaits before
  polling. Supervise the actual provider future with deadline, cancellation and ownership-loss
  detection; drop/cancel and await actual work as required by existing supervision semantics,
  never detach it. Delayed/expired/revoked first use cannot start provider work. Authorization
  linearizes at that final locked check; revocation/cancellation after work starts cannot prove
  rollback. A stale/lost worker, ambiguous commit acknowledgement or restarted adapter cannot
  obtain another reservation from an existing marker. The marker survives crash/restart and
  remains conservative unknown evidence even if the process crashed before the actual send.
  This point deliberately permits no second remote reservation;04.4–07 alone may relax refusal
  with an approved replay contract or authorized proven-not-applied evidence. Remote output is
  an internal observed outcome, never accepted step/tool/model success before04.5 commits its
  authoritative receipt; output/schema failure likewise cannot erase possible-dispatch truth.
- SQL tenant/execution/digest/attempt foreign keys and uniqueness constrain provenance;
  append-only dispatch/receipt facts cannot be mutated/deleted to reset uncertainty. No new
  provider attempt ledger, task state owner or recovery queue. Receipt output/JSON and SQL
  work retain existing bounds; no bound increase. Applied migrations through20260930090000
  remain immutable; new schema changes are additive.

04.3 acceptance: competing step/tool dispatchers yield one record/effect; stale worker,
wrong generation/attempt/tenant/subject and expired deadline/lease refuse without writes;
real committed membership/resource/policy revocation while dispatch waits is rechecked;
stronger approval and lookup errors remain undispatched. Local write/receipt/commit fault
injection proves atomic rollback, valid result survives restart, duplicate call has one
effect. Remote marker is visible before scripted transport and persists after simulated
crash/lost response; delayed first-use expiry, post-reservation revocation/cancellation,
ownership loss during actual work and ambiguous commit acknowledgement cannot start/repeat
unauthorized transport. Restart/retry/reclaim cannot create a second reservation. SQL FK/immutability
negatives and commit-time expiry tests exercise database guards independently of Rust.
Run original per-point gates and accepted workflow_action tests at stock2MiB, fresh migrations,
SQLx prepare/check, graft build and independent actual-code review. No production provider
completion, phase04 combined gate, full-suite pass or bypass removal is claimed here.

Cross-point preservation:04.4 defines provider-idempotency/replay proof (method/header and
MCP session IDs are insufficient);04.5 records remote receipts before completion/tool/model
output;04.6 parks unknowns even after timeout/output validation/budget failure;04.7 authorized
evidence alone establishes applied/not-applied;04.8 cancellation suppresses undispatched work
and retains late accepted receipts without advancing cancelled runs.04.11 atomically creates
message/delivery/action/wait and aggregates each destination without sibling resend.04.9/10
retain all original HTTP/MCP publication/use-time secret/revocation/DNS/redirect/JSON/SSE/
bounded-result/schema/tool-error/session cleanup cases.05–10 retain original nested criteria,
particularly05 action approval versus business review,06 saved Rig calls/root budgets/grant
resolution,07 inherited child authority/revision identities,08 exhaustive replacement-map
bypass removal,09 below-UI sample isolation and10 full acceptance/CI/operational gates.
10.4–08 cutover remains blocked on identified target and explicit operational authorization.

| Point | Deliverable and acceptance |
| --- | --- |
| 04.1 Action contract | Frozen invocation/digest/policy/idempotency/receipt records; direct tools and steps share service; approval remains undispatched until 05. |
| 04.2 Execution item 1 | Current authorization + frozen ceiling, fail closed on directory errors/revocation. |
| 04.3 item 2 | Durable intent, atomic local effect+receipt, remote dispatch protocol; crash/claim tests. |
| 04.4 item 3 | Explicit provider replay contracts; no inference from method/header; unsafe replay negative tests. |
| 04.5 item 4 | Receipt before step/tool return; restart returns committed result without redispatch. |
| 04.6 item 5 | Ambiguous effects park with reconciliation; timeout/crash never blind-retries writes. |
| 04.7 item 6 | Authorized applied/not-applied evidence command; only proven not-applied can retry. |
| 04.8 item 7 | Cancel undispatched work, retain late receipts without advancing cancelled run; races. |
| 04.9 HTTP/tools | Fixed-origin company connections, DNS/connect-time destination guard, no redirects, bounded output/redaction; hostile endpoint and bypass tests. |
| 04.10 MCP | Reuse MCP HTTP adapter behind action port; frozen contract vs discovery, JSON/SSE, inert resource links, tool-error/effect distinction, current secrets/revocation, cancellation/session cleanup; explicit phase04 MCP race/recovery matrix. |
| 04.11 Messaging | Existing canonical message/delivery engine + atomic action intent/wait; exact source reply and stable Message-ID; partial destinations never resend accepted siblings; provider acceptance semantics. Combined phase04 gate. |

### 2026-09-30 concrete04.4 registered provider contract

Affected contract/acceptance lives in [BRIEF-04.4.md](BRIEF-04.4.md). Explicit frozen
SafeRepeat/ProviderIdempotency requires exact trusted registered transport proof before
first dispatch; unsupported/mismatched/expired contracts fail closed and provider receives
the service-owned stable logical-operation key. Persist proof in existing append-only
dispatch policy facts; no new job/lease/attempt ledger or applied migration modification.
No HTTP method/header or MCP metadata/session identifier establishes safe repetition.
Existing markers remain conservative: receipt-owned safe replay activation requires04.5
receipt-first replay, claimant exclusion and persisted first-dispatch proof/retention checks.
04.4 owns real first-call/key behavior and unsafe-repeat negatives;04.5 owns actual repeat
integration,04.6–08 own parking/evidence/cancel audit,04.9/10 own production providers.
This explicit dependency preserves the original criteria; it is not a full replay claim.
Affected expansion Astra PASS/root ACCEPTED2026-09-30; substantive implementation and
retention correction passed independent actual-code review. Final point acceptance/evidence
signoff remains root-owned; full safe replay activation remains required04.5.

### 2026-09-30 concrete04.6 action uncertainty

Affected contracts and acceptance: [BRIEF-04.6.md](BRIEF-04.6.md). Preserve accepted
04.5 shared action-safety predicate and sole run/job/attempt retirement owners. Add
append-only invocation reconciliation audit and internal receipt-precedence state
projection; pending failure paths also consult action safety. Parked runs use their
existing deadline/poller; cancellation/deadline retain unresolved facts. No audit fact
or error becomes not-applied evidence or a replay grant. 04.7 settlement, full04.8
matrix, production04.9/10, messaging04.11, protection05, Rig06 and bypass08 remain
obligations. Affected expansion Astra independent PASS and root accepted17:49Z.
Implementation, original criteria, integration and required gates independently PASS;
root accepted04.6 at18:29Z. BRIEF-04.6 links the criterion-specific final review and
62 action / 268 affected stock2048KiB tests plus all required static/schema/SQLx gates.
No04.7 code before a separate affected expansion acceptance.


### 2026-09-30 concrete04.7 evidence settlement expansion

[BRIEF-04.7.md](BRIEF-04.7.md) defines the complete affected item6 foundation:
current authenticated command authority, trusted registered verifier with exact
snapshot/quiescence binding, applied receipt provenance, one-use final not-applied
coverage/consumption, shared scheduling/marker/SQL predicates, and atomic existing-job
reopening from reconciliation wait. Terminal truth remains audit-only. **Independent
Astra expansion PASS after three grouped corrections; root ACCEPTED18:57Z.**
Fresh implementation worker resumes this accepted contract; no04.7 code performed
in the expansion-only subtree.
Accepted04.6 stays closed; PG stopped and no source/migration/operational work here.
Later04.8–11 and05–10 obligations remain; no production provider/UI integration claim.

## Phases 05–09 — ordered top-level work units

| Point | Deliverable / seams and acceptance |
| --- | --- |
| 05.1 Decision contract | Workflow decision domain/application/persistence: declared choices, schema-valid edits, reviewed artifact/revision; immutable original output. |
| 05.2 Comments/submission | Separate append-only comment and idempotent expected-revision submit commands; settlement+continuation atomic; reviewer/timeout/cancel races and invalid input stays open. |
| 05.3 Assignment/auth/notify | Deterministic eligible reviewers, current access, expiring single-use external credentials, durable precise notifications; action digest subject separate from business review. No eligible reviewer fails. |
| 05.4 Other waits | Scoped durable event/timer correlation, schemas/deadline/sweeper; duplicate and event-before-park/restart checks; phase05 gate. |
| 06.1 Rig integration | Refactor agent runner/harness via action/checkpoint contracts; bounded repairs/root budgets, resume saved calls, remove alternate runtime when migrated; no implicit context/memory/delivery. |
| 06.2 Explicit preparation | Context history cutoff/provenance, scoped memory load/save, bounded classifier/agent-choice handlers; committed result reused, choices not authority. |
| 06.3 Skills/profiles | Instructions+requirements only; omitted uses frozen saved ordered defaults, explicit replaces, intersect grants/ceiling/current policy, missing requirements fail; operational tool/skill loading and traces; phase06 tests cover every nested resolution rule. |
| 07.1 Child calls/tools | Atomic pinned child+parent wait, explicit inputs/resources, inherited ceilings/budgets/cancel, durable completion and saved model call identity. |
| 07.2 Retry/revision | Same retry identity vs new round/new changed-input run; immutable prior records, no budget replenishment. |
| 07.3 Repeated review | Parent-owned bounded repeat, child draft/review fixture, explicit next inputs/exit/exhaustion; last-round accept succeeds, default three rounds then escalation, exact accepted artifact only delivered. |
| 07.4 Other successor feedback | Explicit human feedback/data mappings to other steps; stale artifacts/links cannot settle current round; combined child/restart/cancel phase07 gate. |
| 08.1 Ingress | Retarget thread ingest/channel dispatch to atomic workflow intent; preserve auth/dedup/thread/hop controls, independent eligible bindings, no active binding still stores messages. |
| 08.2 Other entry points | Manual idempotency, schedule slot dedup, child delegation, correlated outreach; explicit assistant setup; remove quorum special loop. |
| 08.3 Interfaces | Company-scoped thin HTTP commands including exact nested resource authorization, CSRF/webhook checks, expected revisions/idempotency. |
| 08.4 Removal | Exhaustive callers per REPLACEMENT-MAP; delete obsolete dispatch/config/state/payload and executable skills after replacement tests; retarget projections/bootstrap/docs. Phase08 unified engine gate. |
| 09.1 Authoring/setup | Existing HTTP pages/HTMX: library/editor/catalogue/read-only graph/versions/bindings/agents/effective capabilities; optimistic edits, retained invalid source, both themes/keyboard. |
| 09.2 Runs/conversations | Authorized run graph/timeline, exact producing-message/decision links, independent simultaneous runs, effect truth and safe controls; SSE owner/reconnect/reread tests. |
| 09.3 Human queue | Distinct business/action subjects, artifact/version/comments/feedback/edit validation, closed/late state; cross-company/private-thread/hostile rendering/CSRF tests. |
| 09.4 Samples | Isolation enforced beneath UI for engine/children/direct tools; scripted providers/intercepted effects, zero production reads/writes by default, no promotion on resume. Phase09 complete user journey gate. |

## Phase 10

| Point | Deliverable and acceptance |
| --- | --- |
| 10.1 E2E | All original acceptance matrix rows, local scripted providers + isolated real PostgreSQL, concurrent claimants/crashes/poison batches. |
| 10.2 CI | Original exact formatting/offline/migrations/SQLx/DB/transport/stock-stack checks, pinned deps/non-root image; resource-bound growth retains early CI failure. |
| 10.3 Operations | Payload-free durable metrics and exact run/action links, every wait maintenance path, safe recovery/revocation docs, fairness under load. |
| 10.4 Cutover item 1 | BLOCKED pending identified target and explicit operational authorization; prepare reviewable artifacts first. No reset inferred. |
| 10.5 item 2 | Requires 10.4: stop old ingress/workers and account for effects; no dual sending. |
| 10.6 item 3 | Requires authorization: deploy fresh schema/app/resources/templates with admission off. |
| 10.7 item 4 | Authorized controlled samples/email/manual/schedule smoke and recovery. |
| 10.8 item 5 | Authorized selected activation/monitoring/emergency stop. |
| 10.9 item 6 | Remove remaining obsolete docs/flags/fixtures/paths, mark superseded plans, exhaustive checks. |
| 10.10 item 7 | Refresh graph and final operational docs; independent combined acceptance review; record any unexecuted production gates honestly. |

### 2026-09-30 affected04.7 applied-request provenance correction

Root accepted the independent Astra PASS of the affected correction contract in
[BRIEF-04.7.md](BRIEF-04.7.md): trusted Applied request attribution distinguishes
covered old requests from legitimate newly authorized effects and preserves
unattributed positive truth conservatively; database-owned fulltransaction witness
binds atomic newentry consumption and initialstate/waitreason to exact deferred
reopen. Five actualpartial-schema findings remain to implement in additiveSQL after
immutable180000. No originalcriterion relaxed, full04.7 not accepted, no04.8work.
Finalreview `/private/tmp/workflow-04.7-schema-expansion-review-final.md`.
