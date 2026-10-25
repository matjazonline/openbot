# P08-D2 — Built-in provisioning tools and assistant-channel setup (rev 2)

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Architecture gate for p08 batch B3a. Rev 2 answers WF-P08-R009 (F036–F045) and replaces rev 1
(`decision-D2-r1.md`, sha256 `dfa1bf4d…0c07`, kept unchanged). Inputs:
- P08-Q2 (run-as), P08-Q4 (agent plus non-public channel on the Builder-chosen reply mode), P08-Q5
  (no built-in approval);
- REPLACEMENT-MAP row 91;
- the RESUME 06C obligation (criterion 13);
- WF-P08-F007 and F012.3;
- deviation D-5.

Facts cite the accepted B2 tree (digest `e43d8c37…b94f`, base `da86871`). The working tree has
moved since by the F035 correction alone (`schedules.rs`, `schedule_tests.rs`); the file list is
unchanged, and the staged deletions were already part of B2.

## Changes from rev 1

| Finding | Change | Section |
| --- | --- | --- |
| F036 | The requester is the configuring actor and the run-as of everything provisioned. | D2.2, D2.3 |
| F037 | A message root counts only when its intent's origin is `application`; the lookup is spelled out. | D2.2 |
| F038 | Every check of the bypassed agent, channel, definition and binding services is named and extracted, not copied; new tx-scoped channel insert. | D2.3 |
| F039 | `list_company_agents` takes the same requester check. | D2.2 |
| F040 | Provisioning authority and composition, refusal mapping (committed refusal receipts under a savepoint), the read tool's path, and reconciliation are specified. | D2.2a |
| F041 | The UI command writes a command record with a request digest, checked first. | D2.4 |
| F042 | One reserved-name rule for built-in names at HTTP and workflow-tool registration and the `tool.call` literal. Classification is restricted to name-only snapshots; trusted-check sites are named; the MCP behaviour is stated; `tools` cannot grant provisioning. | D2.1 |
| F043 | Competing-claimant and authority tests. | Verification |
| F044 | Library copy carries `workflow_tools` and an explicit model for workflow-capable definitions; library → company → assistant-channel test. | D2.6 |
| F045 | `/run/deadline` canonical rendering and documentation. | D2.5 |

## D2.0 Batch plan: split B3

| Batch | Scope | Gate |
| --- | --- | --- |
| **B3a** Provisioning + assistant setup | D2.1–D2.6 | this document |
| **B3b** Interfaces | Company-scoped JSON routes (definitions save/validate/publish/archive; bindings configure/activate/deactivate with the refusal surface; runs start/read/list/cancel/retry with the WF-C002 child rule; comments; decisions; action reconciliation; template copy); workflow tool routes and agent `workflow_tools` grants (WF-C003); minimal `/ui/workflow-decisions/{id}` page. | none |

The split is unchanged (B confirmed it as sound in R009).

## D2.1 Tool contracts and reserved names

- **Names.** `create_agent_channel` and `list_company_agents` (`PROVISIONING_TOOLS`).
- **Reserved names.** `is_reserved_tool_name(name) = is_builtin_message_tool(name) ||
  is_builtin_provisioning_tool(name)` is the single rule. It is refused at:
  - HTTP operation registration: `application/workflow/http.rs` validation and the persistence
    write in `adapters/persistence/workflow/http.rs`. Today neither refuses `message.*`; this
    closes that too.
  - workflow-tool registration (`workflow_tools.rs:180`, which today checks messaging only);
  - the registry `tool.call` literal (`registry/actions.rs:88`, today messaging only).
- **Classification.** `ToolSnapshot::kind()` returns `ToolKind::Provisioning` only for a
  provisioning name with **no workflow pin and no connection slot**:
  - A name-only snapshot, which is an HTTP registration or the built-in, can carry the name only
    as the built-in. A forged one fails the trusted check below.
  - MCP tools are slot-scoped (`connection = Some(slot)`) and stay `ToolKind::Slot`. A third-party
    MCP tool that happens to use one of these names is therefore not shadowed and not refused.
  - If one agent were granted both, the existing declared-name uniqueness check refuses the
    selection at preparation (`agent.rs:119-121`).
- **Trusted-check sites.** `is_trusted_provisioning_snapshot` is required wherever
  `ToolKind::Provisioning` is matched, exactly as for `ToolKind::Message`:
  - `agent_publication.rs:295` (`granted_tool`, capture);
  - `authority.rs:113` (`supported_publication`, Publish and Run);
  - `resources.rs:49` (readiness);
  - `agent_preparation.rs:122` (dispatch target);
  - `agent.rs:248` (target/kind consistency);
  - `ProvisioningActionAuthority::lock_current` (D2.2a).
- **Kind and snapshot.**
  - `builtin_provisioning_snapshot(company, kind)`: fixed schemas, no connection slot, recovery
    `Reconcile`, `policy_revision` 1.
  - Effect: `Read` for `list_company_agents`, `Write` for `create_agent_channel`.
- **No built-in approval (P08-Q5).**
  - `approval_required` is always false.
  - Company action approval policies do not apply to built-ins.
  - A deployment that wants approval adds a `decision.human` step to its workflow.
- **Output schemas.** Each output schema is `oneOf` the success shape and the typed error
  `{error: {code, message}}`:
  - the error is the shape `tools::tool_error` already returns;
  - `code` is one of `workflow.provisioning_forbidden`, `workflow.provisioning_conflict` and
    `workflow.invalid_input`;
  - `validate_action_result` checks receipts against this schema.
- **`list_company_agents`:**
  - Input `{}`, no additional properties.
  - Success: `{agents: [{slug, name, description, provider, model}], models: [{provider, model}]}`.
  - At most 200 agents, sorted by slug.
  - `models` lists every enabled model of each connection that has an API key.
- **`create_agent_channel`:**
  - Input:

    | Field | Rule |
    | --- | --- |
    | `name` | 1..120 |
    | `slug` | agent slug rules |
    | `description` | optional, ≤ 500 |
    | `system_prompt` | 1..32 768 |
    | `provider`, `model` | required; must be an enabled model of a company connection with a key |
    | `reply_mode` | `"autonomous"` \| `"reviewed"` |
    | `channel_slug` | optional, default `slug`; channel slug rules |
    | `channel_name` | optional, default `name` |
    | `tools` | optional, ≤ 32 names; each must resolve through `granted_tool`, and **reserved provisioning names are refused**, so a created agent cannot provision in turn (message tools stay grantable) |
    | `skills` | optional, ≤ 16 exact company skill slugs |

  - Success: `{agent: {id, slug}, channel: {id, slug, address}, workflow_id, version_id,
    binding_id, reply_mode}`.

## D2.2 Execution path and authority

- **Path (§04.10 marker-first, classifier unchanged).**
  1. The agent target is `AgentToolTarget::Provisioning { name }`.
  2. `AgentActionTools` dispatches it to `ProvisioningRuntime::call_tool(fence,
     ProvisioningToolCall{kind, arguments, call})`.
  3. ActionService prepares and freezes the request; the target is
     `ActionTarget::Local{resource: company, resource_kind: "provisioning"}`.
  4. `dispatch_local` (`action_dispatch.rs:210-276`) takes the company lock first (`prelock_on`),
     then the fence, the attempt and the run.
  5. In one transaction it checks for an existing marker, writes the marker, runs
     `SqlLocalAction::apply`, then writes the receipt.
  6. Ambiguity resolves by the receipt.
  7. `agent.run` is already marker-owned (`actions/failure.rs`), so the recovery classifier is
     unchanged.
- **Authority, in two layers.**
  1. The action layer (unchanged): the run actor, i.e. the Builder binding's run-as, must be a
     current owner/admin. This is checked in `prelock_on` and again in `lock_current`.
  2. The provisioning layer (new, both tools; **F039**): the run's **requester** must be a
     current owner/admin, checked with `authorize_company(tx, company, requester)`. This locks
     the company row (already held) and takes `FOR SHARE` on the requester's member and
     principal rows through commit.
     - A refusal is the committed typed error `workflow.provisioning_forbidden`. Nothing is
       provisioned and nothing is listed.
- **Requester derivation (F037)**, run inside `apply` on the action transaction:
  1. **Root.** Walk `workflow_runs.parent_run_id` from the run to the root with a recursive query
     bounded at 64 levels (deeper is refused). A child's requester is its root's.
  2. **Message root.** The root is a message root when `workflow_message_intents (company_id,
     id = root.trigger_id)` exists. B1 admission makes a message trigger the intent id;
     `admission_source` verifies it. The requester is then:
     - `messages (company_id, id = intent.message_id).author_principal_id`;
     - → `principals` with `kind = 'person'`;
     - → `user_id`;
     - **and only if `intent.origin = 'application'`**: the signed-in path, whose author comes
       from the authenticated account (`routes/channel.rs:1305-1324`, `commit.rs:176`).
     - Origins `external` (email: DMARC authenticates a domain, not a member) and `internal`
       (relay), or a non-person author, yield no requester, so the call is forbidden.
  3. **Manual or scheduled root** (no intent row): the requester is `root.actor_id`.
     - Manual: the authenticated caller of `manual::start`, including a rerun.
     - Scheduled: the binding run-as captured on the occurrence, an owner/admin when configured
       and re-authorized at admission.
- **Run-as and attribution (F036).**
  - Everything `create_agent_channel` writes is configured **by the requester**:
    - agent and channel `created_by = CreationProvenance::user(requester)`;
    - draft, version and binding events carry `actor_id = requester`;
    - the binding `run_as = requester`.
  - The D1.1 `check_run_as` therefore passes on its ordinary rule (the run-as is the configuring
    actor). The Builder's run-as never becomes the run-as of what it creates.
  - The reviewed template's `reviewer` parameter is the requester.

## D2.2a Local-action composition, refusals, read path, reconciliation (F040)

1. **Authority and composition.**
   - `ProvisioningActionAuthority: SqlActionAuthority`. `lock_current`:
     - checks the `provisioning` target names the scope's company;
     - runs `authorize_company(run actor)`;
     - parses the kind from the contract name;
     - returns `ResourceStatus{kind: "provisioning", supported_contracts: both names, authorized,
       Ready}` and `CurrentActionPolicy{tool: builtin_provisioning_snapshot(company, kind),
       approval_required: false}`. This mirrors `MessageActionAuthority` (`message.rs:30-49`).
   - `ProvisioningRuntime::new(persistence, lease, catalogue, refused_kinds)` composes
     `PostgresActionDispatch::new(persistence, ProvisioningActionAuthority, ProvisioningEffects)`.
   - `infra::workflow` builds it beside `MessageRuntime` and passes it through
     `AgentActionTools::with_provisioning`. Without it, a provisioning target fails closed (like
     workflow tools, WF-C001 r2).
2. **Refusals: committed typed errors under a savepoint.**
   - `ProvisioningEffects::apply(tx, action)`:
     1. derives the requester;
     2. opens a savepoint (`tx.begin()`, nested);
     3. runs `provision_on` / `list_on`.
   - `ProvisioningRefusal` values roll back to the savepoint and return `Ok(tool_error(code,
     message))`. The marker and receipt then commit with **no effect**, and a replay returns the
     same refusal. A corrected call is a new invocation.
   - Refusals:
     - `Forbidden`: no qualifying requester, or one who is not a current owner/admin.
     - `Conflict`:
       - the agent slug or channel slug/alias is taken, found by a pre-check under the company
         lock;
       - **or** a unique violation on the agents slug or `channel_slugs` constraints, matched by
         constraint name. This is the race with agent or channel creation that does not take the
         company lock.
     - `Invalid(message)`: input schema, normalize/validate, model selection, skill or tool
       resolution, or a template compile or publication diagnostic.
   - Every other error (database or internal) is returned as `Err`. That rolls back the whole
     action transaction, and the existing lease and attempt path handles it as for any local
     action. No marker survives.
3. **The read tool** uses the same local path. Its receipt stores the listing, so a replay shows
   the model what it saw, and `list_on` writes nothing.
4. **Reconciliation.**
   - A provisioning marker always commits with its receipt (`effect_kind='local'`), so
     `PossibleDispatchExists` cannot arise and no provisioning invocation becomes
     reconciliation-eligible.
   - `ProvisioningActionAuthority` does not implement `SqlReconciliationResources`; no
     reconciliation composition accepts `provisioning`, so a request naming one fails closed.

## D2.3 `provision_on`: one transaction, every bypassed check named (F038)

`provision_on(tx, Provisioning{company, requester, agent: New(AgentWrite) | Existing(slug),
channel: {slug, name}, reply_mode, identity: ProvisioningKey})` runs on the caller's transaction:
the action transaction (tool) or the UI command transaction (D2.4). The company lock is already
held on both paths.

1. **Authority.** `authorize_company(tx, company, requester)`.
2. **Pre-checks** under the company lock: the agent slug (New) and the channel slug are free
   among company agents and `channel_slugs`, including aliases. Otherwise `Conflict`.
3. **Agent (New).** The same validations as `AgentUseCases::prepared_agent_write`, extracted
   into shared functions that both use:
   - `AgentWrite::normalize()`;
   - `validate_response_contract` (the configured validator, injected into the runtime);
   - **`model_allowed(connections, provider, model)`**, the pure core of
     `validate_model_selection`, over the new tx-scoped `list_model_connections_on(tx,
     company)`. Provider and model are both required here (the 06C obligation).
   - Then `insert_agent_on` and `create_agent_principal_on`, followed by the skill and tool
     attachments the agent create path writes.
   - No personal channel is written. P08: agent creation creates an agent only.
4. **Channel.**
   - The new `insert_channel_on(tx, company, id, owner_agent: Option<Uuid>, &ChannelWrite)` is
     extracted from the duplicated inserts of `create_with_agent` and
     `create_owned_agent_channel` (`channel.rs:388-460`, `:684-…`). It writes the `channels`
     row, `insert_channel_slugs`, `write_channel_email_binding` and
     `insert_email_allowlist_grants`. Both existing callers switch to it.
   - Provisioning builds the `ChannelWrite` from the company channel defaults (as
     `personal_channel_write` does) with:
     - `participant_emails = None`, which `channel_access` maps to **`team`** (never public,
       P08-Q4);
     - no owner agent;
     - `created_by = user(requester)`.
   - It runs `ChannelWrite::normalize_with(SuppliedByCaller)`.
   - `channel_agents` gets the agent at position 0 until B4 retires D-4's legacy constraint.
5. **Workflow.**
   - The template (`autonomous-response` or `reviewed-support`, D2.5) is decoded. Its `agent.run`
     step's `agent` literal is replaced structurally with the agent's slug, and its `workflow_id`
     is rebased (`rebase_workflow_id`).
   - Compilation reuses the extracted `DefinitionService::compile` core,
     `compile_bundle(decoded, draft, version, captured, refused_kinds)`. It runs `freeze`, the
     workflow identity check and the refused-kind check; `compile` keeps calling it.
   - `captured` comes from the new **`capture_agents_on(tx, actor = requester, draft)`**,
     factored from `AgentPublicationDirectory::capture`. It uses `derive_on` and `granted_tool`
     on the same transaction, so the new agent row is visible.
   - The templates have no HTTP, MCP or child steps. A template that did would be refused,
     because only the agent directory runs in-transaction.
   - Persistence:
     - the existing tx-scoped `insert_draft` (origin = template id and revision);
     - then the new **`publish_on(tx, &PreparedPublication)`**, the body of `publish` after its
       company authorization. `publish` keeps calling it. It covers:
       - the publication-key replay;
       - the draft comparison;
       - **`supported_publication(…, ToolCheck::Publish)`**;
       - **`agent_publication::verify_on`**;
       - **`verify_children`**;
       - `insert_version`;
       - the `published` event (actor = requester).
6. **Binding.**
   - The new `lifecycle::provisioned_binding(target, association, run_as = requester, params,
     message_trigger = direct, bundle) -> PreparedBinding` builds the binding through the same
     checks `BindingService::configure` runs:
     - **`ConfiguredBinding::new`** (params and slots);
     - **`check_message_trigger`**;
     - the application-side `check_readiness`, which is trivially satisfied: the templates
       declare no resources, and a template that declared any is refused.
   - It writes **two changes through the new `save_binding_on(tx, &PreparedBinding)`**, the body
     of `save_binding` without its own transaction, which `save_binding` keeps calling:
     1. *configure* (inactive, revision 1);
     2. *activate*.
   - These are the same `validate_change`, `verify_selection`, `resources::check_readiness`,
     `check_run_as`, message-binding cap, `write_head`, `insert_configuration` and events as two
     ordinary calls.
   - The activation checks `check_bundle` and "selectable content hash equals bound hash" hold by
     construction (the version was written in this transaction), and `verify_selection` re-reads
     it.
   - Params: `{}` for autonomous; `{reviewer: requester}` for reviewed.
7. **Identity.**
   - Every id derives as `UUIDv8(SHA-256("p08.provisioning" ‖ company ‖ key ‖ role))`:
     - roles: agent, principal, channel, workflow, version, publication key and binding;
     - `key` is the action invocation id for the tool, and `(company, command_key)` for the UI.

**Why still one transaction:** no partial agent-without-channel or channel-without-binding state
exists to recover. The services bypassed are replaced by their own checks, extracted rather than
copied (step list above), under the same company lock. Template compilation is bounded inside
the `WriteBound` attempt; the templates are small (F045.3).

## D2.4 "Create assistant channel" (UI/API) (F041)

- **Route.** `POST /api/companies/{company}/assistant-channels` with `{agent_slug, channel_slug,
  channel_name, reply_mode, command_key}`, plus the matching action on the agent page.
- **Authority.** Owner/admin only. The caller is the requester. The route is authenticated.
- **Command record.** New table `workflow_provisioning_commands (company_id, command_key text
  1..128, request_digest text, result jsonb, actor_id, created_at)`, primary key `(company_id,
  command_key)`. The init migration is edited in place.
- **One transaction:**
  1. company lock (`authorize_company(caller)`);
  2. **read the command record first.** Equal digest → return its stored `result` (replay). A
     different digest → 409. The result is never rebuilt from mutable rows.
  3. `provision_on` with `Existing(agent)`;
  4. insert the record with the result.
- **Concurrency.** Two concurrent commands with one key serialize on the company lock; the second
  replays.
- **Refusal mapping:** `Forbidden` → 404 (the route's not-found convention); `Conflict` → 409;
  `Invalid` → 422 with the diagnostic.
- An existing agent without an explicit provider/model is `Invalid` ("choose the agent's model
  first"), because `derive_on` refuses it.

## D2.5 Templates and `/run/deadline` (F045)

- **Shipped catalogue (D-5).** `TemplateCatalogue::shipped()` holds the seed templates:
  - `autonomous-response`: `agent.run` → `message.reply`;
  - `reviewed-support`: `agent.run` → `decision.human` → `message.reply`;
  - the remaining fixtures, offered for copy (B3b route).
- **`/run/deadline`** is the run's own admission deadline, which is immutable and inherited by
  children (`admission_write.rs`).
  - **Canonical rendering:** UTC RFC3339 with microsecond precision and a `Z` suffix
    (`to_rfc3339_opts(SecondsFormat::Micros, true)`). That is the precision Postgres stores, so
    replayed step inputs digest identically.
  - The compiler gives it schema `{type: string, format: date-time}`.
  - It is documented beside `/run/id` in `docs/workflow-language-v1.md`.
  - `reviewed-support` uses `deadline: {ref: /run/deadline}`; its parameters are `{reviewer}`.
  - Waits are already capped at the run deadline (`waits.rs:219`).

## D2.6 Agent Builder (F044)

- **Library definition:**
  - grants `["list_company_agents", "create_agent_channel"]`;
  - the prompt is revised:
    - list existing agents first;
    - propose an explicit provider/model from `models`;
    - ask for autonomous or reviewed replies, explaining the difference;
    - say the channel is team-only;
    - after explicit confirmation, call `create_agent_channel` once;
    - report the address and binding.
- **Library copy.**
  - `library_agent_write` carries the definition's `workflow_tools`; today it writes `None`.
  - When the definition grants workflow tools and its own model is unavailable or absent, the
    copy writes an **explicit** selection: the company's default connection (`is_default`, with
    a key), using its first enabled model. A warning names the choice.
  - If the company has no usable default, the copy keeps inheriting, with the existing warning,
    and D2.4 refuses it with the "choose the agent's model" message.
  - Definitions without workflow tools keep today's behaviour.
- **Reaching the Builder.** An admin runs D2.4 for the company Builder. Install-time setup and
  onboarding remain B4.

## Invariants

1. Every provisioning effect commits with its action marker and receipt in one transaction. A
   replay returns the receipt; nothing is created twice.
2. A refusal commits a typed-error receipt and no effect.
3. A created assistant channel is `team` and has exactly one active binding of a template-derived
   workflow whose agent is the created or selected agent.
4. Provisioning, including listing, requires a current owner/admin run actor **and** a current
   owner/admin requester:
   - authenticated in-app for message roots;
   - the root actor for manual and scheduled roots;
   - the root's requester for children.
5. Everything provisioned is attributed to the requester and runs as the requester.
6. Built-in names cannot be registered as HTTP operations, workflow tools or `tool.call`
   literals. A name-only snapshot under them must be the trusted built-in. Created agents cannot
   be granted provisioning.
7. The provisioned bundle, agent, channel and binding passed the same checks as the definition,
   agent, channel and binding services, through shared code.
8. `/run/deadline` is the run's own deadline in one canonical rendering.

## Failure modes

| Failure | Tool | UI |
| --- | --- | --- |
| Requester not qualifying, or not owner/admin | committed `provisioning_forbidden`, no effect | 404 |
| Slug taken (pre-check) or a unique-violation race | committed `provisioning_conflict`, no effect | 409 |
| Invalid input, model not enabled, template/publication diagnostic | committed `invalid_input`, no effect | 422 |
| Database or internal error | `Err`; whole action transaction rolled back; lease/attempt path | 5xx; nothing written |
| Crash after commit, before observation | replay returns the receipt | retry with the key replays |
| Same UI key, different request | — | 409 from the stored digest |

## Affected contracts

- `ToolKind` (+`Provisioning`, name-only), `AgentToolTarget` (+`Provisioning`),
  `AgentActionTools::with_provisioning`;
- the six trusted-check sites (D2.1);
- reserved-name refusal at HTTP registration (application and persistence), at workflow-tool
  registration and in `registry/actions.rs`;
- the first production `SqlLocalAction` and `ProvisioningActionAuthority`;
- extractions:
  - `save_binding_on`, `publish_on`, `compile_bundle`, `capture_agents_on`,
    `insert_channel_on`, `model_allowed`, `list_model_connections_on`;
  - `lifecycle::provisioned_binding`;
- new table `workflow_provisioning_commands`;
- compiler reference roots (`run.deadline`) and the runtime context;
- the `reviewed-support` fixture and its tests;
- `builtin_agent_library`, `library_agent_write`;
- `docs/workflow-language-v1.md`.

## Verification plan (B3a)

- **DB tests, tool path, through the production engine with a scripted provider:**
  - `list_company_agents`, then `create_agent_channel`, from a signed-in admin's message to the
    Builder channel:
    - agent, team channel, version, active binding (run-as = requester, events by the requester)
      and receipt in one commit;
    - replaying the invocation creates nothing.
  - **F036:** the Builder runs as the owner, the requester is an admin, and the binding's run-as
    and attribution are the admin.
  - **F037:**
    - the same request by an admin over email (origin `external`) → forbidden, nothing written;
    - signed-in admin → accepted;
    - a plain member's in-app message → forbidden.
  - **F039:** a member's `list_company_agents` → forbidden, with no listing in the receipt.
  - **Requester per root kind:** manual root (actor), scheduled root (run-as), and a child of a
    message root (root's author).
  - **Refusals:**
    - slug taken;
    - model not enabled;
    - `tools` naming a provisioning tool;
    - reserved names refused at HTTP and workflow-tool registration and in `tool.call`;
    - an untrusted name-only snapshot under a reserved name refused at publication;
    - an MCP tool with a reserved name stays a slot tool.
  - **Reply modes:**
    - the created autonomous channel answers an inbound message end to end;
    - the reviewed one parks a decision whose deadline equals the run's.
- **Competing claimants (F043, AGENTS.md):**
  - the same provisioning invocation dispatched concurrently by a stale and a current worker →
    one effect, one receipt, the stale one refused;
  - two concurrent UI commands with one key → one creation and one replay;
  - the tool and a UI command (and, separately, a plain channel creation without the company
    lock) racing for one channel slug → one success and one deterministic `provisioning_conflict`
    / 409, with no partial rows.
- **Crash** between the local commit and the observation → replay.
- **UI:** replay; same key with a different request → 409; existing agent without a model → 422.
- **F044:** library Builder → company copy (grants, explicit model) → D2.4 → the tool reachable
  in the created channel.
- **Unit:** compiler `/run/deadline` (schema, canonical rendering); Builder definition (grants,
  prompt clauses); `model_allowed`.
- **Regression:** the existing binding, definition-publish, agent-create and channel-create tests
  on the extracted functions.
- **Gates:** clippy, fmt, fresh migration of `workflow08_test`, stack budget on the provisioning
  path.
