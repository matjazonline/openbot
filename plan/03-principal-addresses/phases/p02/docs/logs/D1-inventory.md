# 03PA-P02 D1 inventory (Explore agent, read-only, 2026-11-14; HEAD 830561e2 + accepted p01 tree dc9329df)

I covered all seven areas. The headline is that a principal target is blocked by more than nullable `channel_id` columns: four structures assume every thread sits on a desk and has a desk's email interface (details under "Could not determine"). Line numbers below are from the working tree with p01 applied. The plan's schema line references are stale: `threads` is now `S:4514` (plan says 4431), `thread_principals` `S:4499` (4416), `external_threads` `S:3859` (3776), `workflow_bindings` `S:5086` (5002) and `notification_principal_can_view` `S:972` (941).

Path keys: **R** = `/Users/mac02/.gui-cloudroom/plugins/environment-git-worktree/host-data/worktrees/thr_jt43h4qc8d-1/openbot`, **S** = `R/migrations/20260817000000_init_schema.sql`. Rust paths are written `R/src/...`.

---

## 1. Schema

**Count: about 86 objects.** 31 sit on the four tables, 28 are keys or checks on tables that depend on them, and 27 are functions (most with a trigger).

**The four tables**
- `threads` `S:4514`: id, company_id, `channel_id NOT NULL` (`S:4517`), subject, created_at, updated_at. No principal column.
- `thread_messages` `S:4483`: id, company_id, `channel_id NOT NULL`, thread_id, message_id, created_at, entry_kind (check at `S:4491`).
- `thread_principals` `S:4499`: company_id, `channel_id NOT NULL`, thread_id, principal_id, role (check at `S:4506`), created_at.
- `external_threads` `S:3859`: id, company_id, `channel_id NOT NULL`, binding_id, external_thread_key (check at `S:3868`), thread_id, created_at, updated_at.

**The 30 most important**
1. `S:6795` `threads_company_channel_id_key UNIQUE(company_id,channel_id,id)`: the target of every composite foreign key below.
2. `S:6787` `threads_channel_id_key UNIQUE(channel_id,id)`.
3. `S:10131` `threads_channel_fk` → channels.
4. `S:8324` `threads_channel_updated_idx (channel_id, updated_at, id)`: feeds the thread-list pages; a principal version is needed.
5. `S:6739` `thread_messages_channel_message_key UNIQUE(channel_id,message_id)`: one row per desk per message. A NULL channel gives no uniqueness, so this needs a principal version.
6. `S:6771` `thread_messages_workflow_history_key UNIQUE(company_id,channel_id,thread_id,message_id)`: the real "membership pin". Six foreign keys reference it (items 12–16).
7. `S:10107` `thread_messages_thread_fk (company,channel,thread)` → threads.
8. `S:6779` `thread_principals_pkey (company,channel,thread,principal,role)`: a primary key cannot hold a nullable column, so this key has to be redesigned.
9. `S:10123` `thread_principals_thread_fk`.
10. `S:9679` `external_threads_binding_fk (company,channel,binding)` → `channel_bindings(company_id,channel_id,id)`: the binding is a desk's transport endpoint. Same for `external_messages_binding_fk` `S:9663`.
11. `S:9687` `external_threads_thread_fk`; `S:6277` `UNIQUE(binding_id,external_thread_key)`.
12. `S:9767`, `S:9774` `internal_notes` foreign keys to threads and thread_messages (`channel_id NOT NULL` at `S:3984`).
13. `S:10953` `workflow_message_intent_association` → thread_messages. Also `S:7518` `UNIQUE(company,message,channel)` and `S:17646` `UNIQUE(company,id,message,channel)`, which `S:17666` (`workflow_escalation_agent_receipts`) references.
14. `S:10905`, `S:11007` `workflow_message_destinations` message and replied foreign keys; `S:5561` requires `kind='channel'` ⇒ `destination_channel_id=channel_id`.
15. `S:10792`, `S:10800` `workflow_history_members` foreign keys to thread_messages and to `workflow_runs(company_id,id,channel_id,thread_id)` (unique key at `S:7646`).
16. `S:12791-12792` `channel_reply_contexts` → thread_messages (`channel_id NOT NULL` at `S:12775`).
17. `S:11190` `workflow_runs` thread foreign key; `S:5882` `workflow_runs_check` (a thread requires a channel).
18. `S:10655` `workflow_bindings` thread foreign key; `S:5097` same thread-requires-channel check.
19. `S:9462` `background_tasks_thread_fk` (`ON DELETE SET NULL (thread_id)`).
20. `S:14170`, `S:14171` `agent_history_sessions` foreign key and check; `S:14201`, `S:14204-14209`, `S:14211` `agent_history_conversations` foreign key, scope check (`thread` ⇒ channel+thread) and unique `(agent,channel,thread)`.
21. `S:16832`, `S:16828` `workflow_admission_refusals` foreign key and check.
22. `S:13510` `guard_channel_reply_message_component` and its trigger `S:13522` `channel_reply_association_immutable` on thread_messages: this produces the 23514 in `R/src/adapters/persistence/workflow/channel_reply_membership_tests.rs:184-205`. It keys only on `(company_id, message_id)` via `workflow_channel_message_owner` `S:13557` and `lock_message_publication_boundary` `S:13411`, and never reads channel_id. The channel/thread pin comes from the foreign keys in items 6 and 12–16.
23. `S:11740`, `S:11750` `guard_workflow_run_snapshot`: run channel_id and thread_id are immutable (the test enables this trigger).
24. `S:12852` `capture_channel_reply_context` (trigger `S:12875` on intents): locks the channel row FOR SHARE and joins reply settings and the binding by channel.
25. `S:12878` `guard_channel_reply_context` (`S:12915`) and `S:12932` `guard_channel_reply_run` (`S:12970`): require run and context channel/thread to be equal.
26. `S:13265` `require_channel_reply_proposal_links` (`S:13309`): the integrity document must name the channel and thread, and the reply sender must be the desk's primary address via `addresses.channel_id` (`S:13295`).
27. `S:13351` `channel_reply_publication_matches`: `filed.channel_id=context.channel_id` and `delivery.channel_id=context.channel_id`.
28. `S:16345` `workflow_follow_reviewer_eligible`: channel access_mode and grants, plus `thread.channel_id=run.channel_id` (`S:16358`).
29. `S:1179` `notify_thread_message` (`S:8616`), `S:1158` `notify_thread_activity` (`S:9332-9333`), `S:9311` `notify_workflow_run_changed` (`S:9330-9331`), `S:1129` `notify_internal_note_change` (`S:8521`): their NOTIFY payloads are keyed on channel_id.
30. `S:972` `notification_principal_can_view`: covered in section 4.

**The rest, by pattern**
- **Run and job association guards:** `preserve_workflow_run_association` `S:1477` (`S:9307`), `validate_workflow_job_association` `S:1832` (`S:8449`), `require_schedule_occurrence_scope` `S:1682` (`S:8595`, run channel = schedule channel and thread IS NULL), `internal_notes_are_immutable` `S:822` (`S:8514`).
- **Agent history guards:** `guard_agent_history_session` `S:14218` (`S:14250`), `guard_workflow_agent_session` `S:14253` (`S:14294`), `agent_history_conversation_unused` `S:14419`, `guard_agent_history_exchange_insert` `S:14533` (`S:14598`), `notify_channel_response_run` `S:14081`.
- **Escalation:** `guard_workflow_escalation_handling_root` `S:18010` (`S:18038`), `workflow_escalation_person_eligible` `S:17438`/`S:17450` and `workflow_escalation_agent_eligible` `S:17626`/`S:17638-17639` (both use `COALESCE(run.channel_id, refusal.channel_id)`), `workflow_escalation_agent_route` `S:17604`.
- **Channel-agnostic, low risk:** `delete_orphan_message` `S:599` (`S:8609`); `workflow_question_event_guard` `S:13626` (thread only).
- **Indexes:** `S:8303`, `S:8310`, `S:8317`, `S:8029`, `S:7676`, `S:7677`, `S:7798`, `S:7840`.
- **Downstream channel-only columns:** `message_deliveries` attribution check `S:4036` (requires channel_id plus source and destination bindings); `notifications.channel_id NOT NULL` `S:4191` with foreign key `S:9934`.

## 2. `workflow_bindings` and `workflow_binding_revisions`

**Schema**
- Columns `S:5086-5100`: company_id, id, state_revision, configuration_revision, active, `channel_id` (nullable, `S:5092`), `thread_id` (`S:5093`), activated_at, `company_wide GENERATED (channel_id IS NULL)` (`S:5095`; must become "both targets NULL").
- Checks: `S:5096` activation shape; `S:5097` thread requires channel.
- Keys: `S:7187` primary key; `S:7194` `UNIQUE(company_id,channel_id,id)`, referenced by `channel_reply_settings` `S:12762-12763` and `channel_schedules` `S:9541`; `S:7201` `UNIQUE(company_id,id,company_wide)`, referenced by `S:9534`. `channel_schedules` also has `binding_channel_id`/`binding_company_wide` at `S:3653-3654` and a check at `S:3665`.
- Foreign keys: `S:10647` channel, `S:10655` thread, `S:10663` company, `S:10671` selected revision.
- `workflow_binding_revisions` `S:5063` has no target columns. Primary key `S:7179`, foreign keys `S:10631`/`S:10639`, immutability trigger `S:11780`, escalation policy constraint `S:16065`.
- No trigger checks a binding's principal kind today. The closest precedent is `guard_address_principal` `S:15570`/`S:15584`.

**SQL functions that assume a desk binding**
- `guard_channel_reply_settings` `S:12832-12836`: `binding.channel_id=NEW.channel_id AND thread_id IS NULL`.
- `capture_channel_reply_context` `S:12866-12871`.
- `workflow_escalation_agent_route` `S:17616-17617`: binding on the agent's provisioned desk.
- The refusal and run rows that copy the binding's channel and thread: `S:16821-16832`, `S:5849-5850`.

**Rust queries: 22 statements on `workflow_bindings`** (16 direct, plus 6 built from the `BINDING_FROM` const at `R/src/adapters/persistence/workflow/reads.rs:71-77`, used at `reads.rs:457`, `:477` and `authoring_reads.rs:278`, `:296`, `:308`, `:324`). Those that assume a channel or join bindings to channels:
- `R/src/adapters/persistence/workflow/bindings.rs:148-150`: INSERT channel/thread via `association_columns`. `:156` is the UPDATE.
- `bindings.rs:231-235`: a message trigger is refused without a channel. `:236` takes an advisory lock per channel. `:239-243` caps message bindings per channel.
- `R/src/adapters/persistence/workflow/binding_rows.rs:108-129`: `association_columns`/`association_from_columns` treat (None, Some) as corrupt. 22 call sites.
- `R/src/adapters/persistence/workflow/message_intents.rs:145-146`: eligibility requires `binding.channel_id = intent.channel_id`.
- `R/src/adapters/persistence/workflow/role_binding.rs:29-38`: department read from `channels FOR SHARE`. `:62` and `:78` filter by `binding.channel_id=$2`.
- `R/src/adapters/persistence/workflow/schedules.rs:584`: channel or company-wide.
- `R/src/adapters/persistence/workflow/authoring_reads.rs:308`: bindings by channel.
- `R/src/adapters/persistence/attention.rs:263`, `:299`, `:561`: visibility by `channel_id = ANY` or the NULL company-wide branch.
- `R/src/adapters/persistence/channel.rs:486`: the channel-deletion retention check.
- Not channel-specific: `admission.rs:255` (FOR SHARE), `schedules.rs:777`, `authoring_reads.rs:111`, `channel_reply.rs:205`.

**`RelatedAssociation`** is defined at `R/src/application/workflow/authorization.rs:54-61` as Company | Channel | Thread{channel_id, thread_id}. It appears in 73 non-test files, with 37 `Thread` variant sites. Its authorization at `authorization.rs:165-190` uses `viewer_access` plus `thread.channel_id`. The HTTP shapes are `R/src/adapters/http/routes/workflows/mod.rs:266-281` (`AssociationBody`) and `R/src/adapters/http/routes/workflows/ui/authoring/bindings.rs:135-145`.

**Admission**
- `R/src/adapters/persistence/workflow/admission_write.rs:7-60` `create` captures the manifest (`:12`), then `insert_run` (`:126-149`, channel/thread at `:129`) and `insert_first_job` (`:177-185`). It has no principal gate.
- The channel-row gate is upstream: `R/src/adapters/persistence/workflow/admission.rs:22-29` → `R/src/adapters/persistence/workflow/association.rs:6-60`, which locks `channels FOR SHARE` (`:21`), the grants, and `threads … channel_id=$2 FOR SHARE` (`:58`). `authorize_association` has 30+ callers.
- The settings_revision gate is in the inbound commit, not in admission: `R/src/adapters/persistence/thread/inbound.rs:167-187` `hold_channel_settings` and `:189-215` `verify_channel_settings` (returns `Superseded`), using `R/src/adapters/persistence/channel_gate.rs:1-40`.

## 3. Thread resolution in ingress

- **Address resolution:** `R/src/application/use_cases/thread/ingest/routing.rs:133-231` `resolve_addresses`. It calls `address_target` (`R/src/application/use_cases/thread/support.rs:76`), then `addressed_slug` (`routing.rs:900-919`):
  - A principal address mailed by a team member is refused (`principal_refusal` `:941-946`, `R/src/application/use_cases/thread/notices.rs:77-83`).
  - A stranger gets the same `Unknown` answer as a slug that names nothing.
  - Then: dedupe by channel id (`:183`), the desk's email binding (`inbound_binding` `:187`/`:283-300`), and `authorize_channel` (`:306-347`: trace cycle `:317`, `participant_access` `:322`, thread party check `:328`, spam `:341`).
- **`ChannelCandidate`** `routing.rs:58-70`: company, `channel: Channel`, `binding_id: ChannelBindingId`, matched_slug, handle, role, step, access.
- **`prepare_channels`** `routing.rs:456-510`: calls `resolve_thread` per candidate, strips quoted history once against the primary thread (`:477-486`), then `prepare_channel` `:514-577`. That covers thread injection (`:526`/`:582`), the turn limit (`:530-541`), `ThreadTarget::Existing` or `Create` (`:542-546`), participants from `access.trusted && channel.add_3rd_party` (`:551`), mentions (`:568`) and `direct_parent` (`:623-651`).
- **`resolve_thread`** `routing.rs:376-427`: every path filters by channel.
  - explicit target thread (`:386`)
  - `find_thread_for_message(channel)` (`:394`)
  - `thread_for_thread_keys(binding_id)` (`:403`)
  - `find_ancestor_thread` (`:431-453`)
  - `find_thread_by_thread_index(channel)` (`:424`)
- **`PreparedChannel`** `R/src/application/use_cases/thread/ingest/commit.rs:25-35`. `CommitPlan::build` (`:63-122`) turns each one into one `ThreadAssociation` (`:83-94`) and one `InboundAdmissionIntent` (`:95-105`, settings_revision at `:103`). The source binding is the primary desk's (`:77`).
- **Types** in `R/src/application/transport/ingress.rs`: `ThreadTarget` `:500-503`; `ThreadAssociation` `:528-541` (channel_id, `binding_id: ChannelBindingId`, target, role, step, principals); `InboundAdmissionIntent` `:597-607` (channel_id, role, mentioned, reply_to, settings_revision); `InboundCommitRequest` `:615-640`.
- **The commit:** there is no `adapters/persistence/thread/commit*`. It is `R/src/adapters/persistence/thread/inbound.rs:115-160` `commit_as_on`, in this order:
  - settings gate `:167`
  - `verify_association_bindings` `:394-455` (`channel_bindings.channel_id = association.channel_id` at `:444`)
  - `resolve_threads` `:523-551`
  - `lock_existing_threads` `:583-621` (thread's channel must equal the association's, `:612`)
  - `open_thread` `:623-654` (`INSERT threads(...channel_id...)` at `:631`)
  - `add_thread_principals` `:656-683`
  - `bind_conversations` `:685-709`, which writes external_threads via `R/src/adapters/persistence/thread/external.rs:200-235`
  - `associate_threads` `:751-783` (thread_messages with channel_id)
  - `create_intents` `:815-872` (threads looked up by channel_id `:821-838`; trace written `:826`/`:842`)
  - Helper structs: `ResolvedThread` `:67-71`, `LockedThread` `:518-521`.
- **Other thread writers:** `R/src/adapters/persistence/workflow/message_writer.rs:52-70`, `R/src/adapters/persistence/thread/mod.rs:305-336`, `R/src/adapters/persistence/thread/message.rs:660`/`:910`.
- **Port and entity:** `R/src/application/ports/thread/mod.rs` takes a channel at `:45-47`, `:54-56`, `:75-77`, `:88-90`, `:108-110`. `Thread.channel_id: Uuid` is at `R/src/domain/entities/thread.rs:10`. There are 28 non-test reads of `thread.channel_id` across 10 files.
- **What changes for a principal target:**
  - The candidate, association and intent need a target enum (desk or principal) in place of `channel_id`.
  - The `addressed_slug` Principal arm becomes a principal candidate: a person is reachable by its owner/self rules, an agent by the team or by `open_to_public`. A stranger refused by policy must still get the `Unknown` answer.
  - `authorize_channel` and its spam check are replaced by `direct_access`.
  - The trace-cycle check needs principal entries.
  - `resolve_thread` and the port's lookups need principal versions.
  - `add_3rd_party` does not apply: a direct thread's participants are the target and the writer.
  - The commit gates per principal, and `create_intents` looks threads up by target.

## 4. Access

- `Channel::participant_access` is at `R/src/domain/entities/channel.rs:140-163`, `viewer_access` at `:179-198`, `ParticipantAccess` at `:124-127`, `ChannelAccessMode` at `:90-94`.
- `PrincipalAccessContext` is at `R/src/domain/entities/participant.rs:275-283` (with `external()` at `:290-299`). The `Principal` struct at `:50-58` has no `open_to_public` or `settings_revision`.
- Callers of `participant_access`: `routing.rs:196`, `:322`, `:723`; `R/src/application/use_cases/thread/mod.rs:222`; `R/src/application/use_cases/channel.rs:388`.
- Callers of `viewer_access`: `use_cases/channel.rs:153`, `:170`; `thread/mod.rs:398`, `:427`; `authorization.rs:177`; `R/src/adapters/persistence/thread_activity.rs:72`.
- `notification_principal_can_view` `S:972-982` covers persons only: owner, a team or public channel, or a view grant; with a NULL channel, owner or admin.
  - SQL callers: `S:1028`, `S:1909`, `S:17450`, `S:17638-17639`. Rust callers: `R/src/adapters/persistence/notification.rs:356`, `R/src/adapters/persistence/notification/projection.rs:187`.
  - Recheck triggers: `S:8556`, `S:8563`.
- **How a thread page decides:** `thread/mod.rs:234-259` `readable_thread(viewer, company, channel, thread)` calls `readable_channel` (`use_cases/channel.rs:143-154`, context from `:121`) and then requires `thread.channel_id == channel.id` (`:254`).
  - Callers in `R/src/adapters/http/routes/ui.rs`: `:354`, `:500`, `:559`, `:973`, `:1018`.
  - Other route callers: `ui_internal_notes.rs:115`, `ui_message_diagnostics.rs:65`, `ui_local_agents.rs:222`, `ui_message.rs:29`, `ui_attachments.rs:83`, `channel/simulation.rs:110`, `:406`.
  - Read helpers: `R/src/application/use_cases/thread/reads.rs:150-252`.
- **Mailbox routes** (`ui.rs:69-74`):
  - `mailbox_page` `:305-397`, `thread_column_fragment` `:401`, `thread_page_fragment` `:447`, `message_pane_fragment` `:488`.
  - Streams: `thread_message_stream` `:543`; `thread_column_stream` `:675`, whose `channel_wake_ups` is keyed by channel (`:696`, `R/src/adapters/http/routes/live_updates.rs:67-73`).
  - Compose and reply: `:833`/`:875`, `:955`/`:999`.
  - `load_viewable_channel` `:206-223`.
- **Desk thread lists:** `R/src/adapters/http/routes/channel.rs:671` (`/threads`), `:708` (`/threads/list`), `:745` (API), `load_thread_page` `:261`. Reads: `thread/reads.rs:98-124`. Rendering: `R/src/adapters/http/pages/mailbox.rs:181`, `:199`, `:2002`, `:2045`.
- **Also by channel:** attention visibility at `attention.rs:188-189`, `:299`, `:545`, `:561`.
- No "Direct" list exists anywhere yet.

## 5. `open_to_public` analogues

- **Desk public mode:**
  - Putting `@public` in the participant list makes `R/src/adapters/persistence/channel.rs:203-230` (`channel_access`) store `Public`.
  - Members-only desks refuse Public (`R/src/adapters/persistence/department_desks.rs:54-70`). Schema: `S:911`, `S:920`, `S:925-926`.
  - When read back, Public becomes `@public` again (`channel.rs:105`).
  - In `participant_access`, Public authorizes everyone but trusts only listed senders or the team (`channel.rs:146-149`). Untrusted senders are spam-scored (`R/src/application/use_cases/thread/ingest/policy.rs:185-195`, applied at `routing.rs:341`).
- **Spam interlock:**
  - `use_cases/channel.rs:81-87` (`is_public`) and `:93-104` (`check_spam_interlock`), enforced in `create_channel` `:214` and `update_channel` `:331`.
  - `SpamScanning` is defined at `R/src/application/use_cases/agent.rs:181-184` and set from config at `R/src/infra/config.rs:678-682`. `ChannelUseCases` holds it; `AgentUseCases` (`agent.rs:234-247`) does not.
  - Where `confirm_spam_disabled` comes in:
    - Desk HTML form: `routes/channel.rs:115`, `:141` → `:395`/`:410` (create), `:566`/`:589` (update).
    - Desk JSON API: `routes/channel.rs:194` → `:769`/`:792`, `:825`/`:853`.
    - Mailbox desk UI: `R/src/adapters/http/routes/ui_channels.rs:273`, `:319`.
    - Checkboxes render at `R/src/adapters/http/pages/channel_settings.rs:609` and `pages/channels.rs:29`.
- **Agent settings form:**
  - Page: `R/src/adapters/http/pages/agent_settings.rs:254` (`agent_edit_pane`), `:289` (body), `:360` (placement card), `:661` (`agent_fields`).
  - Routes: `R/src/adapters/http/routes/ui_agents.rs:56-61`, `:315` create, `:444` update, `:528` view.
  - Older HTML form: `routes/agent.rs:66`, `:455`.
- **Company-agent JSON API:** `R/src/adapters/http/routes/agent.rs:50-61`, `AgentJsonPayload` `:185-195` (`deny_unknown_fields`), `CompanyAgentJsonUpdate` `:200-204`, `AgentResponse` `:222-228`, handlers `:604`, `:653`, `:685`.
- **Use case:** `AgentWrite` `use_cases/agent.rs:39-52`, `create_agent` `:323`, `update_agent` `:555`.
- **Persistence (where a new field would land):**
  - `principals` `S:953-965`: the new column and the "agents only" check go here (kind check `S:963`, shape check `S:964`).
  - Principal rows: `R/src/adapters/persistence/participant.rs:494-515` (`create_agent_principal_on`), `PrincipalDb` `:34`, `PRINCIPAL_SELECT` `:140`.
  - Agent rows: `R/src/adapters/persistence/company_agent.rs:71-85`, `R/src/adapters/persistence/agent.rs:21` (`AGENT_COLUMNS`), `:92-100` (insert), `:256-285` (`UPDATE agents`), `:487` (`UPDATE principals`).
  - The `Agent` entity (`R/src/domain/entities/agent.rs:9`) carries no principal fields, so the form and API need a principal read or join.
- **Channel `settings_revision`:**
  - Defined at `S:917`, `channel.rs:75`, persistence `channel.rs:52`/`:141`.
  - Who bumps it: `channel.rs:547` (`write_channel_settings_on`, under the exclusive gate from `channel_edit.rs:117`), `department_desks.rs:306` (`rederive_on`), `:396` (`set_desk_ownership_on`), and `S:15618` (`department_principal_demoted`, trigger `S:15622`).
  - Who checks it: `thread/inbound.rs:189-215`, plus the agent-notice route (`R/src/adapters/persistence/notification/agent_notice_route.rs:16`/`:51` and `agent_notice_message.rs:101`). The intent's revision is set at `ingest/commit.rs:103`.
  - `channel_reply_contexts.settings_revision` (`S:12782`) is a different thing: the responder settings revision.
  - `principals` has no `settings_revision`, and `ChannelGateKey` (`channel_gate.rs:33-40`) is channel-only.

## 6. Role resolution and C004 role-refusal producers

- **Department from the desk:**
  - `R/src/adapters/persistence/workflow/agent_manifest/roles.rs:50-76` `load_facts_on`: a company association gives MissingDepartment (`:58`); otherwise `SELECT department_id FROM channels` (`:67`), and no department gives MissingDepartment (`:74`). Then `load_context_on` (`:78-107`).
  - `R/src/adapters/persistence/workflow/agent_manifest/mod.rs:115-135` `context_department_on` → `role_context_department_id`. Stored via `agent_manifest/storage.rs:10-14`; schema `S:12397`, `S:16072-16108`. Read by `escalations.rs:118` and `follow_reviewers/mod.rs:76`.
- **Activation refusal:** `role_binding.rs:15-49` (`check_on`, raising `binding.role_needs_department` at `:5-12` and the escalation variant at `:101-104`), called from `bindings.rs:54`. `check_detach_on` `:53-99` is called from `channel.rs:542` and `department_desks.rs:392`. The plan's "ambiguous when the agent is in several departments" refusal belongs here.
- **Refusal at admission:** `admission_write.rs:12` → `AppError::WorkflowRole` (`roles.rs:33-38`) → `admission.rs:56-63` → `R/src/adapters/persistence/workflow/admission_refusal.rs:31-58` (INSERT with the association's channel/thread, `:37-40`) → `R/src/adapters/persistence/workflow/escalations.rs:41-75`. That INSERT (`:61`) takes `origin_department_id` from `channel.department_id` of the refusal's channel (`:65`/`:71`). Replay: `admission.rs:136-146`.
- **C004 inventory of producers:**
  1. `agent_manifest/roles.rs:58` MissingDepartment (company association) and `:74` (desk without a department).
  2. `agent_manifest/roles.rs:164` Unavailable, `:198` WrongKind, `:206` TooMany, `:257` WrongKind, `:267` Unavailable (`role_edges_on` `:169`, `finish_roles_on` `:227`).
  3. `R/src/application/workflow/roles.rs:54-97` `resolve_role`, pure: DepthBound, WrongKind, Ambiguous, TooMany, NoHolder. Also reached via `resolve_people_on` (`agent_manifest/roles.rs:314-325`). Codes are at `R/src/domain/entities/department.rs:110-131`.
  4. `application/workflow/roles.rs:33-52` `refuse_unresolved_sample_role` (`sample/dispatch.rs:22`, `:236`; `sample/history.rs:37`): scripted samples only, not legitimate admission producers per C004.
  5. `R/src/adapters/persistence/workflow/reviewer_resolution.rs:54-69` NoHolder and TooMany: decision time, not admission.
  6. Where outcomes are stored:
     - `R/src/application/workflow/message_admission.rs:96`, `:418`, `:453` → `message_intents.rs:42-46`, `:336-349` (column `S:16039`).
     - `R/src/application/workflow/schedule_admission.rs:224`, `:247` → `schedules.rs:828-854`.
     - Also `R/src/application/services/inbound_event_worker.rs:660` and `R/src/adapters/persistence/agent_publication/commands.rs:110`.
  7. Escalation cases: `escalations.rs:61` (admission), `:114` (run), `escalation_follow.rs:20` (empty reviewers).

## 7. Relay and reply correlation

- **Trace built:**
  - `R/src/adapters/protocols/email/egress.rs:181-188` `wire_relay` appends the source channel and adds a hop. Used by the `X-MailAgents-Trace` header (`:127-142`) and the internal relay (`:170`, `:388`).
  - `R/src/application/workflow/message_planner.rs:225-229` starts a user email's trace with the sender desk; `:263-270` continues a reply's recorded trace.
  - The recorded trace is read at `R/src/adapters/persistence/workflow/message.rs:481-505` and stored at `thread/inbound.rs:826`/`:842` (`S:5603`, at most 5 entries per `S:5618`).
- **Trace parsed and checked:** parsed at `R/src/adapters/protocols/email/parser.rs:306-358` → `R/src/adapters/protocols/email/ingress.rs:394`, and `ingest/relay.rs:188`; the field is `ingress.rs:172`. Checked by the hop limit at `policy.rs:173-174` and the cycle check at `routing.rs:316-320`.
- **Relay sender:** `R/src/application/use_cases/thread/ingest/relay.rs:225-248` `authorize_relay`. A principal target is refused at `:234-237`; From must equal the source desk's address (`:240-246`). Principal detection is at `:80-84`.
- **Reply correlation by channel (ingress):**
  - `routing.rs:376-427`, `:431-453`, `:623-651`.
  - `external.rs:260` and `:285-306` (`association.channel_id = binding.channel_id` at `:302`), `:315`.
  - `thread/mod.rs:477-489`, `:541-547`.
- **Reply correlation (workflow):** `message_admission.rs:289-307` and `:339-380` → `R/src/adapters/persistence/workflow/reply_waits.rs:193-210`, which matches by `thread_id` only and is channel-agnostic. Binding eligibility still uses `intent.channel_id` (`message_intents.rs:145`).

## Could not determine

1. **No desk interface for a principal.** `ThreadAssociation.binding_id`, external threads and messages, and the reply-key lookups all hang off a desk's email interface (its `channel_bindings` row), and a principal has none. Plan 02A.2 says a direct thread resolves "by the same reply-header and subject rules" as a desk, but nothing says what the principal-side key is.
2. **How a principal goes in the trace.** `trace_channels` is a `uuid[]` of channel ids (`S:5603`). The plan wants "the principal's address in the trace" and doesn't say how it's encoded.
3. **What makes `direct_competing_` produce one thread.** Today a new thread is created on every unmatched message. No uniqueness key or lock gives "two concurrent messages to the same agent create one direct thread".
4. **Notifications and replies on direct threads.** `notifications.channel_id NOT NULL` and the `message_deliveries` attribution check both need a target for direct threads. These look like p02/p03 overlap rather than p02 alone.
5. **`AgentUseCases` has no `SpamScanning`.** The interlock for `open_to_public` needs it passed in, or a shared helper.
