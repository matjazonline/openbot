# 03.3 — bounded pure transactional batches

Status: reconciled expansion; awaiting root coverage acceptance before code.
Original authority:03-durable-runtime-and-persistence.md Execution and commits
item2, retaining its nested execution/failure/acceptance requirements.03.1/03.2
are accepted. Scope is pure `data.map` and `decision.rule`; no worker loop or I/O.

## Reconciled implementation contract

- Application owns a narrow batch port, validated batch budget, and synchronous
  pure handler. Mapping returns the frozen `value`; ordered rules return declared
  `choice` and frozen `data`. Reuse compiler output validation and domain routing;
  unsupported step types produce a boundary result, never an invented handler.
- Decision predicates are stored separately from `with` bindings in the compiler.
  Extend activation to freeze their selected choice alongside ordinary inputs,
  include their referenced dependencies in bounded context loading, and persist
  the choice atomically with the existing activation tuple. Replays never evaluate
  predicates again. Additive immutable choice column/guard; no changes to applied
  migration130000. This is an integration extension to03.2, requiring its tests.
- One run-first transaction owns pure advancement, then execution and job locks.
  Only the exact scoped pending, due, unleased workflow job may start advancement.
  A terminal/completed duplicate returns saved progression without following its
  successor and executing another batch. A processing/leased or foreign job fails
  closed.03.4 will integrate fenced I/O ownership separately; a batch call is not
  an external-effect permission or lease claim. No task_attempts parallel ledger.
- Add run state matching domain semantics (`queued`, `running`, `waiting`,
  `succeeded`, `failed`, `cancelled`, with waiting reason when relevant), immutable
  execution route/target and scoped successor identity. Admission defaults queued;
  first batch starts running. End route marks succeeded (business rejection is
  still successful execution), records terminal execution, and validates workflow
  output schema. Existing terminal/waiting runs cannot advance. Error/retry/final
  error routing is not fabricated here: a failed preparation/validation rolls the
  transaction back; classified durable failure/backoff belongs to03.5/03.9.
- Each pure completion writes inputs/output/selected route, completes its existing
  job, and allocates at most one successor execution with next run-wide ordinal
  and an ID-only background_tasks job, all inside the same transaction. Add scoped
  database constraints for successor relationships, immutable route facts, and
  one workflow job per logical execution. Distinct worker attempts reuse the job.
  A compact run audit event uses the existing workflow_run_events owner and saved
  admitting actor; no new audit ledger.03.5 generalizes this atomic progression
  seam to non-pure/fenced outcomes; it must not create a competing commit path.
- Stop after a validated positive per-call step cap (hard ceiling64) or bounded
  elapsed transaction work budget; bound lock/statement waits too. Each value obeys
  the admitted context/output limits, so aggregate batch work is bounded. The next
  pending successor job is the continuation, including at an I/O boundary; no
  recursive call/spin or auxiliary queue. Return typed completed/yielded/boundary/
  replay disposition with identifiers, not serialized context in jobs.
- Recheck deadline at writes/final transaction completion; enforce max_steps
  before allocating successors. No replenishment on retries. Parent-child wakeups,
  waits, polling fairness, worker heartbeats and poison backoff retain their later
  owners. Public API wiring/cutover is outside03.3.

## Affected seams and acceptance

Application workflow batch module; persistence workflow batch/commit helpers;
activation application/adapter extension; additive migration newer than130000;
isolated DB batch tests and affected activation fixture updates. Existing graph
entry points: activation.rs activate_on33–87/resolve106–144; application activation
input_dependencies57–107; compiler rule88–90; domain OrderedRule::decide47–61,
select_route29–62, RunState start/apply. Caller tracing found activation helper
used only by its port and focused tests; input_dependencies only by its resolver.

Required checks:

1. Map→ordered rule→map/end resolves committed outputs and records every input,
   output and route. First matching predicate/default selection and JSON null;
   terminal workflow output validation. Frozen rule choice survives changed
   available predecessor context, lost acknowledgement and replay.
2. Small budget yields exactly one pending continuation with ID-only payload;
   later batch resumes it. Non-pure boundary stays pending and unexecuted.
   Duplicate original job replay does not execute that continuation.
3. Two synchronized independent DB callers cannot double-complete or create extra
   successor/audit records. Run-first contention deadline expiry fails closed.
   Reject foreign/mismatched/leased/not-due/terminal run requests without writes.
4. Inject failure after activation, output, audit and successor staging; full
   rollback restores original job and allows clean retry. Lost commit response and
   reconnect return saved progression. Direct SQL rejects route/choice mutation,
   foreign successor and duplicate logical job. Bounds cover invalid/zero/excessive
   batch cap, run deadline/max_steps, invalid output, and repeated step ordinals.
5. Applied migrations immutable; task-only PG retained on55439. Run isolated
   migration, SQLx prepare sequential with builds, stock2MiB focused activation+
   batch tests, mandatory full DB library suite, locked offline all-target check
   and Clippy, fmt/diff, graph refresh, then independent actual-code/combined review.
   Preserve all existing edits and external index.html work; no stage/commit/reset.

Root owns PROGRESS/RESUME acceptance. Implementer owns this brief. Same nested
reviewer remains eligible below50%; implementation pauses during actual review.
