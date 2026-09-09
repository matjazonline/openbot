# Step 11 — Reviewed Updates and Installation Lifecycle

## Outcome and dependencies

Offer global template improvements without overwriting company changes or altering active runs.
Depends on the installation/revision model, sample validation, activation, and management UI from
[steps 7–10](README.md).

## Reviewed updates

1. Compare an installation's adopted template version with eligible newer published releases.
   Surface available updates in company reads; a new global publication never changes installed
   resources, permissions, triggers, or active revision pointers. An update may be dismissed.

2. Implement a deterministic comparison using stable component/step/field keys and three inputs:
   the last adopted template/component content, current company content/candidate settings, and
   the proposed template release. For fields unchanged locally, propose the new template value;
   for unchanged upstream fields, preserve company changes; for changes on both sides, show an
   explicit conflict. Compare permissions, connection requirements, setup schemas, model/harness
   requirements, roles, and triggers as typed changes, not opaque JSON text diffs.

3. Make additions/removals explicit. Removing a step used by terminal success criteria or changing
   a result schema triggers normal SOP validation. Missing required setup fields require input.
   New tools, provider scopes, data grants, recipients, or automatic-start behavior always appear
   in the review. A renamed stable key is an add/remove unless the template supplies a bounded,
   validated migration mapping; V1 can require manual mapping rather than guessing by labels.

4. Create or update the installation's single draft revision and owned SOP draft. Keep active
   published content unchanged. Generate a new preview hash and sample requirement for executable
   changes. Every conflict resolution is a version-fenced company decision; an LLM may explain a
   diff later but is not the authority that adopts it.

5. Do not overwrite reused agents/skills/channels during an update. For a proposed change to such
   a component, let the manager keep the reused resource if compatible, select a different one,
   or create a new company copy for this installation. Even a resource originally created here
   may now serve another workflow: check current usage before modifying it. Prefer a new copy
   when the change would affect unrelated work. New copies receive reviewed IDs/slugs and remain
   inactive until the draft passes activation checks.

6. Apply approved resource changes and activation atomically through transaction-aware helpers;
   if creating draft-only copies earlier is necessary, mark their provenance and keep them
   inactive, with an explicit cleanup preview when the draft is discarded. Publish the resulting
   SOP/revision and switch the active pointer only after the unchanged candidate passes sample
   review and current authorization/readiness checks. Fence concurrent company edits with locked
   effective hashes/versions. Existing runs retain their pinned procedure/execution configuration.

7. Allow a manager to propose a new draft from local edits without upgrading the template release.
   The source template remains provenance, and the company revision number records the change.
   Editing a global source, company agent prompt, or company skill never updates production
   installation execution implicitly. Revocations and disabled resources remain live restrictions.

## Pause, rollback, archive, and removal

8. Pause changes admission state/generation and disables future starts. In-flight SOP runs and
   already queued deliveries continue under their existing contracts. The UI must state this;
   stopping active work invokes explicit SOP cancellation and existing unsent-delivery controls.
   Resume validates current readiness and advances generation; suppressed source intents do not
   restart automatically.

9. Implement rollback as proposing the content of an earlier installed revision in a new draft,
   followed by current validation, sample, review, and activation. Removed principals, revoked
   connections, and retired capabilities must not be restored as authority. Rollback affects
   future cases; it cannot undo a sent message or a completed business action.

10. Archive an installation only after active-run treatment is explicit: V1 requires its runs to
     finish or be cancelled first. Disable triggers and retain definitions, provenance, run pins,
     and evidence. Normal archive does not delete shared resources or disconnect accounts.
     A separate resource-cleanup preview may remove unused installation-created resources after
     dependency checks; never delete reused resources or cascade through other installations.

11. Make agent/skill/principal/connection deletion reference-aware. Preserve immutable snapshots
     and tombstones/restricted references as needed; surface affected active work through SOP
     exceptions. A disconnected shared account reports all affected authorized installations;
     reauthorization does not silently resume cancelled cases or expand previous scope grants.

## Verification and acceptance

- Test no local changes, only local changes, only upstream changes, conflicts, component removal,
  changed setup/result schema, incompatible harness, and a required scope addition.
- Race two adopters, adoption with a shared-agent edit, activation with pause, and rollback with
  connection revocation. One reviewed version wins; no mixed configuration becomes active.
- Keep a run active while adopting/rolling back and prove it executes its original configuration,
  including after worker restart, while live permission revocation still blocks its tools.
- Test archive/cleanup with resources used by other installations, standalone channels, schedules,
  active runs, and historical evidence. No unintended cascade or account revocation is allowed.
- Accept when template improvements can be adopted and reversed for future work with every company
  customization preserved or explicitly resolved.
