# P09 task — Authoring and operations UI

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Identity

- Phase `p09`; workflow `implement-opus-reviewed-2_2`; records
  `/Users/mac02/dev/openbot/plan/workflow-foundation/phases/p09/docs/` (this directory).
- Root = Author A: this Claude Code session (Opus 5.5 / xhigh, confirmed by the user in the
  request "you are Opus/xhigh"; `~/.claude/settings.json` `modelSettings.claude-opus-5-5.effortLevel
  = xhigh`). Reviewer B: spawned at the first review (general-purpose, opus, xhigh, fresh context);
  agent ID recorded below when spawned.
- Worktree `/Users/mac02/dev/openbot`, branch `workflows`, HEAD `da86871`.
- **Starting artifact:** HEAD `da86871` plus the uncommitted p08 working tree, digest
  `00ec454fd48fd7bc7600589474672feb5f76553e437e4f0d3333f2ec595b6829` (= p08 B4d R032 PASS
  artifact). Manifest `logs/start-snapshot.txt`; per-file hashes `logs/start-filehashes.txt`
  (556 paths) for p09 deltas.
- **Preserved pre-existing work:** every p08 change in that tree (staged deletions included). p09
  never stashes, resets, commits or reshapes it.
- **Snapshot exclusions (exact):** `plan/workflow-foundation/phases/p09/docs
  plan/workflow-foundation/phases/p08/docs plan/workflow-foundation/COORDINATION.md marketing
  website` (p09 and p08 workflow records, the mutable coordination file, and the user's unrelated
  marketing/website trees, as p08 excluded them).
- Compaction: `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=75` in `.claude/settings.local.json` env, observed
  in this session's environment (`echo $CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` → 75).
- Tooling: `graft` CLI/MCP unavailable (executable not found; no `graft/` directory) — discovery
  uses grep/read, as in p08. No `sqlx-cli`: new SQL uses runtime `sqlx::query*` functions, not
  the `query!` macros, so `.sqlx/` needs no regeneration. No `node_modules`: Tailwind cannot be
  rebuilt locally; new markup uses utilities already compiled into `assets/app.css` (checked by
  a class scan per batch), and `npm run check:css` is owed at the gate with the user's go-ahead.

## Prerequisites

- p01–p07 accepted and committed (`bd2ef36`, `da86871`).
- **p08:** every batch accepted in phase (B1…B4d; last R032 PASS at `00ec454f…6829`); its phase
  gate checks (full `cargo test --lib`, `scripts/stack-budget.sh`, `npm run check:css`) are owed
  by the user's choice ("Not now"); nothing committed. p09 builds on that accepted artifact; a
  later p08 gate fix would change the p09 base and need an affected recheck.

## Resources

- Phase PostgreSQL 18.6 cluster `/private/tmp/workflow09-20261024`, `127.0.0.1:55490`, superuser
  `workflow09` (trust, loopback only, max_connections 300); databases `workflow09` (dev) and
  `workflow09_test` (recreate after every init-schema edit).
- `PATH=/opt/homebrew/opt/postgresql@18/bin:$PATH`,
  `DATABASE_URL=postgres://workflow09@127.0.0.1:55490/workflow09`, `SQLX_OFFLINE=true`,
  `CARGO_INCREMENTAL=0`; build cache: the worktree's `target/` (phases run sequentially).
- Restart: `pg_ctl -D /private/tmp/workflow09-20261024 -l /private/tmp/workflow09-20261024/server.log start`.
- Never touch the user's 5432 server, p07's 55470 or p08's 55480 clusters.

## Original requirements

- Plan: `plan/workflow-foundation/09-authoring-and-operations-ui.md` (authoritative), EXPANSION rows
  09.1–09.4, README, REPLACEMENT-MAP (rows and steps naming 09: projections retargeted onto scoped
  run/step/decision/delivery reads; the 09 sample executor is a separate isolated mode),
  NOT_PLANNED (memory out of release; email-only approvers removed).
- **06-D5 (user decision 2026-10-19) supersedes the plan's profile wording.** "choosing a profile is
  optional … whether they came from agent defaults or an explicit profile" and "grant ceilings":
  there are no capability profiles or workflow ceilings; `agent.run` uses exactly the agent's saved
  tools and skills, frozen at publication, with current revocation enforced. p09 shows "Uses the
  agent's tools and skills" with the frozen list, and its source "agent (frozen at publication)";
  the agent's grants are its saved tools/skills and its workflow-tool grants (WF-C003).
- p08 candidates handed to p09 (p08 TASK): F093.2 (removing the only reviewer of an open decision
  leaves it unsettleable: warn in the removal confirmation or offer reassignment), F098.1 (mailbox
  avatar/attribution for workflow replies), F098.2 (the agent pane's "used by" as binding usage),
  F100 (binding status checked under the channel gate: candidate), attention items for runs and
  bindings get links ("no link for runs or bindings until p09"), schedule occurrences link their
  run, the mailbox keeps `run_ids` for p09 links.
- Instructions: `AGENTS.md`, `src/AGENTS.md`, `src/application/AGENTS.md`,
  `src/adapters/persistence/AGENTS.md`, `src/adapters/http/AGENTS.md`,
  `src/adapters/http/pages/AGENTS.md`, `src/infra/AGENTS.md`.

### Acceptance criteria (plan §Acceptance + body)

1. A company admin can copy, edit, validate, test, publish, and activate a reviewed support
   workflow.
2. A reviewer can comment, request a revision with feedback, inspect the next draft, and accept it.
3. Readers can distinguish two simultaneous runs on one thread and follow their separate replies.
4. Tests cover cross-company IDs, private-thread visibility, hostile rendered content, and CSRF
   boundaries.
5. SSE is a wakeup followed by reconciliation; tests cover reconnect, event-before-subscribe, and
   stale partial-page responses; the event-stream owner stays mounted across swaps.
6. Accepted actions give progress feedback; both themes and keyboard interaction are verified.
7. Sample execution proves zero production effects, including through direct agent tools and
   children.
8. (body, authoring) Library: shipped template, company copy, new draft. Editor: YAML source,
   source-located validation, step catalogue reference, resource requirements, input/output
   schemas, read-only graph from the same compiled representation. Versions: compare, publish an
   immutable version; publishing is distinct from activation; show which channels/schedules stay on
   older versions. Bindings: version, resources, schema-driven parameters; readiness validated
   before activation; paused/unconfigured bindings shown plainly, no silent default workflow.
   MCP steps: a company MCP connection per declared slot, the selected tool's frozen schema,
   argument bindings validated; run details show tool/connection identity, bounded results and
   reconciliation status; credentials never in source or previews. Agents: instructions, model
   settings, skills, grants; an explicit assistant-channel action, no implicit channel creation.
   Capabilities per 06-D5 (above). Optimistic revisions detect concurrent edits; unsaved source
   survives a failed validation. Company admins author/publish/configure; membership and channel
   rules govern who starts/inspects runs; operator template management grants no company access.
9. (body, runs) Run state, triggering message, published version, graph, chronological timeline;
   steps expand to authorized input/output, context sources, selected skills/tools, model usage,
   action receipts, child runs, retry/reconciliation history; worker attempts under diagnostics.
   Conversation entries link to their producing run; several active/waiting runs shown
   independently; reply/review links name exact message/artifact/decision identities. Explicit
   cancel, safe retry and reconciliation controls with visible status; queued delivery, provider
   acceptance, human waiting, failure and uncertainty distinguishable; retries never pose as new
   revisions; cancellation explains already-accepted effects.
10. (body, queue) One queue for business decisions and protected-action approvals with clear subject
    labels. Detail: reviewed artifact version, context, discussion, deadline, choice buttons,
    feedback field, schema-driven editable data. "Add comment" leaves it open; "Submit decision"
    validates choice-specific feedback and data before settling. Draft/revision history with the
    feedback that produced each revision. Closed/stale decisions readable, no new outcome; late
    links explain which decision was already settled.
11. (body, samples) Same compiler/engine in an isolated mode enforced below the UI; scripted
    model/decision responses; intercepted actions; fixtures for external reads; no production
    provider contact by default; intended messages/writes captured for inspection; never send
    customer messages, execute write tools, or persist production memory. Sample records and
    resource bindings isolated from production admission; a sample never becomes production by
    resuming; production starts a new authorized run.

## Coordination

- r12 (2026-10-25, p09 as writer): WF-C003 response, sha256 `05eb8e6e…2c69`
  (`logs/coordination-r12.md`).
- Authority `/Users/mac02/dev/openbot/plan/workflow-foundation/COORDINATION.md`; checked r10 at
  startup (2026-10-24, sha256 `1df326af740df24e626c9d5680d15a6df8eb1a2756f0fff0582cc605ac93d9ae`,
  copy `logs/coordination-r10.md`). Writer was the p08 root session; that session has ended
  (`ListAgents`: no other session) and p08 is paused at its phase gate with no workers, so p09
  root takes over as writer in r11 (explicit handoff recorded there).
- Applicable notices and dispositions:
  - **WF-C002 r1** (child runs carry the parent's binding for provenance; binding run lists filter
    or label children; run inspection shows lineage and `rerun_of`) — acknowledged; B2.
  - **WF-C003 r1** (admin UI over workflow-tool routes and agent grants with P07-Q2 semantics; warn
    on reuse of a revoked/re-pointed name; tool-call children in run inspection with the saved call
    id; workflow-tool children in sample execution) — acknowledged; UI in B1b, inspection in B2,
    samples in B4.
  - **WF-C004 r2** (every Postgres pool through `discard_leaked_transactions`; documented
    exemptions) — acknowledged; applies to any pool p09 builds (none planned).
  - **WF-C005 r1** (listing `{items,next}` paging over partly hidden pages; reconciliation is an
    annotation, not a resume; 409/503 client rules; child runs controlled via the parent; decision
    page `/ui/workflow-decisions/{id}` is the notice link target) — acknowledged; B2/B3.
  - WF-C001 r2 targets p08 only (not applicable).

## Batches

Each batch is reviewed once as a whole (no intermediate reviews), then accepted before the next
builds on it. Focused checks are per batch; the full suite is the final gate (user go-ahead).

- **B1a — Library, editor, catalogue, versions, publish (09.1 part 1).** `/ui/workflows`
  workspace (rail entry, owner/admin), definitions list with paging, new draft, shipped template
  copy (keyed), editor (title/description/YAML, expected-revision save, conflict keeps the unsaved
  source, save-and-validate with source-located diagnostics), validation preview from the compiled
  bundle (read-only graph, steps and types, resource requirements, input/params/output schemas,
  agent capabilities per 06-D5), step catalogue reference from the registry, versions list and
  compare (line diff of immutable sources, draft vs version), publish with HTTP/MCP slot selections
  (frozen tool schema shown, no credentials), and which bindings stay on older versions. UI routes
  share command code with the JSON routes (no parallel business logic).
- **B1b — Bindings, MCP slots, agents, workflow tools (09.1 part 2).** Binding create/configure
  (association, version, schema-driven params, resources per slot, trigger, run-as), readiness
  shown before activation, activate/deactivate, paused/unconfigured status on the workflow and the
  channel; agent pane: workflow-tool grants (WF-C003) and "used by" (F098.2); workflow-tool admin
  (register/revoke, reuse warning).
- **B2 — Runs and conversations (09.2).** Runs workspace (paging per WF-C005, children/reruns
  labelled per WF-C002), run detail (state, trigger message, version, graph with progress,
  timeline, step expansion, model usage, receipts, children incl. tool-call children, controls
  history, diagnostics), controls (cancel with accepted-effects explanation, safe retry, rerun,
  reconciliation annotation; 409/503 handling), mailbox links to producing runs, independent runs
  per thread, attention/schedule links, SSE wakeup+reconcile with the required race tests.
- **B3 — Human work queue (09.3).** Subject-labelled queue on `/ui/work`; decision detail
  (artifact version, context, discussion, deadline, choice buttons, choice-specific feedback,
  schema-driven data), comment vs submit, revision history with feedback, closed/stale/late
  handling; F093.2.
- **B4 — Sample execution (09.4).** Architecture gate **P09-D1** (sample isolation: schema,
  enforcement points below the UI, scripted model/decision providers, intercepted effects,
  fixtures, children and direct tools, no promotion) before implementation; then the sample UI and
  the zero-production-effects proofs.
- **Final:** acceptance-journey tests (criteria 1–3, 7), phase review of combined effects, then ask
  the user about the full suite.

## Decisions and deviations

- DV-P09-1: capability profiles/ceilings are not built (06-D5 supersedes the plan bullet; see
  "Original requirements").
- DV-P09-2 (B1a): the registry's authoring notes are corrected because the editor's catalogue now
  shows them to authors: "Execution unavailable." is removed from every kind that executes (all
  but `memory.*`, which now say "Not available in this release … publication refuses it"), and the
  `message.*` note's "at most 128 scoped destinations" becomes 20 (`MAX_MESSAGE_DESTINATIONS`). The
  registry test now pins both (it previously pinned the stale wording).
- DV-P09-3 (B1a): a publication selection with exactly one candidate connection is pre-selected in
  the editor; it stays a visible, changeable select, and nothing is frozen until validation or
  publication sends it.
- DV-P09-4 (B1a): the workflow pane has Editor and "Versions and usage" tabs; binding
  configuration is B1b. Usage (bindings with channel names and schedules, newest vs older
  version, paused) is in the versions tab; bindings on channels the viewer cannot see are counted,
  not shown (same `Visibility` rule as the JSON listing).
- DV-P09-5 (B1a): agent steps name agents by **slug** (`derive_on` resolves `agents.slug`), so the
  reference lists handles, flags agents without an explicit model, and a template copy keeps the
  template's handle (`assistant`) for the author to edit.
- DV-P09-6 (B1a): a refused validation after a successful save says both ("Saved revision N, but it
  could not be validated: …"); a capture `NotFound` (connection disabled/not ready, tool no longer
  approved) is shown as an actionable refusal in the editor instead of a not-found page.

## B1a implementation log (2026-10-24)

Scope delivered (TASK "Batches" B1a):
- Application (pure): `application/workflow/authoring/` — `workflow_graph` (longest-path layered
  DAG of the compiled definition, end node below every step, error edges), `line_diff` (prefix/
  suffix trim + LCS bounded by `MAX_DIFF_CELLS` = 1,000,000 cells, else two blocks),
  `publication_needs` (literal `mcp.call` slot+tool and `tool.call` names), `catalogue_reference`
  (every registered kind but the refused ones, each from a compiling registry example).
- Read port `AuthoringReads` (`application/workflow/reads.rs`), Postgres impl
  `persistence/workflow/authoring_reads.rs`: `version_source`, `workflow_usage` (bindings ≤ 200,
  channel names, ≤ 20 schedules per binding via LATERAL), `publication_options` (approved MCP and
  HTTP operations ≤ 200 each with frozen snapshots; labels are MCP slug / HTTP origin, never an
  endpoint or credential). Engine accessors `authoring_reads()`, `step_catalogue()` (LazyLock).
- JSON routes refactored, behaviour preserved: `definitions::{save_draft, validate_draft,
  publish_draft, archive_draft, copy_template_draft}` are the shared commands both the JSON
  handlers and the UI call (no parallel business logic). `bindings::Visibility` shared.
- UI routes `routes/workflows/ui/{mod,authoring}.rs` (merged into `workflows::router`, under its
  2 MiB body limit): `GET /ui/workflows`, `/list`, `/new`, `/catalogue`, `/{id}`, `/{id}/compare`;
  `POST /new`, `/templates/{t}/copy`, `/{id}/save` (intent save|validate), `/{id}/publish`,
  `/{id}/archive`. Company scope via `load_managed_company`; every command authorizes again.
  Keyed create (`keyed_id("workflow.ui_draft")`) and copy; expected-revision save with conflict
  banner (keeps typed text, shows a diff of the newer revision, next save sends the current
  revision); validation compiles the saved revision with the selections; publish from the
  validation preview sends exactly the validated revision and selections.
- Pages `pages/workflow_authoring/` (workspace, list with "Load more", new pane, pane with tabs,
  editor with located diagnostic + "Go to line" (`WORKFLOW_EDITOR_SCRIPT`), selections with every
  candidate's frozen contract, preview (graph SVG + accessible list, steps, resources, schemas,
  agents' frozen tools/skills per 06-D5, frozen tools, publish form), versions (list, compare,
  usage), catalogue reference with agent handles). New rail entry `UiSection::Workflows`
  (`Icon::Workflow`), visible to owners/admins like Agents. Theme tokens only (SVG uses
  `var(--color-*)`).
- Plan file 09 now carries the standard no-backward-compatibility note (it is already listed in
  `NO_BACKWARD_COMPATIBILITY_NOTES`).

Checks (cluster 55490, `SQLX_OFFLINE=true CARGO_INCREMENTAL=0`,
`DATABASE_URL=postgres://workflow09@127.0.0.1:55490/workflow09`; no schema edit):
- `cargo test --lib application::workflow::authoring` — 6 passed.
- `cargo test --lib routes::workflows::ui` — 15 passed (after two fixture fixes and DV-P09-6).
- Broad focused run `cargo test --lib -- adapters::http application::workflow::registry
  application::workflow::authoring adapters::persistence::workflow::tests` — 1310 passed,
  1 failed: `pages::tests::rendered_markup_carries_no_inline_javascript` flagged the new test
  file's literal hostile `<script>` (`logs/b1a-focused-1.log`); fixed by building it from parts.
- Final run `cargo test --lib -- pages::tests routes::workflows application::workflow::authoring
  application::workflow::registry` — 188 passed, 0 failed, 1 ignored (pre-existing)
  (`logs/b1a-focused-2.log`).
- `cargo fmt --check` clean; `cargo clippy --all-targets -- -D warnings` clean
  (`logs/b1a-clippy.log`).
- CSS: `python3 logs/css-class-check.py src/adapters/http/pages/workflow_authoring/*.rs` — 0
  missing (htmx-provided `htmx-indicator` allowed); dynamic class strings checked by hand
  (all present in `assets/app.css`). `npm run check:css` unavailable (no node_modules).
- Not run here: browser verification in both themes and keyboard walk-through (planned once B1b
  lands, with the app served against the phase cluster).

Freeze: base/HEAD `da86871`, digest
`62a6c35e187548f9e6fbd444c9482f4b3e378764cef3dd2dc0a19f561cf84e50` (exclusions as in Identity;
manifest `logs/b1a-snapshot.txt`). p09-only delta: `logs/b1a-files.txt` (31 files), exact diff
`logs/b1a.diff` (bases: HEAD, new, or `logs/start-base/` — 13 pre-p09 copies of files p08 had
changed, each hash-verified against `logs/start-filehashes.txt`); tree copy `logs/tree-b1a/`,
hashes `logs/b1a-filehashes.txt`. Coordination r11 (`3e8501b4…62f5`) unchanged.

## B1a corrections for WF-P09-R001 (2026-10-24)

R001: CHANGES REQUIRED — F001–F003 Medium, F004–F009 Low (actionable), F010/F011 optional,
F012 tracked verification gap. All actionable findings fixed in one round; F010 and F011 adopted.

- **F001:** the preview lists every frozen agent once (name, handle, model, tools, skills, source
  "the agent, frozen at publication") and each model step says which: a literal handle, or "the
  agent named by `/params/<p>` at run time, one of @a, @b"; a literal handle missing from the
  frozen agents is shown as an error row instead of being dropped. Test
  `a_parameter_selected_agent_step_previews_every_frozen_candidate`.
- **F002:** `AuthoringReads::publication_options(company, mcp_tools, http_tools)` reads only the
  needed names (`name = ANY($2)`, ≤ `MAX_OPTION_NAMES` = 256 per kind, else 400), ranked per name
  with `row_number()`, at most `MAX_CONNECTIONS_PER_TOOL` = 20 per name, one past the bound
  marking `mcp_cut`/`http_cut`; the editor says "More connections offer this tool than the 20
  listed here." Test `the_editor_offers_a_needed_mcp_tool_past_any_company_wide_count_and_cuts_per_tool`
  (202 approvals on one connection, then 21 connections).
- **F003:** the editor form is the dirty-state owner (`data-workflow-editor`, `data-dirty` set on
  input/change); `WORKFLOW_EDITOR_SCRIPT` asks "The editor has changes that are not saved. Discard
  them?" before any htmx request whose target is `#workflow-pane` (tabs, sidebar, New, publish,
  archive) except the reload button (`data-editor-reload`, its own confirm), and a
  `beforeunload` prompt covers full navigations. Requests that fill a region of the pane
  (reference, compare) do not ask. Test
  `the_editor_guards_unsaved_text_and_reports_only_the_work_it_does` (markup and script hooks;
  behaviour itself is part of the browser pass, F012).
- **F004:** the editor's submit buttons own no spinner (the shared `BUTTON_BUSY_SCRIPT` marks the
  pressed one); a status region says "Saving…" or "Saving and validating…" by submitter and
  "Not saved: the request failed. Your text is still here." on an in-place failure; the
  reference body is swapped `outerHTML` (busy state goes with it) and a failed load clears
  `aria-busy`, says so and re-arms the toggle.
- **F005:** a newline follows each `<textarea>` start tag (description and source). Test
  `a_source_and_description_that_start_with_a_newline_keep_it`.
- **F006:** test `an_admin_of_another_company_reaches_no_write_or_version_of_this_one` (own company
  + victim workflow id for save/publish/archive/compare → `NotFound`, victim row unchanged;
  another workflow's version through compare → `NotFound`).
- **F007:** the MCP test now uses a real reviewed connection (`create_mcp_connection` with a
  reviewed discovered tool + `register_mcp_operation`): preselection, no endpoint leak, valid
  preview naming the slot and connection, publish form carrying `mcp:service:lookup`, published
  bundle freezing `lookup` through slot `service`, and a wrong argument as a located diagnostic.
- **F008:** (a) `validated` maps a `Conflict` after the save to `MovedAfterSave` ("Saved revision
  N, but someone else saved revision M before it could be validated."), keeping the typed text
  against M; (b) a publish `Conflict` says "no longer the draft" only when the draft moved (or
  "the workflow is archived"), else "Revision N was not published: <command message>". Tests
  `a_draft_that_moves_after_the_save_keeps_the_text_and_says_both`,
  `a_publish_conflict_that_is_not_a_moved_draft_says_what_the_command_said`.
- **F009:** a save answers with the sidebar out of band (`menu_oob`). Test
  `a_save_refreshes_the_sidebar_and_a_changed_publication_is_flagged`.
- **F010 (adopted):** the preview names the connection each selection freezes ("Connections this
  publish freezes") and carries `previewed_dependencies` = `dependency_digest(bundle)` (sha256 of
  the frozen snapshots and child pins; the content hash cannot be compared because it covers the
  version identity — the nil preview identity differs from the published one). The publish
  banner says when the published version's dependencies differ.
- **F011 (adopted):** `McpSelectionKey { slot: ResourceName, tool: TypeName }`, `http:
  BTreeMap<TypeName, Uuid>` (invalid names select nothing); `OperationRow`/`VersionSourceRow`
  are `sqlx::FromRow` structs.
- **F012:** stays open (browser, both themes, keyboard, `check:css`) for the planned pass after
  B1b; not claimed verified.

Checks: `cargo test --lib routes::workflows::ui` — 23 passed; focused set `cargo test --lib --
adapters::http application::workflow::authoring application::workflow::registry` — 373 passed,
0 failed, 1 ignored (`logs/b1a-r2-focused.log`); fmt check and clippy `-D warnings` clean;
class scan 0 missing.

Freeze: digest `b4dc745de5c69e461189ae0c64ad0d38ec3de67e30eeacc7550c9675e9e2de3c`
(`logs/b1a-snapshot-r2.txt`, same exclusions, HEAD `da86871`). Correction delta against the
R001 artifact: `logs/b1a-r2-files.txt` (7 files), diff `logs/b1a-r2.diff` (base
`logs/tree-b1a/`); tree `logs/tree-b1a-r2/`, hashes `logs/b1a-r2-filehashes.txt`. Coordination
r11 unchanged.

## B1a corrections for WF-P09-R002 (2026-10-24)

R002: not a pass — F003 partly fixed (remains open, Low), new F013, F014, F016 (Low, actionable),
F015 optional (adopted). All fixed in one round.

- **F003 (residual):** `EditorView.unsaved` is set when the editor shows typed text that differs
  from the stored draft (title, description or source — a refused save, a conflict,
  `MovedAfterSave`); the form then renders `data-workflow-editor data-dirty`, so the pane guard
  and `beforeunload` cover server-echoed text too. A stored draft and a successful save render
  clean. Accepting the guard's prompt no longer clears the mark (the swap removes the form; a
  request then refused or failing leaves the text guarded). Assertions in
  `a_concurrent_save_keeps_the_authors_text_and_shows_what_the_other_saved` and
  `the_editor_guards_unsaved_text_and_reports_only_the_work_it_does`.
- **F013:** `Ui::editor_options` (the needs-and-options phase extracted from `editor_body`)
  dedupes each kind's names, reads options for at most `MAX_OPTION_NAMES` (256) of them and
  returns a `too_many_tools` flag; the editor says "This draft names more than 256 tools of one
  kind; connections are offered for the first 256 only, and publication refuses more selections
  than that." The page never fails on the bound. Test
  `a_draft_naming_more_tools_than_are_offered_still_saves_and_renders` (257 literal `tool.call`
  tools: the editor pane renders with the note, and a save answers "Saved revision 2.").
- **F014:** the unsaved-text guard listens on `htmx:confirm`, which htmx fires before
  `htmx:beforeRequest`, so a cancelled request never reaches `BUTTON_BUSY_SCRIPT` (which clears
  only on `htmx:afterRequest`). The script test asserts the listener is on `htmx:confirm`; the
  click behaviour stays in the F012 browser pass.
- **F016:** `preview.rs` `parameter_choices` lists the referenced property's `enum` from the
  definition's `parameter_schema` (the lookup `agent_universe` performs), falling back to "the
  frozen agents" only when there is none. Test
  `a_parameter_selected_step_lists_its_own_parameters_agents` (literal `@x` step + `/params/agent`
  step with enum `[y, z]`: the second says "one of @y, @z").
- **F015 (adopted):** `ui/authoring.rs` split into `ui/authoring/{mod,library,editor,versions}.rs`
  and its tests into `ui/authoring/{tests,editor_tests,publish_tests,access_tests}.rs` (fixture
  and library tests in `tests.rs`, area modules as its children, the routes/workflows pattern).
  Move maps: `logs/b1a-r3-move-map.txt` (route items: 23 same, 14 only `pub(super)` added, 7
  changed — `editor_body` for F003/F013, fmt re-wraps of four signatures, module docs/uses, the
  `mod tests` path), `logs/b1a-r3-test-move-map.txt` (34 same, 4 changed — the two F003/F014
  assertions plus docs/uses — 5 added: the two new tests and three `mod` lines).

Checks: `cargo test --lib routes::workflows::ui` — 25 passed; focused set `cargo test --lib --
adapters::http application::workflow::authoring application::workflow::registry` — 375 passed,
0 failed, 1 ignored (`logs/b1a-r3-focused.log`); fmt check and clippy `--all-targets -D warnings`
clean (`logs/b1a-r3-clippy.log`); class scan over `pages/workflow_authoring/*.rs` 0 missing
(`pages/mailbox.rs` reports `aura`, `aura-holo`, `brand-logo`, identical at HEAD — pre-existing
hook classes, not p09).

Freeze: digest `1484a5bc494ed3c0202e07678f66566fabe31f0f24ea0e5886620254cd6957db`
(`logs/b1a-snapshot-r3.txt`, same exclusions, HEAD `da86871`). Correction delta against the
R002 artifact: `logs/b1a-r3-files.txt` (12 files: 2 page files changed, the old route and test
files removed, 8 split files), diff `logs/b1a-r3.diff` (base `logs/tree-b1a-r2/`); tree
`logs/tree-b1a-r3/`, hashes `logs/b1a-r3-filehashes.txt`; move-map tool `logs/move-map.py`.
Coordination r11 unchanged.

## B1b plan (revised 2026-10-24 after B1a acceptance)

Base: the accepted B1a tree (`logs/b1a-r3-filehashes.txt`, digest `1484a5bc…57db`). UI routes
share command code with the JSON routes, as in B1a.

- **Bindings tab** (`/ui/workflows/{id}?tab=bindings`). It lists every binding of the workflow
  the viewer may see (`workflow_usage` + `Visibility`; hidden ones are counted). Each row shows
  association, version (newest or older, by number), active/paused, message trigger, run-as,
  resources by slot, schedules and the state revision. With no binding it says "Not bound:
  nothing runs this workflow — messages and schedules never fall back to a default workflow."
  - Configure form, one renderer for create and edit:
    - Association: the company, or a channel the viewer can see. Thread bindings stay
      API-only. Association is fixed on edit.
    - Version: published versions of this workflow, newest first. Changing it re-renders the
      form through a GET that includes the form's fields.
    - Params: schema-driven fields from the version's `parameter_schema` — string (a select
      when `enum`), integer/number, boolean, and a JSON field for object/array/untyped. An
      empty optional field is omitted. The server builds the object; `ConfiguredBinding::new`
      validates it.
    - Resources: one select per declared slot by kind. `http` offers HTTP connections (origin
      label), `mcp` MCP connections (name), `messaging` is fixed to the company's resource.
    - Message trigger: none/direct/any/mentioned/mentioned_or_reply, on channels only.
    - Run-as: the owner picks an owner or admin; anyone else keeps the stored value or themselves.
  - Create uses `keyed_id("ui-binding", company, form key)`. A double submit's conflict is
    answered with the list and "This binding was already created."
  - Refusals (readiness, `check_message_trigger`, params diagnostics, run-as) render beside the
    form with every choice kept. A conflict says what changed.
  - Activate/Deactivate send `expected_state_revision`; a readiness refusal on activate is shown
    on that row.
  - `BindingUse` gains `resources` (the current revision's `resources` jsonb).
- **Channel card.** The channel pane lazy-loads "Workflows on this channel" from
  `/ui/workflows/channels/{channel}/bindings`. It shows channel bindings (and API-made thread
  bindings in it) as active or paused, linked to the workflow's bindings tab. With no active
  binding carrying a message trigger it says so plainly: "messages are stored; nothing runs".
  F100 stays a recorded candidate.
- **Agent pane.** Two cards are lazy-loaded from the workflow UI router (owner/admin, like the
  workspace):
  - "Workflow tools": the ordered grants as a one-name-per-line field, with the list it was
    opened with sent as `expected`. A reference shows what each name resolves to: a registered
    workflow tool (workflow and version), a built-in message tool, a provisioning tool (Agent
    Builder only — a grant may keep or drop one, never add it), or otherwise an HTTP operation
    or MCP slot tool that publication resolves against the workflow. On a conflict the text is
    kept and the current list shown. New read: `AuthoringReads::agent_tool_grants`.
  - "Used by" (F098.2): bindings whose bound version froze this agent, directly or through a
    child workflow it pins. This needs gate **P09-B1b-D1** below; hidden channels are counted.
- **Workflow tools admin** (WF-C003), with copy matching the code's P07-Q2 semantics:
  - `/ui/workflows/tools` (a sidebar link) lists registrations: name, workflow, version number,
    params, revision. Each row can re-pin within the same workflow ("published versions keep
    calling the version they froze; versions published from now on freeze version N") or
    revoke, with a confirm.
  - The revoke warning (corrected after WF-P09-F020; see "B1b corrections for WF-P09-R005"):
    revoking stops every published version that froze the tool. Their new runs are refused,
    their bindings can no longer be configured or activated, and their agents' next calls are
    refused. A call already in progress still returns its result, and re-registering the name
    for the same workflow restores them (P07-D2 rev 4 §D2.2, §D2.7).
  - The versions tab gains "Use as a workflow tool" per version (name + static params).
  - If the name already exists for another workflow, the form comes back with the current
    registration, its expected revision and the warning: re-pointing refuses every published
    version that froze the old workflow until it is republished. The author confirms
    explicitly. Re-registering a name for the same workflow says the old pins resume.
  - **DV-P09-7:** WF-C003's "the old pins stay refused" holds only when a name is reused for a
    different workflow. `ToolCheck::Run` admits a pin while its name is registered for the same
    workflow, so a revoked name re-registered for that workflow lets old pins run again. The UI
    states the code's behaviour; record this in the coordination response.
- **Tests:**
  - Tenancy: cross-company binding, channel, agent and tool ids; a member; a channel the admin
    cannot see.
  - Revision conflicts on configure, activate and grant; a readiness refusal kept in the form.
  - Hostile names in every sink; the CSRF route list extended.
  - Schema-driven params round trip; the reuse and re-point warnings.
  - "Used by" through a child workflow.
- **Checks:**
  - Focused: `cargo test --lib routes::workflows::ui`.
  - The touched JSON routes' tests (`routes::workflows`).
  - `adapters::persistence::workflow` tests for the new reads and the publish insert.
  - Pages tests, fmt, clippy, the class scan.
  - After B1b, the browser pass (F012) over B1a and B1b.

### Gate P09-B1b-D1 — frozen agent ids stored with each version (rev 1, 2026-10-24)

- **Decision:**
  - `workflow_versions` gains `agent_ids uuid[] NOT NULL`, edited in place in the init
    migration (no backward compatibility; the phase databases are reset).
  - The publish insert writes `PublishedBundle::frozen_agent_ids()`: the distinct `agent_id`
    of `snapshots().agents`, and recursively of every `children()` bundle (step children and
    workflow-tool pins, 07-D2), in sorted order.
  - The read `AuthoringReads::agent_usage(company, agent)` joins `workflow_bindings` → current
    `workflow_binding_revisions.version_id` → `workflow_versions` where
    `$agent = ANY(agent_ids)`. It returns at most 50 bindings plus a `more` flag, with
    workflow title, version number, newest flag, association, channel name and active state.
- **Invariants:**
  - The column is derived only from the bundle stored in the same row, written once by the
    single insert, and never updated (versions are immutable).
  - It names exactly the agents whose frozen snapshots a run of that version or its pinned
    children can use. Agent ids are trusted identities in the bundle, never model input.
  - The read is company-scoped and authorizes the company like the other authoring reads. The
    route applies `Visibility` to channel associations.
- **Why not the alternatives:**
  - Decoding bundles per view is unbounded: there is no per-company binding cap or bundle cache,
    and a bundle may be 4 MiB.
  - Parsing stored sources misses parameter-selected agents, and an agent's slug can change after
    publication.
- **Failure modes:**
  - The column is not consulted by any execution, admission or authorization path, so a wrong
    list can only mislabel the pane.
  - Deleting an agent leaves its id in old rows; the read joins nothing to `agents`, and the pane
    is only reached for an existing agent.
  - An insert that omits the column fails (NOT NULL).
- **Affected contracts:** the `workflow_versions` DDL; the publish insert
  (`persistence/workflow/definitions.rs`); `PublishedBundle` (one new read-only method). Fixtures
  and SQL test files inserting `workflow_versions` directly: only
  `persistence/workflow/tests.rs:407` (a cross-company copy that must fail on the foreign key) —
  it copies `agent_ids` too, so it still fails on the foreign key rather than on NOT NULL. No
  `.sql` fixture inserts versions. No default: a hand-written insert cannot skip the column.
- **Verification:**
  - Persistence test: publish a parent pinning a child whose agent differs, and assert that the
    stored `agent_ids` hold both.
  - Route test: "used by" lists a binding whose version uses the agent only through a child; a
    binding on a hidden channel is counted; another company's agent id answers NotFound.
- **Gate result:** WF-P09-R004 **PASS** for "P09-B1b-D1 rev 1" (2026-10-24). B's optional
  suggestions are adopted as implementation details. They leave the decision text and the stored
  set unchanged, so this stays rev 1; the B1b batch review checks them.
  - F017: the walk is deduplicated by version id (a diamond of children is visited once), with a
    diamond test.
  - F018:
    - `ORDER BY binding.id` under the limit.
    - The hidden-count copy says it covers the listed rows.
    - The route looks up the agent company-scoped first (NotFound for another company's agent).
    - A CHECK that `agent_ids` holds no NULL element.
    - Invariant and column comment: "display index only; no execution, admission or
      authorization path reads it".
    - Tests: a binding reconfigured off the version drops out; a version with no model step
      stores `{}`.
- **DV-P09-7 confirmed by B:** the code matches accepted P07-D2 rev 4 §D2.2. Re-registering a
  name for the same workflow re-enables old pins; re-pointing it to, or reusing it for, a
  different workflow does not. WF-C003 r1's "old pins stay refused" is the imprecise wording.
  Cite §D2.2 in the coordination response.

## B1b implementation log (2026-10-24)

The gate P09-B1b-D1 rev 1 PASS came first (WF-P09-R004); B's F017 and F018 are adopted as
implementation details (see the gate).

- **Reads** (`AuthoringReads`, `persistence/workflow/authoring_reads.rs`):
  - `binding_use` reads one binding (a configure form past the 200-row listing still works).
  - `channel_usage`; `agent_usage`, which reads `agent_ids`, at most 50, ordered by binding id.
  - `binding_options`: channels, HTTP and MCP connections and people, at most 200 each, with a
    `truncated` flag.
  - `agent_tool_grants` and `tool_targets`.
  - `BindingUse` gains `workflow_title` and `resources`. A shared `binding_uses(rows, limit)`
    serves every listing.
- **Schema (gate):** `workflow_versions.agent_ids uuid[] NOT NULL`, with a no-NULL-element
  CHECK and a "display index only" column comment.
  - `PublishedBundle::frozen_agent_ids()` walks the bundle and its children, each version
    visited once.
  - The publish insert binds it; the one direct test insert copies it.
  - The phase databases were recreated (`workflow09`, `workflow09_test`).
- **Shared commands:** `bindings::{BindingRequest, configure_binding, set_binding_activity}`
  carry what the JSON handlers did before (behaviour preserved). `infra::WorkflowEngine::
  selectable_bundle` loads a selectable version for the form.
- **Schema-driven params** (`application/workflow/authoring/params.rs`, pure, unit-tested):
  - `parameter_fields` gives text (maxLength), integer, number, boolean (unset/true/false), a
    string `enum` as a select, and JSON for anything else.
  - `field_value` gives each field's starting value from stored params.
  - `parameters_from_fields` leaves an empty field out and says which field does not parse.
  - `parameters_from_json` takes the whole object when the schema has no properties or more
    than 64.
- **Bindings tab** (`pages/workflow_authoring/bindings.rs`, `routes/.../ui/authoring/bindings.rs`):
  - `WorkflowTab::Bindings` lists bindings and states "Not bound: nothing runs this workflow …".
  - Create is keyed `keyed_id("workflow.ui_binding", company, form_key)`; a double submit says
    "already created". Configure carries `expected_state_revision`.
  - A refusal or conflict keeps every choice. Activate and deactivate refusals show on the row.
  - Changing the version re-renders the form with `hx-include="closest form"`.
  - An archived workflow's active binding can still be deactivated, not activated or configured.
- **Channel card:** the channel pane lazy-loads `/ui/workflows/channels/{channel}/bindings`
  ("Workflows on this channel", and whether an active binding answers its messages). A channel
  the viewer cannot see is NotFound.
- **Workflow tools:**
  - A sidebar "Tools" button (and `?tools=1`) opens the registrations pane with revoke; the
    confirm carries `REVOKE_WARNING`.
  - The versions tab's "Use as a workflow tool" form asks before replacing a taken name: re-pin
    within the same workflow, or re-point from another, each consequence stated per §D2.2.
- **Agent pane:** two lazy cards from the workflow router:
  - "Workflow tools": ordered grants, one name per line, saved against `expected`; each name
    resolved (workflow tool / message / provisioning / resolved at publication).
  - "Used by" (F098.2).
  - A failed lazy load shows its fallback text (`data-lazy-card`, handled in
    `WORKFLOW_EDITOR_SCRIPT`).
- **CSRF:** `WRITE_ROUTES` lists 11 patterns: save binding, activate, deactivate, register tool,
  revoke tool and grant are added. The test substitutes every path parameter.

Deviations:
- DV-P09-7: see the B1b plan. The UI states the code's §D2.2 semantics, not WF-C003's wording.
- **DV-P09-8 (`check:css`):** CI's `npm run check:css` fails before p09. The tracked
  `assets/app.css` (identical to HEAD) carries about 100 classes p08's removals left unused, and
  a build from the repository root also scans untracked files.
  - Isolated builds of only `src` + `assets/app.css.in`: the pre-p09 tree (reconstructed from
    `logs/start-base/` + HEAD) against the current tree. p09 adds `max-h-96`, `ml-2`, `ml-4` and
    `pl-3`, all already in the tracked CSS, and drops none.
  - p09 does not rebuild `app.css`; that is the user's or the CSS owner's call (reported).
  - `node_modules` was installed with `npm ci --ignore-scripts` (gitignored) for this check.
- **DV-P09-9 (contrast, B1a pages too), from the browser pass:**
  - The diagnostic excerpt `<pre>` gets `text-base-content`. It inherited the error alert's
    text colour on `bg-base-100`, which made it unreadable in dark.
  - Archive and Revoke are filled `btn-error`: the outline was 2.86:1 in light.
  - The four authoring tables' `thead` get `text-base-content` (2.47:1 in light).
  - Labels and hints inside `.label` no longer add their own opacity, which stacked to 2.28:1.
  - Every class used is already in the tracked CSS (class scan: 0 missing).
- **DV-P09-10 (binding form):**
  - Thread bindings stay API-only.
  - The message-trigger select is always shown; the command refuses one on a company binding,
    and the form keeps the choice.
  - Text parameters suggest the company's people by id through a datalist.
  - Only the company owner gets a run-as select (`companies.user_id`, the command's rule).
- **DV-P09-11:** the agent and channel panes show the workflow cards by lazy load from the
  workflow router, so those workspaces take no engine dependency.
- **DV-P09-12 (B2a):** the live-update protocol differs from the B2a design, because the
  design's version broke three things (B2a log, items 1–3):
  - it aborted accepted writes, lost a control's answer, and reset views every 30 s;
  - wake-ups now reload `#run-body` and the list through empty leaf triggers with selector
    syncs;
  - the periodic re-check yields `tick`, which only an unfinished run's body takes;
  - finished steps are `hx-preserve`d.
- No schema check on tool registration: the wire format requires `input_schema` and
  `output_schema`, so a parsed version always has both.

Observations, not changed (outside p09 or pre-existing):
- O1: a draft whose `workflow_id` is not a UUID is refused with the command's unlocated
  "Invalid checked name" (shown as DV-P09-6's "Saved … but it could not be validated: …").
- O2: p08's agent form hints (`opacity-50/60`, stacked on `.label`) are under 3:1 in light.
- O3: daisyUI's inactive tab (3.4:1) and other secondary text sit under WCAG's 4.5:1 for small
  text in light. The browser pass holds p09 views to a 3:1 floor, which all of them meet.
- O4: `workflow_tool_snapshot` maps a registration without schemas to `invalid()` (a database
  error). The wire format makes that unreachable.

Checks (cluster 55490):
- `cargo test --lib -- routes::workflows adapters::http::pages application::workflow::authoring`:
  230 passed (`logs/b1b-focused-1.log`).
- `cargo test --lib -- adapters::persistence::workflow application::workflow::publication
  application::workflow::lifecycle`: 990 passed, 0 failed (`logs/b1b-persistence.log`). This ran
  before the `binding_use` read and the contrast edits; the UI tests cover both.
- Final focused set `cargo test --lib -- adapters::http application::workflow::authoring
  application::workflow::registry`: 388 passed, 0 failed, 1 ignored (`logs/b1b-focused-3.log`).
- `cargo fmt --check` clean; `cargo clippy --all-targets -- -D warnings` clean
  (`logs/b1b-clippy.log`); class scan 0 missing.
- **Browser pass (F012), `logs/f012/`:**
  - Setup: the installed Google Chrome 155.0.8059.86 driven by `playwright-core` 1.55.1 (headless, fresh profile) on
    `localhost:3001`, served by `cargo run` over `workflow09`. Local throwaway secrets and test
    accounts are kept in the session scratch dir only. The server was stopped afterwards.
  - Result: **54/54 checks pass** (`pass-run.log`, `results.json`, screenshots).
  - Template copy; F003 dirty marking; F014 (cancel from Tools/New asks and leaves the button
    usable); F004 status while validating and on a failed save, with the text kept and busy
    cleared.
  - Reference failure and retry; save, validate, publish.
  - Bindings: not bound → save paused → activate.
  - Keyboard: Shift+Tab across the tab strip with a visible 2px focus outline; Enter opens a tab.
  - Tool register and list; agent cards (grant saved, used by, failed-card fallback); channel
    card states who answers.
  - Light and dark views (bindings, editor, versions, tools, agent cards, diagnostic) have no
    text under 3:1. The graph's fill follows the theme. No unexpected page errors.
- Freeze: digest `293c80357296a898b19f1e83b6eed47cb45f5dc74fcc83d00840aab418878555`
  (`logs/b1b-snapshot.txt`, same exclusions, HEAD `da86871`).
  - Delta against the accepted B1a artifact: `logs/b1b-files.txt` (30 files), diff
    `logs/b1b.diff`.
  - Bases: `logs/tree-b1a-r3/`, and `logs/start-base/` for five p08-changed files B1b touched
    first (migration, `agent_settings.rs`, `channel_settings.rs`, `persistence/workflow/
    definitions.rs`, `persistence/workflow/tests.rs`). Each was reconstructed and verified
    against the start hashes.
  - Tree `logs/tree-b1b/`, hashes `logs/b1b-filehashes.txt`.
- Coordination r11 unchanged.

## B1b corrections for WF-P09-R005 (2026-10-25)

R005: changes required. F019 and F020 Medium, F021 Low. F012 keeps a Low keyboard residual, owed
to the phase's final browser pass and not blocking B1b. F017 and F018 are verified. B agrees with
DV-P09-8. All three fixed in one round.

- **F019:** a `Conflict` from the binding command is classified before it is reported.
  - `Ui::binding_moved` re-reads the binding (`binding_use`). "Moved" means a create whose
    keyed binding now exists, or an edit whose state revision differs from the one shown.
  - Only then does the UI say "already created" or "changed since …". Otherwise the command's
    own message is shown as "Not saved: …" in the form, with every choice kept, or as
    "Not activated/deactivated: …" on the row. An example is "Resource slot … is not ready".
  - Test `a_resource_that_is_not_ready_is_refused_as_such_on_create_configure_and_activate`
    (B's probe): a bearer HTTP connection without its secret on create, configure and
    activate. The same key then saves with the ready connection, and a stale row is still said
    as one.
- **F020:** the copy now states P07-D2 rev 4 §D2.2/§D2.7, not the plan's paraphrase (the plan
  text above is corrected too). `pages::REVOKE_WARNING`, `REPOINT_WARNING` and `REPIN_NOTE`:
  - revoking (or pointing the name elsewhere) stops every published version that froze it;
  - their new runs are refused, their bindings can no longer be configured or activated, and
    their agents' next calls are refused;
  - a call already in progress still returns its result;
  - re-registering for the same workflow restores them;
  - re-pinning within the workflow changes only later publications.
  The revoke and re-point banners say the same. Test
  `revoking_or_re_pointing_a_frozen_tool_stops_its_versions_bindings_as_the_warning_says`
  checks both:
  - copy: it pins the facts and the absence of "already waiting";
  - behaviour: after a revoke the binding of a version whose agent froze the tool cannot be
    activated ("Not activated: …"). Re-registering for the same workflow restores activation,
    and re-pointing to another workflow stops it again.
- **F021:**
  - `fields_for(schema, params)` gives fields only when saving them unchanged stores the same
    object. Otherwise the form opens the whole object as JSON: undeclared keys, an empty or
    blank string, a value of the wrong type.
  - A hidden `params_mode` (`fields`/`json`) tells the save which form the author saw.
  - Integer and number fields parse as JSON numbers: `5` stays `5`, large integers keep their
    digits, and a fraction in an integer field is refused.
  - Tests: unit test
    `stored_params_open_as_fields_only_when_saving_them_unchanged_stores_the_same_object`;
    route test `params_the_fields_cannot_carry_open_as_json_and_save_unchanged`.
- Noted by B, adopted: "Used by" says "froze this agent". Not adopted: the version-change GET
  carrying the form in its query (bounded by the source and params limits; recorded).

Checks:
- `cargo test --lib routes::workflows::ui`: 39 passed.
- `cargo test --lib -- adapters::http application::workflow::authoring
  application::workflow::registry application::workflow::publication`: 424 passed, 0 failed,
  1 ignored (`logs/b1b-r2-focused.log`).
- fmt and clippy `-D warnings` clean (`logs/b1b-r2-clippy.log`); class scan 0 missing.
- The persistence suite is unaffected (no persistence change since B's 948-pass run).

Freeze: digest `40320058996d47820d1c1517c683517973c5ac745a38eaf82aa77636fea1156e`
(`logs/b1b-snapshot-r2.txt`). Delta against R005: `logs/b1b-r2-files.txt` (9 files), diff
`logs/b1b-r2.diff`. Tree `logs/tree-b1b-r2/`, hashes `logs/b1b-r2-filehashes.txt`.
Coordination r11 unchanged.

### B1b correction for WF-P09-R006 (2026-10-25)

R006: F019 and F020 fixed. F021 has one Low residual: an `<input>` drops line breaks from its
value, so a stored multi-line text passed `fields_for` but lost its newline on an unchanged
save. B confirmed this in Chrome 155 (`logs/r006-b-newline-check.log`).
- **Fix:** `fields_for` treats a text or choice value containing `\n` or `\r` as not carried,
  so the form opens the whole object as JSON (the JSON textarea escapes them inside strings).
  The unit test adds `"Kind regards,\nSupport"`, `"a\r\nb"` (both JSON) and a one-line value
  with trailing spaces (fields).
- **Checks:**
  - `cargo test --lib -- application::workflow::authoring routes::workflows::ui`: 48 passed.
  - Focused set `cargo test --lib -- adapters::http application::workflow::authoring
    application::workflow::registry application::workflow::publication`: 424 passed, 1 ignored
    (`logs/b1b-r3-focused.log`).
  - fmt and clippy clean.
- **Freeze:** `89d23e56e74c70e6fcea1d60a9b9b87d4cc5c829af3afd34e8d8ab8147760eb9`
  (`logs/b1b-snapshot-r3.txt`). Delta `logs/b1b-r3-files.txt` (2 files), diff
  `logs/b1b-r3.diff`, tree `logs/tree-b1b-r3/`, hashes `logs/b1b-r3-filehashes.txt`.

## B2 plan (read-only design during R005; refine before implementing)

- **Contracts:**
  - WF-C002 r1: binding run lists label children (`parent_run_id`); run inspection shows lineage
    (parent run and execution, round ordinal) and `rerun_of`.
  - WF-C005 r1:
    - listings follow `{items, next}` paging over partly hidden pages;
    - reconciliation is an annotation and is labelled so ("record what happened"; recovery is
      cancel then rerun);
    - client rules: a 409 re-reads the run and uses a new key, a 503 retries with the same key;
    - a child run is controlled through its parent.
  - WF-C003 r1: tool-call children in run inspection, with the saved call id.
  - `src/adapters/http/AGENTS.md` "Make partial-page updates race-safe":
    - subscribe before querying, reconcile on every connect;
    - scope detail reconciliation to events naming that run, keep the `sse-connect` owner
      mounted, use `hx-sync` replace;
    - tests for an event between render and subscribe, and for two reads completing out of
      order.
- **Wake source:** `workflow_run_changed`, already wired (`infra/events.rs`
  `MailboxEvent::WorkflowRunChanged(RunScope {company, channel, thread, run})`, fired on run
  insert and on state or waiting-reason changes).
  - A new `/ui/workflow-runs/events?company_id=` stream filters the company's run scopes, after
    `ui_run_counts`.
  - Step progress inside a running run does not notify, so an open detail of an active run also
    re-checks on a bounded interval (as `RUN_COUNTS_RECHECK_INTERVAL` does). Lag means a full
    reconcile.
- **Pages:**
  - Runs workspace `/ui/workflow-runs`: filter by binding and state, paging by `next`, children
    and reruns labelled, hidden rows counted per page.
  - Run detail `/ui/workflow-runs/{id}`:
    - state and waiting reason; the triggering message (thread link); version (link to the
      versions tab); the graph with progress marks (`NodeMark` from B1a);
    - a chronological timeline (executions, waits, actions, control commands, reconciliations);
    - each step expands to `frozen_inputs` and `committed_output` (authorized as the run is),
      context sources, the agent's frozen tools and skills, model usage, action receipts and
      deliveries, child runs (step and tool-call children with the saved call id), and retry
      and reconciliation history;
    - worker attempts under a diagnostics disclosure.
  - Controls: cancel (explaining effects already accepted), safe retry, rerun (a new run with
    `rerun_of`), and the reconciliation annotation. Each sends a fresh command key, handles
    409/503, and shows visible status.
- **Links:**
  - Mailbox messages link to their runs (`ThreadMessageView.run_ids`, F098.1 attribution).
  - Thread activity shows each active or waiting run separately (criterion 3).
  - Attention items for runs and bindings get hrefs (`persistence/attention.rs item_href`,
    tests asserting None updated).
  - Dashboard job rows and schedule occurrences link their run.
- **New reads** (no schema change expected): execution detail (inputs, output, route, choice),
  model calls and usage per execution, receipts, control commands and reconciliations, child
  runs by parent, worker attempts. All are company-scoped and bounded (newest 200 with
  `*_truncated`, as `run_detail` does).
- **Gate:** likely none. Reads and UI only, over the existing run-change notify; reopen if a
  schema or notify change turns out to be needed.

## B2 split and B2a design (2026-10-25)

B2 is split at a stable contract, the run page `/ui/workflow-runs/{run}?company_id=`:
- **B2a:** runs workspace, run detail, controls and live updates.
- **B2b:** conversation links:
  - mailbox replies to their runs (`ThreadMessageView.run_ids`, F098.1 attribution);
  - a per-run strip on the thread (criterion 3);
  - attention hrefs for runs and bindings;
  - dashboard job and schedule occurrence links.

Each is reviewed once.

- **Who sees runs:** workflow authorization admits owners and admins only
  (`manages_company_operations`), plus channel visibility per association. The run pages use
  exactly that rule, through the same `authorize` and `Visibility` as the JSON routes. Plan 09's
  "existing membership and channel rules govern who can start and inspect runs" is this rule
  today. B2b shows run links only to viewers who may follow them.
- **Reads** (new `OperationsReads`, `persistence/workflow/operations_reads.rs`; company-scoped,
  bounded, lock-free):
  - `run_view(company, run)`:
    - the existing `RunDetail`, plus workflow title, version draft revision and channel name;
    - the run's `input` and `params`, and its triggering message
      (`workflow_message_intent_bindings.run_id` → intent `message_id`);
    - lineage: parent run and execution, the parent wait's `child_ordinal` (round) and
      `child_tool_call_id`, and `rerun_of`;
    - children (≤200, with ordinal and tool-call id), control commands
      (`workflow_control_commands`, ≤200), reconciliations (≤200) and run events
      (`workflow_run_events`, ≤200).
  - `execution_view(company, run, execution)`:
    - `frozen_inputs`, `committed_output`, `frozen_choice`, `committed_route`, `route_target`;
    - model calls (`model_usage_calls` by execution: provider, model, tokens, usage source,
      times, ≤50);
    - receipts (`workflow_action_receipts`, result shown bounded, ≤50);
    - worker attempts (`background_tasks.workflow_execution_id` → `task_attempts`: number,
      status, stop reason, failure class and code, retry safety, times, ≤50);
    - children started by this execution;
    - the agent's frozen tools and skills, from the run's bundle snapshot of that step's agent.
- **Shared commands** (no parallel logic): cancel, retry, start with `rerun_of` (input and
  association from the run, a fresh key) and reconcile are extracted from
  `bindings.rs`/`reconciliations.rs` like B1.
  - Controls send a fresh `command_key` per render and the run's revision.
  - A 409 re-reads the run and says what changed. A `503` (Unavailable) keeps the same key, and
    the page says retrying with that key is safe.
  - A child run's controls point at its parent (WF-C005).
  - Reconciliation is labelled "Record what happened (annotation, not a resume)" with an
    unknown note, and only on `reconcilable` hints. Recovery is spelled out: cancel, then rerun.
  - Cancel explains that effects already accepted stay (receipts listed).
- **Pages:**
  - Workspace `/ui/workflow-runs` (Workflows section; a "Runs" link in the workflow sidebar, and
    each binding row's "Runs"): a sidebar list with state and binding filters, `next` paging
    (Load more), partly hidden pages said plainly, children and reruns labelled.
  - `#run-pane`:
    - header: state, waiting reason, version link, binding, trigger message;
    - the graph with progress marks (`NodeMark`);
    - a chronological timeline (activations, waits, actions, controls, reconciliations, events);
    - per-step `<details>` lazy-loading `execution_view`;
    - lineage and children; controls with visible status; worker attempts under a
      "Diagnostics" disclosure.
- **Live updates:** `/ui/workflow-runs/events?company_id=`.
  - It subscribes to `MailboxEvents` first, authorizes, then yields `resync`.
  - It yields `runs` plus `run-<id>` for each `WorkflowRunChanged` of the company, and `resync`
    on lag. It re-checks access every 30 s and also yields `resync` then, because step progress
    inside a run does not notify.
  - The `<main>` owns `sse-connect`. The list reloads on `sse:runs, sse:resync` with
    `hx-sync="this:replace"`; the pane reloads on `sse:run-<id>, sse:resync` with
    `hx-sync="#run-pane:replace"`.
  - Tests: the stream's first event is `resync`, before any change (event before subscribe); a
    company's run change yields `runs` and `run-<id>`, another company's yields nothing; lag
    yields `resync`; the markup carries the `hx-sync` rules. In the browser pass: two
    overlapping reloads, where the later one wins.
- **Tests:**
  - list paging over a partly hidden page (`next` with fewer items) and children and reruns
    labelled;
  - detail lineage (a `workflow.call` child), rerun_of;
  - cancel, a stale cancel (409, re-read), retry of a failed run and a refused retry, rerun;
  - a child's control pointing at its parent;
  - hostile inputs and outputs escaped; cross-company and member NotFound; CSRF routes;
  - the SSE tests above.
- **Checks:**
  - focused `routes::workflows::ui`, `adapters::persistence::workflow` (new reads),
    `routes::workflows` (extracted commands), pages;
  - fmt, clippy, class scan, and a browser pass for B2's pages.

## B2a implementation log (2026-10-25)

Base: the accepted B1b artifact (`logs/b1b-r3-filehashes.txt`, digest `89d23e56…0eb9`). Delta:
`logs/b2a-files.txt` (22 files), diff `logs/b2a.diff` (+3896/−32), tree `logs/tree-b2a/`.

- **Reads** (`application/workflow/run_views.rs`, `persistence/workflow/run_views.rs`; the design
  called them `OperationsReads`/`execution_view`, built as `RunViews`/`step_view`, reached through
  `WorkflowEngine::run_views()`):
  - `run_view`: `RunDetail` plus title, draft revision, channel name, input, params, triggering
    message, lineage, children, controls, reconciliations and run events. Each list is ≤200 with
    a "cut" flag.
  - `step_view`: frozen inputs, output, choice, route and target; model calls, receipts and
    worker attempts (≤50 each); and the children the step started.
  - `run_bundle` decodes the run's frozen bundle for the graph and the step's frozen agent.
  - `workflow_titles` supplies the list.
  - Every query is company-scoped. Each step list is also scoped to the run: the attempts query
    proves it through `workflow_executions.run_id`, because `background_tasks` has no run
    column.
  - Rows are private `sqlx::FromRow` structs with `From` conversions.
- **Shared commands** (no parallel logic):
  - `routes/workflows/bindings.rs`: `start_command` (`rerun_of`, association), `cancel_command`,
    `retry_command`.
  - `routes/workflows/reconciliations.rs`: `reconcile_command`.
  - The JSON routes call the same functions.
- **Pages** (`pages/workflow_runs/{mod,detail,step}.rs`) and **routes**
  (`routes/workflows/ui/runs/{mod,controls,events}.rs`):
  - Pages: the workspace, the list (state and binding filters, Load more, hidden rows counted),
    the pane and the lazy step body.
  - Controls: cancel (`hx-confirm` naming accepted effects), retry, rerun (pushes the new run's
    URL), and reconcile ("Record what happened (an annotation, not a resume)", an unknown note,
    offered only on `reconcilable` hints and never on a child).
  - Write routes: 4, all in the CSRF test.
  - Authorization: owners and admins plus channel `Visibility`, exactly as the JSON routes. Any
    other run is NotFound.
- **Author-found defects, fixed before review (from the browser pass):**
  1. **An accepted write was aborted.** The pane carried `hx-sync="this:replace"` for its
     wake-up reloads. htmx inherits `hx-sync`, and `this` names the declaring element, so the
     control forms joined that replace group. The run's own wake-up GET aborted the cancel POST:
     the cancel committed, but its answer was discarded. The browser recorded
     `post …/cancel` among the aborted requests, against `src/adapters/http/AGENTS.md`'s "Do not
     abort accepted writes".
     - Fix: both live regions now reload through an empty hidden leaf with an explicit selector
       sync, so nothing inherits it.
       - List: `hx-sync="#run-list:replace"`, shared with the filter form, so the later of two
         reads wins.
       - Pane: `hx-sync="#run-body:replace"`, separate from opening another run
         (`#run-pane:replace`), so a wake-up never cancels a click either.
     - Tests pin the leaf attributes and the absence of `this:replace`. The browser check
       records every `htmx:sendAbort` across navigations: all 7 were list GETs.
  2. **The control's answer was overwritten.** The pane reload replaced the whole pane, banner
     included.
     - Fix: the pane is `#run-pane` = banner + `#run-body`, and a wake-up reloads only the body
       (`?part=body`, typed `PanePart`). The answer stays until another run is opened.
     - Tests: the body route returns the body alone (tenancy too), and the recorded answer sits
       before `#run-body`. Browser: "the answer outlasts the run's own wake-up".
  3. **The periodic `resync` would reset views every 30 s.** It would collapse "Load more" and
     close open steps.
     - Fix: the 30 s access re-check now yields `tick`. The list ignores it. A body listens to
       it only while the run is unfinished, because step progress does not notify. Finished
       steps carry `id="run-step-<execution>" hx-preserve="true"`, so an open finished step stays
       open and loaded across reloads.
     - `run_event_stream` takes the interval. A new test sees `tick` (not `resync`) and the
       stream ending once the viewer's membership is removed. Browser: a cancel made elsewhere
       reloads the body and keeps the open step.
  4. **An ended run showed its parked step as running.** It read "running or waiting" and drew
     it as current.
     - Fix: the new `NodeMark::Stopped` (dashed warning stroke, "stopped") and the step state
       "stopped, run <state>". Unit test plus a browser check.
- **Worker attempts:** only a leased (effectful) step has `task_attempts`; a pure step completes
  without a lease (`CompletionOwner::Pure`).
  - The attempts disclosure is therefore covered by an integration test over the uncertain
    HTTP fixture: a real lease, attempt `failed`, and the run waiting on reconciliation. The
    same test exercises the UI reconcile form end to end: the recorded note, no resume, and a
    replay with the page's key recording nothing more.
  - The page unit test covers the rendering.
- **Out-of-order reads:** the protection is client-side (`hx-sync` replace groups above), pinned
  by markup assertions. The browser exercises it with 6 concurrent run starts: one list, one
  pane, one stream owner, no duplicates.
- **Tests** (`routes/workflows/ui/runs/`): `runs_tests.rs` (fixture, lineage/child controls, list
  paging over hidden runs and rerun labels, tenancy/hostile, CSRF), `controls_tests.rs`,
  `events_tests.rs`, `pages_tests.rs`. 10 tests.

### B2a checks

- `cargo test --lib routes::workflows::ui::runs`: 10 passed.
- `cargo test --lib -- adapters::http`: **358 passed, 0 failed, 1 ignored**
  (`logs/b2a/focused-tests.log`). This covers every route over the extracted commands, the
  pages, the graph and the UI access tests. Phase cluster `127.0.0.1:55490`, own databases per
  test.
- `cargo fmt --check` clean; `cargo clippy --all-targets -- -D warnings` clean
  (`logs/b2a/clippy.log`); class scan over the run, authoring and page files: 0 missing (two
  undefined tokens, `step-body` and `pl-4`, were removed).
- **Browser pass, `logs/b2a/browser/`:**
  - Setup: the same Chrome and playwright-core as F012, a fresh local test owner and company,
    `localhost:3001` over `workflow09`. The server was stopped afterwards; the throwaway
    password stays in scratch.
  - Result: **32/32**.
  - Live list over SSE, without navigation; overlapping wake-ups; pushed URLs and Back; the
    graph; hostile input as text.
  - Enter opens a step; a change elsewhere keeps the open step.
  - Cancel through its confirm, the answer persisting, and the stopped step; rerun to the new
    run.
  - Deep link, state filter, Load more past 50 with no repeats, binding filter.
  - Keyboard: Tab to a run link with a 2px outline, Enter opens it.
  - Both themes: no text under 3:1.
  - Every aborted request was a GET; no unexpected page errors.
- Freeze: digest `a62d5b7ea0affde68111a687eef03c60365a49c4427c669262fef739aa7e70a2`
  (`logs/b2a-snapshot.txt`, HEAD `da86871`, exclusions `plan/workflow-foundation/phases/p09/docs
  plan/workflow-foundation/phases/p08/docs plan/workflow-foundation/COORDINATION.md marketing
  website`); hashes `logs/b2a-filehashes.txt`. Coordination r12 (`05eb8e6e…2c69`) unchanged.

## B2b plan (draft during the B2a review; refine after B2a acceptance)

Plan 09 asks: conversation entries link to their producing run; show multiple active or waiting
runs independently, not one "current task"; reply and review links point to exact identities.
Criterion 3: readers can distinguish two simultaneous runs on one thread and follow their
separate replies. Links are shown only to viewers who may follow them. Today that means owners
and admins (`manages_company_operations`) and a visible channel; others see the plain short id.

- **One run-link helper** in `pages/workflow_runs`: a `run_href` and a chip that links or not.
- **Mailbox thread** (`pages/mailbox.rs`, which already loads `ThreadMessageView.run_ids` through
  `THREAD_RUN_LOOKUP_SQL`):
  - each agent reply says "Written by run X";
  - each inbound message lists the runs it started;
  - all with the same short ids.
- **Per-run strip:** replaces the single newest-run `thread_activity_strip`. It lists every
  unfinished run on the thread (bounded, newest first, with a count of the rest), each with its
  workflow title, state and link.
  - It needs a thread-scoped read; check `application/thread_activity.rs` first.
  - Live updates must follow the mailbox's existing `WorkflowRunChanged`/thread wiring in
    `routes/live_updates.rs`.
- **Attention** (`persistence/attention.rs` `item_href`): `RunReconciliation` and `RunFailure`
  go to the run page; `BindingRefusal` goes to the binding's workflow bindings tab, which may
  need a small resolving route. Update the tests that assert `None`. Check who sees those items
  (`query.manager`).
- **Schedules** (`pages/schedules.rs` `occurrence_row`): the run becomes a link. **Dashboard**:
  check for job or run rows that name a run.
- **Tests:**
  - two bindings admitting one message, so two simultaneous runs on one thread: both appear in
    the strip, and each reply names its own run;
  - a member sees no links;
  - cross-company and private-thread ids; hostile titles;
  - the CSRF list, if a write route appears.

## Status

- 2026-10-24: startup done (coordination checked, resources created, records established).
- B1a implemented and frozen (`62a6c35e…4e50`); R001 CHANGES REQUIRED (F001–F011, F012 gap);
  corrected and frozen at `b4dc745d…de3c`; R002 not a pass (F003 residual, F013, F014, F016;
  F015 optional); corrected and frozen at `1484a5bc…57db`; R003 **PASS** for
  `1484a5bc494ed3c0202e07678f66566fabe31f0f24ea0e5886620254cd6957db`.
- 2026-10-24: **B1a accepted in phase** (root recomputed the digest: `1484a5bc…57db`, HEAD
  `da86871`, same exclusions; coordination r11 `3e8501b4…62f5` unchanged). Findings F001–F011,
  F013–F016 resolved; **F012 open** — not B1a-blocking, phase-blocking (criterion 6). The
  client-side behaviour (the dirty guard and its prompt order, the editor status text, the
  reference failure state) is verified by code reading and the htmx 2.0.4 source only, not in a
  browser; the browser pass after B1b covers both batches. Not integrated; nothing committed.
- B1b implemented, browser pass (F012) done, frozen at `293c8035…8555`. R005: changes
  required (F019, F020 Medium; F021 Low). Corrected and frozen at `40320058…156e`. R006: F019 and F020 fixed; F021
  Low residual (multi-line text). Fixed and frozen at `89d23e56…0eb9`. R007 **PASS** for
  `89d23e56e74c70e6fcea1d60a9b9b87d4cc5c829af3afd34e8d8ab8147760eb9`.
- 2026-10-25: **B1b accepted in phase**. Root recomputed the digest (`89d23e56…0eb9`, HEAD
  `da86871`, same exclusions); coordination r11 was unchanged through the review.
  - F017–F021 resolved; gate P09-B1b-D1 rev 1 implemented as reviewed.
  - F012: browser pass done (54/54); its keyboard residual (Low) is owed to the final browser
    pass, which is phase-blocking under criterion 6.
  - `check:css` (DV-P09-8) predates p09; reported to the user.
  - Coordination r12 published: the WF-C003 response cites P07-D2 rev 4 §D2.2 (sha256
    `05eb8e6e…2c69`, copy `logs/coordination-r12.md`).
  - Not integrated; nothing committed.
- B2a implemented, browser pass 32/32, frozen at `a62d5b7e…70a2`; review requested.

## Resume 2026-10-25 — Codex / implement-astra-reviewed-2.2

- User confirms root is Astra/high; root acts as Author A. New independent reviewer will be
  Astra/medium with no inherited history; predecessor handles are stale. Root settings are
  user-confirmed, not exposed by host metadata. Compaction threshold/occupancy unavailable;
  durable checkpoints used. Previous workflow/settings above are historical.
- Branch `workflows`, HEAD/start commit `da868710d3b235d49856e3e481f8f3735f6b3e56` unchanged.
  Preserve all p08/p09 changes and the exact existing snapshot exclusions.
- Coordination authority `/Users/mac02/dev/openbot/plan/workflow-foundation/COORDINATION.md`,
  r13 ownership handoff; WF-C002 r1, WF-C003 r1, WF-C004 r2, WF-C005 r1 re-acknowledged.
- Handoff was stale: R008 requires F022–F029 corrections. Working tree includes unfinished
  corrections; `logs/b2a-r2/focused-tests.log` reports 361 pass / 1 ignored, persistence log
  71 pass. Neither substitutes for independent recheck. User's last log reports browser 34/41
  and disappearing steps after cancel. Resume there, then B2b, B3, B4 and final acceptance.
- `graft map` fails (command absent); graph directory, common install paths and MCP tool absent.
  Use scoped rg and reads; graph refresh unavailable until graft is installed.
- Cluster 55490 is running; prior phase server is still running despite old HANDOFF. Scratch:
  `/private/tmp/claude-501/-Users-mac02-dev-openbot/ea208e50-9d63-4d67-b4d7-c13beb3ef45f/scratchpad`.
  Retain throwaway secrets there without copying them to evidence.
- Current user's repository instructions require the final full verification suite, overriding
  the skill's ask-only completion default and prior final-gate notes. Run focused checks per
  batch, then formatting, offline compilation, fresh migrations, database-backed full suite,
  stack-budget and frontend checks at the phase gate. No commits/deployments authorized.

## B2a corrections for WF-P09-R008 — recovered and completed 2026-10-25

Predecessor corrections in the working tree, now recovered:
- F022: waiting runs no longer tick; unfinished disclosures and reconciliation forms preserve
  their DOM while revision/epoch hold. Completed steps keep their execution identity.
- F023: pane epoch travels with body reads; keys derive from viewer, epoch, run, revision and
  operation. Rerun history is shown; after settlement the control explicitly says Rerun again
  and includes the latest rerun in its intent. Repeated unchanged requests replay.
- F024: idempotency conflict names a previously used form and possible earlier recorded note.
- F025: selection is sent with refreshes and updated after pane selection. Entries refresh in
  their existing scroll container, reading as many rows as have been loaded.
- F026: all run links share the pane's replace group; write controls do not inherit it.
- F027: extended route tenancy/refusal coverage and a real capacity-one broadcast lag test.
- F028: `routes/workflows/reconciliations.rs` pre-existed p09 (start hash c6412655…d309).
  Its handler body moved into `reconcile_command`, and TargetBody/ReconcileBody became pub.
  The historical b2a.diff wrongly represents this untracked pre-existing file as newly created;
  no verified original copy is available, so do not treat that diff hunk as complete evidence.
  R008 read the entire file and exercised all six p08 reconciliation tests. Future batch bases
  retain copies of every newly touched untracked file before editing.
- F029: three init-schema indexes support controls, trigger messages and parent-child reads.
  Reconciliation/receipt reads start from the existing run-led dispatch index. Before/after
  EXPLAIN ANALYZE BUFFERS evidence is in logs/b2a-r2/explain-{before,after}.txt, from 50,000
  synthetic runs / 200,000 controls / 100,000 receipts on the isolated phase cluster. The
  predecessor reset workflow09 and workflow09_test; focused migration-backed tests ran cleanly.

Resume discoveries and corrections:
- A body GET can arrive after a POST replaced its pane. HTMX 2.0.4 then globally finds the new
  pane's preserved disclosures, moves them to its pantry, swaps the detached old target and
  deletes the live disclosures. Browser trace reproduced empty step li elements after Cancel.
  WORKFLOW_RUNS_SCRIPT now cancels beforeOnLoad on the original requesting element when its
  target is disconnected. It neither aborts a write nor applies stale state to a new pane.
  A shipped-HTMX browser regression holds the GET across a cancel/navigation response.
- The dynamic first page grew from 13 to 50 entries but its hidden row count stayed 13. Loading
  eight more sent 21, so the next wake-up clamped to 50. Every entries refresh now updates the
  row count OOB. Load more joins the list read group. At MAX_LISTED (200) the view explicitly
  reports that older matching runs are omitted; it does not offer pages a refresh cannot retain.
  Caller-supplied counts are clamped to prevent arithmetic overflow/unbounded reads.
- Actual HTTP proxy socket destruction replaces Playwright offline emulation in the browser
  pass; offline emulation had left existing SSE sockets alive. Expected sendError and native
  EventSource errors are counted separately from unexpected errors. Binding expectations read
  current fixture counts instead of the obsolete hard-coded three.
- Regression protocol script scripts/tests/workflow-runs.mjs is included in CI. Full application
  browser journey is reproducible with scripts/tests/workflow-runs-browser.cjs against a phase
  server; its throwaway passwords stay outside the repository, in OUT.

Checks in progress: initial resumed HTTP suite 361 pass / 1 ignored (resume-http.log); browser
protocol 2 pass (browser-protocol.log). Full application browser initially reproduced 34/41;
first fix 40/42 (remaining row-count defect and expected disconnect-event classification).
Recheck the final corrected artifact before acceptance.

Snapshot portability: the installed Astra snapshot script uses GNU realpath flags unsupported
by macOS. PATH=/private/tmp/p09-tools:$PATH supplies a minimal Python realpath adapter for those
flags. The script itself is unchanged; exact exclusions remain the historical five paths.

B2a resume verification: 13/13 run-route tests; the broader HTTP suite before the row-count
correction was 361 pass / 1 ignored. Offline build, fmt and whitespace checks pass. Browser
journey 43/43, protocol 2/2 (logs/b2a-r2/browser-resume and browser-protocol.log). Clippy is
running without source changes. Frozen for recheck at `bb37b8a157f40f67a0ba9f478500d2dd755709ce25267ab1bd6a0642f3b6330e`, HEAD `da868710…3e56`;
manifest b2a-resume-snapshot.txt, correction delta b2a-resume.diff, hashes/tree b2a-resume.
Coordination r13 pinned in logs/coordination-r13.md. No acceptance yet.

## B2a corrections for WF-P09-R009 — 2026-10-25

R009 independently closed F022/F024–F026/F028–F029, retained F023 and F027, and opened F030.
- F023: removed latest-rerun history from the command intent. Every redraw of the same pane
  retains its key even after a first, second or later rerun commits. The separate read action
  “Prepare another rerun” explicitly opens a fresh pane before another rerun can be requested.
  Tests now repeat a second lost-response/reconnect path and assert no third run is created.
- F027: an HTTP reconciliation fixture can be admitted through a real private channel binding.
  Tests use real action and execution identities, try a foreign viewer under both company paths
  and a hidden-channel admin, assert zero evidence, then submit the same fields as owner and
  assert one evidence record. Shared HTTP fixture callers explicitly carry their association;
  the default setup wrapper returns the future directly (no added async stack frame).
- F030: a parent's body listens to its rendered children's run wakeups. Their nonterminal
  transitions refresh child badges while the parent remains waiting. It does not restore ticks
  that discard operator input. Browser protocol test exercises this without reconnect/navigation.
- New fixture test split into authorization_tests.rs to keep controls_tests.rs below 500 lines.
- Preserved bases for newly touched pre-existing fixture helpers are hash-checked against the
  R009 manifest (or unchanged HEAD) and stored in tree-b2a-resume before editing.
- Corrected resume-filehashes generation to retain `deleted` sentinels for absent paths; source
  snapshot digest is unchanged. Initial local signature-check failures and an overlapping-log
  attempt are retained as r009-runs.log; serialized verification uses r009-runs-final.log.

R009 correction checks: run routes14/14, shared HTTP/agent fixture callers54/54, browser
protocol3/3, complete application browser46/46, offline build/fmt/whitespace pass. Logs
`b2a-r2/r009-{runs-final,fixture-callers,protocol,build,clippy}.log` and `browser-r009/`.
Clippy finishing; no source edits. Frozen at `96c6c401cffc64d5566a19b414a52fdcacb7d00a1e402ea3c139d506e43ffde6`,
manifest/diff/tree/hashes `b2a-r009*`, same HEAD and exact exclusions. Coordination r13 unchanged.

## B2a accepted; B2b active — 2026-10-25

WF-P09-R010 PASS accepted by root after recomputing exact digest
`96c6c401cffc64d5566a19b414a52fdcacb7d00a1e402ea3c139d506e43ffde6`.
Coordination r13 unchanged; notices WF-C002r1/C003r1/C004r2/C005r1 acknowledged.
F023/F027/F030 closed; F012 final-phase keyboard gap retained. Clippy all-targets passes.
Root/A remains user-confirmed Astra/high; B /root/reviewer requested Astra/medium.
B2b now implements conversation run attribution, multiple active runs, attention/schedule links.
Focused checks: mailbox page and stream tests, persistence activity/attention and resolving-route
tests, compile/fmt; final phase full gate still required. Existing thread index evaluated before
considering another schema change. Verified pre-edit copies saved in tree-b2a-r009.
Current owned server session28255 port3001; phase cluster55490 and DBs unchanged.
B3 and B4 (P09-D1 architecture gate) remain; no final completion claim.

## B2b implementation — 2026-10-25

- Mailbox replies identify their producing run; incoming messages identify admitted runs. A shared
  chip navigates to the full inspector for owners/admins; other readers get plain identities.
- Open threads list up to 50 unfinished or failed runs independently, newest first, with immutable
  version titles and explicit truncation. The compact thread-row mark remains a newest-run summary.
- Initial and reconnect reads recover current state. Activity wakeups also reconcile attribution
  for the newest 200 messages using the existing batched run lookup; an admission after the bubble
  appeared therefore becomes visible. OOB replacements touch only attribution spans, retaining
  bubbles, stream owner, scroll and unsent composer text. No write or new authorization model.
- Attention run/reconciliation items link to exact runs for managers; binding refusals resolve
  through an authorized GET to the workflow's bindings tab. Schedule occurrences link to their run.
  Dashboard already renders attention destinations; no additional run row was found there.
- Read queries remain company/thread scoped after existing channel/thread authorization. Renderer,
  stream, resolver and persistence tests exercise multiple identities, hostile titles, member labels,
  foreign scopes/private channels, reconnect/event-before-subscribe, and bounded results.
- Query-plan fixture uses transaction-local copies of production table/index definitions, 50,000
  runs on 100 threads: existing thread index serves the read in 0.291 ms (explain-r2.log); no schema
  change or reset is needed. First fixture expansion was slow but completed (explain.log); revised
  seed uses a lateral record expansion. An attempted cancellation arrived after completion and
  changed nothing. Both fixtures rolled back.
- Focused initial pass125/1ignored. Late-attribution read exposed a wrong direction column; corrected
  to the canonical messages join. Next pass126/1failed/1ignored: the new 51-run fixture consumed only
  one four-intent worker page; it now drains each inserted intent explicitly. Failures retained.
- Shipped HTMX/SSE browser protocol1/1 passes; added to CI. Current final focused checks and
  all-target clippy are running. Graft remains unavailable; scoped caller search used.

B2b additional author audit: a reply may originate from a run on a different channel. Company
role alone cannot decide its inspector link. `ThreadRuns.inspectable` now resolves the displayed
run IDs against their actual channels using the existing principal/channel viewer policy;
ordinary members and hidden-channel admins get labels. This is the established policy, not a new
authorization model. A message-commit wakeup refreshes attribution as well as appended messages.
Tests cover readable/hidden producing channels, owner exemption, ordinary membership, and the
label for a visible reply whose producer cannot be inspected. Initial final checks134/1ignored
and clippy passed before this audit; rechecking the source-channel correction now.

B2b final focused verification:136 passed,1ignored (`logs/b2b/focused-r5.log`), including
source-channel link eligibility, route authorization, 51-run bound, schedule/page/stream callers.
All-target offline clippy `-D warnings` passed (`clippy-final.log`), fmt and git diff --check pass.
All11 browser lifecycle/protocol tests pass (`browser-lifecycle.log`), including new attribution
OOB/reconnect preservation. The earlier local non-exhaustive match compiler failure remains in
focused-r4.log and is corrected. Coordination matches pinned r13. Source now frozen; full batch
review input is b2b.diff against accepted b2a-r009, with b2b snapshot/hashes/tree. No B3 edits yet.

## B3 implementation design (read-only preparation during B2b review)

Use the existing WorkflowDecisions commands and their exact decision/revision/artifact identities.
The read projection will expose subject kind (business/action/token allowance), immutable workflow
version and artifact provenance, frozen data schema/choice-specific feedback requirements, and
whether the viewer is assigned to submit. Context links target the exact visible conversation
and execution; they do not grant access to an operator-only inspector or reveal private credentials.
The unified attention queue already uses one source with subject_kind-dependent next-action text;
its heading will identify the subject explicitly rather than requiring inference from the action.

Schema-driven data fields reuse authoring's parameter_fields/fields_for/parameters_from_fields
round-trip logic and fall back to JSON when the value/schema cannot be represented exactly.
Submission remains the existing command's validation, with typed inputs/feedback and the same
command key preserved on refusals. Comments remain a distinct operation; ordinary discussion must
not discard an unfinished decision form. Closed/late links remain on their exact historical decision,
show its outcome, and offer no new settlement. Any next-round link is navigation, not retargeting.

History should identify prior artifacts/feedback and later drafts across production child/repeat
lineage, with bounded read projections and explicit truncation, rather than embedding unbounded
artifact bodies. Confirm child association inheritance and apply existing visibility to links.
No schema or admission/concurrency protocol change is proposed; command ordering/first-settlement
semantics and CSRF middleware remain authoritative. Focused tests cover fields and fallback,
feedback-required choices, comments leaving a decision open, validation preservation, stale/late
and competing submissions, hostile content, role/tenant/private-channel checks, revision lineage,
and browser preservation/keyboard interaction. Final acceptance journeys remain a phase gate.

## Corrections for WF-P09-R011

- F031: define the live message window explicitly as the existing THREAD_HISTORY_LIMIT (200).
  The page states the limit. A small shipped script trims only excess message bubbles after an
  append, retaining a visible anchor and leaving the stream owner/composer intact. Reconnect
  batches now ask inspector eligibility for their actual run IDs rather than borrowing the
  activity snapshot's newest identities. The browser regression reconnects through210 backlog
  messages, verifies the final200 and all their links, then delayed admission cannot resurrect
  a message outside that window. A real DB regression places a run outside both activity/history
  windows and proves its older reconnect batch still obtains authorized links.
- F032: eligibility reads the selected runs first, deduplicates their source channels, and loads
  only those channel IDs through the existing ChannelDb/viewer policy. Empty sets return early.
  A derived cap (200 messages × [producer + max8 admitted bindings] +50 run-strip entries) refuses
  oversized internal lookup requests; it never silently drops legitimate links. Tests add64
  unrelated channels and verify the targeted channel/tenant results. The targeted production
  query plan uses an index with both company and requested-ID predicates (`r011-channel-explain-testdb.log`).
  The first explain attempt found no dev channels and is retained separately; no data changed.
- F033: explicit AS added to the new run/version and message/association SQL aliases.
- Local compile caught the existing binding cap's i64 type; explicit usize conversion fixes the
  derived read bound (`r011-focused.log`). Current focused check is r011-focused-r2.log. Full
  browser lifecycle rerun uses r011-lifecycle.log. Recheck all findings together after checks.

R011 correction checks:129 focused passes/1ignored (`r011-focused-r2.log`); browser lifecycle12/12,
then the strengthened retained-anchor protocol2/2 (`r011-browser-anchor.log`); fmt/whitespace pass.
Clippy initially found the new channel helper after its test module; moved before tests and
all-target offline clippy passes (`r011-clippy-r2.log`). No behavior changed in that move.
`cargo sqlx prepare` unavailable (CLI not installed). Verified the installed sqlx-macros-core's
SQLX_OFFLINE_DIR support and performed equivalent query metadata generation with
`SQLX_OFFLINE=false SQLX_OFFLINE_DIR=/Users/mac02/dev/openbot/.sqlx CARGO_INCREMENTAL=0 DATABASE_URL=postgres://workflow09@127.0.0.1:55490/workflow09 cargo check --offline --all-targets`.
It passes (`r011-sqlx-direct.log`) and all23 existing cache files remain byte-identical. Their
pre-refresh copies were preserved in tree-b2b; no query cache delta. Correction source frozen
for recheck, coordinationr13 unchanged; manifest/hashes/diff/tree b2b-r011.

## B2b accepted; B3 active — 2026-10-25

WF-P09-R012 PASS accepted by root after recomputing exact digest
`5578887c5eb2b7e4eb0e76b62bb02e1140e9fbd4c5bb8cd415530a368af6ca6d`.
F031–F033 closed; F012 final-phase residual remains. Coordinationr13 unchanged and notices
WF-C002r1/C003r1/C004r2/C005r1 satisfied/re-acknowledged. B3 now begins under the recorded design.
Root/A Astra/high and B /root/reviewer Astra/medium roles unchanged. Pre-edit B3 bases are saved
in tree-b2b-r011. B3 read/HTTP/page tests and a real two-round review journey are focused checks;
B4 architecture gate/implementation and final full verification remain. No schema change planned
unless representative read evidence requires an index. Current phase server/DB resources unchanged.

### B3 implementation checkpoint — 2026-10-25

- Added typed subject/context/schema/reviewer projection, bounded exact sibling-round history,
  and explicit subject labels in both queue branches. History uses per-run newest-first probes
  capped at 22×41 identities with a dedicated `workflow_decisions_history` index. This is a
  read index over existing ownership/transaction contracts, not a new protocol.
- Forms distinguish comment from settlement, preserve raw validation failures, and retain the
  exact decision/revision/artifact. Protected requests display their proposal but submit `{}`,
  as the existing protected-action/token commands require. Closed pages expose no write form.
- Stable `decision-shell` serializes writes with HTMX queueing and disables both forms' submit
  buttons; the retained submission survives comment swaps. Detached-response protection now
  includes the decision section. Optional test-generated real markup feeds browser checks.
- Initial offline all-target compilation passed. First decision filter: 44 passed/1 failed:
  the new repeated-review fixture declared an unconstrained input schema while referencing a
  required path. Fixed the fixture's input/output contracts; broader decision filter running.
- Stopped the phase-owned port3001 server (PID40211), reset only workflow09/workflow09_test
  at owned55490 after the squashed init index change. Databases are ready for migration-backed
  tests; dev server is currently stopped. No other phase resources touched.

## P09-D1 r1 — B4 isolated sample execution (proposed; gate not yet requested)

Original requirements: plan09 Sample execution/Acceptance. B4 must use the same compiler/engine,
scripted model/decision responses, fixtures for external reads, interception for messages/writes,
zero production effects through agents/direct tools/children, and no resume/promote path.

Decision: an in-memory sample execution mode in `application/workflow/sample`, exposed through
`WorkflowEngine` and the authorized draft authoring workspace. A sample owns only a frozen
`PublishedBundle`, bounded JSON input/parameters/scripts, ephemeral context, counters and report.
It has **no persistence, provider, delivery, action service, HTTP/MCP client, memory service or
production resource binding handle**. It cannot construct a `PreparedAdmission`, worker claim,
action fence or durable job. Production execution APIs do not accept a sample report.

Shared engine contracts (not duplicated language interpretation): existing draft validation and
publication snapshot resolution compile the saved expected revision and its finite child closure;
`prepare_inputs`/frozen agent selection, rule evaluation, `execute_pure`, `prepare_io`,
`prepare_submission`, `validate_completion`, `child_completion`, `provisional_output`,
`decide_round`, context/schema limits and route selection remain the execution semantics.
Extract a pure activation-preparation helper if needed so production and samples also share the
rule/choice preparation. Durable worker scheduling, leases, retries and external receipts are not
simulated and are not asserted by the sample report. Invalid/missing fixtures stop a sample with
an explicit diagnostic; they never fall through to a production handler.

Sample input contract: root input and parameters plus ordered scripted responses keyed by frozen
workflow ID and step ID (a shared cursor across repeated child invocations). A response supplies
an external/model output, a human choice/data/feedback, or scripted direct-agent tool calls with
fixture outputs. Read fixtures are required for history/HTTP/MCP/tool/memory reads. Every intended
message/write is captured with resolved inputs and its exact execution path; successful intercepted
outputs are schema-checked fixtures. Model calls consume scripts only. Human choices use the same
submission validation and declared choice routing; comments are outside this synchronous sample.

Direct tool safety: resolve each scripted call against the selected frozen agent's granted tools
and frozen tool snapshot; validate input/output contracts. `ToolKind` is matched exhaustively.
Messages, provisioning, HTTP and slot tools are intercepted, never dispatched. Workflow tools run
their pinned child through the same sample mode with frozen params; static workflow.call and
flow.repeat recursively use the same path. Unknown kinds or missing snapshots fail closed.
Memory save is intercepted even though production currently refuses memory kinds at publication;
no production memory store exists in the sample capability set.

Bounds: enforce compiled per-run max_steps and context limits, the root activation/model/repetition
budgets across descendants, plus conservative sample ceilings for total activations, direct calls,
recursion, request JSON and report bytes. Child budgets cannot replenish root counters. Repeated
steps consume distinct script entries. Missing entries are explicit failures; unused entries are
reported. Sample IDs are separately labelled ephemeral identities with no run-control links.
No database/schema changes or sample rows: a fresh request starts from scratch; no resume API.

UI/API: a Sample tab on the definition pane accepts input, params and scripts as labelled JSON
with a compact example/help, expected draft revision, and the same HTTP/MCP publication selections
as validation. POST requires current company-manager authorization and existing CSRF middleware;
revalidate expected revision and tenant-scoped dependencies before execution. Render the exact
source/content identity, step path/input/output, captured intended effects and diagnostics with
HTML escaping. Retain raw invalid input, show visible running/result status, and state that the
report cannot execute customer effects or be promoted; production admission starts a new run.

Verification: pure/scripted engine tests exercise real compiled fixtures, rule routing, human
feedback/schema validation, two repeat rounds, child/workflow-tool recursion and direct agent tools,
root/step/size limits and missing scripts. Capability tests assert the sample module never imports
production effect adapters, and integration tests compare company production run/job/message/action
counts before/after a sample that traverses write tools and children. A loopback trap provider
must receive zero calls. HTTP tests cover expected-revision conflicts, foreign company/dependency
IDs, private source visibility, CSRF and hostile script output. Browser journey tests copy/edit/
validate/sample/publish/activate, keyboard progress and both themes. B4 has one cohesive independent
review after this architecture gate; final full verification remains mandatory under repository
instructions. Review must assess whether shared engine coverage meets the original requirement,
not accept the capability design alone as proof.

### B3 frozen for independent review

Artifact `8cb2171b82b9e72e6d5a69813b81066dc7012e0fc9e3674815221f338e794b5b`,
HEAD/review-base `da868710d3b235d49856e3e481f8f3735f6b3e56`. B3 delta is
`logs/b3.diff` against accepted B2b R012 `5578887c5eb2b7e4eb0e76b62bb02e1140e9fbd4c5bb8cd415530a368af6ca6d`,
15 files; inherited work remains untouched. Snapshot, per-file hashes and tree are retained.
Exclusions unchanged: p09/docs, p08/docs, COORDINATION.md, marketing, website (full paths in
snapshot invocation and prior records). Coordination r13 and all acknowledged notice revisions
unchanged. Entire source worktree is frozen until independent B3 review completes.

Focused verification on PG55490 (explicit dev/test URLs, SQLX_OFFLINE=true,
CARGO_INCREMENTAL=0): `cargo test --offline --lib decision`54/54;
`cargo test --offline --lib attention`14/14; `cargo test --offline --lib approval`46/46.
`cargo clippy --offline --all-targets -- -D warnings` passes after two needless test borrows fixed;
`cargo build --offline`, `cargo fmt --check`, `git diff --check` pass. Shipped-HTMX browser
protocol filters workflow-decisions/runs/conversations:6/6, including actual Rust markup,
keyboard/tab order, both computed theme colors, unsent data/key retention and queued writes.
New browser fixture generation is included in CI. A first keyboard assertion missed the focusable
Data schema disclosure between fields; corrected to test the actual two-tab sequence.

History SQL stress fixture:50000 decisions,100 runs,22 requested runs,41 heads per run; index
plan1.260ms. Initial plan repeated probes due to join order; materializing eligible runs first
makes the bound explicit (logs/b3/history-explain.*). All checks retained under logs/b3/.
Dev server87015 now runs the B3 binary on3001; migration applied cleanly and history index exists.
SQLx CLI remains unavailable; equivalent SQLX_OFFLINE=false SQLX_OFFLINE_DIR=.sqlx
`cargo check --offline --all-targets` refreshed all23 macro metadata files, no changes (50.65s).
P09-D1r1 B4 design gate is pending with B; no B4 source has been implemented. Final full gate
and F012 remain outstanding.

P09-D1r1 accepted on WF-P09-R013 PASS for immutable decision hash
56dcd3ff0290aa3d04f093c963f57964324a69efaa0c148cc9049387a1bfd07f; no findings.
B3 batch review assigned to same B on frozen8cb2171b artifact. Full app runs browser pass
is running on the phase dev server while B uses isolated test fixtures. No source edits.

B3 full application runs browser on migrated current binary:46/46, including reconnect,
unfinished disclosures, cancellation, loaded-list counts/scroll, and lost-answer rerun keys.
Evidence logs/b3/app-browser.log; source remained frozen.

### R014 corrections — F034/F035

B returned changes required on frozen8cb2171b: F034 unfinished local decision data lost when a
comment response observes concurrent closure; F035 source-message fragment cannot load beyond
newest200 and conflicts with newest-message scrolling. Source unfrozen only after final review.
Both fixes are implemented as one correction batch, tests in progress:
- F034 beforeSwap on comment answers captures the current mounted form (including edits after
  request start), and renders meaningful data/feedback with textContent in a read-only recovery
  panel if the incoming exact decision has no submission form. No keys/policy fields exposed;
  no write is aborted, and comment/submission remains serialized. Browser regression closes the
  decision during a held comment request, then asserts latest draft survives and no write form.
- F035 `/ui/message` authorizes company/channel/thread and reads the requested canonical message
  through the existing bounded exact-message projection. Reuses the normal message renderer and
  source-channel run-link eligibility. Standalone focusable message page has no live stream or
  newest-message scroller; the conversation remains capped200. New real-DB test seeds210 messages,
  reads both old and retained sources, denies foreign/mismatched/private scope, and exports actual
  markup for focus/viewport browser verification. No new production SQL/schema required.
New preexisting files were hash-verified against B3 and copied into tree-b3 before edits.

R014 correction results so far: real-DB exact source210-message/authorization regression1/1;
shipped browser decision+run protocol4/4. The browser reproduces closure during comment, edits
while pending, and hostile draft text; recovery remains readable and non-executable. Both source
fixtures (older-than200 and retained non-newest) receive actual focus and are in the viewport.
Initial compile caught a viewer-id argument mismatch in the new route; corrected. Broader affected
page/decision/stream tests and all-target clippy are running before the single R014 recheck.

R014 correction frozen at `f0ed97efaa607c1efb2cf5077f69fc40106355e9387ae7b9b100020921698785`
(same HEAD, exclusions, coordinationr13); logs/b3-r014.diff, snapshot, hashes/tree retained.
Focused decision40/40, UI streams7/7, source1/1, browser4/4 plus final strengtheneddecision1/1;
clippy alltargets, fmt, whitespace pass; affected page CSS scan0 missing. No new production SQL
or migration; exact-message service wraps the existing indexed projection. Entire source frozen
and single F034/F035 recheck assigned to B. Dev binary remains pre-correction B3 until next build.

### R015 correction — F036

R015 confirms F034/F035 fixed and flags only F036: the reused message bubble had ids/note
controls whose mailbox dialog/detail targets do not exist on a standalone page. Added a typed
read-only message rendering mode; it shares content, tombstone handling, attachments and normal
run links, omits mailbox controls, and preserves ordinary mailbox rendering through its original
public entry point. Changed control generation extracted into a helper; no authorization change.
Newly touched preexisting mailbox.rs was hash-verified against b3-r014 and copied to that tree.
The exact-source DB/browser fixture now includes an active internal note and asserts mailbox
controls still exist in ordinary rendering but not on standalone sources; browser exercises the
remaining Open latest conversation link. Focused checks in progress, source not frozen yet.

F036 correction frozen at `44077e91138695e2b1d48eda0b50ae1617bf451078c4c6f79303b7f5bd9da575`,
4-file delta logs/b3-r015.diff vs R015; snapshots/hashes/tree retained. Source regression1/1,
page tests117/1ignored, browser1/1, clippy alltargets/fmt/diff pass. First test failures were scoped
assertion including logout form and CSP source scanner hostile SQL fixture literal; corrected to
assert the article and bind the hostile string as data. All evidence logs/b3-r015. Single F036
recheck assigned; source frozen. Coordinationr13 unchanged. B4 design remains accepted, codepending.

## B3 accepted; B4 implementation active

R016 PASS accepted after root recomputed44077e91138695e2b1d48eda0b50ae1617bf451078c4c6f79303b7f5bd9da575 and coordinationr13. F034–F036 closed; F012 remains. B4 proceeds under accepted P09-D1r1 (R013, hash56dcd3ff0290aa3d04f093c963f57964324a69efaa0c148cc9049387a1bfd07f). Initial pre-edit B4 bases saved/hash-verified in tree-b3-r015; use that accepted artifact as B4 base. No other resources changed. Final full gate remains required.

## B4 implementation checkpoint — isolated samples

P09-D1r1/R013 remains the implementation contract; B3 R016 accepted44077e…da575 is the delta
base. Root/A authored an inert `application/workflow/sample` evaluator over frozen bundles and
shared production activation/completion/repeat/human validation. The infrastructure facade passes
no service handles. Scripts are keyed by workflow/step and consumed across children. Direct agent
tools require frozen grants, validate contracts, exhaustively dispatch ToolKind into fixture capture
or the pinned sandbox child. Root activation/model/repetition budgets and token/step/request/report/
depth/direct-call ceilings apply; no production persistence or dispatch API is reachable.

The authoring Sample tab revalidates the saved expected revision and publication selections, keeps
raw input on parse/validation errors, labels exact content identity and captures trace/effects.
Protected workflow-tool steps require explicit scripted approval and cannot replace child output.
Sample progress/error status and detached-response navigation guard added to existing editor script.
HTTP tests cover current company-manager authorization, foreign tenant objects, revision conflicts,
escaping, MCP fixture endpoint trap and before/after production table counts for direct workflow-tool
children that capture message writes. Core tests cover two human revision rounds, carried edits and
feedback, rules, external kinds, missing/invalid scripts, direct grants/contracts, root and size bounds.
Full application browser script added for copy/edit/validate/sample/publish/activate in both themes.

Initial offline all-target check passed; subsequent UI check caught Box<str> conversion, corrected.
First core tests4pass/2fail: timer fixture incorrectly null (contract is {deadline}), and feedback
refusal expected specific wording instead of shared schema diagnostic; corrected fixtures/assertion.
Next sample-filter pass13/1fail: test tried changing immutable MCP endpoint; fixed to create the
reviewed connection at the loopback trap endpoint initially, retaining the existing helper wrapper.
No production failure established in these fixture corrections. Current checks in progress:
`cargo test --offline --lib workflow::sample` then `... workflows::ui::authoring`, logs
/private/tmp/p09-b4-{core,authoring}-r3.log. Environment uses ownedPG55490 dev/test, offline=true,
CARGO_INCREMENTAL=0. Browser and wider affected checks/review/full phase gate still owed.

### B4 freeze and focused verification

Frozen artifact: HEAD/base `da868710d3b235d49856e3e481f8f3735f6b3e56`, digest
`c09931634e15711cb7a01c585dd8dc0409b7d89c3ecc7556c96888c34e0bc7c4`.
Exact exclusions unchanged: p09/docs, p08/docs, COORDINATION.md, marketing, website (full paths
in b4-snapshot command above and prior freezes). Delta22files versus acceptedB3 R01644077…da575,
manifest/hashes/diff/tree in logs/b4-*. Coordinationr13 rechecked, noticesC002r1/C003r1/C004r2/C005r1
unchanged. B4 uses no production SQL/schema changes; pure activation preparation is shared with
existing durable activation. MCP result envelopes are shared from the registry for direct tools;
MCP step error fixtures and failed message fixtures stop rather than claiming success.

Focused checks (all with offline cargo, incremental disabled, ownedPG55490 dev/test):
- `cargo test --offline --lib workflow::sample`:9/9 pass, logs/b4/p09-b4-core-final.log.
- `cargo test --offline --lib workflows::ui::authoring`:43/43 pass, authoring-r4.log. The later
  production changes only wrap sample step diagnostics with paths and replace String content_hash
  with the existing transparent ContentHash type; core-final and all-target check cover these.
- `cargo test --offline --lib workflow_activation`:11/11 pass including competing connections.
- `cargo clippy --offline --all-targets -- -D warnings`:PASS before the transparent hash change;
  final full gate will repeat clippy on exact final source.
- `SQLX_OFFLINE=false SQLX_OFFLINE_DIR=$PWD/.sqlx cargo check --offline --all-targets`:PASS;
  supported sqlx macro refresh equivalent (CLI absent), no cache changes in22-file delta.
- `node --test scripts/tests/workflow-samples.mjs` with CHROME_PATH installedChrome:1/1pass;
  failed request keeps raw input, busy cleanup, detached sample response cannot undo navigation.
  Added to existing CI browser command.
- `OUT=/private/tmp/p09-b4-browser-final2 node scripts/tests/workflow-authoring-browser.cjs`:PASS,
  real server on3001/owneddevDB. Reviewed-support copy→edit→validate→scripted draft/review/reply
  sample→publish→binding activate. Both themes computed field radius8px, focused outline-1px,
  distinct text colors; keyboard submit/tabs/disclosures/GoToLine/dirty-confirm/binding actions.
  This supplies remaining B1/F012 keyboard evidence in addition to priorB2/B3 browser evidence.
- `cargo fmt --all -- --check`, `git diff --check`:PASS; sample CSS class scan0missing.

Failures retained in logs/b4: initial fixtures corrected to production timer/MCP contracts and a
report-limit fixture changed to reference one bounded input rather than violate source256KiB.
Browser probes corrected unfocused-style inspection and waited for HTMX form initialization;
unknown-source-field validation has no GoToLine control, so located compiler error was used for
that specific keyboard check. Complete reviewed journey now passes. Existing CSS driftDV-P09-8
remains preexisting; final full gate will report it separately rather than rewriting unrelated CSS.

Environment for final verification: server75596 stopped after browser checks; ownedPG55490
restarted with pg_stat_statements preload, compute_query_id on, statement parameter logging0,
tracktop/utilityoff to match CI. No external/sibling cluster touched. No tests currently running.
B4 source now frozen for independent review. Required final full suite and combined acceptance
remain owed; no phase completion claim. Root/A user-confirmed Astra/high; B /root/reviewer.

### Final gate preparation while B4 review is pending

No source edits after freeze. Non-database gates on c099316…bc7c4: browser lifecycle/protocol
suite15/15 PASS (run counts, SSE navigation, agents, runs, conversations, samples); deployment
script, credential-key rotation, transport boundary regressions and boundary scan PASS;
network guard confirms loopback allowed and IPv4/IPv6 external destinations denied; fmt/patch
whitespace PASS. Evidence logs/final. CSS built to /private/tmp/p09-final-app.css without mutating
the checked-in bundle: still differs, as independently established DV-P09-8. B4 class scan0missing.
Prepared logs/final/run-rust-gates.py (not started): unique owned workflow09_final_* testdatabase,
offlinealltargets/clippy/testcompile, network-isolated alltargets DBsuite, stackbudget, leasebudget.
It retains a failed gate's output and stops to investigate before further broad tests. Do not run
against shared testDB concurrently with B; wait for B4 review/fixes/acceptance first.

### R017 corrections (F037/F038)

IndependentB R017 reviewed fullB4,91focused tests+browserPASS; found LowF037 initial-LF textarea
loss and LowF038 missing promised capability source guard. No production-effect leak; F012 closed.
Corrections: textarea templates use the repository's formatting-newline convention; browser
regression extracts the actual template and checks all3raw fields with zero/one/multiple leading
newlines across repeated renders. Fullapplicationjourney now also submits a leading-LF hostile
value and verifies exact restoration. Added scripts/workflow-sample-boundary.py plus its fixture
regressions, executed inCI. It scans every production sample module recursively, expands grouped
Rust imports, permits inert domain/types/serialization and explicitly named pure engine functions,
and rejects effect dependencies, async/unsafe/extern/include escape paths and unguarded test modules.
Test files use the existing tests.rs/*_tests.rs convention; production may not mount them. The
existing zero-record and loopback behavior tests remain. Rootactivation wildcard import narrowed
to only ActivatedExecution/prepare_activation. No architecturechange.

SourceguardPASS and2guardregressionsPASS; samplebrowser2/2PASS inclnewline. FocusedRust12tests and
newbinarybuild running in13758; logs/private/tmp/p09-b4-r017-*. Finalfullgates notstarted yet.

### R017 correction freeze

Exact450480d565582b0b2e03a867ee624a2e5b5c3e017644951b558807482fd5a00b, HEAD/exclusions unchanged.
Seven-file delta b4-r017.diff vsb4 with matching manifests/tree. BothF037/F038 corrected together;
core+sampleHTTP12/12, actualtextarea/progressbrowser2/2, guardfixtures2/2 andguardPASS, fmt/diffPASS.
BinarybuildPASS; rebuiltownedserver57135 on3001 nowservesexactsource. Strengthenedfulladminjourney
running43828, /private/tmp/p09-b4-r017-app-browser.log. Sourcefrozenforrecheck; coordinationr13
byte-identicaltopinnedcopy. FinalRustdriverstillnotstarted (avoidBsharedDBinterference).

Strengthened actual-app F037 checkPASS on rebuilt exactsource: reviewed-support fulljourney with
leading-LF rawinvalidinput restored, logs/b4-r017/app-browser. Finaloperationsbrowser46/46PASS on
samebinary57135, newdevfixturecompany; results/screens logs/final/runs-browser. Passwordfiles remain
outside repository and were not copied. Source450480…5a00b unchanged; B correctionrecheck active.

### R018 residual F038 correction

B R018 verifiedF037fixed,12Rust+2browserPASS, but foundabsolute `::std` qualifiedpaths bypassed
sourceguard. CorrectedF038guard using tokenized qualifiedpaths rather than adjacencyregex, with
leading:: normalization and whitespace/comment handling. Added negativefixturesforrootedfilesystem,
process,reqwest,groupedrootimportandspaced/commentedpaths. Missingmodule rootnowfailsclosed andraw
stringsrequirelexerreview. Guardand3fixturetestsPASS, gitdiffcheckPASS. Rust/UI unchanged; do not
repeatunaffectedRustchecks. Two-file frozen b4-r018delta versusb4-r017, exact
76d100c63dd8735808ebd7b0c84ee0cbbb4e7932818e28575dc55f0e3030c3ea, HEAD/exclusionssame.
Coordinationr13matchespinnedcopy. BsingleF038rechecknext; finalRustdriverstillowed.

## B4 accepted; final verification active

R019PASS acceptedbyrootafterrecomputing exact
76d100c63dd8735808ebd7b0c84ee0cbbb4e7932818e28575dc55f0e3030c3ea andcomparingcoordinationr13
withpinnedcopy. F037/F038fixed,F012closed; allB1–B4implementationbatchesaccepted inphase.
No integration/phasecompletionclaimyet. FinalRustdriver27864 active, uniqueownedtestDB
workflow09_final_157ec64124 onPG55490; sourcefrozen. Driverlogs/status logs/final/driver.log,
rust-results.json; itstopsatfirstfailedgateforinvestigation. Firstofflinecompilationrunning.
Fullsuite includes test_support::a_fresh_database_applies_only_the_init_schema (checks freshinit,
checksumandrerun), so migrations are exercised through actualSQLxmigrator despiteabsentSQLxCLI.
Finalcombinedindependentreviewwillfollowallgates. Root/AandBreusedhandlesunchanged.

### Final database suite passed

On accepted76d100…c3ea: offline all-target compilation, Clippy-Dwarnings and test compilation PASS.
Network-isolated `cargo test --locked --offline --all-targets` PASS: library2306passed/3ignored,
main5passed, zero failures (324.48s library). Ignored tests are the JS dump developer aid and two
optional live-DNS smoke tests explicitly excluded by network-denying CI. Fresh-init migration test
passed; final DB ledger has exactly successful20260817000000 and checksum equals current migration
SHA-384 (logs/final/migration-ledger.log). Driver now running stock2048KiB stack gate, then lease gate.

### Final gates complete; integration decision pending

All final Rust gates PASS on exact76d100…c3ea (logs/final/rust-results.json):
- locked offline all-target compilation; Clippy all-targets-Dwarnings; test compilation;
- network-isolated all-target suite2311passed/0failed/3intentionalignores;
- stock2048KiB library suite2306passed/0failed/3sameignores,322.82s;
- lease-work budget130passed/0failed,187.13s; every required operation recorded, maximum share
  budget.reserve355/1000ms (35.5%, below50% threshold), no threshold raised.
Fresh init checksum/ledger verified; full suite's fresh migration/rerun test passed.
Final decision/source fixtures regenerated with actual Rust tests2/2PASS, browser1/1PASS on those
fixtures (discussion/draft preservation, closed recovery, exact old/retained/note sources).
Final browser/security/style evidence retained as above. CSS baseline driftDV-P09-8 is the only
red check; no full CSS reproducibility claim. SQLx CLI not installed; direct macro refresh and
actual migration tests cover its needed verification, commands not claimed to have run.

Final integration reviewer R020 read-only review has no findings so far and is awaiting these
last logs. Source remains frozen, no outstanding root test process. Owned server57135 still running;
final DB workflow09_final_157ec64124 retained until B final decision, then dispose owned resources.

## Phase p09 final acceptance and closure

Independent WF-P09-R020 **PASS** for exact artifact
`76d100c63dd8735808ebd7b0c84ee0cbbb4e7932818e28575dc55f0e3030c3ea`, unchanged HEAD
`da868710d3b235d49856e3e481f8f3735f6b3e56`. Root recomputed the same snapshot and compared
coordination r13 byte-for-byte after B's final decision (`logs/final/root-acceptance-snapshot.txt`).
All implementation findings closed; all required affected checks and full Rust/database/stock-stack/
lease gates passed. Existing CSS baseline drift DV-P09-8, three intentional test ignores and absent
SQLx CLI with actual migrator/cache verification remain explicit limitations. See R020 for complete
acceptance mapping and HANDOFF for command/result summary.

Phase09 is complete and accepted in phase, including combined p09 integration. This is not a
whole-plan completion or p08 integration claim. Root's coordination r14 is administrative closure
only: records completed p09 notice dispositions, leaves requirements/notices unchanged, and is
pinned in logs/coordination-r14.md. No substantive implementation changed after review.

Owned server stopped; final disposable DB dropped; owned phase PG cluster stopped with directory,
original dev/test DBs and private server keys preserved. Newly generated final-browser password
files removed; sibling resources and all worktree/review evidence preserved. Cleanup succeeded,
logs/final/cleanup.log. Reviewer completed with no active work. No commits, merges or deployments.
The user's required full-suite gate has been performed, so no redundant permission request remains.

## User-authorized commit and push

After phase acceptance, the user requested "commit, push". The combined dependent p08/p09
working tree is being committed to workflows and pushed to origin/workflows with the retained
review evidence. Precommit digest still matches R020 exactly; remote and local HEAD were equal.
Existing verification and phase-status qualifications remain unchanged.
