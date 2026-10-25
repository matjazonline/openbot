# P08-D2 — Built-in provisioning tools and assistant-channel setup (rev 1)

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Architecture gate for p08 batch B3a (split from B3 by D2.0). Inputs:
- the user decisions P08-Q2 (run-as), P08-Q4 and P08-Q5 (TASK.md);
- REPLACEMENT-MAP row 91 ("Action invocation owns authorized/idempotent agent/channel
  provisioning as an effect when invoked by an agent");
- the RESUME 06C obligation, quoted verbatim in criterion 13;
- findings WF-P08-F007 and F012.3;
- deviation D-5 (`TemplateCatalogue` composition).

Facts cite the B2 artifact (digest `e43d8c37…b94f`, base `da86871`).

## D2.0 Batch plan: split B3

| Batch | Scope | Gate |
| --- | --- | --- |
| **B3a** Provisioning + assistant setup | D2.1–D2.6: the built-in tools `create_agent_channel` and `list_company_agents`; the provisioning local action; one-transaction assistant provisioning shared with the UI's "Create assistant channel"; the `/run/deadline` reference and the reviewed template; shipped `TemplateCatalogue` (D-5); Agent Builder prompt and grants. | this document |
| **B3b** Interfaces | Company-scoped JSON routes (definitions save/validate/publish/archive; bindings configure/activate/deactivate with the refusal surface; runs start/read/list/cancel/retry with the WF-C002 child rule; comments; decisions; action reconciliation; template copy); workflow tool routes and agent `workflow_tools` grants (WF-C003); minimal `/ui/workflow-decisions/{id}` page. | none (established patterns) |

Why split: B3a is a cross-subsystem contract that is costly to unwind (a new action path and a new
transaction composition). B3b is breadth over existing commands. One review could not be thorough
over both (cf. F012.5).

## D2.1 Tool contracts

- **Names:** `create_agent_channel` and `list_company_agents`, reserved like `message.send`/`message.reply`:
  - a company HTTP operation or workflow-tool registration with either name is refused;
  - an agent obtains a tool by listing its name in `agents.workflow_tools`, the existing grant
    mechanism;
  - publication captures the trusted built-in snapshot (`agent_publication::granted_tool`).
- **Kind:** a new `ToolKind::Provisioning`, with `builtin_provisioning_snapshot(company, kind)` and
  `is_trusted_provisioning_snapshot`. These mirror `builtin_message_snapshot` (fixed schemas, no
  connection slot, recovery `Reconcile`). `list_company_agents` is effect `Read`;
  `create_agent_channel` is effect `Write`.
- **No built-in approval (P08-Q5).** The trusted snapshot requires none, and company action
  approval policies for HTTP/MCP do not apply to built-ins. Where a deployment wants approval, it
  belongs in a workflow step.
- **`list_company_agents`:**
  - Input `{}` (additional properties refused).
  - Output: `{agents: [{slug, name, description, provider, model}], models: [{provider, model}]}`
    (≤ 200 agents, by slug).
  - `models` lists the company's ready model connections. The Builder needs them to propose an
    explicit provider/model, which is the 06C obligation.
- **`create_agent_channel`:**
  - Input:

    | Field | Rule |
    | --- | --- |
    | `name` | 1..120 |
    | `slug` | the agent slug rules |
    | `description` | optional, ≤ 500 |
    | `system_prompt` | 1..32 768 |
    | `provider`, `model` | required; must name a ready company model connection |
    | `reply_mode` | `"autonomous"` \| `"reviewed"` |
    | `channel_slug` | optional, default `slug`; channel slug rules |
    | `tools` | optional, ≤ 32 `TypeName`s; each must be grantable at creation |
    | `skills` | optional, ≤ 16 exact company skill slugs |

  - Output: `{agent: {id, slug}, channel: {id, slug, address}, workflow_id, version_id,
    binding_id, reply_mode}`.

## D2.2 Execution path (§04.10 marker-first, unchanged classifier)

- `AgentToolTarget::Provisioning`. `AgentActionTools` dispatches it to
  `ProvisioningRuntime::call_tool(fence, ProvisioningToolCall{kind, arguments, call})`:
  1. ActionService prepares and freezes the action, with subject and argument digest as for
     message tools.
  2. `dispatch_local`.
  3. A production `SqlLocalAction`, `ProvisioningEffects`, applies the effect inside the action's
     own transaction. Marker, effect and receipt commit together, so the effect runs at most
     once; a replay returns the stored receipt; ambiguity resolves by the receipt.
  4. This is the first production `SqlLocalAction`.
- **Classifier:** the step kind is `agent.run`, which `marker_owned_effect_kind` already owns, so
  the recovery classifier needs no change (§04.10 obligation preserved).
- **Authority** (P08-Q2 extended; **new security invariant**):
  - The action authorization already requires the run's actor (the binding run-as) to be a
    current owner/admin.
  - `create_agent_channel` additionally requires the run's **requester** to be a current
    owner/admin. The requester is:
    - for a message-triggered root, the triggering message's author principal;
    - for a child, the requester of the root;
    - for manual or scheduled roots, the run actor.
  - Without this, any member allowed to write to the Builder's channel could get agents and live
    channels created with an admin's authority.
  - A refused requester yields a tool error (`workflow.provisioning_forbidden`), not an effect.
  - `list_company_agents` needs only the run actor (read).

## D2.3 Assistant provisioning: one transaction

`provision_assistant_on(tx, AssistantProvisioning)` runs inside the workflow persistence module
and is shared by the tool and the UI command.

Input:
- `{company, actor, requester}`;
- `agent: New(AgentSpec) | Existing(slug)`;
- `channel: {slug, name}`;
- `reply_mode`;
- `identity: ProvisioningKey`.

Steps:
1. **Company lock and authority.** `authorize_company(actor)`, plus the requester check (D2.2).
2. **Agent** (New only).
   - Uses `insert_agent_on`, `create_agent_principal_on` and the skill/tool attachments, with
     explicit provider/model validated against the company's model connections.
   - No personal channel: P08 "agent creation creates an agent only". The coupling itself is
     deleted in B4.
3. **Channel.**
   - A new channel with access mode **`team`** (never public, P08-Q4), enabled, no owner agent.
   - Until B4 retires D-4's legacy constraint, the agent sits at position 0 of `channel_agents`.
   - A slug collision in the company is refused (no merge into an existing channel).
4. **Workflow.**
   - The shipped template (`autonomous-response` or `reviewed-support`, D2.5) is decoded and its
     `agent.run` step's `agent` literal is replaced structurally with the agent's slug. There is
     no textual substitution. `workflow_id` is rebased to the derived id (`rebase_workflow_id`).
   - It is compiled with dependencies captured **in this transaction**. The body of
     `AgentPublicationDirectory::capture` is factored into
     `capture_agents_on(tx, actor, draft, captured)` (it already uses `derive_on` and
     `granted_tool` on a transaction); `capture` keeps opening its own. The new agent row is
     visible here, so `verify_on` compares against what was just written.
   - The bundle is made by the same pure `publication::freeze(decoded, company, version,
     snapshots, children)` that `DefinitionService::compile` uses, followed by the same
     workflow-identity and refused-kind checks. The shared code is extracted, not copied. The
     templates have no child, HTTP or MCP steps, so the inner directories capture nothing; a
     template that had them would be refused.
   - It is written with the existing tx-scoped `insert_draft` (origin = template id/revision),
     `insert_version` and `event`.
5. **Binding.**
   - `Channel(channel)`, `message_trigger = direct`, `run_as = actor`.
   - Params: `{}` for autonomous; `{reviewer: requester}` for reviewed.
   - Active immediately.
   - Written through the tx-scoped pieces `save_binding` already uses (`verify_selection`,
     `check_readiness`, `check_run_as`, the message-binding cap, `write_head`,
     `insert_configuration`, the event). They are factored into one
     `save_binding_on(tx, PreparedBinding)` that both callers use.
6. **Identity.**
   - Every id (agent, channel, workflow, version, binding, publication key) derives
     deterministically from `ProvisioningKey`:
     - for the tool, the action invocation;
     - for the UI, `(company, client command key)`.
   - The derivation is SHA-256-based, as B2-c.
   - The tool is exactly-once by its receipt. The UI command replays: an existing binding with
     the derived id returns the stored result, and an equal key with a different request is
     `Conflict`.

**Why one transaction:** no partial agent-without-channel or channel-without-binding state exists
to recover. Template compile and publication checks are pure or tx-scoped today; the definition
and binding services' own transactions are bypassed only by calling the same validated helpers
under the same company lock. The cost is a refactor of `save_binding` into a tx-scoped core.
Validation of the bundle runs through the same compiler and publication checks as
`DefinitionService::publish`, so no unvalidated path is opened.

## D2.4 "Create assistant channel" (UI/API)

- `POST /api/companies/{company}/assistant-channels` with `{agent_slug, channel_slug, channel_name,
  reply_mode, command_key}`, plus the matching agent-page action.
- Owner/admin only. The caller is the actor and the requester.
- Runs `provision_assistant_on` with `Existing(agent)` in its own transaction.
- Returns the same output shape as the tool.

## D2.5 Templates and the reviewed deadline

- **Shipped catalogue (D-5).** `TemplateCatalogue::shipped()` holds the fixtures as seed
  templates: `autonomous-response` and `reviewed-support` are the reply modes; the rest are
  offered for copy (B3b route).
- **Reviewed deadline.** `decision.human` needs an RFC3339 deadline. A binding parameter would
  be one fixed instant for every future run, so the reviewed template cannot keep
  `deadline: {ref: /params/deadline}`.
  - Add the run-context reference **`/run/deadline`**: the run's own admission deadline, as an
    RFC3339 string, beside the existing `/run/id`.
    - The compiler gives it schema `{type: string, format: date-time}`.
    - The runtime resolves it from the run row.
    - Children see their own (inherited) deadline.
  - `reviewed-support` uses `deadline: {ref: /run/deadline}` and drops the `deadline` parameter.
    Its parameters are `{reviewer}` only.
  - The fixture tests and examples are updated with it.

## D2.6 Agent Builder

- The library definition grants `["list_company_agents", "create_agent_channel"]`.
- The prompt is revised:
  - check existing agents with `list_company_agents` before designing;
  - propose an explicit provider/model from `models`;
  - ask the user to choose autonomous or reviewed replies, explaining the difference;
  - say that the channel is team-only;
  - after explicit confirmation, call `create_agent_channel` once with the agreed spec;
  - report the address and binding.
- Reaching the Builder: an admin runs "Create assistant channel" (D2.4) for the Builder agent.
  Install-time setup and onboarding are B4 ("Agent Builder install").

## Invariants

1. Every provisioning effect is one transaction with its action marker and receipt; a replay
   returns the receipt; no second agent, channel or binding.
2. A created assistant channel is never public (`team`), and is bound to exactly one active
   binding of a template-derived workflow whose agent is the created (or selected) agent.
3. Provisioning requires a current owner/admin actor and requester; no member can provision
   through an agent.
4. Built-in tool names cannot be shadowed by registrations; publication captures only the
   trusted snapshot.
5. The compiled bundle of a provisioned workflow passed the same compile and publication checks
   as a `DefinitionService::publish`.
6. `/run/deadline` is the run's own deadline; the reviewed assistant's decisions expire with
   their run.

## Failure modes

| Failure | Behavior |
| --- | --- |
| Slug collision (agent or channel) | Effect refused; tool error `workflow.provisioning_conflict`; nothing written. |
| Provider/model not a ready company connection | Tool error `workflow.invalid_input`; nothing written. |
| Requester not owner/admin | Tool error `workflow.provisioning_forbidden`; nothing written. |
| Crash after commit, before the tool observation | Replay returns the receipt; the agent continues. |
| Message-binding cap reached on the new channel | Impossible (new channel); the check still runs. |
| Template compile refused (e.g. agent tools not grantable) | Effect refused with the diagnostic; nothing written. |
| UI command retried with the same key | Same result; a different request with the same key → 409. |

## Affected contracts

- `ToolKind` (+`Provisioning`), `AgentToolTarget` (+`Provisioning`), `AgentActionTools`
  (+provisioning runtime), and the agent publication `granted_tool`;
- HTTP operation and workflow-tool registration name checks (reserved names);
- ActionService local dispatch (first production `SqlLocalAction`);
- `save_binding` refactored to `save_binding_on`;
- definition `insert_draft`/`insert_version` reuse;
- compiler reference roots (`run.deadline`) and the runtime context;
- `reviewed-support` fixture and its tests;
- `builtin_agent_library` (prompt, grants);
- agent creation (no personal channel on this path);
- routes (D2.4).

## Verification plan (B3a)

- DB tests:
  - **The tool through the production engine** (scripted provider calls `list_company_agents`,
    then `create_agent_channel`):
    - agent, team channel, workflow version, active binding and receipt written in one commit;
    - a replay of the same call creates nothing new.
  - **Requester:** a plain member's message to the Builder channel → `provisioning_forbidden`,
    nothing written.
  - **Refusals:**
    - slug collision;
    - provider/model refused;
    - reserved names refused as HTTP and workflow-tool registrations;
    - an untrusted snapshot under a reserved name refused at publication.
  - **Reply modes:**
    - the created autonomous channel answers an inbound message end to end;
    - the reviewed one parks a decision whose deadline equals the run deadline;
    - UI command idempotency and conflict.
  - **Crash between local commit and observation** → replay.
- Compiler unit tests for `/run/deadline`.
- Builder definition test (grants, prompt clauses).
- clippy, fmt, fresh migration, stack budget for the provisioning path.
