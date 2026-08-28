# Keep API Keys Out of Rendered HTML

## Goal

Stop sending decrypted provider API keys to the browser. Encryption at rest landed correctly in
audit item 7; the settings forms undo it on every page render.

## Current Risk

Not part of the original audit — found while verifying that item 7 was complete.

The decrypted secret is rendered verbatim into the response body:

- `src/adapters/http/pages/model_connection.rs:92` —
  `<input type="password"{api_key_id} name="api_key" value="{api_key}" ...>`, fed from
  `company_settings.rs:615`, `agent_settings.rs:613`, `channel_settings.rs:902`
- Legacy pages, still routed: `pages/companies.rs:194`, `pages/agents.rs:344`,
  `pages/channels.rs:906`

`type="password"` masks the field visually. The plaintext is still in the HTML source, in the DOM,
and in anything that caches or logs the response.

The JSON API is clean: all three entities mark the field `#[serde(skip_serializing)]`
(`domain/entities/company.rs:16`, `agent.rs:16`, `channel.rs:83`), and the task-parameter renderer
already masks fields named `api_key` (`pages/tasks.rs:272`). This is the one remaining surface.

Unaffected by the database being empty — this is application code and is wrong regardless of what
the tables hold.

## Design

- Render an empty `value` with a placeholder that says whether a key is stored — `•••• stored`
  versus `Not set`. The form never carries the secret.
- Treat a blank submission as "leave unchanged" rather than "clear" in the matching update
  handlers, so saving an unrelated field does not wipe the key. Give clearing an explicit control.
- Do not load the secret for a page render at all. The persistence layer already has the tool:
  `company.rs:230` and `:249` select `NULL::text AS api_key` for list projections. Extend that
  narrower projection to the settings-form read path.

## Implementation Steps

1. Add a stored-or-not flag to the settings field structs so the template can choose its
   placeholder without receiving the value.
2. Change `model_connection.rs:92` to emit an empty `value`, and the three legacy page renderers
   to match.
3. Update the company, agent, and channel update handlers so a blank `api_key` preserves the
   stored value.
4. Point the settings-form read path at a projection that selects `NULL::text AS api_key`.

## Tests

- Page test per entity: the rendered settings HTML for a record with a stored key contains
  neither the plaintext key nor its ciphertext.
- Handler test: submitting the form with a blank `api_key` and a changed model leaves the stored
  key intact.
- Handler test: the explicit clear control removes the key.
- Existing tests for setting a key on create and on update still pass.

## Acceptance Criteria

- No HTTP response body contains a decrypted or encrypted provider API key.
- Saving a settings form without touching the key field does not change or clear it.
- Clearing a key remains possible through a deliberate action.
