---
name: implement-plan-phases
description: Implement an ordered directory of numbered plan phases with one Astra/high subagent per phase, focused phase checks, and one final full-suite gate. Use when asked to execute a multi-phase implementation plan; not for planning or a single phase.
---

# Implement Plan Phases

The user supplies a plan directory. Coordinate its implementation through
`$implement-astra-reviewed-2.2`; keep phase authorship and independent review in
that workflow. This skill owns phase ordering and the final verification gate.

1. Resolve the plan directory and read its overview and repository instructions.
   Use the phase files and order declared by the plan; otherwise discover numbered
   phase files such as `NN-*.md` and sort them numerically. Never assume a fixed
   count or set of names. Resolve dependencies and final-suite commands from the
   plan. Preserve pre-existing worktree changes.
2. Run one phase at a time in plan order. For **each** phase, spawn a fresh
   subagent with `model: "gpt-6-astra"`, `reasoning_effort: "high"`, and
   `fork_turns: "none"` (or the host's equivalent). Give it this core prompt,
   substituting the resolved phase path:

   > use $implement-astra-reviewed-2.2 to implement <phase-file> (you are Astra/high)

   Also supply the plan directory, applicable `AGENTS.md` paths, existing
   authorizations, and these boundaries: implement only this phase; use targeted
   tests for its changed behavior and affected interfaces; record commands,
   results, and executed-test counts; leave the full suite to the coordinator
   after all phases. The phase agent handles its own independent reviewer under
   `$implement-astra-reviewed-2.2`. Do not replace that review with coordinator
   self-review. The phase agent need not ask the user about the final suite;
   this invocation already authorizes it.
3. Wait for the phase agent to finish. Check its durable phase records and review
   evidence for an explicit reviewer pass, resolved blocking findings, and
   meaningful targeted checks. Have the same agent finish gaps or corrections.
   Start the next phase only after the current phase is accepted. If a phase is
   blocked, preserve its evidence and report the blocker instead of treating
   later phases as complete.
4. After **all** phases are accepted and integrated, run the complete verification
   gate once, using the plan's final commands plus repository-required checks.
   For this repository that includes formatting, offline compilation, migrations,
   and database-backed tests. Record exact commands, results, test counts, and
   unavailable checks in the plan's verification evidence. A zero-test filter is
   not a passing check. Fix attributable failures through the owning phase agent
   and its reviewer; rerun affected checks and repeat the full gate when the fix
   changes shared or cross-cutting behavior. Do not claim full verification when
   a required check could not run.

Honor the repository and plan's no-backward-compatibility policy: do not add
backfills, dual reads or writes, legacy decoders, compatibility shims, or staged
rollouts for old data. Reset a disposable local or test database when needed and
authorized by the invocation; a production reset requires separate authorization.
Report phase acceptance, independent review, the final gate, and any remaining
verification gaps. Do not deploy or publish unless separately authorized.
