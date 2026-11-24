# AWC-P05 review findings

## AWC-P05-D1-F1 — D2 freezes the desk list per prepared step; U6 says "prepared per turn"

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); phase AWC-P05, stage architecture gate; round [AWC-P05-D1-R1-N1](agent-review-notes.md#awc-p05-d1-r1-n1); reviewer B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD `fe7791d3`; decision [AWC-P05-D1 r1](logs/AWC-P05-D1-r1-decision.md), sha256 `24c9f9efd8831d3659386568030e6bbbfeb4bc29e29d8086e4d3a6b6960170bd`; pre-gate digest `b2c2b594…5976`. Originates in the decision.
- **Classification:** actionable; medium; requirements (a user decision).
- **Evidence.**
  - U6 (coordination r21; TASK "User decisions") says the description is "prepared per agent turn".
  - D2 fills the list "once, when the step's agent is prepared", and states that "replays and
    resumes of the step reuse the frozen list". A step spans several model turns: a
    `channel.call` parks it, and the resume continues the same exchange from
    `frozen_preparation` (`agent_history/checkpoint.rs:33-60, 121-131`). So a desk added or
    removed mid-step is not reflected on the next turn, which contradicts the user's wording.
  - Freezing per step is the sound choice. The prepared spec, its version check and the history
    checkpoint are per execution, and re-reading per turn would change tool declarations inside
    one frozen exchange, against exactness and replay. But the decision neither notes nor
    justifies the divergence.
- **Required.** State the interpretation: "turn" means the prepared agent step, with the reason.
  Record it in coordination as a clarification of U6, or ask the user if it is unclear. Keep C4.
- **Instruction lesson (candidate).** When a gate narrows a user decision's literal wording, quote
  the wording and say why.
- **Resolution history.** 2026-11-23 open.
  2026-11-23 fixed (AWC-P05-B2-R2-N1, digest `905b73f8…8d5a`):
  - `CallAccess.decisions` is filled through `read_decision`, the decision page's own authority.
  - A render unit covers the child-readable, decision-hidden case.
  - The doc is updated.
  - Closed.

## AWC-P05-D1-F2 — The leak check covers the agent only, not who reads what the model writes

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); phase AWC-P05, stage architecture gate; round [AWC-P05-D1-R1-N1](agent-review-notes.md#awc-p05-d1-r1-n1); reviewer B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD `fe7791d3`; decision [AWC-P05-D1 r1](logs/AWC-P05-D1-r1-decision.md), sha256 `24c9f9efd8831d3659386568030e6bbbfeb4bc29e29d8086e4d3a6b6960170bd`; pre-gate digest `b2c2b594…5976`. Originates in the decision.
- **Classification:** actionable; medium; security and confidentiality (doc 05 A.3 "inaccessible
  channel metadata", p01 I2).
- **Evidence.**
  - D2's "Leak check" argues that only desks the *agent* may write to are listed.
  - The list is model input, so it can reach every audience of the model's output.
    - An ingress run answering an external correspondent can quote members-only desk names, ids
      or intake descriptions in its reply. Final-reply knowledge release judges the run's content
      level, and the decision gives the list no level, so release is never triggered.
    - The frozen spec is stored in `frozen_preparation` and projected by agent-history views
      (`agent_history_views/rows.rs:61-72`, `projection.rs:128`) to anyone who may inspect that
      run.
  - Before p05, no ordinary agent had any desk listing (`list_company_agents` is builder-only),
    so this exposure is new.
- **Required.** State the audience rule and its basis. For example: desk name, id and declared
  intake metadata are company-internal level-0 data; the run's readers and outbound replies may
  carry them; members-only desks are included because the agent holds `participate`. Or narrow
  the list, for example by omitting it in runs whose association is an external correspondent.
  Name the readers of the frozen spec, and add one assertion that matches the rule.
- **Resolution history.** 2026-11-23 open.

## AWC-P05-D1-F3 — A fenced JSON block does not contain a name with a backtick fence

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); phase AWC-P05, stage architecture gate; round [AWC-P05-D1-R1-N1](agent-review-notes.md#awc-p05-d1-r1-n1); reviewer B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD `fe7791d3`; decision [AWC-P05-D1 r1](logs/AWC-P05-D1-r1-decision.md), sha256 `24c9f9efd8831d3659386568030e6bbbfeb4bc29e29d8086e4d3a6b6960170bd`; pre-gate digest `b2c2b594…5976`. Originates in the decision.
- **Classification:** actionable; low; security (prompt injection, doc 05 "malicious labels").
- **Evidence.** D2 renders names and descriptions "JSON-escaped inside one fenced JSON block".
  JSON string escaping leaves backticks alone, so a desk name or field description containing
  ```` ``` ```` closes the fence, and what follows reads as prose outside the "data" block. C5's
  hostile case (quotes, newlines, "ignore previous instructions") does not include a fence.
- **Required.** Escape backticks, for example as `\u0060`, or drop the fence and emit one JSON
  value whose strings cannot terminate it. Add a ```` ``` ```` name to C5.
- **Resolution history.** 2026-11-23 open.

## AWC-P05-D1-F4 — Bounds: field names and types are unbounded, and the overflow order is unstated

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); phase AWC-P05, stage architecture gate; round [AWC-P05-D1-R1-N1](agent-review-notes.md#awc-p05-d1-r1-n1); reviewer B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD `fe7791d3`; decision [AWC-P05-D1 r1](logs/AWC-P05-D1-r1-decision.md), sha256 `24c9f9efd8831d3659386568030e6bbbfeb4bc29e29d8086e4d3a6b6960170bd`; pre-gate digest `b2c2b594…5976`. Originates in the decision.
- **Classification:** actionable; low; correctness (bounds).
- **Evidence.**
  - Per desk the shape bounds the name (64 chars) and each description (120 chars), but not field
    names, enum values, or the rendered type text.
  - 20 desks x 8 fields x a ~120-char description is already about 20 KiB, over the 6 KiB cap. The
    decision does not say which part gives way: whole desks dropped in order, fields per desk, or
    descriptions.
  - It does not say whether "chars" truncation respects UTF-8 boundaries, or that the block must
    stay valid JSON after truncation.
- **Required.** Bound every rendered string. Define the overflow order, for example dropping whole
  desks from the end of the name order and counting them in "N more". State char-safe truncation
  and valid JSON. Assert this in C5 with a maximal-intake case.
- **Resolution history.** 2026-11-23 open.

## AWC-P05-D1-F5 — "Equals check 3 plus callability" is a restatement, not shared code

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); phase AWC-P05, stage architecture gate; round [AWC-P05-D1-R1-N1](agent-review-notes.md#awc-p05-d1-r1-n1); reviewer B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD `fe7791d3`; decision [AWC-P05-D1 r1](logs/AWC-P05-D1-r1-decision.md), sha256 `24c9f9efd8831d3659386568030e6bbbfeb4bc29e29d8086e4d3a6b6960170bd`; pre-gate digest `b2c2b594…5976`. Originates in the decision.
- **Classification:** actionable; low; correctness (invariant 1 and drift).
- **Evidence.**
  - D2 restates `target_on`'s rules (`channel_call_target.rs:386-432`) for a set query: the
    access mode or `participate` grant, the responder read at the selected
    `configuration_revision` through `read_binding_at`, `active`, the inbox association and
    `message_trigger().is_some()`.
  - The responder filter is Rust over a binding snapshot, not SQL, so a hand-written set query can
    drift. The invariants then fail silently, in both directions: a desk listed but refused, or
    callable but unlisted.
  - "Not the calling run's own desk" is narrower than launch's `in_ancestry_on`, so ancestor
    desks are listed and then refused as cycles. That is acceptable as advisory, but unstated as
    a choice.
- **Required.** Build each candidate's `DeskFacts` with the same readers as `target_on`
  (`agent_participates_on`, `settings_on`, `read_binding_at`), and list exactly the desks
  `decide_target` would admit, ancestry aside. Or add to C3 a per-desk assertion that the
  listing equals launch's admission through check 8, on the same fixture. State the ancestry
  choice.
- **Resolution history.** 2026-11-23 open.

## AWC-P05-D1-F6 — Two parts of Batch A left out

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); phase AWC-P05, stage architecture gate; round [AWC-P05-D1-R1-N1](agent-review-notes.md#awc-p05-d1-r1-n1); reviewer B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD `fe7791d3`; decision [AWC-P05-D1 r1](logs/AWC-P05-D1-r1-decision.md), sha256 `24c9f9efd8831d3659386568030e6bbbfeb4bc29e29d8086e4d3a6b6960170bd`; pre-gate digest `b2c2b594…5976`. Originates in the decision.
- **Classification:** actionable; low; requirements completeness.
- **Evidence.**
  - Doc 05 A.4: "Explain that direct arbitrary workflow input is a different invocation
    contract." Neither D2's rendering nor C6 includes it.
  - Doc 05 A.3: "callable status". D3's status "Agents can call this desk" ignores the access
    mode. A members-only desk is callable only by agents with a `participate` grant (check 3), so
    the card overstates who can call. A.3 also says "detail/selection views", and D3 extends only
    the response settings card without saying whether selection views are out of scope.
- **Required.** Add the A.4 sentence to the description, with a C6 assertion. Make D3's status
  reflect the access mode, for example "Any agent with the channel.call tool" for public desks
  versus "Agents granted participation". State the selection-view scope.
- **Resolution history.** 2026-11-23 open.

## AWC-P05-D1-F7 — Optional: say what samples and preview prepare

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); phase AWC-P05, stage architecture gate; round [AWC-P05-D1-R1-N1](agent-review-notes.md#awc-p05-d1-r1-n1); reviewer B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD `fe7791d3`; decision [AWC-P05-D1 r1](logs/AWC-P05-D1-r1-decision.md), sha256 `24c9f9efd8831d3659386568030e6bbbfeb4bc29e29d8086e4d3a6b6960170bd`; pre-gate digest `b2c2b594…5976`. Originates in the decision.
- **Classification:** optional; low.
- **Evidence.** `from_builtin` yields an empty list, and only persistence preparation fills it. The
  sample and preview preparation paths (B2) would show the tool with no desks. Stating that now
  (empty, or a fixture list) keeps B2 from changing the spec shape again.
- **Resolution history.** 2026-11-23 recorded. 2026-11-23 declined by the author (TASK, B2-R1
  corrections); accepted.

### AWC-P05-D1-F1…F7 — resolution updates (recheck AWC-P05-D1-R2-N1, decision r2 sha256 `541203da…28e2`)

- **F1, fixed.** "Per agent turn" is defined as per prepared step, with the exactness and
  history-checkpoint reason, and every new step lists afresh. Closed.
- **F2, fixed by user decision U8** (coordination r22).
  - Only public desks are listed, and no list is given to a run answering an external
    correspondent.
  - The basis is stated: data visible to every member, level 0, and the agent-history readers are
    covered.
  - C3a and C3b assert it.
  - The way "external" is judged has a gap, recorded as new finding F9. Closed otherwise.
- **F3, fixed in substance.** There is no fence; output is `serde_json` with backticks and `<`
  escaped, and a fence name is added to C5. The decision text lost the escape sequences, recorded
  as new finding F8. Closed otherwise.
- **F4, fixed.**
  - Every rendered string is bounded, and types come from a fixed vocabulary with no enum values.
  - Truncation uses `chars()` before serialization, so the JSON stays valid.
  - The fill order is stated: `more_fields`, then stop, then "n more desks".
  - C5 has a maximal case. Closed.
- **F5, fixed.** Each candidate goes through `target_on` and `decide_target`, and C3 compares
  listed with launched per desk. The one difference, the granted members-only desk, comes from U8
  and is asserted. Ancestry now uses the full chain. Closed.
- **F6, fixed.** The direct-workflow-input sentence is added (C6). Status text depends on the
  access mode (C7). Selection-view scope is stated: no selection view picks a call target. Closed.
- **F7, taken.** Samples and preview get an empty list with a line. Closed.

## AWC-P05-D1-F8 — The escaping rule's replacement strings were lost from the decision text

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); phase AWC-P05, stage gate recheck; round [AWC-P05-D1-R2-N1](agent-review-notes.md#awc-p05-d1-r2-n1); reviewer B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD `fe7791d3`; decision [AWC-P05-D1 r2](logs/AWC-P05-D1-r2-decision.md), sha256 `541203da3a7791296a03a9849f9d409852d0568ff6d24ed3ee155a92b1f428e2`; pre-gate digest `d9e156a5…6a2e`. Originates in the decision.
- **Classification:** actionable; low; does not block the gate, because the intent is unambiguous
  from F3. Fix in the batch.
- **Evidence.** r2 D2 "Escaping" reads "every backtick replaced by ``` and every `<` by `<`", and
  C5 reads "renders escaped (```, `<`)". The intended `\u0060` and `\u003c` were evidently
  unescaped when the file was written. Read literally, the rule replaces each character with
  itself.
- **Required (in the batch).** Record the exact replacement strings, `\u0060` and `\u003c`, in
  TASK. C5 must assert that those literal sequences appear in the rendered description and that
  no raw backtick or `<` comes from listed data.
- **Resolution history.** 2026-11-23 open; deferred by B' to batch B1.

## AWC-P05-D1-F9 — "External" is judged only from the root's trigger message; reruns and manual runs on a customer thread count as internal

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); phase AWC-P05, stage gate recheck; round [AWC-P05-D1-R2-N1](agent-review-notes.md#awc-p05-d1-r2-n1); reviewer B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD `fe7791d3`; decision [AWC-P05-D1 r2](logs/AWC-P05-D1-r2-decision.md), sha256 `541203da3a7791296a03a9849f9d409852d0568ff6d24ed3ee155a92b1f428e2`; pre-gate digest `d9e156a5…6a2e`. Originates in the decision.
- **Classification:** actionable; low; security (U8 "none for external"). Fix in the batch.
- **Evidence.**
  - r2 counts a run as internal "when there is no trigger message", read through
    `workflow_message_intent_bindings`.
  - Those rows are written only by message admission (`admission.rs:190-200`,
    `message_intents.rs:342`, `admission_refusal_claims.rs:17`).
  - A manual rerun of an ingress run (`admission_source.rs:7-14, 39-66`: reruns are manual
    starts) has no binding, yet it runs the same workflow on the same thread, with the original
    input's `source_message`. It can therefore `message.reply` to the external correspondent.
  - The same holds for a manual run given a message input on an external thread. Under r2 both
    would get the desk list.
- **Required (in the batch).** Judge "external" from what the run can answer, not only from the
  root's binding. For example: the run's thread association has an external correspondent, or the
  input's `source_message` has a non-`internal_only` audience. A call subtree stays internal.
  Add a rerun-of-ingress case to C3a.
- **Resolution history.** 2026-11-23 open; deferred by B' to batch B1.

## AWC-P05-D1-F10 — Optional: the 40-candidate window and preparation locks

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); phase AWC-P05, stage gate recheck; round [AWC-P05-D1-R2-N1](agent-review-notes.md#awc-p05-d1-r2-n1); reviewer B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD `fe7791d3`; decision [AWC-P05-D1 r2](logs/AWC-P05-D1-r2-decision.md), sha256 `541203da3a7791296a03a9849f9d409852d0568ff6d24ed3ee155a92b1f428e2`; pre-gate digest `d9e156a5…6a2e`. Originates in the decision.
- **Classification:** optional; low.
- **Evidence.**
  - With at most 40 candidates, "n more desks" undercounts when a company has more than 40 public
    desks. It would be better to count the remainder separately, or say "more desks".
  - `target_on` takes `FOR SHARE` locks on each candidate desk, `FOR KEY SHARE` on its unit,
    and share locks in `agent_participates_on`, all held until preparation commits. Desk settings
    writers wait on agent preparation; that is acceptable, but should be stated.
- **Resolution history.** 2026-11-23 recorded.

### AWC-P05-D1-F8, F9, F10 — batch dispositions (AWC-P05-B1-R1-N1, digest `fb860437…fdfd`)

- **F8, fixed.** `channel_call_listing.rs::rendered` serializes with `serde_json`, then replaces
  `` ` `` with `\u0060` and `<` with `\u003c`. Neither character can occur in JSON syntax
  outside a string, so the output stays valid JSON. C5 (`channel_call_listing.rs` tests)
  asserts both sequences, and that no raw backtick or `<` comes from the data. TASK Status records
  the exact rule. Closed.
- **F9, fixed in code, but its required test is missing.** `answers_external_on` implements the
  three sources (trigger audience, thread messages of the run or its root, the input's
  `source_message`), and a call subtree is internal. C3a exercises only the thread source, and
  only with a direct message insert. See AWC-P05-B1-F2.
- **F10, taken.** "More" counts admitted desks plus the public desks past the window. The locks are
  recorded in TASK. Closed.

## AWC-P05-B1-F1 — A prepared-version pin left at 8: `doop_agent_mcp_error_policy_is_frozen_in_preparation_and_checkpoint_identity` now fails

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); phase AWC-P05, batch B1, stage batch review; round [AWC-P05-B1-R1-N1](agent-review-notes.md#awc-p05-b1-r1-n1); reviewer B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD `fe7791d3` (uncommitted); artifact digest `fb860437a159df93045d7975ed0b3c2a6cc104a9958c21edb4cf9559f5e0fdfd`. Originates in the batch.
- **Classification:** actionable; medium; correctness of verification (a regression this batch
  caused, outside its focused filters).
- **Evidence.**
  - `src/application/workflow/agent_tests.rs:91` asserts `strict_spec.prepared["version"] == 8`.
    With `PREPARED_AGENT_VERSION = 9` it fails.
  - B's run of `cargo test --offline --lib -- doop_agent_`: 17 passed, 2 failed
    ([log](logs/AWC-P05-B1-R1-reviewer-version-pins.txt)). One failure is the known
    `doop_agent_current_preparation_reaches_ready_before_model_io` (asserts 7). The other is this
    test, `left: 9, right: 8`, which passed before p05.
  - None of the batch's filters (`agent_history`, `exactness`, `agent_publication` and so on)
    match its name, so TASK's "1 failed (known)" holds only for the filters run.
  - The two schema pins (`agent_history_checkpoint_matches`, `agent_history_preparation_valid`)
    are the only DB ones. Rust readers use the constant (`checkpoint.rs:47,128`,
    `agent_history_views/projection.rs:82`). This test is the only other literal pin B found, by
    searching `src` for version 8 and for the constant.
- **Required.** Assert `PREPARED_AGENT_VERSION` instead of a literal, and rerun the test.
  Recommended: rerun `doop_agent_` as part of the affected filters.
- **Instruction lesson (candidate).** When bumping a persisted format version, search the tests for
  the old literal value, not only for the constant's name.
- **Resolution history.** 2026-11-23 open.

## AWC-P05-B1-F2 — C3a does not exercise F9's trigger and input sources, nor the promised rerun

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); phase AWC-P05, batch B1, stage batch review; round [AWC-P05-B1-R1-N1](agent-review-notes.md#awc-p05-b1-r1-n1); reviewer B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD `fe7791d3` (uncommitted); artifact digest `fb860437a159df93045d7975ed0b3c2a6cc104a9958c21edb4cf9559f5e0fdfd`. Originates in the batch.
- **Classification:** actionable; low; verification against gate finding AWC-P05-D1-F9.
- **Evidence.**
  - TASK Status (F9): "C3a adds a manual rerun of an ingress run on the customer thread (listed
    nothing)".
  - `channel_call_authoring_tests.rs:156-230` only inserts an `external_conversation` message
    into an internal thread (source 2). Its comment says a rerun "would not either", but no rerun
    is run.
  - Source 1 (the root trigger message's audience) and source 3 (`input->>'source_message'`) are
    never exercised.
  - Source 3 is the one that covers a manual run handed a customer message on another thread, and
    its join compares `message.id::text` with the input string, which no test checks.
- **Required.** Add cases that reach `answers_external_on` through source 1 (an ingress-admitted
  run) and through source 3 (a manual run whose input names an external message, with no customer
  message on its thread). Run one as a real manual rerun of the ingress run, or correct the TASK
  record.
- **Resolution history.** 2026-11-23 open.

## AWC-P05-B1-F3 — The operations doc says the approved response is "as edited"; channel finals have no edit path

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); phase AWC-P05, batch B1, stage batch review; round [AWC-P05-B1-R1-N1](agent-review-notes.md#awc-p05-b1-r1-n1); reviewer B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD `fe7791d3` (uncommitted); artifact digest `fb860437a159df93045d7975ed0b3c2a6cc104a9958c21edb4cf9559f5e0fdfd`. Originates in the batch.
- **Classification:** actionable; low; documentation accuracy (AGENTS.md: documentation states what
  the code does).
- **Evidence.**
  - `docs/workflow-operations.md`, "Channel calls", **Approval**: "Only the approved version, as
    edited, becomes the tool result."
  - Gate AWC-P04-D1 r1 D4 and p04 B1 established the opposite. Channel final review is approve or
    reject with `data == {}`, and approval releases exactly `proposal.content`. No edit path
    exists.
- **Required.** Replace this with, for example, "Only the approved proposal's content becomes the
  tool result; reviewers approve or reject, and cannot edit it."
- **Resolution history.** 2026-11-23 open.

## AWC-P05-B1-F4 — Optional notes

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); phase AWC-P05, batch B1, stage batch review; round [AWC-P05-B1-R1-N1](agent-review-notes.md#awc-p05-b1-r1-n1); reviewer B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD `fe7791d3` (uncommitted); artifact digest `fb860437a159df93045d7975ed0b3c2a6cc104a9958c21edb4cf9559f5e0fdfd`. Originates in the batch.
- **Classification:** optional; low.
- **Evidence and suggestions.**
  - A resume still runs a fresh `desk_listing_on` in `candidate_on` (`preparation.rs:269-276`):
    up to 40 `target_on` calls with `FOR SHARE` locks. `frozen::restore_on` then discards it
    for the saved list. The result is correct and byte-equal, but each resume takes those locks
    and can fail on the listing reads. Skipping the listing when a frozen preparation exists would
    avoid both.
  - Deviation 4 says a builder-authored `channel.hand_off` "can only name a run literally". A
    `ref` to an `agent.run` step's output also works, as in p04's `hand_off_through`. The
    runtime refusal is what protects, so only the note is inaccurate.
  - Not verified: the doc's "expiry fails the child, and the caller sees `workflow.child_failed`"
    for a call deadline. B did not trace run-deadline expiry to the child's terminal state.
- **Resolution history.** 2026-11-23 recorded.

### AWC-P05-B1-F1…F4 — resolution updates (recheck AWC-P05-B1-R2-N1, digest `22fdcacd…ba13`)

Recheck by B' (Opus 5.5, Opus substitute reviewer, U7), round
[AWC-P05-B1-R2-N1](agent-review-notes.md#awc-p05-b1-r2-n1), 2026-11-23.

- **F1, fixed.** `agent_tests.rs` and `agent_history_preparation_tests.rs:21` assert
  `PREPARED_AGENT_VERSION`. A search of `src` found no other literal prepared-version pin. B's run
  includes `doop_agent_mcp_error_policy_…`, which passes
  ([log](logs/AWC-P05-B1-R2-reviewer-focused.txt)). Closed.
- **F2, fixed.** `channel_call_authoring_discovery_external_sources` reaches `answers_external_on`
  through production preparation for source 1 and source 3, and ends with a plain sibling that
  still lists.
  - Source 1 is an admitted intent binding to an external message on a thread the run is not on.
  - Source 3 is the input's `source_message` naming an external message on no thread of the run.
  - The rows are inserted by SQL, not by ingress admission or a real rerun. That is acceptable,
    because the predicate under test reads only those rows. The author's mutation check
    (source predicate set to false, then the assertion fails) is accepted as recorded.
  - The C3a comment now matches its setup. Closed.
- **F3, fixed.** `docs/workflow-operations.md:312` reads "Only the approved response becomes the
  tool result; reviewers approve or reject it." Closed.
- **F4 (optional).** Re-listing on resume is declined with a reason in TASK. The deviation-4 note
  is corrected. The deadline claim is backed by p03's
  `channel_call_launch_shorter_deadline_fails_child`. Closed.

## AWC-P05-D2-F1 — The phase function is not total: a completed caller wait with no stored result has no phase

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); phase AWC-P05, batch B2, stage architecture gate; round [AWC-P05-D2-R1-N1](agent-review-notes.md#awc-p05-d2-r1-n1); reviewer B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD `fe7791d3`; decision [AWC-P05-D2 r1](logs/AWC-P05-D2-r1-decision.md), sha256 `de0e861270efe6f8b0c6f7f00a627e761dda8806e594c10502248e01ae7778ca`; pre-gate digest `22fdcacd…ba13`. Originates in the decision.
- **Classification:** actionable; medium; correctness (B.1 "response state", B.5 "parent
  continuation").
- **Evidence.**
  - Order of events: the child terminal writes `parent_wakeup`. The caller's `resume_wait` then
    settles the `child_run` wait to `completed` (`child_waits.rs:293-300`). Only later does the
    caller's continuation consume the call. `consume_on` requires `wait_state == 'completed'` and
    inserts the result row.
  - D1's rows cover "result row exists", child queued, running or waiting, child succeeded "and the
    caller's wait is still open", and child failed or cancelled "not yet consumed". With a
    completed wait and no result row, these states have no phase:
    1. after resume, before the continuation consumes (a normal window, and the one where a stuck
       caller continuation shows);
    2. consumption answered `revoked`, which by design stores no row (p04 D7), so it stays without
       a phase forever;
    3. the caller was cancelled after the child succeeded and before consumption.
  - The `response_ready` gauge inherits the same narrow definition.
- **Required.** Make the function total.
  - Add phases, for example `AwaitingCaller { stalled }` (wait completed, no result, caller
    execution open) and `ClosedWithoutResult { reason }` (the caller execution ended without a
    row: revoked, or the caller cancelled or failed).
  - Define `ResponseReady` and `ChildEnded` over both wait states.
  - V1 should enumerate the input space, including the revoked and caller-cancelled cases, and
    assert totality.
- **Resolution history.** 2026-11-23 open.

## AWC-P05-D2-F2 — `AwaitingFinalReview` keys on `waiting_reason = 'decision'`, which also covers the desk's own human decisions

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); phase AWC-P05, batch B2, stage architecture gate; round [AWC-P05-D2-R1-N1](agent-review-notes.md#awc-p05-d2-r1-n1); reviewer B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD `fe7791d3`; decision [AWC-P05-D2 r1](logs/AWC-P05-D2-r1-decision.md), sha256 `de0e861270efe6f8b0c6f7f00a627e761dda8806e594c10502248e01ae7778ca`; pre-gate digest `22fdcacd…ba13`. Originates in the decision.
- **Classification:** actionable; medium; correctness (B.1 and the focused check "a waiting review
  is distinguishable").
- **Evidence.**
  - A destination workflow may contain `decision.human` steps, which the p04 D8 call child keeps.
    Those park the child with the same `waiting_reason = 'decision'` as the final-reply review.
  - The worker's `decision.human` parking and the final-review parking
    (`channel_reply_approvals::park_on`) both use decision waits.
  - D1 and the `workflow_channel_calls_review_waiting` gauge would therefore report a workflow's
    business decision as "awaiting final review".
- **Required.** Classify as `AwaitingFinalReview` only when the open decision is the final-reply
  approval, for example `workflow_decisions.reply_proposal_id IS NOT NULL`, as p04's
  `review_decision` reads it. Other decision waits are `ChildWaiting { reason }`. Add the
  `decision.human` case to V3 and V9.
- **Resolution history.** 2026-11-23 open.

## AWC-P05-D2-F3 — "Stalled" covers only the child's job, not the caller's continuation

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); phase AWC-P05, batch B2, stage architecture gate; round [AWC-P05-D2-R1-N1](agent-review-notes.md#awc-p05-d2-r1-n1); reviewer B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD `fe7791d3`; decision [AWC-P05-D2 r1](logs/AWC-P05-D2-r1-decision.md), sha256 `de0e861270efe6f8b0c6f7f00a627e761dda8806e594c10502248e01ae7778ca`; pre-gate digest `22fdcacd…ba13`. Originates in the decision.
- **Classification:** actionable; low; requirements (B.5 "parent continuation").
- **Evidence.** `stalled` and `workflow_channel_calls_stalled` read the child job's lease. A child
  that has finished, whose caller's continuation job is stuck (lease expired, or pending past due),
  is not flagged. This is the state that F1's first case exposes. The gauges are built only over
  open caller waits, so a completed wait whose continuation never consumes is invisible to all of
  them.
- **Required.** Flag a caller continuation stall with the same predicate, on the caller's job for
  the calling execution. Count it in a gauge, for example `workflow_channel_calls_awaiting_caller`
  and its stalled share, scanned from completed call waits with no result row and an open caller
  execution, which is still bounded by open work.
- **Resolution history.** 2026-11-23 open.

## AWC-P05-D2-F4 — "Add the `channel_call` module to PURE" does not fit a symbol allowlist, and the module holds the live dispatch port

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); phase AWC-P05, batch B2, stage architecture gate; round [AWC-P05-D2-R1-N1](agent-review-notes.md#awc-p05-d2-r1-n1); reviewer B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD `fe7791d3`; decision [AWC-P05-D2 r1](logs/AWC-P05-D2-r1-decision.md), sha256 `de0e861270efe6f8b0c6f7f00a627e761dda8806e594c10502248e01ae7778ca`; pre-gate digest `22fdcacd…ba13`. Originates in the decision.
- **Classification:** actionable; low; architecture (the sample effect boundary).
- **Evidence.**
  - `scripts/workflow-sample-boundary.py` allows named symbols ("named pure engine functions";
    "New dependencies require an explicit allowlist change").
  - `channel_call.rs` also defines `ChannelCallDispatch`, the live dispatch port
    (`channel_call.rs:237-243`), and `builtin_channel_call_snapshot`.
  - Admitting the module wholesale, if the script is changed to allow that, would let sample code
    name the live port.
- **Required.** List the exact symbols the sample uses, for example `channel_call::ChannelCallRequest`,
  `channel_call::check_intake`, `channel_call::ChannelCallOutcome`, `channel_call::ChannelCallError`,
  `channel_call::channel_call_schemas` and `channel_call::decide_target` / `TargetDecision` /
  `DeskFacts`, plus the needed code constants. `ChannelCallDispatch` stays out. V10 runs the
  script.
- **Resolution history.** 2026-11-23 open.

## AWC-P05-D2-F5 — Responder identity and review policy reach viewers who may only see the desk, or not even that

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); phase AWC-P05, batch B2, stage architecture gate; round [AWC-P05-D2-R1-N1](agent-review-notes.md#awc-p05-d2-r1-n1); reviewer B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD `fe7791d3`; decision [AWC-P05-D2 r1](logs/AWC-P05-D2-r1-decision.md), sha256 `de0e861270efe6f8b0c6f7f00a627e761dda8806e594c10502248e01ae7778ca`; pre-gate digest `22fdcacd…ba13`. Originates in the decision.
- **Classification:** actionable; low; confidentiality (B.1 "authorized links", doc 05 A.3
  "inaccessible channel metadata").
- **Evidence.**
  - D3 capture requires only that the actor "can view the desk". It then freezes the responder
    binding id and revision and `review_required`, and the sample report and identity can expose
    them.
  - B1's desk card, which shows the responder label and review requirement, appears only on pages
    that already authorize channel settings (gate D1 D3).
  - D1's call item shows "responder binding and its revision" unconditionally, while the desk name
    is gated on desk visibility.
  - Ids alone, but they reveal configuration that a desk viewer, or a caller-run viewer, is not
    otherwise shown.
- **Required.**
  - Gate the responder binding and revision in D1 on the same desk visibility as the name, or on
    channel-settings access.
  - In D3, keep the binding identity inside the hashed sample identity and render only what a
    live call would tell the agent: the decision, the intake and whether a review stop applies.
  - Alternatively, require channel-settings access to capture a target.
  - Add the case to V4 and V8.
- **Resolution history.** 2026-11-23 open.

## AWC-P05-D2-F6 — Sample fixtures' `fields` are not checked against the desk's declared result fields

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); phase AWC-P05, batch B2, stage architecture gate; round [AWC-P05-D2-R1-N1](agent-review-notes.md#awc-p05-d2-r1-n1); reviewer B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD `fe7791d3`; decision [AWC-P05-D2 r1](logs/AWC-P05-D2-r1-decision.md), sha256 `de0e861270efe6f8b0c6f7f00a627e761dda8806e594c10502248e01ae7778ca`; pre-gate digest `22fdcacd…ba13`. Originates in the decision.
- **Classification:** actionable; low; requirements (acceptance: "authoring, samples and run
  inspection describe the same invocation and response semantics").
- **Evidence.**
  - D3 freezes the intake schema only, and checks the fixture against the generic `channel.call`
    output schema.
  - Live `fields` follow the destination version's `output_schema.properties.fields`, which p04
    validates at the terminal. `channel.deny` uses the same sub-schema.
  - A sample can therefore return `fields` the desk could never produce.
- **Required.** Freeze the responder version's result-field sub-schema in `FrozenCallTarget`
  (bounded by p01 D3's grammar), and validate a fixture's `fields` against it. Assert it in V6.
- **Resolution history.** 2026-11-23 open.

## AWC-P05-D2-F7 — Optional notes

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); phase AWC-P05, batch B2, stage architecture gate; round [AWC-P05-D2-R1-N1](agent-review-notes.md#awc-p05-d2-r1-n1); reviewer B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD `fe7791d3`; decision [AWC-P05-D2 r1](logs/AWC-P05-D2-r1-decision.md), sha256 `de0e861270efe6f8b0c6f7f00a627e761dda8806e594c10502248e01ae7778ca`; pre-gate digest `22fdcacd…ba13`. Originates in the decision.
- **Classification:** optional; low.
- **Evidence and suggestions.**
  - Declining new `workflow_run_events` kinds is acceptable against B.5. The transitions are
    recorded by existing durable rows: call waits are audited with `wait_parked` /
    `wait_completed` and `wait_id` (`workflow_tool_calls.rs:243`, `child_waits.rs:293`, the
    `workflow_run_event_wait_shape` check), and the child carries its own events and the result
    rows.
  - A refusal leaves no run row, only the agent-history tool result. That is durable, and the
    stuck-call signals do not depend on it. The outcome counter is a rate only, as stated.
  - Suggestion: have V2 or V9 assert that a call wait has its `wait_parked` / `wait_completed`
    pair, since D5 relies on them.
  - The capture's members-only desks are allowed in samples (U8 limits only the listing). Say so
    in the docs, so readers do not expect sample targets to mirror the list.
- **Resolution history.** 2026-11-23 recorded.

### AWC-P05-D2-F1…F7 — resolution updates (recheck AWC-P05-D2-R2-N1, decision r2 sha256 `4fd2383d…94ae`)

- **F1, fixed.** The rules are ordered and total, over the result row, the wait state, the child,
  the caller execution and run state, and `stalled`. V1 enumerates the combinations, including the
  revoked and caller-cancelled cases. One refinement is left, as new finding F8. Closed.
- **F2, fixed.** Final review requires `workflow_decisions.reply_proposal_id IS NOT NULL` on the
  child's open decision; `decision.human` gives `ChildWaiting` (V3, V9). Closed.
- **F3, fixed.** One `stalled` predicate applies to the child's job (rule 2) and the caller's job
  (rule 3), with the `caller_stalled` gauge and a V3 case. Closed.
- **F4, fixed.** Exact symbols are named; `ChannelCallDispatch`, `ChannelCallToolCall` and the
  snapshot builders stay out; V11 has a negative check. Closed.
- **F5, fixed.** The responder in call items and sample capture are both gated on the
  `channel_reply_settings` authority, and the report prints no responder (V2, V8). Closed.
- **F6, fixed in substance.** The result-field sub-schema is frozen and fixtures are validated
  against it (V7). Its undeclared case does not match live, recorded as new finding F9. Closed
  otherwise.
- **F7, taken.** The audit-pair assertion is in V2, and the members-only docs note is added.
  Closed.

## AWC-P05-D2-F8 — Rule 3 and `response_ready` treat a consumed-as-`revoked` call as still waiting for the caller

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); phase AWC-P05, batch B2, stage gate recheck; round [AWC-P05-D2-R2-N1](agent-review-notes.md#awc-p05-d2-r2-n1); reviewer B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD `fe7791d3`; decision [AWC-P05-D2 r2](logs/AWC-P05-D2-r2-decision.md), sha256 `4fd2383d0214bc9f4c616c8b7616ecde3ff14d24f1fc4abd4f53701bb40494ae`; pre-gate digest `22fdcacd…ba13`. Originates in the decision.
- **Classification:** actionable; low; correctness of a view and its gauges. It does not block the
  gate. Fix in the batch.
- **Evidence.**
  - A `revoked` consumption stores no result row (p04 D7), but the caller's agent continues in the
    *same* execution: it reads the observation, takes further turns, and may park on other tools.
  - Until that execution completes, rule 3 gives `AwaitingCaller`, and `response_ready` ("child
    succeeded and no result is stored, wait open or completed") counts the call, as does
    `oldest_seconds`.
  - So a call the caller has already answered appears to be waiting for it, possibly for a long
    time.
  - A durable discriminator exists: the agent exchange checkpoint records each tool invocation's
    state (`checkpoint->'invocations'` with `state='completed'` and `call->>'tool_id'`, read by
    `guard_harness_knowledge_observations`, schema ~12408-12415).
  - The checkpoint is also the right one to use, because a revoked observation is re-judged on
    replay until the turn is checkpointed.
- **Required (in the batch).** In rule 3 and in the `response_ready` and `caller_stalled` gauges,
  treat a call whose invocation the caller's agent checkpoint records as completed as rule 4,
  "observed without a stored result". Add a V1 row and a V3 or V9 case: revoked, then the agent
  continues in the same step.
- **Resolution history.** 2026-11-23 open; deferred by B' to batch B2.

## AWC-P05-D2-F9 — "Without a declared sub-schema, `fields` must be absent, as live" does not describe live behaviour

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); phase AWC-P05, batch B2, stage gate recheck; round [AWC-P05-D2-R2-N1](agent-review-notes.md#awc-p05-d2-r2-n1); reviewer B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD `fe7791d3`; decision [AWC-P05-D2 r2](logs/AWC-P05-D2-r2-decision.md), sha256 `4fd2383d0214bc9f4c616c8b7616ecde3ff14d24f1fc4abd4f53701bb40494ae`; pre-gate digest `22fdcacd…ba13`. Originates in the decision.
- **Classification:** actionable; low; requirements (samples and live describe the same
  semantics). Fix in the batch.
- **Evidence.**
  - Live `done`: `channel_reply_completion::project_on` returns `output.fields` whenever it is an
    object, gated only by the workflow output schema. An output schema that does not declare
    `fields` but allows extra properties lets fields through.
  - Live `channel.deny`: `registry/families.rs::result_fields` defaults to `{"type":"object"}`
    when undeclared, so any fields object passes.
  - r2's sample rule is stricter than live in both cases.
- **Required (in the batch).** Freeze and apply the same effective rule live uses, for example the
  output schema's `properties.fields` if present, and otherwise what the whole output schema
  admits for `/fields`. Alternatively, state the sample's stricter rule as a deliberate deviation,
  with a test. Add a V7 row either way.
- **Resolution history.** 2026-11-23 open; deferred by B' to batch B2.

### AWC-P05-D2-F8, F9 — batch dispositions (AWC-P05-B2-R1-N1, digest `24b290b2…9da4`)

- **F8, fixed.**
  - `caller_consumed` reads the caller execution's harness checkpoint, matching an invocation
    with `state='completed'` and `call->>'invocation_id' = call_id`. It is used in rule 3
    (`call_phase`) and in the `caller_side` filter that `response_ready`, `caller_stalled` and
    the age gauge share.
  - V3 covers a revoked consumption followed by a second call in the same step, and its recorded
    mutation check fails without the predicate. B's run passes it.
  - Closed.
- **F9, resolved by deviation 1.** With no declared result sub-schema, any `fields` object is
  accepted. That matches `channel.deny`'s default and `done`'s object check.
  - It is looser than live in one case: a responder whose whole output schema forbids `fields`.
    It is recorded and documented ("or any object when it declares none"), and accepted.
  - A declared sub-schema is enforced (V7).
  - Closed.

## AWC-P05-B2-F1 — The final-review link is gated on reading the child, not on opening the decision

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); phase AWC-P05, batch B2, stage batch review; round [AWC-P05-B2-R1-N1](agent-review-notes.md#awc-p05-b2-r1-n1); reviewer B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD `fe7791d3` (uncommitted); artifact digest `24b290b23293e66b2e534d4dfbd2546ffd6f9fa180917d8859313890f48e9da4`. Originates in the batch.
- **Classification:** actionable; low; requirements (gate D1: "The decision link is drawn only if
  the viewer may open that decision"). Close it with a fix or a recorded deviation.
- **Evidence.**
  - `pages/workflow_runs/calls.rs:30-37` draws the review link when
    `access.runs.contains(child)`. Its comment reads "The decision page reauthorizes; it is
    linked only beside an openable child".
  - Run inspection and decision access are different authorities: reviewers versus run
    inspectors. A viewer who can read the child but is not allowed to open the decision gets a
    link that fails.
  - The decision id appears in the link, so a viewer who may not open the decision still learns
    it. That costs little.
  - TASK's deviations do not mention the change.
- **Required.** Gate the link on the decision-read authority, with one batch read like the run
  set, or record the deviation and its reason in TASK and in the doc's "linked to the decision"
  wording.
- **Resolution history.** 2026-11-23 open.

## AWC-P05-B2-F2 — Optional notes

- **Context:** 2026-11-23; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); phase AWC-P05, batch B2, stage batch review; round [AWC-P05-B2-R1-N1](agent-review-notes.md#awc-p05-b2-r1-n1); reviewer B', Opus 5.5 (effort as assigned: medium), the Opus substitute reviewer (U7); worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`; HEAD `fe7791d3` (uncommitted); artifact digest `24b290b23293e66b2e534d4dfbd2546ffd6f9fa180917d8859313890f48e9da4`. Originates in the batch.
- **Classification:** optional; low.
- **Evidence and suggestions.**
  - `metrics.sql` `call_facts` drives from `workflow_executions WHERE completed_at IS NULL`.
    The schema has no index on `workflow_executions` or `workflow_channel_calls` that serves
    that filter, so each sample scans executions in full. That matches the existing time-filtered
    `steps` CTE (line 21), so it is no worse than today. But "bounded by open work" holds for the
    rows returned, not for the rows scanned. A partial index, or driving from `workflow_waits`
    and the calls, would make it true.
  - Deviations 2, 3, 4, 6 and 7 are accepted:
    - 2: `parse`, as live;
    - 3: the unused fixture stops, a sensible strictness;
    - 4: the added symbols are pure, and the boundary script passes in B's run;
    - 6: the stand-ins are limited by the root-bound admin fixture and recorded as such. The render
      unit together with `call_access` reading Forbidden/NotFound as hidden is adequate;
    - 7: `is_visible()` is used in both readers, and V4 proves it.
  - `desk_state_on` keeps the desk `FOR SHARE` / unit `FOR KEY SHARE` first, then the principal
    and grants `FOR SHARE`. `settings_on` and `read_binding_at` take no row locks, so the lock
    order is unchanged.
  - `run_page_for_tests` is `#[cfg(test)]` at both re-exports and at its definition
    (`runs/mod.rs:909`).
  - CI and `check.sh`: the render test writes `WORKFLOW_CALLS_BROWSER_FIXTURES`, and the
    browser file is added to the native-markup gate and to its own CI step, following the
    decision-forms pattern.
- **Resolution history.** 2026-11-23 recorded.
