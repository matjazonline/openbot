# Phase 2 — Reassign an internal target, on one copy of the composition

Read [`general_plan_instructions.md`](general_plan_instructions.md) first, then
[`phase1.md`](phase1.md), which this extends.

**Goal.** The tool gains `reassign_internal_target`, the third and last operation `OwningAgent` is
permitted. The prerequisite is that the replacement message and delivery are composed in **one**
place, shared by the manager route and the tool, rather than copied.

## Why this is its own phase

`ReassignInternalTarget` is the only delegation operation whose command carries a payload the caller
must build: `DelegationCommandRequest.replacement: Option<OutreachTargetRequest>`
(`src/application/task_queue.rs:205-208`). `insert_replacement`
(`src/adapters/persistence/task/controls.rs:422-477`) then refuses a replacement that does not match
the requested channel, task, company, message id, or `DeliveryPurpose::Outreach` — so the payload is
not advisory, it is checked, and building it wrong is a `BadRequest` rather than a silent mismatch.

Today exactly one caller builds it: `ThreadUseCases::compose_reassigned_target`
(`src/application/use_cases/thread/mod.rs:1591-1683`), ~90 lines that read the reassignment context,
load the source and target channels, build the platform addresses, compose the delivery, and write
the `MessageWrite` with its two participants and `ThreadEntryKind::Delegation`.

A second copy inside the tool would be a second definition of what a delegation message *is* — the
kind of duplication `src/AGENTS.md` names ("Extract to a named helper the first time you'd write the
second copy"), and worse than most, because `insert_replacement`'s checks would pass while the
*content* drifted: a reassignment made by an agent could carry a different sender, a different entry
kind, or no relay trace, and nothing would fail.

So: extract first, then call it twice.

**Files touched**

| File | Change |
|---|---|
| `src/application/services/outreach_reassignment.rs` | **new**: the shared composition |
| `src/application/use_cases/thread/mod.rs` | `compose_reassigned_target` becomes a call into it |
| `src/application/services/agent_delegation_tool.rs` | the third operation |
| `src/application/services/agent_runner/mod.rs` | the tool gains the two handles composition needs |
| `src/adapters/persistence/task/tests.rs` | reassignment under `OwningAgent` |

---

## 2.1 Extract the composition

New module, application layer (it uses ports only — no `axum`, no `sqlx`, per the dependency rule):

```rust
/// Everything composing a replacement internal target needs, named so the two channel ids and the
/// two slugs cannot be swapped at a call site.
pub struct ReassignmentRequest<'a> {
    pub company: &'a Company,
    pub task: &'a BackgroundTask,
    pub outreach_id: Uuid,
    pub target_id: Uuid,
    pub new_channel_id: Uuid,
    /// Also the source key's discriminator, so one command composes one message.
    pub command_id: Uuid,
    pub app_domain_name: &'a str,
}

pub async fn compose_replacement_target(
    task_persistence: &dyn TaskPersistence,
    channel_persistence: &dyn ChannelPersistence,
    deliveries: &DeliveryComposer,
    request: ReassignmentRequest<'_>,
) -> AppResult<OutreachTargetRequest>;
```

The body is `compose_reassigned_target`'s, moved verbatim. Two things must not change while moving
it, because `insert_replacement` checks both:

- `source_key: format!("task:{}:reassign:{command_id}", task.id)` — the delivery's idempotency key.
  With the tool deriving `command_id` from the invocation or the call id (phase 1 §1.3), a retried
  tool call composes the *same* source key, which is what makes the retry attach instead of
  composing a second message.
- `message_id` is generated once and used for both the delivery and the `MessageWrite`
  (`insert_replacement` asserts `delivery.message_id == request.id`).

`ThreadUseCases::compose_reassigned_target` then deletes down to one call, passing
`self.config.app_domain_name.as_str()`. `ThreadUseCases::execute_delegation_command`
(L1559-L1589) keeps its shape and its `match` on the operation — only the helper's body moves. Its
existing behaviour is covered by the manager-route path, so **the suite must stay green with no
assertion changes in this step**; land it as its own commit if that helps prove it.

Splitting a function that was already at 90 lines is also a chance to obey `src/AGENTS.md` rather
than move a violation: the body has three phases marked by what it loads — the context and the two
channels, the composed delivery, the message write. Make them three synchronous-where-possible
helpers in the new module (`resolve_channels`, `compose_delivery`, `delegation_message`), and keep
the public function down to their three calls. Two of the three need no `async` at all, which also
keeps the agent-runner call chain shallow.

## 2.2 The tool's third operation

Input gains `DelegationToolOperation::ReassignInternalTarget` and reuses `target_channel` for the
target being moved, plus:

```rust
/// The channel to move the request to, as `channel` or `company/channel`.
new_channel: Option<String>,
```

Both selectors resolve through `outreach_tool::resolve_channel_target`, so both are subject to the
running agent's `sub_agent_scope` — an agent restricted from a sibling cannot reach it by
reassignment any more than by outreach. That is the whole reason to reuse the resolver rather than
parse a channel id: **a tool taking a raw `new_channel_id` would bypass the sub-agent scope**, which
is an authorization hole this phase must not open. Say so in a comment at the call site.

Validate before composing:

- `target_channel` must name an active internal target on the open outreach (phase 1's lookup).
- `new_channel` must resolve, must not be the same channel (`apply_operation` refuses that at
  `controls.rs:657`, but failing here gives the model a usable message instead of a `Conflict`).
- `apply_operation` also requires the resolved channel to be **enabled**
  (`compose_reassigned_target`'s `filter`), which surfaces as a `BadRequest`; let it.

Then:

```rust
let replacement = compose_replacement_target(…, ReassignmentRequest { … }).await?;
self.persistence
    .execute_delegation_command(DelegationCommandRequest { command, replacement: Some(replacement) })
    .await
```

The tool needs `channel_persistence: Arc<dyn ChannelPersistence>` and `deliveries: DeliveryComposer`
for this — exactly what `OutreachAndAwaitQuorumTool::new` already takes
(`src/application/services/outreach_tool.rs:114-127`), and exactly what the agent runner already has
in scope at the construction site (`agent_runner/mod.rs:448-497`). Add them to the tool's
constructor; do not reach for `ThreadUseCases`.

Error mapping is phase 1's, unchanged. Note that composition happens **before** the command, so a
composition failure means nothing was written — the delivery is composed, not enqueued, until
`insert_replacement` runs inside the command's transaction.

---

## Tests

**Pure:**

- `reassign_internal_target` without `new_channel`, and with `new_channel` equal to
  `target_channel`, both fail with their own messages.
- `reassign_internal_target` carrying `timeout_hours` fails.
- A `new_channel` outside the running agent's `sub_agent_scope` fails. Model this on
  `a_restricted_agent_cannot_outreach_to_an_excluded_sibling_by_selector`
  (`outreach_tool.rs` tests) — it is the same resolver and the same class of hole.

**Against the database** (`src/adapters/persistence/task/tests.rs`):

- An owning agent reassigns its internal target: the old target row is `superseded`/`cancelled` as
  `apply_operation` leaves it, a new target row exists with `replaces_target_id` pointing at the old
  one and `internal_channel_id` the new channel, and the outreach version is bumped once. Assert on
  the ids the test created only.
- The replacement carries the same `source_key` shape the manager route produces, so a retried tool
  call with the same `command_id` attaches rather than composing twice. Submitting the identical
  command twice must leave exactly one replacement row for that outreach.
- Reassigning an **external** target is refused (`controls.rs:647-651`, `BadRequest`), and the
  refusal reaches the model as a failure rather than as an error.

**Regression, and the point of §2.1:**

- The manager route's reassignment tests pass unchanged. Add one assertion that the two paths agree:
  compose a replacement through the extracted helper with a fixed `command_id` and assert the
  resulting `MessageWrite`'s entry kind, participants and sender match what the manager route
  produces for the same inputs. That is the drift this phase exists to prevent.

## Done when

- [ ] `compose_replacement_target` has exactly one definition and two callers.
- [ ] `ThreadUseCases::compose_reassigned_target` is a call, not a copy, and no existing assertion
      changed when it became one.
- [ ] The tool reassigns through the resolver, so `sub_agent_scope` binds both selectors.
- [ ] A retried reassignment composes one message, not two.
- [ ] `cargo test` and `cargo clippy --all-targets -- -D warnings` are green.
