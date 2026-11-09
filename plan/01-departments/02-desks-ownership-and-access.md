# 02 Desks, ownership and access

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

A desk can belong to a department. A department desk can be members-only, in which case the
department's membership decides who may write to and read it, through the grant rows channel
policy already consults. The product calls channels desks. Continues in
[03](03-leads-and-coordination.md).

## Batch 02A Ownership

1. Schema: `channels.department_id uuid` with
   `FOREIGN KEY (company_id, department_id) REFERENCES departments (company_id, id) ON DELETE RESTRICT`.
   `UNIQUE (company_id, id)` on `channels` if it does not already exist for composite keys.
2. `Channel` (`src/domain/entities/channel.rs:44`) gains `department_id: Option<DepartmentId>`;
   `ChannelDb`, `CHANNEL_SELECT` (`src/adapters/persistence/channel.rs:38`, `:88`),
   `ChannelWrite`, `ChannelUpdate` and `write_channel_settings_on` (`:450`) carry it.
3. Assigning or clearing a desk's department is a channel settings edit: it goes through
   `ChannelUseCases::update_channel` (`src/application/use_cases/channel.rs:310`), moves
   `settings_revision`, and requires that the department belongs to the same company (the
   composite foreign key is the backstop).
4. Channel delete (`channel.rs:401`) is unchanged: ownership never blocks deleting a desk, only
   retained conversations and workflow configuration do. Department delete is what the foreign
   key restricts.

## Batch 02B Members-only desks

1. Schema: `channels.members_only boolean NOT NULL DEFAULT false` with
   `channels_members_only_owned CHECK (NOT members_only OR department_id IS NOT NULL)`, and
   `channels_members_only_allowlist CHECK (NOT members_only OR access_mode = 'allowlist')`.
   The second check is what keeps `Channel::participant_access` and `viewer_access` untouched: a
   members-only desk is an allowlist channel, and allowlist semantics already say "listed
   principals, plus the owner".
2. Derived grants. One persistence function `sync_department_grants_on(tx, department)` deletes
   every `department_membership` grant on the department's members-only channels and inserts the
   current set: each member, the lead, and the parent department's lead (decision 7), each with
   `participate` and `view`. It runs inside:
   - every membership and lead change in [01](01-departments-and-membership.md);
   - every re-parent, for both the old and the new parent's subtree one level down;
   - `write_channel_settings_on` when `members_only` or `department_id` changes.

   It moves `settings_revision` on each channel it touches, so an inbound message routed under the
   old grants is refused at commit, as any settings edit is today (`Channel::settings_revision`
   doc comment, `channel.rs:70`).
3. Configured allowlist entries (`configured_allowlist` provenance) stay alongside derived ones, so
   a members-only desk can still admit a named outside address. `channel_access`
   (`src/adapters/persistence/channel.rs:193`) keeps deriving the mode from the email list;
   `members_only` forces `Allowlist` when the list is empty. Update the form round-trip test
   `a_channel_form_round_trips_its_email_allowlist_through_principal_grants` (`:997`) so derived
   grants are neither shown as emails nor deleted by a form save.
4. `notification_principal_can_view` (`init_schema.sql:941`) already reads grant rows and needs no
   change; `notification_recheck_channel_access` fires on grant writes and re-evaluates. Add a
   test that removing a member revokes their pending notifications on a members-only desk.
5. Agent principals. Derived grants are also written for agent members. Internal relay authorizes
   by the sending channel's address (`authorize_relay`, `docs/inter_channel_agent_communication.md`),
   and the receiving channel's `participant_access` then sees the run's actor. Confirm in a test
   which principal a relayed message carries into `participant_access`, and document the result
   in [03](03-leads-and-coordination.md)'s dispatch section; if it is the binding's run-as person
   rather than the agent, an agent lead must also be a person-reachable grant and the plan's
   decision 7 is applied to that person.

## Batch 02C The word "desk"

1. UI copy: every user-visible "channel" in `src/adapters/http/pages/` becomes "desk" (singular,
   plural, capitalized). The 1,646 occurrences of the word in that directory are mostly CSS
   classes, element ids, `hx-*` targets and hrefs; those stay. Change only text nodes, `title`,
   `placeholder`, `aria-label` and flash messages. Start with the nav item `nav-channels`
   (`pages/layout.rs:118`), the channels page heading (`pages/channels.rs:164`), the settings
   breadcrumb (`pages/channel_settings.rs:133`) and the company workspace count
   (`pages/company_settings.rs:420`).
2. Email copy: `format_help_email_body` and the undeliverable-address bounce
   (`writable_channel_directory`, `docs/system_addresses.md`) say desk. Their unit tests assert
   the new wording.
3. Agent-facing text: tool descriptions and the agent directory tool (`AgentDirectoryEntry`,
   `src/application/agent/authoring_reads.rs:138`) say desk where they say channel, so prompts
   and product agree.
4. URLs (`/companies/{id}/channels`), route names, JSON fields, `X-MailAgents-Channel-*`,
   database and Rust identifiers do not change (decision 2). Add one line to
   `src/adapters/http/pages/AGENTS.md`: "The product word for a channel is desk; identifiers keep
   channel."

## Focused checks

- `department_desk_ownership_`: assigning a desk to a department of another company is refused;
  deleting a department with a desk is `Conflict`; clearing the desk's department then allows it.
- `department_desk_members_only_`: a member can write to and read a members-only desk, a
  non-member teammate cannot, the owner can; removing the member revokes both in one commit and
  bumps `settings_revision`; the parent lead can write, the grandparent lead cannot; a configured
  allowlist address survives a membership change.
- `department_desk_relay_`: an agent member's desk relaying to the members-only department desk is
  admitted; a non-member agent's is refused with a definite refusal, not a retry.
- Rendering tests in `pages/tests.rs` assert "desk" in nav, headings and flash messages, and a
  test greps the pages directory for `>Channel` and `Channel<` text-node patterns to keep the
  word from returning.
