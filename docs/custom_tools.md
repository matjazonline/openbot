# Custom Agent Tools

`mail-agents-server` provides three application-owned custom tools:

```text
create_agent_channel
outreach_and_await_quorum
list_company_agents
```

`create_agent_channel` permanently provisions a specialist agent and a regular channel assigned to it. The child inherits the company's provider, model, and credentials. Creation is atomic, idempotent within the current task, and approval-gated by default. The tool only exists on task-backed runs with a resolved agent identity, so a channel-only configuration cannot impersonate an agent creator.

`outreach_and_await_quorum` sends an individual message to each permitted recipient, pauses the current background task, and resumes it after the configured percentage of distinct recipients replies. It covers both third-party outreach and delegation to another agent in the same company — one tool, because both are "contact someone and wait." Targets are external by default; a same-company channel policy enables durable inter-channel agent calls, see [Inter-Channel Agent Communication](inter_channel_agent_communication.md).

`list_company_agents` is the read-only address book that makes delegation usable: it returns the sibling agent channels this agent may call, with each one's description. Without it, callable addresses have to be hardcoded into a system prompt and go stale silently when a channel is renamed or disabled.

## Agent YAML

Custom tools are implemented and registered by the Rust server, but an agent must explicitly grant them in its YAML configuration. Registration alone does not expose a tool to the model: the runtime's effective tool set is built from the top-level `tools:` list, so `hitl` or `tool_security` entries for a tool that is not in that list are inert.

```yaml
name: VendorResearchAgent
system_prompt: |
  You coordinate requests for information from external contacts.
  When no suitable specialist exists, use create_agent_channel to create one.
  Use list_company_agents to discover callable specialists.
  Use outreach_and_await_quorum when the task requires replies from third parties.
  Use one recipient and a 100 percent threshold for single-party delegation.
  After the outreach resumes, synthesize the received replies for the original requester.

llm:
  provider: openai
  model: gpt-5.4-mini

tools:
  - name: create_agent_channel
  - name: list_company_agents
  - name: outreach_and_await_quorum

hitl:
  default_timeout_seconds: 86400
  on_timeout: reject
  tools:
    create_agent_channel:
      require_approval: true
      approval_context:
        - name
        - slug
        - description
    outreach_and_await_quorum:
      require_approval: true
      approval_context:
        - target_emails
        - completion_threshold_percent
        - timeout_hours
        - subject

tool_security:
  tools:
    create_agent_channel:
      timeout_ms: 10000
      max_output_chars: 2000
    list_company_agents:
      timeout_ms: 5000
      max_output_chars: 4000
      config:
        max_results: 50
    outreach_and_await_quorum:
      timeout_ms: 10000
      max_output_chars: 4000
      config:
        max_targets: 50
        max_timeout_hours: 720
        allowed_target_scope: external_only
```

The server supplies the shown HITL and security settings as defaults. Channel and agent configuration is merged over those defaults. Keep approval enabled unless the channel is explicitly trusted to send external mail autonomously.

Do not add task, company, channel, thread, or worker identifiers to the YAML or tool arguments. The server injects those values from the trusted task execution context.

## Creating an Agent Channel

The model calls `create_agent_channel` with:

```json
{
  "name": "Contract Researcher",
  "slug": "contract-researcher",
  "description": "Researches contract terms and identifies material differences.",
  "instructions": "Analyze the supplied contract question carefully. Cite the relevant clauses, distinguish facts from assumptions, and return a concise recommendation to the delegating agent."
}
```

| Field | Type | Requirements |
|---|---|---|
| `name` | string | Non-empty after trimming; used for both the agent and channel |
| `slug` | string | Valid unused agent and channel slug; normalized with the existing slug rules |
| `description` | string | Non-empty summary shown by `list_company_agents` |
| `instructions` | string | Non-empty system instructions for the new agent |

The company, parent agent, source channel, and task are injected by the server and cannot be selected or spoofed by tool arguments. The parent cannot supply provider, model, credentials, participants, or arbitrary agent/channel configuration.

One transaction creates:

- an agent with the supplied name, slug, description, and instructions;
- an enabled, team-only channel with third-party additions disabled;
- the channel-to-agent assignment; and
- the idempotency record for the current task and normalized request.

The returned JSON contains `created`, `agent_id`, `channel_id`, `channel`, `name`, `slug`, `interfaces`, and `warnings`. Repeating the same normalized request in the same task returns the original pair with `created: false`. It does not create a duplicate or overwrite a resource with a conflicting slug.

Both resources record `created_by` provenance for the parent agent, source channel, and source task. They otherwise remain ordinary persistent resources: owners may edit or delete them, and future agent runs can discover them through `list_company_agents`.

Creation and delegation are deliberately separate. After provisioning, pass the returned `channel` selector to `outreach_and_await_quorum` in `target_channels`:

```json
{
  "target_channels": ["contract-researcher"],
  "subject": "Review limitation-of-liability terms",
  "body": "Compare sections 8 and 12 and report any conflicting caps."
}
```

There is no creation quota or automatic cleanup. Keep approval enabled unless the relevant channel or agent is explicitly trusted to create persistent specialists autonomously.

## Outreach Tool Arguments

The model calls the tool with:

```json
{
  "target_channels": ["supplier"],
  "target_emails": [
    "alice@supplier.example",
    "bob@supplier.example",
    "carol@supplier.example"
  ],
  "completion_threshold_percent": 67,
  "timeout_hours": 48,
  "subject": "Availability confirmation",
  "body": "Please confirm available capacity for the requested delivery window."
}
```

| Field | Type | Requirements |
|---|---|---|
| `target_channels` | string array, optional | Same-company agent channels to delegate to (as `channel` from `list_company_agents`, e.g. `supplier` or `company/channel`) |
| `target_emails` | string array, optional | External email addresses; 1 to `max_targets` total targets across `target_channels` and `target_emails`; duplicates are normalized and removed. Platform channel addresses are refused here — use `target_channels` |
| `completion_threshold_percent` | number, optional | Greater than 0 and at most 100. Omitted means 100 |
| `timeout_hours` | integer, optional | 1 to `max_timeout_hours`. Omitted means `default_timeout_hours` |
| `subject` | string | 1 to 300 characters after trimming |
| `body` | string | 1 to 20,000 characters after trimming |

Platform addresses under the configured application domain cannot be outreach targets in `target_emails` under any policy — name them in `target_channels` instead. `same_company_channels` permits direct agent-channel selectors in the current company and delivers them through trusted internal transport instead of SMTP. Each target receives a separate message and is never exposed to the other targets through `To` or `CC`.

The required response count is:

```text
ceil(number_of_targets * completion_threshold_percent / 100)
```

Examples:

| Targets | Threshold | Required replies |
|---:|---:|---:|
| 1 | 100% | 1 |
| 3 | 50% | 2 |
| 4 | 50% | 2 |
| 10 | 20% | 2 |

## Single-Party Delegation

The quorum tool also covers single-recipient delegation. Use one target and a 100 percent threshold:

```json
{
  "target_emails": ["vendor@supplier.example"],
  "completion_threshold_percent": 100,
  "timeout_hours": 24,
  "subject": "Invoice clarification",
  "body": "Please confirm the tax amount on invoice INV-1042."
}
```

Both optional fields may be omitted, which is the short form a delegated request should use. For delegating to a same-company agent channel:

```json
{
  "target_channels": ["billing"],
  "subject": "Invoice clarification",
  "body": "Please confirm the tax amount on invoice INV-1042."
}
```

Defaults are resolved before the idempotency key is computed, so the short form and the fully-spelled-out form of the same request hash alike — a retry that switches between them re-attaches to the existing outreach instead of sending twice.

There is no separate `delegate_to_third_party` or `delegate_to_agent` tool. Delegation is a target-scope and approval question, not a different action.

## Execution Lifecycle

1. The model requests `outreach_and_await_quorum`.
2. The configured HITL policy sends an approval request and pauses the task.
3. Approval resumes the original task, and the model repeats the approved tool call.
4. The tool validates and normalizes its arguments.
5. One database transaction creates the outreach, its target rows, the question each target was asked, and the delivery that carries it, and changes the task to `waiting_for_third_party_reply`.
6. The tool returns immediately. It does not hold an invocation open while waiting for people.
7. The delivery worker sends one message per target and records the provider key each went out under.
8. Correlated replies are added to the thread as context without creating separate agent tasks.
9. Reaching quorum changes the parent task to `pending` exactly once.
10. The resumed agent receives the original request, collected replies, and an outreach progress summary.
11. The final response is sent to the original requester, and the outreach is marked completed.

Outreach creation is idempotent for the same task and normalized arguments. A resumed agent repeating the same tool call receives the existing outreach state instead of sending duplicate emails.

## Reply Verification

A reply is accepted as an outreach response only when all of these values match:

```text
company + channel + thread
sender email == outreach target
In-Reply-To or References contains that target's outbound Message-ID
```

Only the first response from each target counts toward quorum. Additional correlated replies can remain in thread history but do not increment the response count. Concurrent replies are serialized through database row locking so only one request can cross the threshold and resume the task.

Messages that do not satisfy the sender and outbound-reference checks are rejected as unauthorized thread injection and are not added to the thread.

## Timeout Decisions

When the outreach expires below quorum, the task changes to `pending_approval`. The timeout approval email provides these actions:

| Action | Result |
|---|---|
| `proceed_partial` | Resume with all responses currently available, including none |
| `extend_24h` | Return to waiting with a deadline 24 hours from the decision |
| `extend_48h` | Return to waiting with a deadline 48 hours from the decision |
| `reject` | Cancel the outreach and stop the parent task |

A valid reply arriving while timeout approval is pending still counts. If it reaches quorum, the pending timeout approval expires and the parent task resumes automatically.

## Configuration Notes

- The creation tool ID must be exactly `create_agent_channel` everywhere.
- `create_agent_channel` requires approval by default. Channel or agent configuration may explicitly override that policy through the normal merged HITL configuration.
- The tool is registered only when the run has a durable task and resolved parent agent; its company and provenance never come from model input.
- Creating a child does not call it automatically. Use the returned address with `outreach_and_await_quorum`.
- Dynamically created channels are immediately eligible for `list_company_agents` and internal outreach because they are enabled and have an assigned agent.
- The canonical tool ID must be exactly `outreach_and_await_quorum` everywhere.
- A YAML `tools:` grant without the Rust implementation causes agent build validation to fail.
- Omitting the tool from `tools:` means the model has no access to it.
- Tool-specific values under `tool_security.tools.outreach_and_await_quorum.config` reach the Rust tool because `AgentRunner` reads that path off the merged agent config and hands it to the tool directly. They do *not* arrive through `ToolExecutionContext.custom_config`: the `ai-agents` tool-security engine is disabled unless `tool_security.enabled` is set, and a disabled engine hands every tool an empty custom config.
- Granting a tool in `tools:` is enough to make the model aware of it. A grant is only advertised when the provider has a tool choice, so `ensure_config_fields` sets `llm.tool_choice: auto` for any config that grants at least one tool; without that a config listing `tools:` would run with no tools and no error. Name a choice explicitly only to override it — `required` to force a call, `none` to keep the grant but disable it.
- `allowed_target_scope` accepts `external_only` (default), `same_company_channels`, or `any`.
- `default_timeout_hours` (default 96) fills in an omitted `timeout_hours`. A default above `max_timeout_hours` is rejected, not clamped.
- `internal_requires_approval` (default `true`) governs whether a call whose recipients are *all* same-company agent channels may skip human approval. Anything other than an explicit `false` — absent, malformed, or the wrong type — means `true`.
- `list_company_agents` reads only `max_results` (default 50) from its own `tool_security` config block, and never requires approval.
- `timeout_ms` limits creation of the durable outreach, not the human response window. `timeout_hours` controls the response deadline.
- At least one channel participant or company team member must be available as the approver when HITL is enabled.
- Do not place secrets in tool arguments, YAML custom configuration, or tool output.
