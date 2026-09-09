# Step 10 — Library, Setup Wizard, and Management UI

## Outcome and dependencies

Expose the completed company journey using the commands from
[steps 4–9](README.md). Keep the existing agent and skill libraries useful for standalone creation;
the workflow library is the outcome-oriented entry point.

## Implementation

1. Add a company-visible **Workflow library** page with bounded search/filtering. Each item shows
   the business outcome, required tools, required human involvement, supported start methods,
   sample output, and compatibility/readiness findings. Avoid promised time savings or provider
   support not verified by the adapter. Show retired releases only in relevant history views.

2. Add an operator authoring page or structured import/edit surface for the same package schema.
   Reuse global agent/skill source pickers and validation errors from step 4. Published releases
   are read-only; creating a new release begins a draft. Tenant browsing does not expose operator
   controls or unpublished instructions.

3. Build the resumable setup wizard around the persisted setup draft:

   1. Review the workflow and choose its destination/name.
   2. Connect/select business accounts and review data access.
   3. Supply business defaults and assign people/agents, with create/reuse choices.
   4. Configure start methods and review resources, permissions, and behavior.
   5. Install inactive configuration, run the sample, and inspect its results.
   6. Activate the reviewed revision.

   The install and activate actions describe their actual effects. OAuth account linking may finish
   before installation; the wizard explains that the account can be reused elsewhere. Setup
   errors preserve valid selections and point to the field or capability needing attention.

4. Add an **Installed workflows** view with state, active revision, coordinator, destination,
   connected account health, trigger settings, recent run outcomes, and update availability.
   Show runs from SOP projections, exceptions from their authoritative sources, and notification
   counts from existing notifications. Do not create another work queue or duplicate run statuses.

5. Add **Start workflow** to authorized thread actions and installation detail. Generate per-case
   forms from the published SOP input schema. Distinguish workflow defaults from case inputs and
   display role changes before starting. Reuse the SOP run panel, result forms, evidence links,
   review controls, task transfer, and cancellation rather than building library-specific copies.

6. Add draft-change indicators for modified installed components. Clearly distinguish active
   production configuration, editable company resources, and the candidate revision awaiting
   sample/review. Provide preview/update/pause/archive controls with the exact scope defined in
   step 11. Shared resource usage is visible before any separate agent/skill edit.

7. Use existing `/ui` shell/components and the applicable HTTP/pages guides. Escape package and
   provider text; do not render arbitrary package HTML. Keep secrets in dedicated connection
   actions. Show progress after a form is accepted, preserve error state, and prevent duplicate
   submissions without interrupting an accepted write. Use durable command IDs across retries.

8. Recheck company and channel/thread authorization on every list/detail/event/preview/mutation
   route and every reused-resource picker. Use normal CSRF/origin controls for browser mutations.
   Treat SSE as an identifier-only wake-up, reload authorized projections on connect/reconnect,
   and keep its owning element mounted during partial updates. Bound history and catalogue pages.

## Verification and acceptance

- Exercise the complete journey with a first-time company and with a company reusing agents,
  skills, and a connection. Verify disabled installation, sample review, and activation are visible.
- Test interrupted setup/OAuth navigation, reload after successful install with a lost response,
  stale preview, occupied slug, missing scope/model, failed sample, and expired draft.
- Test users with member/admin/owner/operator roles and valid IDs belonging to another company or
  restricted channel on every new route.
- Test repeated clicks and out-of-order partial reads. If SSE is added, cover an event between
  initial render and subscription and reconnect after missed events.
- Check keyboard navigation, labelled validation errors, light/dark themes, and narrow screens.
- Accept when a manager can install, test, activate, inspect, and pause the first workflow without
  editing JSON or knowing package hashes, tool IDs, or database terminology.
