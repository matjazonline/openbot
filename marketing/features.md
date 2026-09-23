# busybots: Product Overview & Feature Matrix
*Enterprise-Grade Autonomous Operations & Deterministic Workflow Platform*

---

## 🌟 The Core Value Proposition (The Hero Pitch)

> **"Put your business operations on autopilot without handing the keys to a black-box LLM."**
>
> Most AI agent platforms make a fatal architectural mistake: they let non-deterministic LLMs control business logic, state transitions, and live API integrations inside an opaque prompt loop. When the model hallucinates or loops, your critical business processes derail.
>
> **busybots replaces black-box agent loops with Deterministic Workflow Steps.** Your business logic, state machines, human approval gates, and external integrations run on a rock-solid, durable workflow engine engineered in Rust. AI agents are invoked only where cognitive reasoning or natural language synthesis is genuinely required—and strictly as bounded, scoped steps within a deterministic execution graph.

---

## ⚡ The Architecture Shift: Deterministic Workflows vs. Uncontrolled Agent Loops

| Traditional "Autonomous Agent" Tools | busybots: Deterministic Workflow Engine |
|---|---|
| **Non-Deterministic Routing:** The LLM decides what tool to call, what to email, and what step comes next. Prone to loops, skipped steps, and unpredictable outputs. | **Guaranteed Orchestration (DAG):** Business logic, conditional branching, prerequisites, and sequences are defined deterministically in code or YAML. The engine guarantees the path. |
| **Monolithic Failure:** If step 4 of an agent prompt fails, the entire prompt context must be re-run from scratch—burning tokens and risking duplicate side effects. | **Durable Step Isolation:** Every step is an individually leased, durable background task in PostgreSQL. If an external API blips, only that exact step retries—with strict idempotency fences. |
| **Credential Exposure:** Agents receive raw API keys or database credentials directly in their LLM context window. | **Zero-Trust Action Handlers:** Agents have zero credentials. After an agent drafts or a human approves, pure deterministic handlers (`WorkflowActionHandler`) execute external side effects. |
| **Unpredictable Cost & Latency:** Multi-turn reasoning loops spin unpredictably, accumulating latency and compounding token bills. | **Predictable Cost & Flow:** Routine data fetching, transformations, and routing cost $0 in LLM fees; models run only for discrete cognitive tasks with bounded token limits. |
| **Opaque Auditing:** Debugging requires sifting through miles of unstructured LLM thought logs. | **Crystal-Clear Observability:** Every step occurrence, payload, evidence artifact, and state transition is captured in a structured timeline in your Kanban/Tasks dashboard. |

---

## 🚀 Part 1: Live Features (Available Today)

### 1. Deterministic Multi-Agent Pipelines & Channel Chaining
*Turn chaotic communications into predictable, orchestrated assembly lines.*
* **Sequential Agent Pipelines (`+` Syntax):** Address requests to chained pipelines in a single line (e.g., `support+billing+legal@yourcompany.com`). busybots deterministically routes context through Step 1 (`support`), aggregates structured output, passes it to Step 2 (`billing`), and then Step 3 (`legal`)—producing a single, cohesive, multi-specialist customer reply.
* **Deterministic Role Context & Mention Triggers:** Channels in `To` execute automatically; channels in `Cc` are recorded for context and execute *only* when deterministically triggered by `@slug` or explicit name mentions.
* **Quiet / Context-Only Ingestion:** Ingest internal notes, call transcripts, or CRM context via `channel.quiet@` or `[[quiet]]` tags directly into thread state—enriching agent memory without triggering unexpected auto-replies.
* **Automated Cron Schedules:** Proactively fire scheduled agent executions (daily reporting, recurring checks) that execute under authorized team member identities with strict schedule leasing.

### 2. Human-in-the-Loop (HITL) & Safe Delegation
*Zero unintended emails. Full AI acceleration with deterministic human gates.*
* **Intelligent Manual Handoff (`external_reply_handling`):** When outside replies arrive on existing customer threads, the system halts autonomous replies and creates a **"Needs Instruction"** handoff. Team members claim the thread, instruct the agent, review generated drafts (**"Drafting"** $\rightarrow$ **"Draft Ready"**), and approve dispatch.
* **Durable Approvals & Outreach Gates:** Sensitive operations or external outreach are parked in `pending_approval` state, surfacing interactive approval cards in `/ui` and email links.
* **Response Reviews:** Outbound AI drafts can be held for peer or manager sign-off, protecting your brand voice and guaranteeing factual accuracy.

### 3. Military-Grade Security, Spam Shield & Loop Prevention
*Defense-in-depth protection before an agent ever touches an input.*
* **3-Stage Inbound Spam & Injection Defense:**
  * *Stage 1 (Local Heuristic Engine):* High-speed Rust-native scanner catching spoofed headers, malformed MIME, and suspicious links.
  * *Stage 2 (External Daemon Scoring):* Deep integration with Rspamd and SpamAssassin daemons.
  * *Stage 3 (Pre-Flight LLM Guardrail):* A dedicated, pre-execution classifier intercepts prompt injections, adversarial jailbreaks, and toxic inputs before the main agent executes.
* **Anti-Loop Defense & Turn Limits:** Strict bounds enforce maximum 20 messages/hour per thread, 5-hop inter-workflow limits, cycle detection, and standard auto-reply headers (`Auto-Submitted: auto-replied`, `X-Auto-Response-Suppress: All`) to permanently block AI-to-AI ping-pong storms.
* **Participant Access Control (ACLs):** Lock channels down to internal team members, an explicit customer email whitelist, or open public access (`@public`).

### 4. Organization Governance & Enterprise Continuity
*Built for companies that cannot afford lost context or dropped accounts.*
* **Role-Based Access Control (RBAC):** Three distinct tiers: **Owner**, **Admin**, and **Member**. Critical operations (billing keys, company deletion, member management) remain exclusive to the Owner.
* **Intelligent Offboarding & Work Handover (`MemberWorkAtStake`):** When removing a departing employee, the platform audits all active schedules, assigned tasks, and open thread handoffs, enforcing atomic reassignment before removal to ensure zero dropped threads.
* **Versioned AES-256 Key Encryption (BYOK):** Connect your company's own AI providers (OpenAI, Anthropic, Gemini, Groq, Ollama, DeepSeek). Credentials use versioned AES-256-GCM encryption with seamless multi-machine key rotation.

### 5. Unified Command Center & Observability
*Complete visibility into every task, delivery, and team member.*
* **Mailbox Workspace (`/ui`):** 3-column live reader (Channels, Threads, Message Bubbles) powered by HTMX and Server-Sent Events (SSE) for instant, reactive updates without page refreshes.
* **Real-Time Kanban & Tasks Workspace (`/ui/tasks`):** Live board tracking all background executions across `Pending`, `Processing`, `Completed`, `Stopped`, and `Dead-Letter` states with correlation-chain lineage inspection.
* **Manual Overrides & Delegation:** Transfer task ownership, assign work to teammates, or halt and resume running tasks with a single click.
* **Deliveries Audit Queue (`/ui/deliveries`):** Comprehensive delivery logs tracking outgoing email statuses, provider response codes, and automatic exponential backoff retries.
* **Executive Dashboard (`/ui/dashboard`):** High-level view of company-wide throughput, latency distributions, and query performance.

---

## 🔮 Part 2: Planned Features & Roadmap (Coming Soon)

### 1. Pure Deterministic Workflow Execution ("Everything is a Workflow")
*Deconstruct monolithic agent prompts into durable, isolated workflow steps.*
* **Discrete Reusable Step Types:** Instead of asking an LLM to orchestrate everything, build pipelines with native, predictable step primitives:
  * `transform`: Instant JSON extraction and reshaping via JsonPath / jq (zero token cost).
  * `condition / branch`: Deterministic if/else routing based on structured step outputs.
  * `http.request`: Reliable outbound REST API & Webhook calls with automated retries.
  * `agent.run`: Isolated cognitive tasks (drafting, summarization) bounded by strict response schemas.
  * `hitl.approval`: Native human approval gates.
  * `channel.deliver_reply`: Transactional message delivery.
* **Per-Step Leases & Idempotency:** Every step is an individually leased `BackgroundTask` in PostgreSQL (`FOR UPDATE SKIP LOCKED`). Worker crashes, timeouts, or network interruptions recover at the exact step that failed, without re-executing completed work.

### 2. Standard Operating Procedures (SOPs) as Versioned Code
*Transform training manuals into executable, auditable operating protocols.*
* **Declarative SOP Engine (`ProcedureDefinitionV1/V2`):** Codify end-to-end business procedures into immutable, versioned DAGs. Define exact step sequences, role requirements (human vs. agent), and required proof of completion.
* **Training Within Industry (TWI) Job Breakdowns:** Guide human operators and AI agents with structured breakdowns: **Action** (what changes), **Key Points** (quality/safety criticals), and **Reasons** (why it matters).
* **Verification Checklists (Checklist Manifesto):** Embed `read_do` and `do_confirm` checklist pause points before irreversible operations (e.g., sending high-value quotes, processing refunds).
* **Structured TeamSTEPPS Handoffs:** Guarantee zero loss of context when passing work between human reps and AI agents with structured acknowledgments and uncertainty flags.
* **Lean Continuous Improvement (PDSA):** Real-time tracking of active work time vs. idle wait bottlenecks, WIP limits, and version-to-version performance comparisons.

### 3. Declarative Action Registry ("Workflow-as-a-Tool")
*Empower agents to propose actions without giving them direct execution power.*
* **Zero-Trust Execution Ports:** Agents never hold direct database credentials, Stripe tokens, or email send privileges.
* **Pluggable Action Handlers (`WorkflowActionHandler`):** Agents output a structured proposal (e.g., `refund_customer(amount: $50)`). Once a manager approves, busybots' deterministic handler executes the Stripe API call securely outside the LLM context.
* **Checkpoint & Resume:** If an agent requests an action that requires approval, the harness state parks securely and automatically resumes when the human approves.

### 4. Turnkey Business Workflow Package Library
*1-click installation of battle-tested enterprise workflows.*
* **Curated Solutions Marketplace:** Pre-configured packages for common business processes (e.g., *Customer Support Triage*, *HubSpot Quote Preparation*, *VIP Ticket Escalation*, *Invoice Dispute Resolution*).
* **Visual Setup & Dry-Run Wizard:** Review step previews, bind human/agent roles, configure channel triggers, and run sandboxed sample simulations before going live.
* **Safe Version Adoption:** Test new workflow releases in preview mode with clear diffs before rolling out to active business channels.

### 5. Multi-Channel Expansion: Native Slack & Webhook Ingress
*Universal orchestration across email and team chat.*
* **Bi-Directional Slack App:** Full Slack workspace integration with interactive block-kit approvals, channel bindings, and thread synchronization.
* **Transport-Neutral Core:** Seamlessly cross-post or escalate conversations between Email, Slack, and internal webhooks under a unified identity and ACL model.

### 6. Ephemeral MicroVM Sandbox Execution
*Secure, air-gapped code and document interpretation.*
* **Hardware-Isolated MicroVMs:** Spin up millisecond-speed isolated Linux microVMs where agents can safely analyze untrusted spreadsheets, run Python/Bash data analytics, and transform documents without touching host infrastructure.

---

## 🏆 The busybots Advantage

```
┌─────────────────────────────────────────────────────────────┐
│                          busybots                           │
├──────────────────────────────┬──────────────────────────────┤
│    WHERE AGENTS SHINE        │   WHERE DETERMINISM MATTERS  │
│   (Cognitive & Generative)   │      (Workflows & SOPs)      │
├──────────────────────────────┼──────────────────────────────┤
│ • Understanding natural lang │ • Order of operations & DAG  │
│ • Drafting empathetic replies│ • Approval rules & gates     │
│ • Summarizing history        │ • API credentials & payments │
│ • Extracting unstructured data│ • Database state & retries  │
│ • Proposing next best actions│ • Access control & auditing  │
└──────────────────────────────┴──────────────────────────────┘
```

By decoupling **cognitive agent generation** from **deterministic workflow execution**, **busybots** delivers the speed and power of autonomous AI with the predictability, safety, and auditability of enterprise software.
