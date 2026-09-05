# Phase 6 — UI

Read [`general_plan_instructions.md`](general_plan_instructions.md) first. Depends on phases 1–5.

**Goal.** Make all of it editable: a company skill library, an operator-only global skill library,
and an agent capability picker that replaces hand-typed JSON with checkboxes.

Three surfaces, each following one that already exists. Nothing here is novel; the value is in
copying the right precedent.

---

## Rules that bite, before writing any HTML

From `src/adapters/http/pages/AGENTS.md`:

- Two shells. `ui_shell` is daisyUI 5 + Tailwind v4; `base_layout`/`public_layout` are Tailwind only
  with **no daisyUI**. All three surfaces here use `ui_shell`.
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

Routes on `routes/ui_companies.rs`'s existing router; the `Workspace` extractor gains
`skill_use_cases`.

```
GET    /ui/companies/{company_id}/skills
POST   /ui/companies/{company_id}/skills
GET    /ui/companies/{company_id}/skills/{skill_id}
PUT    /ui/companies/{company_id}/skills/{skill_id}
DELETE /ui/companies/{company_id}/skills/{skill_id}
POST   /ui/companies/{company_id}/skills/from-library
POST   /ui/companies/{company_id}/skills/instruction-row   ← fragment, adds a blank row
```

Gate writes on `pane.editable` (owner-only), as the Resend panel does; members get the existing
"Only the company owner can edit these settings" card.

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
- Add/remove rows are htmx fragment swaps against `POST …/instruction-row`; do not reach for
  client-side JS. Rows are named `instructions[0][kind]`, `instructions[0][text]`, … and reindexed
  server-side on parse, so a removed middle row leaves no gap.
- **Show the last-step rule in the form, not only in the error.** A skill must end on a prompt step
  (`SkillExecutor` returns only there). Disable `Save` with a hint — "a skill must end with a prompt
  step" — when the last row is a tool step, and still refuse it server-side, since the client hint
  is a convenience and the server rule is the guarantee.
- Bad `args` JSON is a field-level message on that row, not a whole-form failure that loses the
  other rows.

## 2. Global skill library

Mirror `routes/agent_library.rs` exactly — it is the established operator surface:

```
GET/POST/PUT/DELETE  /api/skill-library[/{id}]
GET                  /ui/skill-library
```

Gated by `require_operator(&user, &users, &config)`, which checks `config.is_operator(&account.email)`
against `OPERATOR_EMAILS` (`infra/config.rs:139`) and returns **`AppError::NotFound`, not 403**, for
everyone else — the existence of the page is itself operator-only. The nav entry goes in the account
menu, not the rail (`agent_library_entry` in `pages/mailbox.rs:1458` is the precedent), and is
absent entirely for non-operators. `UiSection` (`pages/mailbox.rs:35`) does **not** gain a variant —
a global library is not a rail section.

The editor is the same `company_skills.rs` widget with `company_id = None`. Extract the row renderer
so both pages call it; do not fork it.

## 3. Agent capability picker

`pages/agent_settings.rs`, replacing the contents of the collapsed "Custom model & config"
`<details>` (`fn agent_fields()`, :947). `AgentDraft` (:70) gains `harness_kind`,
`granted_tool_ids`, `skill_ids`, `sub_agent_ids`; `SubmittedAgent` (`routes/ui_agents.rs:1207`) and
`AgentForm` (`routes/agent.rs:72`) gain the same, and `parse_config_form` keeps handling the
residual JSON.

Four controls:

1. **Harness** — a `<select>` over `HarnessKind::ALL`. One option today, so render it `disabled`
   with a note that other runtimes are not yet available. Iterating `ALL` is what keeps the options
   and the stored-value parsing from drifting apart when a second lands.
2. **Tools** — a checkbox grid over `CatalogueTool::grantable()`, grouped by `ToolSource` with
   "Built-in" and "This platform" headings, each with its `description` as help text. Show the
   grant count against the 32 bound.
3. **Skills** — a multi-select reusing `pages/agent_library_multi_select.rs`, ordered, listing the
   company's skills with `description` as the subtitle. A "Browse library" action opens the
   copy-on-pick flow.
4. **Sub-agents** — an optional allowlist, default empty. Label it for what it means:
   *"Leave empty to let this agent reach every agent in the company."* Empty-means-unrestricted is
   an inversion that reads as a bug to the next person; say it in the UI, not only in a doc comment.

The raw `config_json` textarea **stays**, for the residual spec — `states`, `process`, `reasoning`
and the rest of `AgentSpec`. Its placeholder and help text change to say that `tools` and `skills`
are now set above and are rejected here.

`AgentCreateTab::{Easy, Simple, Advanced}` is unchanged in shape; the capability picker appears on
`Advanced` and on the edit pane.

---

## Tests

Route tests, per `src/adapters/http/AGENTS.md`:
- every new route attempts another tenant's `company_id` and `skill_id` → `NotFound`
- `/ui/skill-library` as a non-operator → `NotFound`, not `403`
- a state-changing request without a valid Origin → refused
- a member (not owner) cannot write company skills

Page tests, asserting on rendered HTML as `pages/company_resend.rs` does:
- `the_tool_grid_offers_only_grantable_tools` — assert `command` appears nowhere in the markup
- `a_skill_instruction_with_html_in_its_text_is_escaped` — the XSS regression
- `an_empty_sub_agent_allowlist_renders_the_unrestricted_note`
- `the_harness_select_lists_every_harness_kind`

Form-parse tests:
- `removing_a_middle_instruction_row_reindexes_the_rest`
- `an_unparseable_args_field_is_a_row_level_message_and_keeps_the_other_rows`
- `a_rejected_save_re_renders_the_draft_rather_than_returning_a_4xx`

## Done when

Every step of the end-to-end walkthrough in `general_plan_instructions.md` passes, including step 5
(the allowlist proof) and step 9 (the sub-agent allowlist), and:

```sh
cargo fmt --check && cargo clippy --all-targets
DATABASE_URL="postgres://mac03@localhost:5432/mail_agents" cargo test
SQLX_OFFLINE=true cargo build
./scripts/stack-budget.sh
```
