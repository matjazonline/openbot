# Plan05 p04 — reviewer notes

Persistent review log for phase p04 (documentation and final evidence). One entry per round.

## 05SC-P04-B1-R1-N1

**Identity.** 2026-11-18, workflow implement-opus-reviewed-2_4, phase p04, batch B1, stage
batch review (round R1, first p04 review). Records [TASK](TASK.md), [REVIEW](REVIEW.md),
[findings](agent-review-findings.md). Reviewer: independent subagent B; model reported by the
host as Fable 5.1 (`claude-fable-5-1`), effort not exposed (assignment requested Fable
5.2/medium; not claimed). Worktree /Users/mac02/orca/workspaces/openbot/plan03-04, branch
feature/plan05-scopes, phase start = review base = HEAD
3d7148b1b496e491e139fc90e0534ef8d7d296ab, no commits since base; artifact is the staged diff,
digest 499d9b65b220c1c968394cf561c3252ce9338c8876226019b65bb1b700761a42 with exclusions
`plan/05-scopes/phases/p04/docs` and `plan/05-scopes/COORDINATION.md`, reproduced at start and
end ([start](logs/B1-R1-reviewer-start.txt), [end](logs/B1-R1-reviewer-end-snapshot.txt)). No
gated decisions (documentation of accepted contracts). Coordination:
`plan/05-scopes/COORDINATION.md` revision 18, byte-identical to the pinned
[r18 copy](logs/B1-R1-coordination-r18.md); applicable notices 05SC-C001/r1, C002/r1,
C003/r1, C004/r1, C005/r2, C006/r1, C007/r1 (C007 reviewer preference superseded for p04 by
revision 18). Freshness rechecked before returning; no applicable requirement changed.
Original requirements: [README](logs/original-README.md) and
[04](logs/original-04-integration-and-final-verification.md), both verified identical to
`git show 3d7148b1:...`.

**Coverage and evidence.** Reviewed the whole staged batch: `docs/agent-authoring.md`,
`docs/departments.md`, `docs/deploy.md`, `services/memory/README.md`,
`plan/05-scopes/README.md`, `plan/05-scopes/04-integration-and-final-verification.md`, and the
p03 publication-only records (`phases/p03/docs/HANDOFF.md`, three `B1-publication-*.txt`
logs). Confirmed no runtime, schema, script, dependency or CI file is in the diff and the
working tree holds only the staged batch, the unstaged coordination file and the untracked p04
records. Checks run by me (read-only): snapshot digest twice, `git diff --cached --check`,
`check-evidence.py` on [B1-R1.evidence.json](logs/B1-R1.evidence.json) (valid: 5 PASS, 1 FAIL
trail-check with disposition, 0 NOT EXERCISED), SHA-256 of the reused p03 manifest, heading
uniqueness of the four new anchors, `git ls-remote origin refs/heads/main` (3d7148b1, matches
the p03 remote proof), commit date of 3d7148b1. Every documented tool name, selector, scope
label, payload shape, workflow scope literal, refusal code, CLI syntax, 60-second deadline,
JSON Lines summary fields, env var names, service operation ids and HTTP paths, knowledge
routes and inputs, `unit_id`-first upload rule, 256-unit bound, manual read/write authority
rule, department-delete 409 condition (FK RESTRICT), namespace retirement, library URL and
paging were traced to source lines, listed in
[B1-R1-reviewer-checks.txt](logs/B1-R1-reviewer-checks.txt). The 04 completion-record numbers
and identities were checked against p03 TASK/REVIEW/notes and p02 TASK/notes. Author's
acceptance mapping AC1–AC5 checked: AC1/AC2/AC3 are inspection claims (docs vs source) and are
met except for F1; AC4 met except the F2 count; AC5 (runtime identical, links, whitespace,
evidence, Trail) confirmed by my own checks; Trail check FAIL is a lexical match at
`docs/departments.md:223` to an RcDom-ownership learning, unrelated to the text, disposition in
[B1-trail-disposition.md](logs/B1-trail-disposition.md) is reasonable. Nothing behavioral was
executed by me: this is a documentation-only batch and AGENTS.md does not require Rust or
database suites; the runtime proof is the accepted p03 evidence, reused unchanged. Lightly
reviewed: the advisory (non-failing) Trail matches beyond the first, and the pre-existing deploy.md
memory prose surrounding the new paragraphs (read for consistency, not re-verified line by
line). Not verified: whether the author's `trail build` output is complete (174 KB log present,
not read in full).

**Outcome.** Changes required. Findings: [05SC-P04-B1-R1-F1](agent-review-findings.md#05sc-p04-b1-r1-f1)
(medium, actionable: agent-authoring claims lead report reads for deskless runs without a
resolved placement; code and original 02 give root read only),
[05SC-P04-B1-R1-F2](agent-review-findings.md#05sc-p04-b1-r1-f2) (low, actionable: 04 record
"180 service tests" conflates 178 PASS + 2 ignored),
[05SC-P04-B1-R1-S1](agent-review-findings.md#05sc-p04-b1-r1-s1) (optional: route table omits
existing `DELETE /documents/{id}`). The 04 completion record otherwise honestly reports the
two retained full-run failures, the native fresh-download 404, the Trail lexical conflict, Doop
deferral, and makes no whole-CI or all-green claim. No PASS for digest 499d9b65…a42; a recheck
needs only the F1/F2 deltas plus any text the author touches.

**Process and instruction feedback.** Batch size was right for a docs/evidence phase: one
cohesive review was feasible. No architecture gate was needed. The author's records were
useful (exact source spans for CLI and search draft, pinned originals, pinned coordination).
Missing from them: a per-paragraph trace for behavioral statements (see F1 lesson). Candidate
instruction (documentation batches): acceptance mapping rows for docs should name the branch or
test proving each refusal/edge statement, not only the happy path. Single instance; recorded
for later analysis, not adopted.

## 05SC-P04-B1-R2-N1

**Identity.** 2026-11-18, implement-opus-reviewed-2_4, phase p04, batch B1, stage correction
recheck (round R2) of [05SC-P04-B1-R1-N1](#05sc-p04-b1-r1-n1). Reviewer: same independent
subagent, Fable 5.1 (`claude-fable-5-1`), now user-authorized for this phase per TASK "B1-R1
result and corrections"; effort not exposed. Worktree /Users/mac02/orca/workspaces/openbot/plan03-04,
branch feature/plan05-scopes, base/HEAD 3d7148b1b496e491e139fc90e0534ef8d7d296ab, no commits
since base; staged digest db7a281d2171092056186a794140a1c710c1b67174817a19e183457a5257210f with
the same exclusions, reproduced at start and end ([end log](logs/B1-R2-reviewer-end-snapshot.txt);
author's [corrections snapshot](logs/B1-R1-corrections-snapshot.txt) agrees). Coordination
revision 18 byte-identical to the pinned [r18 copy](logs/B1-R1-coordination-r18.md); applicable
notices unchanged (C001–C004/r1, C005/r2, C006/r1, C007/r1 superseded for p04 reviewer choice).

**Coverage and evidence.** Rechecked by finding ID: F1 paragraph against the run-scope source
and unit test; F2 row against p02 records; S1 row against the knowledge router and
`api_delete` extractor. Checked that the delta is confined to the three named files: the staged
diffstat differs from R1 only by one added line in `docs/departments.md` (74→75); the other nine
files keep their R1 line counts, and the same ten paths are staged. `git diff --cached --check`
exit 0; heading uniqueness unchanged; working tree still holds only the staged batch, the
unstaged coordination file and untracked p04 records; no runtime path staged. Unchanged covered
work from R1 is reused, not re-reviewed (same base, same source tree, docs-only batch). No
behavioral tests run (documentation-only; AGENTS.md exemption applies). The round evidence
manifest is still `B1-R1.evidence.json` pinned to the R1 digest; the author did not issue an R2
manifest. Its checks (whitespace, integration-docs boundary, link audit, runtime identity,
trail) concern content that the correction did not alter except whitespace, which I re-ran, and
the link audit, whose 16 links are untouched by the delta; the R1 manifest's artifact field no
longer matches the staged digest, which root must note at acceptance (a fresh manifest for
db7a281d…210f would remove the mismatch; not a blocking defect in the deliverables).

**Outcome.** **PASS** for staged digest db7a281d2171092056186a794140a1c710c1b67174817a19e183457a5257210f
on base/HEAD 3d7148b1. F1 and F2 fixed, S1 implemented; no new findings. The completion record
still reports the two retained full-run failures, native download 404, Trail lexical FAIL and
Doop deferral without an all-green claim.

**Process and instruction feedback.** Correction round was small and single-pass; no gated
decision involved. Candidate from R1 stands unchanged. Observation: a correction round that
changes the digest leaves the retained manifest pointing at the old digest unless A reissues
one; the verification-contract reference already says a changed check gets a new round/manifest,
so no new instruction is supported.
