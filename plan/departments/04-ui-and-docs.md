# 04 UI and documentation

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Owners and admins manage departments from the company settings; everyone sees which department a
desk belongs to; documentation describes departments and uses the word desk. Continues in
[05](05-integration-and-final-verification.md).

## Batch 04A Company settings: Departments tab

1. Add `CompanyTab::Departments` (`src/adapters/http/pages/company_settings.rs:19`) and its pane,
   following the list-and-pane shape of `company_settings_body` (`:308`) and the channel settings
   page (`pages/channel_settings.rs`). The pane shows the tree (indented by parent), and for the
   selected department: name, slug, description, parent, lead, members, owned desks.
2. Forms: create (name, slug, description, parent, lead), edit, delete, add member, remove member.
   Lead and member pickers list the company's person and agent principals. Routes live in
   `src/adapters/http/routes/ui_departments.rs`, mounted like `ui_channels.rs:47`, and call
   `DepartmentUseCases` only. Refusals render on the form (`Conflict` for a taken slug or a
   department that still owns desks, `BadRequest` for a cycle).
3. `workspace_links` (`company_settings.rs:414`) adds a Departments count beside the Desks count.
4. Nav: no new top-level item. Departments are reached through company settings; the desks list
   groups desks by department.

## Batch 04B Desk settings and lists

1. `ChannelDraft` (`pages/channel_settings.rs:56`) gains `department` (a select) and
   `members_only` (a checkbox shown only when a department is selected). The checkbox's help text
   says who gets access: members, the lead, and the parent department's lead.
2. The desks list (`pages/channels.rs:111`) groups desks under their department heading, with
   "No department" last. `channel_row_fragment` (`:362`) shows the department name as a badge.
3. Agent settings (`pages/agent_settings.rs`) show the agent's departments and lead roles as
   read-only text with links to the Departments tab; membership is edited there, not on the agent.

## Batch 04C Documentation

1. New `docs/departments.md`: what a department is, desks and ownership, members-only access and
   the one-step hierarchy rule, leads and the department desk, dispatch and escalation over the
   relay, delete rules. Name only behavior the code has after [01](01-departments-and-membership.md)
   through [03](03-leads-and-coordination.md).
2. Replace "channel" with "desk" in prose across `docs/*.md` (173 occurrences) where it means the
   product concept; keep it in code spans, table and column names, headers and type names. Add a
   terminology note at the top of `docs/transport_architecture.md`: "desk" is the product word for
   a channel.
3. Update `docs/inter_channel_agent_communication.md` with a section on department dispatch and
   escalation, and `docs/agent-authoring.md` with the directory's department fields.
4. Add this plan's paths to `NO_BACKWARD_COMPATIBILITY_NOTES` (`src/main.rs:166`); done when the
   plan was written.

## Focused checks

- Rendering tests in `pages/tests.rs` for the Departments pane, the tree indentation, the desk
  badge and the members-only help text.
- Route tests in `routes/ui_departments_tests.rs` mirroring `channel_authorization_tests.rs`: a
  member gets `404` on every department write and sees only their departments.
- Documentation-only checks: links resolve, quoted file:line anchors match the final code, and no
  prose "channel" remains outside code spans (a grep the reviewer runs and records).
