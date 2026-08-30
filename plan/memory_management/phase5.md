# Phase 5 — The UI

Read [`general_plan_instructions.md`](general_plan_instructions.md) first, and
`src/adapters/http/pages/AGENTS.md` before touching any markup — the styling rules there have a
failure mode that looks correct in dark and does nothing in light.

**Goal:** upload, list, watch status, download and delete, on both the company and the agent pane.

Needs phases 1–2 landed to be useful; phases 3–4 to be truthful.

---

## 0. What the surrounding code is

No template engine. Every page is a Rust `format!` over raw `r##"…"##` literals, styled with
Tailwind + daisyUI 5, driven by **htmx 2.0.4**. Strict CSP `script-src 'self'`: **no inline
`<script>`, no `onclick`**. Behaviour attaches through `data-action` delegation
(`EVENT_DELEGATION_SCRIPT` in `mailbox.rs`), and any new JS is a `pub(crate) const &str` appended in
`pages::application_javascript()` (`mailbox.rs:631`).

`/ui` renders through `ui_layout` (daisyUI available). Do not add `rounded-*`, `focus:ring-*` or
`border-*` utilities to individual fields — restyling happens by redefining daisyUI's CSS variables
in one place. Do not copy `input-bordered` / `select-bordered`: they do not exist in daisyUI 5.

---

## 1. The shared fragment — `src/adapters/http/pages/memory_documents.rs` (new)

One renderer, two hosts. Start with `use super::*;`; register with `mod memory_documents; pub use
memory_documents::*;` in `pages/mod.rs`.

```rust
pub struct DocumentManager<'a> {
    pub company_id: Uuid,
    pub agent_id: Option<Uuid>,          // None => the company scope
    pub documents: &'a [MemoryDocument],
    pub error: Option<&'a str>,
    pub can_manage: bool,
    /// False when the company has no ready memory binding: the list still shows, the upload
    /// control is replaced by the reason.
    pub memory_ready: bool,
}

pub fn document_manager(manager: &DocumentManager<'_>) -> String;
```

Renders `<section id="memory-documents-{host}">` — `host` is the agent uuid or `"company"` — holding:

- a heading, and one line of copy naming the two things a reader cannot infer: **external senders
  never see company documents** (the `External` audience gate), and documents share the agent's
  `memory_max_results` budget with conversation memory.
- the **upload control**, copied from `avatar_picker.rs:80`:
  `hx-post`, `hx-encoding="multipart/form-data"`, `hx-target="#memory-documents-{host}"`,
  `hx-swap="outerHTML"`, an `hx-params` whitelist so the enclosing form is never posted along,
  `hx-vals` for the scope context, `hx-disabled-elt="this"`, `hx-indicator`, and
  `accept="{DocumentFormat::ACCEPT_ATTRIBUTE}"` so the browser filters before the bytes are sent.
- **one row per document**: title, a format badge, size via the existing `file_size` helper
  (`mailbox.rs:2101`), relative date via `format_date`, a status badge, a download link when
  `storage_key` is present, and a delete button.
- a **failed** row also shows `ingest_error` and a `Retry` button.
- an empty state: `"No documents yet. Upload one and every agent this channel allows will be able to
  recall it."`
- the count against `MAX_MEMORY_DOCUMENTS_PER_SCOPE`, so the cap is visible before it is hit.

Status badges from `MemoryDocumentIngestStatus::label()`: *Waiting* (ghost), *Adding to memory*
(with a spinner), **In memory** (success), *Failed* (error). **"In memory" and not "Indexed"** —
phase 3 treats provider acceptance as done and does not poll for index completion.

### Progress feedback, which is a rule here and not a nicety

`src/adapters/http/pages/AGENTS.md`: a click must never leave the reader wondering whether the app
received it, and the feedback starts only once the action is *accepted*.

- The upload input disables itself and shows a spinner for the duration.
- **While any row is non-terminal the fragment polls itself** —
  `hx-get="/ui/memory-documents?…" hx-trigger="load delay:2s" hx-swap="outerHTML"` — and the trigger
  is **omitted entirely** once every row is `ready` or `failed`. An always-on poll is a background
  request every two seconds forever; a never-on poll leaves an upload looking stuck.
- Mark the section `aria-busy="true"` while anything is in flight.

### Escaping

Every value here is user-supplied: title, filename, error text, agent name. `escape_html_text` for
text nodes; the attribute/URL encoder for attributes, `hx-vals` and **`hx-confirm`** — escaping for a
text node is not automatically safe inside a quoted attribute. The delete confirm interpolates a
title and is the obvious place to get this wrong.

---

## 2. The two hosts

### Company — a third tab

`company_settings.rs`: `CompanyTab::{Settings, Team, Documents}` (`:59`) with `from_query`
recognising `"documents"`, and `CompanyPaneBody::Documents(&'a str)` beside `Team(&'a str)` (`:81`)
— the body arrives pre-rendered, exactly as the team tab does. Add the third `<a role="tab">` to the
tablist at `:321`; a tab is a whole pane and a plain URL is what makes it shareable, so it stays an
`<a href>`, not htmx.

URL: `/ui/companies?company_id=…&tab=documents`.

### Agent — a section under the form

`agent_settings.rs`: a `documents: Option<&'a str>` field on `AgentEditPane`, rendered **after** the
closing `</form>` (`:236`) but inside the same scrollable body. Never inside the form — the fragment
has its own POST and DELETE, and nesting forms is invalid HTML that browsers resolve unhelpfully.

`None` on the create pane (no id to attach to yet) and for library agents (`company_id IS NULL`),
which have no company memory database. Give the create pane a one-line note saying documents can be
added once the agent is saved.

---

## 3. Routes — `src/adapters/http/routes/ui_memory_documents.rs` (new)

Follow `ui_agents.rs`: a `Workspace` custom extractor (`:105`) instead of five `State(…)`s, entity id
in the path and `company_id` in the query, and **fragments back, never redirects** (there is no HTTP
redirect anywhere in these UI writes).

```
GET    /ui/memory-documents?company_id=…[&agent_id=…]         the list fragment (refresh + poll)
POST   /ui/memory-documents?company_id=…[&agent_id=…]         multipart upload
DELETE /ui/memory-documents/{document_id}?company_id=…
POST   /ui/memory-documents/{document_id}/retry?company_id=…
GET    /ui/memory-documents/{document_id}/download?company_id=…
```

- The upload route carries its own
  `.layer(DefaultBodyLimit::max(MAX_MEMORY_DOCUMENT_BYTES + 64 * 1024))` — the envelope allowance
  mirrors `UPLOAD_BODY_LIMIT` (`ui_uploads.rs:44`). This is the outermost bound; the domain bound in
  `MemoryDocumentUpload::parse` is the second.
- Read the multipart in **one pass** like `read_upload` (`ui_uploads.rs:175`) — a multipart body is a
  stream, the fields cannot be read twice, and the file may arrive before or after them. Take the
  filename from the part's `filename()`, treat it as a hint, and sanitize it.
- **A refusal is the fragment with an error line, not an HTTP error.** Size, format, "no readable
  text", the per-scope cap, the duplicate conflict, and "memory is not ready" all render in place.
- `download` reuses the shape of `ui_attachments.rs`: authorize first, then `BucketKind::Private`,
  then serve with `Content-Disposition: attachment` (ASCII fallback plus RFC 5987),
  `Cache-Control: private, no-store`, `x-content-type-options: nosniff`, and a content type
  downgraded to `application/octet-stream` for anything that is not plainly safe — an HTML
  "document" must never render on-origin.
- Register in `routes/mod.rs` inside the `protected` merge.

Storage stays optional: with no bucket configured the upload still succeeds — the extracted text is
the record of truth — and only the download link is absent.

---

## 4. Tests

`pages/tests.rs`, in the house style: build an entity, call the renderer, assert on exact markup
strings. Every renderer test asserts the htmx wiring, the prefilled values, **and both halves of the
escaping check**.

- `the_document_manager_wires_upload_delete_and_download_to_the_right_ids`.
- `a_document_title_is_escaped_in_the_row_and_in_the_confirm` — positive
  (`contains("Q3 &lt;draft&gt;")`) and negative (`!contains("Q3 <draft>")`), in the text node **and**
  inside `hx-confirm`.
- `the_list_polls_only_while_something_is_pending` — assert `hx-trigger` present with a pending row
  and **absent** when every row is terminal.
- `a_failed_document_shows_its_error_and_a_retry_button`.
- `a_document_without_a_storage_key_offers_no_download`.
- `a_reader_who_cannot_manage_sees_no_upload_or_delete` — assert *absence*
  (`!html.contains("hx-delete=")`), the way the existing permission tests do.
- `the_upload_control_is_replaced_by_a_reason_when_memory_is_not_ready`.
- `the_company_pane_lights_the_documents_tab_and_renders_its_body`.
- `the_agent_edit_pane_puts_documents_outside_the_settings_form` — assert the section id appears
  after `</form>`.
- `a_new_agent_and_a_library_agent_have_no_document_section`.

Route-level tests at the bottom of the route file, building a real multipart body by hand as
`ui_uploads.rs:223` does and driving `Multipart::from_request`.

---

## 5. Styling verification — `cargo test` cannot do this

Nothing in `tests.rs` asserts on classes, radii or outlines, and it should stay that way. Run the
server on `:3001`, open the pane, and read the **computed** style of a real field:

```js
const c = getComputedStyle(document.querySelector('#memory-documents-company input.file-input'));
[document.documentElement.getAttribute('data-theme'), c.borderRadius, c.outlineOffset];
```

**Then flip the theme toggle and read it again.** Both themes, every time: daisyUI's light tokens
arrive at a specificity nothing at token level outweighs, so a change can look right in dark — the
default, and the theme you are probably already in — and do nothing in light.

---

## Done when

- `cargo test` green, including the new page and route tests.
- Both themes verified on computed values, not screenshots.
- The end-to-end walk-through in the general instructions passes, including every refuse path.
- No inline `<script>` and no `onclick` anywhere in the new markup; any new behaviour is a
  `data-action` case.
