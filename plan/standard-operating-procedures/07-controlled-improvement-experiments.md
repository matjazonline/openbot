# 07 — Controlled experiments on future workflow versions

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependencies

Let a procedure owner test a specific improvement and explicitly adopt, adapt, or abandon it.
Depends on [02](02-guidance-and-authoring.md),
[05A](05-flow-measures-and-capacity.md), and
[06](06-debriefs-and-improvement-backlog.md). This group can proceed alongside 05B after those
dependencies, but shared admission/capacity SQL changes integrate sequentially under one owner.

## Batch A — Experiment definition and admission

1. Create a company-owned experiment referencing baseline/candidate published workflow versions,
   source improvement items, problem, prediction, target population, primary/balancing measure
   keys, eligibility, count/time bounds, stop conditions, and an authorized human owner. Both
   versions use the current supported schema; this is not a legacy decoder or data rollout.
2. Validate compatible inputs, role requirements, resources, allowed channels, and effect policies
   for both versions through current publication/readiness rules. The existing binding's normal
   version stays the baseline. Publishing a candidate does not activate it or start a pilot.
3. Explicitly activate the experiment through the existing binding/admission boundary. Atomically
   authorize an incoming case, test eligibility and caps, record its stable allocation, select one
   concrete version/configuration, and admit one ordinary immutable run. Use deterministic
   selected-case/count/time cohorts initially; random assignment is outside the first increment.
4. Scope admission replay to the existing company/source command identity so retries retain the
   allocated version and do not consume another place. Record ineligible/over-cap behavior as
   baseline admission or explicit rejection according to the experiment policy. No fallback may
   silently bypass a required reviewer or select a different candidate after a committed choice.
5. Stopping or exhausting a pilot prevents new candidate allocation; admitted runs keep their
   selected version and frozen configuration. Cancellation is a separate explicit command after
   assessing effects. Respect existing worker/tenant limits and source fairness.

Focused checks: add DB-backed `sop_experiment_` tests racing starts for the last pilot place,
duplicate admission, stop versus admission, revoked owner/resource, source eligibility, and two
simultaneous cases in one thread. Assert one allocation/run per source, exact caps, no version
switch on replay, and no extra external effect. Run existing admission, binding, and capacity tests.

## Batch B — Study and human adoption

1. At pilot end freeze the cohort membership and report measure definitions/versions, observation
   cutoff, sample sizes, distributions, missing data, exceptions, rework, human correction time,
   cost, and debrief themes. Late evidence creates a new report revision; it cannot silently alter
   the report underlying a recorded decision.
2. Report baseline/candidate eligibility differences and the limits of a selected/time cohort.
   A faster agent call alone is not improvement when waiting, corrections, cost, or downstream
   defects rise. Claims link to source evidence and remain subject to its visibility rules.
3. Let an authorized human record adopt, adapt, abandon, or gather more evidence against the exact
   report/experiment revision. Adoption uses existing publication/binding configuration commands
   with explicit version guards. Adaptation creates a new draft/experiment. Neither operation
   migrates running cases, automatically grants capabilities, or republishes an agent's suggestion.
4. Generate optional summaries with bounded model work in ordinary workflows. Large report builds
   use restartable, supervised, idempotent work and expose progress/lag. Reports and model outputs
   cannot change pilot allocation or the binding's selected version by themselves.

Focused checks: add `sop_experiment_` tests for frozen cohorts, late evidence, missing/non-comparable
measures, restricted drill-down, conflicting adoption commands, changed binding revision, and
agent attempts to adopt. Exercise an end-to-end baseline/candidate journey with scripted effects.

## Acceptance

- A pilot selects a version once at normal admission and stays within durable caps under contention.
- The owner can review evidence and uncertainty, then explicitly change only future admissions.
- Existing runs and effect histories never move between versions or get replayed by adoption.
