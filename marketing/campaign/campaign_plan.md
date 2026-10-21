# BusyBots.net Launch Campaign: Pre-release and Post-release

*Channel focus: X. Supporting channels: LinkedIn (repost long-form), email list, the busybots.net site. Dates are relative to launch day L; fill in once the release date is fixed. Tweet numbers refer to `tweets.md`.*

## Goals

| Phase | Goal | Metric |
|---|---|---|
| Pre-release (L-28 to L-1) | Build a waitlist and an argument | 150 early-access requests; 500 followers; one reply thread with 20+ real workflow answers |
| Launch week (L to L+7) | Convert the waitlist | 30 workspaces provisioned; 10 first workflows configured |
| Post-release (L+8 to L+60) | Prove it with scenarios and numbers | 3 cohort case studies with measured before/after; 100 active approving seats |

## Audience and message

- **Primary.** Ops, finance and support leads at 20-500 person companies that run on email with customers and vendors.
- **Secondary.** CTOs and CISOs in regulated or EU companies who need BYOK and self-hosting.
- **Core message.** Leave busy work to BusyBots. Agents think. People lead.
- **Differentiator vs Grok Bot and Dots.** Shared team threads over email, deterministic workflows, zero-credential agents, any model. See `competitor_comparison.md`.

## Prerequisites before any post goes out

1. Decide the canonical domain and inbox. Current copy mixes `busybots.net` and `earlyaccess@busybots.ai`. Pick one and update all marketing files.
2. Landing page live with the hero, one scenario, the comparison table and the early-access form or mailto.
3. Six visual assets: architecture diagram (tweet 11), approval-card mock, shared-thread screenshot with [[quiet]] note, Kanban timeline, comparison table image, logo card.
4. A short demo video (under 90 seconds) of scenario 1 for launch day.
5. Replace "military-grade" in `hero.md` with "defense in depth."

---

## Pre-release: L-28 to L-1

**Cadence.** One tweet per weekday, two on Tuesday and Thursday. One thread per week. One reply-prompt per week.

### Week 1 (L-28 to L-22): the problem
- Posts: 1, 3, 6, 8, 18.
- Thread: "Three bad options for AI in operations" (chatbot silos, black-box loops, rigid automation) ending with a teaser. Build from `../comparisons.md`.
- Reply prompt: tweet 14 ("which workflow would you hand off first"). Save every answer; they become launch-week content (tweet 25) and cohort recruiting leads.
- LinkedIn: long-form version of the thread.

### Week 2 (L-21 to L-15): the approach
- Posts: 2, 4, 9, 10, 16.
- Thread: "Agents think. People lead." Walk through the five-stage pipeline with the architecture diagram (tweet 11 as the opener).
- Reply prompt: "What's the worst thing an AI tool has sent on your behalf?" Collect horror stories; do not name vendors in replies.

### Week 3 (L-14 to L-8): the plumbing
- Posts: 5, 12, 13, 15, 17.
- Thread: inbox hygiene (12 + 48) and BYOK (13). Technical audience day; cross-post to Hacker News "Show HN" only if the self-hosted core is actually installable.
- Open the early-access form publicly; post tweet 19.

### Week 4 (L-7 to L-1): countdown
- Posts: 20 on L-7, then one scenario teaser per day using shortened versions of 26, 27, 28 without the "Scenario:" prefix.
- Email the waitlist: launch date, what day one includes, and a request to reply with their first workflow.
- Line up 3-5 early-access users willing to quote-tweet on launch day.

### Pre-release rules
- No head-to-head comparisons with Grok Bot or Dots before launch. Hold tweets 36-41.
- No hard performance or cost numbers until measured. Use the illustrative use-case figures only as "scenario" framing.
- Answer every reply within a day. Pre-launch replies are the cohort.

---

## Launch week: L to L+7

### Launch day (L)
- 09:00 local: tweet 21 with the demo video.
- 09:15: launch thread (22) with 8-10 posts: what it is, the pipeline diagram, scenario 1, the approval card, zero-trust handlers, pricing (externals free, self-hosted free), how to get in. Pin it.
- 12:00: tweet 23 (three ways to start).
- 15:00: tweet 24 (why not another chat window).
- Email the waitlist with the same video and a direct provisioning link.
- LinkedIn: founder post on why email, linking the thread.
- Reply to every quote-tweet and question the same day.

### L+1 to L+7
- L+1: tweet 25 with results from the week-1 reply prompt.
- L+2: scenario 26 with screenshot.
- L+3: tweet 42 (externals free).
- L+4: scenario 27.
- L+5: tweet 44 (audit timeline) with Kanban screenshot.
- L+7: short recap thread: workspaces provisioned, first workflows live, what broke and what you fixed. Honesty here buys trust for the comparison phase.

---

## Post-release: L+8 to L+60

**Cadence.** One tweet per weekday. Two series run in parallel.

### Series 1: Scenario of the day (L+8 to L+21)
- Tweets 28-35, one per weekday, each with a thread screenshot or approval-card image.
- Each links to the matching section on the site's use-cases or security page.
- Encourage early-access users to reply with their own version.

### Series 2: Positioning (L+15 to L+30)
- Tweets 36-41, spaced two or three days apart, interleaved with scenarios.
- Publish `competitor_comparison.md` as a page on the site and link it from tweet 36.
- Tone: respectful of Grok Bot and Dots, precise about the differences, honest about where they are stronger. Expect replies from their users; answer with the "different buyers" framing (38, 41).

### Series 3: Proof and product (L+22 to L+60)
- Tweet 43 once at least three cohort companies have a measured before/after. Publish the measured range, not the pricing-page target.
- Tweet 46 (start with one workflow) with a short case study.
- Tweet 47 template for every shipped feature; always cite how many cohort teams asked for it.
- Tweets 45, 48, 49 as filler on quiet days.
- Tweet 50 as the standing CTA every Friday.
- First case study thread around L+30: one company, one workflow, time before and after, a quote from the approver.
- Second and third case studies at L+45 and L+60, ideally one finance, one support, one sales ops.

### Ongoing
- Weekly "what shipped" post, Fridays.
- Monthly long-form on LinkedIn: one topic from the pre-release threads expanded with cohort data.
- Collect every "can it do X" question; the ones you answer "yes" to become tweets, the ones you answer "not yet" become the public roadmap post.

---

## Content inventory

| Asset | Used by | Status |
|---|---|---|
| 50 tweets | all phases | `tweets.md` |
| Competitor brief and table image | series 2, site page | `competitor_comparison.md` |
| Benefits by buyer | sales follow-ups, landing page | `business_benefits.md` |
| 12 scenarios | series 1, demo script, site | `scenarios.md` |
| Architecture diagram | tweet 11, launch thread | to produce from `../how_it_works.md` |
| 90-second demo video | launch day | to produce |
| Approval card, thread, Kanban screenshots | scenarios | to capture from `/ui` |
| Waitlist emails (L-7, L, L+7) | email | to write |

## Risks

- **Overclaiming.** The 60-80% cost figure and the "military-grade" phrasing are the two most likely to draw public pushback. Both are addressed above.
- **Comparison backlash.** Posting 36-41 before the product is live and stable invites "show me" replies you cannot answer. Hold them to L+15.
- **Domain confusion.** `.net` on the site, `.ai` in the inbox. Fix before week 1.
- **Roadmap items presented as live.** Step primitives, SOP engine, Slack, marketplace and microVMs are listed as roadmap in `../features.md`. Tweets 9, 33, 34, 35 lean on SOP checklists and zero-trust handlers. Confirm which are live at launch and move any that are not into "coming soon" wording.
