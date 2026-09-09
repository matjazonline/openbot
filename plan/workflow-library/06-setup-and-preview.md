# Step 6 — Setup Drafts and Installation Preview

## Outcome and dependencies

Turn a selected published package into a concrete, reviewable company configuration before creating
agents or activating work. Depends on [publication](04-catalogue-and-publication.md) and
[connection readiness](05-business-connectors.md).

## Implementation

1. Add company-authorized start/edit/validate/preview setup commands. Bind a draft to one immutable
   template version. Every mutation carries company, command ID, expected draft version, and actor.
   An expired/retired-version draft may be inspected but must be refreshed before installation.

2. Gather destination channel, installation name/slug, human coordinator, role assignments,
   business setup fields, connection slots, model/harness choices, and trigger settings. Validate
   every selected resource with the active company and applicable channel/thread permission.
   Principal selection must obey existing role-assignment and schedule run-as authority.

3. Offer `CreateFromPreset` and `ReuseCompanyResource` for compatible agent/skill slots. Load
   company resources through bounded authorized pickers. Compatibility checks include harness,
   model access, tool requirements, skill content, data scope, role eligibility, and execution
   budgets. A same-name or same-slug resource is not automatically compatible.

4. Reusing an agent creates a binding, not a permission/configuration edit. Show its current
   instructions, skills, and extra capabilities and the narrower capabilities allowed inside this
   workflow. If required capability is absent, offer a separate explicit company-agent edit or a
   new agent; do not silently attach a skill or tool. Reusing a skill selects its actual current
   content and hash as a company customization, never pretends it matches the preset.

5. Resolve a new agent's model/provider through current company configuration and validate the
   effective pair. A suggested model unavailable to the company may produce an explicit alternative
   in the preview. Installation cannot silently fall back after review; changed availability
   produces a stale/incompatible preview. Propagate provider/configuration lookup errors instead
   of converting an outage into a successful fallback.

6. Implement a pure compiler from package plus validated selections to `WorkflowInstallPlan`:
   normalized company SOP draft, proposed agent/skill/channel writes, references to reused
   resources, role/connection bindings, effective tool ceilings, disabled triggers, resolved model
   configuration, resource counts, and blocking/readiness findings. Use package component keys
   as logical identities; assign stable new-resource UUIDs once in the setup draft so retries
   produce the same mapping. Ordinary agent/skill/SOP validators remain authoritative.

7. Generate a server-owned preview hash covering package bytes, setup draft version, compiled
   configuration, selected resource hashes/versions, account identity/scopes/data grants,
   effective model policy, and created-resource IDs. Readiness timestamps and volatile token
   ciphertext are excluded; relevant status/grant generations are included. Store the preview
   and its bounded findings for 30 minutes. Credential rotation alone must not invalidate it.

8. Present the exact proposed changes: resources created/reused, assigned people, business
   information, accessible CRM records, outgoing-message review policy, destination channel,
   trigger behavior, and unresolved requirements. Explain that installation creates inactive
   configuration, while activation admits work. Do not expose technical hashes as setup questions.

9. Recheck authorization, slug availability, quotas, selected-resource hashes, and connection
   status at installation commit. Where an existing resource lacks an edit version, compare its
   canonical effective configuration under a row lock; do not treat `updated_at` alone as a fence.
   External readiness is advisory and checked again at runtime, not a permanent grant.

## Suggested application surface

Use `start_setup`, `save_setup`, `validate_setup`, and `preview_installation` commands with typed
request/result structures. HTTP adapters may expose them under company-scoped
`workflow-setups/{id}` routes. Preview is read-only regarding agents, SOPs, schedules, and provider
business records; OAuth linking is a separate explicitly requested account operation.

## Verification and acceptance

- Test mismatched company IDs for every role, agent, skill, channel, account, and model binding.
- Test occupied slugs, changed shared-agent content, missing model connections, expired setup,
  removed role principals, and revoked/scopeless CRM connections.
- Prove previews create no agents, SOP runs, tasks, schedules, or customer messages.
- Test two draft editors and an edit racing with preview: an old preview cannot approve new data.
- Prove reusing an agent with extra tools cannot add those tools to a workflow's effective scope.
- Accept when the manager can review all consequential configuration and an install command can
  reproduce it exactly or return a specific stale-preview conflict.
