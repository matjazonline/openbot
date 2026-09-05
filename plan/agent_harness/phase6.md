# Phase 6 — UI

Read [`general_plan_instructions.md`](general_plan_instructions.md) first. Depends on phases 1–5.

**Goal.** Make all of it editable: a company skill library, an operator-only global skill library,
and an agent capability picker that replaces hand-typed JSON with checkboxes.

Three surfaces, each following one that already exists. Nothing here is novel; the value is in
copying the right precedent.

---

## Rules that bite, before writing any HTML

From `src/adapters/http/pages/AGENTS.md`:

- Two shells. `ui_layout` is daisyUI 5 + Tailwind v4; `base_layout`/`public_layout` are Tailwind only
  with **no daisyUI**. All three surfaces here use `ui_layout`.
- Every action needs visible progress feedback — `aria-busy`, a spinner, `Saving…`. The
  `[.htmx-request_&]:hidden` pattern in `pages/company_resend.rs` is the one to copy.
- Restyle daisyUI by redefining CSS variables, not per-field utilities. Do not use `input-bordered`;
  it does not exist in v5.
- **Escape for the output context** — `escape_html_text` for text nodes, an attribute encoder for
  attributes and `hx-confirm`. This phase renders user-authored skill instructions and tool args
  back into a form, so this is not theoretical.

From `src/adapters/http/AGENTS.md`:

- Scope the object being used, not a sibling: every caller-supplied `skill_id` loads through an
  ownership predicate **in the same statement**. There must be a test attempting another tenant's id
  for every new route.
- Operator data needs an explicit operator check.
- CSRF/Origin on state-changing browser requests.
- `hx-sync="#target:replace"` where reads compete.

And the pattern that makes htmx forms usable: **a rejection re-renders the fragment with the draft
and the message, it does not return a 4xx** — htmx will not swap a 4xx, so a validation error
returned as a status code silently does nothing. `routes/ui_companies.rs`'s Resend handlers are the
model.

## 1. Company skill library

A third tab beside `Settings` and `Team`.

`pages/company_settings.rs` today models the body as an enum holding the *pre-rendered* fragment
rather than a `tab` field beside it, so "Skills is lit" and "the skills pane is showing" are the same
fact. Keep that:

```rust
pub enum CompanyTab { Settings, Team, Skills }

pub enum CompanyPaneBody<'a> {
    Settings,
    Team(&'a str),
    Skills(&'a str),
}
```

Tabs are plain links (`?tab=skills`), not htmx swaps — "a tab is a whole pane, and a plain URL is
what makes one shareable". `CompanyCounts { channels, agents }` gains `skills`, and
`workspace_links` a third entry.

New `pages/company_skills.rs`, following `pages/company_resend.rs` exactly: one `Section<'a>` params
struct holding borrowed data plus `draft: Option<&Draft>`, `error` and `notice`; one public
`fn company_skills_section(section: &Section<'_>) -> String`; small private `fn`s per sub-block
returning `String::new()` when absent.

The list is cursor-paginated with `MAX_SKILL_PAGE_SIZE` (100), deterministic
`updated_at DESC, id DESC` ordering, and next/previous links that preserve `?tab=skills`. The agent
picker fetches a bounded page/search result plus its explicitly selected ids. It must never load an
unbounded company or global library merely because the output is HTML.

Routes on `routes/ui_companies.rs`'s existing router; the `Workspace` extractor gains
`skill_use_cases`.

```
GET    /ui/companies/{company_id}/skills
POST   /ui/companies/{company_id}/skills
GET    /ui/companies/{company_id}/skills/{skill_id}
PUT    /ui/companies/{company_id}/skills/{skill_id}
DELETE /ui/companies/{company_id}/skills/{skill_id}
POST   /ui/companies/{company_id}/skills/from-library
GET    /ui/companies/{company_id}/skills/library       ← bounded safe browse/copy projection
POST   /ui/companies/{company_id}/skills/instruction-row   ← fragment, applies one draft action
```

Use `pane.editable` to hide/disable writes in the owner-only UI, as the Resend panel does; members
get the existing "Only the company owner can edit these settings" card. That flag is presentation,
not authorization: every mutation use case rechecks owner access before persistence.

### The instruction editor

The one genuinely new widget. A repeating list of rows, each a `<select>` for the kind plus the
fields that kind needs:

```
[Prompt ▾]  ┌──────────────────────────────────────┐  [↑] [↓] [✕]
            │ textarea                             │
            └──────────────────────────────────────┘

[Tool   ▾]  tool: [datetime ▾]   output_as: [____]   [↑] [↓] [✕]
            args: ┌──────────────────────────────┐
                  │ {"format": "iso8601"}        │  (JSON object)
                  └──────────────────────────────┘

                                          [+ Add instruction]
```

- The tool `<select>` is populated from `CatalogueTool::grantable()`, so an ungrantable tool cannot
  be selected. That is the picker half of phase 1's guarantee; the compiler filter is the other
  half, and both should exist.
- Add/remove/move/change-kind all post the **complete current draft** to
  `POST …/instruction-row`, with a bounded action value (`add`, `remove:<index>`, `up:<index>`,
  `down:<index>`, `kind:<index>:prompt|tool`). The endpoint parses a flat, explicitly supported form
  representation into `Vec<DraftInstruction>`, applies one action, reindexes it, and swaps the whole
  editor fragment. Do not assume `serde_urlencoded` understands PHP-style names such as
  `instructions[0][kind]`; either use indexed flat field names with a dedicated parser or submit one
  size-bounded JSON field.
- Every row control uses `hx-include` for the enclosing editor, `hx-sync` so the last read/edit intent
  wins, and carries the existing Origin/CSRF protection. The server rejects more than 32 rows and an
  instruction draft body above the serialized-size bound before allocating/parsing the full shape.
  Apply a route-level `DefaultBodyLimit` only slightly above that encoded bound so Axum refuses an
  oversized request before the form extractor buffers it; application validation remains the
  second boundary after decoding.
- **Show the last-step rule in the form, not only in the error.** A skill must end on a prompt step
  (`SkillExecutor` returns only there). The kind `<select>` posts a `change-kind` action, so the
  returned fragment can disable `Save` with a hint — "a skill must end with a prompt step" — when
  the last row is a tool step. Still refuse it server-side, since the client hint is a convenience
  and the server rule is the guarantee.
- Bad `args` JSON is a field-level message on that row, not a whole-form failure that loses the
  other rows.

## 2. Global skill library

Mirror `routes/agent_library.rs` exactly — it is the established operator surface:

```
GET/POST/PUT/DELETE  /api/skill-library[/{id}]
GET                  /ui/skill-library
```

Every route above, reads included, is gated by `require_operator(&user, &users, &config)`, which
checks `config.is_operator(&account.email)` against `OPERATOR_EMAILS` (`infra/config.rs:139`) and
returns **`AppError::NotFound`, not 403**, for
everyone else — the existence of the page is itself operator-only. The nav entry goes in the account
menu, not the rail (`agent_library_entry` in `pages/mailbox.rs:1458` is the precedent), and is
absent entirely for non-operators. `UiSection` (`pages/mailbox.rs:35`) does **not** gain a variant —
a global library is not a rail section.

That operator gate applies to the global management surface, not to the company copy-on-pick
catalogue. The company-scoped `/skills/library` route verifies owner access and returns only the
bounded safe projection needed to browse and copy published skills; it exposes no global edit
controls. Without this distinct read path, a company owner could not use the "Browse library"
action described below.

The editor is the same `company_skills.rs` widget with `company_id = None`. Extract the row renderer
so both pages call it; do not fork it. The existing global agent-library workspace also loads a
bounded page of global skills and uses the capability picker for library agents. It hides
sub-agents, which cannot exist without a company, and permits only global skill ids. Copying that
library agent later copies its attached skills into company-owned rows in the same transaction. A
library agent assigned directly to a company channel is unrestricted among that company's siblings;
show that warning beside the direct-assignment action and recommend copy-on-pick when a restricted
scope is needed.

## 3. Agent capability picker

`pages/agent_settings.rs`, replacing the contents of the collapsed "Custom model & config"
`<details>` (`fn agent_fields()`, :947). `AgentDraft` (:70) gains `harness_kind`,
`granted_tool_ids`, `skill_ids`, `sub_agent_ids`; `SubmittedAgent` (`routes/ui_agents.rs:1207`) and
`AgentForm` (`routes/agent.rs:72`) gain the same. The JSON API payload/response and global-library
payload/response gain the same fields too; form-only support would leave two mutation paths that
silently erase capabilities. `parse_config_form` returns the validated harness-specific config,
not an unrestricted `serde_json::Value`.

Four controls:

1. **Harness** — a `<select>` over `HarnessKind::ALL`. One option today, so display it disabled with
   a note that other runtimes are not yet available **and submit the canonical value in a hidden
   input**; disabled controls are not successful form controls. Iterating `ALL` is what keeps the
   options and stored-value parsing from drifting apart when a second lands.
2. **Tools** — a checkbox grid over `CatalogueTool::grantable()`, grouped by `ToolSource` with
   "Built-in" and "This platform" headings, each with its `description` as help text. Show the
   grant count against the 32 bound. Skills may imply tool grants; show each selected skill's
   required tools beside it and include them in the effective count so attaching an outreach or
   agent-creation skill cannot look less privileged than it is. When a native tool has editable
   policy, show typed bounded controls beneath it (outreach target scope/count/timeouts and directory
   result count); never send users back to `tool_security` JSON. Approval requirements and runtime
   ceilings are displayed as platform-managed, not editable.
3. **Skills** — a multi-select reusing `pages/agent_library_multi_select.rs`, ordered, listing the
   company's skills with `description` as the subtitle. A "Browse library" action opens the
   copy-on-pick flow.
4. **Sub-agents** — an optional allowlist, default empty. Label it for what it means:
   *"Leave empty to let this agent reach every agent in the company."* Empty-means-unrestricted is
   an inversion that reads as a bug to the next person; say it in the UI, not only in a doc comment.
   If the selection is non-empty, direct or skill-implied `create_agent_channel` is an invalid
   combination until an ephemeral-child policy exists; show the field-level explanation and enforce
   it again in the use case.

The raw `config_json` textarea **stays** for the versioned, typed phase-4 advanced options: bounded
reasoning mode/iterations, bounded reflection mode/retries, and the disambiguation enabled flag. Its
label changes to **ai-agents advanced options**. Help text links the exact accepted shape and says
that capabilities, model selectors/prompts, planning, approval, tool security, context,
runtime/spawner/persona, storage, and provider settings are managed elsewhere and rejected here.
Parse it through `AiAgentsAdvancedConfigV1`; never validate and retain a generic JSON object. Hide or
replace this field for another harness rather than reinterpreting the document.

Ordinary Axum URL-encoded forms do not provide a reliable nested/repeated-vector contract here. Use
one shared bounded parser for the hidden comma-separated UUID values already established by
`agent_library_multi_select`, and update those hidden inputs from the vendored `/assets/app.js`.
All list fields use `#[serde(default)]`, so clearing every checkbox means an empty selection rather
than a 422 or preservation of stale grants. The server independently deduplicates, scopes and bounds
every id.

`AgentCreateTab::{Easy, Simple, Advanced}` is unchanged in shape; the capability picker appears on
`Advanced` and on the edit pane.

---

## Tests

Route tests, per `src/adapters/http/AGENTS.md`:
- every new route attempts another tenant's `company_id` and `skill_id` → `NotFound`
- every global skill-library route, including JSON reads, as a non-operator → `NotFound`, not `403`
- a state-changing request without a valid Origin → refused
- a member (not owner) cannot write company skills
- a company-skill write cannot attach a library or another tenant's skill id

Page tests, asserting on rendered HTML as `pages/company_resend.rs` does:
- `the_tool_grid_offers_only_grantable_tools` — assert `command` appears nowhere in the markup
- `a_skill_instruction_with_html_in_its_text_is_escaped` — the XSS regression
- `an_empty_sub_agent_allowlist_renders_the_unrestricted_note`
- `the_harness_select_lists_every_harness_kind`
- `the_disabled_harness_display_has_a_submitted_hidden_value`
- `selected_skills_show_the_tools_they_grant_implicitly`
- `a_library_agent_picker_offers_only_global_skills_and_no_sub_agents`

Form-parse tests:
- `removing_a_middle_instruction_row_reindexes_the_rest`
- `moving_and_changing_a_row_round_trips_the_complete_draft`
- `an_unparseable_args_field_is_a_row_level_message_and_keeps_the_other_rows`
- `a_rejected_save_re_renders_the_draft_rather_than_returning_a_4xx`
- `clearing_every_checkbox_submits_empty_lists_and_removes_old_grants`
- `json_create_and_update_round_trip_every_capability_field`
- `unknown_or_security_owned_advanced_config_paths_are_rejected_with_the_path_named`

## Done when

Every step of the end-to-end walkthrough in `general_plan_instructions.md` passes, including step 5
(the allowlist proof) and step 9 (the sub-agent allowlist), and:

```sh
npm run check:css
cargo fmt --all -- --check
SQLX_OFFLINE=true cargo check --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents" cargo test --locked --all-targets
./scripts/stack-budget.sh
```
