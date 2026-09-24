# Enterprise Security & Governance: Operational Peace of Mind

> **Our Vision: Run your business. Not the busywork.**  
> *Redefining teamwork for the AI age. Agents think. People lead.*

---

## 🛡️ Built for Enterprise Trust from Day One

Deploying AI in mission-critical operations requires more than impressive demos—it demands ironclad security, complete auditability, and absolute brand protection.

You cannot afford an AI agent emailing confidential terms to a customer, leaking API keys, or executing unauthorized financial adjustments.

**BusyBots is engineered with defense-in-depth architecture in Rust and PostgreSQL**, ensuring that every interaction between your human workforce, your AI agents, and external contacts adheres to strict enterprise governance.

---

## 🔒 The Five Governance Pillars

### 1. Zero Rogue Actions & Brand Protection ("People Lead")
In BusyBots, AI agents are strictly advisory until authorized by a human:
- **Approval Gates:** External emails and sensitive side effects are parked in `Needs Instruction` or `Draft Ready` status until an authorized teammate clicks **[Approve & Dispatch]**.
- **Context Isolation:** Internal deliberations, private agent whispers (`[[quiet]]`), and margin debates stay strictly within your internal shared discussion space. They are physically barred from leaking into customer-facing email replies.

### 2. Zero-Trust Action Handlers
Most agent platforms expose raw database credentials, Stripe tokens, or CRM API keys directly in the LLM's prompt context—making them vulnerable to prompt injection or extraction attacks.
- **Pure Structured Proposals:** BusyBots agents have zero system credentials. An agent can only output a structured proposal (e.g., `propose_credit(account_id: "acct_881", amount: 150)`).
- **External Handlers:** Once an authorized human approves the proposal, a pure, deterministic Rust handler (`WorkflowActionHandler`) executes the API call securely outside the LLM context.

### 3. 3-Stage Inbound Spam & Injection Shield
Before an inbound email or attachment ever reaches an AI agent, it passes through three consecutive security checkpoints:
1. **Stage 1 (Local Heuristic Engine):** High-speed Rust-native scanner detects spoofed sender headers, malformed MIME parts, and suspicious URLs.
2. **Stage 2 (External Daemon Scoring):** Deep integration with Rspamd and SpamAssassin daemons scores sender reputation and content anomalies.
3. **Stage 3 (Pre-Flight LLM Guardrail):** A dedicated, lightweight classifier intercepts prompt injection attacks, adversarial jailbreak attempts, and toxic inputs before the main agent processes the text.

### 4. Anti-Loop Defenses & Runaway Prevention
To permanently prevent AI-to-AI infinite ping-pong storms (e.g., your agent and a vendor’s auto-responder emailing each other endlessly):
- **RFC 3834 & Exchange Suppression:** Outbound messages automatically inject standard headers (`Auto-Submitted: auto-replied`, `X-Auto-Response-Suppress: All`).
- **Strict Turn Limits:** Hard ceiling of 20 messages per hour on any single thread.
- **Cycle & Hop Detection:** Maximum 5-hop inter-workflow limit with automated loop termination and alert notification.

### 5. Seamless Continuity & Offboarding (`MemberWorkAtStake`)
When an employee leaves the company or shifts departments, what happens to the accounts and threads they managed?
- **Atomic Work Handover:** BusyBots' `MemberWorkAtStake` protocol audits all active cron schedules, delegated approvals, and open customer handoffs.
- **Zero Dropped Threads:** The platform enforces atomic reassignment to a new team member before an account can be deactivated, ensuring institutional continuity.

---

## 🔐 Data Privacy, BYOK, & Sandboxing

- **Bring Your Own Keys (BYOK):** Connect directly to your enterprise accounts with OpenAI, Anthropic, Gemini, Groq, or local air-gapped models via Ollama. 
- **Versioned AES-256-GCM Encryption:** All API keys and secrets are encrypted at rest with versioned keys and seamless multi-machine rotation.
- **Hardware-Isolated MicroVM Sandboxes:** When agents need to execute data transformations or parse untrusted spreadsheets, they run inside ephemeral, millisecond-spawned Linux microVMs, fully air-gapped from your production database.
- **Role-Based Access Control (RBAC):** Strict segregation across **Owner**, **Admin**, and **Member** tiers. Sensitive billing, key rotation, and member deletion remain exclusive to Owners.

---

## 📬 Enterprise Security Inquiries & Early Access

Leave busy work to BusyBots and focus on what matters most.

**[Email us to get early access](mailto:earlyaccess@busybots.ai?subject=Enterprise%20Security%20%26%20Early%20Access%20Request)**  
Contact our security and architecture team at `earlyaccess@busybots.ai` to request our full architecture whitepaper or schedule an enterprise security review.
