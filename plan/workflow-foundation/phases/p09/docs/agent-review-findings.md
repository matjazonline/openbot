# P09 agent review findings

> Review artifact (excluded from the frozen snapshot). Owned by reviewer B; entries are appended,
> never deleted. Common context for every finding below unless stated: workflow
> `implement-opus-reviewed-2_2`; phase `p09`; worktree `/Users/mac02/dev/openbot` (branch
> `workflows`, HEAD `da86871`); batch B1a (library, editor, catalogue, versions, publish); stage
> batch review; review round `WF-P09-R001` (`agent-review-notes.md`); reviewer B Opus 5.5 / xhigh;
> reviewed artifact digest `62a6c35e187548f9e6fbd444c9482f4b3e378764cef3dd2dc0a19f561cf84e50`;
> task `TASK.md`, review `REVIEW.md`. Line numbers refer to that artifact. None originates in a
> gated decision (B1a had no gate).

## WF-P09-F001 — A parameter-selected agent's frozen tools and skills are never shown

- Date 2026-10-24. **Actionable defect**, severity **Medium**, category requirements.
- Evidence: `src/adapters/http/pages/workflow_authoring/preview.rs:58-64` — for an `agent.run` /
  `decision.agent` / `ai.classify` step whose `agent` input is not a literal, the row says "the
  agent is chosen by a parameter at run time, from the agents this version freezes below." and
  returns; nothing below (or anywhere in `workflow_preview`) iterates
  `bundle.snapshots().agents`, so the frozen candidate agents, their models, tools and skills are
  not displayed. 06-D5 explicitly supports this selector (`publication/capabilities.rs:90-130`
  `agent_universe`: `/params/<name>` with a finite string enum), so it is a valid, publishable
  draft. Required (plan 09 "Display the resolved tools/skills … in validation previews", TASK
  criterion 8 per 06-D5): the preview shows "Uses the agent's tools and skills" with the frozen
  list for every model step. Observed: only literal-agent steps get that; the parameter case
  promises a list that does not exist. Also `.find(...)?` at :65-70 silently drops a literal step
  whose key has no snapshot (should be unreachable after publication validation, but it hides
  rather than reports).
- Reproduction (confirmed): probe `probe_b_parameter_selected_agent_preview`
  (`logs/r001-b-probes.log`): two agents `assistant`/`helper`, `agent: {ref: /params/agent}`
  with an enum parameter → "Revision 2 is valid", and the whole "Agents and capabilities"
  section is the single row "start · the agent is chosen by a parameter at run time, from the
  agents this version freezes below."; neither candidate nor any `tools:` line appears.
- Fix direction: list every frozen agent snapshot (name, handle, model, tools, skills, source
  "the agent, frozen at publication") once, and have the parameter rows point at it; add a test.
- Instruction lesson: hypothesis — the batch's capability test covered only the literal case;
  candidate rule for TASK templates: "for each supported selector form in a contract (06-D5
  literal and `/params` enum), test the UI rendering of each".
- Resolution history: 2026-10-24 open.
  - 2026-10-24 R002 recheck (`WF-P09-R002`, digest `b4dc745de5c69e461189ae0c64ad0d38ec3de67e30eeacc7550c9675e9e2de3c`): **fixed**. `preview.rs` `frozen_agents` lists every frozen agent once (name, handle, model, tools, skills, source) and `model_steps` says which agent each step runs; a literal missing from the frozen set is an error row. Test `a_parameter_selected_agent_step_previews_every_frozen_candidate` passes (`logs/r002-b-focused.log`). A residual imprecision of the new text is logged separately as WF-P09-F016.

## WF-P09-F002 — Publication options are cut company-wide at 200, so a needed tool can be missing

- Date 2026-10-24. **Actionable defect**, severity **Medium**, category correctness / bounds.
- Evidence: `src/adapters/persistence/workflow/authoring_reads.rs:148-184` reads every approved
  MCP and HTTP operation of the company, ordered by name, `LIMIT 201`, regardless of what the
  draft needs; `routes/workflows/ui/authoring.rs:255-271` calls it whenever the draft has any
  need. When the company has more than 200 approved operations of a kind, needed tools whose
  names sort after the cut are absent: for an MCP need, `editor.rs:360-364` then states falsely
  "no company MCP connection has this tool approved. Approve it on an MCP server first." (the
  generic "more than this list shows (200)" note appears beside it); for an HTTP need,
  `editor.rs:383-386` silently `continue`s as if it were a workflow or built-in tool, leaving the
  author no way to select the connection, so validation/publication can only fail. Each editor
  render also loads up to 402 snapshots of up to 64 KiB (schema check) whatever the needs.
  Required: "MCP steps: select a company MCP connection for the declared resource slot" (plan 09);
  `src/AGENTS.md` "Bound work …" — the bound should not change correctness.
- Reproduction (confirmed): probe `probe_b_publication_options_past_the_200_cut`
  (`logs/r001-b-probes.log`): one connection approving `a000`…`a200` and `lookup`, draft =
  `mcp.call` example → no `mcp:service:lookup` select, the "no company MCP connection has this
  tool approved" text, and the cut note.
- Fix direction: read options for the needed names only (`name = ANY($2)`, needs are bounded by
  the draft's steps), keep a per-name bound, and say "list cut" rather than "not approved" when a
  bound truncates; test with >200 operations.
- Instruction lesson: cause unknown; candidate check "a read bounded for display must be filtered
  by what the caller needs before the bound is applied".
- Resolution history: 2026-10-24 open.
  - 2026-10-24 R002 recheck: **fixed**. `publication_options(company, mcp_tools, http_tools)` reads only the needed names (`name = ANY($2)`), ranks per name with `row_number()` and cuts at 20 per name with a per-name note; table/label fragments in `options_sql` are static literals. Test `the_editor_offers_a_needed_mcp_tool_past_any_company_wide_count_and_cuts_per_tool` covers 202 approvals and 21 connections. The new `MAX_OPTION_NAMES` refusal introduced a new defect, logged as WF-P09-F013.

## WF-P09-F003 — Publish, tabs, sidebar, New and Archive discard unsaved editor text silently

- Date 2026-10-24. **Actionable defect**, severity **Medium**, category requirements (unsaved
  source).
- Evidence: the preview's publish form (`pages/workflow_authoring/preview.rs:197-208`) is a
  separate form that replaces `#workflow-pane` and sends none of the editor fields. If the author
  edits the source after validating and then publishes, the edits are lost either way: success
  swaps in the versions tab (`ui/authoring.rs:764-776`); a refusal (capture `NotFound`, a
  `Diagnostic` at publication, a conflict) re-renders the editor from the *stored* draft because
  `state.typed` is `None` (`ui/authoring.rs:778-809`). A refused publication is a failed
  validation, so plan 09 "preserve unsaved source when validation fails" is not met on that path.
  The same pane replacement happens without a prompt from the tab links
  (`pages/workflow_authoring/mod.rs:236-247`), sidebar entries (`mod.rs:86-95`), New
  (`mod.rs:51-54`) and Archive (`editor.rs:172-176`, whose confirm is about archiving, not the
  edits); only "Reload saved draft" asks (`editor.rs:146-149`), so the hazard was recognised but
  covered for one control.
- Impact: typed YAML vanishes without warning in the main validate → adjust → publish flow.
- Fix direction (one of): keep the editor dirty state client-side and confirm (or block publish)
  before any pane-replacing request while dirty; or include the editor's fields in the publish
  request and, on refusal, re-render them as `typed`. Test the refusal path keeps typed text.
- Instruction lesson: hypothesis — "preserve unsaved source" was implemented for the save form
  only; candidate rule for `pages/AGENTS.md`: "a pane that holds an editable form treats every
  control that replaces it as a navigation away from unsaved input".
- Resolution history: 2026-10-24 open.
  - 2026-10-24 R002 recheck: **partly fixed; remains open, severity lowered to Low.** Text typed in the browser is now guarded (`data-dirty` on input/change; a confirm before any request that replaces `#workflow-pane`; `beforeunload`). Not covered: text the *server* echoes back unsaved. After a refused save (`BadRequest`), a save conflict, or `MovedAfterSave`, the editor re-renders the author's unsaved text in a form with no `data-dirty`. A tab, sidebar entry, New, publish or archive click then discards it without asking, which is the path where the banner has just said "nothing of yours was saved". Probe `probe_b2_conflict_echo_is_not_marked_dirty` (`logs/r002-b-probes.log`): conflict banner and unsaved text shown, form tag carries no `data-dirty`. Fix direction: render `data-dirty` on the form whenever the editor shows `typed` text that differs from the stored draft, and test it. The guard also introduced WF-P09-F014.
  - 2026-10-24 R003 recheck (`WF-P09-R003`, digest `1484a5bc494ed3c0202e07678f66566fabe31f0f24ea0e5886620254cd6957db`): **fixed.** `editor_body` computes `unsaved` when the shown `typed` title, description or source differs from the stored draft, and the form then renders `data-workflow-editor data-dirty`. This covers a refused save, a conflict and `MovedAfterSave`. A stored draft and a successful save render clean (asserted in `a_concurrent_save_keeps_the_authors_text_and_shows_what_the_other_saved` and `the_editor_guards_unsaved_text_and_reports_only_the_work_it_does`), which matches B's R002 probe case. Accepting the prompt no longer clears the mark, so a request that is then cancelled (Archive's own confirm) or fails keeps the text guarded. The click behaviour remains in F012.

## WF-P09-F004 — Progress feedback that misreports the work or never clears

- Date 2026-10-24. **Actionable defect**, severity **Low**, category UI / progress feedback
  (`pages/AGENTS.md` "Every user action needs visible progress feedback").
- Evidence: (a) `editor.rs:118-145` — the `[.htmx-request_&]` variants are keyed on the form's
  `htmx-request` class, so clicking **Save draft** (intent `save`) shows "Validating…" and a
  spinner on the *Save and validate* button, while `BUTTON_BUSY_SCRIPT` adds a second spinner to
  the pressed Save draft button (it owns no spinner): the status names work that is not
  happening. (b) `catalogue.rs:9-16` — `#workflow-reference-body` carries `aria-busy="true"`,
  which an `innerHTML` swap never removes, so the loaded reference stays announced as busy; on a
  failed load the "Loading the reference…" spinner stays indefinitely (only the toast reports
  it), against "on an in-place failure … clear the busy state".
- Fix direction: give each submit button its own indicator (e.g. per-button `hx-indicator` or a
  submitter-scoped class) with "Saving…" for save; swap the reference body with `outerHTML` (or
  clear `aria-busy` on `htmx:afterRequest`) and render an error state on failure.
- Instruction lesson: candidate note for `pages/AGENTS.md`: "`[.htmx-request_&]` is form-scoped;
  a form with two submit buttons needs submitter-scoped feedback".
- Resolution history: 2026-10-24 open.
  - 2026-10-24 R002 recheck: **fixed.** The editor's submit buttons own no form-scoped spinner (the shared busy script marks the pressed one), and a status region names the work by submitter. The reference body is swapped `outerHTML`; a failed load clears `aria-busy`, says so and re-arms the toggle. The test asserts the markup and the script hooks; the behaviour itself is part of the browser pass (F012). Note: "Not saved: the request failed" is also shown when the save committed but the response failed (see F013).

## WF-P09-F005 — A source or description starting with a newline loses it in the textarea

- Date 2026-10-24. **Actionable defect**, severity **Low**, category correctness (unsaved source).
- Evidence: `editor.rs:129` and `editor.rs:133-135` render `<textarea …>{escaped}</textarea>`
  with the content directly after the start tag. The HTML parser drops a single LF immediately
  after `<textarea>`, so a stored source beginning with a blank line shows without it, the next
  save silently changes the source (and its content hash), and the diagnostic's "Go to line N"
  (`editor.rs:202-230`, numbered on the stored source) selects the line below the one reported
  (`WORKFLOW_EDITOR_SCRIPT` counts `field.value` lines). Sources saved through the JSON API may
  start with a newline; shipped templates do not. Probe `probe_b_leading_newline_markup`
  (`logs/r001-b-probes.log`) confirms a stored leading LF is emitted directly after the start
  tag (so the parser consumes it).
- Fix direction: emit a newline right after each `<textarea …>` start tag (the parser consumes
  it), and test a source that starts with `\n` round-trips through the form.
- Instruction lesson: candidate for `pages/AGENTS.md` escaping section: "a `<textarea>`/`<pre>`
  value is preceded by a newline in the markup".
- Resolution history: 2026-10-24 open.
  - 2026-10-24 R002 recheck: **fixed.** A newline follows both `<textarea>` start tags. Test `a_source_and_description_that_start_with_a_newline_keep_it` passes.

## WF-P09-F006 — Cross-tenant id tests miss the own-company path for writes and compare's version

- Date 2026-10-24. **Actionable defect** (verification), severity **Low**, category verification
  / security tests (`src/adapters/http/AGENTS.md` "Tests must attempt a valid id belonging to
  another tenant for every new detail, nested, download, and mutation route"; TASK criterion 4).
- Evidence: `ui/authoring_tests.rs:621-738` sends the stranger and the plain member through the
  victim company's `company_id`, which `load_managed_company` stops before any read. The more
  realistic attack — an admin of their own company passing *their* `company_id` with the victim's
  `workflow_id` — is tested only for `pane` (:720-731), not for `save`, `publish`, `archive` or
  `compare`; `compare` is never asked for a version id of another workflow or another company
  (`from`/`to` are nested ids). By inspection all of these are safe today (company-scoped
  `draft`/`definition`/`version_source`; a foreign `save` is `Conflict` then `NotFound`), so this
  is missing evidence, not an exploitable defect.
- Fix direction: add the own-company + foreign-id cases for every write route and a foreign
  version id for compare, asserting `NotFound` and no row change.
- Instruction lesson: the AGENTS rule exists; candidate clarification "'another tenant's id'
  means the attacker's own valid scope plus the victim's object id, not only the victim's scope".
- Resolution history: 2026-10-24 open.
  - 2026-10-24 R002 recheck: **fixed.** Test `an_admin_of_another_company_reaches_no_write_or_version_of_this_one` covers save, publish, archive and compare with the attacker's own company and the victim's workflow (all `NotFound`, victim row unchanged), plus another workflow's version through compare. Another *company's* version is reached through the same `(company, workflow, id)` predicate, so the cross-workflow case exercises it.

## WF-P09-F007 — The MCP authoring path is not proven through the UI

- Date 2026-10-24. **Actionable defect** (verification), severity **Low**, category verification.
- Evidence: `ui/authoring_tests.rs:865-936` validates with an MCP selection but asserts only
  `"Saved revision 2"`, which holds whether the validation passed or was refused (DV-P09-6). No
  test shows that a valid preview's publish form carries the MCP selection and publishes a version
  freezing that tool, nor that a wrong argument binding is reported as a located diagnostic (plan
  09 "inspect the selected tool's frozen schema, and validate argument bindings").
- Observation (probe `probe_b_mcp_validate_and_publish_through_the_ui`,
  `logs/r001-b-probes.log`): with the test's own fixture shape (operation and grant rows inserted
  directly, no reviewed discovery) the validation is **refused** — "Saved revision 2, but it
  could not be validated: MCP grants must uniquely name reviewed discovered tools" — so the
  shipped test passes on a refusal while reading as the MCP happy path. With a real connection
  (created through `create_mcp_connection` with a reviewed discovered tool, operation through
  `register_mcp_operation`, as p08's `mcp_test_support` does) the UI path works: the only
  candidate is preselected, no endpoint text leaks, the preview is valid and lists the tool
  "through slot service", its publish form carries `mcp:service:lookup`, publishing succeeds,
  and a wrong argument name yields the located diagnostic. So the behaviour is right; the
  project's evidence for it is missing.
- Fix direction: assert the valid outcome, the hidden `mcp:<slot>:<tool>` field in the publish
  form, a successful publish whose bundle freezes the tool through the slot, and a bad-argument
  diagnostic.
- Instruction lesson: candidate rule "a test that accepts several outcomes must name which one
  the fixture produces".
- Resolution history: 2026-10-24 open.
  - 2026-10-24 R002 recheck: **fixed.** `an_mcp_step_validates_publishes_through_the_selected_slot_and_locates_bad_arguments` uses a reviewed connection and asserts preselection, no endpoint leak, a valid preview naming slot and connection, the publish form's hidden selection, the published bundle freezing `lookup` through `service`, and a located bad-argument diagnostic. This matches B's R001 probe.

## WF-P09-F008 — Two refusals reach the author as the wrong message

- Date 2026-10-24. **Actionable defect**, severity **Low**, category correctness / feedback.
- Evidence: (a) `ui/authoring.rs:693-735` — after a successful save, `validate_draft` can answer
  `Conflict` when another save lands between the two calls; `refusal()` (:166-186) does not map
  `Conflict`, so the request fails with a 409 toast although revision N was saved, contrary to
  DV-P09-6 ("a refused validation after a successful save says both"), and the form keeps the old
  `expected_revision`. (b) `ui/authoring.rs:778-800` maps every publish `Conflict` to "Revision
  {expected} is no longer the draft (now revision {current}) …", dropping the command's message;
  for a conflict that is not a draft move — e.g. `agent_publication::verify_on` "Agent X changed
  or does not match its saved definition" (`persistence/workflow/agent_publication.rs:152-170`)
  — it reads "Revision 2 is no longer the draft (now revision 2)".
- Fix direction: map validate `Conflict` to a "saved, but the draft moved before validation"
  banner with the current revision; in publish, say "no longer the draft" only when the current
  revision differs, otherwise show the command's conflict message.
- Instruction lesson: cause unknown.
- Resolution history: 2026-10-24 open.
  - 2026-10-24 R002 recheck: **fixed.** (a) a `Conflict` from validation after the save becomes `MovedAfterSave` and keeps the typed text against the newer revision (test `a_draft_that_moves_after_the_save_keeps_the_text_and_says_both`); (b) a publish `Conflict` says "no longer the draft" only when the revision moved, says archived when archived, and otherwise shows the command's message (test `a_publish_conflict_that_is_not_a_moved_draft_says_what_the_command_said`). Residual, not filed: an archive racing between save and validation would read "someone else saved revision N" with N unchanged; it is a narrow race.

## WF-P09-F009 — The sidebar keeps the old title and revision after a save

- Date 2026-10-24. **Actionable defect**, severity **Low**, category UI consistency.
- Evidence: `ui/authoring.rs:220-232` documents `menu_oob` as "after a write that adds or renames
  one", and create/copy/archive use it, but `save` (:614-690) returns only the pane, so a renamed
  title and the "draft revision N" line in the sidebar (`mod.rs:93-94`) stay stale until a
  reload, while the pane header shows the new ones.
- Fix direction: append `menu_oob` to the save response (it already exists).
- Resolution history: 2026-10-24 open.
  - 2026-10-24 R002 recheck: **fixed.** `save` appends `menu_oob`. Test `a_save_refreshes_the_sidebar_and_a_changed_publication_is_flagged` passes.

## WF-P09-F010 — Optional: tell the author when the published version differs from the preview

- Date 2026-10-24. **Optional suggestion**, severity Low, category UX / integrity.
- Evidence: publication re-captures agent, skill and tool snapshots (`definitions::publish_draft`
  → `DefinitionService::publish`), so a version can freeze different tools/skills than the
  preview showed if an agent or approval changed in between; the publish banner
  (`ui/authoring.rs:766-769`) names the version and draft revision but not the content hash.
- Suggestion: carry the previewed `content_hash` as a hidden field and, when
  `bundle.content_hash()` differs, say so in the banner (UI-only; no command change). Related:
  the preview's frozen-tools list (`preview.rs:114-147`) says "through slot service" but not
  which connection the hidden selection names, while the editor's select above may since have
  been changed to another one; naming the selected connection label in the preview would make
  "publishes exactly these selections" visible.
- Resolution history: 2026-10-24 open (optional).
  - 2026-10-24 R002 recheck: **adopted and verified.** The preview names each selected connection ("Connections this publish freezes") and carries `previewed_dependencies` = `dependency_digest(bundle)`, a sha256 over the frozen snapshots and child version ids. The content hash is unusable here because it covers the version identity. Serialization is deterministic: the snapshots are Vecs, serde_json maps are BTreeMap (no `preserve_order` feature), and children come from a BTreeMap. Covered by the MCP test (no false flag) and the forced-mismatch test.

## WF-P09-F011 — Optional: newtypes and named rows for the new selection and read shapes

- Date 2026-10-24. **Optional suggestion**, severity Low, category style (`src/AGENTS.md`
  "Newtypes over bare String", "Name your tuples").
- Evidence: `WorkflowSelections.mcp: BTreeMap<(String, String), Uuid>` (`editor.rs:13-15`) keys
  slot and tool as two bare strings (a swap compiles; `ResourceName`/`TypeName` exist);
  `OperationRow = (Uuid, String, bool, String, Value, bool)` (`authoring_reads.rs:11`) depends on
  the SELECT order of two strings and two booleans; also the 6-tuple in `version_source`.
- Suggestion: a small `McpSelectionKey { slot, tool }` and `#[derive(sqlx::FromRow)]` row
  structs.
- Resolution history: 2026-10-24 open (optional).
  - 2026-10-24 R002 recheck: **adopted and verified.** `McpSelectionKey { slot: ResourceName, tool: TypeName }`, `http: BTreeMap<TypeName, Uuid>` (invalid names select nothing, tested), `OperationRow`/`VersionSourceRow` as `sqlx::FromRow` structs.

## WF-P09-F012 — Verification gap: browser, both themes, keyboard and `check:css` not run

- Date 2026-10-24. **Verification gap** (tracked; not a B1a code defect), severity Low, category
  verification (TASK criterion 6; `pages/AGENTS.md` "Verify a styling change in both themes, on
  the computed value").
- Evidence: TASK "B1a implementation log" — "Not run here: browser verification in both themes
  and keyboard walk-through (planned once B1b lands)"; `npm run check:css` unavailable (no
  `node_modules`); the class-scan stand-in (`logs/css-class-check.py`, rerun by B: 0 missing)
  does not check computed styles, SVG token rendering, or keyboard use of tabs/disclosures.
- Disposition needed: keep open until the planned browser pass records both themes and a
  keyboard walk-through of the B1a pages; it must not be reported as verified before then.
- Resolution history: 2026-10-24 open (deferred by plan to after B1b).
  - 2026-10-24 R002 recheck: still open by plan (not claimed). It now also covers the new client behaviour of F003/F004 (dirty guard, status text, reference failure state), which only a browser pass can show.
  - 2026-10-24 R003 recheck: still open by plan. B's view on acceptance: **not blocking B1a's acceptance** for its server-verified scope. It **does block phase acceptance** (criterion 6). Any B1a acceptance record must say that the client behaviour (dirty guard, status text, reference failure state, the `htmx:confirm` ordering of F014) is verified by code and htmx-source reading only. B1b edits the same pages, so the planned pass after B1b covers both.
  - 2026-10-24 R005 (digest `293c80357296a898b19f1e83b6eed47cb45f5dc74fcc83d00840aab418878555`): **largely closed; residual open (Low, not blocking B1b).**
    - Evidence checked: `logs/f012/` (`pass.js`, `pass-run.log` 54/54, `results.json`, 14
      screenshots). It drove the installed Chrome 155 headless via playwright-core 1.55.1
      against the app on localhost:3001.
    - Closed by real-browser evidence:
      - both themes via the rail toggle, with each mode confirmed (`data-theme` and the body
        background);
      - computed-colour contrast of every visible text node in the p09 views, in light and
        dark, at a 3:1 floor (O3 notes the tabs and secondary text under 4.5:1 are daisyUI
        defaults);
      - the graph's node fill and stroke changing with the theme;
      - F003 dirty marking on typing; F014 cancel from Tools/New (prompt shown, button left
        usable);
      - F004 status while validating, and a failed save that keeps the text and clears busy;
      - the reference panel failing and retrying; the lazy-card fallback;
      - the end-to-end template copy → publish → bind → activate → tool → grant → used-by path.
    - `check:css`: B independently rebuilt the CSS in isolation (scratch: current tree and the
      reconstructed pre-p09 tree, both from `assets/app.css.in` with the project's
      `node_modules`, nothing written to the worktree). p09 adds `max-h-96`, `ml-2`, `ml-4` and
      `pl-3`, all already in the tracked `app.css`, and drops none. The tracked file is stale by
      96 classes before p09 (p08 removals), plus `start-1`/`start-2`, which are Tailwind false
      positives from Rust strings. DV-P09-8 holds: a pre-existing, unrelated `check:css`
      failure, owed to the user or CSS owner.
    - **Residual:** keyboard coverage is thin.
      - Checked: Shift+Tab across the tab strip, a visible focus outline in dark, and Enter on a
        tab.
      - Not checked: keyboard use of the editor's submit buttons and "Go to line", the binding
        form, the disclosures, the confirm dialogs; focus visibility in light.
      - Carry this into the phase's final browser pass (criterion 6 covers B2–B4 pages too).

- Resolution update 2026-10-25 / R017 at `c09931634e15711cb7a01c585dd8dc0409b7d89c3ecc7556c96888c34e0bc7c4`: **F012 residual fixed**. Inspected scripts/tests/workflow-authoring-browser.cjs and logs/b4/p09-b4-browser-final2.log plus result.json: keyboard editor submit/Go-to-line/disclosure/dirty confirmation in both themes, keyboard binding save/activate, sample focus traversal and computed theme/focus styling. Combined with retained B2/B3 browser evidence and earlier F012 coverage, the named keyboard residual is closed. This does not waive final full-suite/integration checks or the separately recorded pre-existing CSS drift DV-P09-8. Browser journey was author-executed; reviewer inspected script/results rather than rerunning a stopped dev server.

## WF-P09-F013 — A draft naming more than 256 tools of one kind makes the editor unrenderable

- Date 2026-10-24; round `WF-P09-R002` (correction recheck); digest
  `b4dc745de5c69e461189ae0c64ad0d38ec3de67e30eeacc7550c9675e9e2de3c`. **Actionable defect**
  (introduced by the F002 correction), severity **Low**, category correctness.
- Evidence: `authoring_reads.rs` `publication_options` returns `BadRequest("A draft names at
  most 256 tools of each kind")` when `mcp_tools` or `http_tools` exceeds `MAX_OPTION_NAMES`;
  `ui/authoring.rs` `editor_body` propagates it with `?`. `publication_needs` parses without a
  step-count bound (`compiler/wire.rs` `parse_workflow`; `MAX_GRAPH_NODES` = 1024 is enforced
  later by `validate`), so such a draft is accepted by save. Probe `probe_b2_many_literal_tools_and_the_editor`
  (`logs/r002-b-probes.log`): 257 `tool.call` steps with distinct literal tools → needs parse
  (257), the editor GET fails 400, and a save **commits revision 2** yet its response is a 400.
  The client then says "Not saved: the request failed" (F004 status) although it was saved, and
  the editor cannot be reopened until the source is fixed through the JSON API.
- Impact: narrow (publication refuses more than 256 selections anyway), but the editor must stay
  usable to fix such a draft, and the status contradicts the stored state.
- Fix direction: do not fail the page. Read options for at most `MAX_OPTION_NAMES` names and
  render the rest as "too many tools to offer connections for; at most 256", or skip the
  options read and show that note. Add a test.
- Instruction lesson: candidate check "a bound added to a read used while rendering an editor
  degrades the view; it does not fail the request".
- Resolution history: 2026-10-24 open.
  - 2026-10-24 R003 recheck (digest `1484a5bc494ed3c0202e07678f66566fabe31f0f24ea0e5886620254cd6957db`): **fixed.** `Ui::editor_options` dedupes each kind's names, reads options for at most `MAX_OPTION_NAMES`, returns a `too_many_tools` flag, and the editor shows a note instead of failing. Test `a_draft_naming_more_tools_than_are_offered_still_saves_and_renders` (257 literal `tool.call` tools: the pane renders with the note; a save answers "Saved revision 2.") passes (`logs/r003-b-focused.log`); equivalent to B's R002 probe.

## WF-P09-F014 — Cancelling the unsaved-text prompt from New leaves the button disabled and spinning

- Date 2026-10-24; round `WF-P09-R002`; digest `b4dc745d…de3c` (full above). **Actionable defect**
  (introduced by the F003 correction), severity **Low**, category UI / progress feedback.
- Evidence: `WORKFLOW_EDITOR_SCRIPT` cancels with `event.preventDefault()` in
  `htmx:beforeRequest`. `application_javascript` (`pages/mailbox.rs:820`) registers
  `BUTTON_BUSY_SCRIPT` before it on `document.body`, so the busy handler runs first. For a
  pressed button without its own spinner it prepends a spinner, sets `aria-busy`, disables the
  button and records it, and it cleans up only in `htmx:afterRequest`. htmx 2.0.4
  (`assets/htmx-2.0.4.min.js`) returns from a cancelled `beforeRequest` without firing
  `afterRequest` (`if(!he(r,"htmx:beforeRequest",H)){oe(s);m();return e}`). The sidebar's
  **New** button (`pages/workflow_authoring/mod.rs:51-54`, icon + text, no `.loading`)
  targets `#workflow-pane`. With a dirty editor, choosing Cancel at "Discard them?" leaves New
  disabled, spinning and `aria-busy` until a page reload. Tabs and sidebar entries are links
  (not handled by the busy script); publish and archive own spinners. Established by reading
  the code; no browser run.
- Fix direction: run the guard before the busy script, e.g. a capture-phase listener on
  `document` or the `htmx:confirm` event, which precedes `beforeRequest`. Alternatively, have
  the busy script clean up when `beforeRequest` is cancelled. Cover it in the F012 browser pass.
- Instruction lesson: candidate for `pages/AGENTS.md`: "a handler that cancels
  `htmx:beforeRequest` must run before the shared busy-button handler, which only cleans up
  on `htmx:afterRequest`".
- Resolution history: 2026-10-24 open.
  - 2026-10-24 R003 recheck (digest `1484a5bc494ed3c0202e07678f66566fabe31f0f24ea0e5886620254cd6957db`): **fixed.** The guard moved to `htmx:confirm`. In `assets/htmx-2.0.4.min.js`, `htmx:confirm` is fired with `{target, elt, …}` and, when cancelled, returns (`if(he(r,"htmx:confirm",G)===false){oe(s);return e}`) before the `hx-sync` handling, the `hx-confirm` prompt and `htmx:beforeRequest`. So the busy-button handler never sees a cancelled request, and the `detail.target.id` check still works. Event order is: guard prompt, then the element's own `hx-confirm`, then `beforeRequest`. The editor's own form still sets its status in `beforeRequest`. Asserted by the script test; clicks remain in F012.

## WF-P09-F015 — Optional: split the growing route and test files before B1b

- Date 2026-10-24; round `WF-P09-R002`. **Optional suggestion**, severity Low, category style
  (`src/AGENTS.md` "Split a function along the phases it already has", "Keep the file and its
  test module navigable").
- Evidence: `ui/authoring.rs` is 987 lines and `editor_body` about 95 (past the ~80 trigger;
  the options gathering is a separable phase). `ui/authoring_tests.rs` is 1573 lines. B1b will
  add bindings and agents to the same module.
- Suggestion: extract the needs-and-options step of `editor_body` into a helper. Split the
  workspace into `ui/authoring/{library,editor,versions}.rs` and the tests by area when B1b
  lands.
- Resolution history: 2026-10-24 open (optional).
  - 2026-10-24 R003 recheck (digest `1484a5bc494ed3c0202e07678f66566fabe31f0f24ea0e5886620254cd6957db`): **adopted and verified.** `logs/move-map.py`, rerun by B, reproduces `logs/b1a-r3-move-map.txt` and `logs/b1a-r3-test-move-map.txt` identically apart from file-path labels (routes: 23 same, 14 visibility-only, 7 changed, 4 added; tests: 34 same, 4 changed, 5 added). An independent line-multiset comparison (visibility stripped) of `tree-b1a-r2/…/authoring.rs` against the four new route files leaves only: the module docs and uses, three signature re-wraps, the `#[path]`/`mod` lines, and the `editor_body` changes for F003/F013 plus the extracted `editor_options`. For the tests, the same comparison against the four new test files loses no old line except the module doc comment; the additions are the two new tests, the F003/F014 assertions, docs and `mod` lines. The tree-b1a-r2 bases match B's own R002 copy.

## WF-P09-F016 — A parameter-selected step lists every frozen agent, not the ones its parameter allows

- Date 2026-10-24; round `WF-P09-R002`; digest `b4dc745d…de3c`. **Actionable defect** (residual of
  the F001 correction), severity **Low**, category requirements (06-D5 capability display).
- Evidence: `preview.rs` `model_steps`, `Binding::Reference` arm — "the agent named by
  `/params/<p>` at run time, one of …" joins **all** `bundle.snapshots().agents`. The frozen set
  is the union over every model step (`publication/capabilities.rs` `agent_universe`), and a
  binding's parameter is validated against that step's `parameter_schema` enum. So with a
  literal `@x` step and a `/params/agent` step whose enum is `[y, z]`, the preview says the
  second can run "one of @x, @y, @z"; `@x` is not selectable there. The new test has a single
  step, where the union equals the enum, so it does not distinguish the two. Found by code
  reading.
- Fix direction: list the referenced property's `enum` from the definition's
  `parameter_schema`, the same lookup `agent_universe` performs. Add a two-step test.
- Resolution history: 2026-10-24 open.
  - 2026-10-24 R003 recheck (digest `1484a5bc494ed3c0202e07678f66566fabe31f0f24ea0e5886620254cd6957db`): **fixed.** `parameter_choices` lists the referenced property's `enum` from `parameter_schema` (the `agent_universe` lookup) and falls back to "the frozen agents" only when there is none. Test `a_parameter_selected_step_lists_its_own_parameters_agents` (literal `@assistant` step and a `/params/agent` step with enum `[helper]`): the second says "one of @helper" and not "@assistant". Passes.

## WF-P09-F017 — Optional (gate P09-B1b-D1 rev 1): name the closure walk of `frozen_agent_ids`

- Date 2026-10-24; round `WF-P09-R004` (architecture gate review); decision P09-B1b-D1 rev 1
  (TASK "Gate P09-B1b-D1"). **Optional suggestion**, severity Low, category architecture.
- Evidence: the decision says "recursively of every `children()` bundle". Storage dedupes the
  closure by version id (`publication/storage.rs` `store_bundle`: ≤ 256 distinct versions), but
  the in-memory children form a DAG. Diamonds are legitimate: two children may pin the same
  grandchild, and depth is ≤ 32 (`MAX_DEPENDENCY_DEPTH`). A walk without a visited set counts
  paths, not versions, so its cost is exponential in depth for such a DAG. On the publish path
  this is bounded today only indirectly: `authority::supported_publication` (called before
  `insert_version`, `definitions.rs:204-213`) refuses a closure walk of more than 256 visits.
  The new method would be correct only while every caller keeps that ordering.
- Suggestion: specify a visited-set walk by `VersionId` (as `store_bundle` does), bounded by
  `MAX_DEPENDENCIES`. Add a diamond case (a grandchild pinned by two children, with its agent
  counted once) to the persistence test.
- Resolution history: 2026-10-24 open (optional).
  - 2026-10-24 R005 (digest `293c80357296a898b19f1e83b6eed47cb45f5dc74fcc83d00840aab418878555`): **adopted and verified.** `PublishedBundle::frozen_agent_ids` walks with a `VersionId` visited set (`publication/mod.rs`). Test `used_by_lists_a_binding_that_reaches_the_agent_only_through_a_child_diamond` publishes a diamond (two children pinning one grandchild) and asserts the agent is stored once.

## WF-P09-F018 — Optional (gate P09-B1b-D1 rev 1): pin down the "Used by" read and its tests

- Date 2026-10-24; round `WF-P09-R004`; decision P09-B1b-D1 rev 1. **Optional suggestion**,
  severity Low, category architecture / verification.
- Suggestions:
  1. **Order and bound.** Give `agent_usage` a deterministic `ORDER BY` (for example workflow
     title, then binding id) under `LIMIT 51`, so the 50 shown and `more` are stable. Say in the
     copy that the hidden-channel count covers only the fetched rows, and that `more` means more
     bindings exist, visible or not.
  2. **Agent scope.** The read alone answers an empty list for another company's agent, because
     bundles only hold own-company agents. The planned NotFound must come from a company-scoped
     agent lookup in the route. State that, and note that the managed-company gate is the
     route's (`AuthoringReads` grant nothing), which the invariant's "authorizes the company like
     the other authoring reads" blurs.
  3. **Display-only rule.** Make "no execution, admission, authorization or revocation path reads
     `agent_ids`" an invariant (and a `COMMENT ON COLUMN`), not only a failure-mode note. A later
     "agent in use" guard built on it would turn a mislabel into a wrong decision.
  4. **Shape at the boundary** (`persistence/AGENTS.md`): `NOT NULL` does not stop NULL
     elements. Consider `CHECK (array_position(agent_ids, NULL) IS NULL)`, and optionally a
     cardinality bound.
  5. **Tests to add:**
     - a binding reconfigured from a version that froze the agent to one that does not
       disappears from "Used by" (the join uses the current configuration revision);
     - a version with no model steps stores `'{}'`;
     - the cross-company `INSERT … SELECT` test still fails on the foreign key, because NOT NULL
       is checked before the deferred FK trigger and the copy must therefore include `agent_ids`.
- Resolution history: 2026-10-24 open (optional).
  - 2026-10-24 R005 (digest `293c80357296a898b19f1e83b6eed47cb45f5dc74fcc83d00840aab418878555`): **adopted and verified.**
    - `agent_usage` orders by `binding.id` under `LIMIT 51`. The hidden-count copy says "of the
      listed bindings".
    - The route looks the agent up company-scoped first; another company's agent is NotFound
      (tested, and members too).
    - `CHECK (array_position(agent_ids, NULL::uuid) IS NULL)` and the "display index only"
      `COMMENT ON COLUMN` are in the init schema.
    - Tests: reconfigured off the version → gone; a `data.map` version stores an empty array;
      the cross-company copy includes `agent_ids`.
    - The `persistence::workflow` run is recorded in R005.

## WF-P09-F019 — A not-ready resource is reported as a duplicate create or a concurrent change

- Date 2026-10-24; round `WF-P09-R005` (B1b batch review); digest `293c80357296a898b19f1e83b6eed47cb45f5dc74fcc83d00840aab418878555`. **Actionable defect**,
  severity **Medium**, category correctness / requirements (B1b plan: "Refusals (readiness …)
  render beside the form with every choice kept"; "a readiness refusal on activate is shown on
  that row").
- Evidence: the binding command reports an unavailable resource as `AppError::Conflict("Resource
  slot {slot} is not ready")` (`application/workflow/binding/resources.rs:73-78`, readiness
  `Unavailable`, e.g. a bearer HTTP or MCP connection with no secret yet). The UI routes treat
  every `Conflict` as a revision or key conflict (`routes/workflows/ui/authoring/bindings.rs`):
  - create (:497-511) answers "Binding X was already created; it is listed above." and renders a
    fresh form, so nothing was created and every choice is gone;
  - configure (:512-522) answers "Someone changed this binding while you saved it … saving again
    replaces its configuration", but saving again fails the same way;
  - activate (:565-570) answers "This binding changed since the page was shown … Nothing was
    changed."
  The real reason is never shown. `binding_refusal` handles `NotFound` (revoked) and
  `BadRequest` but never sees this `Conflict`.
- Reproduction (confirmed): probe `probe_b5_unready_resource_on_create_configure_and_activate`
  (`logs/r005-b-probes.log`). It uses an `http.request` version, one bearer HTTP connection
  with no credential, and one whose credential is then removed. Create says "already created"
  with 0 bindings and the selection gone; activate says "changed since the page was shown";
  configure says "Someone changed this binding". No test covers a real resource-readiness
  refusal (`a_refused_binding_keeps_every_choice_and_says_why` only exercises params and
  trigger refusals).
- Fix direction: tell the two apart before choosing the message. On create, the keyed binding
  either exists (a double submit) or does not (show the command's message as "Not saved: …"
  with the form kept). On configure and activate, compare the current state revision with the
  one sent; when unchanged, show the command's message. Add a test with an unavailable
  connection on all three paths.
- Instruction lesson: hypothesis — the command reuses `Conflict` for a readiness state. Candidate
  rule for UI work over commands: "map a command's error by what it means in that command, not by
  its status class; enumerate the command's `Conflict` sources before reading every
  `Conflict` as a revision conflict".
- Resolution history: 2026-10-24 open.
  - 2026-10-25 R006 recheck (`WF-P09-R006`, digest `40320058996d47820d1c1517c683517973c5ac745a38eaf82aa77636fea1156e`): **fixed.** `Ui::binding_moved` reads the binding before choosing a message. A create's `Conflict` reads as "already created" only when the keyed binding exists. An edit's or a row's reads as a concurrent change only when the state revision differs from the one sent (or the binding is gone). Otherwise the command's message shows as "Not saved: …" with the form kept, or "Not activated/deactivated: …" on the row. Test `a_resource_that_is_not_ready_is_refused_as_such_on_create_configure_and_activate` (B's probe turned into a test) covers all three paths, the same key then saving, and a genuinely stale row still said as one. Passes (`logs/r006-b-focused.log`).

## WF-P09-F020 — The revoke and re-point warnings misstate what P07-D2 rev 4 does

- Date 2026-10-24; round `WF-P09-R005`; digest `293c80357296a898b19f1e83b6eed47cb45f5dc74fcc83d00840aab418878555`. **Actionable defect**, severity **Medium**,
  category requirements (WF-C003 r1 "warns …"; DV-P09-7; P07-D2 rev 4 §D2.2, §D2.3, §D2.7).
- Evidence: `pages/workflow_authoring/tools.rs:12` `REVOKE_WARNING`: "Revoking refuses every
  call to this tool from published versions that froze it, including runs already waiting on
  one. …". The re-point consequence (:136) says: "Pointing it here refuses every call from
  published versions that froze the other workflow, until they are republished." Against the
  accepted decision and the code:
  1. **Understated:** revocation, and a re-point (which acts as one), also refuses **new runs**
     of every version that froze the tool, plus configuring or activating its bindings, not
     just calls. Admission runs `bindings::verify_selection` → `supported_publication(Run)`
     (`admission.rs:109`, `bindings.rs:115-136`), and D2 rev 4 says "Revocation … refuses new
     runs and new tool calls" (§D2.2) and "revoking refuses new admissions (and binding
     activation readiness)" (§D2.7). A whole workflow can stop starting, and the warning does
     not say so.
  2. **Overstated:** "including runs already waiting on one" is wrong for a call already made:
     "a call parked before the revocation still delivers its result" (§D2.7). D2.3 step 2 also
     answers a completed child wait even if the registration was revoked meanwhile. The B1b plan
     text carried the same wording, so the error began in the plan and the code copied it.
  The same-workflow re-registration sentence is correct (§D2.2).
- Fix direction: state that revoking (or pointing the name at another workflow) stops new runs
  of every published version that froze it, and their bindings' activation, and refuses their
  agents' next calls. State that a call already in progress still returns its result, and that
  re-registering for the same workflow restores them. Pin the copy in a test against these
  facts.
- Instruction lesson: "UI copy that states a decision's consequences is checked against the
  decision's verification list, not the plan's paraphrase".
- Resolution history: 2026-10-24 open.
  - 2026-10-25 R006 recheck (digest `40320058996d47820d1c1517c683517973c5ac745a38eaf82aa77636fea1156e`): **fixed.** I checked `REVOKE_WARNING`, `REPOINT_WARNING`, `REPIN_NOTE` and the two banners against P07-D2 rev 4:
    - new runs refused (§D2.2; `admission.rs:109` → `verify_selection`);
    - bindings can no longer be configured or activated (`save_binding_on` runs `verify_selection` for every change but deactivate);
    - the agents' next calls refused (§D2.3 step 3);
    - "a call already in progress still returns its result" (§D2.7);
    - same-workflow re-registration restores them; another workflow keeps them stopped until republished (§D2.2);
    - re-pinning leaves published versions running their frozen pin (§D2.2).

    Test `revoking_or_re_pointing_a_frozen_tool_stops_its_versions_bindings_as_the_warning_says` pins the copy's facts and drives revoke → activation refused → re-register → activated → re-point → refused. Passes. The B1b plan bullet was corrected too.

## WF-P09-F021 — The parameter fields drop undeclared keys and change number types on save

- Date 2026-10-24; round `WF-P09-R005`; digest `293c80357296a898b19f1e83b6eed47cb45f5dc74fcc83d00840aab418878555`. **Actionable defect**, severity **Low**,
  category correctness (round-trip fidelity of stored params).
- Evidence: `application/workflow/authoring/params.rs`:
  - `parameters_from_fields` (:98-132) builds only the declared properties. When the schema
    allows other keys (`additionalProperties` absent or true, or `patternProperties`), keys a
    binding holds beyond `properties` (for example set through the JSON route) are silently
    removed by the next save from the form.
  - A `number` field parses to `f64` and `Number::from_f64`, so a stored `5` comes back as
    `5.0`: a different JSON value, and a new configuration revision with nothing changed.
    Integers beyond 2^53 also lose precision.
  - An empty text field means "omit", so an empty string cannot be expressed; whitespace-only
    input is omitted too.
  What it does with schemas it cannot express is sound: a non-object schema, no properties, or
  more than 64 → one JSON object; a property it cannot type → one JSON value per field.
- Fix direction: fall back to the whole-object JSON field when stored params hold keys outside
  `properties`, or carry them through unchanged. Keep a number that parses as an integer as an
  integer. Optionally distinguish "empty string" from "unset" for text fields. Add round-trip
  tests (stored → form → saved equals stored).
- Resolution history: 2026-10-24 open.
  - 2026-10-25 R006 recheck (digest `40320058996d47820d1c1517c683517973c5ac745a38eaf82aa77636fea1156e`): **mostly fixed; remains open (Low).** `fields_for` shows fields only when rebuilding from them reproduces the stored object; otherwise the whole object opens as JSON, and a hidden `params_mode` makes the save parse what was shown. Undeclared keys, empty or whitespace-only strings, and type mismatches all fall back correctly. Integer and number fields parse as JSON numbers (5 stays 5, large integers keep their digits). Unit and route tests pass.
    - **Residual:** the check runs on the server's values, but a text field is an `<input>`, and HTML's value sanitization strips newlines. A stored text parameter containing a newline (a signature, a multi-line instruction) passes `fields_for`, is shown in an `<input>`, and is submitted with the newline removed. So saving the form unchanged still changes the stored object. Confirmed in the installed Chrome 155 headless on a data-URL page with the exact markup: attribute `"Kind regards,\nSupport"`, `input.value` and the submitted value `"Kind regards,Support"` (`logs/r006-b-newline-check.log`). The Rust tests cannot see this because they submit `field_value` directly.
    - Fix: in `fields_for` (or `field_value`), treat a text value containing `\n` or `\r` as not carried, or render such text parameters as a `<textarea>` (whose CRLF normalisation then needs the same check). Add a unit case.
  - 2026-10-25 R007 recheck (`WF-P09-R007`, digest `89d23e56e74c70e6fcea1d60a9b9b87d4cc5c829af3afd34e8d8ab8147760eb9`): **fixed.**
    - `fields_for` now returns `None` when a text or choice value contains `\n` or `\r`, so
      such params open as the whole object in the JSON textarea.
    - Unit cases: `"Kind regards,\nSupport"` and `"a\r\nb"` go to JSON, and a one-line value
      with trailing spaces stays in fields.
    - The JSON path keeps them. `to_string_pretty` escapes line breaks inside strings, and the
      textarea's CRLF normalisation touches only whitespace between tokens.
    - Confirmed in Chrome 155 headless with the exact escaped markup, after the start-tag
      newline: the submitted JSON parses equal to the stored object
      (`logs/r007-b-json-textarea-check.log`).
    - Route test `params_the_fields_cannot_carry_open_as_json_and_save_unchanged` and the unit
      test pass (`logs/r007-b-focused.log`).

## WF-P09-F022 — The 30 s reload of a run waiting on reconciliation wipes the note being typed

- Date 2026-10-25; round `WF-P09-R008` (B2a batch review); digest `a62d5b7ea0affde68111a687eef03c60365a49c4427c669262fef739aa7e70a2`.
  **Actionable defect**, severity **Medium**, category requirements / correctness (lost operator
  input). Sources: WF-C005 r1 ("record what happened" with an unknown note); the B2a design, which
  offers that annotation "only on `reconcilable` hints"; DV-P09-12, which kept the 30 s `tick`
  for unfinished runs.
- Evidence:
  - The body's reload leaf listens to `sse:tick` whenever the run has not ended
    (`pages/workflow_runs/detail.rs:106-108`, `:126-130`). "Waiting" has not ended, so a run
    waiting on reconciliation reloads every 30 s (`ui/runs/events.rs:98-101`).
    `controls_tests.rs:131-134` pins exactly this trigger on the reconciliation fixture's run.
  - The reload swaps `#run-body` outerHTML. The "Record what happened" `<details>` and its
    `<textarea name="note">` (`detail.rs:592-606`) live inside that body, and nothing preserves
    them. Only finished steps carry `hx-preserve` (`:455-459`).
  - So within 30 s of opening the form, and on any `run-<id>` or `resync`, the disclosure closes
    and the typed note is gone without a word. The uncertain step itself is unfinished, so its
    opened `<details>` (frozen inputs, Diagnostics: worker attempts) collapses on the same tick
    too, just while the operator is reading it to decide what to write.
- Reproduction (confirmed at markup level): probe `probe_r008_tick_reload_replaces_the_reconcile_note`
  (`logs/r008-b-probes.log`). The waiting-on-reconciliation body's leaf is `sse:run-…, sse:resync,
  sse:tick`, and the note's textarea sits at byte 5425 of `#run-body` with no `hx-preserve` in
  the body. The browser pass never typed into the form (`logs/b2a/browser/b2a-pass.js` has no
  reconcile step).
- Fix direction (author's choice):
  - keep the form and any open unfinished step across reloads (stable ids plus `hx-preserve`,
    keyed by marker and revision so a changed action still re-renders); or
  - tick only when a reload can show something new. Step progress happens while `running`; a
    run waiting on reconciliation or a decision changes only through events; a run waiting on a
    child needs its children's badges, which its parent pane could follow by their own events; or
  - skip the reload while the body holds unsent input, using a listener in a static script (CSP
    forbids htmx trigger filters, which need eval).
  - Add a browser check: type a note, let a tick pass, and the text and the open state remain.

- 2026-10-25 **WF-P09-R009** correction recheck, artifact `bb37b8a157f40f67a0ba9f478500d2dd755709ce25267ab1bd6a0642f3b6330e` (Reviewer B, requested `gpt-6-astra` / medium, independent context): **Fixed for the reported input-loss defect.** Waiting reconciliation runs no longer tick; the reconciliation disclosure is preserved by marker/target/epoch/revision, and unfinished step disclosures by execution/revision. The real-browser protocol reconnect test preserves both typed text and open state. The affected change to waiting-parent freshness has a separate regression, F030.

## WF-P09-F023 — A reload replaces the command keys, so "press it again" can start a second rerun

- Date 2026-10-25; round `WF-P09-R008`; digest `a62d5b7ea0affde68111a687eef03c60365a49c4427c669262fef739aa7e70a2`. **Actionable defect**, severity
  **Medium**, category correctness / requirements. Sources:
  - WF-C005 r1 client rule: "after a 503 (timeout), retry the same request with the same key";
  - the B2a design: "A `503` keeps the same key, and the page says retrying with that key is
    safe";
  - plan 09 criterion 5 ("test reconnect").
- Evidence:
  - Every render of the controls draws a fresh `Uuid::new_v4()` key (`detail.rs:260` for cancel,
    retry and rerun; `:611` for the annotation). The page promises "If a request gets no answer,
    press it again: it repeats the same request, which is safe." (`detail.rs:303-308`).
    `controls.rs:1-6` relies on a failed request leaving "the page and its key as they were".
  - That holds only until the next body reload. The leaf reloads `#run-body`, controls included,
    on `sse:run-<id>`, `sse:resync`, and `sse:tick` for an unfinished run. The stream's first
    event on every connection is `resync` (`events.rs:76`), and the SSE extension reconnects
    after any drop (`assets/htmx-ext-sse-2.2.3.js:199-228`). A lost answer usually comes with a
    dropped connection, because the network or the server failed for both. So the reconnect's
    `resync` re-renders the forms with new keys before the operator presses again.
  - Cancel and retry are still protected by their revision: if the first one committed, the
    reload shows the new state and the button is gone. **Rerun has no such guard.**
    `start_command` ignores the revision, `rerun_of` is not unique
    (`workflow_runs_reruns` is a plain index), and the original run's body neither ticks (it has
    ended) nor shows its reruns. A pressed-again rerun after a reload therefore starts a second
    run, with its own effects.
- Reproduction (confirmed end to end, own database on 55490): probe
  `probe_r008_a_reloaded_rerun_form_starts_a_second_run` (`logs/r008-b-probes.log`):
  - first press → "Started run 0a731d0c as a rerun of 12c2ab53";
  - the same key again → "Already started run 0a731d0c" (the replay works while the page
    holds);
  - after a body reload, the reloaded form's key → "Started run 56a8988b"; reruns of the
    original: **2**.
  - Probe `probe_r008_reload_replaces_the_command_keys` shows every key changing between two
    renders at the same revision (waiting, cancelled and failed runs).
- Fix direction: keep a control's key stable across reloads while the run's revision is
  unchanged. For example:
  - the leaf sends the keys it holds (`hx-include` of the forms' `command_key`) and the server
    renders them back unless the revision moved; or
  - the key is derived from a per-pane nonce that only opening a run or a control's answer
    renews.
  Then test a no-answer followed by a reconnect `resync` in the browser, with the second press
  replaying ("Already started"). That test also closes criterion 5's client-side reconnect,
  which the B2a evidence exercises only on the server (first event `resync`).

- 2026-10-25 **WF-P09-R009** correction recheck, artifact `bb37b8a157f40f67a0ba9f478500d2dd755709ce25267ab1bd6a0642f3b6330e` (Reviewer B, requested `gpt-6-astra` / medium, independent context): **Still open (Medium).** `pages/workflow_runs/detail.rs:369-384` includes the newest rerun ID in the command key. Thus a committed rerun followed by a lost response and reconnect still replaces the key, even at the same original-run revision and pane epoch. The new history and first `Rerun` → `Rerun again` label reduce ambiguity only for the first rerun. For a run already rerun once: open it, press **Rerun again**, let the POST commit but lose its response, reconnect, then press **Rerun again** as the unchanged instruction at `:395` says. The new key starts a third run instead of replaying the second. Before and after reconnect both buttons and explanatory notes have the same wording. `controls.rs:135-147` submits that new key to `start_command`; the original run's revision does not guard rerun. The current test `a_redraw_repeats_its_keys_until_the_command_shows_on_the_page` explicitly asserts a changed key after commit, and `scripts/tests/workflow-runs-browser.cjs:309-320` stops after checking history/label: it does not retry that reloaded form. This residual is established by the key derivation, rendering and command trace, not a new mutation of browser fixtures. Retain a logical request through uncertainty/reconnect, and renew intent only on a deliberate separate action/acknowledgement; cover the already-rerun case with a dropped committed response, reconnect and retry, asserting exactly one additional run.

- 2026-10-25 **WF-P09-R010** correction recheck, artifact `96c6c401cffc64d5566a19b414a52fdcacb7d00a1e402ea3c139d506e43ffde6` (independent Reviewer B, requested `gpt-6-astra` / medium): **Fixed.** `detail.rs` no longer includes rerun history in the command key. The same pane epoch and revision retain the same rerun intent after commitment and reconnect; the separate “Prepare another rerun” read opens a fresh pane epoch explicitly. The changed database test proves replay both before and after a second rerun with exactly two resulting runs. Inspected the actual application browser script and `logs/b2a-r2/browser-r009/pass.log` (46/46): the committed second response is dropped, an actual SSE disconnect/reconnect retains the key, and retry creates no third run. No write entered the read replacement group.

## WF-P09-F024 — A same-key note after a lost answer reads as "Nothing was recorded" when one was

- Date 2026-10-25; round `WF-P09-R008`; digest `a62d5b7ea0affde68111a687eef03c60365a49c4427c669262fef739aa7e70a2`. **Actionable defect**, severity
  **Low**, category correctness / feedback (the same class as F008).
- Evidence: `controls.rs:206-212` maps `RevisionConflict` and `IdempotencyConflict` to one
  message: "Nothing was recorded: the run changed since the page was shown." In this UI the form
  keeps its key only when a request got no answer (by design, WF-C005). If the operator then
  edits the note and presses again, the same key now carries a different request digest
  (`reconciliations.rs:8-9`: the digest includes the evidence). The first note may well have
  been recorded, and the answer says the opposite, with the wrong cause.
- Reproduction (confirmed): probe `probe_r008_a_same_key_edited_note_reads_as_nothing_recorded`
  (`logs/r008-b-probes.log`). The first note is "Recorded: …"; the edited note under the same key
  answers "Nothing was recorded: the run changed since the page was shown."; evidence rows: 1.
- Fix direction: give `IdempotencyConflict` its own message, for example "This form was already
  used for a different note; an earlier press may have been recorded. The run as it is now is
  shown." Add a unit case.

- 2026-10-25 **WF-P09-R009** correction recheck, artifact `bb37b8a157f40f67a0ba9f478500d2dd755709ce25267ab1bd6a0642f3b6330e` (Reviewer B, requested `gpt-6-astra` / medium, independent context): **Fixed.** `controls.rs:206-222` separates idempotency conflict from revision conflict and says nothing *new* was recorded and the earlier note may be in the timeline. The focused database test sends a changed note under the same key, checks that message and one evidence row, then checks the new form key.

## WF-P09-F025 — The list's wake-ups drop the open run's highlight and collapse "Load more"

- Date 2026-10-25; round `WF-P09-R008`; digest `a62d5b7ea0affde68111a687eef03c60365a49c4427c669262fef739aa7e70a2`. **Actionable defect**, severity
  **Low**, category UI consistency. Source: DV-P09-12 item 3 names collapsing "Load more" as a
  defect worth fixing.
- Evidence:
  - `list` renders with `selected: None` (`ui/runs/mod.rs:263`), so the first `runs` wake-up
    removes the `menu-active` mark the workspace drew (`pages/workflow_runs/mod.rs:113-117`).
  - Opening another run swaps only `#run-pane` (`:107-109`), so until a wake-up the list marks
    the previous run.
  - Every visible run change in the company sends `runs` (`events.rs:84-91`), and the leaf
    reloads page 1 over the whole region (`mod.rs:178-179`). Pages added by "Load more" and the
    list's scroll position are lost each time.
  - DV-P09-12 removed exactly this reset from the 30 s timer, but in an active company it now
    happens on every run state change. The browser pass's Load more check ran with no runs
    changing.
- Fix direction:
  - render the selection from the request (the leaf can carry `run_id`, or read
    `HX-Current-URL`), and move the mark when a run opens;
  - once more pages are loaded, either reload up to the last loaded cursor, or replace the
    automatic reload with a "the list changed, refresh" row.

- 2026-10-25 **WF-P09-R009** correction recheck, artifact `bb37b8a157f40f67a0ba9f478500d2dd755709ce25267ab1bd6a0642f3b6330e` (Reviewer B, requested `gpt-6-astra` / medium, independent context): **Fixed for reviewed scope.** Selection is carried by `#run-selected`, pane selection triggers list reconciliation after settle, and entries reload inside the retained scrolling list up to the loaded count. The final OOB count correction covers a first page growing before Load more. Reads and Load more share the list replacement group; input counts are clamped, and the 200-row cap is visibly disclosed. Focused route assertions and the inspected 43/43 browser evidence cover selection, loaded rows and scroll.

## WF-P09-F026 — Run links inside the pane compete for `#run-pane` without `hx-sync`

- Date 2026-10-25; round `WF-P09-R008`; digest `a62d5b7ea0affde68111a687eef03c60365a49c4427c669262fef739aa7e70a2`. **Actionable defect**, severity
  **Low**, category correctness (race). Source: `src/adapters/http/AGENTS.md`: "Read-only htmx
  controls that can issue competing requests to one target must use `hx-sync="#target:replace"`,
  so the last user intent wins"; plan 09 criterion 5 (stale partial-page responses).
- Evidence:
  - The list links sync on `#run-pane:replace` (`pages/workflow_runs/mod.rs:107-109`). The
    pane's own run links do not: `run_link` (`detail.rs:57-64`) for parent, rerun original,
    children, wait children and the child's "controlled through its parent" link, and the step
    body's child links (`step.rs:150-152`).
  - htmx resolves the target when a request is sent, and an outerHTML swap into a detached
    target returns without swapping (`assets/htmx-2.0.4.min.js`, the outerHTML swap's
    `if(!i){return}`).
  - So between an in-pane click and a list click, the first response wins and the later click
    is silently dropped, whatever the click order.
- Fix direction: give `run_link` and the step's child links the same
  `hx-sync="#run-pane:replace"`, and assert it in the markup tests.

- 2026-10-25 **WF-P09-R009** correction recheck, artifact `bb37b8a157f40f67a0ba9f478500d2dd755709ce25267ab1bd6a0642f3b6330e` (Reviewer B, requested `gpt-6-astra` / medium, independent context): **Fixed.** List, lineage, wait, child and step-child links share `run_pane_attrs` and `hx-sync="#run-pane:replace"`. The controls remain outside that replace group. Markup checks cover parent/child/step links; the shipped-HTMX protocol check also proves a late body response cannot steal preserved disclosures after cancel or navigation.

## WF-P09-F027 — Tenancy and the stream's lag path are tested for only part of the new surface

- Date 2026-10-25; round `WF-P09-R008`; digest `a62d5b7ea0affde68111a687eef03c60365a49c4427c669262fef739aa7e70a2`. **Actionable defect** (verification),
  severity **Low**, category verification (plan 09 criterion 4; the B2a design's test list "lag
  yields `resync`"; F006 precedent).
- Evidence:
  - `other_tenants_and_plain_members_reach_no_run_and_hostile_content_is_escaped` checks the
    pane, the body, the step and cancel for a member and for a stranger on their own company id
    (`runs_tests.rs:541-566`). It does not check retry, rerun, reconcile, the list or the event
    stream.
  - For an admin on a hidden channel, only the pane is checked (`:447-450`).
  - Rerun is the one UI write that composes its own authorization:
    - `visible_run`, then `start_command(Some(association))`;
    - with an association given, `start_command` skips `visible_binding` (`bindings.rs`,
      `start_command`).
    - A test should pin that a stranger, a member and a hidden-channel admin each get NotFound
      and start nothing.
  - The stream's broadcast-lag arm (`events.rs:83`, `Some(Err(_))`) is untested. The test
    sends `MailboxEvent::Resync`, which is the listener-reconnect arm.
- The code paths read correctly (each handler calls `ui.company` first, and the run reads are
  company-scoped); this finding is about missing tests only.
- Fix direction:
  - extend the tenancy loop to every route, including the event stream's handler;
  - add the hidden-channel admin case for step, rerun, cancel and reconcile, asserting that no
    run or evidence was created;
  - add a lag test (a receiver of capacity 1 overrun before polling) expecting `resync`.

- 2026-10-25 **WF-P09-R009** correction recheck, artifact `bb37b8a157f40f67a0ba9f478500d2dd755709ce25267ab1bd6a0642f3b6330e` (Reviewer B, requested `gpt-6-astra` / medium, independent context): **Partially fixed; still open (Low, verification).** Retry/rerun/list/stream tenancy cases and a genuine capacity-one broadcast lag path now run successfully. Hidden-channel cancel and rerun are refused. However, the only new UI reconciliation refusal is a plain company member (`controls_tests.rs:274-288`); there is still no foreign-company run/marker case or hidden-channel admin reconciliation test through this UI route. The general tenancy loop at `runs_tests.rs:672-749` never invokes `reconcile`. The hidden-channel step assertion at `runs_tests.rs:573` supplies `Uuid::new_v4()` rather than an existing execution, so NotFound would also occur without the visibility gate. Complete the already-requested reconciliation cases with real action/run/execution/marker identities and assert no evidence or run changes; use a valid execution for the hidden step test. Existing JSON-route tests cover the shared command but do not pin the UI form/route boundary. No authorization exploit was observed by inspection.

- 2026-10-25 **WF-P09-R010** correction recheck, artifact `96c6c401cffc64d5566a19b414a52fdcacb7d00a1e402ea3c139d506e43ffde6` (independent Reviewer B, requested `gpt-6-astra` / medium): **Fixed.** The new `authorization_tests.rs` uses real uncertain action/run/execution/marker identities in both company and private-channel scopes. Foreign viewers are tried under their own and the target company paths; a company admin without private-channel access is refused. Both reconciliation and step routes return NotFound, no evidence is recorded, and the owner successfully submits the identical form as a positive control. Existing member, cancel/retry/rerun/list/stream and lag coverage is retained. B reran all 14 run-route tests successfully. Scoped fixture setup feeds the same association to configuration and admission; unchanged callers explicitly supply Company. Inspected their 54/54 focused test evidence.

## WF-P09-F028 — The B2a diff shows a modified p08 file as created

- Date 2026-10-25; round `WF-P09-R008`; digest `a62d5b7ea0affde68111a687eef03c60365a49c4427c669262fef739aa7e70a2`. **Actionable defect** (records), severity
  **Low**, category evidence integrity.
- Evidence:
  - `routes/workflows/reconciliations.rs` existed before p09: untracked p08 work, hash
    `c6412655…d309` in `start-filehashes.txt` and `b1b-r3-filehashes.txt`.
  - `logs/b2a.diff` presents it as `@@ -0,0 +1,124 @@` and counts it in "+3896". No base copy
    exists in `tree-b1b-r3/`, `start-base/` or the p08 records.
  - The B2a log's claim that `reconcile_command` is extracted "with no parallel logic" therefore
    cannot be checked by diff.
  - I relied instead on the six p08 JSON reconciliation tests passing over the extracted command
    (`logs/r008-b-focused.log`: `reconciliation_outcomes_follow_the_status_table`,
    `competing_reconciliations_record_once`, `…needs_a_ready_resource_even_to_replay`,
    `…refuses_strangers_members_and_foreign_markers`, `…annotates_both_http_forms`,
    `workflow_timeouts_answer_service_unavailable`), and on reading the whole file. I found no
    behaviour change.
- Fix direction:
  - say in the B2a log that the file pre-existed and what changed: the handler body moved into
    `reconcile_command`, and `TargetBody`/`ReconcileBody` became `pub`;
  - for later batches, save a base copy of any untracked pre-existing file a batch first
    modifies, so its diff is a real diff.

- 2026-10-25 **WF-P09-R009** correction recheck, artifact `bb37b8a157f40f67a0ba9f478500d2dd755709ce25267ab1bd6a0642f3b6330e` (Reviewer B, requested `gpt-6-astra` / medium, independent context): **Fixed as a records correction, with original evidence limitation retained.** TASK “B2a corrections for WF-P09-R008” explicitly identifies the pre-existing untracked file, its old hash, the extraction and visibility changes, and the missing verified original. It does not claim the historical diff is a recovered original. Reuse R008's whole-file inspection and six p08 reconciliation tests. The current correction snapshot retains the complete tree for future deltas.

## WF-P09-F029 — Five per-run reads walk the company's whole history in their table

- Date 2026-10-25; round `WF-P09-R008`; digest `a62d5b7ea0affde68111a687eef03c60365a49c4427c669262fef739aa7e70a2`. **Actionable defect**, severity
  **Low**, category bounds / performance. Source: `src/adapters/persistence/AGENTS.md` "A bounded
  result set does not imply bounded database work … Operational views must cap their working
  set", and the same section's rule to justify any index with representative plans.
- Evidence:
  - The run pane reloads on its run's events, on every `resync` and, while the run is
    unfinished, every 30 s. The step body loads on open.
  - These reads filter by run, but no index on their table leads with
    `(company_id, run_id)` or `(company_id, parent_run_id)`. The single init migration has only:
    - `workflow_control_commands_pkey (company_id, command_key)`;
    - `workflow_action_reconciliations_pkey` and `workflow_action_receipts_pkey`
      `(company_id, invocation_id)`;
    - `workflow_run_parents` keys led by `child_run_id`;
    - for `workflow_message_intent_bindings`, the pkey `(company_id, intent_id, binding_id)` and
      a partial index that excludes admitted rows.
  - With sequential scans disabled, the planner's best paths put the run in a `Filter` after an
    `Index Cond` on `company_id` alone:
    - controls, reconciliations and receipts: a bitmap scan of every company row;
    - the trigger message: a walk over the company's intents;
    - children: a skip scan over the `child_run_id`-led key, driven by all the company's runs.
    - `workflow_run_events` gets a proper `(company_id, run_id)` index condition, for contrast.
    - Probe `probe_r008_run_view_reads_have_no_run_index` (`logs/r008-b-probes.log`; empty
      tables, so this shows access paths, not costs).
  - The B2a log says the reads are "company-scoped" and lists bounds on the rows returned. It
    gives no plan for the work done.
- Fix direction, per the persistence rule:
  - collect `EXPLAIN (ANALYZE, BUFFERS)` at a representative volume;
  - where the working set is the company's history, drive the read through an indexed run path:
    - receipts and reconciliations through `workflow_action_dispatches`'
      `(company_id, run_id, execution_id, …)` key;
    - otherwise add the `(company_id, run_id)` or `(company_id, parent_run_id)` index in the
      init schema (the database is reset) and record the before and after plans.

- 2026-10-25 **WF-P09-R009** correction recheck, artifact `bb37b8a157f40f67a0ba9f478500d2dd755709ce25267ab1bd6a0642f3b6330e` (Reviewer B, requested `gpt-6-astra` / medium, independent context): **Fixed.** Three run-leading indexes in the init schema support controls, message provenance and child-parent reads. Reconciliation and receipt queries use the existing run-led dispatch key; the `(company_id, invocation_id)` primary key preserves join cardinality. Inspected `logs/b2a-r2/explain-before.txt` and `explain-after.txt`: representative old company scans become run index conditions/nested loops (controls 200k rows; receipts 100k). Focused migrated isolated-database tests exercise reconciliation and accepted receipts with the changed reads. The new reruns query uses the existing company/rerun index; its result is bounded with an explicit truncation flag.

## WF-P09-F030 — Waiting parents no longer refresh their children's current state

- Date 2026-10-25; workflow `implement-astra-reviewed-2.2`; phase p09, B2a correction recheck,
  note WF-P09-R009; reviewer B (requested gpt-6-astra / medium; independent context).
  Worktree `/Users/mac02/dev/openbot`, HEAD/base `da868710d3b235d49856e3e481f8f3735f6b3e56`,
  artifact `bb37b8a157f40f67a0ba9f478500d2dd755709ce25267ab1bd6a0642f3b6330e`. Records: TASK.md / REVIEW.md.
- **Actionable defect, Low, correctness / live operations.** Introduced by the F022 correction,
  rather than the original gated design. Plan 09 requires child-run inspection and distinct
  running/human-waiting/uncertain states; WF-C002/C003 require child lineage in inspection.
- Evidence (code and event trace, not a browser fixture mutation):
  - `pages/workflow_runs/detail.rs:161-183` listens only to the current run's event and resync,
    adding tick only when the **parent** state is running. A parent parked on `child_run` is
    waiting (`persistence/workflow/children.rs:111`).
  - The same parent body displays each child's current state and waiting reason in `children`
    (`detail.rs:721-734`).
  - `ui/runs/events.rs:32-39,94-102` emits `runs` plus `run-<changed child id>`, not a parent event.
    The SQL notification contains only NEW.id (`init_schema.sql:8842-8862`).
  - Parent continuation waits for a terminal child fact (`child_waits.rs:31-44`); a child going
    queued → running → waiting on a human decision/reconciliation leaves the parent waiting.
  - Therefore opening the parent while the child is queued and then allowing that child to wait
    leaves the parent's child badge queued indefinitely on a healthy SSE connection. The list
    updates, but it does not redraw the open parent. Previously tick refreshed this view.
- Required correction: retain safe refresh for waiting parents, or subscribe the parent body to
  relevant child wakeups, while preserving unsent reconciliation text. Verify a child transition
  to a nonterminal waiting state while its parent remains waiting, without reconnect/navigation.
- Instruction lesson: F022 itself already noted that suppressing waiting ticks needs child
  wakeups. Candidate scoped rule: when narrowing refresh eligibility, enumerate every mutable
  related object rendered in that region and preserve its invalidation path.
- Resolution history: open in R009.

- 2026-10-25 **WF-P09-R010** correction recheck, artifact `96c6c401cffc64d5566a19b414a52fdcacb7d00a1e402ea3c139d506e43ffde6` (independent Reviewer B, requested `gpt-6-astra` / medium): **Fixed.** The body's reload leaf adds `sse:run-<child>` for each rendered child, while the waiting parent's own revision/state may remain unchanged. Notifications remain visibility-checked and child-scoped; no broad company refresh or waiting tick was added. Current route markup asserts the actual child ID and B independently ran the shipped-HTMX browser protocol (3/3): a child wakeup updates its badge while the parent remains waiting and the SSE connection count stays one. Existing preservation/reconnect checks continue to pass. The explicit existing child-list truncation bounds these subscriptions to the rendered children.

## WF-P09-F031 — The live attribution window can exclude messages still mounted in the conversation

- Date 2026-10-25; workflow `implement-astra-reviewed-2.2`; phase p09, B2b whole-batch review,
  round WF-P09-R011; independent Reviewer B, requested gpt-6-astra / medium (actual runtime
  settings not separately exposed). Worktree `/Users/mac02/dev/openbot`, branch workflows,
  HEAD/base `da868710d3b235d49856e3e481f8f3735f6b3e56`; artifact `0d695075870e4287db513ffcb96c9cb1b5d54ab66f0332e441f7759e281e83a4`.
  Original requirements: plan 09; records TASK.md / REVIEW.md.
- **Actionable defect, Low, correctness / requirements.** Plan 09 requires conversation entries
  to identify/link their producing runs and SSE reconnect reconciliation.
- Evidence (code-path analysis; no new application-server fixture mutation):
  - `persistence/thread_activity.rs:138-144` always selects only the newest 200 message identities
    from the database. `:64-74` uses those and the newest 50 unfinished/failed runs as the only
    inspector-eligibility input. `pages/workflow_runs/mod.rs:352-360` emits attribution OOB updates
    only for that 200-message set.
  - The actual live pane retains old bubbles and appends indefinitely: `routes/ui.rs:619-645`
    streams successive 50-message batches after its durable cursor, including a backlog older
    than the newest 200. It neither drops old bubbles nor marks their attribution as stale.
  - Reproduction sequence: keep a question mounted, append at least 200 later messages, then
    admit a run for that question. Its run wakeup refreshes the strip but the question receives
    no OOB attribution and remains unlabelled. Reconnecting does not fix it because the same
    newest-200 query excludes it again.
  - Second manifestation: reconnect after more than 200 messages have accumulated. A completed
    producing run referenced by an older backlog message is absent from both eligibility inputs;
    the stream renders that message as a plain run label even for an owner who may inspect it,
    and later attribution refreshes never correct that older bubble.
- Required correction: define a bounded reconciliation protocol for the identities the client
  actually displays (or an explicit bounded live-history window), and determine inspector
  eligibility for each streamed batch's actual run IDs. Do not solve this by removing bounds.
  Test delayed admission on a still-mounted message outside the newest-200 DB window and a
  reconnect backlog exceeding 200, preserving composer and stream lifecycle.
- Instruction lesson: a query's newest-N bound is not necessarily the live client's retained
  window. Candidate scoped guidance: document and test the relationship between retained DOM,
  durable stream cursor and bounded reconciliation scope.
- Resolution history: open in R011.

- 2026-10-25 **WF-P09-R012** correction recheck, artifact `5578887c5eb2b7e4eb0e76b62bb02e1140e9fbd4c5bb8cd415530a368af6ca6d` (independent Reviewer B, requested gpt-6-astra / medium): **Fixed by an explicit bounded live-history contract.** The mailbox now states that only the newest 200 messages are retained; each bubble has a canonical identity, and THREAD_HISTORY_SCRIPT removes only excess bubbles after append while preserving the retained anchor, stream owner and composer. Therefore a delayed admission cannot leave an out-of-window bubble mounted indefinitely. More importantly, `thread_message_stream` authorizes each actual streamed batch's own run IDs via `inspectable_runs` before rendering, independent of the newest activity window. B reran the real DB regression that places an old run outside both activity/history windows and confirms eligibility for its backlog batch, and the shipped-HTMX browser regression reconnecting through 210 messages, retaining 200 correctly linked messages and unsent text. The trimming script runs after the existing body-level live-edge handler and adjusts the retained anchor without replacing its container. This changes the live history limit explicitly, as permitted by the R011 remediation, rather than removing bounds.

## WF-P09-F032 — Every activity refresh loads every company channel to authorize a bounded run set

- Date 2026-10-25; workflow `implement-astra-reviewed-2.2`; phase p09, B2b whole-batch review,
  round WF-P09-R011; independent Reviewer B, requested gpt-6-astra / medium (actual runtime
  settings not separately exposed). Worktree `/Users/mac02/dev/openbot`, branch workflows,
  HEAD/base `da868710d3b235d49856e3e481f8f3735f6b3e56`; artifact `0d695075870e4287db513ffcb96c9cb1b5d54ab66f0332e441f7759e281e83a4`.
  Original requirements: plan 09; records TASK.md / REVIEW.md.
- **Actionable defect, Low, bounds / performance.** Violates the source boundary-bound rule and
  persistence rule that operational views cap their working set and avoid unnecessary broad reads.
- Evidence: `persistence/thread_activity.rs:189-195` invokes `list_by_company_id` before loading
  the selected runs' channel IDs. `persistence/channel.rs:369-379` fetches every company channel
  without a limit. Its `CHANNEL_SELECT` includes settings, aliases, participant projections and
  principal-grant aggregates, not just visibility data. This occurs for every new `thread_runs`
  call, including an empty run-ID set, every message/run wakeup and reconnect of every open pane.
  Thus one thread with one run repeats work proportional to all company channels and their grants.
  The supplied EXPLAIN evidence covers OPEN_THREAD_RUNS_SQL only and does not assess this read.
- Required correction: load the distinct source channels of the requested company-scoped run IDs
  and apply the existing viewer policy to those channels only (plus the established owner/member
  handling). Keep the read bounded to the run set; do not add arbitrary channel truncation, which
  would silently revoke legitimate links. Cover an unrelated-channel population and private
  producing-channel visibility, and retain a representative plan or equivalent bounded-read proof.
- Instruction lesson: reusing an authorization policy does not require reusing an unbounded
  catalogue read; source-level and persistence bounds already cover this case.
- Resolution history: open in R011.

- 2026-10-25 **WF-P09-R012** correction recheck, artifact `5578887c5eb2b7e4eb0e76b62bb02e1140e9fbd4c5bb8cd415530a368af6ca6d` (independent Reviewer B, requested gpt-6-astra / medium): **Fixed.** `inspectable_runs` first scopes requested runs by company and IDs, deduplicates their source channels, then calls `channels_by_ids` with only that set. Empty identity/channel sets return early. Existing membership and `Channel::viewer_access` policy remain unchanged; no arbitrary catalogue truncation affects authorization. The internal limit is derived from retained-message count, producer/admission cap and run-strip count and rejects oversized input. B reran the test with 64 unrelated channels and cross-company/empty-set assertions and retained the source-channel authorization regression. Inspected the production-query plan `logs/b2b/r011-channel-explain-testdb.log`: company and requested channel IDs are index conditions; only the selected channel's aggregates execute. No index or schema change.

## WF-P09-F033 — New SQL omits required AS table aliases

- Date 2026-10-25; workflow `implement-astra-reviewed-2.2`; phase p09, B2b whole-batch review,
  round WF-P09-R011; independent Reviewer B, requested gpt-6-astra / medium (actual runtime
  settings not separately exposed). Worktree `/Users/mac02/dev/openbot`, branch workflows,
  HEAD/base `da868710d3b235d49856e3e481f8f3735f6b3e56`; artifact `0d695075870e4287db513ffcb96c9cb1b5d54ab66f0332e441f7759e281e83a4`.
  Original requirements: plan 09; records TASK.md / REVIEW.md.
- **Actionable defect, Low, repository instructions.** `src/adapters/persistence/AGENTS.md`,
  “Write SQL for the reader,” explicitly says “Always write AS for a table alias,” including FROM
  and JOIN targets. This is an explicit repository requirement, not a reviewer style preference.
- Evidence: newly added `OPEN_THREAD_RUNS_SQL` uses `FROM workflow_runs run` and
  `JOIN workflow_versions version` (`thread_activity.rs:118-119`); the new message read uses
  `FROM thread_messages association` and `JOIN messages message` (`:139-140`).
- Required correction: use explicit AS in these new query aliases; no unrelated query reformatting.
  Focused formatting/offline compilation and the affected query tests suffice.
- Instruction lesson: existing guidance is sufficient; no new rule proposed.
- Resolution history: open in R011.

- 2026-10-25 **WF-P09-R012** correction recheck, artifact `5578887c5eb2b7e4eb0e76b62bb02e1140e9fbd4c5bb8cd415530a368af6ca6d` (independent Reviewer B, requested gpt-6-astra / medium): **Fixed.** The newly introduced run/version and message/association SQL now uses explicit AS table aliases. The unchanged shared CHANNEL_SELECT was not reformatted. Focused query tests and offline compilation succeeded; git diff --check is clean.


## WF-P09-F034 — Comment refresh loses an unfinished submission when the decision closes

- Identity: 2026-10-25; implement-astra-reviewed-2.2; Reviewer B (requested Astra/medium; runtime model not independently exposed); phase p09; worktree /Users/mac02/dev/openbot, workflows; base/HEAD da868710d3b235d49856e3e481f8f3735f6b3e56; artifact 8cb2171b82b9e72e6d5a69813b81066dc7012e0fc9e3674815221f338e794b5b.
- Context: [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); B3 whole-batch review WF-P09-R014. Actionable defect; **Medium**; correctness/input preservation.
- Evidence: `src/adapters/http/pages/workflow_decision.rs:43-51,77-81` preserves submission input only when supplied as Submission; a closed read replaces the preserved form with settled content. `src/adapters/http/routes/workflows/decisions.rs:198-230` rereads after the comment command, which carries only Comment inputs and discards even those on success. If another reviewer settles, or the deadline expires, while this reviewer writes and posts a comment, the response removes the local unsent data and feedback. HTMX preservation needs a matching incoming element and cannot preserve a form absent from the closed response. A comment posted immediately before another reviewer settles has the same read-after-command race.
- Reproduction: actual Rust-rendered waiting/closed fixtures with shipped HTMX, type `Unfinished draft` and `Keep this feedback`, post a comment, return the closed section. Both values disappear. [Probe](logs/r014-comment-close-probe.mjs), [passing defect assertion](logs/r014-comment-close-probe.log). This is a deterministic response-state probe, not a claim that a live concurrent database schedule was executed.
- Required behavior: retain recoverable unsent submission text when an ordinary discussion refresh observes a closed/stale decision; disable settlement for that exact identity. Add browser coverage for comment responses observing expiry/concurrent settlement, including both success and refusal as applicable.
- Instruction lesson: normal open-state preservation tests do not cover transitions where the incoming form disappears. Candidate: exercise preservation across terminal-state responses, not only same-state swaps. Cause otherwise unknown.
- Resolution history: 2026-10-25 open at R014.

- Resolution update 2026-10-25 / R015 at `f0ed97efaa607c1efb2cf5077f69fc40106355e9387ae7b9b100020921698785`: **F034 fixed**. Comment beforeSwap captures latest live raw data/feedback when incoming closed markup removes the form, renders it as escaped read-only recovery text, and leaves settlement disabled. Independent rendered browser verifies edits made after request start and hostile text; successful own submission has no recovery panel.

## WF-P09-F035 — Source-message link cannot reach a message outside the newest history window

- Identity: 2026-10-25; implement-astra-reviewed-2.2; Reviewer B (requested Astra/medium; runtime model not independently exposed); phase p09; worktree /Users/mac02/dev/openbot, workflows; base/HEAD da868710d3b235d49856e3e481f8f3735f6b3e56; artifact 8cb2171b82b9e72e6d5a69813b81066dc7012e0fc9e3674815221f338e794b5b.
- Context: [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); B3 whole-batch review WF-P09-R014. Actionable defect; **Low**; context/navigation.
- Evidence: new `src/adapters/http/pages/workflow_decision.rs:94-109` labels a URL as Source message but supplies only a fragment `#message-runs-{id}` to the ordinary thread page. `src/adapters/http/routes/ui.rs:301` loads ordinary thread history; `src/adapters/persistence/thread/views.rs:273-292` restricts that history to the newest THREAD_HISTORY_LIMIT (200). A trigger followed by 200 newer messages has no anchor in the destination DOM and no message-id parameter reaches the server, so clicking the source link opens unrelated recent content. The mailbox additionally unconditionally scrolls to newest on load (`pages/mailbox.rs:497-523`); exact retained-anchor scrolling also needs verification. The absent-anchor case is established by code-path inspection, not a live browser test.
- Required behavior: provide an authorized destination that actually renders the identified source message, including beyond the current window, with correct focus/scroll. Preserve the bounded ordinary conversation read. Add a navigation case with a source older than the newest 200 and a retained non-newest source.
- Instruction lesson: a syntactically exact fragment is insufficient unless the destination loads that identity. Candidate: validate exact-context links against destination pagination/window and scrolling. Cause otherwise unknown.
- Resolution history: 2026-10-25 open at R014. This finding is on the newly introduced B3 context link; R012's bounded conversation activity behavior remains accepted.

- Resolution update 2026-10-25 / R015 at `f0ed97efaa607c1efb2cf5077f69fc40106355e9387ae7b9b100020921698785`: **F035 fixed**. Exact-message destination authorizes company/channel/thread and reads one canonical message independently of newest200, with authorized run links and focus. Independent DB case and browser old/retained cases pass. Newly exposed component integration issue tracked separately as F036.

## WF-P09-F036 — Exact-source page renders mailbox controls without their targets

- Identity: 2026-10-25; implement-astra-reviewed-2.2; Reviewer B (requested Astra/medium, runtime identity not independently exposed); p09 B3 R014 correction recheck / WF-P09-R015; worktree /Users/mac02/dev/openbot, workflows; base/HEAD da868710d3b235d49856e3e481f8f3735f6b3e56; artifact `f0ed97efaa607c1efb2cf5077f69fc40106355e9387ae7b9b100020921698785`; [TASK.md](TASK.md), [REVIEW.md](REVIEW.md).
- Classification: actionable defect; **Low**; UI integration.
- Evidence: `pages/workflow_decision.rs:211-236` reuses `message_bubble_chat` outside its mailbox container. That component always renders an ids button (`pages/mailbox.rs:2509-2520`) targeting `#message-diagnostics-body` and opening `#message-diagnostics`; both exist only in the mailbox message pane (`:2289-2292`) and are absent on the new page. Clicking ids on either exact-source fixture emits `htmx:targetError` and does nothing. [Browser probe](logs/r015-source-controls-probe.mjs), [result](logs/r015-source-controls-probe.log): 1/1 passes defect assertion for old and retained sources. Inspection also finds active internal-note correction/removal controls target absent `#detail-pane` (`mailbox.rs:2424-2443`) when this generic authorized route displays a note; that variant was not browser-executed.
- Required behavior: every control rendered on the standalone page must have a functioning destination, or the standalone read renderer must omit controls it cannot support. Test actual source-page controls, not only body/focus. No change to authorization or ordinary mailbox behavior is required.
- Instruction lesson: reusing a component also imports its target/dialog/container dependencies. Candidate: inventory and exercise embedded actions when mounting existing components outside their original container. Cause otherwise unknown.
- Resolution history: open 2026-10-25 at R015. F034/F035 are fixed; this is newly affected integration from their correction.

- Resolution update 2026-10-25 / WF-P09-R016 at `44077e91138695e2b1d48eda0b50ae1617bf451078c4c6f79303b7f5bd9da575`: **F036 fixed**. Explicit read-only message rendering omits mailbox diagnostic/note actions while preserving ordinary links, content, inactive-note status and tombstones. Existing mailbox wrapper selects unchanged Mailbox behavior. Independent real-DB old/retained/active-note rendering, ordinary control assertions, page tests and browser checks pass. Remaining attachment/download and authorized run links have full-navigation destinations; latest-conversation link browser navigation passes. No residual finding.


## WF-P09-F037 — Sample textarea drops an initial newline on response

- Identity: 2026-10-25; implement-astra-reviewed-2.2; Reviewer B (requested Astra/medium; runtime identity not independently exposed); p09 B4 whole batch / WF-P09-R017; /Users/mac02/dev/openbot workflows; base/HEAD da868710d3b235d49856e3e481f8f3735f6b3e56; artifact `c09931634e15711cb7a01c585dd8dc0409b7d89c3ecc7556c96888c34e0bc7c4`; [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); accepted P09-D1 r1.
- Classification: actionable defect; **Low**; raw-input preservation.
- Evidence: `src/adapters/http/pages/workflow_authoring/sample.rs:40-42` inserts raw input immediately after the textarea opening tag. HTML parsing removes the first LF after that tag. A submitted `\n{invalid JSON` returns as `{invalid JSON`; the displayed parser diagnostic refers to line2 while the returned editor text now starts at line1. Repeated responses remove further leading newlines. All three sample textareas share this template. [Browser probe](logs/r017-newline-probe.mjs), [result](logs/r017-newline-probe.log) extracts the exact template and proves the parser behavior; this is a renderer-level probe, not a live HTTP journey.
- Required behavior: preserve exact raw textarea input including initial newlines on success/refusal, with browser regression coverage. Use the established repository textarea convention. P09-D1 explicitly requires raw invalid input retained.
- Instruction lesson: recurrence of the same HTML parsing mechanism as F005/F021; newly added textarea templates need the same invariant. No source guidance edited.
- Resolution history: open 2026-10-25 R017.

- Resolution update 2026-10-25 / R018 at `450480d565582b0b2e03a867ee624a2e5b5c3e017644951b558807482fd5a00b`: **F037 fixed**. Template prepends the sacrificial LF; independent actual-template browser checks preserve one/multiple initial newlines for input/params/scripts and no-leading-LF content. Sample progress protocol and focused core/HTTP tests pass.

## WF-P09-F038 — Accepted sample capability-boundary regression test is absent

- Identity: 2026-10-25; implement-astra-reviewed-2.2; Reviewer B (requested Astra/medium; runtime identity not independently exposed); p09 B4 whole batch / WF-P09-R017; /Users/mac02/dev/openbot workflows; base/HEAD da868710d3b235d49856e3e481f8f3735f6b3e56; artifact `c09931634e15711cb7a01c585dd8dc0409b7d89c3ecc7556c96888c34e0bc7c4`; [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); accepted P09-D1 r1.
- Classification: actionable verification gap; **Low**.
- Evidence: accepted `logs/p09-d1-r1.md` Verification explicitly promises capability tests asserting the sample module never imports production effect adapters. The complete new sample tests, HTTP sample tests and CI delta contain no capability/dependency guard. Current source inspection confirms the evaluator has only inert bundle/data/counters and no production service handle; existing count and loopback tests pass. Those observations do not implement the agreed enduring regression check across every evaluator/child/direct-tool module.
- Required behavior: add a CI-executed boundary assertion covering the production sample module tree (with explicit test-code handling), or an equally durable enforceable dependency boundary demonstrating it cannot acquire production effect capabilities. Keep behavioral zero-effect tests; a source/import guard supplements them.
- Instruction lesson: track accepted architecture verification promises explicitly into the implementation checklist. No current production-effect leak claimed. Cause unknown.
- Resolution history: open 2026-10-25 R017. R013 approved the design/test obligation, not its later fulfillment; this is an implementation verification omission, not a newly rejected architecture.

- Resolution update 2026-10-25 / R018 at `450480d565582b0b2e03a867ee624a2e5b5c3e017644951b558807482fd5a00b`: **F038 residual open (Low)**. CI now runs the new recursive source guard and its negative-fixture tests, and current production sources pass. However, `scripts/workflow-sample-boundary.py:91` matches only paths beginning without a preceding colon, so valid absolute paths beginning `::` are silently skipped. The guard accepts `fn run(){::std::process::Command::new("curl");}`, `fn run(){::std::fs::read("credentials");}`, and `fn run(){::reqwest::blocking::get("https://example.test");}` with zero violations. Reviewer temp-module probe: [r018-boundary-probe.log](logs/r018-boundary-probe.log). This is a source-check test only; no credential read, subprocess or network request executed. Handle rooted qualified paths (and Rust path token spacing) and include negative fixtures for those spellings in current/new modules. The current evaluator still has no observed effect capability; this is a regression-guard coverage defect, not an effect leak in production code. No new finding ID because the same accepted guard obligation remains incomplete.

- Resolution update 2026-10-25 / R019 at `76d100c63dd8735808ebd7b0c84ee0cbbb4e7932818e28575dc55f0e3030c3ea`: **F038 fixed**. Qualified paths are tokenized, including rooted and whitespace/comment-separated paths, then normalized before the allowlist. Missing production module fails closed. Independently ran recursive guard and3fixture tests; repeated original three forbidden rooted-path probes plus commented/space variant, all rejected (logs/r019-boundary-probe.log). CI wiring retained. This guard supplements previously reviewed behavioral isolation tests; it is not a claim of a formal Rust capability type system.
