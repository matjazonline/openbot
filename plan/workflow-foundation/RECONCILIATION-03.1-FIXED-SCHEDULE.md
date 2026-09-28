# Remaining 03.1 reconciliation — fixed schedule channel

2026-09-28. Expansion only; production edits await root coverage acknowledgment.
Read original03, EXPANSION shared/03.1, BRIEF-03.1 remaining table, RESUME,
JOB-ISOLATION-REMAINING and latest JOB-SLICE A2 handoff. Existing accepted fragments
and audit inventories remain evidence; this does not restart their discovery.

## Shared decisions and boundaries

The user's fixed-channel requirement supersedes A2's movement-preserving contract:
a schedule keeps its creation channel permanently. Creation still selects a channel;
ordinary schedule editing, frozen occurrence snapshots, scope and lease fences remain.
Reject attempted reassignment without partial changes at application/HTTP/persistence/DB
boundaries; the editor must not offer channel reassignment. A channel parameter used
by an API route is scope, never a requested destination. No compatibility shim or
old business-data upgrade is required. Applied migrations through20260928101000 stay
immutable; correction uses a new migration without restoring redundant constraints.

Preserve all staged/unstaged/untracked work. No commit/reset/deploy. Root owns
PROGRESS/RESUME acceptance; implementer owns this reconciliation and JOB-SLICE evidence.
The required topology exists: implementer01a0e76c-477d-7513-8410-c44e6131c54a
Astra/medium, nested reviewer01a0e76c-8b9b-72d1-8524-028752d48a08 Astra/medium,
capacity verified before edits. Reviewer remains idle until review packet; edits stop
during review. Task-only PG55439 is stopped; restart only for implementation and
explicitly hand back or stop it. No other database may be used.

## Ordered coverage

| Checkpoint | Deliverables and affected seams | Acceptance and dependencies |
| --- | --- | --- |
| A2 fixed channel | Application ScheduleUseCases::update_schedule; SchedulePersistence update contract/implementations; JSON and UI update inputs/editor; additive channel_schedules immutability trigger; existing A2 provisioning/occurrence guards and tests. Initial graph spans: application/use_cases/schedule.rs:356–388; adapters/http/routes/schedule.rs:271–281; routes/ui_schedules.rs:831–856. | Trace all callers (graft callers depth all plus exhaustive literal fallback for ambiguous methods) before refactoring. Reject wrong channel through application, API/form, adapter and raw SQL atomically; preserve creation choice, legitimate edits/run-as authorization, saved occurrence identity and worker fences. Replace movement assertions; real capture/change contention proves no committed reassignment. Refresh affected/full DB stock2MiB, migrations, offline check/Clippy, SQLx, fmt/diff/graft; independent review before B. |
| B historical provenance | Add insertion-only visible scoped legacy task provenance for thread_handoff_events; task row lock through commit because events intentionally have no task FK. Inspect handoff generation/read/complete/fail/resolve/draft/expiry seams already enumerated in JOB-ISOLATION. | Taskless events remain valid; historical rows survive deletion and remain immutable without live-task filtering. Reject workflow/foreign/missing/invisible parent before influence; verify legacy draft/complete/fail/expiry and real deletion contention. A2 acceptance first. |
| C/D coordinated job gate | Nullable channel only for workflow jobs; required legacy domain channel retained. Exact NULL-aware job association equals execution's run company/channel/thread; immutable or protected parent association/execution links. Update affected macro decoding/SQLx metadata. | Company-only/channel/thread workflow fixtures; legacy NULL and wrong company/channel/thread/execution rejection; parent UPDATE versus job INSERT race; mixed legacy claim/control/recovery/projection exclusion and shared attempts. Keep workflow_disabled and channel NOT NULL until complete isolation/nullable gate independently reviewed together; only then enable with additive schema gate. Fresh migration/full DB stock2MiB/static/SQLx/integration review. B acceptance first. |
| Atomic admission/snapshot | Existing PreparedAdmission/WorkflowAdmission transaction creates durable input/bundle/binding/resources/limits/deadline snapshot, run/first execution/ID-only first job/history/source aliases/audit using accepted normalized owners. No second queue or attempt owner. | Final-write/deferred failure rolls every record back; stored codec roundtrip/corruption rejection; real same-key and different-key same-source competing admissions create one run/job; changed input/key conflicts; two messages in one thread remain independent. C/D acceptance first. |
| Source replay/authority | Canonical source + logical binding and command equivalence remain separate; saved bundle/config survives binding edits/deactivation/removal/archive. New/replay/alias reauthorize actual actor, canonical message/occurrence/parent source and saved resources under accepted lock order. Reader selection grants no authority. | Missing/foreign/inconsistent sources and revoked actor/channel/resource fail with no writes; current source/resource revocation races; manual stable source; child action fails closed until phase04 owner. Preserve supported MCP authority matrix, no invented grants. |
| Committed history/whole03.1 | Bounded deterministic single-statement membership snapshot with scoped pinned message IDs; inspection/context consumes saved membership only. | Older-created uncommitted message arriving after capture absent; later admission sees it; overflow rejects with rollback. Combined actual-code independent review, full DB stock2MiB and required static/migration/SQLx/graft gates before whole03.1 acceptance. No03.2 activation/claim/runtime/wait handlers. |

No unresolved architecture decision is introduced. Detailed local API shape is chosen
after complete A2 caller inspection, without retaining channel movement. Reuse accepted
unaffected fragment checks only while their assumptions remain valid; never treat the
pre-steering1948-pass run as fixed-channel evidence. Record exact commands/log paths and
criterion links at each checkpoint; root accepts each before dependent work starts.

Initial discovery: graft map, one ask, callers(depth all), exhaustive update_schedule grep:
4 counted calls, estimated savings2635709 tokens (graph estimate, not measured billing).
The method name is ambiguous in graph caller edges; exhaustive grep finds JSON and UI
adapters plus two application tests and their route registrations. Complete persistence
trait/update/editor inventory remains the first implementation discovery step.
