# 02 — Build team suggestions without parsing member HTML

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Depends on [01](01-rendering-contract-and-server-alerts.md).

## Implementation

1. Replace the team dropdown's `item.innerHTML` construction in `APP_SCRIPT` with fixed elements
   created through DOM APIs. Put `email`, `username`, and role labels in `textContent`. Keep the
   existing matching, keyboard/mouse selection, and selected-chip behavior.
2. Audit other `innerHTML`, `insertAdjacentHTML`, dynamic attribute, and URL sinks fed by account,
   company, channel, message, or provider data in `APP_SCRIPT` and generated `/assets/app.js`.
   Replace unsafe sinks with text/DOM construction or a narrowly documented sanitizer. Maintain
   the central CSP as a separate defense.
3. Test with a team member whose editable username contains tags, quotes, and event attributes.
   Another member's autocomplete must show that string as text and create no attacker-specified
   node. Include an email with markup-shaped characters where the domain model permits it.

## Focused checks and acceptance

Run `security_html_injection_` tests and an actual browser test using the served JS and team API
response, not only a string comparison against the implementation. Exercise the existing selection
interaction as a positive control. Record the commands, browser/runtime prerequisites, and result.
