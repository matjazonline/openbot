# Operational Work Queue and Business Metrics

## Expected result

Every unresolved human action appears once in a shared operational queue with a responsible person
or team queue, next action, age, priority, and due/expiry time. Opening or reading an item never
resolves it. Managers can see workload and aging without depending on technical task states or
notification unread counts.

## Attention read model

- Add a derived `AttentionItem` application projection with source kind/id, company, channel,
  optional thread/task/correlation, state, responsible principal or channel team queue, priority,
  due/expiry time, version, created/updated time, and authorized deep link.
- Derive items from authoritative tasks, manual handoffs, pending response reviews, delegation
  timeout/failure decisions, and permanent delivery failures. Do not create a second generic
  lifecycle state machine.
- Create task attention only when a human owns the active task, the task is unassigned, or a
  failure/decision requires human action. Ordinary agent-owned queued or processing work remains
  visible in task status but does not clutter the human work queue.
- Add `BusinessPriority::{Normal, High, Urgent}` and optional `business_due_at` to tasks and
  standalone handoffs. New work defaults to `Normal` with no business due time, preserving current
  behavior. Reviews inherit task/handoff values; delegation decisions use the earlier of the
  inherited due time and outreach expiry.
- Tasks use task ownership for responsibility. Reviews use their explicit reviewer. Handoffs may
  name a responsible principal or default to the channel team queue. Delegation and delivery
  decisions go to the current human owner, or the channel team queue if the task is agent-owned or
  unassigned.
- Priority/due/assignment changes are version-fenced, idempotent, authorized, and audited. They do
  not alter worker scheduling, ownership, or external delivery unless a separate explicit command
  does so.

## API and UI

- Expose scoped `My work`, `Unassigned`, and manager `Team work` queries with deterministic cursor
  pagination. Sort overdue first, then due within 24 hours, then urgent/high/normal, then oldest.
- Render the same item once even when several technical events describe it. Show source business
  status and allowed next action, not leases, retries, or provider payloads.
- Claiming an unassigned task uses task ownership. Claiming a handoff updates its responsible
  principal. Reviews and delegation decisions must be reassigned through their own source command.
- Resolving the source removes the item on reconciliation. Viewing, marking a notification read, or
  navigating away never resolves it.
- SSE carries identifiers only and is a wake-up. Initial load, reconnect, lag, and authorization
  changes re-query the current projection.

## Business measurement

- Build company-authorized operational summaries from durable source/audit events:
  unassigned count, oldest actionable age, time-to-claim, time waiting on delegation, review
  turnaround, timeout/cancel/reassign rate, time-to-logical-external-response, and permanent
  delivery-failure count.
- Keep business reports separate from process-local technical dashboards. Do not put company,
  principal, channel, message, or target identifiers into metrics labels.
- Instrument query duration and working-set size. Add indexes only after representative production
  statistics and recorded `EXPLAIN (ANALYZE, BUFFERS)` evidence.
- V1 deadlines are elapsed UTC instants, not business-hours SLAs. Do not promise business calendars,
  holidays, or local-time digests until company/user timezone policy is added.

## Test and acceptance plan

- Every active source produces exactly one attention item in `My work`, `Unassigned`, or the
  authorized team view; resolved/withdrawn source states produce none.
- Test ownership/reviewer/handoff reassignment, due/priority changes, source resolution, disabled
  principals, restricted channels, cross-company IDs, cursor stability, and SSE reconciliation.
- Verify notification read/dismiss actions cannot mutate attention or source state.
- Validate business durations from immutable events across retries, transfers, waiting periods,
  and provider-unknown outcomes.
- Cap working sets, display truncation where necessary, and record representative query plans before
  accepting mailbox-list performance.
