These rules cover how `/ui` is styled. They sit under `src/AGENTS.md`, which still applies to the
Rust in this directory.

Two shells render from here and they are **not** interchangeable:

- `ui_layout` (`mailbox.rs`) — every `/ui` response. Loads daisyUI 5 and Tailwind v4, both from
  CDN, and carries one `<style>` block holding the page CSS consts (`BRAND_LOGO_STYLES`,
  `DARK_THEME_BLUES`, `FIELD_STYLES`).
- `base_layout` / `public_layout` (`layout.rs`, both wrapping `layout`) — login, onboarding, and
  the older `agents.rs`, `channels.rs`, `tasks.rs`, `simulation.rs` pages. Tailwind only, **no
  daisyUI**.

Nothing you add to the `/ui` `<style>` block reaches the second set, and daisyUI component classes
(`input`, `select`, `btn`, `card`) do nothing there. Check which shell a page renders through
before styling it.

# Every user action needs visible progress feedback

When an interaction starts work that does not finish immediately, the UI must visibly acknowledge
it. A click must never leave the user wondering whether the application received it. Show feedback
on the control or region responsible for the work: for example, disable a submitted button, add a
spinner, replace its label with a present-tense status such as `Saving…`, and mark the affected
region `aria-busy="true"`. Keep the feedback visible until the work succeeds or fails.

Start progress feedback only after the action is actually accepted. For native forms, use the
`submit` event rather than the button's `click` event so browser validation failures do not leave
the form looking busy. Prevent accidental duplicate actions while work is pending, but do not
disable unrelated navigation or controls unless using them would corrupt the operation. On an
in-place failure, restore the controls, clear the busy state, and show an actionable error; a full
page response may replace the pending UI instead.

Match the indicator to the scope of the work. Use button-level feedback for a single submitted
action, a local loading or skeleton state for a region being refreshed, and page-level progress
only when the whole page is unavailable. Do not use an indefinite spinner for a process that is
waiting for user input or has otherwise stopped making progress; show its actual state instead.

# Restyle daisyUI by redefining its variables, not its components

daisyUI drives its look from CSS custom properties: `--radius-field` for every field-sized
component (inputs, selects, textareas, buttons, tabs), `--radius-box`, `--radius-selector`,
`--color-primary`, `--color-base-*`. Redefining the variable in the shell's `<style>` block
restyles every component that reads it, in one place — that is what `DARK_THEME_BLUES` and
`FIELD_STYLES` do.

Do not instead add `rounded-*`, `focus:ring-*`, or `border-*` utilities onto individual daisyUI
fields. There are 40-odd fields across `agent_settings.rs`, `channel_settings.rs`,
`company_settings.rs`, `team_settings.rs`, `task_monitor.rs`, `outbox.rs` and `mailbox.rs`, and a
per-field override is a rule that drifts out of sync with the other thirty-nine.

Overriding `border-radius` directly on `.input` / `.select` is also wrong: daisyUI writes the four
corners individually as `border-start-start-radius: var(--join-ss, var(--radius-field))` so that
`.join` can square off the inner edges of a grouped control. A blanket `border-radius` breaks
that. Set the variable.

# A token override that must apply in the light theme needs `!important`

**This is the one that will silently half-work.** daisyUI defines the light theme as:

```css
:root, :root:has(input.theme-controller[value=light]:checked), [data-theme=light] { … }
```

and our theme switch (`THEME_CONTROLLER` in `mailbox.rs`) is exactly that checkbox — a
`.theme-controller` with `value="light"`. So whenever the reader is in light, daisyUI's tokens
arrive at specificity (0,4,1) via the `:has()` arm, and nothing you can write at token level
outweighs it. `:root`, `[data-theme]`, `[data-theme="light"]` are all (0,1,0) and lose.

Dark is not symmetric. Its rule is `:root:has(input.theme-controller[value=dark]:checked),
[data-theme=dark]`, and there is no `value="dark"` controller in this app, so only the (0,1,0) arm
ever matches and a plain `[data-theme="dark"]` override wins on source order. That is why
`DARK_THEME_BLUES` needs no `!important` and `FIELD_STYLES` does.

The failure mode is quiet: the change looks correct in dark — the default, and the theme you are
probably already in — and does nothing in light. If a token override is meant to apply in both
themes, write it `!important` and say why in the const's doc comment.

Ordering matters too. The `<style>` block must stay **after** the two daisyUI `<link>` tags in
`ui_layout`'s `<head>`; equal-specificity rules are decided by source order, and moving it above
them reverts every override at once.

# Let daisyUI's focus outline be the only edge

A focused daisyUI field paints its own 1px border *and* a 2px outline held 2px away from it —
three concentric edges, which reads as a double border. `FIELD_STYLES` pulls the outline to
`outline-offset: -1px` so it lies over the border and one flush 2px ring shows. An outline is not
part of layout, so nothing shifts when focus lands.

That fix covers `.input`, `.select` (including `:open`), `.textarea` and `.file-input`. Checkboxes,
toggles and ranges keep the offset halo on purpose — they have no interior to hold a ring. Don't
extend the rule to them, and don't add `focus:` utilities to individual fields to compensate.

# Don't copy `input-bordered` and friends into new markup

`input-bordered`, `select-bordered` and `textarea-bordered` appear on many existing `/ui` fields
and **do not exist in daisyUI 5** — zero matches in the shipped stylesheet. They are inert v4
leftovers that survived the upgrade. Harmless where they sit, but they are not the reason those
fields have borders (v5 borders every field by default), so don't propagate them.

# Verify a styling change in both themes, on the computed value

`cargo test` will not catch any of this — nothing in `tests.rs` asserts on classes, radii or
outlines, and it should stay that way. Run the server, open `/ui`, and read the *computed* style
of a real field rather than trusting the screenshot:

```js
const c = getComputedStyle(document.querySelector('input.input'));
[document.documentElement.getAttribute('data-theme'), c.borderRadius, c.outlineOffset];
```

Then flip the theme toggle in the top bar and read it again. Both themes, every time — see the
`!important` rule above for why one of them is not evidence about the other.

# Escape for the output context

Every value originating from a user, inbound email, provider, database free-text field, or URL
parameter is untrusted when rendered. Escape text nodes with `escape_html_text`; use a dedicated
attribute/URL encoder for attributes, `hx-confirm`, data attributes, and links. Escaping for a text
node is not automatically safe inside a quoted attribute or JavaScript context.

This includes subjects, sender/display names, message ids, clean and raw bodies, company/channel/
agent names, badges, toast messages, and confirmation prompts. Helpers that return HTML do not make
their arguments safe: either accept an already-escaped type or escape inside the helper exactly
once. Never place inbound HTML directly into the page.

Sanitized Markdown rendered through the shared `ammonia` policy is the only raw-HTML exception.
Keep that sanitizer centralized and test hostile tags, event handlers, URLs, quotes, and markup in
both text and attribute sinks. CSP is a backstop, not a reason to omit escaping.
