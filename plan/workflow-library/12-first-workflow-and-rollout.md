# Step 12 — First Workflow, Verification, and Rollout

## Outcome and dependencies

Release one complete **Prepare customer quotes** workflow after
[steps 1–11](README.md), with measured setup effort and case outcomes. Maintain the synthetic package
and examples from step 2 throughout implementation; this step completes production verification.

## First package contract

Use one quote agent, one salesperson/reviewer role, and a human coordinator who may also be the
reviewer. Connect one HubSpot account for customer/product reads and an existing authorized email
binding for delivery. The output is a reviewed quote artifact and its recorded logical delivery;
this release does not create HubSpot quote objects or invoices.

| SOP step key | Owner | Accepted output |
| --- | --- | --- |
| `confirm_request` | Human salesperson | Exact customer reference, selected products, quantities, and approved request details |
| `prepare_quote` | Quote agent | Structured quote, source evidence, and immutable internal draft artifact |
| `review_quote` | Human reviewer | Review/approval reference bound to exact artifact, terms, and recipient/action payload |
| `deliver_quote` | Quote agent with scoped delivery capability | Reference to the same approved artifact and authoritative successful delivery outcome |

Keep this a V1 linear procedure. Inbound start inputs contain the source inquiry and optional
suggested selections; the first human result resolves missing/ambiguous customer/product details.
An owner can transfer work or request explicit rework through the SOP foundation. Do not invent a
branching expression or an unimplemented autonomous clarification loop to make the example work.

## Implementation

1. Configure currency, allowed product set, company-supplied quote terms, validity duration, reply
   channel, human reviewer/coordinator, and provider-record freshness policy in the setup schema.
   Do not infer recipient authorization, tax rules, discounts, credit terms, or product identity
   from an LLM answer. Explicitly unsupported catalogue/currency combinations block readiness.

2. Validate each case using typed quantities and decimal monetary values. Implement quote
   calculation/result validation as a small domain/application capability used by the quote
   workflow, not executable package code. Recompute line amounts and totals from the approved
   quantities and stored provider price evidence. Reject overflow, invalid precision, inconsistent
   currencies, unknown products, and an invented price. Human corrections produce a new draft
   artifact with recorded rationale and must be reviewed again.

3. Persist bounded customer/product evidence through the existing internal artifact/message
   boundary with company/run visibility, selected fields, source references, and timestamps. Do
   not rely only on a mutable CRM link as proof of the price used. Revalidate the configured
   freshness/validity requirements before delivery; an expired quote or changed required source
   data creates rework/review, not silent repricing of an approved artifact.

4. Wire the review step to the existing review/approval authority for the exact outgoing quote.
   The delivery step reuses that authorization only if payload, recipients, and versions match.
   It cannot pass arbitrary content to an unrestricted send tool. A queued message remains a
   waiting delivery step until the configured authoritative delivery success condition is met.
   Report provider acknowledgement distinctly from customer receipt/read/acceptance.

5. Ship manual start first. Add an explicitly routed quote-inquiry channel after the inbound gate
   passes. The generic schedule implementation from step 9 receives its own fixture/test workflow;
   do not enable a recurring quote sender merely to demonstrate schedules.

6. Publish operator documentation for creating/importing a release, compatibility failures,
   retirement, and source attribution. Publish company guidance for account linking, data scopes,
   human review, sample limitations, pause/archive semantics, and reviewed updates. Provider
   endpoint/scope references belong in the connector documentation from step 5. Production
   configuration examples must name only settings the implementation actually reads/validates.

## Release verification

Run the shared checks in the [README](README.md) and the narrow tests from every step. Add an
end-to-end suite using real PostgreSQL and deterministic provider/model fixtures covering:

- Publish → setup → reused/new resources → install → sample → activate → manual SOP run → exact
  human-approved quote → one logical delivery → completed case.
- An inbound inquiry creates one run, with no competing generic response; a scheduled fixture
  creates one run per accepted durable slot.
- A human correction/rejection creates rework; no draft or unapproved recipient escapes through
  ordinary task completion, automatic replies, outreach, simulation, or delivery retry.
- Lost install/activation responses, duplicate ingress, worker restart, stale result submissions,
  competing materializers, cancellation, and provider-unknown delivery outcomes.
- Connection revocation, expired credentials, changed prices, missing products, departed people,
  inaccessible model configuration, and cross-company/restricted-channel reads and mutations.
- New template publication plus conflicting company edits, tested adoption, rollback proposal,
  pause/resume, and archive while shared resources remain available to other users.

Contract fixtures should assert business outcomes and isolation, not exact LLM prose. Use a
separate, explicitly reviewed live-account pilot for OAuth, provider scopes/features, supported
model behavior, real delivery, and source freshness. Keep secrets and customer records out of
fixtures, logs, screenshots, and committed evidence.

## Observability and measures

Add bounded metrics for publish/install/activation failures, setup validation classes, sample
outcomes, start suppression/conflicts, connection authorization/rate failures, and revision drift.
Keep company IDs, template slugs, account IDs, and case data out of process metric labels. Carry
correlation IDs across setup commands, admission tasks, SOP runs, connector calls, and deliveries.

Build company-authorized reports from installation events, SOP events, and delivery state: setup
completion rate, time to first successful case, confirmation-to-delivery time, first-pass review
acceptance, rework, and failure rate. Report human touch time only if measured; waiting time is
not labor saved. Exclude simulation runs from production outcome reports and compare pilot cases
with a recorded manual baseline before claiming savings.

## Rollout gates and acceptance

1. Enable operator catalogue authoring and internal company setup only; keep production starts off.
2. Verify atomic install, sample isolation, current-permission checks, and update comparison using
   production-shaped database/concurrency tests. Keep offline compilation and stack-budget CI green.
3. Enable manual production starts for an internal pilot with human review and a tested email
   binding. Confirm recovery and the end-to-end audit history before onboarding other companies.
4. Enable inbound automatic starts after duplicate-response, pause, and source-replay tests pass;
   enable schedule targets after their attribution/materialization tests pass.
5. Exercise connector outage/reauthorization, a conflicting template update, and rollback with
   active runs. Verify the pause control stops new admission without claiming to undo prior effects.
6. Expand to pilot companies only after measured setup and review burden are acceptable to those
   users. Choose the second workflow from observed demand and reuse the same package/install/SOP
   contracts rather than implementing another engine.

The release is complete when a company can select this package, connect tools, assign people,
install, test, activate, complete a reviewed customer case, and adopt a later version while
retaining its customizations and the history/configuration of existing runs.
