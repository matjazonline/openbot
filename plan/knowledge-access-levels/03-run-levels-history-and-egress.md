# 03 — Run levels, history filtering, message stamps, and the release check

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Every workflow run knows:

- **the most it may read**, its *knowledge ceiling*, frozen at admission;
- **the most it has read**, its *content level*, which only rises.

Anything the run releases is checked against the receiver's level, and anything it stores is
stamped with its content level. This covers D3, D5, P5 and P6. Filtering search results alone is
not enough. Without this phase, a level-5 answer earlier in a thread reaches a newly CC'd customer
through the next run's history, or a level-5 run posts its findings to a level-0 HTTP endpoint.

## Batch 03A — Run columns and ceiling at admission

- Add to `workflow_runs`:
  - `knowledge_ceiling public.access_level NOT NULL`;
  - `content_level public.access_level NOT NULL DEFAULT 0`;
  - `CHECK (content_level <= knowledge_ceiling)`.
  
  Every input to a run is filtered to its ceiling, so the check can only fail if there is a bug.
  Write both columns in `insert_run` (`src/adapters/persistence/workflow/admission_write.rs:94`).
- The audience is computed by one pure function,
  `audience_level(readers: impl Iterator<Item = AccessLevel>) -> AccessLevel`, built on
  `AccessLevel::lowest`. The ceiling at admission is:
  - **message-triggered run**: the audience of the trigger thread. That is every Person or
    External principal in `Thread::participant_principal_ids`, resolved by
    `PrincipalAccessContext` (01C). Agent and System principals are excluded. Read this in the
    admission transaction (D3).
  - **manual run**: the level of the member who starts it.
  - **scheduled or webhook run**: the level of the binding's `run_as` actor
    (`src/application/workflow/binding/mod.rs:77`). With no actor, the ceiling is `PUBLIC`.
  - **child run**: the parent's ceiling. The child's content level starts at the parent's
    content level when the call is made, because the child receives the parent's data.
- If the trigger message carries a stamp above the ceiling (an agent message hopping between
  channels), the run is not admitted. The outcome is typed and visible to the team.

## Batch 03B — Content level rises with what is read

- Add one persistence helper, `raise_content_level(tx, run, level)`, which runs
  `UPDATE workflow_runs SET content_level = GREATEST(content_level, $level)`. It is called inside
  the transaction that commits the output that read the content:
  - the knowledge tool observation (04);
  - the `context.history` step output (03C);
  - a child result consumed by its parent (the child's content level).
  
  Never call it in a separate transaction (invariant 4).
- The release check (03D) locks the run row `FOR SHARE` and reads `content_level` in the
  transaction that freezes the action. A parallel branch's output and its rise commit together, so
  an action either sees the rise or did not consume that output.

## Batch 03C — History respects the ceiling

- Add `messages.access_level public.access_level NOT NULL DEFAULT 0`.
- Admission's history capture (`src/adapters/persistence/workflow/admission_history.rs:4`) skips
  messages whose `access_level` is above the run's ceiling. A skipped message counts towards
  neither `eligible_count` nor any marker. The run cannot tell that it existed (D4).
- `frozen_history` (`src/adapters/persistence/workflow/history_reader.rs:53`) raises the content
  level to the highest level among the messages it returns, in its own transaction. That
  transaction already commits the read.

## Batch 03D — Stamp on store, check on release

- Stamp: every message written by a workflow message action takes the run's `content_level` at
  write time, in the same transaction (`insert_canonical_message`,
  `src/adapters/persistence/thread/message.rs:697`, and its callers). Human-authored messages
  stay at 0.
- The release decision is one pure function,
  `may_release(content: AccessLevel, destination: AccessLevel) -> bool`. It is applied in one
  place: action preparation, `ActionService::prepare_step` and `prepare_tool`
  (`src/application/workflow/actions/service.rs:83-94`). Confirm that message, HTTP, MCP and
  provisioning actions all freeze through it. Any path that does not must be routed through it,
  not given a copy of the check.
- Destination level, resolved at freeze time (not at admission):

  | Destination | Destination level |
  | --- | --- |
  | Reply on a thread (email or channel) | the audience of the thread's *current* participants. A CC added mid-run lowers it (P6) |
  | Email to an address | that member's level if the address is a verified identity of a member, otherwise `PUBLIC` |
  | Internal note, in-app message, internal reply | the run's content level (allowed). Visibility is filtered per viewer by the stamp (05) |
  | HTTP connection / MCP connection | the connection's `access_level` (P5) |
  | Provisioning built-ins | `PUBLIC`: agent configuration is visible to the whole team |
  | Child workflow | allowed. The child inherits the ceiling and content level (03A) |

- P5 schema: add `access_level public.access_level NOT NULL DEFAULT 0` to
  `workflow_http_connections` and `company_mcp_connections`. Admins set it through the existing
  connection forms, capped by `may_assign_level`'s admin rule (01D).
- A refusal produces no egress, and its outcome depends on how the action was started:
  - **tool call**: the model receives `tool_error("knowledge.audience_below_content", …)`
    (`src/application/workflow/tools.rs:111`) and can continue without that call;
  - **step action**: the step fails with the terminal failure
    `knowledge.audience_below_content`, and the run fails into attention (P6).
    No partial send, automatic regeneration or sender override (P6, option A). The withheld
    output stays in the run's step data, which the run view shows only to viewers at or above
    the run's content level (05D).
  
  Neither the error nor the attention text contains content.
- Logs and metrics record run, company, step and the two levels. They never record content or
  addresses.
- Egress paths added by other plans use this same check:
  - automatic channel reply publication and human-input prompts (channel-handles-send 04 and 05);
  - publication of remembered agent outputs (autonomous-agent-history 05B).
  
  Whichever plan lands second adds its call to `may_release` and a refusal test. It must not copy
  the rule.
- Remembered agent exchanges (autonomous-agent-history) take their stamp from `content_level`,
  read in the same locked transaction as the final checkpoint. History selection raises
  `content_level` the same way `context.history` does (03C).

## Focused checks

Prefix `knowledge_run_level_`:

- the ceiling for each trigger kind, including an empty audience → 0;
- a mixed CC thread → 0;
- a child inherits the ceiling and content level;
- a stamped trigger above the ceiling is not admitted;
- history skips higher messages and the eligible count does not reveal them;
- a history read raises the content level;
- reply release with a CC added mid-run is refused;
- connection levels gate tool calls;
- provisioning after a level-5 read is refused;
- stamps equal the content level at write time.

**Competing claimants** (prefix `knowledge_run_level_race_`): two parallel branches, one
committing a level-5 observation and one freezing an email to a level-0 address, run
concurrently many times. Every frozen action's recorded content level is at least the highest
level of the outputs it consumed. Use `own_database`.

Run the existing admission, history and action-authorization families alongside these.
