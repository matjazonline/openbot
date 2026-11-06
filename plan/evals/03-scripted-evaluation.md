# 03 — Scripted evaluation

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on 02. Deliver a useful offline suite runner by composing frozen subjects, existing sample
execution and deterministic checkers. Share orchestration/reporting with later live evals without
adding I/O or a provider-selection flag to the current sample interpreter.

## Batch 03A — Execute cases through existing samples

Prepare each case's execution-only projection into the current `SampleRequest`, with no client-
supplied agent definitions. Reuse authorized definition capture and the validated `PreparedSample`
owner, retain the subject closure pinned at admission, then call `sample::execute(prepared)`.
Do not recapture different active agent revisions between cases, repeats or a resumed attempt.
Keep current validation, child execution, simulated approvals, effect interception and budgets.
Normalize `SampleReport` into eval evidence without pretending
sample paths are production run IDs or simulated effects are delivered effects.

Evaluate supported output/path/schema, expected branch/status, operation arguments and call-count
assertions over structured evidence. Nonempty unexpected/unused fixtures, stopped execution or
insufficient evidence are explicit failures/incomplete results according to the frozen policy.
Do not compare entire natural-language outputs byte-for-byte when the workflow allows alternatives.
Keep assertions and reference answers out of the sample request and all resulting subject context.
Preserve the existing `authorize_release` recheck before returning evidence and apply the same
current authority requirement to stored report reads/exports after a long-running eval completes.

Exercise published child workflows and agent tools through their existing sample boundaries.
Scripted success establishes workflow behavior under supplied responses; it is not a measurement
of a live model's judgment or proof of actual external authorization.

**Focused checks:** `eval_scripted_` with existing sample fixtures, including JEV automatic/human
paths, child tools, intercepted writes, unknown tools, stopped samples and unused fixtures.
Run affected `workflow_sample_`/sample-tool tests and both workflow-sample boundary scripts.

## Batch 03B — Durable bounded orchestration

Reuse the existing bounded job/lease infrastructure and lifecycle patterns; do not create a new
scheduler. Add only the eval-owned job payload/state and purpose-specific persistence operations
needed to claim, execute and settle a case/repeat/subject attempt. Keep new ownership fields and
claims in the responsible eval/job layer rather than writing eval business rules in a worker loop.

Persist immutable admission manifests and logical attempt identities. Claim with fencing/revision
checks, bounded retries/backoff and current company authority; settle evidence and verdict state
atomically. A lost lease cancels and awaits the real work. Late results cannot overwrite a newer
attempt or complete a cancelled/expired run. Resume only missing eligible attempts and expose
repeated attempts; do not hide failing cases behind selective retries.

Apply global and company capacity limits, queue admission bounds, run/case deadlines, bounded
shutdown, and terminal states for exhausted/poison jobs. Report partial progress with counts for
expected, completed, failed, cancelled and unavailable attempts. A full failed claim batch must
not hot-spin or starve other companies. Small suites use this same path rather than a second
untracked background execution path hidden behind an HTTP handler.

**Focused checks:** `eval_jobs_`, `eval_cancellation_`, and `eval_fairness_` with isolated real
PostgreSQL: two competing workers, crash/restart, lease loss, cancellation racing settlement,
duplicate notifications, poison batches, retry exhaustion and another company's continued progress.
Assert case/evidence counts and stale-owner rejection, not only worker return values.

## Exit criteria

A versioned suite runs offline and produces durable per-case evidence through existing samples.
Concurrent workers, restarts and cancellation cannot manufacture a passing or duplicate result;
ordinary workflow samples retain their current offline behavior.
