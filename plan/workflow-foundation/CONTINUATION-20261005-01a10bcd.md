# 04.7 continuation — root 01a10bcd

Scope: finish original 04.7 only; no 04.8. This record supplements the latest
[RESUME](RESUME.md) and [handoff](HANDOFF-20261005-TOKEN-EFFICIENT.md). Root baseline
is `8e76282dbf51179047c39b3e93a3f3cde464b01c`; preserve inherited WIP and
`LICENSE.md`. No staging, commit, deployment, schema, dependency, cache, or resource
bound change. Original damaged PostgreSQL cluster on port 55439 remains untouched.

## Acceptance and exact evidence

- Original row 3c remains accepted from the prior root. The row 3d validation and
  native-bound nine-test subgroup has independent code PASS at
  `/Volumes/ssd1/dev/workflow-recovery-20261005/reviewer-row3d-01a10bcf/REVIEW.md`.
  The subsequent new-test provider-identity correction has independent preparation
  and final code PASS from reviewer UUID `01a10be5-2255-7422-9b63-81bcee88341f`.
  Final pre-cancel source manifest is
  `/Volumes/ssd1/dev/workflow-recovery-20261005/row3d-final834-gates-01a10bcf/exact-pending-source834.json`.
  Focused validation 5, application 14, format/offline/Clippy/SQLx/graft pass on
  that version. Its unsharded admission gates failed 435/1, 431/5, then 433/3;
  none is a combined acceptance pass. Logs are in that evidence directory.
- A separate legitimate cancelled-wait protocol gap was reproduced before a fix:
  deterministic real cancel-first claimants returned `Invalid stored workflow
  record`, while resume-first passed. The one-arm `cancelled => Refused` correction
  and two competing-order regressions have independent preparation and actual-code
  PASS at `/Volumes/ssd1/dev/workflow-recovery-20261005/reviewer-cancel-01a10c02/REVIEW.md`.
  Focused 65 distinct affected tests and format/offline/Clippy/SQLx/graft passed.
  Author evidence is `/Volumes/ssd1/dev/workflow-recovery-20261005/author-diagnosis-01a10bf8/CORRECTION.md`.
  Current frozen 835-source manifest is the adjacent `corrected-source835.json`,
  SHA256 `515e07af74b3fb9d54bbd8765f1f44d6e7a5c9c55c263e06c23f3106c9299b9f`;
  all 125 protected hashes match. This scoped correction does not close full row 3d.
- A fresh 835-source unsharded admission run failed 436/2 with two lease timeout
  errors; application 14 passed. Exact logs:
  `/Volumes/ssd1/dev/workflow-recovery-20261005/author-diagnosis-01a10bf8/BROAD835.md`.
  A reviewed, predeclared eight-shard runner then passed shard 0 (48) and failed
  shard 1 (51/1), stopping with 99 unique passes and 352 selected tests unrun.
  It is **not** a full sharded PASS. Evidence and exact runner review are in
  `/Volumes/ssd1/dev/workflow-recovery-20261005/author-timeout-01a10c0f/SHARDED-RESULT.md`
  and `/Volumes/ssd1/dev/workflow-recovery-20261005/reviewer-cancel-01a10c02/RUNNER-REVIEW.md`.
  Historical failures and causes remain separate; no run was relabelled successful.

## Current bounded work

Shard 1 failed an unchanged 128-sibling test with an unprinted non-`Committed`
dispatch result. An unchanged isolated control passed; the cause is unknown.
Author diagnosis and permitted diagnostic-only preservation exception:
`/Volumes/ssd1/dev/workflow-recovery-20261005/author-timeout-01a10c0f/SIBLING-DIAGNOSIS.md`.
Reviewer UUID `01a10c25-d270-7e33-ac81-83b052004657` gave preparation and actual-code
PASS for failure-only variant/phase/renewal-window/provider-call output in the
accepted loop, with assertions, await order, one renewal, 127 iterations and limits
intact. Evidence: `/Volumes/ssd1/dev/workflow-recovery-20261005/reviewer-sibling-01a10c25/PREP.md`
and `/Volumes/ssd1/dev/workflow-recovery-20261005/author-sibling-01a10c27/RESULT.md`.
Both exact callers passed at stock 2 MiB; format, whitespace and offline all-targets
passed. The source835 diagnostic manifest SHA256 is
`f4e7ed427f82924f1574ea2afcdb79a40ed33ea53a20df3e440d56dbec938372`.
One separately labeled diagnostic shard1 run failed 51/1 on a different existing
lock-probe test; the instrumented sibling case passed and its historical cause
remains unknown. That diagnostic run is not acceptance evidence; see
`/Volumes/ssd1/dev/workflow-recovery-20261005/author-sibling-01a10c27/DIAGNOSTIC-RESULT.md`.
Root stopped broad/shard reruns at this point and returned to original row 3d work.
Fresh author `/root/author_row3d` is preparing the remaining row 3d architecture
against the original requirements. Expansion at
`/Volumes/ssd1/dev/workflow-recovery-20261005/author-row3d-01a10c33/EXPANSION.md`
received independent scoped preparation PASS at
`/Volumes/ssd1/dev/workflow-recovery-20261005/reviewer-sibling-01a10c25/ROW3D-PREP.md`
for native diagnostic, chronology and registered-validity matrices. The reviewer
requires native fields in a bounded fixture MESSAGE envelope for immediate CHECK
negatives; bare rethrow is insufficient. The genuine registered-Unknown prospective
fixture and one unmodified positive service COMMIT are now scoped ACCEPTED: focused1
PASS at stock2MiB/defaultparallel, format/whitespace/offline-alltargets PASS, exact
source/NEWPG preservation, and independent corrected code PASS by reviewer UUID
`01a10c47-4939-78d2-aa26-5985247a389e`. The sole review finding tightened the
generated revision assertion to exactly `before+1`; corrected runtime passed.
Evidence: `/Volumes/ssd1/dev/workflow-recovery-20261005/author-row3d-01a10c33/POSITIVE-RESULT.md`.
The subsequent first scalar native CHECK negative is also scoped ACCEPTED after
independent actual-code PASS by the same reviewer and focused2 PASS, formatting,
whitespace, offline all-targets, strict Clippy, SQLx prepare/check and graft PASS.
Its authentic prospective nested candidate hits exact native23514 diagnostic CHECK;
the distinct unexpected-acceptance sentinel, rollback/public/catalog checks and
unchanged positive full-pair COMMIT pass. Evidence:
`/Volumes/ssd1/dev/workflow-recovery-20261005/author-native-negative-01a10c4d/RESULT.md`.
Current source838 manifest SHA256 is
`9ebfe2648fb5493894a023340de0cea4b54c5f3691a71502037113eec2012678`;
source836 positive SHA256 was
`eef98d65bed1611be8f4f7288db165e93ba22b7e3161e28e9d8fab69ae33b06f`.
Prior accepted bodies remain exact except child registrations, and all125protected
hashes match. These scoped acceptances do not close array/SQL-text-byte,
chronology or validity matrices, nor full row3d. No broader gate rerun was made.

Remaining original work: finish row 3d native evidence diagnostic/time/validity,
malformed stored evidence at its real relational owner and safe-storage projection;
row 3e natural COMMIT/full logical and injected rollback; row 3f populated upgrades;
final original 04.7 full integration checks and independent review. The no-secrets
contract remains an explicit architecture question for full-row acceptance: safe
owned verifier projection does not prove arbitrary trusted-diagnostic or operator-note
sanitization. Independent native work proceeds without claiming that criterion.

## Resources and workflow

Authorized isolated NEWPG: PostgreSQL 18.6, system identifier
`7693072113847127473`, `mac03/workflow_admission`, port `55440`, max connections
`200`, data `/Volumes/ssd1/dev/workflow-recovery-20261005/test-pg`. Both database
URLs are `postgres://mac03@127.0.0.1:55440/workflow_admission`; use locked/offline,
`SQLX_OFFLINE=true`, stock `RUST_MIN_STACK=2097152`, default test parallelism.
Every completed assignment rechecked identity, zero other clients and no disposable
databases; all task-owned processes are now drained.
Never reset/drop/reinitialize/repair/restart/stop original port 55439. Raw copies
are not verified recovery backups.

Workflow: root model/session settings unchanged, Astra medium A/B workers with no
subagents; root accepts only after independent PASS plus required checks. Root UUID
`01a10bcd-f1aa-7591-8129-91b65e0d689f` measured 142,453/258,400 = 55.13% at
2026-10-05T13:55:29Z. Worker context threshold 50%; root threshold 60%.
Runtime total token spend is not established by these context samples.

## Next bounded action and closure

Use a fresh root and fresh Astra author/reviewer; all workers in this root are
completed. Verify current source838 and125protected manifests, HEAD/index and exact
NEWPG identity before edits/tests. Build on the accepted scalar fixture to add native
diagnostic array and calibrated SQL-jsonb-text16384/16385 cases, with the same
authentic NEW identity, bounded MESSAGE diagnostics, deliberate corrected positives,
complete rollback/public/catalog comparisons and independent actual-code review.
Then implement covered-entry chronology and registered validity cases under the
reviewed expansion. Use focused checks for each point; defer another broad gate until
the original row3d matrix is complete or a concrete cross-test fault is corrected.

Do not reinterpret the repeated broad/shard failures as a passing suite. The native
scalar candidate checks its immediate CHECK before a full command/coverage/audit pair
exists; the unchanged positive separately proves full-pair COMMIT. Full row3d must
also explicitly settle safe-storage/no-secrets scope before acceptance. Row3e, row3f
and final04.7 remain required. No04.8.

Root checkpoints early at58.21% measured context (150,423/258,400,
2026-10-05T14:00:08Z) because the next implementation/review group exceeds the
remaining headroom below this workflow's60% root threshold. No project worker,
build/test/SQLx/graft operation, owned database or pending approval remains. The
small task-created `facts.rs/graft/.cache` telemetry directory was removed after
verification; inherited WIP was not cleaned.
