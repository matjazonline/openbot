# 03PA-P01 D1 inventory (Explore agent, read-only, 2026-11-14, HEAD cb46c6fd)

The full blast radius is below, grouped under your six headings. Paths are relative to `/Users/mac02/.gui-cloudroom/plugins/environment-git-worktree/host-data/worktrees/thr_jt43h4qc8d-1/openbot`. Nothing was edited.

**Stale line references in the plan:**
- `channel_slugs` is at `init_schema.sql:3677`, not 3600.
- `AddressedTarget` is at `transport/ingress.rs:412`; line 429 is `AddressedRecipient`.
- `Channel::inbound_address` is at `channel.rs:229`.
- The `workflow_provisioning_commands` comment is at `init_schema.sql:5672`, not 5594.
- `Channel::address_for` already exists (`channel.rs:249`) and is used by desks only.

## 1. `channel_slugs` reads and writes

**Schema (`migrations/20260817000000_init_schema.sql`)**
- `:3677` table `(company_id, channel_id, slug citext, is_primary, created_at)`; `:3683` `channel_slugs_format` check.
- `:6111` `channel_slugs_company_slug_key UNIQUE (company_id, slug)`. This is the per-company uniqueness everything below relies on.
- `:6119` primary key `(channel_id, slug)`. `:7926` `channel_slugs_primary_idx` allows one primary per channel. `:9537` foreign key to `channels(company_id,id)` with cascade delete.
- `:13275` the only SQL function that reads it: `require_channel_reply_proposal_links()` (`:13245`). It checks that a `reply_email` sender slug equals the desk's primary slug.
- No triggers are defined on the table.

**Rust**
- `src/adapters/persistence/channel.rs`
  - `:94`, `:98` `CHANNEL_SELECT` reads the primary slug and `alias_slugs`. Every channel load uses it, including `DirectoryCache::channels` for ingress.
  - `:157` `insert_channel_slugs`, the only INSERT. It writes one row per slug; the primary goes first, then aliases.
  - `:185` `slug_conflict_error` maps error 23505 to "Address … already in use by another desk". It depends on the uniqueness constraint.
  - `:356` `create` calls the insert.
  - `:383` `get_by_company_slug_and_channel_slug` looks up by slug or alias. Called from `preview_inbound_route` (`use_cases/channel.rs:368`), which serves the simulation `verify`.
  - `:531`/`:536` `write_channel_settings_on` handles renames and aliases by DELETE all rows then re-insert. It is called from `channel_edit.rs:104` `update_channel_on`.
- `src/adapters/persistence/workflow/provisioning.rs`
  - `:565` `taken_slug` runs `EXISTS … WHERE company_id AND slug` as a pre-check. It depends on uniqueness.
  - Callers: `provision_guarded` (`:556`, re-checks after a failure), `provision_on` (`provisioning_native.rs:46`), `plain_desk_on` (`provisioning_department.rs:106`) and `taken_by_another` (`provisioning_builder.rs:243,247`).
  - `:748` `malformed` documents the `channel_slugs_format` regex.
- `src/adapters/persistence/workflow/provisioning_native.rs:71` `provision_on` inserts the slugs for an assistant or agent desk.
- `src/adapters/persistence/workflow/provisioning_department.rs:174` `insert_plain_desk_on` inserts the slugs for a department desk.
- `src/adapters/persistence/workflow/message.rs:278` `channel_route_on` reads the primary slug as the email sender. Used by `send_target_on`/`reply_target_on`.
- `src/adapters/persistence/department_roster.rs`
  - `:35` `AGENT_DESK` join: `agent:{id}` record, then channel, then primary slug.
  - `:43` `DEPARTMENT_DESK` join, the same shape for `department:{id}`.
- `src/adapters/persistence/department.rs:922` `desks()` joins primary slugs for department-owned desks.
- Comment-only references: `src/application/transport/ingress.rs:370` (the `SystemAddress` safety argument) and `src/adapters/protocols/email/selector.rs:230`.
- `use_cases/channel.rs:497` `find_similar_channel_slugs` is only a name match. It computes bounce suggestions in memory from `Channel::slugs`; caller `routing.rs:172`.

**Tests touching the table or uniqueness**
- `channel.rs:799` `alias_slugs_are_unique_within_a_company_and_free_across_companies` (uniqueness).
- `provisioning_dispatch_tests.rs:453` `provisioning_losing_an_address_race_is_a_conflict_refusal` (uniqueness). It also checks `pg_stat_activity` for `INSERT INTO channel_slugs` at `:492`.
- `provisioning_dispatch_tests.rs:176` `written` counts rows. `:1025` raw insert in `agent_default_desk_builder_creation_refusals_are_tool_errors`.
- `department_agent_desks_tests.rs:112` and `department_lead_desk_tests.rs:31` `desk_row` helpers.
- `integration/tests.rs:1103` `no_channel_has_an_email_binding_that_disagrees_with_its_primary_slug`.
- Other uniqueness-dependent tests:
  - `agent_default_desk_refuses_a_taken_address_or_slug`
  - `agent_default_desk_competing_creators_of_one_slug_make_one_agent`
  - `agent_default_desk_onboarding_taken_address_rejects_and_commits_nothing`
  - `department_lead_desk_taken_address_refuses_the_department`
  - `department_lead_desk_onboarding_address_collision_with_an_agent_is_rejected` (`onboarding/tests.rs:959`)
  - `assistant_channel_route_maps_results_and_refusals`
  - `onboarding_second_channel_error_rolls_back_every_effect`
- `.sqlx/` has no `channel_slugs` entries. `docs/system_addresses.md` mentions it.

## 2. The automatic agent desk

**Creation**
- `src/adapters/persistence/company_agent.rs`
  - `:39` `default_desk_key(agent) -> "agent:{id}"`.
  - `:71` `create_company_agent_on`. At `:94-110` it calls `assistant_channel_on` with `channel_slug = write.slug`, `ReplyMode::Autonomous` and that key.
  - `:53` `CreatedCompanyAgent.desk`; `:60` `desk_id()`.
- `src/adapters/persistence/workflow/provisioning.rs:846` `assistant_channel_on` replays or stores the result under the command key, then calls `provision_guarded`.
- `src/application/use_cases/agent.rs:139` `CreatedAgent.desk_id`; `:147` trait `CompanyAgents`.

**Callers (every creation path)**
- Agents page: `ui_agents.rs:356` `create_agent`, then `AgentUseCases::create_agent` (`use_cases/agent.rs:323`), then `CompanyAgentService::create`, then `create_on` (`company_agent.rs:210`).
- Legacy form: `routes/agent.rs:357` `create_agent_handler`.
- JSON API: `routes/agent.rs:617` `create_agent_json`.
- Library copy: `ui_agents.rs:415` `create_from_library`, then `use_cases/agent.rs:384` `create_agent_from_library`, then `copy_from_library` (`company_agent.rs:264`).
- Onboarding: `persistence/onboarding.rs:198` `Session::insert`, from `complete` (`:110`), called by route `onboarding.rs:229` `create_agents`.
- Agent Builder: `provisioning_builder.rs:95` `builder_on`, from `create_agent_channel_on` (`:15`), from `apply` (`provisioning.rs:198`).
  - `:81-86` treats `desk_slug == write.slug` as the default desk and refuses other settings there.
  - `:107` returns `created.desk` as the answer.
  - `:145` `answers_as_default_desk`.

**Consumers of `agent:{id}` or of the agent's own desk**
- `init_schema.sql:17551` `workflow_escalation_agent_route`. It joins on `command_key='agent:'||agent.id` (`:17560`), then the desk channel, the binding id from the result, and the email `channel_bindings` endpoint.
  - Used by `workflow_escalation_agent_eligible` (`:17577`) and `workflow_escalation_agent_route_matches` (`:17619`).
  - Those feed `guard_workflow_escalation_agent_receipt` (`:17632`), `workflow_escalation_target_admissible` (`:17945`) and `workflow_escalation_agent_handling_failure` (`:18246`).
  - The receipts table keeps `desk_id` and has a foreign key to `workflow_message_intents(...,channel_id)`.
- Rust consumers of that route:
  - `notification/agent_notice_route.rs:22` `read_on`; `:42` `lock_on` gates on `desk_id`.
  - `workflow/escalation_assignment.rs:13` `target_on`. `reassign_on` (`:30`) requires `target.desk` for agent reassignment.
- `init_schema.sql:5672` table comment that lists the `agent:{agent_id}` key.
- `department_roster.rs`
  - `:31` `AGENT_DESK`.
  - `:93` `agent_desks_on`, used by `list_on` = `list_company_agents` (`provisioning.rs:367`; `"desk"` field at `:448`) and by `agent_authoring/directory.rs:26` `placed` (via `entry`/`page`).
  - `:327` `roster_on` inside `department_context_on` (`:239`). Caller: `agent_history/preparation.rs:239` `candidate_on`.
  - Rendered at `application/workflow/agent_department.rs:96-102` as "Agent X (slug): address".
- `application/workflow/provisioning.rs:60` `LIST_COMPANY_AGENTS_DESCRIPTION` tells the model what `list_company_agents` returns.
- `application/agent/authoring_reads.rs:146-151` `AgentDirectoryEntry.desk` / `AgentDesk`.
- Agent settings: `pages/agent_settings.rs:360` `agent_placement_card` (`:364-371`, `#agent-default-desk`); `:448` "The agent already has its own desk"; `:99` comment.
- Onboarding:
  - `routes/onboarding.rs:276` `stored_onboarding_channel`, `:291` `onboarding_key`, `:314` `complete` reload.
  - `persistence/onboarding.rs:249` `channel()` raises an error if no desk exists.
  - `use_cases/onboarding.rs:89` `OnboardedAgent.channel`; `pages/onboarding.rs:150`.
- Builder prompt: `builtin_agent_library.rs:34`, `:41`, `:166-169`.
- Docs: `docs/departments.md:66-75`, `docs/inter_channel_agent_communication.md:67-68`.

**Tests asserting automatic desks (about 78 in total)**
- **23 named `agent_default_desk_*`:**
  - `routes/onboarding.rs:523`
  - `agent_api_tests.rs:289`
  - `company_agent_tests.rs:163` (syn guard over production creation calls)
  - 9 in `department_agent_desks_tests.rs`, at lines 138, 204, 252, 287, 335, 371, 471, 489, 530
  - 6 in `provisioning_dispatch_tests.rs`, at 934, 953, 987, 1012, 1074, 1178 (helper `default_desks` at `:920`)
  - 4 in `onboarding/tests.rs`, at 298, 808, 885, 922
  - `use_cases/agent_tests.rs:251`
- **34 escalation tests** (35 test fns in the files). Every test in `workflow/escalation_agent*_tests.rs` and `escalation_origin*_tests.rs` gets its agent from `CompanyAgentService::create` (`escalation_agent_tests.rs:50` `provision_handler`) and uses `f.created.desk_id` / `default_desk_key`. They need `workflow_escalation_agent_route` to resolve.
- **About 21 others** (approximate):
  - `department_roster_tests.rs:60`
  - `provisioning_directory_tests.rs:78`, `:231`, `:284`
  - `department_lead_desk_tests.rs:165`, `:349`, `:393`, `:623` (via `own_desk`)
  - `onboarding/tests.rs:321`, `:381`, `:959`
  - `department_ui_tests.rs:574`
  - `desk_wording_tests.rs:141`
  - `builtin_agent_library.rs:161`
  - `provisioning_runtime_tests.rs:163`, `:774`
  - `attention_authorization_tests.rs:251`
  - `skill_tests.rs:366`
  - `agent_api_tests.rs:172`
  - `application/workflow/provisioning_tests.rs:80`
  - `autonomous_agent_provisioning_tests.rs:93` (uncertain)

## 3. Address parsing and classification

- **`EmailChannelSelectorParser`** (`adapters/protocols/email/selector.rs:62`)
  - `new(app_domain)`, used at:
    - `ingress.rs:238`
    - `smtp/server.rs:631`
    - `egress.rs:384`
    - `http/pages/mod.rs:34`
    - `routes/channel/simulation.rs:247`, `:495`
    - tests at `use_cases/channel.rs:531`, `:538` and `thread/tests.rs:161`
  - `parse(&str) -> Option<EmailChannelSelection>` (`:73`). It splits on `+` and strips context suffixes.
    - Callers: `classify`, `EmailIngressAdapter::disposition_of` (`ingress.rs:332`), `SmtpServer::sender_needs_spam_scan` (`server.rs:631`), `render_email`/`internal_target` (`egress.rs:384`, `:432`), simulation `preview_recipient` (`:246`) and `simulation_recipient_matches` (`:489`).
  - `classify(EmailAddress) -> EmailRecipientDestination` (`:107`). Variants are `Channel`, `External` and `InvalidPlatformAddress` (`:54`). Caller: `ingress.rs:320`.
  - `parse_platform_address(&str) -> Option<(CompanySlug, String)>` (`:128`). Callers: `ingress.rs:312` and `pages/mod.rs:30`, which is used by `mailbox.rs:2093`.
  - Output type is `ChannelSelector` (`domain/entities/transport/mod.rs:436`: `CurrentCompany` / `Qualified`). It is serde-serialized into the outbound email `internal_target`.
- **`EmailIngressAdapter::classify(&self, &str) -> AddressedTarget`** (`ingress.rs:311`, private)
  - It matches `SystemAddress::parse` first, then parser `classify`.
  - It maps `InvalidPlatformAddress` to `Channels(vec![])` and `External` to `Outsider`.
  - Its only caller is `route` (`:284`), which is called by `accept` (`:247`).
- **`AddressedTarget`** (`application/transport/ingress.rs:412`): `Channels(Vec<ChannelSelector>)`, `System{company,address}`, `Outsider`.
  - Matched at `:466` `channel_pipelines`, `:476` `system_addresses`, `:484` `outsiders`.
  - Constructed at `ingress.rs:315-327` and at `thread/ingest/mod.rs:516` `single_channel_routing`. The latter is used by internal relay (`relay.rs:179`) and by trusted compose (`canonical.rs:57`).
  - `SystemAddress` is at `:378`.
- **Address grouping:** there is no separate module. Grouping is `ThreadUseCases::resolve_addresses` (`thread/ingest/routing.rs:131`), which iterates `channel_pipelines()`. It matches desks in memory with `Channel::matches_slug` (`:166`); an unknown slug gets `find_similar_channel_slugs`. Then `prepare_channels` (`:428`) produces the intents.
  - Tests in `thread/address_grouping_tests.rs` (mounted at `thread/tests.rs:2437`): `:88`, `:107`, `:129`, `:155`, `:172`, `:194`.
- **`Channel` address helpers** (`domain/entities/channel.rs`)
  - `slugs` `:216`, `matches_slug` `:221`.
  - `inbound_address(&CompanySlug,&str) -> EmailAddress` `:229`, `inbound_addresses` `:238`, `address_for(&ChannelSlug,&CompanySlug,&str)` `:249`.
  - `inbound_address` callers: `channel_settings.rs:204`, `channels.rs:415`, `mailbox.rs:1769`, `simulation/compose.rs:149`, `simulation/live.rs:520`, `routes/ui.rs:854`/`894`/`982`/`1022`, `canonical.rs:50`, `routing.rs:628` (`channel_was_mentioned`), `routing.rs:704`.
  - `inbound_addresses` caller: `channel_settings.rs:643`.
  - `address_for` callers: `department_roster.rs:86`, `provisioning_native.rs:91`, `provisioning_department.rs:140`, `message_planner.rs:102` (`plan_email`, the From address), `relay.rs:211` (`authorize_relay`).
- **`writable_channel_directory(&self, &Company, &QualifiedIdentity, &mut DirectoryCache) -> Vec<ChannelDirectoryEntry>`** (`routing.rs:672`). It returns team-only results, desks only, grouped by department.
  - Callers: `undeliverable_rejection` (`:641`, the bounce) and `answer_system_addresses` (`:720`, `_help`). `send_system_reply` is at `:761`.
  - Entry type is at `notices.rs:67`. Rendering: `push_channel_directory` `:149` ("Desks you can write to:"), `format_help_email_body` `:188`, `format_bounce_email_body` `:102`.
- **`RESERVED_SLUG_SUFFIXES`** (`channel.rs:21`) and `RESERVED_SUFFIX_SEPARATORS` (`:24`) are used by:
  - `strip_quiet_prefix` (`channel.rs:30`)
  - `strip_context_suffix` (`selector.rs:153`)
  - `validate_slug` (`use_cases/channel.rs:446`). That function already covers agent slugs (`use_cases/agent.rs:66`, `mcp.rs:29`), departments (`department.rs:56`) and channels and aliases (`channel.rs:46`, `:71`).
  - `format_help_email_body` (`notices.rs:228`)
  - tests `selector.rs:224` and `thread/tests.rs:3367`

## 4. External ingress paths

- **SMTP:** `SmtpServer::handle_command` RCPT (`smtp/server.rs:477-500`) collects recipients without classifying them; every syntactically valid RCPT gets a 250.
  - `finish_data_transaction` (`:536`) then calls `parse_raw_mime_to_payload(..rcpts)` at `:551`, then `self.ingress.accept` at `:573`, then `preflight_inbound` (`:578`) and `commit_prepared_inbound` (`:590`).
  - It also calls `sender_needs_spam_scan` (`:629`). That parses `raw_payload.to` and calls `ThreadUseCases::sender_trust` (`thread/mod.rs:202`), which is a desk-only `matches_slug` lookup. A principal address would always count as a stranger.
- **SendGrid:** `sendgrid_inbound_webhook` (`routes/webhooks/sendgrid.rs:36`) calls `decode_sendgrid_request` (`sendgrid/inbound.rs:60`; the envelope `to` is the routing recipient). That calls `accept` (`:93`), then `preflight_inbound` (`webhooks/sendgrid.rs:63`) and `commit_prepared_inbound` (`:84`).
- **Resend:** `resend_api_inbound_webhook` (`webhooks/resend_api.rs:105`) only stores the event; the tenant comes from the token, not the recipient. `ResendApiInboundDecoder::decode_event` (`resend_api/inbound.rs:137`) then calls `read_mail` (`:205`).
  - The recipient is `received_for.first()` or `to.first()`, at `:221`.
  - Then `accept` (`:237`) and `preflight_inbound` (`:164`); the commit request goes to the inbound-event worker (worker not traced).
- **Other `accept` callers:** simulation (`routes/channel/simulation.rs:472`, Application trust) and `thread/test_support.rs:1667`.
- **Where they converge:** all external paths go through `EmailIngressAdapter::accept` → `route` → `classify` (the per-recipient decision), then `ThreadUseCases::preflight_inbound` (`thread/ingest/mod.rs:245`) → `answer_system_addresses` → `resolve_addresses`.
- **Internal relay** skips classification. `render_email` takes `internal_target` from `parse` (`egress.rs:384`). Then `relay_internal` → `internal_inbound_message` → `single_channel_routing`. `authorize_relay` (`relay.rs:197`) requires From to equal the source desk's `address_for`.

## 5. Person principals and agent slugs

- **`create_person_principal_on(conn, company_id, user_id, display_label, email)`** (`persistence/participant.rs:330`)
  - `:358-373` promotes an existing *external* principal to `person` with an UPDATE. This path would also need to write the slug and address.
  - `:374` reuses an existing person row as is.
  - `:387` is the fresh INSERT.
- **Production callers (only two):**
  - Company creation (`persistence/company.rs:177`, `create`; also the onboarding step 1 company). The owner's `username` and `email` are read at `:145`.
  - Invite accept (`company_invite.rs:409`, `accept_pending_invite`). `username` is read at `:403`; `user_email` is a parameter.
  - All other callers are tests.
- **Membership, registration and onboarding:** no other path creates a person principal. Membership rows are only inserted at `company.rs:168` and `company_invite.rs:381`, and user registration (`persistence/user.rs:105`/`171`/`428`) creates no principal.
- **Available name:** only `users.username`, a citext with a not-blank check and no slug format (`init_schema.sql:4549-4560`). There is no display-name column, so a slug would have to be derived from it.
- **Other principal inserts** that would need a slug or a refusal:
  - `resolve_or_create_external_identity_on` (`participant.rs:270`, external)
  - `ensure_system_principal_on` (`:419`, system)
  - `account_identity.rs:133` `detach_old_address` (external)
- **Where an agent's slug is set:** `insert_agent_on` (`persistence/agent.rs:92`), from `create_company_agent_on:84`. The agent principal is created by `create_agent_principal_on` (`participant.rs:439`), which upserts `display_label` only.
- **Lazy upsert:** `resolve_author` (`thread/message.rs:337`) calls `create_agent_principal_on` with only `agent_id` and `display_label`. With `principals.slug NOT NULL` its INSERT branch would need the slug.
- **Rename:** `update_agent_on` (`agent.rs:276`; `UPDATE agents SET … slug = $2`). Called by `update_agent` (`:256`, via `AgentPersistence::update` `:398`) and by `save_company_agent` (`agent_authoring/mod.rs:28`). The HTTP routes are `routes/agent.rs:504`, `:702` and `ui_agents.rs:478`, through `AgentUseCases::update_agent` (`use_cases/agent.rs:555`).
- **A rename does not update the desk slug today.** It touches neither `channel_slugs` nor the principal. `onboarding/tests.rs:381` `onboarding_new_key_after_rename_does_not_replay_an_old_channel` asserts that the old desk keeps the old slug.

## 6. Other "desk slug == agent slug" sites

- `company_agent.rs:101-103`: the desk slug is set to the agent slug.
- `provisioning_builder.rs:77-86`: `channel_slug` defaults to the agent slug, and the "own address is its autonomous default desk" refusal.
- `provisioning_runtime_tests.rs:147` `spec()`: a reviewed desk is moved to `{slug}-agent` to avoid the collision.
- Tests that address an agent by mailing its slug:
  - `provisioning_runtime_tests.rs:239` (`builder_provisions_an_assistant_for_a_signed_in_admin`, mails "helper")
  - `provisioning_runtime_tests.rs:889` (`department_context_reaches_the_lead_on_its_department_desk`, mails "helper"; `:863` binding by `agent:{id}`)
  - `escalation_origin_relay_tests.rs:939` `department_agent` and `:1071` (two-agent cross-desk exchange)
- **Department dispatch sample:** the lead prompt lists "Agent helper (helper): helper@…" (asserted at `provisioning_runtime_tests.rs:875-887`). It comes from `roster_on` and `agent_department.rs:96`.
  - The `department-lead.yaml` fixture (`application/workflow/fixtures/department-lead.yaml`) has no addresses, only `agent: {literal: lead}`. Dispatch is deferred to `channel.call` per the 01-departments coordination notes.
- Roster address test helpers: `department_roster_tests.rs:33` and `provisioning_directory_tests.rs:61`.
- Workflows reference agents by slug (`provisioning_native.rs:140` `retarget_agent`). `autonomous_agent_admission_tests.rs:232` asserts that an agent slug rename cannot redirect an existing workflow.
- The email `channel_bindings` endpoint key equals the primary desk address (`channel.rs:277` `write_channel_email_binding`). The escalation route reads it as the agent's `address`, so there is no principal equivalent yet.

**Could not determine:**
- How the inbound-event worker commits Resend events after `decode_event`.
- Whether `autonomous_agent_provisioning_tests.rs:93` and `onboarding/tests.rs:321` actually assert the desk, or only mention it.
- The exact size of the "others" test bucket. It came from grep hits plus helper call graphs, not a full transitive analysis.

🌱 trail saved about 178k tokens (about $0.09) on the calls whose savings lines were shown; later calls' lines were cut from the output, so the real total is higher.
