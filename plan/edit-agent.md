Agent edit pane: Settings + Channel tabs

 Context

 Every agent created through the Agents workspace also gets a personal channel it owns
 (channels.owner_agent_id, UNIQUE, so at most one). That channel's address follows the agent's
 handle, its owner is pinned at position 0, and it can only be deleted by deleting the agent
 (owned_channel_delete_guard trigger + use_cases/channel.rs:464). It is, in effect, part of the
 agent — but today the only way to configure it is to leave /ui/agents, go to /ui/channels, and
 find it there.

 Meanwhile the agent edit pane's "Run by" chip row (agent_settings.rs:1026) lists every
 channel with a channel_agents row for this agent — which always includes the owned channel, so
 the one channel that isn't really a separate thing is mixed in with the ones that are.

 Outcome: the agent edit pane gets two tabs. Settings is today's agent form. Channel is
 the owned channel's settings form plus its schedules card. "Run by" stays on the Settings tab but
 now lists only the other channels this agent is assigned to.

 ---

 Approach

 1. Tab mechanism: server-side ?tab=, htmx-swapped

 Mirror the CompanyTab / CompanyPaneBody pattern in
 src/adapters/http/pages/company_settings.rs:57-93 and :296-354 — the codebase's established
 answer for "a tab is a whole pane". Its doc comment is the reasoning: a plain URL is what makes a
 tab shareable and back-button-able.

 Deviation from company_tabs: use htmx-swapped <a role="tab"> links rather than plain href
 full page loads, copying the attribute set the agent sidebar entry already uses
 (agent_settings.rs:229-234):

 hx-get="/ui/agents/{agent_id}?company_id={company_id}&tab=channel"
 hx-target="#agent-pane" hx-swap="outerHTML" hx-sync="#agent-pane:replace"
 hx-push-url="/ui/agents?company_id={company_id}&agent_id={agent_id}&tab=channel"

 #agent-pane is already a swap target with PANE_SKELETON, so this is both cheaper and consistent
 with the rest of the workspace. Class list stays daisyUI's tabs tabs-border -mb-px mt-3 +
 tab-active, same as company_tabs.

 Deliberately not the client-side showAgentTab pattern (agent_settings.rs:13, used by the
 create pane): that renders both panels into one DOM at once, which here would mean two live
 <form>s, colliding element ids, and a tab state that has to be re-derived on every re-render path
 (agent save, channel save, the OOB prompt-generator swap). Only one tab's form exists at a time
 with the server-side approach.

 2. src/adapters/http/pages/agent_settings.rs

 Add near the top:

 #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
 pub enum AgentTab { #[default] Settings, Channel }

 impl AgentTab {
     /// What `?tab=` names. Anything unrecognised is the settings the pane opens on.
     pub fn from_query(value: Option<&str>) -> Self { /* "channel" => Channel, _ => Settings */ }
 }

 Hold the body rather than a tab field beside it, exactly as CompanyPaneBody does, so "Channel
 is lit" and "the channel is showing" cannot disagree:

 pub enum AgentPaneBody<'a> {
     Settings,
     /// The agent's owned personal channel, as its own form.
     Channel(&'a AgentChannelTab<'a>),
 }

 pub struct AgentChannelTab<'a> {
     pub channel: &'a Channel,
     pub schedules: &'a [ChannelSchedule],
     pub spam_scan_enabled: bool,
     pub memory_ready: bool,
     /// What the user last typed, when a save was rejected; `None` shows the stored channel.
     pub draft: Option<&'a ChannelDraft<'a>>,
     pub error: Option<&'a str>,
 }

 AgentEditPane gains pub body: AgentPaneBody<'a> and keeps everything it has. No new field for
 the owned channel — it is already derivable from used_by the way agent_display_address
 (:263) and delete_warning (:1054) derive it.

 agent_edit_pane (:311) splits along the seam it already has — per src/AGENTS.md, the header is
 shared and each body becomes its own fn:

 - fn agent_settings_body(pane) -> String — today's {error_html}{used_by_html}<form hx-put=…>
   block, moved verbatim.
 - fn agent_channel_body(pane, tab: &AgentChannelTab) -> String — new:
   - form_error_banner(tab.error)
   - <form hx-put="/ui/agents/{agent_id}/channel?company_id={company_id}" hx-target="#agent-pane" hx-swap="outerHTML" class="space-y-4"> with <input type="hidden" name="form_mode" value="advanced">, then reuse channel_fields (channel_settings.rs:581, already
     pub(super) and already called from this file at :575):
 channel_fields(&ChannelFields {
     company: pane.company,
     app_domain_name: pane.app_domain_name,
     agents: &[],                                   // owner is fixed; no picker rendered
     id_prefix: &format!("agent-channel-{agent_id}"),
     draft: tab.draft.unwrap_or(&stored),           // stored_draft(channel, &participants, &aliases)
     spam_scan_enabled: tab.spam_scan_enabled,
     memory_ready: tab.memory_ready,
     owner: ChannelOwner::Existing(pane.agent),
 })
     ChannelOwner::Existing is what already makes the slug readonly, renders the owner card with
     the hidden agent_ids, and prints owned_address_help. No Delete button — consistent with
     channel_edit_pane_with_memory:337-348, which suppresses it for owned channels because the DB
     forbids the delete. Save / Cancel buttons copy the settings tab's markup (spinner + Saving...,
     required by pages/AGENTS.md); Cancel is hx-get back to …&tab=channel.
   - channel_schedules_card(company_id, channel.id, tab.schedules) below the form
     (channel_settings.rs:1026, already pub). Its buttons target its own
     #channel-schedules-card, so it works unchanged inside #agent-pane.
   - stored_participants / stored_alias_slugs / channel stored_draft are reused from
     channel_settings.rs (the first two are already pub; the third is private — either make it
     pub(super) or build the ChannelDraft inline from those two helpers).
 - fn agent_tabs(company_id, agent_id, tab, owned: Option<&Channel>) -> String — the strip, in the
   header div. Renders nothing at all when owned is None, so an agent without a personal
   channel looks exactly as it does today rather than showing a dead tab.

 New import: use crate::entities::schedule::ChannelSchedule; (as channel_settings.rs:8-12 does).

 used_by_channels (:1026) — filter the owned channel out of the chips. Do it at the call site in
 agent_edit_pane, where the owned channel is already found, and keep used_by_channels a pure
 "render these chips" function. delete_warning(agent_id, pane.used_by) must keep receiving the
 full list: it counts owned / blockers / detached separately (:1053).

 Add Channel::is_owned_by(&self, agent_id: Uuid) -> bool to
 src/domain/entities/channel.rs and use it at the four existing copies of
 owner_agent_id == Some(agent_id) (agent_settings.rs:266, :1056, :1061,
 channel_settings.rs:335) plus the new call sites — src/AGENTS.md's "one decision, one place".

 3. src/adapters/http/routes/ui_agents.rs

 - WorkspaceQuery (:68) gains pub tab: Option<String> for the full-page GET.
 - New struct EditPaneQuery { company_id: Uuid, tab: Option<String> } for edit_pane and the new
   PUT. Leave CompanyQuery alone — the other handlers have no tab.
 - Workspace (:202) gains schedule_use_cases: Arc<ScheduleUseCases> from
   state.schedule_use_cases (app_state.rs:35); view() passes it into AgentSettingsView.
 - AgentSettingsView gains, alongside its existing agents() / channels() / used_by():
   - schedules(channel_id) — same body as ui_channels.rs:486.
   - memory_ready() — extract the call already inlined in channel_step (:881) so both paths use
     one helper.
   - edit_pane(...) takes a tab: AgentTab and, for AgentTab::Channel, finds the owned channel
     in the already-loaded channels slice (no extra query), then loads its schedules and
     memory_ready. If no owned channel exists it falls back to AgentTab::Settings rather than
     erroring.
   - saved_response(...) takes the AgentTab it re-rendered, so HX-Push-Url carries it.
 - agents_page (:254) passes AgentTab::from_query(query.tab.as_deref()) through.
 - New route PUT /ui/agents/{agent_id}/channel → update_owned_channel, added to the router
   at :62. It mirrors update_channel (ui_channels.rs:401) and reuses that module's parsing so
   the two panes cannot drift on what a submitted channel means:
   a. scoped_company, load agent, load channels, find the owned channel — AppError::NotFound
      when there is none.
   b. SubmittedChannel::new(form) (already imported at :50).
   c. submitted.write(Some(submitted.agent_ids())) — needs a pub(super) fn agent_ids(&self) -> Vec<Uuid> accessor on SubmittedChannel (ui_channels.rs:611), since the field is private and
      pub form is already exposed for this same cross-module reason. On Err(message), re-render
      the Channel tab with Some(&submitted.draft()) and the message.
   d. channel_use_cases.update_channel(user_id, company.id, channel.id, write, submitted.form.confirm_spam_disabled()). No new authorization or ownership logic is needed:
      use_cases/channel.rs:421-433 already pins the slug and forces the owner to position 0 for any
      owned channel.
   e. On Ok, answer with the agent pane on the Channel tab plus the OOB sidebar list — the
      sidebar row shows the owned channel's address via agent_display_address, so a slug or alias
      change has to refresh it. On Err, re-render the Channel tab with the draft and the error.

 4. Tests

 - src/adapters/http/pages/tests.rs
   - Update agent_edit_pane_lists_the_channels_running_the_agent (:1282): the owned channel
     must no longer appear among the "Run by" chips, while the delete warning still counts it.
   - New: the Settings tab is the default and renders both tab buttons when the agent owns a channel;
     no tab strip at all when it owns none.
   - New: the Channel tab renders the owner card, the readonly primary address, the schedules card,
     and no Delete Channel button — modelled on
     owned_channel_locks_owner_and_primary_address_and_hides_delete (:1762).
   - New: a rejected channel save keeps what was typed (mirrors
     agent_edit_pane_keeps_a_rejected_save_in_the_form, :1307).
 - src/adapters/http/routes/ui_agents.rs inline tests (:1103): the new PUT saves the owned
   channel and comes back on the Channel tab.

 No schema, migration, or SQL query changes — so no cargo sqlx prepare run is needed.

 ---

 Critical files

 ┌─────────────────────────────────────────────┬──────────────────────────────────────────────────────────────────────────────────────────────┐
 │                    File                     │                                            Change                                            │
 ├─────────────────────────────────────────────┼──────────────────────────────────────────────────────────────────────────────────────────────┤
 │ src/adapters/http/pages/agent_settings.rs   │ AgentTab, AgentPaneBody, AgentChannelTab, agent_tabs, split agent_edit_pane into header +    │
 │                                             │ two bodies, filter "Run by"                                                                  │
 ├─────────────────────────────────────────────┼──────────────────────────────────────────────────────────────────────────────────────────────┤
 │ src/adapters/http/routes/ui_agents.rs       │ tab query params, ScheduleUseCases, schedules() / memory_ready() on the view, PUT            │
 │                                             │ /ui/agents/{id}/channel                                                                      │
 ├─────────────────────────────────────────────┼──────────────────────────────────────────────────────────────────────────────────────────────┤
 │ src/adapters/http/routes/ui_channels.rs     │ pub(super) fn agent_ids() accessor on SubmittedChannel                                       │
 ├─────────────────────────────────────────────┼──────────────────────────────────────────────────────────────────────────────────────────────┤
 │ src/adapters/http/pages/channel_settings.rs │ widen channel stored_draft to pub(super) (only if not built inline)                          │
 ├─────────────────────────────────────────────┼──────────────────────────────────────────────────────────────────────────────────────────────┤
 │ src/domain/entities/channel.rs              │ Channel::is_owned_by                                                                         │
 ├─────────────────────────────────────────────┼──────────────────────────────────────────────────────────────────────────────────────────────┤
 │ src/adapters/http/pages/tests.rs            │ update one test, add four                                                                    │
 └─────────────────────────────────────────────┴──────────────────────────────────────────────────────────────────────────────────────────────┘

 Reused, not rewritten

 - channel_fields + ChannelFields + ChannelOwner::Existing — channel_settings.rs:542-640
   (this file already calls it at :575 for the create flow's channel step)
 - channel_schedules_card — channel_settings.rs:1026
 - stored_participants / stored_alias_slugs — channel_settings.rs:1008-1024
 - SubmittedChannel (new / draft / write) — ui_channels.rs:611
 - ChannelUseCases::update_channel, which already enforces owned-channel invariants —
   use_cases/channel.rs:421
 - CompanyTab / CompanyPaneBody as the shape to copy — company_settings.rs:57-93, :296-354
 - CHANNEL_SETTINGS_SCRIPT's syncChannelAgents / spam-confirm handlers are already in the global
   /ui bundle (mailbox.rs:827), so the channel form needs no new JS

 ---

 Verification

 1. cargo fmt && cargo clippy --all-targets — clean.
 2. DATABASE_URL="postgres://mac03@localhost:5432/mail_agents" cargo test — the page and handler
    tests above.
 3. Run the app and drive it in the browser (server on :3001):
    - /ui/agents?company_id=… → pick an agent → header shows Settings | Channel; Settings is
      lit and the pane is unchanged apart from "Run by" no longer listing the personal channel.
    - Click Channel → the channel form appears, URL becomes …&agent_id=…&tab=channel, address
      field is readonly with the "Rename … in Agents" link, the owner card sits under Agents, the
      schedules card is below, and there is no Delete Channel button.
    - Change the description and an alias, Save Changes → stays on the Channel tab, sidebar row
      updates. Reload the pushed URL → comes back on the Channel tab.
    - Add @public to participants with spam scanning off → the interlock checkbox appears and the
      save is refused until it is ticked (same behaviour as /ui/channels).
    - Add a schedule from the card, toggle and delete it → only the card re-renders.
    - Back button returns to the Settings tab.
    - Open the same channel at /ui/channels?company_id=…&channel_id=… → shows the saved values.
    - Rename the agent's handle on the Settings tab → the Channel tab's primary address follows it.
    - Check both light and dark themes (pages/AGENTS.md: a token override that works in dark is not
      evidence about light).
