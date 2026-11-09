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

## Focused checks

- `direct_thread_`: a person's direct thread is invisible to a teammate and visible to the owner;
  an agent's is visible to the team; an outsider's mail to an agent bounces unless
  `open_to_public`; `settings_revision` refuses a message routed under a stale slug.
- `direct_binding_`: a binding on a person principal is refused; an agent binding admits a run for
  a direct message; a company-wide binding is still `company_wide`.
- `direct_competing_`: two concurrent messages to the same agent create one direct thread.
