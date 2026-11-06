# Workflow evaluations

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and status

Status: proposed implementation, 2026-11-06. This plan does not implement evals or claim that any
workflow, provider, or integration has passed the checks described here.

Use one shared evaluation runner with a versioned suite for each workflow. An authorized company
member can define cases and success criteria, evaluate an exact frozen workflow/agent version,
and compare a candidate with a baseline. Reports show individual failures, hard-gate violations,
quality by criterion, usage, and latency. Different workflows share execution/reporting machinery
while retaining their own definition of success.

Build on existing workflow samples, immutable publication, agent execution, company authorization,
model connections, and bounded job execution. Keep the current sample runner offline. Introduce
paid model evaluation through a separate application operation with explicit authority and budgets.
Do not build another workflow engine, provider SDK stack, generic grader plugin system, or hosted
eval-service dependency.

## Implementation order

| Group | Deliverable | Depends on |
| --- | --- | --- |
| [01 — Contracts and dataset model](01-contracts-and-dataset-model.md) | Cases, versions, execution modes, limits, and hard/quality verdicts | Current sample/publication contracts |
| [02 — Storage and suite management](02-storage-and-suite-management.md) | Company-scoped immutable suite revisions, import/export, authorization | 01 |
| [03 — Scripted evaluation](03-scripted-evaluation.md) | Shared runner over existing offline samples, deterministic assertions, durable results | 02 |
| [04 — Live models and integrations](04-live-models-and-integrations.md) | Budgeted live inference with intercepted tools; disposable integration acceptance | 03 |
| [05 — Graders and hard gates](05-graders-and-hard-gates.md) | Workflow-specific deterministic/model rubrics and human calibration | 03; 04 for paid grading |
| [06 — Comparisons and authoring](06-comparisons-and-authoring.md) | Matched baseline/candidate reports, workflow UI, release evidence | 04–05 |
| [07 — Verification and operations](07-verification-and-operations.md) | End-to-end proof, final full CI gate, retention and operating guidance | All preceding groups |

Each numbered file is one logical implementation group with focused checks and exit criteria.
Implement the scripted path first; it is useful independently of model credentials. No completed
group authorizes real customer data, paid inference, or external writes during implementation.

## Execution modes and boundaries

| Mode | Executes | Establishes |
| --- | --- | --- |
| Scripted | Existing sample interpreter with scripted model/tool/human responses | Branching, data flow, assertions, and simulated effect behavior |
| Live model | Real model calls; fixture-controlled external reads/writes, isolated state, scripted human decisions | Actual model choices and output quality under the supplied environment |
| Disposable integration | The normal application against explicitly provisioned test companies/accounts/services | Real transport, authorization, concurrency, and external state behavior |

The first two modes never dispatch business effects to production services. Disposable integration
is an operator/CI test environment, not a switch that lets ordinary eval requests use production
connections. Its evidence names the tested revisions, fixtures, identities, and external state.
Simulation does not prove production authorization or actual remote effect counts.

## Decisions carried from the discussion

- Start with about 20–30 reviewed cases per workflow, then grow from observed failures. Include
  ordinary inputs, ambiguity, missing context, failures, human review, and adversarial content.
  Small suites establish regressions, not statistical proof of universal correctness.
- A case contains inputs/context, fixture setup, expected outcome, allowed/forbidden actions,
  and a grading rubric. Expected answers and grading-only data are never model/tool input.
- Keep development/regression and held-out cases explicitly separated. Freeze dataset, grader,
  workflow/agent/tool revisions and model settings for every run. Compare the same case/repeat
  pairs; repeat stochastic cases and expose missing/failed attempts in the denominator.
- Permission violations, unauthorized disclosure, missing required approval, and forbidden or
  duplicate effects are hard failures. A high quality average cannot override them. Zero observed
  violations is scoped to the executed suite; graders are not runtime authorization controls.
- Prefer deterministic checks for structure, facts, state, and effects. Use explicit model rubrics
  for subjective quality, calibrated against human labels. Model judges have no tool access.
- Datasets, traces, grading prompts, exports, and reports remain company-scoped and at least as
  restricted as all source content. Using a judge is another provider disclosure and paid call.
- Eval execution cannot publish a workflow, activate an agent, move a binding, send a reply, or
  mutate production memory. A separate authorized user action remains responsible for deployment.

## Existing owners and reference guidance

- [Sample contracts](../../src/application/workflow/sample/types.rs):13–48, 63–112 define scripted
  inputs, steps, intercepted effects, synthetic usage, and reports with configuration hashes and
  selected agent/workflow revisions.
- [SampleDefinitionCapture](../../src/application/workflow/sample_capture.rs) and its
  [PostgreSQL adapter](../../src/adapters/persistence/workflow/sample_capture.rs):41–81 already
  capture the authorized definition closure and recheck release authority. Client-supplied agent
  definitions are rejected; internal capture selects them. This is definition capture, distinct
  from the proposed reviewed capture of a failed run as a dataset case.
- [PreparedSample](../../src/application/workflow/sample/definitions.rs):43–78 and
  [sample execution](../../src/application/workflow/sample/mod.rs):84–158 own validated frozen
  execution/report assembly; [step dispatch](../../src/application/workflow/sample/dispatch.rs)
  and [tool dispatch](../../src/application/workflow/sample/tools.rs) own its offline behavior.
- [Sample authoring route](../../src/adapters/http/routes/workflows/ui/authoring/sample.rs)
  and [sample report UI](../../src/adapters/http/pages/workflow_authoring/sample.rs) are
  existing workflow entry points. Extend their surrounding authoring area instead of a new app.
- [Publication contracts](../../src/application/workflow/publication/mod.rs) and native agent
  manifests own pinned dependencies. Agent revisions publish independently; a workflow version
  alone does not identify which active agent revision an eval selects.
- [Sample boundary check](../../scripts/workflow-sample-boundary.py) must continue to protect
  ordinary sample execution when live-model evals are introduced.
- [Official OpenAI evaluation guidance](https://developers.openai.com/api/docs/guides/evaluation-best-practices)
  supports task-specific cases, explicit metrics, human calibration, and repeatable comparisons.
  This plan uses those methods without depending on a hosted eval platform or a particular vendor.

Read applicable AGENTS.md files and graft context before implementation; trace every shared symbol
before changing it. Record focused commands, selected/executed counts, results and unavailable
checks per batch. Run the final complete verification gate once after integration. Every Markdown
file here must appear in `NO_BACKWARD_COMPATIBILITY_NOTES` in `src/main.rs`.
