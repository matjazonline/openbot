# Rig agents: configuration and operations

Rig is the only **in-process** agent runtime. It shares the server's privileges and is not a
sandbox. The application owns credentials, authorization, durable workflow runs, decisions and
delivery. The system-prompt generator makes one tool-free Rig completion (`RigTextClassifier`)
with the credential it resolved. That call writes no model usage row, so usage analytics do not
count it.

## Configure an agent

In Agents, open an agent's settings (or create an agent) and save. Company and library forms use
the same fields. Model connections and skills are separate settings. There is no runtime selector,
no runtime configuration and no tool catalogue: the legacy native tools, built-in tools, agent MCP
selections and executable skill tool steps were removed in phase 06C. Workflow tools an agent may
use are named by its `workflow_tools` and dispatched by workflow actions, not by this runtime.

For `POST /api/companies/{company}/agents`, this is a minimal request. The company must already
have a model connection and an enabled model; credentials are not part of this body.

```json
{
  "name": "Assistant",
  "slug": "assistant"
}
```

An agent carries no runtime settings of its own (p08 B4c): payloads naming `config_json`,
`run_timeout_secs`, `response_contract`, `sub_agent_ids` or the retired `harness_kind`,
`granted_tool_ids` and `native_tool_policy` fields are rejected. A run's deadline and its
final-answer contract come from the workflow step that runs the agent.

## Verified compatibility

The following is application support verified with local, request-checked provider fixtures using
the real SDK parsers and tool bridge. It is not a live authentication test or a promise that every
upstream model works. Choose only a model enabled in the company's connection; pilot the particular
model and capabilities before production use.

| Company provider key | Rig transport | Verified fixture coverage |
| --- | --- | --- |
| `google` | Gemini GenerateContent | Text, tool calls/results, continuation, usage, structured validation/repair |
| `openai` | Explicit Chat Completions | Text, tool calls/results, continuation, usage, structured validation/repair |
| `anthropic` | Messages | Text, tool calls/results, continuation, usage, structured validation/repair |
| `groq` | Groq OpenAI-compatible completion client | Text, tool calls/results, continuation, usage, structured validation/repair |
| `xai` | Native xAI Responses | Text, tool calls/results, continuation with distinct call/item IDs, usage, structured validation/repair |

| Capability | Rig behavior |
| --- | --- |
| Tools | The agent's `workflow_tools`, declared to the model and dispatched as workflow actions; the runtime has no built-in, native or MCP tools of its own |
| Skills | Each attached skill's instructions are appended to the agent's prompt; the model follows them |
| Context | The step's `context` input, whatever the workflow passes in (a `context.history` step's thread history, the trigger's input), fenced as untrusted JSON |
| Structured final responses | Optional shared JSON Schema contract, enforced by the application |
| Host access / delegation | No shell, filesystem, HTTP, provider-hosted tools, or Rig-managed sub-agent runtime |
| Streaming / multipart completions | Unsupported |

Skills are prompt steps, not deterministic recipes: appending one to the prompt executes nothing.
An agent attaches at most 16 skills, and a skill has at most 32 steps of up to 8,000 characters
each. Only `agent.run` steps use an agent's skills and tools.

## Structured final answers

A workflow model step's literal `output_schema` is its final-answer contract
(`docs/workflow-language-v1.md`); the application wraps it in the envelope below. These examples
are envelopes as the application parses them.

```json
{
  "response_contract": {
    "version": 1,
    "format": "json_schema",
    "schema": {
      "$schema": "https://json-schema.org/draft/2020-12/schema",
      "type": "object",
      "properties": {
        "status": { "type": "string", "enum": ["done", "needs_input"] },
        "summary": { "type": "string", "maxLength": 500 }
      },
      "required": ["status", "summary"],
      "additionalProperties": false
    }
  }
}
```

A valid answer is `{"status":"done","summary":"The request is complete."}`. Bounded local
references are also supported; this alternative contract accepts the same answer:

```json
{
  "response_contract": {
    "version": 1,
    "format": "json_schema",
    "schema": {
      "$defs": { "status": { "type": "string", "enum": ["done", "needs_input"] } },
      "type": "object",
      "properties": {
        "status": { "$ref": "#/$defs/status" },
        "summary": { "type": "string", "maxLength": 500 }
      },
      "required": ["status", "summary"],
      "additionalProperties": false
    }
  }
}
```

Draft 2020-12 is used when `$schema` is omitted; other dialect declarations are rejected. Schemas
are meta-validated and limited to 65,536 serialized bytes, 4,096 visited nodes and depth 32 including
reference traversal. The envelope is limited to 65,792 input bytes. External/file references and
recursive schemas are rejected; regular expressions use a non-backtracking engine with a 64 KiB
compiled-pattern limit. The JSON examples above are read by the application's parser tests in CI.

A run already prepared with a different contract fails closed rather than delivering under the
new one.

The final candidate must be exactly one JSON value. The application validates it **after
sanitization and before completed-answer persistence**, then checks the frozen publication/delivery
body again. Final response bodies are capped at 64 KiB; checkpoint and serialized-output limits
also apply. Successful JSON delivery omits the product footer. Provider-native structured
generation cannot replace these checks.

An invalid answer can use at most two additional repair calls within the original model, token and
deadline budgets. Repair calls expose no tools. Exhaustion is terminal `invalid_output`, with no
completed answer or answer delivery; restarting a worker does not reset repair counters. Without a
contract, ordinary text remains the default.

## Execution and limits

Every agent runs as a workflow model step, whatever started the run: an inbound message, a
schedule, a manual start or a parent's child call (p08). A model turn's tool calls are saved in
the checkpoint before they run, and execute one at a time with their original arguments and IDs.
Completed results are replayed after a restart, and the final output is consumed once, with the
step's completion.

| Resource | Enforced ceiling |
| --- | --- |
| Model attempts | 8 per model step, shared by continuations and repairs; not configurable per agent or step |
| Tool attempts | 64/run; concurrency 1; individual deadline 30 seconds or remaining run time |
| Input reservation | 131,072/request; 1,048,576/run |
| Output reservation | 16,384/request; 262,144/run |
| Arguments / durable tool result | 64 KiB each; model-facing result also capped at 16,384 characters |
| Serialized checkpoint | 2 MiB including conservative pretty-JSON measurement |
| Execution claims | 64/run |
| Active execution | At most 300 seconds/claim, 3,600 seconds/run; caller can shorten |
| Provider HTTP | 10-second connect, 120-second request, 4 MiB completion body; outer run deadline still applies |

Reservations count serialized input bytes conservatively and charge output/active time upfront.
Unknown requests remain charged; retries, suspension and process restart cannot reset budgets.
Provider automatic retries are disabled. Cancellation stops local work but cannot undo a remote
effect already accepted. The [deployment guide](deploy.md#rig-rollout-on-a-fresh-database) covers
pilot evidence and recovery.

## Company HTTP MCP

In company settings, open **MCP servers**. Create a `streamable_http` connection with an endpoint,
enabled state and authentication type (`none` or `bearer`). Save a bearer token separately, then
use the connection test/refresh to initialize and discover tools. Review discovered schemas and
grant tool names explicitly. Workflow MCP actions call granted tools; agents no longer select
connections (removed in 06C). Definitions and encrypted secrets belong to the company and are
separate from model credentials.

Saving checks local configuration, not connectivity or authentication. Refresh tests both enabled
and disabled definitions; bearer connections require a token first. Discovery grants nothing.
Refresh revokes grants for removed/changed schemas; review and re-grant them before use. Execution
rechecks discovery fingerprints and definition and credential revisions before dispatch.
Endpoint/auth changes clear the token. Library export/copy does not transfer company connections or
secrets.

| Surface under `/api/companies/{company}` | Operation |
| --- | --- |
| `/mcp-connections` | GET catalog; POST definition |
| `/mcp-connections/{id}` | GET; PUT `{expected_revision, definition}`; DELETE `{expected_revision}` |
| `/mcp-connections/{id}/credential` | PUT `{expected_revision, token}`; null removes the token |
| `/mcp-connections/{id}/refresh` | POST `{expected_revision}` |

A definition has `slug`, `endpoint_url`, `transport: "streamable_http"`, `enabled`,
`auth: {"type":"none"}` or `{"type":"bearer"}`, and optional `tool_grants: [{"name":"remote_name"}]`.
Use the revision returned by the most recent read/write. GET exposes `auth.secret_set`, never the
token. All operations require company-management authorization; request bodies are capped at 128 KiB.

Public destinations must use HTTPS with no userinfo, query or fragment. DNS answers are validated
and pinned; redirects, environment proxies and automatic HTTP/session replay are disabled. Only
deployment-owned `MCP_INTERNAL_ENDPOINTS` can allow exact IP-literal URLs, including scheme, port
and path (for example `http://127.0.0.1:8080/mcp`). Startup rejects malformed or hostname exceptions.
Company settings cannot widen this policy. JSON and POST SSE responses are supported; stdio,
legacy SSE, OAuth, unsolicited GET streams, sampling, roots, elicitation, resources and prompts
are not enabled.

Limits: 64 live definitions/company, 32 granted tools/connection, 100 discovered tools/connection, 64 KiB/schema and arguments, 2,048 bytes/URL, 16 KiB/bearer token and 1 MiB wire
data/discovery or call. Discovery has a 30-second total deadline; initialization/call deadlines are
30 seconds, DNS/connect 5 seconds and idle reads 10 seconds. Each process allows 32 sessions total
and four per exact endpoint.

Remote intent is journaled before network dispatch. Completed receipts bypass the network;
uncertain effects become `indeterminate` and are never automatically replayed. There is no automated
reset interface or exactly-once remote-effect guarantee; the run's reconciliation route records an
operator note and resumes nothing. See the recovery procedure in the deployment guide before
retrying an uncertain operation.

## Diagnostics and verification boundaries

A model step's output metadata carries safe run diagnostics: `execution_diagnostics` records
`harness`, `provider`, `model`, `prompt_characters`, `response_characters`, `history_message_count`,
`model_calls`, `tool_call_count`, `tool_names`, `supported_tool_ids`, `token_usage_source`,
`unreported_usage_calls`, `unresolved_model_calls`, `structured_validation`, `repair_call_count`
and `terminal_invalid_output_reason`. Validation is `not_configured`, `pending`, `validated` or
`invalid`. The application adds elapsed duration.

The durable projection adds run/revision/state and wait IDs, reservation counts/limits,
`continuations`, `replayed_invocations` and `indeterminate_effect`. Terminal diagnostics remain
available without a reply. Usage is reported, estimated or mixed; missing counters and unresolved
requests use reserved estimates. Aggregate usage includes repair requests exactly once across
recovery; there is no separate repair-only billing total. These counts are not provider invoices.
Schemas, candidates, prompts, arguments/results, credentials and approval tokens are excluded.

Startup validates configuration, registry coverage and local transport construction; it does not
authenticate company keys or probe model availability. Agent writes/preflight validate typed
settings and capabilities; publishing a workflow validates its steps' `output_schema`s. CI uses
local provider/MCP fixtures plus PostgreSQL for durable approval, competing claims, decision and
delivery tests. Live model authentication and a
controlled MCP endpoint pilot are separate operator checks, not startup guarantees.

The [verification record](rig-verification.md) contains the release
matrix. See [adapter maintenance](../src/adapters/harness/rig/README.md) for exact dependency pins
and upgrade gates, and [rollout](deploy.md#rig-rollout-on-a-fresh-database) for the deployment checklist.
