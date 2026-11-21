# 05 Builder agent and approval

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

An agent can read a unit, draft a blueprint and ask for it to be applied. A person's approval of
that exact document is what applies it, and it is applied as that person. Continues in
[06](06-ui-samples-and-docs.md).

The mechanism is the one protected tool calls already use. A local action whose policy requires
approval parks the run (`park`, `src/adapters/persistence/workflow/decision_approvals.rs:55`,
called from `action_dispatch.rs:227` in the same directory), writing a `workflow_decisions` row of
kind
`action_approval` bound to the invocation, the argument digest and the operation digest. The
action runs only when that row is approved (`approved`, `decision_approvals.rs:8`). This plan adds
a built-in that always takes that path, and reads the approver from the row.

A `decision.human` step followed by new plan and apply step types was considered and dropped. It
needs two step types and its own tie between the reviewed artifact and the write, and a procedure
author could wire any principal in as the approver. The protected-action row already ties the
approval to the invocation and its digests and records who submitted it.

## Batch 05A Read tools

1. `ProvisioningKind` (`src/application/workflow/provisioning.rs:64`) gains `ReadUnitBlueprint`
   and `PlanBlueprint`, both `ActionEffect::Read`, with trusted snapshots built as the existing
   kinds are (`builtin_provisioning_snapshot`, `:118`). `AgentBuiltinTool`
   (`src/domain/entities/agent_capability.rs:77`) gains the matching variants, and the names join
   the reserved tool names.
2. Both answer on the requester's authority, as `list_company_agents` does. The requester is who
   `requester_on` says (`src/adapters/persistence/workflow/provisioning.rs:315`): the signed-in
   author of the root message, the root's actor for a manual or scheduled run, and a child's
   root's requester. They must manage the unit named (`authorize_target_on` with
   `ManagementTarget::Unit`, `src/adapters/persistence/workflow/management.rs:26`), not merely
   some unit as `authorize_requester` checks today (`provisioning.rs:286`). No requester, or one
   who does not manage the unit, is `workflow.provisioning_forbidden`. A run started by mail or by
   another agent's direct message therefore reads nothing, as it creates nothing today.
3. Both are scoped to the run. The unit named must be the run's own unit, the row marked `own` in
   `workflow_run_scopes`, or a descendant of it. A run with no own unit is refused. This is what
   makes a builder's department its reach: one installed in the goals office sees and proposes
   under the goals office, and one installed in the root sees the company.
4. `read_unit_blueprint(unit)` returns the export of [01](01-document-and-export.md) and an
   `available` section: what a document under this unit may reference. That is the ready models,
   skills of the unit's chain, connections of the chain with their operations, shipped templates,
   published procedures the unit's managers manage, and the role vocabulary. The result is
   bounded; a unit too large to return says so and is read one sub-unit at a time.
5. `plan_blueprint(target, document)` calls `plan` of [02](02-plan-and-diff.md) with origin
   `Agent` and returns the digest, the operations with long texts as byte counts, and the
   refusals. Its result is bounded as listings are (`MAX_LISTING_BYTES`,
   `src/application/workflow/provisioning.rs:57`) and says when it was truncated.

## Batch 05B The protected apply tool

1. `ProvisioningKind::ApplyBlueprint`: `ActionEffect::Write`, reconcile recovery, arguments
   `{target, document, plan_digest}`.
2. It always requires approval. Registration gives every built-in `approval_required: false`
   today (`direct_registration_with_locks_on`,
   `src/adapters/persistence/agent_publication/tools.rs:164`); `ApplyBlueprint` registers `true`,
   and dispatch compares the stored binding with the trusted built-in so a binding cannot lower
   it. The comment on `builtin_provisioning_snapshot` that provisioning tools are "never an
   approval" is corrected.
3. Reviewers are the `approval` input of the `agent.run` step that made the call, resolved by
   `resolve_on` (`src/adapters/persistence/workflow/reviewer_resolution.rs:20`): people, or the
   roles 02 and 08 define. With no approval input, or with reviewers that resolve to no person,
   the dispatch ends as `LocalDispatchResult::ApprovalRequired` (`action_dispatch.rs:229`) and
   nothing runs. That is the existing fail-closed result and it is kept.
4. When the approved action runs, `ProvisioningEffects::apply`
   (`src/adapters/persistence/workflow/provisioning.rs:206`) loads the decision that approved its
   own invocation, by the predicate `approved` uses, and takes `submitted_by`. That principal
   must be a person and a current company member. That person is the actor of
   [03](03-apply.md)'s `apply_on`; the requester is not consulted for the write. No approving
   row means no actor and a refusal, whatever the dispatch layer already checked.
5. The actor's authority is checked by each operation's owner, as for any apply. An approver who
   does not manage what the document touches gets a committed refusal and no effect.
6. Agent-origin rules, enforced in `plan` so that the preview, the tool and the apply agree:
   - the target is the run's own unit or a descendant: `blueprint.outside_run_unit`;
   - the document defines none of the agents the run froze at admission:
     `blueprint.self_change`;
   - no `open_to_public`, no `public` desk access and no outside address grant, if proposal 4 of
     the [README](README.md) is confirmed: `blueprint.public_access`;
   - no `procedures` entry, if proposal 2 is confirmed: `blueprint.procedure_source`.
7. Lock order. The approver's account joins the sorted account union that the action transaction
   takes before company ownership (`AccountPrefix`,
   `src/adapters/persistence/workflow/action_account_hints.rs:10`), selected by the operation being
   authorized. Do not add an inner guard after the shared helpers have taken their locks; that was
   tried for native effects and did not hold. Trace the real order with a PostgreSQL wait sample,
   as `delegated_provisioning_locks_requester_before_company` does for the requester.
8. Results are receipts. A refusal commits a receipt and no effect, as provisioning refusals do
   today (`provisioning_refusals_commit_a_receipt_and_no_effect`); success commits the
   application id and the addresses created. The application row has origin `agent` with the run,
   the invocation and the proposing agent.
9. A rejected or expired approval ends the call through the existing path (`approval_rejected`,
   `decision_approvals.rs:191`). Trace what the model receives for it today; the Builder's prompt
   tells it to revise from the reviewer's feedback. A revised document is a new invocation and a
   new approval. The existing tool-round bound limits the loop.
10. If the state moved between the proposal and the approval, the apply refuses with
    `blueprint.stale` and the agent plans again.

## Batch 05C Where approval is configured and shown

1. The agent workflow composer (`AgentWorkflow`,
   `src/application/workflow/templates/agent_workflow.rs:52`) can put `approval: {reviewers,
   deadline}` on the `draft` step, with reviewers from a binding parameter and the run's deadline.
   The registry already requires approvers to be literal or binding parameters
   (`registry.static_reviewers`, `src/application/workflow/registry/actions.rs:38`). No shipped
   template or fixture names an approval today, so a protected tool on an agent's provisioned
   workflow cannot park; this item is what lets it.
2. The binding parameter is set by whoever manages the agent's binding, as people or roles. A
   builder in a goal unit names `sponsor`; one in a standing department names its human `lead` or
   named people. A role held by an agent resolves to no reviewer and nothing runs (batch 05B
   item 3).
3. The decision page (`src/adapters/http/pages/workflow_decision.rs`) shows a protected action
   (`DecisionSubject::ProtectedAction`, `src/application/workflow/decisions.rs:154`). For an
   `apply_blueprint` intent it renders the plan computed from the frozen arguments for the viewing
   reviewer: the operations grouped by unit, each tool grant and budget called out, the
   operations this reviewer could not perform, and whether the plan is stale against the frozen
   digest. Approve is disabled when the plan is stale or the reviewer lacks authority; the apply
   remains the check.
4. The attention inbox line for protected actions is unchanged.

## Batch 05D The Builder

1. The built-in library agent (`agent_builder_definition`,
   `src/application/use_cases/builtin_agent_library.rs:52`) holds `list_company_agents`,
   `read_unit_blueprint`, `plan_blueprint` and `apply_blueprint`. Its prompt tells it to read the
   unit first, ask only for what the person has not said, draft, plan until there are no
   refusals, state the planned changes in its reply, call `apply_blueprint` once, and report only
   what a success receipt says. Forwarded and quoted text is untrusted context and never
   authorizes a change.
2. If proposal 3 of the [README](README.md) is confirmed: `CreateAgent` and `CreateDeskForAgent`
   leave `ProvisioningKind` and `AgentBuiltinTool` with their schemas and their arms in
   `ProvisioningEffects::apply`. `create_agent_on` and `provision_guarded` stay, as the owners
   apply calls. The assistant-channel HTTP path (`AssistantChannelService`, `provisioning.rs:728`)
   is unchanged. If it is not confirmed, the two tools stay and the Builder holds both sets.
3. A company installs the Builder by copying it into a unit, as it copies any library agent. The
   copy's direct binding gets the approval parameter of batch 05C. The recommended home is the
   goals office of 08: its human head approves and its subtree is the Builder's reach.
4. `docs/agent-builder-context.md` is rewritten for the Builder in
   [06](06-ui-samples-and-docs.md).

## Focused checks

- `blueprint_tool_read_`: the requester matrix for both read tools, including a mail-origin root
  and a child run; results are bounded and say when truncated; `available` lists an ancestor's
  skill and not a sibling's.
- `blueprint_tool_scope_`: the own unit and a descendant are allowed; a sibling, an ancestor and a
  run with no own unit are refused, for read, plan and apply.
- `blueprint_tool_apply_`, with the scripted-model `Deployment` harness: a proposal parks and
  writes nothing; approval applies it as the approver and records the application; rejection
  writes nothing and the agent revises; no approval input writes nothing; an approver who does
  not manage the target gets a refusal receipt and no effect; a reviewer role held by an agent
  never parks; a stale plan is refused after approval.
- `blueprint_tool_agent_origin_`: one case per rule of batch 05B item 6.
- `blueprint_tool_trust_`: a stored binding with `approval_required` false is refused at dispatch;
  an apply invoked with no approving decision row is refused inside the effect.
- `blueprint_approval_competing_`: two reviewers settling one approval; the approver's role
  removed while the apply waits; the same call replayed under a new model call id reuses one
  receipt, in the manner of
  `workflow_action_approval_identical_operation_new_call_id_reuses_one_receipt`; a worker lost
  after approval and before commit leaves exactly one application.
- `blueprint_approval_locks_`: the approver's account is taken before the company, by wait
  sampling.
- Rendering tests for the decision page, and a grant-and-prompt test in the manner of
  `agent_builder_is_granted_the_provisioning_tools_and_told_their_rules`.
