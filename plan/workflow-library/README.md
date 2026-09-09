# Business Workflow Library

## Outcome

Let a company select a business outcome, connect its tools, assign people and agents, review the
configuration, test it, and activate a company-owned workflow. A global workflow template packages
an SOP, agent presets, skills, integration requirements, setup fields, and trigger defaults.

This is an implementation plan, not a description of features already shipped. Each numbered file
is one implementation step with dependencies, changes, verification, and acceptance criteria.

```text
operator publishes immutable workflow package
  -> company completes setup and reviews a concrete preview
  -> atomic installation creates or links company resources
  -> sample run verifies the candidate configuration
  -> company activates a published installation revision
  -> manual action / inbound message / schedule starts an ordinary SOP run
```

## Existing foundations and prerequisites

The repository already contains:

- Global agents and skills, represented by `company_id = None`, with operator management in
  `src/adapters/http/routes/{agent_library,skill_library}.rs`.
- Company agent creation from a global definition through
  `AgentUseCases::create_agent_from_library`, including an owned personal channel; skill copying
  through `SkillManagementPersistence::copy_library_to_company`.
- Typed product-supplied agent definitions in
  `src/application/use_cases/builtin_agent_library.rs`.
- A fixed tool catalogue, capability loading, company model configuration, and harness adapters.
- Transport installations and channel bindings, scoped encrypted credentials, canonical messages,
  durable tasks and deliveries, approval/outreach handling, and interval/one-off schedules.

The [SOP foundation](../sop/01-procedure-execution-foundation.md) is a prerequisite for installation
and execution. Its immutable procedure versions, results, reviews, ownership fences, and recovery
must exist before dependent steps are enabled. The
[operational techniques plan](../sop/02-operational-techniques.md) is optional: the first package
uses the V1 SOP contract and does not depend on conditional branches, practice-required roles,
debriefs, or automatic improvement proposals.

The existing integrations are transport-shaped. Business connectors need their own small account
and capability boundary; a CRM must not become a `TransportKind` merely to reuse credential code.

## Delivery sequence

| Step | Implementation | Depends on |
| --- | --- | --- |
| 1 | [Architecture, ownership, and release scope](01-architecture-and-scope.md) | Existing contracts; SOP design |
| 2 | [Versioned workflow package schema](02-package-schema.md) | 1; SOP definition validator |
| 3 | [Persistence, provenance, and application ports](03-persistence-and-ports.md) | 2; SOP schema before linked migrations |
| 4 | [Global catalogue and publication](04-catalogue-and-publication.md) | 2–3 |
| 5 | [Business connectors and the first adapter](05-business-connectors.md) | 1–3 |
| 6 | [Setup drafts and installation preview](06-setup-and-preview.md) | 4–5 |
| 7 | [Atomic company installation](07-atomic-installation.md) | 6; SOP draft/publish APIs |
| 8 | [SOP execution bindings and sample runs](08-runtime-and-sample-runs.md) | 7; complete SOP execution/recovery |
| 9 | [Activation and automatic starts](09-activation-and-triggers.md) | 8 |
| 10 | [Library, setup wizard, and management UI](10-product-surface.md) | 4–9 |
| 11 | [Reviewed updates and installation lifecycle](11-updates-and-lifecycle.md) | 7–10 |
| 12 | [First workflow, verification, and rollout](12-first-workflow-and-rollout.md) | 1–11 |

Build the first package fixture alongside step 2 and use it throughout the sequence. Step 12 is
its release gate, not the first time an actual workflow is exercised. Backend steps expose their
application commands and narrow API routes as needed; step 10 completes the coherent user journey.

## Initial product decisions

- The library is curated by platform operators. Company users consume published versions and own
  their installed copies. Public submissions, billing, ratings, and a marketplace are deferred.
- One package contains one SOP and a bounded set of component presets. Several installations of
  the same template are allowed for different teams or channels.
- Reuse existing company agents, skills, channels, and connections when explicitly selected and
  compatible. Setup never silently changes a shared resource or broadens its permissions.
- The worked example is **Prepare customer quotes**, using a HubSpot connector for customer and
  product reads plus the existing email delivery path. HubSpot is a planning assumption for the
  first adapter, not a claim that every target company uses it. Capability names remain neutral.
- The first connector performs reads only. The workflow stores its reviewed quote in the company
  thread and sends through the existing delivery engine. Creating CRM quotes, invoices, payments,
  or arbitrary remote writes requires a later durable effect protocol and is outside this release.
- Manual start ships before automatic starts. New installations and triggers remain inactive until
  explicit activation. Sample runs cannot send customer messages or modify external records.
- Template updates are proposed, compared with company changes, tested, and explicitly adopted.
  Existing production runs retain their original procedure and installation revisions.

## Shared implementation rules

Use the repository and subsystem `AGENTS.md` rules. Keep pure validation in the domain, ports in
the application layer, and SQL/HTTP/provider code in adapters. Reuse existing task, SOP, approval,
notification, and delivery state owners; do not create a second workflow engine.

All proposed names and bounds in these files are implementation targets. Do not describe them as
working configuration options until the application reads and enforces them. Use additive
migrations; this plan does not authorize a database reset or inherit the Slack plan's reset premise.

Every SQL-bearing step must regenerate and commit `.sqlx/` metadata and pass the relevant database
tests. CI must keep formatting, offline compilation, migrations, database-backed tests, and the
stock-stack budget. New concurrency protocols require competing claimants, not only sequential
mocks. A raised bound must retain an early-failing regression or budget check in CI.

Verification commands for implementation, with `DATABASE_URL` supplied for the intended test
database and a migrated development database used when regenerating SQLx metadata:

```sh
cargo fmt --all -- --check
git diff --check
SQLX_OFFLINE=false cargo sqlx migrate run
SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets
SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets
SQLX_OFFLINE=true cargo check --locked --all-targets
SQLX_OFFLINE=true cargo clippy --locked --all-targets -- -D warnings
SQLX_OFFLINE=true cargo test --locked --all-targets
scripts/transport-boundary-check.sh
scripts/stack-budget.sh
```

Use isolated fixtures and a dedicated test database for tests. Provider contract tests use a local
stub in CI; real-account verification is a separate pilot check with documented results. This
documentation-only change does not require running the application test suite.
