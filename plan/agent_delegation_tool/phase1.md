# Phase 1 — The tool: extend, and cancel an internal target

Read [`general_plan_instructions.md`](general_plan_instructions.md) first.

**Goal.** `adjust_delegated_outreach` exists, is grantable, is registered on both harnesses, and
performs `ExtendOutreach` and `CancelTarget` through the unchanged
`TaskPersistence::execute_delegation_command`. It is not reachable from a production run until
phase 3; everything about it is testable now.

**Files touched**

| File | Change |
|---|---|
| `src/domain/entities/tool_catalogue.rs` | new id const, catalogue entry, Rig arm, two test constants |
| `src/application/task_queue.rs` | one new `TaskPersistence` method, `own_delegation_controls` |
| `src/adapters/persistence/task/outreach.rs` | its query |
| `src/adapters/persistence/task/operations.rs` | the trait impl forwarding to it |
| `src/application/services/agent_delegation_tool.rs` | **new**: the tool |
| `src/application/services/mod.rs` | `pub mod agent_delegation_tool;` |
| `src/application/services/outreach_tool.rs` | `resolve_channel_target` becomes `pub(crate)` |
| `src/application/services/native_tools.rs` | field, builder, declaration, `invoke` arm |
| `src/application/services/agent_runner/mod.rs` | construct it beside `TaskOwnershipTool` |
| `src/adapters/harness/rig/tools_capability_tests.rs` | add the declaration to the schema check |
| `docs/custom_tools.md`, `docs/rig.md` | the hand-written native tool lists |

---

## 1.1 Catalogue (`src/domain/entities/tool_catalogue.rs`)

Add beside the other ids (L16-L20):

```rust
pub const DELEGATION_CONTROL_TOOL_ID: &str = "adjust_delegated_outreach";
```

Add the entry after `TASK_OWNERSHIP_TOOL_ID`'s (L171-L176), keeping natives last:

```rust
CatalogueTool {
    id: DELEGATION_CONTROL_TOOL_ID,
    label: "Adjust work you delegated",
    description: "Extend the response window on a request you sent, or drop or move an internal \
                  recipient who will not answer. Cannot cancel the request or stop the task.",
    source: ToolSource::Native,
},
```

The description's second sentence is load-bearing: it is what an operator reads in the picker, and
the whole point of this authority is that it stops short of abandoning work.

Add the id to `supports_harness`'s Rig arm (L188-L203). The AiAgents arm excludes only
`REQUEST_APPROVAL_TOOL_ID`, so it needs no edit — but confirm that is still true rather than assuming
it.

Two test constants move: `assert_eq!(seen.len(), ALLOWED_BUILTIN_TOOL_IDS.len() + 5)` (L372) becomes
`+ 6`, and the id joins the list in `the_native_tools_are_grantable_and_grouped_last` (L388-L394).

## 1.2 The read the agent needs (`own_delegation_controls`)

The tool must learn, for the task it is running on, the outreach it created: its id, its current
`version` (the fence value), its status, and its active targets with enough detail to match a channel
selector. The general file explains why `get_collaboration_summary` is the wrong source.

Add to `TaskPersistence` (`src/application/task_queue.rs`, beside
`outreach_reassignment_context` at L296-L302):

```rust
/// The open outreach a given agent principal created on this task, as the agent's own delegation
/// tool needs to see it: the fence version and the targets still waiting.
///
/// Scoped by creator on purpose. It answers "what may *you* adjust", so a task whose outreach was
/// created by someone else reads as `None` rather than as something to be authorized later.
async fn own_delegation_controls(
    &self,
    company_id: Uuid,
    task_id: Uuid,
    creator: PrincipalId,
) -> AppResult<Option<OwnDelegationControls>>;
```

with, in the same file:

```rust
pub struct OwnDelegationControls {
    pub outreach_id: Uuid,
    pub version: u64,
    pub status: OutreachStatus,
    pub expires_at: DateTime<Utc>,
    pub targets: Vec<OwnDelegationTarget>,
}

pub struct OwnDelegationTarget {
    pub id: Uuid,
    /// `None` for an external recipient, which this authority may not cancel.
    pub internal_channel_id: Option<Uuid>,
    pub address: EmailAddress,
}
```

No default method body — `src/AGENTS.md` forbids a silently-successful default on a correctness
operation, and every test double must say what it returns.

The query lives in `src/adapters/persistence/task/outreach.rs` and is one statement:

```sql
SELECT outreach.id, outreach.version, outreach.status, outreach.expires_at
  FROM task_outreaches AS outreach
 WHERE outreach.company_id = $1 AND outreach.task_id = $2
   AND outreach.created_by_principal_id = $3
   AND outreach.status IN ('waiting', 'timeout_pending_approval')
 ORDER BY outreach.created_at DESC, outreach.id DESC
 LIMIT 1
```

then its `status = 'active'` targets (`id, internal_channel_id, email`) ordered by `id`. Filtering on
status in SQL rather than in Rust is deliberate: `waiting` and `timeout_pending_approval` are exactly
the two states `apply_operation` accepts, so a row this returns is a row the command can act on, and
the two lists cannot drift apart silently. Bound the target list the way
`load_collaboration_targets` does rather than reading an unbounded set.

Both reads go in one transaction so the version and the targets agree; the version is only a fence,
and `execute_delegation_command` re-locks and re-checks it, so a stale read costs a `Conflict` the
model can retry, never a wrong write.

`cargo sqlx prepare -- --all-targets` after this — see `src/AGENTS.md`.

## 1.3 The tool (`src/application/services/agent_delegation_tool.rs`)

Mirror `task_ownership_tool.rs` closely; a reviewer should be able to diff them.

```rust
pub use crate::entities::tool_catalogue::DELEGATION_CONTROL_TOOL_ID;

#[derive(Debug, Clone)]
pub struct DelegationToolContext {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub lease: crate::entities::task::TaskLeaseRef,
    /// Resolving a channel selector needs the same slugs `outreach_and_await_quorum` resolves with.
    pub company_slug: CompanySlug,
    pub app_domain_name: String,
    pub sub_agent_scope: SubAgentScope,
}
```

Input, all of it `Deserialize + JsonSchema` so the three forbidden operations are not expressible:

```rust
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum DelegationToolOperation {
    Extend,
    CancelInternalTarget,
}

#[derive(Deserialize, JsonSchema)]
struct DelegationToolInput {
    operation: DelegationToolOperation,
    /// New response window in hours from now, 1 to 720. Required for `extend`.
    timeout_hours: Option<u32>,
    /// The internal channel to drop, as `channel` or `company/channel` — the same value
    /// `list_company_agents` reports. Required for `cancel_internal_target`.
    target_channel: Option<String>,
    reason_detail: Option<String>,
}
```

`reason` is **not** an input. Each operation has exactly one honest `DelegationReason`
(`DeadlineChanged` for extend, `TargetUnavailable` for cancel), and letting the model pick from the
enum would put a free choice in the audit trail that nothing checks. `reason_detail` stays: it is the
one field where the agent's own words belong.

`call(&self, call_id, args, invocation)` runs, in order:

1. Deserialize; a parse error is `ToolInvocation::failure`, as everywhere else.
2. `let TaskOwner::Agent(actor) = self.context.lease.claimed_owner else { … failure }` — the same
   guard as `task_ownership_tool.rs:121-125`. This is not the authorization (that is `authorize`'s);
   it is how the tool knows which principal to submit as.
3. `own_delegation_controls(company_id, lease.task_id, actor)`. `None` is
   `ToolInvocation::failure("You have no open request to adjust on this task.")` — a state the model
   should be told about plainly, not an error.
4. Build the operation:
   - **extend**: require `timeout_hours`, reject outside `1..=MAX_TIMEOUT_HOURS`
     (`outreach_tool.rs:265` — import it, do not repeat `720`), reject a `target_channel`. Produces
     `DelegationOperation::ExtendOutreach { outreach_id, expires_at: Utc::now() + Duration::hours(h) }`.
   - **cancel_internal_target**: require `target_channel`, reject `timeout_hours`, resolve the
     selector with `outreach_tool::resolve_channel_target` (made `pub(crate)`; it already applies
     `sub_agent_scope` and refuses an address or a malformed selector), then find the one active
     target whose `internal_channel_id` matches. No match is a failure naming the channels that *are*
     open, which is the one message that makes the tool self-correcting. Produces
     `DelegationOperation::CancelTarget { outreach_id, target_id }`.

   Refusing the other operation's field rather than ignoring it copies
   `task_ownership_tool.rs:128-130` ("Release does not accept a target.") and keeps a confused call
   from looking like a successful one.
5. Submit:

```rust
let command = DelegationCommand {
    company_id: self.context.company_id,
    task_id: self.context.lease.task_id,
    command_id: invocation
        .map(|inv| inv.invocation_id.0)
        .unwrap_or_else(|| stable_command_id(self.context.lease.execution_generation, call_id)),
    expected_version: controls.version,
    actor: DelegationActor {
        principal_id: actor,
        authority: DelegationAuthority::OwningAgent,
    },
    reason,
    reason_detail: input.reason_detail,
    operation,
};
```

The `command_id` derivation is `task_ownership_tool.rs:187-191` and `stable_command_id`
(L216-L225) unchanged: the harness invocation id when there is one, otherwise
`SHA-256(execution_generation ‖ 0x00 ‖ call_id)` truncated to 16 bytes. Both are stable across a
retry of the same call and distinct across a re-execution of the task, which is what
`stored_result` needs to tell a retry from a new request. Copy the helper rather than sharing it
only if the two derivations are ever allowed to differ — they are not, so **extract it**: move
`stable_command_id` into a small shared module (`src/application/services/tool_command_id.rs`) and
have both tools call it. One derivation, one place.

6. Call `TaskPersistence::execute_delegation_command` **directly** with
   `DelegationCommandRequest { command, replacement: None }`. Not through `ThreadUseCases` — the tool
   is constructed by the agent runner, which the thread use case already reaches, and routing back
   would be a cycle. Phase 2 revisits this for reassignment, which is the only operation needing a
   composed `replacement`.
7. Map the result: `Ok(result)` → `ToolInvocation::success` with `outreach_id`, `operation`,
   `outreach_version`, `task_status`, `target_id`, `delivery_cancellation`. `Err(Conflict(m))` and
   `Err(NotFound(m))` → `ToolInvocation::failure(m)`; anything else → `denial_or_error`. See the
   general file for why.

The run continues: this is a `success`, never a `suspended`. The task stays parked on its outreach
exactly as it was — the outreach's fields changed, not the task's suspension.

Keep `call` under the 80-line trigger in `src/AGENTS.md` by extracting the two operation builders as
plain synchronous-where-possible helpers returning
`Result<(DelegationOperation, DelegationReason), String>`, with the `String` becoming the failure
message. Those helpers are pure decisions given the loaded `OwnDelegationControls`, so they unit-test
with no database and no mocks.

## 1.4 Registration

`native_tools.rs`: a `delegation: Option<AgentDelegationTool>` field (L41-L50), a
`with_delegation_controls` builder (beside L80-L84), a `declarations.push` in
`rebuild_declarations` — **after** the ownership push (L105-L107), because the declaration order is
documented as the catalogue's and the catalogue lists it after `transfer_or_release_task` — and an
`invoke` arm (L136-L158) forwarding `call_id, args, invocation`.

`agent_runner/mod.rs`: construct it inside the same `if let` that builds the ownership tool
(L481-L488), from the same `task_persistence` and `context`. Every field
`DelegationToolContext` needs is already in `OutreachToolContext` at that point — `company_slug`,
`app_domain_name`, `sub_agent_scope`, `lease`, `company_id`, `channel_id` — so this adds no new
plumbing into the runner. Do it *before* `context` is moved into `OutreachAndAwaitQuorumTool` at
L489-L494.

Docs: add the id to the `docs/custom_tools.md` list (L9) and to the Rig table row in `docs/rig.md`
(L64), with one line on what it may and may not do.

---

## Tests

**Pure, no database** (inline `#[cfg(test)] mod tests` in the new module):

- `extend` without `timeout_hours` fails; with `0`, with `721` fails; with `1` and with `720`
  succeeds and produces `expires_at` in that window.
- `extend` carrying a `target_channel` fails rather than ignoring it, and
  `cancel_internal_target` carrying `timeout_hours` likewise.
- `cancel_internal_target` naming a channel with no active target fails, and the message names the
  channels that do have one.
- `cancel_internal_target` naming a channel that is an **external** target's address fails — the
  selector resolver already refuses an address (`outreach_tool.rs` tests cover the resolver; assert
  here that the tool does not fall back to matching on `address`).
- The input schema does not deserialize `"cancel_outreach"`, `"proceed_with_partial"` or
  `"stop_task"` as an operation. This is the test that keeps the three forbidden operations out of
  reach even if `controls.rs` ever loosened.
- `stable_command_id` is stable for one `(execution_generation, call_id)` and differs across
  generations. If the helper moves to a shared module, the test moves with it.

**Against the database** (`src/adapters/persistence/task/tests.rs`), modelled on
`owning_agent_authority_is_limited_to_its_internal_delegation_scope` (currently L6546-L6617) and
built on `delegation_fixture` (L6124) — that fixture makes **external** targets, so a
cancel-internal test needs an internal one, which `OutreachTargetIdentity::InternalChannel` and a
second seeded channel give:

- `own_delegation_controls` returns the outreach and its active targets for the creating principal,
  and `None` for another principal in the same company, for the same principal on another company's
  task, and once the outreach is cancelled or completed. Scope every assertion to the ids the test
  created — this database is shared and tests run in parallel, so no `count(*)` over a whole table.
- An owning agent's `CancelTarget` on an **internal** target succeeds and bumps the version to 2;
  on an **external** target it is `NotFound` (this is `controls.rs:568-572`, and it is the one
  restriction a reader of the tool's input would not guess).
- `ExtendOutreach` from the creating agent moves `expires_at` and leaves the task parked — assert the
  task's `status` is still `waiting_for_third_party_reply` afterwards, because a control that
  silently woke the task would be a much worse bug than one that refused.
- The audit row in `delegation_control_commands` for the command id records
  `('agent', 'owning_agent', 'deadline_changed')`, the way
  `delegation_commands_are_idempotent_fenced_and_audited` asserts its own.
- The same `command_id` twice returns the stored result and does not bump the version again.

**Registration:**

- `every_declaration_names_a_catalogued_tool_and_carries_an_object_schema`
  (`native_tools.rs:204-230`) gains the declaration.
- `every_real_native_declaration_compiles_without_schema_or_name_translation`
  (`rig/tools_capability_tests.rs:31-49`) gains it too — a `schemars` schema that Rig cannot compile
  is the failure mode this catches, and `Option<String>` plus two enums is exactly the shape that
  has bitten before.
- An empty `NativeToolHost` refuses the new id as `AppError::Internal`, like the existing
  `an_empty_host_offers_nothing_and_refuses_every_call`.

## Done when

- [ ] `adjust_delegated_outreach` is in the catalogue, supported on both harnesses, and the two
      catalogue test constants are updated.
- [ ] `own_delegation_controls` exists on the port with no default body, is implemented once, and
      `.sqlx` is regenerated.
- [ ] The tool submits `OwningAgent` commands for two operations and nothing in `controls.rs`
      changed.
- [ ] `Conflict` and `NotFound` reach the model as tool failures; nothing else does.
- [ ] `stable_command_id` has one definition, shared with `task_ownership_tool.rs`.
- [ ] Tests above pass; `cargo test` and `cargo clippy --all-targets -- -D warnings` are green.
