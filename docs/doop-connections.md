# Configure and verify a company Doop connection

Use the pinned source/image in [the integration record](../integrations/doop/README.md).
The current acceptance uses disposable synthetic data. Confidential use requires the separate
[file-access acceptance](../plan/fix-file-access/README.md). For repeatable local instances,
credentials and teardown, use [the test runner](doop-testing.md).
For rotation, disable/disconnect, backups and retention, use [the operations runbook](doop-operations.md).

## Create the connection

Sign in to Openbot as the intended company's administrator. In Doop, an operator configures
that company's managed bot and workspace through `MANAGED_COMPANY_WORKSPACES`. Invite the
intended humans explicitly; Openbot company membership does not create Doop membership.
Record the bot ID and workspace ID, and have an intended human create a small verification
canvas in that workspace through the normal Doop UI. Record its canvas ID. These are trusted
setup values, never values inferred from a title, model response or arbitrary URL.

Use the company MCP connection page, or the existing authenticated connection API. All paths
below are relative to Openbot and require its normal signed-in session and browser-origin
checks. Replace `COMPANY` and `CONNECTION` with their UUIDs.

`POST /api/companies/COMPANY/mcp-connections`:

```json
{
  "slug": "doop",
  "endpoint_url": "https://doop.example.test/mcp",
  "transport": "streamable_http",
  "enabled": true,
  "auth": { "type": "bearer" },
  "tool_grants": []
}
```

Set the bot bearer key through the connection credential form or
`PUT /api/companies/COMPANY/mcp-connections/CONNECTION/credential`, supplying the current
`expected_revision` and `token`. Read the key from a private credential source; do not put it
in a command line, checked-in request, workflow, catalog recipe or model prompt. Replacement
returns the new connection revision. The existing company-qualified encrypted credential owner
stores it. Identical `doop` slugs in different companies are independent.

Keep the HTTPS endpoint and existing DNS, redirect and TLS policy. A disposable internal origin
needs the existing exact operator-owned endpoint exception; the test runner supplies its own
bounded loopback policy. Endpoint identity cannot be edited in place: follow the existing
connection replacement rules and review a new publication/binding.

## Discover, grant and review

Refresh using the current revision:

`POST /api/companies/COMPANY/mcp-connections/CONNECTION/refresh`

```json
{ "expected_revision": 2 }
```

Read the returned revision and current discovery in the connection page or
`GET /api/companies/COMPANY/mcp-connections/CONNECTION`. Before approving anything, inspect each
tool's complete `input_schema`, `output_schema`, `input_schema_fingerprint` and
`output_schema_fingerprint`. A null output schema/fingerprint means that the provider declared
no structured output contract. Descriptions and `readOnlyHint` are untrusted metadata.

Explicitly grant only the operations in [catalog.json](../integrations/doop/catalog.json) through
the existing connection grant form or update API. The update body contains `expected_revision`
and `definition`, using the same connection definition fields as creation. Each grant is an object
such as `{ "name": "get_profile" }`. Refresh never expands grants. A changed input **or output**
schema removes its old grant; inspect the new contract before granting and reviewing it again.

Review each granted operation through
`PUT /api/companies/COMPANY/mcp-connections/CONNECTION/operations/TOOL`. For a first read review:

```json
{
  "expected_revision": 3,
  "expected_operation_version": null,
  "effect": "read",
  "approval_required": false
}
```

Use the recipe's explicit `effect` and `approval_required` for that tool. The three writes
(`create_canvas`, `create_frame`, `edit_frame_html`) require approval in the shipped recipe.
The server freezes schemas from current discovery; submitting a schema or snapshot is rejected.
Keep the returned operation `version`. A subsequent review or revoke must use that exact version;
a competing review or changed catalog returns a conflict and requires a fresh read.

`GET /api/companies/COMPANY/mcp-connections/CONNECTION/operations` lists reviewed definitions in
bounded pages. Follow its returned cursor to inspect the full selection. The `schema_current`
and `granted` flags distinguish a historical review from current eligibility. Dispatch also
checks live authority and credentials. To revoke, send `DELETE` to the tool's operation path with
`expected_revision` and `expected_operation_version`.

The editing agent's six-tool selection is listed separately in the recipe. Canvas creation,
profile/setup and canvas listing stay outside that agent. Discovery of deletion, imports,
uploads, public publishing, full replacement, streaming or PDF export does not grant them.

## Run the read-only identity verification

Import [verify-company.workflow.json](../integrations/doop/verify-company.workflow.json) through
ordinary workflow authoring, setting its `workflow_id` to the definition being saved. It uses
the existing format version 1, `mcp.call` and `data.map`; it has no model step or external write.
Publish using only `get_profile` and `get_canvas` on the `service` resource slot. For the normal
`POST /api/companies/COMPANY/workflow-definitions/WORKFLOW/publish` request, the selections are:

```json
{
  "expected_revision": 1,
  "command_key": "reviewed-doop-verification-1",
  "mcp": [
    { "connection": "CONNECTION_UUID", "slot": "service", "tool": "get_profile" },
    { "connection": "CONNECTION_UUID", "slot": "service", "tool": "get_canvas" }
  ]
}
```

Bind `service` to this company's reviewed connection. Set these binding parameters from the
trusted setup record and activate the binding through normal workflow configuration:

```json
{
  "expected_bot_id": "BOT_ID",
  "expected_workspace_id": "WORKSPACE_ID",
  "verification_canvas_id": "HUMAN_CREATED_CANVAS_ID"
}
```

Start with empty input `{}`. User input cannot override those parameters or select another
connection. The workflow checks the actual profile, workspace presence, exact workspace, and
authenticated known-canvas identity/workspace. Success returns `verified: true` with those IDs.
Wrong bot, missing workspace, wrong workspace, wrong canvas and wrong canvas workspace stop at
their corresponding named verification steps. An inaccessible canvas fails at `canvas`.

Confirmed MCP credential rejection is a setup error (HTTP 422), distinct from Openbot session
authentication. Before an action enters a possible remote call, the workflow reports
`mcp.authentication_rejected`; transport unavailability retains `mcp.unavailable`. A rejection or
connection loss after possible entry retains uncertainty and reconciliation rather than proving
that no effect occurred or automatically issuing another call.

Verification is a setup proof, not ongoing authorization or membership synchronization. Keep
Doop's current membership guards and Openbot's final-dispatch checks. Changing the intended
workspace requires reviewed binding configuration. Publish the document-creation/editing workflow
only after the intended identity verification succeeds.
