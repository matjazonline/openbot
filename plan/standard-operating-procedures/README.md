# Standard operating procedures on the workflow engine

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and status

Make a company procedure understandable, executable, observable, and improvable by humans and
agents. An SOP describes the business outcome, responsibilities, instructions, required evidence,
checks, and improvement measures of an ordinary workflow. The implemented workflow engine remains
the only owner of execution.

This is proposed implementation work, not a description of shipped SOP features. It replaces the
removed SOP proposals. Their separate procedure definitions, procedure runs, step occurrences,
result acceptance engine, background-task dispatch, and V1/V2 compatibility design are discarded.
Existing workflow versions, runs, executions, outputs, human decisions, waits, actions, and delivery
receipts supply those execution capabilities.

For example, invoice handling becomes:

```text
start an ordinary workflow run
  -> agent extracts invoice data using the published job guidance
  -> deterministic checks validate required evidence
  -> a human reviews the exact invoice proposal and resolves exceptions
  -> an existing action performs the authorized operation
  -> the run timeline shows work, waiting, checks, and rework
  -> a separate debrief may propose an improvement to a future workflow version
```

## Implementation groups

| Group | Deliverable | Prerequisites |
| --- | --- | --- |
| [01 — Shared contracts](01-shared-contracts-and-workflow-integration.md) | SOP definition fields, role/evidence contracts, publication identity, event vocabulary, and fixtures | Existing workflow engine |
| [02 — Guidance and authoring](02-guidance-and-authoring.md) | Job breakdowns, observable completion criteria, human/agent rendering, and sample practice | 01 |
| [03 — Verification](03-verification-and-evidence.md) | Evidence validation, deliberate pause points, independent review, and checks bound to exact artifacts | 01 |
| [04 — Coordination](04-coordination-and-handoffs.md) | Run briefs, responsibility views, acknowledged human handoffs, and coordinator escalation | 01 |
| [05 — Flow and capacity](05-flow-measures-and-capacity.md) | Business flow measures and advisory WIP; a later batch adds enforced WIP | 01; enforced WIP also needs 04 |
| [06 — Debriefs](06-debriefs-and-improvement-backlog.md) | Bounded debriefs, evidence-linked observations, and a human-owned improvement backlog | 01 |
| [07 — Improvement experiments](07-controlled-improvement-experiments.md) | Explicit pilot cohorts, version comparisons, and human adoption decisions | 02, 05A, 06 |
| [08 — Integration and release](08-integration-verification-and-release.md) | End-to-end journeys, full verification gates, operating guidance, and pilot acceptance | Gates depend on the milestone below |

Each numbered file defines focused batches and acceptance criteria. Schema and application names
in them are implementation targets. Do not advertise a configuration key or validation guarantee
until the application reads and enforces it.

## What can be implemented asynchronously

Here, asynchronous implementation means independent workstreams with stable contracts, not
independent execution engines. Complete and merge 01 before concurrent runtime implementation.
Before that gate, work on examples, UI sketches, and evaluation cases may proceed independently.

After 01, these tracks can proceed in parallel:

| Track | Owned implementation | Shared integration points |
| --- | --- | --- |
| A | 02 guidance and its editor/view components | Published SOP contract and agent context assembly |
| B | 03 evidence/check validation and verification components | Compiler, human decisions, and shared action entry |
| C | 04 responsibility/handoff commands and coordination components | Human-decision transactions and wait maintenance |
| D | 05A read projections and advisory flow views | Run/execution records and the metric catalogue |
| E | 06 debrief records and improvement views | Authorized run references and terminal-run observations |

03 and 04 can develop their domain rules, services, and tests concurrently, but changes to shared
decision persistence must be integrated by one owner in sequence. 02 and 03 share editor assembly;
add their components independently and serialize the final wiring. Do not merge conflicting
edits to a migration, SQLx metadata entry, compiler wire format, route assembly, or shared fixture
without re-running the affected checks.

Assign one integration owner for `migrations/`, `.sqlx/`, shared compiler/publication types,
decision/wait SQL, admission/capacity SQL, route registration, and shared CSS. Other tracks propose
changes through that owner or use separate modules until integration. Workstreams use isolated
branches/worktrees and exchange the contract/fixture revision and focused-check results; do not
let concurrent work silently redefine shared fields or transaction order.

05B starts after 04 and 05A. 07 starts after 02, 05A, and 06. Capacity enforcement and experiment
admission both touch admission/capacity ownership: integrate their SQL changes sequentially.
08's scenario design and test fixtures start during 01; its release gates wait for actual features.

## What runs asynchronously in the product

- Human review, handoff acknowledgment, clarification deadlines, and capacity waits park through
  workflow-owned durable state; no worker sleeps while waiting for a person.
- Notifications use durable delivery. Reminder/deadline work extends the existing maintenance
  paths and preserves lease, retry, fairness, and poison-batch behavior.
- A debrief or improvement suggestion may arrive after a run ends. It cannot reopen or change the
  original run. Any model-assisted analysis is an ordinary bounded workflow with explicit input.
- Large aggregate refreshes and experiment reports use bounded, restartable work with supervised
  shutdown. Read projections may lag; authorization, capacity, and pilot caps use source state in
  the deciding transaction.

## Milestones and scope

1. **Executable SOP experience:** 01–04 and 05A, with the first release gate in 08. A person can
   author guidance, run a case, verify its result, hand off a human decision, and inspect progress.
2. **Flow and learning:** 05B and 06, with a fresh gate in 08. Capacity enforcement is separately
   enabled only after advisory measures have established useful limits.
3. **Controlled improvement:** 07 and the final gate in 08. Humans test and adopt future versions
   using evidence, without moving existing runs between versions.

These are delivery increments using the current schema, not staged data compatibility rollouts.
Each milestone runs the full verification suite once after its implementation is complete.

Excluded: another SOP scheduler, general human-task engine, arbitrary scripts/BPMN import,
automatic publication/adoption by agents, individual performance rankings, a training/certification
system, and the historical workflow-library installation engine. Guidance practice uses isolated
workflow samples; mandatory production competency qualification needs a separate scope.

## Verification and evidence

Follow [08](08-integration-verification-and-release.md) for exact commands and the final full-suite
gate. Every batch records its changed interfaces, command, result, executed-test count, and any
unavailable check. A test filter matching zero tests is not verification. Keep focused checks
small during implementation; broaden early for compiler/publication, decision, or admission changes.
Every new concurrency protocol must include real competing claimants.

The graph tool and index were unavailable while this plan was scoped. Current anchors are
[workflow operations](../../docs/workflow-operations.md),
[release verification](../../docs/workflow-verification.md), and the source inventory in
[01](01-shared-contracts-and-workflow-integration.md). Re-query and refresh the graph when available.
