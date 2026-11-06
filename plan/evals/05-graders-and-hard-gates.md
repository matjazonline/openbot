# 05 — Graders and hard gates

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on 03; paid judges use 04. Score task success with appropriate evidence and separate
non-negotiable hard failures from subjective quality. Keep grader definitions versioned and
reusable through a small code-owned set of check types, not executable code uploaded by users.

## Batch 05A — Deterministic checks and workflow rubrics

Implement bounded schema/JSON-path value assertions, expected decisions, allowed/forbidden tool
names/arguments, required approvals, call/effect counts and terminal status checks. Paths select
structured data; they do not evaluate expressions or execute Python/shell/JavaScript. Validate
assertions against actual report contracts and return an explicit missing-evidence result when
required steps/state cannot be observed. Avoid assumptions about an exact valid reasoning path.

| Workflow family | Primary checks | Quality judgment, where useful |
| --- | --- | --- |
| Classification/routing | Expected label/branch, fallback, per-class confusion counts | Ambiguity and correct escalation |
| Extraction | Output schema, field values, absent/null handling | Faithfulness to source text |
| Support/research | Required supported facts/citations, forbidden claims, abstention | Correctness, completeness, clarity |
| External action | Authorized intent/destination, validated arguments, approval, counted effects | Whether the action meets the request |
| Document creation | Required content, factual assertions, intended workspace/link/export | Readability, layout and visual completeness |
| Human review | Escalation, reviewer context, decision/continuation and refusal behavior | Adequacy of the review handoff |

Routing/extraction scores use labeled cases; citation presence alone does not prove source support.
For integration tests, real state determines actual effects. For simulated runs label those counts
as attempted/intercepted, not delivered. Document visuals require the relevant rendered artifact
or human review; a text-only judge cannot certify clipping, private font loading or PDF usability.

Freeze which checks are hard gates. Any observed permission violation, unauthorized disclosure,
missing required approval or forbidden/duplicate effect fails the evaluated run regardless of
quality averages. Missing/error/ungraded mandatory checks make the verdict incomplete or failed,
never passed. A judge saying an action is safe cannot authorize that action at runtime.

**Focused checks:** `eval_assertion_`, `eval_hard_gate_`, and `eval_metrics_` tests for every
checker, missing evidence, invalid schemas/paths, rare-class failures, synthetic/real effect
distinction and quality averages that attempt to hide hard violations.

## Batch 05B — Model judges and human calibration

Use the existing provider port for an explicitly authorized company judge model, with no tools,
strict structured output, bounded evidence and frozen rubric/references. Separate judge input from
subject preparation. Treat answers, tool traces and reference documents as untrusted data rather
than instructions. Never give the judge credentials or use its free-text explanation as executable
policy. The subject cannot select its judge, alter thresholds or write its own human labels.

Prefer specific pass/fail criteria and blinded pairwise comparison with randomized or balanced
answer order. Record criterion scores, short evidence references, judge revision/model/settings
and ordering; do not require or retain hidden reasoning. Validate outputs and finite score ranges.
Invalid output, provider failure, missing visual evidence or insufficient context is ungraded/error.

Calibrate with reviewed human examples, including deliberately bad answers and tricky borderline
cases. Measure disagreement and retain human adjudication. A stronger judge or agreement on a
small set is not proof of accuracy. Keep held-out labels separate from prompt tuning; changes to
judge, rubric or threshold create new grading identity and invalidate incompatible comparisons.

Each judge call must pass company model access, content/provider egress policy and the shared run
budget before dispatch. A company approving provider A for execution does not implicitly approve
provider B to receive traces for grading. Redact unnecessary sensitive fields, preserve access
restrictions, and count grading usage/latency separately from subject usage/latency.

**Focused checks:** `eval_judge_`, `eval_grader_security_`, and `eval_calibration_` with local
judge fixtures for valid/invalid output, injected instructions, position bias fixtures, missing
context, budget exhaustion, forbidden provider egress and repeatable aggregation. Separately run
authorized real-judge calibration and record agreement/disagreements and unavailable checks.

## Exit criteria

Every score is attributable to a specific criterion and evidence. Hard violations cannot be
averaged away; model-based scores have explicit limits, reviewed calibration and separate authority.
