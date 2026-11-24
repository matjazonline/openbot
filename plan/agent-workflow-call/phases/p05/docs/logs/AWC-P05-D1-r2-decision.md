# Gate AWC-P05-D1 r2 — delta on r1

r1 is `logs/AWC-P05-D1-r1-decision.md` (sha256 `24c9f9ef…70bd`). r2 answers the review AWC-P05-D1-R1-N1
(F1–F7). Sections not named here stand as in r1. Coordination r22 records the new user decision U8.

## D2 — Desk discovery, replaced where stated

- **Granularity (F1).** U6's "prepared per agent turn" means per prepared agent step.
  - The user's option read "when the agent's turn is prepared"; preparation is the step's.
  - The list is built once, in the preparation transaction, and frozen in the prepared spec (v9). Resumes and
    replays of the step reuse it.
  - The reason: the prepared spec and the history checkpoint are already fixed per step. Changing a tool
    declaration mid-step would break replay exactness and the frozen history budget.
  - A new step (and every later preparation) lists afresh.
- **Audience (F2), user decision U8 (2026-11-23): "Public desks; none for external".**
  - Only `access_mode = 'public'` desks are ever listed. A members-only desk is never listed, even when the
    agent has a `participate` grant; the agent may still call it by an id from its instructions.
  - The list is omitted when the calling run answers an external correspondent:
    - In a call subtree, `enclosing_call_source_on` finds a call, and the correspondent is the calling agent,
      so the run counts as internal.
    - Otherwise the root run's trigger message is read (`workflow_message_intent_bindings` →
      `workflow_message_intents.message_id` → `messages.audience`). A run is internal only when there is no
      trigger message, or its audience is `internal_only`. Anything else counts as external.
    - When omitted, the description says: "Desk listing is unavailable while answering an external
      correspondent; call a desk by an id from your instructions."
  - **Basis.** Everything listed is the id, name and declared caller-facing intake of public desks:
    - any company member can see these, and every member may write to the desk;
    - stored previews that run inspectors see (the agent-history views) therefore show nothing a member could
      not already see;
    - no listing reaches a model answering an external correspondent;
    - the listed text adds no knowledge level, because it is level-0, company-visible metadata.
  - Assertions are in C3a and C3b below.
- **Filter equals launch (F5).**
  - Candidates are the company's public desks, ordered by name then id, at most 40.
  - Each candidate goes through the launch readers: `target_on` (shared, unchanged), then
    `decide_target(Some(facts))`. A desk is listed only on `TargetDecision::Proceed`.
  - Writability, ancestry (every ancestor, not only the run's own desk), archive, enabled and responder are
    therefore exactly what launch judges at that moment.
  - Later launch steps (levels, destination admission, D1.1 steps 6–8) are not pre-judged. The list stays
    advisory, and launch re-runs everything.
  - C3 asserts, per fixture desk, that "listed" equals "`start_on` launches it" in the same state.
- **Escaping (F3).** No fenced block. The list is one JSON array produced by `serde_json`, with every backtick
  replaced by ``` and every `<` by `<`, under the data-not-instructions line. A name containing
  ```` ``` ```` is added to C5.
- **Bounds (F4).**
  - Name: at most 64 chars. Field name: at most 64 chars.
  - Type: one of a fixed vocabulary taken from the bounded intake grammar (p01 D3): `string`, `number`,
    `boolean`, `enum`, `array`. Enum values are not listed.
  - `required`: a boolean. Description: at most 120 chars.
  - Truncation is by `chars()`, so on a character boundary, and happens before serialization, so the JSON stays
    valid.
  - **Budget fill.** Desks go in order, each with its fields in declared order (at most 8). The serialized
    array must stay ≤ 6 KiB:
    1. fields are dropped from that desk's tail first, and the desk records `"more_fields": n`;
    2. if the bare desk (id and name) still does not fit, listing stops;
    3. the remainder becomes "n more desks".
- **Samples and preview (F7).** Sample and preview preparation build the list empty, with the line "Sample runs
  list no desks". B2 can keep the spec shape.

## D3 — Channel views, amended (F6)

- **Status by access mode.**
  - Public: "Any agent in the company can call this desk."
  - Members-only: "Only agents granted participation can call this desk; it is not listed to agents."
  - Not callable: the first failing reason (disabled, no responder selected, unit archived).
- **Scope.** The desk's channel settings page is the channel detail view, and the block goes there. No channel
  *selection* view picks a call target: the model picks the desk at runtime from the list or its instructions.
  The existing responder selection on the same card is unchanged. No other selection view is in scope.

## D2 help text, amended (F6, A.4)

The description adds: "This sends a request message to a desk; its responder decides how to handle it. To run
a specific workflow with direct input, use that workflow's own tool instead."

## Verification plan, amended rows

| ID | Test | Assertion |
| --- | --- | --- |
| C3 | `channel_call_authoring_discovery_lists_callable_public_desks` | Fixture desks: a public desk; a public desk in the caller's ancestry; a members desk with a grant; a members desk without; a disabled desk; an archived unit's desk; a desk with no responder; the run's own desk; a principal inbox. Only the first is listed. For each, "listed" equals the `start_on` outcome being a launch, except the members desk with a grant: it launches but is not listed (U8), and the test says so. |
| C3a | `channel_call_authoring_discovery_omitted_for_external` | A run triggered by an external-audience message prepares with no desks and the omission line. An internal-trigger run and a call child (whose root is external) list desks. |
| C3b | `channel_call_authoring_discovery_level_zero` | The prepared step's list adds no knowledge level: the run's `content_level` is unchanged by preparation. |
| C5 | `channel_call_authoring_discovery_bounds` | Adds: a name containing ```` ``` ```` and `</` renders escaped (```, `<`); 20 desks × 8 fields × 120-char descriptions stays ≤ 6 KiB with `more_fields` and "n more desks"; a multi-byte name truncates on a boundary; the output parses as JSON. |
| C6 | `channel_call_authoring_help_examples` | Adds the direct-workflow-input sentence. |
| C7 | `channel_call_authoring_desk_card` | Adds the public and members-only status texts. |

## Invariants, replaced

1. Only public desks that launch would admit at preparation time ever reach the model through the list.
2. No list reaches a model answering an external correspondent.
3. The list is frozen per prepared step and advisory; launch authority is unchanged.
4. There is no automatic grant of `channel.call`.
