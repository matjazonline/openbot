# Review p7a-arch-r4: phase 05 limited recheck (round 4)

Reviewer B, generation 4 (agent `a5916c4a63f198c6f`, nonce `run=8e7c1e85 role=B gen=4`).
Date 2026-11-08. Read-only: no project file was edited and nothing was committed.

**Verdict: PASS.** The confirmed U2 refinement and my round-3 notes are applied consistently.
Two text inconsistencies remain (R4-m1, R4-m2). Neither changes the architecture, so they are
carried into P7c and P7f rather than blocking this gate.

## Artifact identity

- Full diff `tmp/memory-service-run/frozen/p7a-arch-r4.diff`: sha256
  `4a443ccdbe24aea349596b66b0c973fe7ac7e5527e0172fd0392dbe63c989fb0` (recomputed, matches).
- Incremental from round 3, `tmp/memory-service-run/frozen/p7a-r4-incremental-from-r3.diff`
  (+53/−31): sha256 `7563eeecdafd2d16f553861dc0cb84d19779cc76caa9a647d4bd32ce9567993a`
  (recomputed, matches).
- Tree, recomputed with a temporary index (base `a6bed48a`, `tmp/` excluded):
  `6238f2bc088e25a5299e0f36dac2c25f352c703e`, matching the worktree.
- `git diff --no-index --check` on the record is clean.

## Scope

- I read the whole r3→r4 diff.
- I grepped the Phase 05 section for leftover "level 0", "exact", "precise" and "consumed 4"
  wording.
- I checked the `messages` role CHECK in the init schema: `messages_role_check` allows `human`,
  `agent` and `system`.

## Edits checked

| Edit | Status |
| --- | --- |
| U2 refinement in E3 and E12 | Applied consistently. `ConversationalInput` is inexact and falls back to the run's trusted ceiling. `AuthoredConfiguration` is an exact 0. Stamps and release are unchanged. The declassification note now applies only to authored configuration. |
| `decision.human` | The authored-enum choice stays exact. Free-text feedback and `message.ask` replies go through `human_input_exactness`. |
| Human-message exactness derived from role at read time | Applied (but see R4-m1). |
| Tests (E11) | The consumed-4 case moved to schedule runs and their children. Added: a 6/8/9 message run saving at 6, a thread window labelled at the lowest-reader level, the `decision.human` split, and a mixed audience with a level-0 reader saving at 0. |
| R3-m1 | Resolved. A dedicated `disable_epoch` is bumped only by disable. A stale claim becomes `failed(disabled)` and wakes its wait. A poison-batch case is added. E2, E6, E8 and E9 all use the epoch consistently. |
| R3-n1 | Applied to `workflow_runs.content_exact` (`NOT NULL`, no DEFAULT). The history columns contradict this; see R4-m1. |

## Cross-effects requested by root

- **No remaining text assumes message-driven runs are exact.** The r1 "Trigger message: precise"
  table is gone, and E11 now expects message runs to fall back.
- **Manual-run fallback.** The fallback is the producing ceiling, which E3 defines as
  `knowledge_ceiling`. For manual runs that is `actor_level_on` of the actor, set at admission
  (`knowledge.rs:104-107`) and immutable (`access_level.rs` `RunKnowledge`; trigger). It is
  therefore frozen at admission. One wording nit: E3 says "the actor's or run-as clearance for
  manual runs", but run-as is the schedule case. A schedule run that consumes a `decision.human`
  free-text feedback or a `message.ask` reply falls back to its run-as clearance.
- **Child inheritance.** Children copy `content_exact` at admission (E3), so a child of a
  message-driven run is inexact. Schedule children stay exact unless they consume conversational
  input. An inexact child also lowers its parent. This is consistent with the new E11 cases.

## Carried findings (not blocking)

### R4-m1 (Minor, carry into P7c): the `messages` exactness column has contradictory shape and no rule for `system` messages

- *Problem:*
  - E3 says the `content_exact` history columns are `NOT NULL` with no DEFAULT.
  - E3 also says human messages "carry no stored bit".
  - The `messages` table also holds `role = 'system'` rows (`messages_role_check`), and the record
    gives no exactness rule for them.
- *Required:*
  - Make `messages.content_exact` nullable.
  - Add a CHECK that it is NULL for `human` and NOT NULL for `agent` and `system`.
  - Writers of system messages state it, defaulting to `false` unless the message comes from an
    exact run.
  - `agent_history_exchanges.content_exact` stays `NOT NULL` with no DEFAULT.

### R4-m2 (Minor, carry into P7f): the thread-capture settings text is now inaccurate

- *Location:* E8, record line ~1508.
- *Problem:* The text still says thread capture shares "level-0 human thread content … because
  levels cannot restrict level-0 content". After the refinement, a thread window is labelled at the
  thread's lowest-reader level (E8, two paragraphs later). The text is accurate only when a level-0
  reader, such as an external, is present.
- *Required:* Rewrite the settings text to say:
  - Captured thread memory is labelled at the thread's lowest-reader level.
  - Any reader cleared to that level, in any channel or thread the agent serves, can recall it.
  - A thread with a public or external reader is captured at level 0 and is visible everywhere.

## Nits

- Fix the "run-as for manual runs" wording noted above.
- No DEFAULT on `workflow_runs.content_exact` will break raw-SQL test fixtures that insert runs.
  That loud failure is intended; budget the fixture churn in P7c.
