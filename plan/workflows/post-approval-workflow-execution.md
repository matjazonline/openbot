To standardize post-approval executions into a configurable workflow engine where steps and post-approval task types can be declared and executed independently of the agent, you can implement a **Declarative Step & Action Dispatcher** pattern.
This separates the system into four decoupled layers:
1. **Workflow & Step Definitions** (What runs next and under what conditions)
2. **Post-Approval Transition Engine** (How the approval transitions into the next task)
3. **Pluggable Action Handlers** (Pure execution ports for external APIs like Gmail, Webhooks, etc.)
4. **Task Worker Dispatcher** (Routing task types to their respective handlers)
### Architecture Diagram
                 [Step 1: Agent Draft]
             (Agent has NO send tool)
                          │
                          ▼
            [Tool Call: `request_approval`]
    ┌──────────────────────────────────────────────┐
    │ HumanApproval Created:                       │
    │  • action_type: "email.gmail_send"           │
    │  • next_task_type: "workflow.action_execute" │
    │  • payload: { to, subject, body, thread_id } │
    └─────────────────────┬────────────────────────┘
                          │
                          ▼
             [Step 2: In-App Human Review]
                   `/ui/approvals/{id}`
                          │
                   User confirms
                          │
                          ▼
         [Post-Approval Transition Engine]
      Enqueues child task: `NewTask::caused_by`
    ┌──────────────────────────────────────────────┐
    │ BackgroundTask (Step 3):                     │
    │  • correlation_id: <inherited from parent>   │
    │  • task_type: "workflow.action_execute"      │
    │  • payload: { handler: "email.gmail_send",   │
    │               params: { ... } }              │
    └─────────────────────┬────────────────────────┘
                          │
                          ▼
             [Step 3: Task Worker Routing]
         Delegates to `WorkflowActionRegistry`
                          │
     ┌────────────────────┼────────────────────┐
     ▼                    ▼                    ▼
[GmailSendAction]   [WebhookAction]     [SlackAction]
(Direct API call)   (Direct HTTP POST)  (Direct API call)
### 1. The Core Abstraction: `WorkflowActionHandler` Port
Define a cohesive trait for all non-agent post-approval execution steps in `src/application/services/`:
#[async_trait]
pub trait WorkflowActionHandler: Send + Sync {
    /// Unique identifier for this action (e.g. "email.gmail_send", "webhook.dispatch", "jira.create_issue")
    fn action_type(&self) -> &'static str;

    /// Execute the action with the leased task context and validated payload
    async fn execute(
        &self,
        ctx: &ActionExecutionContext,
        payload: &serde_json::Value,
    ) -> AppResult<ActionResult>;
}

pub struct ActionExecutionContext {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Option<Uuid>,
    pub correlation_id: CorrelationId,
    pub task_id: Uuid,
    pub lease: TaskLeaseRef,
}
#### Example Handler: `GmailSendActionHandler`
pub struct GmailSendActionHandler {
    credentials: Arc<dyn UserOAuthPersistence>,
    http_client: reqwest::Client,
}

#[async_trait]
impl WorkflowActionHandler for GmailSendActionHandler {
    fn action_type(&self) -> &'static str {
        "email.gmail_send"
    }

    async fn execute(
        &self,
        ctx: &ActionExecutionContext,
        payload: &serde_json::Value,
    ) -> AppResult<ActionResult> {
        let params: GmailSendParams = serde_json::from_value(payload.clone())
            .map_err(|e| AppError::BadRequest(format!("Invalid Gmail payload: {e}")))?;

        // 1. Retrieve user's stored OAuth token
        let token = self.credentials.get_valid_token(ctx.company_id, &params.user_id).await?;

        // 2. Direct API call to Gmail
        let res = self.http_client
            .post("https://gmail.googleapis.com/gmail/v1/users/me/messages/send")
            .bearer_auth(token.secret())
            .json(&serde_json::json!({
                "raw": params.raw_mime_base64,
                "threadId": params.thread_id
            }))
            .send()
            .await?;

        Ok(ActionResult::Completed { external_id: res.json()["id"].as_str().unwrap().to_string() })
    }
}
### 2. Standardized Approval Payload
When an agent requests approval (or when a workflow enters an approval step), the approval payload specifies **what happens on confirmation**:
{
  "summary": "Send response to customer regarding pricing",
  "approval_type": "email.gmail_send",
  "on_approved": {
    "task_type": "workflow.action_execute",
    "action": "email.gmail_send",
    "payload": {
      "to": "client@example.com",
      "subject": "Re: Enterprise Pricing",
      "body": "Hi John, attached is the revised pricing...",
      "thread_id": "18f29bc..."
    }
  },
  "on_rejected": {
    "task_type": "workflow.action_execute",
    "action": "thread.add_internal_note",
    "payload": { "note": "Email draft was rejected by reviewer." }
  }
}
### 3. Setting the Task Type After Approval (`settle_task`)
When the reviewer clicks **"Confirm & Execute"** in `/ui/approvals/{id}`, the transition engine settles the approval and triggers the next step.
There are two approaches to transition the task:
#### Recommended: Chained Task via `NewTask::caused_by`
Rather than mutating the old task in-place, the platform spawns a child task that inherits the `CorrelationId`:
// Inside transition_approval / settle_task in src/adapters/persistence/approval/transitions.rs
if approved {
    if let Some(on_approved) = approval.payload.get("on_approved") {
        let next_task_type = on_approved["task_type"].as_str().unwrap_or("workflow.action_execute");
        let action_payload = on_approved.clone();

        // Spawn chained task inheriting correlation_id and thread context
        let next_task = NewTask::caused_by(
            &parent_task,
            parent_task.channel_id,
            parent_task.thread_id,
            next_task_type,
            action_payload,
        );

        self.task_persistence.insert_task(&next_task).await?;
    }
}
**Why chained tasks are superior:**
1. **Isolated Retry Budgets:** If the Gmail API experiences a temporary 503 error, only Step 3 retries. The agent prompt is not re-executed, and the human is not asked to approve again.
2. **Clear Audit History:** On the Task Board / Monitor (`/ui/tasks`), you can inspect the full causal timeline:
- `Task 1 (agent_draft)` $\rightarrow$ *Completed*
- `Approval (human_review)` $\rightarrow$ *Approved*
- `Task 2 (workflow.action_execute)` $\rightarrow$ *Completed*
### 4. Routing in `TaskWorker` via `ActionRegistry`
In `src/application/services/task_worker.rs`, standardize how tasks are routed based on `task_type`:
// src/application/services/task_worker.rs
async fn run_task(&self, task: &BackgroundTask, lease: TaskLeaseRef) -> Result<TaskExecutionOutcome, RunFailure> {
    match task.task_type.as_str() {
        // 1. Existing agent runs
        AGENT_DISPATCH_TASK => self.run_agent_task(task, lease).await,
        SCHEDULED_AGENT_RUN_TASK => self.run_scheduled_task(task, lease).await,

        // 2. Extensible post-approval / workflow action executor
        "workflow.action_execute" => {
            let action_name = task.payload["action"].as_str()
                .ok_or_else(|| RunFailure::Terminal("Missing action name in payload".into()))?;

            let handler = self.action_registry.get(action_name)
                .ok_or_else(|| RunFailure::Terminal(format!("Unknown action: {action_name}")))?;

            let ctx = ActionExecutionContext {
                company_id: task.company_id,
                channel_id: task.channel_id,
                thread_id: task.thread_id,
                correlation_id: task.correlation_id,
                task_id: task.id,
                lease,
            };

            handler.execute(&ctx, &task.payload["payload"]).await
                .map_err(|e| RunFailure::from(e))?;

            Ok(TaskExecutionOutcome::Replied)
        }

        unsupported => Err(RunFailure::Terminal(format!("Unsupported task_type: {unsupported}"))),
    }
}
### 5. Summary of Benefits
Feature	Hardcoded Scripting	Abstract Workflow + Action Registry
Agent Security	Agent holds send tokens (vulnerable to prompt injection)	Agent has zero send tools or credentials
Extensibility	New actions require custom code inside agent prompts	Add any new action (Slack, CRM, Webhook) by implementing one trait
Failure Recovery	Network failures require restarting the whole agent run	Only the external action step retries
Reusability	Post-approval logic is tied to email	Any workflow (refunds, deployments, emails) shares the same pipeline



TODO:
- WorkflowActionRegistry should also handle existing agent call/execution after approval
- how/where do we configure action after approval
- we should also be able to use workflow actions as tool calls (one tool that calls workflows) so agents can call/execute a predefined workflows with params
- can we define basic reusable workflow step types like http calls etc so we can reuse them with params in workflow
- workflow steps should have shared context so each step can read values from previous steps and set its own
- workflows could be defined with yaml or json
- everything could be workflow - the channel can set workflow for its message handling and ai-run is just (only) step in its workflow, so we could define workflow with: ai-run=>HITL=>gmail-send
- HITL is basically a decision step performed by human
- is sending/replying to message provider (email, slack,...) also part of workflow steps?
- we could have a step to load memory that is passed to agent in later step so we have transparent procedure we can modify with different steps
- we could have classifier step before to include right tools/skills into context
