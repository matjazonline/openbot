# Step 7 — Atomic Company Installation

## Outcome and dependencies

Create one inactive company-owned installation from a reviewed preview, with no partially copied
resource graph. Depends on [step 6](06-setup-and-preview.md), company agent/skill creation, and the
SOP foundation's draft persistence/publishing contracts.

## Implementation

1. Add `install_workflow` accepting company, setup draft, expected version, accepted preview hash,
   command UUID, and actor. Resolve authorization and normalize outside the transaction, then
   recheck all commit-sensitive facts under the lock order from step 1. A concurrent command with
   the same identity/content returns the original installation and mapping; changed content
   conflicts. Mark the setup draft consumed atomically so different command IDs cannot install
   the same draft twice. A second intentional installation begins a new setup draft.

2. Factor transaction-aware adapter helpers from current agent/skill/channel creation and library
   copy code. Reuse normalization, principal creation, tool checks, slug rules, and creation
   provenance. Do not call independently committing `create_agent_from_library` once per preset:
   it reads mutable global content and permits partial installation if a later copy fails.
   The install port consumes the frozen package and reviewed plan in one transaction.

3. Create company skill copies first, mapping package skill keys to IDs once. Several preset
   agents using one package skill receive the same company copy. Link explicitly reused skills
   without editing them. Then create new agents, their required principals/personal channels,
   capability associations, and package component mappings. A conflicting company slug is an
   explicit conflict, never an overwrite or unreviewed automatic rename.

4. New personal channels remain disabled. Installation does not create public transport bindings
   or external subscriptions. Validate a selected destination channel's access and show any later
   dispatch-policy change in the preview. A shared existing channel or reused agent's personal
   channel keeps its current behavior; installation creation cannot toggle it for unrelated work.

5. Create the company procedure and its draft version by compiling component IDs and validating
   through the ordinary SOP authoring path. Persist the draft installation revision, configured
   business inputs, role defaults, connection references/data restrictions, component provenance,
   and normalized adopted-content baselines. Role defaults are installation configuration;
   actual role bindings for a case are created by the SOP start transaction.

6. Commit resource creation, installation/revision, procedure draft, disabled trigger definitions,
   component mapping, setup consumption, command receipt, and installation event together. Any
   required notification is enqueued in the same transaction through the existing durable path.
   Do not call providers, generate prompts, provision memory remotely, or start agent work while
   holding the transaction. If existing creation hooks need external provisioning, enqueue their
   durable jobs atomically and expose pending readiness before activation.

   Disabled trigger definitions are typed configuration inside the draft revision at this stage.
   Step 9 creates operational trigger bindings/schedules only during reviewed activation.

7. Record origin using existing `CreationProvenance` plus normalized installation/component
   relations. Do not encode ownership in a prompt or agent name. Created resources remain ordinary
   company resources with normal editing/authorization controls. A later edit marks the draft
   configuration changed and invalidates its preview/sample evidence before publication.

8. Return installation ID, revision ID, procedure ID, typed created/reused resource mapping, and
   readiness findings. The manager can inspect the installed draft immediately. All public start
   paths must reject its unpublished procedure/configured installation until activation; a task
   cannot bypass this by presenting a procedure ID directly.

## Failure and recovery

Local installation has no long-running lease: it is a bounded database transaction. A lost HTTP
response is recovered through the command receipt/setup-consumption record. Account linking
already completed in step 5 is independent and may remain after an installation failure; it must
not be silently disconnected, because it could serve another workflow.

Use company quotas and database bounds during copying. Reject a package exceeding the transaction
resource budget before work begins. Do not replace atomic creation with an unbounded partial-copy
worker to avoid enforcing that budget.

## Verification and acceptance

- Race identical installs, different commands consuming one setup, installations with colliding
  slugs, and two installations approaching the company quota.
- Race installation with a reused-agent edit, principal removal, connection revocation, and
  template retirement; either produce the reviewed configuration or reject stale state.
- Inject failure after each write phase and prove no partial agents, channels, skills, procedure,
  trigger configuration, mappings, or notifications remain. Simulate commit success with a lost response.
- Prove several agents share one intended copied skill, and two installations cannot mutate each
  other's reused or created components through their mapping IDs.
- Accept when repeating the install request produces one inactive installation with a complete
  company-owned resource graph and no external business effect.
