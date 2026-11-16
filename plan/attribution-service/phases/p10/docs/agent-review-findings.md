# P10 independent findings

## AS-P10-B1-F1 — disposable absolute cache symlink enters frozen artifact

- Date: 2026-11-16. Workflow implement-astra-reviewed-2-3; p10 whole-batch and
  combined-integration review AS-P10-B1-R1; note AS-P10-B1-N1.
  Reviewer /root/attribution_reviewer, retained independent B requested Astra/high;
  actual model/effort telemetry unavailable. Worktree is the assigned isolated
  attribution worktree in [TASK](TASK.md).
- Artifact: `0c42f5f05564bd55bbef877a43380925536f135111e1e33dc8f195b815607fa6`,
  HEAD/start/base `6e55258cbbdc7633940805f366fb17fe240f02aa`.
- Classification: actionable artifact/reproducibility defect, P2. Status OPEN.
- Evidence: `services/attribution/target` is an absolute symbolic link to
  `/tmp/as-attribution-target`, reported `??` by `git status --short` and included
  in both [start](logs/AS-P10-B1-reviewer-start.snapshot.txt) and
  [end](logs/AS-P10-B1-reviewer-end.snapshot.txt) frozen inventories. The service
  `.gitignore:1` pattern `/target/` ignores directories but not this symbolic link.
  It is the21st actual changed/new snapshot path beyond the20 intended source
  paths in the transfer preimage manifest; see
  [independent inventory](logs/AS-P10-B1-reviewer-inventory.txt).
- Impact: an invocation-specific absolute cache link is accidentally treated as
  part of the delivery source and can be staged with the implementation. It does
  not work in a clean checkout and must not travel with the independent service.
  Extraction ignores target and thus its passing test does not detect this artifact.
- Required correction: after the running root check releases it, remove only the
  owned temporary link and restore the preserved ignored target directory (or
  otherwise ensure this disposable cache is absent from source inventory). Preserve
  the user's runner files. Re-freeze and review the corrected final artifact; no
  production logic change or full-suite repeat is required for cache cleanup alone.
- Instruction lesson: inspect the actual snapshot inventory, including symlink
  type, instead of treating a generated file manifest as exhaustive. Existing
  reproducible-build/standalone rules already cover the needed behavior.
- Resolution history: root acknowledged this finding and will keep the active
  runner's link until safe cleanup, then restore its preserved target directory.
  No correction reviewed in R1.


### AS-P10-B1-F1 resolution — R2 — 2026-11-16

**CLOSED** on artifact
`55ecc5eabdcb9493aa9c5ecead843cb075554aa686be101994280bb1b38efb97`.
Service `.gitignore` now uses `/target`, which covers either a generated directory
or symlink. Independent `git check-ignore` and source snapshot inventory confirm
that the temporary absolute cache link is absent from source while the active
runner can keep using it. Only this one-line ignore change was needed; no user's
runner file or active cache changed. R2 [inventory](logs/AS-P10-B1-R2-reviewer-inventory.txt)
and [start snapshot](logs/AS-P10-B1-R2-reviewer-start.snapshot.txt) retained.
This finding closure does not confer final acceptance; R2 final evidence remains pending.

## R2 final disposition — 2026-11-16

F1 remains **CLOSED**; no open attribution-source finding on55ecc5ea.
AS-P10-B1-N2 gives PASS only for the verified affected attribution scope.

**AS-P10-VG1 — unresolved repository verification gap:** department-directory
lifecycleConflict in stock and current focused control is not causally diagnosed.
Baseline and final observed current case pass;21read-only samples show no expired
lease. It must not be called proven expiry or confirmed unrelated. Preserve this
gap and other failed/unexercised repository checks in root's final report. It is
not evidence of an identified attribution-source defect and is outside the
contract surface certified by the scoped PASS; whole-repository success is not
accepted. Exact evidence/limitations are in N2's final assessment.

## AS-P10-PUSH-R1 final disposition

No new integration finding. AS-P10-PUSH-R1-N1 PASS on3688ab5d for publication
integration with accepted attribution implementation preserved. F1 remains closed;
AS-P10-VG1 and earlier failed/unexercised whole-repository checks remain unchanged.
