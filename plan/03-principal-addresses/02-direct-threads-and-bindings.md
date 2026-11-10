# 02 Direct threads and bindings

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

A message to a principal lands in a direct thread with the access rules of decision 5, and an
agent principal's binding admits a run for it. Continues in
[03](03-agent-default-binding-and-relay.md).

## Batch 02A Threads

1. `threads` (`init_schema.sql:4431`): `channel_id` nullable, add `principal_id`, check exactly one
   set, composite foreign keys. `thread_messages`, `thread_principals` (`:4416`) and
   `external_threads` (`:3776`) take the same pair; the membership-check constraints that today
   pin `(company_id, channel_id, thread_id)` (see
   `channel_reply_publication_component_membership_checks_new_tenant_and_message`,
   `src/adapters/persistence/workflow/channel_reply_membership_tests.rs:184`) pin the target pair
   instead.
2. Thread resolution in ingress (`application/use_cases/thread/ingest/`) resolves a principal
   target to its direct thread by the same reply-header and subject rules as a desk.
3. Access. Add `Principal::direct_access(context) -> ParticipantAccess` and
   `Principal::direct_viewer_access(context) -> bool` in
   `src/domain/entities/participant.rs`, the only definitions of decision 5, mirroring the
   channel pair. `notification_principal_can_view` (`:941`) gains the principal-target branch.
   Pages that list threads for a channel gain a "Direct" list per principal the viewer may read.
4. Add `principals.open_to_public boolean NOT NULL DEFAULT false`, constrained to be false for
   non-agent principals, and carry it through the domain, persistence, company-agent JSON API and
   agent settings form. The control says "Allow external messages to this agent's direct address";
   it changes only principal-target admission, never a desk's access policy. Apply the existing
   public-ingress spam interlock, including explicit `confirm_spam_disabled` acknowledgement when
   scanning is unavailable, and bump the principal's `settings_revision` on change. Opening a desk
   uses the ordinary desk control and never requires this flag; the intermediate `members_only`
   desk flag must be cleared when selecting public, as 01-departments' constraints require.

## Batch 02B Bindings

1. `workflow_bindings` (`init_schema.sql:5002`): `channel_id` nullable, add `principal_id`, check
   at most one set, `company_wide` generated from both being null. A trigger refuses a binding on
   a `person`, `external` or `system` principal. `thread_id` requires whichever target is set.
2. Admission (`admission_write.rs:7`) gates on the principal row the way it gates on the channel
   row, with a `settings_revision` on `principals` moved by every edit that affects routing (slug,
   `open_to_public`). Role resolution from [02-workflow-roles](../02-workflow-roles/README.md) on
   a principal binding uses the agent's departments: a role binding on an agent that is in exactly
   one department resolves there; in several, activation is refused as ambiguous.
3. Reply correlation and the relay treat a principal target as a hop with the principal's address
   in the trace.
4. A run admitted on a public desk stays desk-bound and uses desk admission authority. Running its
   selected agent is not a second inbound message to the agent's direct address and must not be
   gated by `open_to_public`. Replies to that external sender retain the desk address and thread,
   with the existing audience and content-level checks; do not redirect them to a private direct
   thread. Direct mail to the agent is separately checked against the principal policy.

## Focused checks

- `direct_thread_`: a person's direct thread is invisible to a teammate and visible to the owner;
  an agent's is visible to the team; an outsider's mail to an agent bounces unless
  `open_to_public`; `settings_revision` refuses a message routed under a stale slug.
- `direct_binding_`: a binding on a person principal is refused; an agent binding admits a run for
  a direct message; a company-wide binding is still `company_wide`.
- `direct_public_settings_`: the API and settings form round-trip `open_to_public`; it defaults to
  false, is refused on non-agent principals and requires the spam acknowledgement when scanning
  is unavailable. Revoking it invalidates stale principal routing without changing desk access.
- `public_desk_private_agent_`: an external email to a public desk is admitted and answered by
  its bound agent with `open_to_public = false`, using the desk's thread and reply address; the
  same sender's direct email to that agent is refused. Opening only the direct address admits
  that mail but does not open a restricted desk. Preserve spam and external audience/egress
  refusals through both SMTP and signed provider-webhook ingress.
- `direct_competing_`: two concurrent messages to the same agent create one direct thread.
