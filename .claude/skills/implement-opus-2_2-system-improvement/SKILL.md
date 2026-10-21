---
name: implement-opus-2_2-system-improvement
description: "Distill implement-opus-reviewed-2_2 phase records from a supplied directory into scoped AGENTS.md/CLAUDE.md rules and evidence-backed Opus 2.2 workflow improvements in Claude Code. Use for retrospective lesson extraction and instruction consolidation, with bounded cleanup of analyzed phase artifacts; not for implementation or ordinary code review."
---

# Implement Opus 2.2 System Improvement

Claude Code adaptation of `implement-2.2-system-improvement` for records produced
by `implement-opus-reviewed-2_2`. Turn demonstrated implementation, architecture,
review, and coordination failures into concise instructions for future work.
Invocation authorizes justified instruction edits and the bounded cleanup below,
unless the user requests analysis only. A justified no-op is preferable to
redundant rules. Do not spawn agents, change application code, alter role
settings, or modify other workflow variants. This skill is self-contained; do not
load the v3 retrospective workflow.

## Resolve the supplied directory

Accept the records directory passed by the user from `implement-opus-reviewed-2_2`,
normally `<plan-dir>/phases/<phase-id>/docs/`. Also accept its containing phase
directory when it identifies exactly one records directory. Use the resolved paths
in TASK/HANDOFF for adopted legacy layouts; do not assume all phases use the
default layout. Resolve the canonical directory, phase ID, owning worktree, and
plan identity before editing. Records whose entries name another workflow, such as
`implement-astra-reviewed-2.2`, still yield technical lessons; report their
workflow lessons for that variant instead of editing it.

Use absolute paths: Claude Code may reset the shell's cwd between Bash calls, and
a records directory or worktree outside the session's working directory may need
`/add-dir` before edits or cleanup.

If given a plan root containing multiple phases, ask which phase directories to
analyze unless the user explicitly selected all of them. If the directory is absent
or ambiguous, request the path; never choose by modification time. Analyze multiple
phases only when requested, and keep evidence identities and extraction records
separate. Never silently mine sibling phases, root review archives, or another
worktree's evolving records. Read linked external evidence only as needed to
understand the selected scope; it does not become a cleanup target.

Read `<phase-docs>/system-improvement.md` first when present to recognize previously
processed evidence and intentional deletions. Then read TASK.md, REVIEW.md,
HANDOFF.md when present, agent-review-notes.md, agent-review-findings.md, retained
gated decisions, and relevant logs/manifests. Missing findings do not establish a
clean result. Follow linked requirements, actual reviewed code/callers, tests,
correction history, and verification evidence as needed. Follow repository
discovery instructions. Read large records in bounded chunks through the selected
scope; report unavailable sources and unprocessed portions instead of claiming
completeness from a sample.

## Establish reusable lessons

Group observations by causal mechanism and affected contract. Distinguish flawed
gated decisions, author implementation errors, reviewer omissions, verification
gaps, and phase/resource coordination failures. Check the exact decision revision
and artifact reviewed. An earlier pass demonstrates a miss only when a later
confirmed defect links to that review's actual coverage. A clean pass alone does
not demonstrate that the workflow or guidance was effective.

Treat candidate improvements as hypotheses and follow their resolution histories.
Dismissed claims are not positive evidence; open/deferred claims need confirmation;
optional preferences require an established constraint before becoming mandatory.
Count copies, repeated mentions, and rechecks as one incident. Prefer recurring
causes and consequential confirmed invariants, including a single serious
correctness, authorization, or data-loss defect. Reject fixture accidents,
obsolete behavior, unsupported causal explanations, and task-specific choices
without a reusable decision. Do not turn unrelated test failures into repair work.

Maintain a compact mapping of source paths and phase/note/finding/decision IDs,
reviewed artifact, confirmed cause, independent occurrences, affected paths,
candidate instruction, destination, and disposition: add, refine, relocate,
already covered, reject with reason, or unresolved. Save observable evidence and
conclusions, not private reasoning transcripts or copied archives.

## Select ownership and edit

- Technical obligations belong in the narrowest applicable instruction file that
  Claude Code actually loads for the affected paths. It reads `AGENTS.md` only
  where no `CLAUDE.md`, `CLAUDE.local.md` or `.claude/CLAUDE.md` takes precedence
  (unless that file imports `@AGENTS.md` or the project loads both), and loads a
  subdirectory's file only when a file in that subtree is read. Write to the file
  already serving that scope; never add a `CLAUDE.md` that would hide an existing
  `AGENTS.md`. Inspect effective ancestors, overrides, relevant sibling
  instructions, and actual implementation/caller/test paths. Filesystem ancestry
  determines coverage: `feature/AGENTS.md` does not cover sibling `feature.rs` or
  tests elsewhere. Keep caller obligations where callers load them; root rules must
  apply broadly.
- Workflow obligations belong in the installed
  [Opus 2.2 skill](../implement-opus-reviewed-2_2/SKILL.md) or its responsible
  reference. If several copies are installed, edit the one the user names or ask,
  and report copies left unsynchronized. Read the current affected instructions
  before editing. Improve author/reviewer assignments, gate selection, evidence
  capture, verification, acceptance, or concurrent-phase ownership only where the
  evidence supports a reusable change. Preserve separate Opus author/reviewer
  ownership, independent (never forked) reviewer context, artifact-bound
  acceptance, and focused verification. Change its Claude Code mechanics (Agent
  tool settings, compaction setting, worktree and cwd rules) only on evidence they
  are wrong, after checking current Claude Code documentation. Do not create a
  senior role or import v3's guidance model. Surface consequential policy conflicts
  rather than silently changing role settings or weakening acceptance.
- Refine an existing rule before adding another. Repeated violations of a clear,
  visible rule do not justify duplication or stronger wording. Investigate
  visibility, ambiguity, or missing enforcement; report gaps prose cannot solve.
- Create child instructions only for cohesive subtrees with recurring specialized
  decisions. Inherit general rules and include only the local delta. For contracts
  spanning layers, keep the invariant at its shared owner and distinct obligations
  locally. Do not create a file per finding, relocate code, or require blanket
  sibling-file reads to obtain instruction coverage.
- Preserve unrelated edits, exceptions, and verification commands. When moving a
  rule, verify all affected paths retain coverage before removing the old text.

Write compact imperative rules: trigger/scope, mechanism or invariant, and decisive
verification. Generalize the demonstrated mechanism rather than accidental symbols.
Keep source IDs, review stories, blame, dates, counts, and status history in the
extraction record. Operative instructions must work without historical archives.
Apply supported edits within scope rather than stopping at proposals; leave
instructions unchanged when no candidates survive.

## Validate and retain extraction evidence

Check each rule rejects the demonstrated mistake while permitting a valid
implementation. Inspect conflicts, links, caller coverage, and the diff. Repeating
extraction from the same evidence should add no further rules. Compare applicable
instructions before and after for affected implementation paths, callers/tests,
and an unrelated sibling, including inherited text and mandatory reference reads.
Report a small path/count comparison; use word counts when token measurements are
unavailable. Shorter text alone does not prove improved behavior or token savings.

Validate changed skills with the skill-creator skill's validator when available;
otherwise check frontmatter and links directly and report the limitation. Run only
checks affected by instruction changes; documentation-only edits need no
application build or test suite. Leave confirmed unrelated failures unchanged and
report them.

After edits and validation succeed, save or update
`<phase-docs>/system-improvement.md`, preserving previous entries. Record analyzed
scope and source identities, candidate dispositions, adopted destinations,
supporting observations and confirmed causes, coverage comparisons, limitations,
validation, and cleanup status. Retain enough substantive evidence to explain
adopted lessons after source removal; IDs and links to deleted files are insufficient.
Do not modify phase implementation, acceptance, or progress status during extraction.

## Bounded cleanup

After successful analysis and validation, automatically clean up only fully
analyzed, disposable review artifacts unnecessary for further phase progression.
Candidates are review notes, resolved findings, superseded decision copies, and
their dedicated supporting logs. A validated no-op extraction can qualify. This
is an exception to Opus 2.2's normal retention, never permission for cleanup
during implementation. Analysis-only requests authorize neither edits nor cleanup.

- Never delete original requirements, implementation plans, TASK.md, REVIEW.md,
  HANDOFF.md, checkpoints, dependency tracking, active decisions, extraction
  records, or mixed files containing still-needed material.
- Check remaining batches, next actions, open findings, acceptance requirements,
  and inbound references, including dependent and concurrent phases. Preserve
  anything needed for execution, resumption, verification, acceptance, or later
  work. If dependency or completion status is uncertain, retain it and explain why.
- Enumerate individual eligible files, exact analyzed identities, and reasons in
  the extraction record before deletion. Both the named path and resolved target
  must be inside a selected phase records directory. Use path-component
  containment, inspect symlinked parents, and recheck boundaries before deletion.
  Delete each file by its absolute path; never use globs, `rm -r`, external
  symlink targets, or sibling phase files. Do not broaden cleanup to the
  containing plan or phase directory.
- Preserve targets of retained links. Only retrospective references in the
  extraction record may be redirected to sufficient retained evidence; do not
  rewrite progression files or outside references to make deletion eligible.
- Delete only the exact analyzed version. Retain changed files and files being
  written by active work. Do not clean up incomplete analysis or proceed after
  failed validation; summaries cannot replace required acceptance evidence.
- Record actual deletion results and check retained links. After interruption,
  reconcile pending entries with disk and verify edits and candidate identities
  still agree before resuming. Do not infer deletion success from intent.

Report instruction paths and rule-to-source mappings, significant rejected or
already-covered candidates, independent recurrences, coverage comparisons,
validation limits, and files deleted or retained with reasons. Distinguish
validated instruction edits from unproven reductions in future mistakes;
instruction improvements do not repair application defects.
