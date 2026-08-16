# SHOULD ALREADY BE IMPLEMENTED

Yes. This can work with the existing database schema. No new tables or columns are required.
**Reuse Mapping**
Purpose
Agent A original conversation
A waiting for B
Durable A → B delivery
Agent B independent conversation
Agent B execution
B waiting for third party
B → A result correlation
Resume Agent A
No direct parent-task/child-task relation is necessary. The relationship can be inferred through:
A outreach target
  -> A outreach outbox
  -> A→B Message-ID
  -> B background_tasks.source_message_id
**Message Flow**
M0  Human -> A channel
M1  A channel -> B channel
M2  B channel -> third party
M3  Third party -> B channel
M4  B channel -> A channel
M5  A channel -> human
1. `M0` creates thread `TA` and task `tA`.
2. A sends `M1` through the existing outreach mechanism with B as the single target and a 100% threshold.
3. `tA` changes to `waiting_for_third_party_reply`.
4. Internal delivery of `M1` creates a separate thread `TB` and normal task `tB`.
5. B uses the existing outreach tool to send `M2` and wait for `M3`.
6. Once B has enough information, its normal task response is addressed to the original sender, which is A’s channel.
7. `M4` references `M1`, so it matches A’s existing outreach target.
8. `M4` is added to `TA` as context-only and resumes `tA`; it does not create another A task.
9. A reruns with `M0`, `M1`, and `M4` in its thread history, then sends `M5` to the human.
A and B naturally get separate threads because thread resolution is scoped by channel in `src/application/use_cases/thread.rs:588`.
**Agent A Tool Call**
The existing tool can represent the channel call:
{
  "target_emails": ["agent-b@acme.mailagents.example"],
  "completion_threshold_percent": 100,
  "timeout_hours": 96,
  "subject": "Acquire supplier capacity data",
  "body": "Contact the supplier and return available quantity and earliest delivery date."
}
The current rejection of platform targets in `src/application/services/outreach_tool.rs:272` must be replaced with target-scope validation.
Recommended tool configuration:
tool_security:
  tools:
    outreach_and_await_quorum:
      config:
        allowed_target_scope: same_company_channels
        max_targets: 1

hitl:
  tools:
    outreach_and_await_quorum:
      require_approval: false
Agent B would use:
tool_security:
  tools:
    outreach_and_await_quorum:
      config:
        allowed_target_scope: external_only
This keeps one tool and one persistence implementation while preventing A from bypassing B and contacting external recipients directly.
**Required Changes**
1. **Allow same-company channel targets**
Modify `outreach_tool.rs` to support:
external_only
same_company_channels
any
For `same_company_channels`, verify:
destination is an exact platform channel address
destination company == source company
destination channel != source channel
destination has an agent configured
no pipeline, quiet suffix, or subaddressing
All same-company agent channels can be authorized automatically as requested.
2. **Provide channel resolution to the tool**
The outreach tool currently only receives task persistence. It also needs channel/company lookup so it can validate the destination at execution time.
This affects:
- `src/application/services/outreach_tool.rs`
- `src/application/services/agent_runner.rs`
- `src/application/use_cases/thread.rs`
3. **Deliver same-company messages internally**
Do not send A → B or B → A through public SMTP.
The outbox worker should inspect the destination:
same-company platform channel -> trusted internal ingestion
everything else -> existing SMTP dispatcher
Internal delivery should still construct normal email metadata:
Message-ID
In-Reply-To
References
From
To
X-MailAgents-Channel-ID
X-MailAgents-Hop-Count
X-MailAgents-Trace
This lets the existing message and thread persistence work unchanged.
4. **Add a trusted internal-ingress entry point**
Current code considers a message internal if it contains `X-MailAgents-Channel-ID` or has a platform-looking sender in `src/application/use_cases/thread.rs:265`. That is spoofable through the public webhook or SMTP server.
Use two entry points:
ingest_normalized_message(...)          // untrusted external ingress
ingest_internal_channel_message(...)    // trusted server-generated delivery
Only the internal outbox/direct-dispatch path may call the trusted method. Public headers must never grant internal-channel authorization.
No database field is needed. Trust is an in-process invocation property.
5. **Internally dispatch B’s normal response**
A → B goes through `email_outbox` because it is an outreach message.
B → A is B’s normal task response. Before SMTP dispatch in `src/application/use_cases/thread.rs:1289`, detect that the recipient is a same-company channel and route it through trusted internal ingestion.
The resulting `M4` should be:
From: B channel
To: A channel
In-Reply-To: M1
References: M0 M1
6. **Permit correlated returns through loop detection**
Current cycle detection rejects B → A because A already appears in `X-MailAgents-Trace` at `src/application/use_cases/thread.rs:383`.
Allow a repeated channel only when all of these hold:
delivery came through trusted internal ingress
destination thread already exists
sender exactly matches an outstanding outreach target
In-Reply-To or References contains that target's exact outbound Message-ID
message will be context-only and resume an existing task
All other repeated-channel deliveries remain rejected.
7. **Fix the resumed-task idempotency guard**
This is required even for the existing external outreach workflow.
When A resumes, `find_outbound_reply` can mistake `M1` for A’s final human response because both reply to `M0`. Likewise, B can mistake `M2` for its final response to `M1`.
Update `find_outbound_reply` in `src/adapters/persistence/thread.rs:431` to exclude messages whose `Message-ID` belongs to an outreach outbox:
AND NOT EXISTS (
    SELECT 1
    FROM email_outbox outbox
    JOIN task_outreach_targets target
      ON target.outbox_id = outbox.id
    WHERE outbox.provider_message_id = em.message_id
)
8. **Keep correlated B responses context-only**
The existing flow already does this at `src/application/use_cases/thread.rs:885`:
save M4 in A thread
record outreach response
change A task to pending
skip creation of another A task
That behavior should remain unchanged.
9. **Expose callable channels to Agent A**
Agent A must know which channel to target. This can be provided through either:
system prompt/config with known channel addresses
runtime context containing same-company callable channels
a read-only list_company_agent_channels tool
Runtime context is the smallest option if the list is short.
10. **Separate timeout expectations**
A’s wait timeout must be longer than B’s external outreach timeout:
A waits for B: 96 hours
B waits for third party: 72 hours
Without a direct parent-child task field, B cannot automatically extend A’s timeout. The prompts and tool limits should enforce this relationship.
**Important Invariant**
A→B creates a new B thread and B task.
B→A never creates a new A task.
B→A resumes A’s original task and adds context to A’s original thread.
A’s final response still replies to the original human message.
The only architectural decision to confirm is transport: I recommend direct trusted internal delivery for same-company channels while preserving email-shaped headers and message records, rather than routing internal calls through SMTP.

PROMPT USED:
- implement and use recommended transport instead of smtp 
- add new .md to docs
