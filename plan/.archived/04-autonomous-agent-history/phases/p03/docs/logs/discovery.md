# P03 owner discovery (orientation, not an approved design)

Accepted source c24664b2 (P02 B1-R2 + root acceptance), coord6 C002r5/C003r2.
No P03 source changes or gated D1 yet. Original03 and07 requirements read fully.
P03 dedicated pair openbot_ah_p03(_test) created and baseline migrations PASS,
proving acceptedP02 schema only. PG65037/85060 root-owned; no active Cargo/DB checks.
Same9 exclusions;82 accepted owned dependency paths,36 external preserved inputs.

Graft exact owner context already read:
- waits.rs8–23 row types;64–92 sweep_waits capped128 and each resume;336–343
  select_due only deadline/terminal candidates (would need readiness integration).
- polling.rs98–151 CANDIDATES_SQL: generic waiting row becomes PollWork::Wait;
  readiness includes event, child terminal, effect receipt/conflict and deadlines.
  No native per-conversation loop needed. Preserve bounded cursor/page owner.
- wait_commit.rs5–118 full resume_on read: prelock channel reply/company then expiry,
  waits::lock (run/execution), wait row, typed branch handling; model effect delivery
  continues same job rather than completes business output. Existing source function
  >80lines: extract new branch rather than extend its violation.
- wait_commit.rs123–138 continue_delivery settles exact effect wait, makes run
  running, approvals::reopen_job, audits, returns WaitProgress::Continued.
- decision_approvals.rs46–95 actual approval parking,125–154 close/parked proof,
 220–227 confirm and263–273 reopen_job read. close_attempt is decision-specific:
  completed attempt stop_reason approval_parked, completed job cleared worker/fence,
  run waiting reason decision. Do not reuse this verbatim for an agent-turn reason.
  reopen_job completed→pending increments existing continuation_count under native
  limit; actual retry/model allowance unchanged. Native limit function is existing.
- lease.rs170–187 confirm_suspension tries message receipt, workflow tool child,
  then approval; new ordinary turn suspension needs its own exact attempt/wait proof.
- lease_claim.rs6–88 claim_on and168–189 install read: source run/job lock, immutable
  binding/step kind, due/reconciliation/fairness gates, source activation, new fence,
  processing job/attempt; triggers own counters. No provider call at claim.
- worker.rs172–243 PollWork::Wait calls resume_wait without active-job handler slot;
  Job path active guard and I/O execute.299–330 fail_on_guard handles native guard
  violations. Need read141–170 process error path for current authority refusal
  semantics before designing turn-wait resume; avoid a permanently failing head.

Traces: reopen_job has4 callers (action approval, token allowance, effect delivery,
workflow tool child); workflow_tool_calls327–350 continue_call and child_waits25–66
are remaining useful native examples. Existing global agent-before-local fence
boundary is agent_history/preparation::locked_on; use current source/association/
agent authority before local run/conversation for any new turn-wait resume too.
Dynamic prepare_agent calls are ambiguous to Graft edges; exhaustive grep required
before API changes. Actual port in application/workflow/agent.rs265–269, one native
implementation, production Rig workflow caller plus test callers. A preparation
that parks must be explicit control flow, preserve no credential/provider entry,
and not be mapped to a provider failure. Final choice needs D1 gate, no code yet.

SQL is unindexed: relevant init ranges now5825+ workflow_waits definition;
check_workflow_fenced_completion around145–235 and later completed→pending branch;
workflow_job_continuation_limit around3037. Last combined read output TRUNCATED
for these SQL spans. Read complete exact chunks before finalizing D1. Need exhaustive
native CHECK/trigger/caller surfaces for new wait/run/attempt reason, continuations,
run state transition, poll readiness and deadline/terminal cleanup. Source eligibility
must remain actual frozen agent.run; generic/model-only fixtures remain isolated.

Unresolved design points to investigate/resolve in one cohesive D1:
1. Durable per-conversation active identity before checkpoint (Queued exchange may
   own turn while checkpoint absent), native fencing and pointer release through
   both checkpoint update and model settlement, all terminal hooks. Reopened older
   pending positions cannot displace newer active or be lost behind a completed
   watermark. Existing lease governs worker replacement; logical turn survives waits.
2. Reuse native durable waits + poll/sweep readiness. To avoid repeated wakes/reparks
   consuming continuation allowance, investigate reserving the eligible turn and
   reopening its waiting job atomically, with current global authority before local
   locks. Wake notifications are hints only; readiness SQL must reconstruct after
   crash/no wakeup. Blocked handler returns Suspended, no held connection/worker slot.
3. Finite per-conversation/tenant pending admission bounds and hard query/work/deadline
   bounds. Counter/allocation/terminal/control-retry lock order needs explicit cycle
   analysis: no waiting on another source run/job under conversation lock. Preserve
   legitimate control retry and accounting; overload must be controlled/atomic.
4. Current authority failure at queued-head resume must settle via a genuine existing
   owner rather than hot retry/strand successors. Trace worker guard handling and
   typed capability refusal/classification before deciding the boundary.

These are investigation questions, not reviewed decisions. Do not implement or
start a dependent phase until the cohesive P03 architecture has independent B PASS.
