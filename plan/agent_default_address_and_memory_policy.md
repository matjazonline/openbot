# Give every created agent an address and make memory policy explicit

## Goal

Every company agent must be reachable when its creation commits. Creating a standalone agent
creates an enabled, agent-owned personal channel in the same transaction. Creating an agent as
part of a new channel creates both rows and their position-0 assignment in the same transaction.
No endpoint may persist an agent merely as an intermediate step in an unsaved channel form.

At the same time, memory configuration must separate infrastructure, agent policy, and channel
permission:

- Company settings select and provision the one memory provider used by that company.
- Every memory scope for work in that company uses that company provider and remote database.
- Agent memory uses an `agent_<agent id>` namespace in that company database.
- User memory uses a stable hashed-email namespace in that company database. It never follows the
  user into another company and never uses a user-selected provider.
- Agent settings decide whether the agent may use memory, its recall mode and result limit, and how
  persisted facts are extracted.
- Channel settings grant read and write permission independently for company, agent, and user
  memory.
- Enabling a channel's user-memory read or write permission applies to both company members and
  authorized external participants. There is no separate external-memory switch.

This is a clean schema reset. Edit the squashed init schema directly, recreate the development and
test databases, and do not add compatibility fields, backfills, transitional reads, or a follow-up
migration.

## Product decisions

| Question | Decision |
|---|---|
| Standalone agent creation | Atomically create the agent, owned channel, primary slug, and position-0 assignment. |
| Channel-first agent creation | Atomically create the agent and the channel being requested; that channel is its reachable address but remains a user-created standalone channel. |
| Inline agents | Remove non-atomic inline creation. Advanced channel forms select an existing agent or link to the Agents workspace. |
| Agent-spawning tool | Atomically create an owned personal channel and retain its existing task/request idempotency. |
| Agent/channel slug collision | Reject the operation with the full address in the error; write nothing. |
| Rename | Atomically update the agent slug and owned channel primary address; preserve the old primary as an alias. |
| Owned-channel position 0 | Must always be the owner agent, even while the channel is disabled. |
| Disable owned channel | Allowed. It remains owned and assigned but stops accepting traffic. |
| Delete owned channel directly | Refused. Delete the owning agent instead. |
| Delete owning agent | Cascade-delete its owned channel and channel data. Enabled standalone channels where the agent is position 0 still block deletion. |
| Library agents | No owned channel because they have no company. When assigned to a company channel, their memory uses that channel's company provider and an agent-ID namespace within it. |
| Company channel defaults | Participants/access, `add_3rd_party`, and six memory grants. Personal channels are always initially enabled. |
| Missing/unready memory provider | Store the requested grants and allow creation. Runtime effective access remains off until the provider is ready. |
| Public default without spam scanning | Reject the defaults save. If scanning becomes unavailable later, create a team-only personal channel and return a visible warning. |

## Memory authorization model

Memory grants are permissions, not provider configuration. Do not clamp stored channel grants based
on current provider readiness and do not reject channel or agent writes merely because the provider
is absent, provisioning, failed, or unavailable.

For each recall or persistence target, effective access requires all of the following:

1. The channel's company has a selected provider, a ready binding, and a provider implementation
   configured in this process.
2. The executing agent has `memory_enabled = true`.
3. The channel grants the operation for the requested scope:
   `retrieve_<scope>_memory` for recall or `persist_<scope>_memory` for persistence.
4. The scope identity is available: an agent ID for agent memory and a normalized sender email for
   user memory.
5. The agent's operation policy permits the request: recall uses its recall mode and result bound;
   persistence uses its persistence mode and extraction rules.

The scope-specific opt-in is represented as follows:

- Company scope: selecting the company provider opts the company into memory infrastructure; the
  channel grant decides whether this conversation may read or write the company scope.
- Agent scope: `agents.memory_enabled` is the agent's opt-in and master memory policy switch; the
  channel agent-scope grant is additionally required.
- User scope: the channel's user-memory grant authorizes tenant-scoped memory for the current
  sender. It applies equally to members and authorized external senders. The normalized email is
  hashed before becoming a collection identifier.

`memory_enabled` defaults to `FALSE`: memory remains explicit even if a company default grants a
scope. The existing `memory_recall_mode`, `memory_max_results`, and `memory_persistence_mode` remain
agent policy. When `memory_enabled` is false, the coordinator performs no recall or persistence for
any scope.

Keep the existing protection that company-memory recall is not disclosed to an external audience.
User-memory recall for an external sender is allowed when that sender is authorized on the channel,
the agent is memory-enabled, and `retrieve_user_memory` is granted. User-memory persistence follows
the equivalent write rule. All authorization/membership lookup errors propagate; they do not become
"not a member" defaults.

The existing provider layout already supports tenant isolation: one remote database per company,
with `company`, `agent_<uuid>`, and hashed `user_<digest>` collections. Preserve that shape. The
same email may produce the same PII-free collection label in two companies, but the remote database
IDs differ, so no data crosses the company boundary.

## Schema

Edit `migrations/20260817000000_init_schema.sql` in place.

### Company defaults

Add these columns to `companies`:

```sql
default_add_3rd_party BOOLEAN NOT NULL DEFAULT TRUE,
default_participant_emails CITEXT[],
default_retrieve_company_memory BOOLEAN NOT NULL DEFAULT FALSE,
default_retrieve_agent_memory   BOOLEAN NOT NULL DEFAULT FALSE,
default_retrieve_user_memory    BOOLEAN NOT NULL DEFAULT FALSE,
default_persist_company_memory  BOOLEAN NOT NULL DEFAULT FALSE,
default_persist_agent_memory    BOOLEAN NOT NULL DEFAULT FALSE,
default_persist_user_memory     BOOLEAN NOT NULL DEFAULT FALSE,
CONSTRAINT companies_default_participants_bounded CHECK (
    default_participant_emails IS NULL
    OR (
        cardinality(default_participant_emails) <= 64
        AND array_position(default_participant_emails, NULL) IS NULL
        AND array_position(default_participant_emails, ''::citext) IS NULL
    )
)
```

Do not store a default `access_mode`; derive it from participants through the same rule used for
ordinary channels: absent/empty means `team`, `@public` means `public`, otherwise `allowlist`.

Represent these fields in the domain as `CompanyChannelDefaults`. Implement `Default` manually so
`add_3rd_party` is true, participants are absent, and all memory grants are false. Use
`EmailAddress` rather than bare `String` for domain participant values; database row structs retain
`Vec<String>`.

Normalize and validate defaults at the application boundary:

- trim and lowercase participant entries;
- remove blanks and duplicates;
- accept valid email-shaped entries and the `@public` sentinel;
- reject more than 64 entries with `BadRequest` before persistence; and
- reject `@public` while spam scanning is disabled.

### Agent memory policy

Add to `agents`:

```sql
memory_enabled BOOLEAN NOT NULL DEFAULT FALSE
```

Carry it through `Agent`, `AgentWrite`, all agent queries, inserts, updates, forms, JSON payloads,
tool-created agents, and fixtures. It is policy, not a provider selector; no provider field is added
to agents or users.

### Owned channels

Add to `channels`:

```sql
owner_agent_id UUID,
CONSTRAINT channels_owner_agent_key UNIQUE (owner_agent_id),
CONSTRAINT channels_owner_agent_fk
    FOREIGN KEY (company_id, owner_agent_id)
    REFERENCES agents(company_id, id) ON DELETE CASCADE
```

`NULL` means a user-created standalone channel. A non-NULL value means the channel is the personal
address of that agent. The composite FK carries tenancy and prevents a library agent from owning a
company channel.

Add a `DEFERRABLE INITIALLY DEFERRED` constraint trigger over `channels.owner_agent_id` and
`channel_agents` inserts, updates, and deletes. At commit, every channel with an owner must have
that exact agent at position 0, regardless of `enabled`. Keep the existing enabled-channel trigger:
it independently requires every enabled channel to have some position-0 agent.

Prevent direct deletion of an owned channel while its owner row still exists. The database guard
must permit the channel deletion caused by `ON DELETE CASCADE` after deleting the owner. Exercise
both paths against PostgreSQL rather than relying on trigger-order reasoning alone.

Add `owner_agent_id: Option<Uuid>` to `Channel`, `ChannelDb`, `CHANNEL_SELECT`, inserts and updates.
All runtime query column orders and bind orders must be checked manually.

## Application ports and transaction boundaries

### Owned-agent lifecycle port

Define a narrow application-owned port for state that must commit together rather than adding an
optional correctness method to a broad persistence trait:

```rust
#[async_trait]
pub trait OwnedAgentChannelPersistence: Send + Sync {
    async fn create_owned_agent_channel(
        &self,
        company_id: Uuid,
        agent: AgentWrite,
        channel: ChannelWrite,
    ) -> AppResult<(Agent, Channel)>;

    async fn update_agent_and_owned_address(
        &self,
        agent_id: Uuid,
        write: AgentWrite,
    ) -> AppResult<Agent>;
}
```

The production implementation is required; do not provide a silently successful default. Test
doubles explicitly implement the methods they exercise.

`create_owned_agent_channel` performs one transaction:

1. Insert the agent.
2. Insert the channel with `owner_agent_id = agent.id`.
3. Insert its primary `channel_slugs` row.
4. Insert participants.
5. Insert the owner at `channel_agents.position = 0`.
6. Commit only after all uniqueness, tenancy, and deferred ownership checks pass.

Keep the existing channel-first `create_with_agent` path, but name its lifecycle explicitly as
standalone and leave `owner_agent_id = NULL`. Share a private persistence helper where useful; do
not duplicate the positional SQL and bind list.

### Standalone agent provisioning

Make the standalone command explicit, for example:

```rust
pub async fn create_addressable_agent(
    &self,
    user_id: Uuid,
    company_id: Uuid,
    agent: AgentWrite,
) -> AppResult<ProvisionedAgent>
```

It must retain every rule currently enforced by `AgentUseCases::create_agent`:

- verify the caller is a company manager;
- set user creation provenance on agent and channel;
- normalize and validate the agent;
- validate provider/model selection against company model connections;
- read `CompanyChannelDefaults` from the already-authorized company;
- derive the effective personal channel write; and
- call the atomic owned-agent lifecycle port.

Do not route standalone agent creation directly through `ChannelUseCases` if that bypasses agent
model validation or provenance.

Return the effective channel and zero or more structured warnings alongside the agent. HTML routes
show warnings in the saved pane; JSON and tool results expose them as data.

### Pure personal-channel derivation

Keep the mapping pure:

```rust
pub(crate) fn personal_channel_write(
    agent: &AgentWrite,
    defaults: &CompanyChannelDefaults,
    spam_scanning: SpamScanning,
) -> PersonalChannelDecision
```

`PersonalChannelDecision` contains the `ChannelWrite` plus warnings. Name, slug and description
come from the agent; the owner channel starts enabled. Copy all company channel defaults without
consulting memory readiness.

If `@public` somehow exists while spam scanning is unavailable, remove only `@public`, preserve
explicit participants, derive the resulting access mode normally, and add a warning saying that
the personal channel was created without public access. This is a runtime safety fallback, not the
normal settings-save path.

### Atomic rename

`update_agent_and_owned_address` loads and updates the agent and owned channel in one transaction.
When the slug changes:

1. Validate the new agent write and model selection before entering persistence.
2. Lock the agent and owned channel/address rows needed for the update.
3. If no owned channel exists, update only the agent; this covers channel-first and library-agent
   semantics.
4. If the new slug is already primary, treat it as an idempotent no-op for the address.
5. Otherwise demote the old primary to an alias and promote/insert the new primary.
6. Map address uniqueness failures to a `BadRequest` naming the full address.
7. Commit the agent update and address change together.

A collision rolls back the agent update. Never use the former two-transaction "update then
resync" design; retrying that design can permanently skip resynchronization after the agent row has
already changed.

### Delete and owned-channel mutations

Agent deletion continues through `AgentPersistence::delete`:

- its owned channel cascades in the same statement;
- enabled standalone channels where it is position 0 trigger a conflict and roll the whole delete
  back;
- disabled-channel and non-position-0 assignments are removed by the existing FK cascade; and
- library-agent deletion keeps its existing in-use guard.

Classify the UI impact into owned channel deletion, enabled position-0 blockers, and other
assignments that will merely be removed. Confirmation text must state all destructive effects and
be escaped for the `hx-confirm` attribute context.

For an owned channel, channel settings may edit:

- enabled state, including disabling it;
- aliases;
- participant/access settings;
- `add_3rd_party`;
- the six memory grants; and
- channel display name/description.

The primary slug is read-only with a link to rename the agent. The position-0 agent is read-only
and must remain the owner; future additional pipeline agents may occupy positions after zero.
Direct channel deletion returns `Conflict` with instructions to delete the owning agent.

## Remove non-atomic inline agent creation

Delete the legacy `POST /companies/{company_id}/agents/inline` endpoint, its `InlineAgentForm`, and
the embedded "Create New Agent Inline" UI. Replace it with a link to the Agents workspace.

Remove the side effect from `resolve_channel_agents`: it may parse/select existing IDs but must not
persist an agent. Specifically:

- New `/ui/channels` Simple creation continues to build an `AgentWrite` and call the existing atomic
  channel-plus-agent transaction.
- Advanced channel creation selects an existing company or library agent.
- Channel updates select existing agents only.
- JSON channel creation with a supplied system prompt may retain its atomic
  `create_channel_with_agent` behavior.
- No failed or abandoned channel form can leave an unassigned agent behind.

## Agent-spawning tool

The tool-created agent/channel pair is owned: set `owner_agent_id` and enforce the same position-0
invariant. Derive its channel from the company defaults through the same pure function so user and
tool provisioning do not drift.

Preserve the tool's advisory-lock and `agent_channel_provisions` ledger. The stable request hash
continues to represent the logical tool request, not mutable ambient company defaults; a retry of
the same task/request returns the originally provisioned rows even if defaults changed afterward.
The initial transaction stores the effective defaults snapshot on the channel.

## Memory coordinator changes

Remove `ChannelUseCases::check_memory_interlock` from channel create and update. Keep a readiness
query for display and diagnostics, but change the UI from disabled controls to an inactive-state
notice: permissions can be configured before a provider becomes ready.

Before resolving scopes, `MemoryCoordinator` requires a concrete agent with
`memory_enabled = true`. If absent or disabled, return no recalled context and a skipped persistence
report without contacting the provider.

Continue to resolve the active binding by `company.id` once per operation. Then:

- company scope uses collection `company`;
- agent scope uses `agent_<executing agent id>`;
- user scope uses the existing normalized, hashed sender-email collection for members and external
  participants alike; and
- library agents use their library UUID for the agent collection inside each invoking company's
  database.

Authorization must happen before memory recall/persistence is invoked. An external sender only
reaches user memory after the inbound channel authorization path has accepted that sender. Do not
let a guessed sender address query memory through a preview or simulation endpoint without the same
authorization decision.

Keep best-effort persistence behavior and the existing bounds, truncation and metrics. Add skip
metrics that distinguish `agent_policy_disabled`, `provider_unavailable`, `scope_identity_missing`,
and `channel_grant_absent` without logging memory contents or raw user addresses.

## UI and API

### Agents

- Add `memory_enabled` to company-agent create/edit forms and JSON payloads.
- Keep recall mode, maximum results, and persistence behavior under Agent memory policy; do not show
  a provider picker.
- Show a live full-address preview on standalone creation using
  `Channel::address_for` and the configured application domain.
- Show the owned address in the agent list and pane header. Load company channels once and build an
  owner-agent map; do not issue one channel query per agent.
- For channel-first legacy agents with no owned channel, show the actual channel address when there
  is one unambiguous position-0 channel, otherwise show the handle without claiming it is an
  address.
- Rename failures preserve the submitted draft while displaying the collision error; the stored
  agent and channel remain unchanged.

### Company settings

Add a "New agent channel defaults" section containing:

- participant list/access;
- `add_3rd_party`; and
- read/write grants for company, agent, and user memory.

User-memory copy must explicitly say it includes authorized external participants and remains
isolated to this company. Provider selection remains in the company memory infrastructure section.
Do not disable grant controls while provisioning is pending; show why they are currently inactive.

Wire the fields through `/ui` forms, legacy company forms, JSON payloads, drafts, use cases,
persistence, and all company reads. Because this is a clean break, all first-party clients are
updated together rather than carrying old payload behavior.

### Channels

- Keep six independent scope/action grants.
- Explain that user memory includes authorized external senders.
- Show provider readiness as effective-state information, not as permission to edit.
- For owned channels, render primary address and position-0 agent as read-only and remove the direct
  delete action; disabling remains available.

All names, addresses, warnings and confirmation messages use the correct text or attribute escaping
for their output context.

## Files expected to change

| Area | Files |
|---|---|
| Schema | `migrations/20260817000000_init_schema.sql` |
| Domain | `src/domain/entities/{company,agent,channel,memory}.rs` |
| Agent lifecycle | `src/application/use_cases/agent.rs`, a narrow owned-channel port/module, `src/adapters/persistence/{agent,channel}.rs` |
| Channel policy | `src/application/use_cases/channel.rs` |
| Memory runtime | `src/application/services/memory_coordinator.rs`, relevant memory tests and construction wiring |
| Tool provisioning | `src/application/services/agent_channel_tool.rs`, `src/adapters/persistence/agent_channel.rs` |
| Agent routes/UI | `src/adapters/http/routes/{ui_agents,agent}.rs`, `src/adapters/http/pages/agent_settings.rs` |
| Channel routes/UI | `src/adapters/http/routes/{ui_channels,channel}.rs`, `src/adapters/http/pages/{channel_settings,channels}.rs` |
| Company routes/UI | `src/application/use_cases/company.rs`, `src/adapters/persistence/company.rs`, `src/adapters/http/routes/{ui_companies,company}.rs`, `src/adapters/http/pages/company_settings.rs` |
| Wiring and fixtures | application startup wiring, mocks, entity literals, HTTP/page tests, SQLx metadata |

The list is directional, not exhaustive. Find every `impl` of the changed ports and every `Company`,
`Agent`, and `Channel` literal before declaring the work complete.

## Verification

### Pure policy tests

- `CompanyChannelDefaults::default()` matches SQL defaults exactly.
- Participant normalization handles blanks, duplicates, case, `@public`, invalid entries, and the
  64-entry bound.
- Personal-channel derivation copies all defaults, never clamps memory grants, and returns a warning
  when it must remove `@public`.
- Effective memory access is an exhaustive matrix over provider readiness, agent
  `memory_enabled`, channel read/write grants, scope identity, and member/external audience.
- Agent-disabled policy prevents all provider calls.

### Database tests

- Owned provisioning writes agent, owner channel, primary slug, participants, defaults, and
  position 0 atomically.
- Competing provisions for the same company/slug produce one winner and one address-specific
  `BadRequest`; the loser leaves no agent row.
- Cross-company `owner_agent_id` is rejected.
- A library agent cannot own a company channel.
- Replacing/removing position 0 from an owned channel is rejected even when disabled.
- An owned channel can be disabled and re-enabled without losing ownership or assignment.
- Direct owned-channel deletion is rejected; deleting its owner cascades successfully.
- Deleting an owner that is also position 0 on another enabled channel conflicts and rolls back all
  cascades.
- Atomic rename keeps both old and new addresses, moves `is_primary`, and is idempotent.
- Rename onto a taken address rolls back both the agent and channel changes.
- Tool provisioning remains idempotent under competing identical requests and sets ownership.
- Company A and company B use different remote database IDs for the same normalized user email.
- A library agent's memory namespace is isolated by the invoking company database.

### Route and UI tests

- Standalone `/ui`, legacy HTML, and JSON agent creation all return an addressable agent.
- The inline-agent endpoint and UI no longer exist.
- Simple channel and JSON prompt creation remain atomic channel-first operations.
- Failed channel creation leaves no agent.
- Agent address preview and saved full address use the configured application domain.
- Owned channel UI locks primary slug, position 0, and delete while allowing disable and permission
  edits.
- Company and channel copy states that user memory includes authorized external participants.
- Memory grants can be saved without a ready provider and become effective when readiness changes.
- Public defaults are rejected without spam scanning; the runtime fallback warning is visible and
  represented in JSON/tool results.
- Hostile names and addresses are safe in text, attributes, and deletion confirmations.

### Clean database and build loop

Recreate both databases because the squashed schema is intentionally edited in place. Then run the
schema, inspect the changed tables, and regenerate SQLx metadata:

```sh
DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents" cargo sqlx database reset -y
DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents_test" cargo sqlx database reset -y
psql "postgres://$(whoami)@localhost:5432/mail_agents" -X \
  -c '\d+ companies' -c '\d+ agents' -c '\d+ channels' -c '\d+ channel_agents'
DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents" \
  cargo sqlx prepare -- --all-targets
```

Run the repository gates, including offline compilation and the database-backed suite:

```sh
cargo fmt --check
SQLX_OFFLINE=true cargo check --locked --all-targets
cargo clippy --all-targets -- -D warnings
DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents" cargo test --locked --all-targets
DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents_test" scripts/stack-budget.sh
git diff --check
```

Do not raise a stack, collection, or payload bound to clear a failure. If any bound is approached,
keep or add the early-failing test/CI signal required by the repository rules.

## Out of scope

- Cross-company or portable personal memory.
- User-selected memory providers.
- A separate external-participant-memory switch; channel user-memory grants cover authorized
  members and external senders together.
- Executing a multi-agent pipeline. Storage ordering and the owned position-0 invariant remain
  compatible with future agents at later positions.
- Backfills, rolling-deploy compatibility, old durable payload support, and follow-up migrations.
