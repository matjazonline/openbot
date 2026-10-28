# 01 — Make shared alerts text-only

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Untrusted names and error messages cannot become markup through common response helpers. This
phase covers server rendering; [02](02-team-autocomplete-dom.md) removes the client-side sink and
[03](03-browser-regression-and-release.md) checks the real browser path.

## Implementation

1. Treat `success_alert` and `error_alert` message arguments as plain text and escape them in the
   helper. Audit callers for intentionally supplied HTML; give those a separate typed/template
   rendering path rather than a flag that disables escaping. Validate or constrain the link target
   supplied to `success_alert`, and escape its label and attribute value.
2. Trace user-controlled account names, email addresses, provider errors, and validation messages
   through public registration/login and authenticated HTMX responses. Fix any other shared helper
   that inserts those values into HTML without context-appropriate encoding.
3. Preserve response status and existing HTMX target behavior; the security change must not turn
   a rejected form into a successful write or remove usable error feedback.

## Focused checks and acceptance

Add `security_html_injection_` tests that submit markup-shaped usernames and error values through
the relevant route and assert the response contains encoded text, not new DOM elements. Run
`cargo test --locked --offline --lib security_html_injection_` after confirming the filter selects
tests, plus existing user/alert route tests. Record commands and results.
