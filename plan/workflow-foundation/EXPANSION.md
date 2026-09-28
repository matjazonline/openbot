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
