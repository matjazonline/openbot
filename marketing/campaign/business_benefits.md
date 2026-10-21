# Business Benefits of BusyBots

*Written for buyers: ops leads, finance managers, heads of support and sales ops, CTOs of regulated companies. Each benefit names the feature that delivers it and the number or proof point to collect from early-access cohorts before using it publicly.*

## 1. Time back on recurring email work
**What changes.** Invoice matching, RFP first drafts, multi-department customer replies and shipping exceptions move from "someone does it by hand" to "agent drafts, human approves."
**Features.** Forward-and-instruct ingress, attachment parsing, shared company knowledge, Draft Ready cards.
**Illustrative figures from use cases.** Invoice audit from 20 minutes to a 10-second review. RFP turnaround from a week to about an hour. Cross-department reply from three days to minutes.
**Measure in cohort.** Median handling time per thread type before and after; approvals per teammate per day.

## 2. Zero unapproved external communication
**What changes.** No agent can send an email, issue a credit or call an external API on its own. Everything external parks in pending approval.
**Features.** Approval gates, external_reply_handling ("Needs Instruction"), zero-trust action handlers, context isolation that strips internal notes from outbound mail.
**Why buyers care.** Removes the single biggest objection to agents in customer-facing roles: brand and legal exposure from a hallucinated concession.
**Measure.** Count of drafts edited or rejected before dispatch (shows the gate is real and used).

## 3. Lower and predictable AI spend
**What changes.** Routing, branching, parsing and transforms run in compiled Rust with no model call. Models are invoked only for bounded cognitive steps with token limits.
**Features.** Deterministic DAG, step types (transform, condition, http.request), bounded agent.run, BYOK at your negotiated rates.
**Claim hygiene.** `pricing.md` says 60-80% lower operational AI spend. Treat this as a target until measured on at least three cohort companies; publish the measured range.

## 4. Institutional knowledge that compounds
**What changes.** Every resolution, pricing decision and policy clarification lands in a shared thread and knowledge base, not in one employee's private chat history.
**Features.** Shared discussions, [[quiet]] context ingestion, company knowledge base, thread history retrieval.
**Why buyers care.** Onboarding time for new staff drops; "ask Sarah, she knows" stops being the retrieval system.

## 5. Consistency through executable SOPs
**What changes.** Procedures become checklists the agent must satisfy before producing a draft. The same checks run at 3am Sunday as at 9am Monday.
**Features.** SOP engine, read_do and do_confirm checkpoints, TWI job breakdowns, structured handoffs.
**Why buyers care.** Audit readiness, fewer errors from skipped steps, less dependence on individual diligence.

## 6. Reliability under real-world failure
**What changes.** A flaky vendor API or a worker crash retries exactly the failed step. No duplicate emails, no re-run of completed work.
**Features.** Per-step PostgreSQL leases (FOR UPDATE SKIP LOCKED), idempotency fences, exponential backoff in the deliveries queue.
**Why buyers care.** Duplicate customer emails and double refunds are reputational incidents, not bugs.

## 7. Full auditability
**What changes.** Every step occurrence, payload, evidence artifact and state transition is recorded and visible on the Kanban board and deliveries log.
**Features.** /ui/tasks with correlation-chain lineage, /ui/deliveries with provider response codes, executive dashboard.
**Why buyers care.** Compliance teams can answer "who approved this and what did the agent see" without reading model logs.

## 8. Security posture a CISO can accept
**What changes.** Agents never hold credentials. Inbound mail passes two spam stages before a model sees it. Keys are AES-256-GCM encrypted with rotation. Loops are bounded by headers, turn limits and hop limits.
**Features.** Zero-trust handlers, two-stage spam shield, anti-loop defenses, RBAC, planned microVM sandboxes for untrusted attachments.
**Why buyers care.** Prompt injection through a spoofed vendor email cannot move money or leak keys.

## 9. No vendor or model lock-in
**What changes.** Choose OpenAI, Anthropic, Gemini, Groq, DeepSeek or local Ollama per agent. Run the core on your own infrastructure for free, or in a private VPC.
**Why buyers care.** EU data residency, existing enterprise LLM contracts, and leverage in model price negotiations. Direct contrast with Grok Bot (Grok only) and Dots (GPT-6 only, not for EU Pro users).

## 10. Zero friction for everyone outside the company
**What changes.** Customers, vendors and partners keep using email. They never log into anything, and they are never charged as seats.
**Why buyers care.** Adoption risk is the usual killer of new tooling. Here the external side has nothing to adopt.

## 11. Business continuity through staff changes
**What changes.** Offboarding audits a leaver's schedules, assigned tasks and open handoffs and forces reassignment before removal.
**Features.** MemberWorkAtStake protocol, manual overrides and delegation on the task board.
**Why buyers care.** No dropped customer thread when someone leaves on Friday.

## 12. Proactive situational awareness
**What changes.** Scheduled agents surface what needs a human today: SLA risks, pending approvals, stalled deliveries.
**Features.** Cron schedules under authorized identities, morning digest threads.
**Why buyers care.** Leadership attention goes where it is needed instead of across six dashboards.

---

## Benefits by buyer

| Buyer | Lead with | Proof to show |
|---|---|---|
| Finance / AP | Invoice audit scenario, zero-trust refunds, audit trail | Draft card with flagged discrepancy; deliveries log |
| Head of Support | support+billing+legal pipeline, Needs Instruction handoff, SOP checklists | One unified reply vs three; handoff screen |
| Sales ops / Deal desk | RFP scenario, margin SOP, shared pricing knowledge | 85% auto-answered, 15% flagged |
| Operations / Logistics | Exception handoff, in-thread team decision, deterministic warehouse action | Thread with ops lead and AM deciding |
| CTO / CISO | Zero credentials in agents, BYOK, self-host, spam shield, loop limits | Architecture diagram; security page |
| CEO / COO | "Agents think. People lead." Time back, continuity, cost predictability | Cohort before/after numbers |
