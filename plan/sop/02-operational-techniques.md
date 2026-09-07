# Plan 2 — Evidence-Informed Operating Techniques for SOPs

## Outcome

Extend the durable procedure foundation from [Plan 1](01-procedure-execution-foundation.md) with
concrete techniques for clearer work, safer coordination, and continuous improvement across human
and agent participants.

The product should help teams do five things well:

1. Explain work so a capable participant can perform it correctly.
2. Catch critical omissions at the point where they matter.
3. Transfer authority and uncertainty without losing context.
4. See flow, waiting, overload, and rework across the whole procedure.
5. Test procedure changes and retain what actually improves outcomes.

The techniques are adapted from established operating and teaching methods. Product behavior must
be evaluated against observed outcomes; naming a technique is not evidence that its implementation
works.

## Source methods and product interpretation

| Source | Useful method | Product interpretation |
| --- | --- | --- |
| [Training Within Industry (TWI) Job Instruction](https://www.twi-institute.com/job-instruction/) | Break a job into what to do, how/key points, and why; demonstrate, practice, and check progress | Structured job breakdowns, examples, practice mode, and result criteria shared by humans and agents |
| [AHRQ TeamSTEPPS tools](https://www.ahrq.gov/teamstepps-program/resources/modules/index.html) | Briefs, huddles, debriefs, closed-loop communication, structured handoffs, mutual support | Run briefs, acknowledged handoffs, clarification loops, exception escalation, and short debriefs |
| [AHRQ handoff guidance](https://www.ahrq.gov/teamstepps-program/curriculum/communication/tools/handoff.html) | Transfer information together with authority/responsibility; receiver acknowledges and can question it | Offered/accepted handoffs for selected risk levels, explicit uncertainty and contingencies |
| [The Checklist Manifesto](https://us.macmillan.com/books/9780312430009/thechecklistmanifesto/) and the [WHO checklist implementation manual](https://www.who.int/publications/i/item/9789241598590) | Short checks and deliberate pause points in complex work; local adaptation, coaching, and feedback | Critical verification at transitions, evidence-backed checks, local templates, and measured adoption |
| [Lean standardized work](https://www.lean.org/lexicon-terms/standardized-work/) | Make sequence, timing, and work in progress visible as a baseline for improvement | Work/wait separation, active-work limits, bottleneck views, and version-to-version comparison |
| [IHI Model for Improvement](https://www.ihi.org/library/model-for-improvement) | State an aim and measures, then test changes through Plan–Do–Study–Act cycles | Hypothesis-bearing draft versions, bounded pilots, comparison, review, and adopt/adapt/abandon decisions |
| [Fundamentals of Business Process Management](https://fundamentals-of-bpm.org/) | Discover, model, analyze, redesign, automate, and monitor end-to-end processes | Explicit outcomes/exceptions, event-based process analysis, and redesign grounded in actual runs |

Do not reproduce proprietary worksheets or long source text. Use the underlying techniques and
plain product vocabulary, retaining links and attribution in authoring guidance.

## Dependency on Plan 1

This plan begins only after Plan 1's versioning, result acceptance, events, ownership fencing,
recovery, and baseline measures pass their release gates. Extend these Plan 1 contracts:

- `ProcedureDefinitionV1` becomes a tagged `ProcedureDefinitionV2`; V1 remains readable and
  runnable without silently gaining V2 behavior.
- `job_breakdown`, `verification`, `coordination`, `exception_policy`, `measures`,
  `work_in_progress_policy`, `improvement_policy`, `decision_outcomes`, and `review_policy` become
  validated typed sections.
- New behavior writes to the existing procedure, occurrence, result, task ownership, approval,
  and event records.
- Technique-specific views derive from the shared event ledger and source records.
- Technique configuration is copied into the immutable published version and therefore remains
  stable for a run.

## Technique 1: job breakdowns that explain action, judgment, and purpose

### Definition

Add an optional `job_breakdown` to a step:

- `important_steps`: 1–12 ordered units of meaningful progress;
- for each unit, a short `action`, 0–5 `key_points`, and a corresponding `reason` for each key
  point;
- optional positive example, counterexample, common failure, and recovery guidance;
- `completion_criteria` linked to fields or evidence in the declared step result; and
- `competency_mode`: `reference`, `guided_first_run`, or `practice_required`.

An action says what changes. A key point identifies a detail that affects safety, quality, ease,
or success. A reason explains why the key point matters. The editor warns when these are merely
three paraphrases of the same sentence, when a step has no observable completion criterion, or
when the breakdown has become a long policy manual.

### Execution

- Human task views progressively disclose the action, key points/reasons, examples, inputs, and
  completion criteria. Keep the essential action and current evidence visible while work is done.
- Agent prompts render the same structured content into delimited instructions. Require the
  structured result fields that correspond to completion criteria; do not ask the model to assert
  vaguely that it followed the SOP.
- In `guided_first_run`, require the participant to confirm or demonstrate each important step in
  the result. `practice_required` creates a non-production simulation occurrence whose output a
  qualified reviewer assesses before the role can be bound to production work.
- Track guidance opens and confirmations as low-value interaction telemetry only when useful for
  usability. Never treat clicks as proof that the work was done correctly.

### Authoring assistance

Offer an agent-assisted draft from existing instructions or exemplary completed cases, but mark it
as a proposal. Publishing requires a procedure owner to verify action, key points, reasons,
examples, criteria, and authorized source material. The drafting agent cannot publish its own
rewrite.

## Technique 2: critical checks and deliberate pause points

### Definition

Add `verification` to selected steps or transitions:

- mode: `read_do` for unfamiliar/infrequent sequences or `do_confirm` for skilled work checked
  after execution;
- a bounded list of critical checks, each with stable key, concise prompt, responsible role,
  required evidence type, and failure action;
- a pause point: before starting, before accepting a result, before an irreversible effect, or
  before completing the run;
- optional independent verifier role and separation-of-duties rule; and
- a reason explaining the consequence the check controls.

Reserve required checks for consequential omissions. Routine explanatory detail belongs in the job
breakdown. The editor displays estimated check count and frequency, warns about duplicated or
unverifiable checks, and requires a failure action for every critical check.

### Execution and integrity

- At a pause point, display the exact proposed artifact or action payload, current evidence, and
  changes since any earlier review. Bind verification to a canonical hash and version.
- A stale verification fails if the result, recipients, payload, or relevant source state changes.
- Evidence may be a structured value or reference to a source record; a bare checkbox is allowed
  only for explicitly low-risk confirmation.
- Failed checks route through their declared action: correct within the same occurrence, request
  rework, create an approval, ask the coordinator, or stop the run.
- Existing approval policy remains the authorization boundary for protected tools and effects.
  Procedure verification records quality/safety confirmation and must not grant broader tool or
  data access.
- Use the current response-draft and delivery machinery for external publication. Verification
  accepts the frozen artifact that will be sent, not a prompt describing what an agent intends to
  produce.

### Anti-fatigue controls

Measure verification volume, failure/catch rate, time, overrides, rework caused, and defects that
escaped despite a pass. Flag checks that are always passed, never catch anything, or regularly
require exceptions for owner review. Do not auto-delete them; evidence may show that a preventive
check is valuable even with few failures.

## Technique 3: structured team coordination and closed-loop handoffs

### Run brief

Add optional `coordination.run_brief` fields shown when a run starts:

- objective and expected successful outcome;
- coordinator and role bindings;
- relevant context and known constraints;
- anticipated risks and uncertainties;
- planned parallel work and dependencies;
- stop/escalation conditions; and
- opportunity for required roles to acknowledge or raise a clarification.

For routine low-risk runs, the brief is an automatically generated summary requiring no meeting or
extra step. For exceptional or high-risk runs, it can be a short explicit coordination occurrence.

### Handoff contract

Extend task transfer commands with a structured handoff record while retaining the existing
bounded private instruction for compatibility. The structured record contains:

- current situation and desired next outcome;
- work completed and result/evidence references;
- open questions, assumptions, and degree of uncertainty;
- next recommended action and due time;
- risks, escalation triggers, and contingencies; and
- sender and intended receiver snapshots.

Add handoff policy per step: `immediate`, `acknowledged`, or `synchronous_review`.

- `immediate` uses the current atomic transfer and notification behavior.
- `acknowledged` records an offer while the current owner remains accountable. The receiver may
  accept, decline with reason, or ask a bounded clarification question. Acceptance atomically
  performs the existing fenced transfer.
- `synchronous_review` uses the same durable offer/accept protocol but requires a designated
  communication channel or meeting reference before acceptance; the system does not pretend that
  an electronic notification proves mutual understanding.

Handoff offers have expiry, reminder/escalation policy, idempotent commands, and one active offer
per task. Transfer racing with task completion or another offer follows the task ownership version
fence. Decline/expiry leaves the sender as owner and routes the declared escalation; it never makes
the task silently unassigned.

### Check-back and clarification

Allow the receiver to restate the requested outcome and uncertainty in a short acknowledgment.
The sender can confirm or correct it before a high-risk transfer becomes effective. Persist these
messages as private handoff records with strict size and round limits; they are not canonical
customer conversation and cannot become an unbounded chat system.

### Huddles and mutual support

Add a coordinator “replan” command for a running procedure. It shows blocked/overdue work,
capacity, changed assumptions, and current risks, then permits bounded role rebinding, task
transfer, due-time changes, waiver, or rework using existing fenced commands. Record the reason and
the set of reviewed state versions. A participant can request assistance or raise a stop condition
without relinquishing ownership; this creates coordinator attention and a durable event.

## Technique 4: explicit decisions, exceptions, and rework

### Decision outcomes

Make a decision result declare bounded named outcomes such as `approved`, `needs_revision`, or
`not_applicable`. Each outcome maps to one of a small set of server-executed transitions:

- satisfy the step and release configured successors;
- request rework at a named earlier step;
- waive named optional successors with a reason;
- create coordinator attention; or
- cancel the run under an authorized terminal reason.

Publishing validates every target, verifies that rework is bounded by a maximum cycle count, and
proves that required terminal steps remain reachable. User-authored expressions and scripts remain
out of scope.

### Exceptions

Add typed exception reasons: missing input, conflicting evidence, policy ambiguity, unavailable
role, failed critical check, external wait, capacity limit, and other-with-bounded-detail. A step's
`exception_policy` selects allowed actions, coordinator/approver role, response deadline,
reminders, and terminal behavior.

An agent may report an exception and propose an action but cannot waive a required check or expand
its authority unless policy explicitly permits its bound role to do so. Every waiver records the
requirement, actor authority, evidence/result version, reason, and effect on terminal success.

## Technique 5: short debriefs and learning from completed work

### Debrief trigger and form

Configure `coordination.debrief` as `never`, `sampled`, `exception_only`, or `always`, with a
maximum time/field budget. Trigger a debrief after completion/cancellation when the run had rework,
missed deadlines, critical-check failure, waiver, unexpected escalation, high cost, or a sampled
ordinary outcome.

Ask a small stable set of questions:

- Did the run achieve its intended outcome?
- What helped?
- What created delay, uncertainty, error, or unnecessary work?
- Which procedure instruction or check should change, if any?
- Is the observation specific to this case or likely systemic?

Store responses as structured `procedure_run_debriefs` linked to event/result evidence. Permit
human and agent observations, identify provenance, and distinguish facts from proposed changes.
Debriefing never edits the procedure version that just ran.

### Improvement backlog

Create deduplicated `procedure_improvement_items` from authorized debrief submissions or manager
analysis. Each item has category, evidence run IDs, impact/frequency estimates, owner, state, and a
link to a proposed draft version or experiment. Agent clustering and summaries are suggestions;
humans retain merge, prioritization, and publication authority.

The debrief UI focuses on the process and evidence. Avoid participant scoreboards and generated
blame narratives. Access to source runs remains governed by their original thread/channel ACLs.

## Technique 6: flow management and bottleneck visibility

### Measures

Make Plan 1's baseline timestamps into explicit, versioned measures:

- outcome: successful run rate, first-pass acceptance, escaped defect/correction rate, and customer
  or downstream result where available;
- flow: total lead time, touch time, waiting time, queue age, handoff latency, and review latency;
- quality: rework cycles, exception/waiver rate, critical-check catches, and incomplete evidence;
- capacity: active work per role/principal, ready backlog, oldest ready item, and blocked duration;
- automation: agent runtime, tool/provider failure, token/cost totals, human correction time, and
  percentage requiring human intervention; and
- balancing measures: overdue work, abandonment, human workload, external-delivery failure, and
  unfair concentration of work across a team.

Definitions name the aim and a small set of primary/balancing measures. They reference metrics
from a controlled catalogue; users cannot create unbounded metric labels or SQL expressions.
Maintain precise metric semantics and version them when formulas change.

### Work-in-progress policy

Add advisory WIP limits by procedure, role, or step. At first, show overload while allowing normal
activation. A company may enable enforcement only after observing its real capacity; enforcement
leaves excess occurrences durably `ready` for the materializer rather than discarding work.
Coordinator overrides are version-fenced, bounded, and audited.

Use work-conserving, fair task materialization across companies and roles. A full batch counts as
backlog only after rows have moved out of the claimable set. Add the poison-batch and competing
claimant tests required by repository policy.

### Views

Provide a flow view of work time versus wait time, active/ready/blocked counts, first-pass yield,
rework paths, and step-to-step transition time by procedure version. Only compare cohorts with the
same metric semantics and sufficient sample count. Display distributions and percentiles alongside
averages so a few long-running cases remain visible.

## Technique 7: controlled PDSA-style procedure improvement

### Improvement proposal

Extend a draft procedure version with `improvement_policy` and an optional change experiment:

- problem statement and evidence links;
- specific aim and target population;
- proposed change and prediction;
- primary and balancing measures;
- pilot size/time bounds and eligibility rules;
- stop conditions and responsible human owner; and
- decision choices after study: adopt, adapt, abandon, or gather more evidence.

### Pilot assignment

Publishing an experimental version does not replace the active standard. A deterministic,
auditable allocator assigns only eligible new runs to the pilot, capped by count and time. Record
the assignment before any step begins and pin each run to its assigned version. Exclude emergency,
restricted, or otherwise ineligible cases by explicit policy.

Randomized allocation is optional and should be added only with enough volume and appropriate
review. Initial pilots may use a time-boxed or selected-case cohort, but the UI must state the
limits of causal conclusions.

### Study and decision

At pilot end, freeze the cohort and generate a review containing measure definitions, missing data,
sample sizes, distributions, exceptions, qualitative debrief themes, and operational costs. An
agent may summarize and identify patterns but must link claims to durable runs/events and expose
uncertainty.

Only an authorized human procedure owner records the adopt/adapt/abandon decision. Adoption
publishes a normal version and changes the active pointer for future runs. Adaptation creates a new
draft/experiment. Existing runs never migrate automatically.

## Product authoring workflow

The editor should guide authors through a small sequence:

1. State the procedure's purpose, customer/downstream outcome, owner, and start trigger.
2. Identify roles and the end-to-end flow, including waits, decisions, and exceptions.
3. Give each work step an expected result and acceptance criteria.
4. Add job breakdown detail only where knowledge or judgment needs teaching.
5. Add critical checks only for consequential omissions and bind them to evidence/failure actions.
6. Choose coordination policies for starts, risky handoffs, escalation, and debriefs.
7. Select a few outcome, flow, and balancing measures.
8. Validate the graph, permissions, capacity bounds, result schemas, and technique burden.
9. Pilot the version before broad adoption when the change is material.

Provide authoring diagnostics as warnings versus publication errors. Missing referenced roles,
unbounded rework, unverifiable required evidence, invalid separation of duties, and unreachable
terminal success are errors. Length, excessive checks, excessive coordination, and weak reasons
are warnings requiring author judgment.

## Implementation sequence

### 1. V2 schema and migration compatibility

Implement tagged V2 parsing, migration/preview from V1, typed technique sections, validation, and
round-trip tests. V1 continues to execute unchanged. Extend the editor before enabling runtime
behavior so teams can review the complete published contract.

### 2. Job breakdown and critical verification

Add structured step rendering for humans/agents, completion-criterion mapping, verification pause
points, hash-bound evidence, failure routes, and workload metrics. Pilot on agent-draft/human-review
work where accepted artifacts and corrections are already observable.

### 3. Coordination and handoff acknowledgment

Add run briefs, structured handoff offers, accept/decline/clarify commands, expiry/escalation,
assistance requests, and coordinator replanning. Route effective transfers through the existing
task ownership transaction and cancellation wake-up.

### 4. Decisions, exceptions, and bounded rework

Add named decision outcomes, validated server transition mappings, exception policies, waivers,
and rework limits. Ensure all paths converge on completion, cancellation, or observable
coordinator attention.

### 5. Debriefs, measures, and flow views

Add trigger policies, debrief storage, improvement backlog, version-aware metric projections, WIP
views, and advisory limits. Establish retention and access policy before aggregating run data.

### 6. Controlled improvement experiments

Add experiment definitions, deterministic bounded assignment, cohort freeze, study reports, and
human adoption decisions. Begin with opt-in pilot companies and one procedure family.

## Test and evaluation plan

### Contract and concurrency tests

- V1/V2 definitions retain their original behavior and hash through round trips.
- Verification bound to an old result/artifact version cannot approve a changed effect.
- Two handoff acceptors, acceptance versus completion, offer expiry versus acceptance, and
  transfer versus cancellation produce one fenced outcome.
- Parallel decision results activate each successor once; rework caps cannot be bypassed by retry
  or duplicate command IDs.
- WIP materializers use competing claimants and poison-batch tests and remain fair under one
  company's burst.
- Pilot assignment is stable under retries and concurrent starters and never moves an existing
  run to a different version.

### Authorization and confidentiality tests

- Procedure roles, task ownership, verifier separation, coordinator powers, and procedure-owner
  publication authority are checked separately.
- Agent-authored observations cannot approve checks, waive policy, or publish/adopt procedure
  changes without the declared authority.
- Private handoff, debrief, and improvement evidence never enter canonical customer messages,
  delivery payloads, notifications, logs, or unauthorized aggregate drill-downs.
- Revoked principals and tool grants fail closed at the moment of effect, even after prior
  acknowledgment or verification.

### Performance evaluation

For each technique, define a baseline and success/stop rule before the pilot. Compare at least:

- first-pass acceptance and observed output defects;
- total lead time split into work and waiting;
- human touch/correction time;
- rework and escalation frequency;
- verification and handoff burden;
- agent runtime/cost and technical failure; and
- balancing outcomes such as overdue work, abandonment, or workload concentration.

Instrumentation and samples must be large and stable enough for the claimed conclusion. Report
missing data and cohort differences. A shorter agent runtime does not count as improvement when
human correction, waiting, or downstream errors increase.

### Usability evaluation

Observe procedure authors and participants completing representative work. Check whether they can
identify the next action, required result, owner, uncertainty, failed check, and escalation path
without reading implementation terminology. Measure time and errors, then revise labels and
information density before broad release.

## Operational and ethical safeguards

- Treat agent suggestions, generated handoffs, inferred causes, and debrief summaries as claims
  with provenance, not facts.
- Do not turn SOP adherence metrics into individual performance scores without a separately
  reviewed organizational policy and valid measurement design.
- Keep audit retention distinct from analytics retention. Aggregate or minimize content after the
  operational need ends and preserve source ACLs on drill-down.
- Let participants report that the procedure is wrong, unsafe, or inapplicable and route that
  report to a human coordinator. A procedure is a current best-known method, not authority to
  ignore contradictory evidence.
- Keep approval volume and human attention bounded. Route humans to consequential uncertainty and
  effects; measure whether prompts become routine acknowledgments.

## Rollout gates

1. Plan 1 production-shaped concurrency/recovery tests and baseline instrumentation pass.
2. One procedure owner reviews V2 authoring and can explain every required check and escalation.
3. Run job breakdown and verification in observe-only mode before they block progress.
4. Enable acknowledged handoffs for selected high-risk steps, retaining immediate transfer for
   ordinary work.
5. Enable WIP enforcement only after advisory metrics expose realistic capacity and override
   behavior.
6. Start improvement experiments only after cohort assignment, measure semantics, and decision
   authority are independently reviewed.
7. Expand each technique based on outcome and balancing measures, not feature adoption counts.

## Acceptance criteria

- Humans and agents receive the same structured action, key points/reasons, required result, and
  acceptance criteria where a job breakdown is configured.
- Critical checks occur at declared pause points, bind to exact evidence/artifact versions, and
  have explicit failure paths.
- Selected handoffs transfer responsibility only after durable receiver acknowledgment and allow
  clarification without losing the current accountable owner.
- Decisions, waivers, exceptions, and rework follow bounded server-validated transitions and leave
  complete evidence-linked histories.
- Teams can see work time, waiting, rework, overload, and version-level outcomes without building a
  second operational state model.
- Debriefs create reviewable improvement items; they never mutate active or historical procedure
  definitions.
- A pilot version can be tested on a bounded cohort and adopted, adapted, or abandoned by an
  authorized human without moving existing runs.
- At least one pilot demonstrates improved outcome or flow measures without unacceptable movement
  in its balancing measures before the techniques become defaults.

## Out of scope

- Claiming universal effectiveness from a named method or from interaction/adoption metrics.
- Automatically publishing agent-generated SOPs, checks, waivers, or experiment conclusions.
- Arbitrary process expressions, executable BPMN, open-ended handoff chat, or unlimited rework.
- Surveillance-oriented individual rankings, hidden productivity scoring, or cross-ACL content
  aggregation.
- Replacing domain-specific regulation, professional judgment, or safety review with this generic
  procedure framework.
