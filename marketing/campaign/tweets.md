# BusyBots.net: 50 Tweets

All under 280 characters. Grouped by campaign phase. `[ ]` marks a placeholder to fill (image, number, feature, handle) before posting.
Sources for product claims: `../features.md`, `../use_cases.md`, `../security_and_governance.md`, `../pricing.md`.

---

## Phase A: Pre-release (teasers, problem framing, education) — 20

1. Your team already has a universal collaboration tool. It's called email. Every customer, vendor and partner has it. We built BusyBots on it so AI agents can join the thread without anyone installing anything. busybots.net

2. "Agents think. People lead." That's the whole product philosophy in four words. Agents do the drafting, synthesis and cross-checking. Humans keep the decisions, the approvals and the brand voice.

3. The problem with always-on personal AI agents: they're personal. One person, one bot, zero team visibility. Business work happens in shared threads. That's where your agents should live too.

4. An AI agent that holds your Stripe key in its prompt is one prompt injection away from issuing refunds. In BusyBots agents hold zero credentials. They propose. Deterministic handlers execute, after a human approves.

5. Forward an email. Add one line: "@ap-agent check this invoice against PO #1128 and draft a reply." Get a draft back in your thread, ready to approve. That's the entire onboarding.

6. Most "autonomous agent" demos hide the failure mode: the loop that runs 15 times, burns $50 in tokens and emails a discount nobody approved. Business logic should be deterministic. AI should be a bounded step inside it.

7. We're building BusyBots for ops teams drowning in busywork: invoice checks, RFP answers, multi-department customer replies, shipping exceptions. Early access opens soon. busybots.net

8. Chatbots gave every employee a private assistant. Nobody else sees the prompt, the context or the result. The company gets zero smarter. Shared discussions fix that: humans and agents in the same thread, grounded in shared company knowledge.

9. SOPs are the most underrated asset in any company. Most sit in a PDF nobody reads. BusyBots turns them into executable checklists an agent must satisfy before a draft can even reach a human.

10. Not every step needs an LLM. Routing, branching, JSON reshaping, header parsing: compiled Rust, $0 in tokens. Models run only where reasoning or language is actually needed. Predictable bills follow.

11. BusyBots in one picture: inbound email → spam shield → shared discussion → bounded agent reasoning → human approval → deterministic execution. Early access soon. [architecture diagram]

12. Vendor auto-responder meets AI agent. Infinite reply storm. We ship RFC 3834 headers, 20 msg/hour thread limits and 5-hop cycle detection. Boring plumbing is what makes agents safe to put on a real inbox.

13. BYOK from day one. OpenAI, Anthropic, Gemini, Groq, DeepSeek or local Ollama. Your keys, your negotiated rates, AES-256-GCM at rest. Your workflow vendor should not also be your model lock-in.

14. Question for ops leaders: what's the one recurring email workflow you'd hand to an agent tomorrow if you trusted it? Replies shape our early-access cohort.

15. Self-hosted core, free during rollout. Rust engine, PostgreSQL, any SMTP provider. If you want to see how it works before you trust it with customers, you can. busybots.net

16. Multi-specialist pipelines in one address: support+billing+legal@yourcompany.com. Each agent applies its lens, the thread aggregates, the customer gets one coherent reply. Not three emails over three days.

17. [[quiet]] notes: drop call transcripts, CRM context or internal debate into a thread without triggering a reply. Agents get the context. The customer never sees the chatter.

18. The question isn't "can an AI write this email?" It can. The question is "who approved it, which checklist did it pass, and what happens when step 4 fails at 3am?" We built the product around the second question.

19. Early access cohort forming. We're looking for teams with one painful recurring email workflow and a willingness to tell us when we're wrong. earlyaccess@busybots.ai

20. 7 days to early access. The first cohort gets hands-on onboarding, their first workflow configured with us, and a direct line to the people writing the code.

---

## Phase B: Launch week — 5

21. BusyBots is live for early access. Leave busy work to BusyBots: shared discussions where your team and AI agents work together, in email, with your SOPs enforced and a human on every approval. busybots.net

22. Launch thread 🧵 What BusyBots does, who it's for, what it refuses to do (send email without approval), and how to get in. 1/[n]

23. Three ways to start today: 1) forward an email to your agent alias, 2) open the shared workspace in your browser, 3) run the self-hosted core on your own box. No SDK. No new app for your customers.

24. Why we didn't build another chat window: your customers won't log into it, your vendors won't adopt it, and your team already lives in email. Meet people where the work already is.

25. To everyone who answered "which workflow would you hand off first": invoice matching won by a mile. It's the first turnkey recipe in early access.

---

## Phase C: Post-release, scenarios — 10

26. Scenario: vendor invoice arrives, 21% tax instead of the contracted 19%. AP agent parses the PDF, checks the vendor master, flags the $420 gap, drafts the correction. Finance manager clicks approve. 20 minutes → 10 seconds.

27. Scenario: enterprise customer wants 200 more seats, their volume discount, and a GDPR confirmation. One email to support+billing+legal@. One unified draft. One AE approval. Minutes, not days.

28. Scenario: 15-page RFP lands Friday afternoon. Deal desk agent answers 85% from shared company knowledge, flags 15% as "Needs Rep Clarification", prices by the margin SOP. Rep fills the gaps Monday and ships it.

29. Scenario: carrier reports shipment #88192 in a customs hold. Agent finds affected orders, checks backup inventory, drafts two options. Ops lead asks in-thread: "Dallas has 4 units. Re-ship?" "Yes, VIP." Approved. Warehouse API fires.

30. Scenario: 7:30 every morning a scheduled agent audits open handoffs, SLA deadlines and delivery queues, then posts: "3 items need you today." Leadership starts the day knowing exactly where humans are needed.

31. Scenario: a customer replies to a thread your agent handled last week. BusyBots does NOT auto-reply. It parks the thread as "Needs Instruction." A teammate claims it, instructs the agent, reviews the draft, dispatches.

32. Scenario: a departing employee owned 12 schedules and 4 open customer handoffs. Offboarding won't complete until every one is reassigned. Zero dropped threads, enforced by the platform, not by memory.

33. Scenario: agent proposes refund_customer(amount: 50). That's all it can do: propose. A manager approves. A deterministic handler, outside the LLM context, calls Stripe. The model never saw the key.

34. Scenario: external API blips at step 4 of 6. Only step 4 retries, behind an idempotency fence. Steps 1-3 don't re-run. No duplicate emails, no re-burned tokens. Every step is a leased PostgreSQL task.

35. Scenario: new hire on support. Instead of a 20-page manual, the SOP checklist runs in-thread: "Invoice under $5,000? VAT verified?" The agent can't draft until both are confirmed. Same quality at 9am Monday and 3am Sunday.

---

## Phase D: Post-release, positioning vs Grok Bot and OpenAI Dots — 6

36. Grok Bot and OpenAI Dots gave every individual a persistent agent with its own cloud computer. Impressive. But a business isn't a collection of individuals with private bots. It's shared threads, shared knowledge and shared accountability.

37. "Always-on agent that acts on your behalf" vs "agent that drafts, human that approves." Both are valid. Only one is something your compliance team will sign off for customer-facing email.

38. A personal agent that learns your preferences is great for you. An agent grounded in the company's approved pricing sheet, DPA clauses and vendor master is great for the company. Different products, different buyers.

39. Dots connect to 4,000 apps. Grok Bots run on their own computers. BusyBots agents connect to one thing: your email domain. Every customer and vendor on earth already supports it.

40. Model lock-in is the quiet cost of platform agents. Grok Bots run Grok. Dots run GPT-6. BusyBots runs whichever model you bring, including local Ollama, and you can pick per agent.

41. We're not competing with Grok Bot or Dots for your personal to-do list. We're competing with the 40 minutes your finance team spends on each vendor invoice. Different fight.

---

## Phase E: Post-release, benefits, proof, CTAs — 9

42. What you pay for: seats for internal teammates who approve drafts. What you don't pay for: every customer, vendor and partner who emails in. External participants are free. Always.

43. The economics in one line: deterministic steps cost $0 in tokens, so model spend goes only to reasoning and drafting. [Cohort figure once measured, e.g. "X% lower operational AI spend"] [chart]

44. Every step, payload, evidence artifact and state change lands in a structured timeline on the Kanban board. When someone asks "why did we send that?", you have an answer in 30 seconds, not a log dive.

45. Three roles: Owner, Admin, Member. Billing keys, company deletion and member removal stay with Owner. Boring, predictable governance is a feature when agents are in the loop.

46. First cohort lesson: the teams who got the most value started with ONE workflow, ran it draft-only for a week, then widened. Start small. Trust is earned per workflow.

47. Shipping this week: [feature]. Requested by [n] early-access teams. Keep telling us what hurts.

48. Spam before agents: a two-stage shield (Rust heuristics, then Rspamd/SpamAssassin) runs before any email reaches a model. A spoofed vendor email shouldn't be your first prompt-injection incident.

49. If your team spends more time forwarding, chasing and copy-pasting than deciding, that's busywork. Hand it to BusyBots. Keep the deciding. busybots.net

50. Early access is open. Email earlyaccess@busybots.ai with your company, team size and the one workflow you'd automate first. We reply within 24 hours and configure it with you.

---

## Posting notes

- Tweets 1-20 pair naturally into threads: 3+8 (silos), 4+33 (zero-trust), 9+35 (SOPs), 12+48 (inbox hygiene), 10+43 (cost).
- Tweets 26-35 work as a daily "Scenario" series with one image each (thread screenshot or approval card mock).
- Tweets 36-41 should go out only after launch; pre-launch comparisons invite replies you can't yet back with a live product.
- Replace `busybots.net` and `earlyaccess@busybots.ai` with whatever domain the live site and inbox actually use; the current marketing copy mixes `.net` and `.ai`.
- Avoid the phrase "military-grade" from `hero.md`; it attracts ridicule on X. "Defense in depth" reads better.
