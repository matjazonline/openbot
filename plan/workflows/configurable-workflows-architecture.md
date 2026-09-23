# Configurable Durable Workflows Architecture Plan

## Executive Summary

This document specifies the architectural transition of the platform from a hardcoded message dispatch pipeline (`email_agent_dispatch`) to a **Workflow-Native Architecture** ("Everything is a Workflow").

- **Execution Model (Option A - Pure Durable Steps):** Every workflow step is executed as an individually leased, durable `BackgroundTask` row via `NewTask::caused_by`. Each step benefits from independent retry policies, strict lease-fencing (`TaskLeaseRef`), auditable status transitions, and timeline tracking in `/ui/tasks`.
- **Workflow Definitions:** Globally available template YAML definitions committed directly in the repository (under `workflows/`).
- **Clean Slate:** No backward compatibility shims or dual-mode legacy migrations are needed; the database schema and channel configuration will be cleanly redesigned.

---

## 1. Core Architecture & Mental Model

### Workflow Pipeline Overview

```
[Inbound Trigger] (Email, Slack, Webhook, Schedule, or Agent Tool Call)
       │
       ▼
[Channel / Trigger Resolver]
       │ identifies workflow template (e.g., "customer-support")
       ▼
[Task 1 (Step 0: classifier)] ───► Claimed by TaskWorker ───► StepHandler::execute
       │ updates WorkflowContext: steps.classifier = { category: "refund", urgency: "high" }
       │ enqueues child task: NewTask::caused_by(parent, ..., "workflow.step_execute", ...)
       ▼
[Task 2 (Step 1: memory.retrieve)] ───► Queries Hindsight/HydraDB for context
       │ updates WorkflowContext: steps.memory = { facts: [...] }
       │ enqueues child task
       ▼
[Task 3 (Step 2: agent.run)] ───► Calls AgentRunner with dynamic prompt & filtered tools
       │ updates WorkflowContext: steps.draft = { reply_text: "..." }
       │ enqueues child task
       ▼
[Task 4 (Step 3: hitl.approval)] ───► Parks task as `pending_approval`
       │ creates HumanApproval row for reviewer
       │ ── User clicks "Approve" (or edits draft) in /ui/approvals/{id} ──
       │ settle_task enqueues next step: NewTask::caused_by
       ▼
[Task 5 (Step 4: channel.deliver_reply)] ───► Enqueues transactional delivery outbox
       │ updates WorkflowContext: steps.delivery = { status: "queued" }
       │ Workflow ends (no further steps)
```

---

## 2. Answers to Core Design Questions

### 1. `WorkflowActionRegistry` handling existing agent execution after approval
There are two distinct post-approval scenarios handled cleanly by the step registry:
- **Autonomous Post-Approval Actions (e.g. `channel.deliver_reply` or `http.request`):** The agent has already finished drafting; the human approves it. Control does **not** return to the LLM. The engine directly triggers the deterministic action step with the approved (or human-edited) payload.
- **Interactive Tool Approval (Agent Continuation):** If an agent tool call required approval (e.g. `database.run_migration`), the approval step outputs an `AgentResume` action, unparking the harness checkpoint and returning the human's approval payload to the LLM's conversation history.

### 2. How and where to configure actions after approval
Configured directly in the **Workflow Definition (YAML/JSON)**:
```yaml
- id: review_email_draft
  type: hitl.approval
  title: "Review draft reply to {{trigger.from}}"
  payload: "{{steps.draft_reply.content}}"
  on_approved:
    next_step: send_reply
  on_rejected:
    next_step: notify_slack_failure
```
The workflow graph explicitly routes what runs next on approval, rejection, or timeout.

### 3. Workflow Actions as Tool Calls ("Workflow-as-a-Tool")
- Agents should **not** possess raw credentials to external services (Gmail send tokens, Stripe keys, database writes).
- Workflows declare `as_tool: enabled: true` with a schema (e.g., `refund_customer`, `create_jira_issue`).
- The engine compiles these into tool definitions. When the agent calls `refund_customer(amount: 50, reason: "damaged item")`, it triggers that sub-workflow.
- The sub-workflow enforces business rules, policy bounds, rate limits, and HITL gates *externally*, outside the prompt context.

### 4. Basic reusable workflow step types
The core runtime provides standard, reusable primitives:
- **`http.request`**: Reusable REST/Webhook call (URL, method, headers, JSON body with templating).
- **`transform`**: JsonPath / jq expression to reshape outputs.
- **`condition` / `branch`**: Conditional routing (`if steps.classifier.intent == 'refund'`).
- **`agent.run`**: Invokes the LLM harness with dynamic prompt, tools, and memory.
- **`hitl.approval`**: Human decision step.
- **`channel.deliver_reply`**: Native reply to the triggering channel.
- **`memory.retrieve` / `memory.persist`**: Query or write to memory engines.

### 5. Workflows defined in YAML
- Versioned YAML definitions parsed with `serde`.
- Built-in templates stored in `workflows/*.yaml` in the repo.
- Simple expression syntax (e.g., `{{steps.draft.output.text}}` or `{{trigger.message.body}}`).

### 6. "Everything is a Workflow" (Channels bind to Workflows)
- A `Channel` is an ingress listener: it binds an address/connection to a `workflow_template`.
- When an inbound email or Slack message arrives, the channel kicks off an execution instance of that workflow.
- Different channels use different workflows:
  - *VIP Support Channel:* `classifier -> hitl_approval -> direct_agent_call`
  - *General Support Channel:* `memory_lookup -> agent_draft -> hitl_approval -> send_email`
  - *Internal Bot Channel:* `agent_run -> send_reply` (autonomous).

### 7. HITL as a first-class decision step
- Human-in-the-Loop is a standard workflow step: `WorkflowStep::HumanDecision`.
- When reached:
  1. The step creates a `HumanApproval` row and suspends the task (`TaskStatus::PendingApproval`).
  2. The reviewer sees it in `/ui/approvals/{id}` with full context from prior steps.
  3. The reviewer can **Approve**, **Reject**, or **Edit**.
  4. The result (including human edits to the draft) is saved to the step's output context and the workflow advances to `on_approved` or `on_rejected`.

### 8. Is sending/replying part of workflow steps?
- **Yes.** Sending is a step (`channel.deliver_reply`).
- This completely separates drafting from delivery:
  - Workflows can deliver replies across multiple channels (e.g. reply by email **and** post summary to Slack).
  - Draft generation cannot accidentally send emails or leak unreviewed content.

### 9. Step to load memory transparently
- Decouples memory from the agent harness.
- A `memory.retrieve` step queries the memory store (Hindsight, HydraDB) and sets `steps.memory.memories`.
- The subsequent `agent.run` step references `{{steps.memory.memories}}` in its prompt.
- **Benefit:** Full observability. You can see the exact memories retrieved in the task trace before the LLM ran.

### 10. Classifier step before agent run
- A fast, low-cost classifier step (`classifier`) analyzes the incoming message for intent, language, and sentiment.
- The classifier determines which tools and skills the agent actually needs, avoiding prompt bloat and confusing the LLM with unnecessary tools.

---

## 3. Declarative Workflow Specification (YAML Example)

File: `workflows/customer-support.yaml`
```yaml
id: "customer-support"
name: "Customer Support with HITL Quality Gate"
version: 1
description: "Classifies intent, fetches memory, drafts answer, gates on human review, and delivers reply"

as_tool:
  enabled: true
  description: "Triage and handle a support query with human oversight"
  parameters:
    query: { type: "string", description: "Customer inquiry text" }

trigger:
  type: "channel.inbound"

steps:
  - id: "classify"
    type: "classifier"
    config:
      input: "{{trigger.message.body}}"
      categories: ["refund", "bug_report", "general_inquiry", "sales"]
      urgency_levels: ["low", "medium", "high"]

  - id: "retrieve_memory"
    type: "memory.retrieve"
    config:
      query: "{{trigger.message.body}}"
      sender: "{{trigger.message.from}}"
      limit: 5

  - id: "draft_reply"
    type: "agent.run"
    config:
      agent_slug: "support-agent"
      skills:
        - "faq_search"
        - "{{'billing_tools' if steps.classify.category == 'refund' else null}}"
      prompt_context:
        category: "{{steps.classify.category}}"
        urgency: "{{steps.classify.urgency}}"
        memories: "{{steps.retrieve_memory.memories}}"
        original_message: "{{trigger.message.body}}"

  - id: "review_gate"
    type: "hitl.approval"
    condition: "{{steps.classify.category == 'refund' or steps.classify.urgency == 'high'}}"
    config:
      title: "Review support reply to {{trigger.message.from}}"
      summary: "Category: {{steps.classify.category}} | Urgency: {{steps.classify.urgency}}"
      draft_payload:
        to: "{{trigger.message.from}}"
        subject: "Re: {{trigger.message.subject}}"
        body: "{{steps.draft_reply.content}}"
    on_approved:
      next_step: "send_reply"
    on_rejected:
      next_step: "internal_note"

  - id: "send_reply"
    type: "channel.deliver_reply"
    config:
      body: "{{steps.review_gate.approved_body or steps.draft_reply.content}}"
      subject: "Re: {{trigger.message.subject}}"

  - id: "internal_note"
    type: "thread.note"
    config:
      content: "Draft rejected by reviewer: {{steps.review_gate.rejection_reason}}"
```

---

## 4. Execution Loop & Chaining via `NewTask::caused_by`

### Step Progression Algorithm in `TaskWorker`
When `TaskWorker` claims a `BackgroundTask` with `task_type = "workflow.step_execute"`:

1. **Load Workflow & Step:**
   - Deserialize `WorkflowContext` from `task.payload`.
   - Resolve `WorkflowDefinition` from `WorkflowTemplateRegistry` by `workflow_id`.
   - Resolve current step definition by `step_id`.
2. **Evaluate Condition (Skip Check):**
   - If the step has a `condition` and it evaluates to `false`, determine default next step and immediately spawn child task without executing.
3. **Interpolate Expressions:**
   - Render `config` templates using variables from `trigger` and `steps.*`.
4. **Execute Handler:**
   - Lookup handler from `WorkflowStepRegistry` and call `handler.execute()`.
5. **Handle Step Outcome:**
   - **`StepOutcome::Continue { output }` / `StepOutcome::JumpTo`:**
     - Insert `output` into `workflow_context.steps[current_step_id]`.
     - Find next step in workflow graph.
     - If next step exists:
       - Enqueue child task via `NewTask::caused_by(task, task.channel_id, task.thread_id, "workflow.step_execute", next_payload)`.
       - Mark current task as `Completed`.
     - If no next step:
       - Mark current task as `Completed` (Workflow Finished).
   - **`StepOutcome::Suspended`:**
     - Task status remains `PendingApproval` (handled by approval transition logic).

---

## 5. Database Schema Redesign (Clean Slate)

Since database reset is planned:

### 1. Simplify `channels` Table
Drop legacy columns:
- Remove `retrieve_company_memory`, `retrieve_agent_memory`, `retrieve_user_memory`
- Remove `persist_company_memory`, `persist_agent_memory`, `persist_user_memory`
- Remove `response_trigger`, `agent_ids`, `owner_agent_id`
Add clean workflow linkage:
- Add `workflow_template VARCHAR(128) NOT NULL DEFAULT 'customer-support'`
- Add `workflow_overrides JSONB NOT NULL DEFAULT '{}'::jsonb`

### 2. Retain & Leverage Existing Strengths
- Keep `background_tasks` with its robust leasing, generation fences, and attempt accounting.
- Keep `NewTask::caused_by` and correlation indexing for full causal timelines.
- Keep `human_approvals` and `task_approval_waits` as the universal HITL backing.
- Remove redundant tables like `channel_response_reviews` (all review flows now go through universal `human_approvals`).

---

## 6. Implementation Phasing

1. **Phase 1: Domain & Engine Infrastructure**
   - Create `src/domain/entities/workflow.rs` (YAML schema types, AST, template parser).
   - Implement `WorkflowTemplateRegistry` loaded from `workflows/*.yaml`.
   - Implement lightweight template variable interpolator.

2. **Phase 2: Step Handlers & Action Registry**
   - Create `src/application/services/workflow/`:
     - `registry.rs` (`WorkflowStepRegistry`).
     - `context.rs` (`WorkflowContext`).
     - Step handlers: `classifier.rs`, `memory.rs`, `agent.rs`, `hitl.rs`, `deliver.rs`, `http.rs`.

3. **Phase 3: TaskWorker Integration**
   - In `src/application/services/task_worker.rs`, register `"workflow.step_execute"`.
   - Route execution to `WorkflowEngine::execute_step`.

4. **Phase 4: Approval Settle Integration**
   - Update `src/adapters/persistence/approval/transitions.rs` to route post-approval continuations into child workflow step tasks.

5. **Phase 5: Channel & Ingress Decoupling**
   - Update `Channel` domain entity and database schema.
   - Update `thread/ingest/` to enqueue the channel's designated workflow step instead of hardcoded `email_agent_dispatch`.

6. **Phase 6: Workflow Templates & Verification**
   - Write standard YAML templates in `workflows/`:
     - `customer-support.yaml` (Classifier $\rightarrow$ Memory $\rightarrow$ Agent $\rightarrow$ HITL $\rightarrow$ Deliver).
     - `autonomous-assistant.yaml` (Direct Memory $\rightarrow$ Agent $\rightarrow$ Deliver).
     - `triage-only.yaml` (Classifier $\rightarrow$ Slack Alert).
   - Re-run test suite and verify causal task chaining on `/ui/tasks`.
