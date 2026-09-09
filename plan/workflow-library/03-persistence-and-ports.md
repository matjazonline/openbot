# Step 3 — Persistence, Provenance, and Application Ports

## Outcome and dependencies

Persist global packages, company setup, and installed revisions after
[step 2](02-package-schema.md). Migrations linking procedures depend on the SOP foundation schema.
Introduce only the tables needed by these owners; connector and trigger steps add their own rows.

## Implementation

1. Add global `workflow_templates` and `workflow_template_versions`. A template owns its slug,
   listing, current published release, and archive marker. A version owns its release number,
   draft edit version, lifecycle state, schema version, bounded package JSONB, canonical hash,
   author, publication time, and change summary. Enforce unique template slug and release number,
   at most one draft per template, and database immutability of published content. Retirement may
   change lifecycle fields through an audited command; it cannot change the frozen package.

2. Add `workflow_setup_drafts` with company/author/template-version IDs, expiry, monotonic version,
   bounded non-secret selections, and reviewed-preview hash. Store connection references, never
   tokens. Selections are not executable resources. Default expiry is 7 days, with a maximum of
   20 live drafts per company; enforce both and use bounded cleanup without deleting audit data.

3. Add `workflow_installations` with company, name/slug, source template, owned procedure,
   destination channel, coordinator, lifecycle, active published revision, monotonic version,
   creator, and timestamps. Enforce company-scoped slug uniqueness, not one installation per
   template. Define a nullable active revision for `configured`; `active` requires a published
   revision. Archive retains pointers required by history.

4. Add `workflow_installation_revisions` with company/installation IDs, revision number, source
   template version, draft edit version, lifecycle, associated procedure version, validated setup,
   accepted preview hash, execution configuration, and timestamps. One draft per installation;
   published content is immutable. Normalize role and connection bindings where referential
   integrity and lifecycle lookup need them; bounded typed configuration stays in JSONB.

5. Add `workflow_installation_components` keyed by company, revision, component kind, and stable
   package component key. Record whether each resource was created for this installation or reused,
   its source component hash, and the last adopted normalized content for update comparison.
   Use typed nullable target columns with an exactly-one-target constraint and composite tenant
   foreign keys, or separate typed component tables. Do not use unchecked `(kind, resource_id)`
   polymorphic references. Preserve resource history through tombstones or restricted deletion.

6. Add scoped command receipts and append-only events. A command has UUID, actor, operation kind,
   normalized fingerprint, result IDs, and timestamp. Same identity/content returns the original
   result; changed content conflicts. Global publication receipts/events have global scope;
   installation commands/events have explicit company scope. Do not use nullable tenant fields
   as an authorization shortcut. Lifecycle events reference IDs/hashes and safe reasons only.

7. Make tenant identity part of every foreign key to company resources. Verify installation,
   revision, procedure version, channel, principal, and component association agree on company
   and parent. Add deferred constraints where an atomic installation needs mutually referring
   rows. Published procedure/revision correspondence must be checked in the commit path and
   database constraint triggers where ordinary foreign keys cannot express it.

8. Define cohesive application-owned ports: `WorkflowCataloguePersistence`,
   `WorkflowSetupPersistence`, `WorkflowInstallationCommit`, and `WorkflowLibraryReader`.
   Commit methods accept validated command structures and perform one logical transaction.
   Correctness methods have no default implementation. SQLx transactions stay in adapters.

9. Add indexes from concrete queries: published catalogue pages, company installations, an
   installation's revision history, live setup drafts by expiry, and component/connection users.
   Use keyset pagination and inspect representative query plans. Enforce an initial maximum of
   100 non-archived installations per company behind a locked quota check; later increases need
   an updated budget fixture. Keep historical rows pageable without truncating active-run pins.

## Retention and compatibility

Use additive migrations and regenerate `.sqlx/`. Do not rewrite deployed agent/skill schemas or
copy existing company data into global templates. Published versions referenced by installations
and runs remain readable after catalogue retirement. Command deduplication records for durable
installation/start identities cannot be pruned while a supported retry can recreate their effects.

Deletion policy must preserve tenant deletion requirements without allowing component deletion to
cascade into another installation or unrelated channel. Normal removal is archive; any eventual
hard purge operates only after active references and retention requirements are satisfied.

## Verification and acceptance

- Apply migrations to an empty database and a database populated with current agents, skills,
  channels, installations, and schedules; existing behavior remains valid.
- Test invalid cross-company links and wrong-parent revision links with rollback-only inserts.
- Test two writers creating the same draft/release and two commands racing at a company quota.
- Test published-content/event immutability and source-template retirement with retained reads.
- Regenerate metadata and pass the shared formatting, offline compilation, migration, and
  database checks in the [README](README.md).
- Accept when these invariants survive direct SQL misuse as well as application-level validation.
