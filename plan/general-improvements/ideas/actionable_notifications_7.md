# Actionable Collaboration Notifications

## Expected result

People receive notifications only when they need to make a decision or when a meaningful state change affects work they own. Silent context additions remain silent by default. Notifications are deduplicated, preference-aware, permission-safe, and link directly to the relevant task, thread, delegation, or review action.

The first implementation may use in-app and email delivery, while keeping the event and preference model suitable for future chat or webhook channels.

## Plan summary

1. Define a notification event taxonomy based on domain transitions rather than low-level database or worker events.
2. Classify events as actionable, informational, digest-only, or silent, with conservative defaults.
3. Resolve recipients from ownership, explicit assignment, review responsibility, mentions, and company roles.
4. Add durable notification records, deduplication keys, delivery attempts, read state, and per-user preferences.
5. Implement in-app notification views and deep links; add email delivery for high-priority actionable events.
6. Add aggregation and digest behavior for repeated updates such as multiple delegated replies.
7. Enforce authorization both when producing notification content and when following its link.
8. Add monitoring and tests for retries, ownership changes, withdrawn actions, disabled users, and cross-company isolation.

## Initial event candidates

- Task assigned or transferred to you.
- Your task is blocked past its deadline.
- Delegation failed, timed out, or requires a decision.
- A draft requires your approval.
- Your submitted draft was rejected or edited materially.
- A task you own completed or failed permanently.

Quiet notes, ordinary context additions, worker retries, and routine internal-agent messages should not notify by default.

## Scope decisions for the detailed plan

- Choose first-release delivery channels and digest frequency.
- Define escalation rules for unacknowledged critical actions.
- Define mention semantics separately from assignment.
- Decide how ownership transfer withdraws or replaces obsolete notifications.
- Establish company defaults versus individual preferences.

## Dependencies and sequencing

Consumes events from all earlier plan parts. The event taxonomy may be designed early, but delivery should follow stable ownership, delegation, and review transitions.

## Acceptance signals

- Each actionable transition creates at most one active notification per responsible user.
- Silent context never causes notification noise under default settings.
- Obsolete actions are withdrawn or clearly marked resolved.
- Notification content never reveals threads or private context to unauthorized users.
- Delivery retries do not create duplicate emails or in-app records.
