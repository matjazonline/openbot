# Custom Agent Tools & HITL Workflows in `mail-agents-server`

This document outlines the architecture, schemas, and workflows for custom agent tools in `mail-agents-server`, specifically focusing on **Third-Party Delegation**, **Sender Verification**, and **Multi-Party Outreach with Quorum Thresholds & HITL Timeout Handling**.

---

## 1. Single-Target Delegation (`delegate_to_third_party`)

### Overview
Allows an LLM agent to send an email query to an external third-party (e.g. vendor, partner, or auditor), pause the background task, and resume when a valid reply is received.

### Tool Definition
```json
{
  "name": "delegate_to_third_party",
  "description": "Delegates an inquiry to an external third party via email and sets task to waiting.",
  "parameters": {
    "type": "object",
    "properties": {
      "target_emails": {
        "type": "array",
        "items": { "type": "string" },
        "description": "List of external email addresses to delegate to."
      },
      "subject": { "type": "string", "description": "Email subject" },
      "body": { "type": "string", "description": "Email body content" }
    },
    "required": ["target_emails", "subject", "body"]
  }
}
```

### Workflow
1. **Agent Call:** The agent invokes `delegate_to_third_party(target_emails=["vendor@supplier.com"], ...)`.
2. **HITL Gate (Optional):** If human approval is required, an email is sent to the administrator with `[Approve]` / `[Reject]` links.
3. **Dispatch & Task Pause:** The email is dispatched via `OutboundDispatcher::send()`. The task payload is updated with the delegation state, and task status transitions to `TaskStatus::WaitingForThirdPartyReply`.
4. **Sender Verification Check:**
   When an inbound reply arrives matching the thread via `In-Reply-To` / `References`:
   - The system checks if `sender` is in `thread.participant_emails` OR in `target_emails` of a `WaitingForThirdPartyReply` task.
   - **Authorized:** Resumes task (`TaskStatus::Pending`) and appends reply to thread history.
   - **Unauthorized (Option A):** Generates `BounceInfo` and sends an automated `[Undeliverable]` bounce email to the unauthorized address without appending to thread history.

---

## 2. Multi-Party Outreach & Quorum Thresholds (`outreach_and_await_quorum`)

### Overview
Enables an agent to send queries to a group of external contacts and wait until a specified percentage (quorum) of responses is received before resuming agent execution.

### Tool Definition
```json
{
  "name": "outreach_and_await_quorum",
  "description": "Sends an email query to a list of recipients and waits until a quorum percentage of responses is reached.",
  "parameters": {
    "type": "object",
    "properties": {
      "target_emails": {
        "type": "array",
        "items": { "type": "string" },
        "description": "List of recipient email addresses."
      },
      "completion_threshold_percent": {
        "type": "number",
        "description": "Percentage threshold of responses needed (e.g., 50.0 for 50%)."
      },
      "timeout_hours": {
        "type": "integer",
        "description": "Maximum hours to wait for responses."
      },
      "subject": { "type": "string" },
      "body": { "type": "string" }
    },
    "required": ["target_emails", "completion_threshold_percent", "timeout_hours", "subject", "body"]
  }
}
```

### Task Payload Schema (`quorum_outreach`)
```json
{
  "quorum_outreach": {
    "outreach_id": "outreach-9876",
    "target_emails": [
      "alice@supplier.com",
      "bob@supplier.com",
      "carol@supplier.com",
      "dave@supplier.com"
    ],
    "total_targets": 4,
    "required_threshold_percent": 50.0,
    "received_responses": ["alice@supplier.com"],
    "current_count": 1,
    "current_percent": 25.0,
    "status": "awaiting_quorum",
    "expires_at": "2026-08-14T10:00:00"
  }
}
```

---

## 3. Partial Quorum Timeout & HITL Intervention

### Overview
If the task timeout (`expires_at`) occurs before reaching the required response percentage, the system does NOT fail or hang indefinitely. Instead, it triggers a **Human-In-The-Loop (HITL)** decision request.

### Timeout HITL Actions & Links

1. **`proceed_partial` (`/approvals/{token}?action=proceed_partial`):**
   - **Action:** Approves proceeding with available partial responses.
   - **Task State:** Resumes task (`TaskStatus::Pending`) with `quorum_outreach.status = "proceed_partial"`.
   - **Thread Message:** Injects system notice into thread history.
   - **Agent Context:** `AgentRunner` receives all collected vendor replies and a context notice:
     `"[Partial Quorum Proceed Notice: Manager approved proceeding with partial data (Received 1/4 responses, 25.0%).]"`

2. **`extend_24h` / `extend_48h` (`/approvals/{token}?action=extend_24h`):**
   - **Action:** Extends the outreach response deadline by 24 or 48 hours.
   - **Task State:** Updates `expires_at = Utc::now() + Duration::hours(X)` and maintains `TaskStatus::WaitingForThirdPartyReply`.
   - **Thread Message:** Injects `"[HITL Timeout Extended]: Manager extended outreach timeout by X hours."`

3. **`reject` (`/approvals/{token}?action=reject`):**
   - **Action:** Cancels the outreach task.
   - **Task State:** Sets task status to `TaskStatus::Stopped`.
   - **Thread Message:** Injects `"[HITL Rejected]: Manager cancelled outreach task after partial timeout."`

---

## 4. Verification & Security Matrix

| Scenario | Sender In `participant_emails` / `target_emails` | Quorum Reached | Task Status Action | Result |
| :--- | :---: | :---: | :---: | :--- |
| **Valid Reply (Awaiting Quorum)** | Yes | No (< 50%) | Retain `WaitingForThirdPartyReply` | Recalculate percent; wait for more replies |
| **Valid Reply (Quorum Met)** | Yes | Yes (>= 50%) | Resume to `Pending` | Agent executes with all replies in context |
| **Unauthorized Sender** | No | N/A | Rejected | Option A Bounce Email dispatched; thread untouched |
| **Timeout (Partial Responses)** | Yes | No (< 50%) | Transition to `PendingApproval` | Email sent to manager with `proceed_partial` / `extend_24h` / `reject` links |
