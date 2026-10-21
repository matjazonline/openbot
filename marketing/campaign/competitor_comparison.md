# BusyBots vs Grok Bot vs OpenAI Dots

*Positioning brief for marketing and sales. Competitor facts as of 2026-10-21 come from public coverage and changelogs; verify against xAI and OpenAI announcements before quoting a number in public.*

## What they are

**xAI Grok Bot** (beta since 2026-08-11). Persistent personal agents. Each Bot runs on its own computer with access to approved apps, has memory, routines, and can coordinate with other Bots. Included with SuperGrok, Cursor Pro and Cursor Teams since 2026-08-26, usage billed separately. Enterprise controls (access management, network and audit controls) since 2026-09-03. Runs Grok 4.6, a model tuned for long-running agent work.

**OpenAI Dots** (announced DevDay, 2026-09-30). Always-on agents inside ChatGPT, powered by GPT-6 Astra. Each dot has its own cloud computer and browser, connects to 4,000+ apps via plugins, learns user preferences, and runs "proactive research" in the background (read-only on connected apps). Reachable via ChatGPT, Slack and Teams. First dot included for Pro and Business Premium; Enterprise is admin-enabled beta. Not available to Pro users in the EEA, Switzerland or the UK. "Specialist dots" with company identities and credentials are an enterprise-pilot preview. Users can set Custom Rules (allow, block, require approval), and OpenAI itself says to "always review consequential work."

**BusyBots** (early access). Company workspace where human teammates and specialized AI agents share discussion threads, reachable by email or browser. Deterministic Rust workflow engine owns business logic, approvals and side effects; agents are bounded reasoning steps. BYOK, self-hostable, zero-credential agents, SOP checklists, human approval gate on external communication.

## Side-by-side

| Dimension | Grok Bot | OpenAI Dots | BusyBots |
|---|---|---|---|
| Unit of work | One person's persistent bot | One person's always-on dot | A shared company thread with people and agents |
| Where work happens | Bot's own computer, Grok/Cursor UI | Dot's cloud computer, ChatGPT/Slack/Teams | Email threads and a shared browser workspace |
| Who can participate without onboarding | Nobody; need Grok/Cursor access | Nobody; need ChatGPT access | Any customer, vendor or partner with an email address |
| Model | Grok only | GPT-6 only | Any: OpenAI, Anthropic, Gemini, Groq, DeepSeek, local Ollama; per-agent choice |
| Who decides the next step | The model | The model, within Custom Rules | A deterministic DAG; the model fills bounded steps |
| Credentials | Bot has app access | Dot signs into sites, holds company credentials for specialist dots | Agents hold zero credentials; handlers execute after approval |
| External actions (email, refunds) | Bot acts; review varies | Dot acts; auto-review for account-affecting actions; user rules can require approval | Always parked until a human clicks Approve & Dispatch |
| Company knowledge | Per-user memory | Per-user preferences; specialist dots in pilot | Shared, company-wide knowledge base and thread history |
| SOP enforcement | None stated | None stated | Executable checklists (read_do, do_confirm) gate drafts |
| Failure recovery | Not documented | Not documented | Per-step PostgreSQL leases, idempotency, retry only the failed step |
| Auditability | Enterprise audit controls | Monitoring system can pause a dot | Structured timeline per step, deliveries log, Kanban lineage |
| Inbox hygiene | n/a | n/a | Two-stage spam shield, anti-loop headers, turn and hop limits |
| Hosting | xAI cloud | OpenAI cloud | Self-hosted core (free), hosted Team, private VPC or on-prem Enterprise |
| EU availability | Yes | Not for Pro in EEA/CH/UK | Yes, including on-prem |
| Pricing model | Plan plus separate bot usage | Plan tier; first dot included | Free self-hosted; $49 per internal seat; externals free |
| Offboarding | Not documented | Not documented | Atomic reassignment of schedules and handoffs before removal |

## The differences that matter in a pitch

1. **Team vs individual.** Grok Bot and Dots are personal agents that may one day coordinate. BusyBots starts from the shared thread: the agent is a teammate in a channel, not an assistant in a tab. Everyone sees the same context, drafts and approvals.

2. **Email as the universal ingress.** Both competitors require the other party to be inside their platform. BusyBots' customers and vendors never know an agent is involved; they just get a well-written reply from your domain.

3. **Deterministic control plane.** Competitors let the model choose the next action and bolt rules on top. BusyBots defines the sequence, gates and integrations in code or YAML and calls the model only for reasoning and drafting. This is the line between "an assistant with guardrails" and "a workflow engine with an assistant inside."

4. **Zero-trust by construction, not by policy.** A dot can sign into sites; a Grok Bot has app access. A BusyBots agent can only emit a structured proposal. Prompt injection can at worst produce a bad proposal that a human sees and rejects.

5. **Model and vendor independence.** Each competitor is a distribution channel for its own model. BusyBots is model-agnostic and self-hostable, which matters for regulated buyers, EU data residency, and anyone with existing enterprise LLM contracts.

6. **Operations-grade reliability.** Durable step leases, idempotency fences and retry-the-failed-step-only are standard in workflow engines and absent from the agent products' public materials.

## Where the competitors are stronger (be honest in replies)

- **Breadth of integrations.** Dots' 4,000-app plugin ecosystem and Grok Bot's app access cover far more SaaS surface than BusyBots' email, Slack (roadmap) and HTTP action handlers.
- **Consumer polish and distribution.** Both ship inside products with hundreds of millions of users. BusyBots is early access.
- **Proactivity.** Dots' background research and Grok Bots' routines initiate work. BusyBots is reactive plus cron schedules; it does not browse on its own.
- **Personal productivity.** For one person's research, drafts and scheduling, a personal agent is the right tool. BusyBots is not that product.

## Recommended framing

Do not position BusyBots as "a better Grok Bot." Position it as the layer a company puts between its inbox and its agents when the output is customer-facing, money-moving, or audited. Suggested one-liner:

> Grok Bot and Dots give a person an agent. BusyBots gives a company a team of agents that work inside shared email threads, follow your SOPs, and never send anything a human hasn't approved.

Sources: [Business Today on Dots](https://www.businesstoday.in/technology/artificial-intelligence/story/openai-introduces-dots-always-on-ai-agents-that-work-247-and-act-on-your-behalf-558688-2026-09-30), [TNW on Dots](https://thenextweb.com/news/openai-dots-always-on-ai-agents-cloud-computers-devday), [Grok Bot changelog](https://www.gradually.ai/en/changelogs/grok-bot/), [Grok 4.6 for business](https://www.layer3labs.io/guides/grok-4-6-for-business), [xAI news](https://x.ai/news).
