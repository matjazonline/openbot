# Step 2 — Versioned Workflow Package Schema

## Outcome and dependencies

Implement a typed, bounded package format after [step 1](01-architecture-and-scope.md). Use the SOP
definition validator from the SOP foundation; the package must not introduce a second graph format.

## Implementation

1. Add validated newtypes for template slug, component key, role key references, connection slot,
   capability key, setup field key, schema version, release number, and content hash. Parse status
   and policy enums at boundaries. Generic display names and explanations remain free text.

2. Add `WorkflowPackageV1` with these typed sections:

   | Section | Required content |
   | --- | --- |
   | Identity | Stable slug, integer release, title, summary, business outcome, change summary |
   | Compatibility | Package/SOP schema versions, implemented feature IDs, supported harness kinds |
   | Procedure | One supported SOP definition plus explicit bindings from SOP roles/steps to component keys |
   | Agent presets | Stable keys, frozen allowed agent configuration, skill keys, tool requirements |
   | Skill presets | Stable keys and full validated skill instructions |
   | Connection requirements | Slots, capability versions, required/optional operations, resource restrictions |
   | Setup schema | Bounded typed business inputs, defaults, sensitivity, help, and approved customization points |
   | Role defaults | Human/agent eligibility and suggested agent preset; no tenant principal IDs |
   | Trigger presets | Tagged manual, inbound-channel, or schedule specifications; disabled defaults |
   | Examples | Synthetic run inputs, expected result schemas, and review guidance |

3. Reuse `AgentWrite`/`SkillWrite` normalization through pure shared validators, with a package
   allowlist for agent fields. Exclude credentials, backend endpoints, arbitrary `config_json`,
   native command/network privileges, tenant IDs, and provider connection IDs. The package may
   request a supported harness/model capability; company setup resolves an actual enabled model.
   Freeze any supported harness configuration as a typed value, not an unchecked JSON object.

4. Permit drafts to reference existing global agents and skills as authoring sources. Publication
   resolves them once into package-local presets and records source IDs/hashes for attribution.
   Source IDs are informational after publication; deleting or editing a library row cannot
   change the package. Deduplicate skills by component key, never by title or slug alone.

5. Keep company setup inputs separate from per-case SOP start inputs. Use a finite binding enum
   such as `SetupField`, `RoleSlot`, `ConnectionSlot`, and `ComponentRef` for declared destinations.
   Bind into typed SOP/agent fields through a compiler. Reject raw JSON-pointer patches, expression
   evaluation, and string substitution into executable configuration. Business text reaches prompts
   as delimited data. A package cannot use a field value to grant tools or choose an approver.

6. Compile the procedure's skill references to placeholder component identities for validation,
   then to company skill IDs at installation. Run the same SOP graph/result/role validators in
   both stages. V1 packages containing SOP V2-only behavior must be rejected until the server
   implements and advertises it. Unknown fields and unsupported capability versions fail closed.

7. Compute a canonical hash of the complete executable package, including every frozen preset,
   schema, trigger requirement, and compatibility field. Use canonical serialization with sorted
   object keys and preserved ordered arrays. Keep listing artwork and editorial annotations out
   of executable configuration and disallow remote assets in executable content.

8. Start with the following proposed application-enforced ceilings, plus the tighter existing
   agent, skill, and SOP bounds: 2 MiB package bytes; 8 agents; 32 skills total; 8 connection slots;
   32 setup fields; 4 trigger presets; 5 examples; 64 KiB configured setup data; 100 catalogue items
   per page. Bound aggregate prompt/configuration size as well as each component. Emit clear
   publication errors identifying the exceeded bound. Add fixtures immediately below and above
   each boundary; raising a ceiling must update the resource-budget check and explain why.

## First fixture

Validation accepts an explicit capability registry. Early pure tests use fixture implementations;
production publication/installation require registered executable adapters. Thus catalogue code
can land in step 4, but the CRM package cannot be released until step 5 supplies its capabilities.
Use prompt-only skill recipes and declared capability requirements until their concrete tool IDs
are registered; do not bypass the existing `Skill` tool validator with made-up tool IDs.

Add a synthetic `prepare-customer-quotes` package fixture from the start. It has one quote agent,
one human reviewer/coordinator role, CRM customer/product read requirements, a reviewed email
delivery step, currency/terms setup, and a manual trigger. Use stable component/step keys across
fixture releases. A second fixture changes a prompt and a required capability to exercise upgrades.

This fixture is authored against the concrete
[first-workflow contract](12-first-workflow-and-rollout.md), without waiting until rollout to find
that the package cannot express the intended business case.

## Verification and acceptance

- Test canonical round-trips/hashes, unknown fields, bounded nesting, duplicate keys, missing
  components, unbound roles, invalid DAGs, unsupported tools/harnesses, and invalid result schemas.
- Prove source library edits cannot affect a published fixture's bytes or validation result.
- Prove setup data cannot inject an endpoint, expand tools, change recipients, or replace approval
  authority through interpolation.
- Accept when the complete example compiles to a valid SOP and explicit component requirements
  without SQL, HTTP, a provider call, or a harness-specific type in the domain.
