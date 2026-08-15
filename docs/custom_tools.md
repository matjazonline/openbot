# Custom Agent Tools

`mail-agents-server` provides one application-owned custom tool:

```text
outreach_and_await_quorum
```

The tool sends an individual email to each external recipient, pauses the current background task, and resumes it after the configured percentage of distinct recipients replies. If the deadline expires first, the configured approver can proceed with available responses, extend the deadline, or stop the task.

## Agent YAML

Custom tools are implemented and registered by the Rust server, but an agent must explicitly grant them in its YAML configuration. Registration alone does not expose a tool to the model.

```yaml
name: VendorResearchAgent
system_prompt: |
  You coordinate requests for information from external contacts.
  Use outreach_and_await_quorum when the task requires replies from third parties.
  Use one recipient and a 100 percent threshold for single-party delegation.
  After the outreach resumes, synthesize the received replies for the original requester.

llm:
  provider: openai
  model: gpt-5.4-mini

tools:
  - name: outreach_and_await_quorum

hitl:
  default_timeout_seconds: 86400
  on_timeout: reject
  tools:
    outreach_and_await_quorum:
      require_approval: true
      approval_context:
        - target_emails
        - completion_threshold_percent
        - timeout_hours
        - subject

tool_security:
  tools:
    outreach_and_await_quorum:
      timeout_ms: 10000
      max_output_chars: 4000
      config:
        max_targets: 50
        max_timeout_hours: 720
```

The server supplies the shown HITL and security settings as defaults. Channel and agent configuration is merged over those defaults. Keep approval enabled unless the channel is explicitly trusted to send external mail autonomously.

Do not add task, company, channel, thread, or worker identifiers to the YAML or tool arguments. The server injects those values from the trusted task execution context.

## Tool Arguments

The model calls the tool with:

```json
{
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
| `target_emails` | string array | 1 to `max_targets` valid external addresses; duplicates are normalized and removed |
| `completion_threshold_percent` | number | Greater than 0 and at most 100 |
| `timeout_hours` | integer | 1 to `max_timeout_hours` |
| `subject` | string | 1 to 300 characters after trimming |
| `body` | string | 1 to 20,000 characters after trimming |

Platform addresses under the configured application domain cannot be outreach targets. Each target receives a separate email and is never exposed to the other targets through `To` or `CC`.

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

There is no separate `delegate_to_third_party` tool.

## Execution Lifecycle

1. The model requests `outreach_and_await_quorum`.
2. The configured HITL policy sends an approval request and pauses the task.
3. Approval resumes the original task, and the model repeats the approved tool call.
4. The tool validates and normalizes its arguments.
5. One database transaction creates the outreach, target records, and outbox emails and changes the task to `waiting_for_third_party_reply`.
6. The tool returns immediately. It does not hold an invocation open while waiting for people.
7. The outbox worker sends one message per target and records each outbound `Message-ID`.
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

- The canonical tool ID must be exactly `outreach_and_await_quorum` everywhere.
- A YAML `tools:` grant without the Rust implementation causes agent build validation to fail.
- Omitting the tool from `tools:` means the model has no access to it.
- Tool-specific values under `tool_security.tools.outreach_and_await_quorum.config` are available to the Rust tool through `ToolExecutionContext.custom_config`.
- `timeout_ms` limits creation of the durable outreach, not the human response window. `timeout_hours` controls the response deadline.
- At least one channel participant or company team member must be available as the approver when HITL is enabled.
- Do not place secrets in tool arguments, YAML custom configuration, or tool output.
