# Phase 01 replacement map

This is an inventory of the **current** implementation and a target ownership map, not a claim
that the workflow runtime or its interfaces ship today. After phases 02–07 provide replacements,
dependent UI/API callers and projections must be retargeted before phase 08 deletes each legacy
path; phase 09 completes the authoring and operations journey, and phase 10 proves the cutover.
A source span below names an existing definition or caller, not a new API.
The phase 01 domain and application contracts alone do not replace any production dispatcher.

The owner names are the authoritative records in [01](01-architecture-and-domain.md):
**version**, **binding**, **run**, **step execution**, **job/attempt**, **agent checkpoint**,
**action invocation**, **human decision**, and **delivery**. Authentication, channel policy,
canonical messages/threads, credentials, and resource storage remain infrastructure. The port
named in a target row is a required responsibility, not an assertion that the port exists yet.
Each row assigns exactly one owner to the retained behavior; a mixed legacy function appears in
more than one row because it currently crosses those ownership boundaries.

## 1. Message dispatch and delivery

| Current code and caller/projection | Retained invariant → target owner and port | Phase and removal gate |
| --- | --- | --- |
| `ThreadUseCases::execute_claimed_agent_task_and_dispatch` `src/application/use_cases/thread/dispatch.rs:346–362`, called by `TaskWorker::run_task` `src/application/services/task_worker.rs:954–1056`; `run_claimed_dispatch` `dispatch.rs:775–895` combines agent answers. Ingress planning/commit is `src/application/use_cases/thread/ingest/mod.rs:280–310,390–466`, and claimed work reloads stored context via `src/application/use_cases/thread/reload.rs:107–180`. | One source message is admitted independently for **each eligible channel binding**. **Binding** owns version/parameter selection and active revision; its admission port must deduplicate by source event plus binding. Ingress validation, channel visibility, and canonical message association stay infrastructure. A waiting run does not hold up another message in the same conversation. | 02–03 binding/admission, 08 replace old task enqueue and combined dispatch. Remove only after duplicate ingress and multi-address tests prove independent runs. |
| Same dispatch path plus `src/application/use_cases/thread/dispatch.rs:374–680,773–900`; task payload commit `src/application/task_queue.rs:224–239`, `src/adapters/persistence/task/operations.rs:1487–1737`. | **Run** owns input snapshot, lifecycle state, budgets, and causal links through a run-transition port rather than task payload as another status owner. | 03, 08. Remove task-payload run state only after crash, retry, and concurrent-message tests pass. |
| The combined agent answer/route in `src/application/use_cases/thread/dispatch.rs:775–895` and committed reply in `src/adapters/persistence/task/operations.rs:1487–1737`. | **Step execution** owns one step's resolved input, immutable committed output, and selected route through a step-commit port. | 03, 06, 08. Remove combined answer routing after branch/atomic-commit tests pass. |
| `TaskWorker::run_task` `src/application/services/task_worker.rs:954–1056` and current background task/lease rows `migrations/20260817000000_init_schema.sql:1954`; `commit_agent_dispatch` fences writes in `src/adapters/persistence/task/operations.rs:1487–1557`. | **Job/attempt** owns scheduling, exclusive claim, lease, retry/backoff, and fencing. Retain the tested stale-worker and competing-claimant guarantees. | 03, 08. Remove old task types when new claims and commits fence every run and no old worker can dispatch. |
| `plan_agent_deliveries` and `commit_dispatch` `src/application/use_cases/thread/dispatch.rs:1376–1471,1597–1663`; `commit_agent_dispatch` persists message, review candidate, and deliveries in `src/adapters/persistence/task/operations.rs:1487–1737`; delivery worker is separate from task worker (`src/application/services/task_worker.rs:388–389`). | **Delivery** owns provider-specific attempts and receipts through an outbox port. The canonical reply and owed delivery commit together; a provider retry must keep the stable outbound identity. | 04, 08. Retain atomic reply/delivery tests and verify duplicate provider attempts cannot produce a second logical reply. |

The existing task/thread projection is not a new state owner: `THREAD_TASK_LOOKUP_SQL`
`src/adapters/persistence/thread/views.rs:177–197` feeds `fetch_thread_tasks` `:201–231`.
Retarget task detail/status (`src/adapters/http/routes/ui_tasks.rs:204–332,1038–1114`;
`src/adapters/http/pages/task_board.rs:628–640`) and the mailbox
(`src/adapters/http/routes/ui.rs:699–760`) to run/step/job/decision projections in 08–09.
Attention and notification links currently project legacy work through
`src/adapters/http/routes/attention.rs:178–346` and
`src/adapters/http/routes/notifications.rs:53–232`; their targets must point to scoped run,
decision, or delivery detail after cutover.
The current simulation can send: `delivery_for` selects `ReplyDelivery::Send` for `Run` and
`ReplyDelivery::InAppOnly` for `RunTest` (`src/adapters/http/routes/channel.rs:1327–1334`),
through the channel simulation routes `:967–1030,1051–1223,1233–1301`. Retarget both modes
with explicit future effect/delivery policy; preserve the test mode's in-app behavior and the
run mode's authorized real-send behavior. The phase 09 sample executor is a separate isolated
mode with intercepted effects (`09-authoring-and-operations-ui.md`, Sample execution).

### Hidden dispatch preparation and memory effects

| Current code and caller/projection | Retained invariant → target owner and port | Phase and removal gate |
| --- | --- | --- |
| Dispatch composes private handoff/review feedback and recalls memory before running an agent in `src/application/use_cases/thread/dispatch.rs:475–499,972–1010`; `AgentRunner` applies the configured spam guardrail/classifier in `src/application/services/agent_runner/mod.rs:309–324`. | **Step execution** owns explicit, committed context/recall/classification results and the route selected from them. Preparation steps replace hidden prompt mutation; authorization and ingress checks still apply. | 03, 06, 08. Remove automatic preparation only after deterministic input/output, rejection, and resume tests cover the explicit steps. |
| Dispatch automatically persists agent output, including the pending-review path, in `src/application/use_cases/thread/dispatch.rs:655–665,877–880,1143–1163`; provider persistence is behind `src/application/services/memory_coordinator.rs`. | **Action invocation** owns an explicit authorized memory write, idempotency identity, and effect receipt; memory binding/provider storage remains infrastructure. | 04, 06, 08. Replace automatic output saves with explicit writes for the intended answer/draft/feedback policy, and verify retry/recovery before removing the old hook. |

## 2. Scheduled dispatch

| Current code and caller/projection | Retained invariant → target owner and port | Phase and removal gate |
| --- | --- | --- |
| `ScheduleUseCases::trigger_schedule_now`, `execute_schedule_trigger`, `execute_schedule_run`, `process_due_schedules` `src/application/use_cases/schedule.rs:454–477,548–706,709–766`; callers are JSON/UI routes `src/adapters/http/routes/schedule.rs:296–305,391–402`, workspace `src/adapters/http/routes/ui_schedules.rs:739–750`, and worker poll `src/application/services/task_worker.rs:390–437`. | **Binding** owns the schedule's selected immutable workflow version, parameters, resource slots, and activation revision. The existing schedule clock/slot calculation remains infrastructure. | 02, 08–09. Retarget schedule create/edit/run-now routes and workspace before deleting schedule-to-agent settings. |
| `SCHEDULED_AGENT_RUN_TASK` `src/application/use_cases/schedule.rs:93`, `ScheduledRunPayload` `src/domain/entities/schedule.rs:363–408`, enqueue `src/application/use_cases/schedule.rs:648–706`, and special worker/dispatch arms `src/application/services/task_worker.rs:954–970`, `src/application/use_cases/thread/dispatch.rs:366–372,374–681`. Materialization persistence is `src/adapters/persistence/schedule.rs:345–576`; tables begin `migrations/20260817000000_init_schema.sql:2086,3093`. | **Run** owns one immutable admission per materialized schedule slot, including run-as identity and input snapshot. Its admission port preserves slot deduplication, including retries. | 03, 08. Remove scheduled-agent task type/payload and dispatch branch after duplicate-slot and retry-without-second-agent-call tests pass. |
| Schedule history `src/application/use_cases/schedule.rs:417–452`, `src/adapters/persistence/schedule.rs:577–641`, and schedule UI `src/adapters/http/pages/schedules.rs:212–382`. | **Run** owns outcome and causal schedule link, exposed through a scoped inspection port; schedule history is a projection. | 08–09. Repoint history/thread links before dropping `schedule_runs`-to-task assumptions. |

## 3. Response review and edited publication

| Current code and caller/projection | Retained invariant → target owner and port | Phase and removal gate |
| --- | --- | --- |
| Review policy/assignment and commands in `ResponseReviewUseCases` `src/application/use_cases/response_review.rs:367–421,424–513`; UI policy/list/detail/approve/reject/reassign/edit routes `src/adapters/http/routes/ui_response_reviews.rs:130–404`; page projection `src/adapters/http/pages/response_reviews.rs:8–316`. Storage uses `src/adapters/persistence/response_review.rs:524–563` and `response_drafts`/`response_reviews` tables `migrations/20260817000000_init_schema.sql:2963,3025`. | **Human decision** owns assignment, deadline, comments/feedback, reviewed artifact identity/version, submission and audit through a decision port. First valid authorized submission wins; invalid edits remain open. | 05, 07, 08–09. Retarget policy and queue UI; remove response-review state tables/owners only after review/revision and competing-submission tests pass. |
| Draft preparation `src/application/use_cases/thread/human_completion.rs:40–55`; handoff publication also enters review via `send_draft` and `send_edited_draft` `src/adapters/http/routes/thread_handoffs.rs:331–377,392–493`. | **Step execution** owns the draft artifact/output being reviewed and the route chosen after the decision. The accepted version must be the exact edited/reviewed artifact; rejection feedback is explicit step output. | 05, 07–08. Retarget both ordinary review and handoff paths before deleting their special branches. |
| Reviewed publication enters `approve_on` `src/adapters/persistence/response_review/commands.rs:267–359`, which atomically records the approved canonical reply, delivery obligation, draft/review status, task completion, and handoff resolution. The decision port is `src/application/use_cases/response_review.rs:417–421`. | **Action invocation** owns authorized publication of the exact approved artifact/version, with stable intent and receipt; canonical message storage stays infrastructure. | 04–05, 08. Preserve no-publication-on-rejection/stale-version and atomic publication tests. |
| The same `approve_on` transaction `src/adapters/persistence/response_review/commands.rs:267–359` inserts the delivery obligation; the provider worker later sends it. | **Delivery** owns provider attempts and receipts once publication is committed. | 04–05, 08. Preserve atomic reply/delivery enqueue and retry tests; no provider send may precede approved publication. |

## 4. Protected-action approvals

| Current code and caller/projection | Retained invariant → target owner and port | Phase and removal gate |
| --- | --- | --- |
| `AgentApprovalHandler` `src/application/services/harness/approvals.rs:107–191` is supplied by `AgentRunner::approvals` `src/application/services/agent_runner/mod.rs:414–437`; `ApprovalUseCases`/port `src/application/use_cases/approval.rs:61–110,129–237,288–410`. Current rows are `human_approvals` `migrations/20260817000000_init_schema.sql:2360` with storage `src/adapters/persistence/approval.rs:127–230,516–606`. | **Human decision** owns deterministic eligible approver, frozen reviewed operation/artifact, scoped single-use link, deadline, response and audit. An approval is for one invocation digest; it cannot broaden grants. | 05, 08. Move pending/answered links and restart-safe decisions before removing harness-specific approvals. |
| The proposed tool call and saved continuation currently pass through the harness approval bridge `src/application/services/harness/approvals.rs:116–191` and agent checkpoint path in `src/application/services/agent_runner/mod.rs:414–437`. | **Action invocation** owns authorized operation, idempotency identity, intent, and effect receipt through the shared action service. No effect dispatches before approval; recheck current access at dispatch. | 04–06, 08. Keep crash/restart, argument-integrity, and approval-before-effect tests. |
| Approval notification built by `src/application/use_cases/approval.rs:154–279`; link/assigned/channel routes `src/adapters/http/routes/approval.rs:44–145`; attention projection `src/adapters/http/pages/attention.rs:299–341`. | **Delivery** owns queued request/decision notification and provider receipt. Link and queue pages project the human decision, never act as authorization or state owners. | 04–05, 08–09. Retarget pages and preserve notification atomicity and token access tests. |

## 5. Outreach and quorum delegation

| Current code and caller/projection | Retained invariant → target owner and port | Phase and removal gate |
| --- | --- | --- |
| Native tool ID `outreach_and_await_quorum` `src/domain/entities/tool_catalogue.rs:16,150`; host dispatch `src/application/services/native_tools.rs:121–159`; `OutreachAndAwaitQuorumTool::call` `src/application/services/outreach_tool.rs:162–236`; durable create/pause `src/adapters/persistence/task/operations.rs:522–688`. | **Action invocation** owns each frozen authorized message send and effect receipt. Run-local delegation becomes a child-workflow call; communication in an existing conversation becomes an explicit message action followed, if needed, by a response wait. Existing target/channel access checks remain outside prompts. | 04, 07–08. Remove native quorum tool/catalog entry only when both replacement paths work and approvals still gate protected sends. |
| Reply correlation `src/adapters/persistence/task/operations.rs:689–759`, quorum tally/timeout `:209–340,760–898`, worker timeout decision `src/application/services/task_worker.rs:776–912`; tables `migrations/20260817000000_init_schema.sql:3341,3370`. | **Step execution** owns the durable wait request, matching criteria, and committed matched response through an event-consumption/step-commit port. One matching response satisfies the conversation wait; the old multi-target quorum execution loop has no replacement behavior. | 03, 05, 08. Remove `task_outreaches` quorum state after reply/timeout/restart/cancel tests for the new wait, and remove quorum-only tests as obsolete. |
| The same timeout and wake paths `src/adapters/persistence/task/operations.rs:760–898`, `src/application/services/task_worker.rs:776–912` park and resume parent work. | **Run** owns waiting/running lifecycle transitions and deadlines through its transition port. | 03, 05, 08. Preserve independent runs and restart-safe timeout transitions. |
| Controls and reassignment currently use `src/adapters/persistence/task/controls.rs:393–426,644–1016`; UI routes `src/adapters/http/routes/ui_tasks.rs:581–626`, mailbox control `src/adapters/http/routes/ui.rs:1971–2063`, and team redirect `src/adapters/http/routes/ui_team.rs:632–669`. | **Human decision** owns retained authorized human control/comment/assignment audit through scoped decision commands. | 05, 08–09. Remove old decision controls after scoped decision UI and race tests cover them. |
| The same legacy delegation controls `src/adapters/persistence/task/controls.rs:644–1016` also stop or continue parent work after target changes. | **Run** owns child-run lifecycle and continuation through its transition port; quorum-specific status is not carried forward. | 07–09. Remove parent-task continuation controls after child completion/cancel tests cover them. |

## 6. Agent-channel creation

| Current code and caller/projection | Retained invariant → target owner and port | Phase and removal gate |
| --- | --- | --- |
| `AgentUseCases::create_addressable_agent`/`_with` `src/application/use_cases/agent.rs:609–668` are called by agent API/UI `src/adapters/http/routes/agent.rs:379–423,475–508,718–729`, `src/adapters/http/routes/ui_agents.rs:521–540,621–666`. Channel creation joins an agent via `ChannelUseCases::create_channel_with_agent` `src/application/use_cases/channel.rs:348–371`, called by `src/adapters/http/routes/channel.rs:554,1639` and `src/adapters/http/routes/ui_channels.rs:276–341`. Onboarding collects the agent/channel choice `src/adapters/http/routes/onboarding.rs:220–293`. | Agent save persists only agent identity/instructions as infrastructure. **Binding** owns explicit channel-to-workflow selection, activation and revision through a binding command. Channel identity, addressing, provider binding, visibility and agent resource storage remain infrastructure. | 02, 08–09. Retarget API, onboarding and both UI flows to explicit setup; remove implicit personal-channel orchestration only after atomic setup and access tests pass. |
| Native `create_agent_channel` ID `src/domain/entities/tool_catalogue.rs:17,165`; `AgentChannelProvisioning` and tool `src/application/services/agent_channel_tool.rs:102–107,223–271`; native host `src/application/services/native_tools.rs:121–159`; builder seed `src/application/use_cases/builtin_agent_library.rs:63–81`; persistence `src/adapters/persistence/channel.rs:388–489,653–685`. | **Action invocation** owns authorized/idempotent agent/channel provisioning as an effect when invoked by an agent. The created resources stay in infrastructure. | 04, 08–09. Replace native tool and builder instructions, preserve scope/approval/atomic provisioning tests, then remove legacy tool path. |
| Implicit workflow activation coupled to agent/channel creation in `src/application/use_cases/agent.rs:609–668` and `src/application/use_cases/channel.rs:348–371`. | **Binding** owns explicit activation/revision; saving an agent is not an activation command. | 02, 08–09. Require an explicit setup command and remove implicit activation after UI/API replacement. |

## 7. Executable skill instructions

| Current code and caller/projection | Retained invariant → target owner and port | Phase and removal gate |
| --- | --- | --- |
| `SkillInstruction::{Prompt,Tool}` `src/domain/entities/skill.rs:92–104,111–206`; authoring `src/application/use_cases/skill.rs:30–74,304–365`, `src/adapters/http/routes/skill_library.rs:267–300,450–615,784–800`; persistence `src/adapters/persistence/skill.rs:39–63,350–420`; agent settings render `src/adapters/http/pages/agent_settings.rs:1730–1752`. | **Version** freezes reusable instruction text and declared capability requirements with the published agent bundle. Saved skill/resource records and authoring access stay infrastructure. Ordered executable procedures move to workflow definitions/steps. | 02, 06, 08–09. Remove `SkillInstruction::Tool` editor/storage/execution shape after published procedures and migrated fixtures are validated; no legacy interpreter. |
| Alternate compilation `src/adapters/harness/ai_agents/compile.rs:196–217`; Rig template rendering/validation `src/adapters/harness/rig/skills.rs:181–219`; current capability/skill loading and snapshot construction `src/application/services/agent_runner/params.rs:238–318,111–195`. | **Version** freezes selected skills/capabilities in the published specification. | 02, 06, 08. Keep saved-selection and malformed-template tests while replacing executable skill steps. |
| The same capability snapshot `src/application/services/agent_runner/params.rs:111–195,238–318` feeds current execution. | **Step execution** records the selected skill/capability input resolved from the frozen version. | 03, 06, 08. Verify deterministic step inputs and restart with changed live skill settings. |
| Rig model continuation and conversation state are represented in `src/application/services/harness/runs/state.rs:334–374` and committed in `src/adapters/harness/rig/execution.rs:447–509`. | **Agent checkpoint** owns only model conversation, pending calls, tool-result copies, and continuation for replay; the selected skill is input from version/step records. | 06, 08. Keep restart/resume tests without treating checkpoint copies as capability grants. |
| Tool instructions in `src/domain/entities/skill.rs:160–206` and their current compilation `src/adapters/harness/ai_agents/compile.rs:196–217`. | **Action invocation** owns any retained selected tool call and receipt; skill text/profile selection cannot grant a capability. | 04, 06, 08. Keep scope and authorization tests while moving procedures into steps. |

## 8. Alternate harness runtime

| Current code and caller/projection | Retained invariant → target owner and port | Phase and removal gate |
| --- | --- | --- |
| `HarnessKind`/`HarnessConfig` `src/domain/entities/harness.rs:34–80,405–440`; registry registers `AiAgents` and `Rig` `src/adapters/harness/mod.rs:20–43`; runner chooses execution `src/application/services/agent_runner/mod.rs:287–390`; old adapter `src/adapters/harness/ai_agents/mod.rs:74`. | **Agent checkpoint** owns one Rig continuation, pending calls and completed results. Remove the second runtime rather than preserving a selector compatibility layer. | 06, 08. Prove Rig tool approval, resume, deadline, structured output and memory parity before removing `ai_agents` code/dependency. |
| Final agent reply conversion in `src/application/services/agent_runner/mod.rs:287–390` and `src/application/use_cases/thread/dispatch.rs:775–895`. | **Step execution** owns validated agent output and route, rather than harness-specific dispatch. | 03, 06, 08. Remove harness-specific reply publication after structured-output and failure-state tests pass. |
| Agent write/API/UI selectors `src/application/use_cases/agent.rs:52–133,489–526`, `src/adapters/http/routes/agent.rs:95–232,279–315`, `src/adapters/http/routes/ui_agents.rs:169–170,1066–1067`, onboarding `src/adapters/http/routes/onboarding.rs:60–67,243–293`, pages `src/adapters/http/pages/onboarding.rs:116–154`; persisted `agents.harness_kind`/`config_json` `src/adapters/persistence/agent.rs:35–97,422–490`, schema check `migrations/20260817000000_init_schema.sql:1883–1909`; deployment key `DEFAULT_AGENT_HARNESS` `src/infra/config.rs:965–1004`. | **Version** freezes a single Rig-compatible agent specification. Agent storage/config parsing remains infrastructure but loses alternate selection. Existing configuration keys must be removed from docs/examples together with code rather than silently ignored. | 06, 08–09. Remove selector fields, database check branch, deployment config and alternate examples only after all creation/edit/read projections use Rig. |

## Removal and verification ledger

1. Deliver 02–07 owners and purpose-specific ports first: immutable version/binding, independent
   run, step commit, fenced job, Rig checkpoint, authorized action receipt, human decision, and
   outbox delivery. Keep authentication, ingress validation, channel/thread policy and canonical
   message storage as infrastructure. No old and new dispatcher may send for the same binding.
2. For each phase 08 removal, first retarget its dependent UI/API callers and projections to the
   new owner ports: schedule materialization, approval/outreach controls, task monitor, thread
   lookup, simulation modes, onboarding, mailbox, attention, notifications, reviews, and resource
   setup. Then replace message/schedule/manual/child trigger entry points and delete their legacy
   branches/tables/config; every accepted message gets its own run per eligible binding. No
   compatibility constructor or dual executor.
3. In 09 finish authoring and the operations journey across schedules, reviews, task/run detail,
   and the human queue on scoped run/step/decision/delivery projections. Their current routes are
   `src/adapters/http/routes/{attention.rs,notifications.rs,ui_tasks.rs,ui_schedules.rs,ui_response_reviews.rs,approval.rs}`;
   the new projections must never own workflow state.
4. In 10 retain or replace the invariant tests named below and pass the complete acceptance matrix:
   duplicate ingress/slot, competing claimants, stale lease, crash/restart, revocation, review
   races, exact approved arguments, atomic reply/delivery, and provider retry. Fresh-database
   cutover is separately authorized; this map does not authorize a reset or deployment.

Production discovery used `graft ask` for the eight families, `graft callers` for concrete entry
symbols, and exhaustive `graft grep` for `OUTREACH_TOOL_ID`, `CREATE_AGENT_CHANNEL_TOOL_ID`,
`SkillInstruction`, `THREAD_TASK_LOOKUP_SQL`, and `SCHEDULED_AGENT_RUN_TASK`; exact ranges were
checked against targeted source `rg -n`. The graph covers 511 indexed files at this checkpoint.
Bootstrapping is centralized at `src/infra/setup.rs:86,192–272,328`: it installs the builder
library and wires harnesses, skills, approvals, review, channel provisioning, and schedules. The
phase 08 replacement must update these dependencies and the worker's scheduled poll, not only
HTTP handlers.
`graft callers` loses cross-file edges for ambiguous names such as `call`, `execute`, `decide`,
and `process_due_schedules`; the scoped source searches above supplied those caller links.
`graft grep SkillInstruction` found 87 hits in 17 indexed files and the displayed result was
capped; its authoring, persistence, and both harness subtrees were searched separately. The
`default_agent_harness` search found 90 hits in 29 indexed files and was likewise narrowed to
domain, application, adapter, UI, and infra subtrees. Ranked `ask` results were never treated as
an exhaustive caller list. Migration SQL and plan/docs are outside the indexed Rust coverage and
were searched directly. Re-run these discovery queries before phase 08 removal because the code
will have moved by then.

Representative invariant tests to retain or replace, separate from production entry points:
`src/application/use_cases/thread/external_reply_tests.rs:482–828`,
`src/application/use_cases/thread/rig_execution_tests.rs:51–220,413–499`,
`src/application/use_cases/thread/structured_response_tests.rs:21–393`,
`src/application/services/task_worker.rs:2825–3271`,
`src/adapters/persistence/task/tests.rs:1164–1307,3143–3393,3525–3800`,
`src/adapters/persistence/schedule.rs:812–963`,
`src/application/use_cases/approval.rs:1155–1274`,
`src/application/use_cases/agent_tests.rs:491–587`,
`src/adapters/persistence/channel.rs:1052–1258`,
`src/adapters/harness/rig/skills_tests.rs:171–202`, and
`src/domain/entities/harness.rs:610–693`.
