# Frequently Asked Questions (FAQ)

> **Our Vision: Run your business. Not the busywork.**  
> *Redefining teamwork for the AI age. Agents think. People lead.*

---

## 💡 The Core Concept

### 1. What makes BusyBots fundamentally different from other AI agent platforms?
Most AI platforms let non-deterministic LLMs run wild in open-ended prompt loops: the model decides what steps to take, what tools to invoke, and what to email. When the model hallucinates or gets stuck, your business operations derail.

**BusyBots combines collaborative discussions with deterministic workflows.**
- **Collaborative Workplace:** Human teammates and specialized AI agents work together in the same shared discussions.
- **Grounded in SOPs & Shared Knowledge:** Agents follow your company’s proven standard operating procedures and verified knowledge base.
- **Deterministic Engine:** Business logic, state transitions, approval gates, and side effects are executed by a rock-solid, durable workflow engine engineered in Rust.
- **People Lead:** AI handles cognitive drafting and research, while human operators retain final review and one-click dispatch.

---

### 2. Why is email the primary interface rather than a web portal or chat app?
> **"Email is at such a low-level and foundational technology that it's easy to connect with anyone."**

Email is the universal nervous system of global commerce. Every vendor, customer, partner, and employee already has an email address and knows how to use it.
- **Zero Friction:** Nobody needs to download an app, create a new username, or log into a portal.
- **Native Group Dynamics:** Email threading, CCs, and attachments already support multi-party collaboration naturally.
- **Universal Reach:** You can connect your agents with any company in the world on day one.

---

### 3. How does "Forward an email with instructions and attachments" actually work?
It's as simple as delegating to a human colleague:
1. When you receive a vendor invoice, customer question, or RFP, you simply forward the email to your channel or agent alias (e.g., `dealdesk@yourcompany.com`).
2. Add your instructions above the forwarded text:  
   *"@agent extract line items from the attached PDF, verify against Q3 pricing in shared knowledge, and prepare a response draft for Sarah."*
3. BusyBots parses the email, unpacks attachments, runs the relevant SOP checklist, and opens a shared discussion with a **Draft Ready** proposal.
4. Sarah reviews the draft, makes any desired tweaks, and clicks **[Approve & Dispatch]**. The client receives a clean, professional email from your domain.

---

## 🤝 Collaborative Discussions & Workflows

### 4. What does "Agents think. People lead." mean in practice?
It means you never have to worry about a rogue AI sending an embarrassing email or hallucinating a contract concession.
- **Agents Think:** Agents perform the heavy cognitive labor—synthesizing long threads, analyzing complex spreadsheets, cross-referencing company policies, and generating initial drafts.
- **People Lead:** Humans set the strategy, review the drafts, provide internal direction, and grant final approval for external communications and financial actions.

---

### 5. What are Shared Discussions, and how do they differ from traditional ticket queues?
Traditional helpdesks treat customer interactions like assembly-line tickets: a representative is assigned a ticket, selects a canned template, and closes it.

**In BusyBots, interactions are living shared discussions.**
- Human specialists and specialized AI agents (e.g. `support+billing+legal`) join the same thread.
- Teammates share ideas, debate approaches, and use private internal notes (`[[quiet]]`) to brainstorm solutions with agents without external customers ever seeing internal chatter.
- Everyone has full visibility, and the team collaborates to build the best outcome.

---

### 6. What is "Shared Company Knowledge" and how does it prevent mistakes?
AI models make mistakes when they lack context. BusyBots maintains a centralized **Shared Company Knowledge Base** containing:
- Approved product and pricing sheets.
- Terms of service, SLAs, and legal guidelines.
- Historical thread resolutions and operational playbooks.

When an agent handles an inquiry, it retrieves the exact, verified institutional facts rather than relying on generic internet training data.

---

### 7. How do Standard Operating Procedures (SOPs) ensure consistency?
Instead of hoping an LLM remembers a 20-page document, BusyBots codifies SOPs into executable step graphs with **verification checklists** (`read_do` and `do_confirm`):
- The agent is required to verify specific criteria (e.g., *"Is the invoice under $5,000? Is the VAT number verified?"*) before generating a response.
- Checklists must be satisfied before the human approval card can be dispatched.

---

## 🔒 Security, Privacy & Tech

### 8. Can our external customers or vendors see our internal agent discussions?
**Never.** BusyBots strictly isolates internal discussion state from external transport deliveries. Private agent deliberations, internal notes, and `[[quiet]]` tags are stripped before any external email is generated. External contacts only receive the final, approved reply.

---

### 9. Which AI models and email systems can we use?
- **AI Models:** BusyBots supports **Bring Your Own Key (BYOK)**. Connect OpenAI (GPT-4o), Anthropic (Claude 3.5 Sonnet), Google Gemini, Groq, or self-hosted open-source models via Ollama and DeepSeek.
- **Email Providers:** Native support for Google Workspace, Microsoft 365, SendGrid, Postmark, Amazon SES, and standard SMTP/IMAP servers.
- **Team Chat:** Bi-directional integration with Slack is fully supported.

---

### 10. How do I get early access?
We are onboarding teams into our private Early Access cohort.

**[Email us to get early access](mailto:earlyaccess@busybots.ai?subject=Early%20Access%20Request%20-%20FAQ)**  
Send an email to **`earlyaccess@busybots.ai`** with:
1. Your company name and team size.
2. The primary operational workflow you want to automate.
3. Your current email or helpdesk setup.

Our team will review your request and get you set up with a private workspace.
