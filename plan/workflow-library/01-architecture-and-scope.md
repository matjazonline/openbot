# Step 1 — Architecture, Ownership, and Release Scope

## Outcome and dependencies

Fix the boundaries before adding schema or UI. Read the
[SOP foundation](../sop/01-procedure-execution-foundation.md), agent/skill copy paths, integration
ports, schedule materialization, and task ownership contracts. Record any missing prerequisite
as work in its owning subsystem, not as a temporary implementation inside the library.

## Implementation

1. Introduce the following vocabulary in domain/module documentation:

   | Concept | Owns |
   | --- | --- |
   | `WorkflowTemplate` | Global identity, catalogue listing, and retirement |
   | `WorkflowTemplateVersion` | Published package bytes, schema version, and content hash |
   | `WorkflowSetupDraft` | A company's unfinished selections and reviewed preview |
   | `WorkflowInstallation` | Company-owned identity, coordinator, channel, and admission state |
   | `WorkflowInstallationRevision` | Configured package, role defaults, execution bindings, and procedure-version reference |
   | `ProcedureRun` | Progress of one business case; always the SOP engine's state |
   | `BusinessConnection` | One company's provider account, authorization status, and supported capabilities |

2. Define the installation lifecycle as `configured`, `active`, `paused`, or `archived`.
   `configured` has no production admission. `active` admits against its current published
   revision. `paused` stops new admissions; existing runs continue. `archived` is terminal for
   admission and retains history. Failed health checks are typed readiness findings, not another
   copy of task/run status. Cancelling existing runs remains an explicit SOP action.

3. Define revision states `draft`, `published`, and `retired`, with at most one draft per
   installation. Published/retired content is immutable. An active installation may have a draft
   update while production continues on the published revision. Each installation owns one
   company production procedure so the one-draft rule agrees with SOP versioning. Isolated
   sample procedures from step 8 are test-only artifacts, not additional production procedures.
   Record installation ownership on the procedure: ordinary SOP edit/publish/start routes must
   delegate to installation commands for these procedures, preserving revision/activation fences.

4. Preserve these state owners: SOPs own steps/results/reviews, tasks own executable work and
   leases, approvals own protected-action authorization, deliveries own outbound attempts,
   schedules own time slots, and existing attention/notification projections show actionable
   work. Installation events cover configuration and lifecycle only.

5. Define authorization separately for global publication, company configuration, resource
   selection, run initiation, and run inspection. Global authoring is operator-only. Company
   owner/admin management follows existing membership rules. Team members may browse published
   packages and start configured workflows only through authorized channels/threads. A global
   operator role must not implicitly grant access to company credentials or case content.

6. Require company-owned agents for installed role bindings. Global agent/skill rows may supply
   frozen presets at publication; installed runs never execute by dereferencing a mutable global
   definition. Keep existing standalone agent-library behavior available.

7. Establish one configuration lock order for install/update/activation: command identity, company
   authorization/quota guard, template version when needed, installation, revision/setup draft,
   selected resources in stable type/ID order, then procedure. SOP run transitions keep their own
   run-first order and never acquire installation locks afterward. Admission takes installation
   locks before creating the new run. Document the provider-revocation ordering in step 5.

8. Put new domain types under `src/domain/entities/workflow_library/`; orchestration and ports
   under `src/application/use_cases/workflow_library/`; persistence under
   `src/adapters/persistence/workflow_library/`. Keep connector modules separate and factor
   shared validation/transaction helpers out of existing large modules where required.

## Scope controls

The initial release has no package scripts, arbitrary expressions, executable downloads, recursive
package dependencies, global runtime agents, automatic template adoption, or generic tool-server
installer. Packages declare requirements that implemented platform capabilities satisfy.

Do not add accounting/CRM writes under the read connector. Such writes need canonical operation
payloads, approval binding, stable idempotency, ambiguous-outcome reconciliation, and their own
competing-dispatcher tests before a package can advertise them.

## Verification and acceptance

- Review each proposed table/worker/state against the ownership table; remove duplicated state.
- Confirm that a global edit, template retirement, installation pause, and run cancellation have
  distinct effects that can be expressed without changing an unrelated subsystem's state machine.
- Check authorization scenarios for cross-company component IDs and restricted threads.
- Accept when the first workflow can be traced from package selection to an ordinary SOP run,
  with an explicit owner for every durable fact and no circular dependency between steps.
