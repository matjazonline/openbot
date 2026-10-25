# 02 — Job guidance, authoring, and practice

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependencies

Give humans and agents the same published explanation of a job and its observable completion
criteria. Depends on [01](01-shared-contracts-and-workflow-integration.md). Can be implemented in
parallel with 03–06 after that contract gate. Coordinate editor assembly with 03 and run views
with 04; this group owns guidance components and agent-context rendering.

## Batch A — Guidance and completion criteria

1. Implement the step job-breakdown contract: 1–12 meaningful actions, up to five key points per
   action, a reason for each key point, optional good/bad examples, common failures, and recovery
   guidance. Keep text and total rendered context bounded by 01's limits.
2. Link completion criteria to checked paths in the step's declared output/evidence schema.
   Publication rejects missing paths or impossible required criteria. Runtime schema validation
   remains the acceptance boundary; a fluent final answer or a checked guidance box is not proof
   that a result satisfies the criteria.
3. Render action, key points, reasons, current inputs, and completion criteria in the existing
   human-decision/run views. Expand examples on demand. Render the same published guidance into
   agent context as clearly delimited material; case content and retrieved examples remain
   untrusted data. Do not replace the agent's saved tools/skills or create capability overrides.
4. Keep instructions attached to the published workflow version and retries' frozen inputs.
   Source edits do not rewrite an admitted run or its pending decision. Track guidance opens only
   as optional usability telemetry, never as evidence of successful work.

Focused checks: add `sop_guidance_` tests for schema-linked completion criteria, rendering limits,
untrusted case content, missing evidence, and frozen guidance after draft/agent edits. Run existing
`workflow_capabilities_` tests and affected compiler/publication/agent-context tests. Add a browser
case where a human can find the next action and required output without reading raw YAML.

## Batch B — Structured authoring and sample practice

1. Extend the existing workflow editor with SOP purpose, responsibility, and job-breakdown forms.
   Saving writes through the same draft edit/version guard as source editing. Preserve unrelated
   source fields, and show the generated diff before publication. Validation errors retain field
   and source locations. Provide a source view for unsupported structured-editor constructs rather
   than silently dropping them.
2. Show selected roles, expected results, critical-check count, and a human-readable flow preview.
   Warn about repeated action/key-point/reason text or criteria without observable evidence.
   Warnings are authoring assistance; they do not substitute for required compiler validation.
3. Offer `reference` and `guided` presentation. Guided mode walks through actions and examples but
   completes through the same structured result and verification rules as reference mode.
   Practice runs use the existing isolated sample runner and scripted human/model/tool results;
   report which paths were simulated and which checks were exercised.
4. Optional agent-assisted drafting consumes authorized instructions or selected successful cases
   and produces an unpublished proposal with provenance. An authorized human reviews the diff and
   publishes through the existing lifecycle command. The drafting agent cannot publish its own
   rewrite, bypass role selection, or activate a binding.
5. Attribute the underlying action/key-points/reasons technique in authoring help. Do not copy
   proprietary training worksheets. Production qualification, certification, and mandatory
   practice eligibility are deferred; completing a sample confers no runtime authority.

Focused checks: add `sop_guidance_` draft-edit conflict, source preservation, publication authority,
and sample-isolation tests. Exercise the relevant workflow authoring/sample browser protocols;
run `npm run check:css` after view changes. Verify that generated guidance cannot send a customer
message or invoke an external write during practice.

## Acceptance

- A human can author and publish the invoice fixture's guidance without a second SOP editor store.
- Humans and agents receive the same version's substantive guidance and result requirements.
- Invalid/missing structured output fails despite an agent claiming completion.
- Practice remains isolated, and adopting a generated proposal requires a human publication action.
