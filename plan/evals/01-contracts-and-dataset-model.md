# 01 — Contracts and dataset model

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Define the shared model before adding storage or live inference. Trace the existing sample,
publication, access-level, provider-budget and job contracts. Preserve their owners; domain types
remain independent of transport, SQLx, browser and provider SDK types.

## Batch 01A — Cases, revisions, and frozen subjects

Use typed identities for a company-owned suite, immutable suite revision, case, run and attempt.
A suite targets one workflow identity and has an editable draft plus immutable reviewed revisions.
Changing cases, expected answers, fixtures, rubric, split, hard gates or thresholds creates a new
revision. Runs retain the exact content digest and selected case IDs, never a moving draft alias.

Separate execution input from grading-only data. Execution input contains workflow input/params,
explicit conversation/history/retrieval snapshots, tool fixtures and human decisions. Grading data
contains reference facts/labels, required and forbidden behavior, assertions, rubrics and human
labels. The preparation path cannot put grading data into agent context or fixture responses.

Freeze each subject's published bundle/content hash, child workflow versions, agent/skill/tool
revisions, model/provider settings and relevant resource policies once at run admission. Agent
revisions publish independently: never resolve the currently active agent anew for each case or
repeat. Reuse `SampleDefinitionCapture`, `PreparedSample`, configuration hashes and native manifest
ownership; extend that owner for published/eval captures where necessary. A draft may be evaluated
only through its authorized, revision-checked capture path without activating it. Persist the
validated capture/manifest identities needed for resume; do not reconstruct them from moving aliases.
Secrets are opaque company-scoped references resolved at execution; never snapshot credential bytes.
Record connection identity/revision and provenance without claiming third-party models are immutable.

Fixtures are bounded data selected deterministically by operation, validated arguments and logical
call occurrence/path. Unexpected calls, exhausted fixtures and ambiguous matches fail visibly.
Do not return the next fixture regardless of the actual tool arguments. Expected behavioral
constraints must allow legitimate alternative traces rather than asserting every internal step.

**Focused checks:** `eval_contract_`, `eval_fixture_`, and `eval_subject_` unit tests for schema
validation, stable hashing, immutable identity, missing/foreign dependencies, correct fixture
matching, and structural separation of expected answers from subject inputs.

## Batch 01B — Modes, statuses, and enforced limits

Represent scripted and live-model requests as explicit variants with distinct requirements.
Keep disposable integrations as operator/CI environment evidence; do not add a production-write
flag to the eval request. Distinguish execution status, grader status and verdict: completed,
failed, cancelled, expired or unavailable execution; passed/failed/error/ungraded checks; and
pass/fail/incomplete aggregate verdicts. A stopped run or missing judge never becomes a pass.

Record case/repeat/subject identity, observable steps and attempted effects, actual usage or
explicitly synthetic usage, wall duration, diagnostics and per-criterion evidence. Never label
sample token counts as provider billing. Price estimates require a recorded price basis; unknown
usage/pricing stays unknown. Do not collect hidden model reasoning as an eval requirement.

Initial code-owned caps: 100 cases/revision, five repeats, two compared subjects, and 1,000 total
case attempts/run including retries; 256 KiB per case, 8 MiB per suite payload and 1 MiB per case
report. Preserve any stricter sample/model/tool limits. Bound total stored run payload, import
size, queue depth, case/run wall time and judge calls before implementation acceptance; set and
test concrete defaults in the owning layer. A user-supplied lower inference budget is enforced
before dispatch and shared across subject, retries and grading.

Reports are paginated; comparisons do not concatenate an entire suite into one model prompt.
Limit/refusal/truncation must be explicit and must not silently omit required grading evidence.
Any raised bound needs an early-failure CI signal and a recorded reason.

**Focused checks:** `eval_bounds_` and `eval_verdict_` tests at/beyond each boundary, including
overflowing case×repeat×subject counts, report allocation, synthetic/unknown usage and incomplete
grading. Select tests with `-- --list` first and reject a zero match.

## Exit criteria

The contracts specify data ownership, immutable comparison identity, input/answer separation,
execution modes, limits and honest verdicts without introducing another workflow language.
