# Attribution service p01 — boundaries and contracts

## Identity and status

- Request: implement `plan/attribution-service` using `implement-astra-reviewed-2-3`;
  the user states root is xhigh. The entire ordered plan is authorized.
- Original phase requirements: `../../../01-application-boundaries-and-contracts.md`.
- Workflow: `/Users/mac02/orca/workspaces/openbot/attribution-service/.agents/skills/implement-astra-reviewed-2-3/SKILL.md`.
- Record root: `/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/phases/p01/docs`.
- Root: `/root`; effort xhigh (user-confirmed); actual model not exposed by the host.
  Local config names `gpt-6-astra`/xhigh, which is a default, not proof of this
  running session's model. Model confirmation or separate author selection is pending.
- A: not assigned. B: not spawned. Requested settings are Sol 6.1/xhigh author
  and independent Astra/high reviewer, with no inherited reviewer conversation.
- Effective context capacity, occupancy, and compaction threshold: unknown;
  use host defaults and durable records. No global settings changed.
- Worktree: `/Users/mac02/orca/workspaces/openbot/attribution-service`.
- Branch: `attribution-service`.
- Starting commit, prospective review base, and current HEAD:
  `2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`.
- Preserved pre-existing changes: none; worktree was clean at entry.
- Snapshot exclusions: exactly `plan/attribution-service/phases/p01/docs` and
  `plan/attribution-service/COORDINATION.md`; workflow records only.
- Current checkpoint digest: `0a3f44130cada7b6467926621da786841d0af591c1d52fd94ffdfb946080305f`;
  [snapshot](logs/AS-P01-discovery.snapshot.txt). This is not a reviewed artifact.
- Coordination authority: `/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md`, revision 1 read/acknowledged.
  Applicable notices: none. Pending cross-phase requests: none.
- Prerequisites: phase 01 has no earlier phase dependency. Product decisions and
  workflow role resolution below are pending. p02–p10 remain unstarted.
- Implementation: not started. Review: not started. Verification: discovery only.
  Accepted in phase: no. Integrated: no.

## Owned scope and resources

Phase 01 owns boundary contracts, authentication sequence, identity/type and
tenant rules, concrete reconciliation examples, and resolved pilot decisions.
Root currently owns coordination/TASK/HANDOFF and prerequisite recording only.
No application code, schema, credentials, sessions, database, or deployment changed.
No worker processes, test databases, ports, containers, or build caches allocated.
No commits, pushes, production resets, or deployment are authorized by the skill.

## Existing behavior traced before implementation

- `src/adapters/http/session.rs:94–177` issues Openbot-local signed cookies,
  stores session records, and validates liveness before returning a verified
  actor. Cookies and signing secrets cannot become cross-application credentials.
- `src/adapters/http/routes/user.rs:296–364` consumes a Google login flow and maps
  external identity through the existing user use cases. `trail grep 'oidc'`
  found no OIDC implementation in indexed code. Existing Google/Apple OAuth
  login support does not establish an issuer suitable for attribution SSO.
- `src/domain/entities/company_member.rs:63–87` defines owner, admin, member,
  and no membership. Existing management permits owner/admin. This is evidence
  for a future permission mapping, not approval of attribution permissions.
- `src/application/use_cases/company.rs:190–246` owns readable/owner/manager
  access. Missing/foreign company is 404, insufficient known membership is 403,
  and persistence errors propagate. Reuse this authority through its owning
  application API, not by reading Openbot tables from attribution.
- `src/adapters/persistence/company.rs:241–260` performs an uncapped company/user
  membership lookup. The company selector has a separate cap; it cannot be an
  authorization lookup. The existing regression is
  `company_access_reaches_the_oldest_of_201_companies_outside_the_switcher_cap`.
- `src/adapters/persistence/company.rs:104–138,311–316` locks the company,
  performs integration and memory cleanup, deletes, and commits. Any durable
  attribution lifecycle delivery must join the owning transaction and survive
  deletion; it cannot be a post-commit HTTP callback with no outbox.
- Caller tracing for `delete_for_user` reaches company HTML/JSON deletion and
  UI deletion through `CompanyUseCases::delete_company_for_user`. The graph has
  ambiguous `managed_company` definitions, so exhaustive `trail grep` was used;
  existing workspace, agent, channel, skill, connection, MCP, and local-agent
  consumers depend on current management semantics.
- `services/` currently contains `memory/`; attribution does not yet exist.

## Batches and architecture gate

Implement phase 01 as one cohesive contract batch, covering 01A and 01B. Before
acceptance, freeze the selected identity proof/permission, lifecycle ordering,
merchant identity/deduplication, durability, and privacy/retention contracts in a
decision revision for B. No gate decision is approved yet. Do not start p02 on
an unaccepted p01 artifact.

## Pending decisions

The original phase file contains the authoritative decision register. Required
user input was requested asynchronously on 2026-11-15:

1. Confirm root is Sol 6.1/xhigh, or select a separate Sol 6.1/xhigh author.
   The skill requires an answer before spawning any agent; elapsed time is not
   confirmation. Existing user confirmation already establishes effort only.
2. Identify the real checkout platform/store and available sandbox/pilot setup.
3. Select the maintained SSO arrangement and mapping to Openbot accounts; existing
   Openbot sessions alone cannot supply standards-based cross-app SSO.
4. Resolve pilot region, consent/placement policy, volume/latency/error targets,
   retention and suspension/deletion deadline, and explicitly confirm the 503
   durable-redirect failure behavior required by the plan.
5. Agree the role-to-attribution-permission mapping and domain-control proof
   policy before their respective contracts are implemented. No grant is inferred
   from an arbitrary client-supplied user/company ID.

No provider, privacy policy, account-ownership change, or operating target has
been silently selected. These are missing material requirements, not a request
to reauthorize routine implementation.

## Acceptance mapping

| Criterion/source | Production path and case | Decisive assertion | Check/evidence | Status |
| --- | --- | --- | --- | --- |
| AC1 / 01A.1–3 | SSO, authority API, lifecycle API, public/merchant ingress | Wire contracts bound each credential to its audience/purpose and define bounded errors/retries | OpenAPI validation and `attribution_contract_` cases in both owners; command selected after tooling exists | NOT EXERCISED |
| AC2 / 01A.4 | Tenant/store-scoped read/write and nested objects | Two companies/two stores and multi-company actor cannot substitute foreign IDs or body actor IDs | Nonzero contract fixture selection plus negative examples | NOT EXERCISED |
| AC3 / 01B | Real merchant and SSO arrangement | Selected protocol supplies stable order/refund/checkout identities and account-bound identity proof | Provider contract review and selected sandbox evidence | BLOCKED on choice |
| AC4 / 01B | Ad A then B, two sold products, line refund | One paid order stays on B; merchandise/tax/shipping reconcile separately | Concrete paper ledger, then independent assertions in later phase tests | NOT EXERCISED |
| AC5 / 01B | Pilot durability/privacy/operations | Explicit failure/consent/retention/volume/latency/revocation values have responsible owner | Approved decision register and B architecture review | BLOCKED on choices |

## Contract impact

| Contract | Producers/consumers/equivalent surfaces | Inherited obligations/disposition | Focused checks |
| --- | --- | --- | --- |
| Identity proof | Openbot user/session owner → attribution callback/session → company authority | Account-bound proof; no cookie or secret sharing; current revocation; no caller actor substitution | SSO replay/claims and affected existing session/revocation tests in p03 |
| Company permission | Existing company use cases → new authority API → attribution operations | Keep existing read/manager/owner semantics; active company binds nested resources even for multi-company actors | Contract denial fixtures; company access regressions in p03 |
| Company lifecycle | Existing company transaction → durable notification → attribution tombstone/status | Ordered/idempotent; delete cannot revive; outage freshness bound; no distributed transaction | Competing delete/provision/lifecycle cases in p03 |
| Merchant identity/money | Selected provider → store-scoped ingress → immutable order/refund facts → reports | Distinct event/order/refund identities; exact minor units; durable acknowledgement | Contract ledger in p01; provider/race/analytics tests in p06–p08 |

## Verification plan and results

- Discovery used `trail map`, literal `trail ask`/`trail skeleton`, `trail callers`,
  and exhaustive `trail grep` before opening the referenced source ranges.
- Before changing source, finish reading applicable source-level AGENTS.md files;
  the initial combined read was truncated and is not sufficient for implementation.
- Per batch: list intended focused tests first, assert a nonzero selection, run
  those tests against phase-owned fixtures, and retain commands/counts/results.
- Contract batch: validate both APIs, credential/tenant examples, and concrete
  ledger; record unexercised provider decisions explicitly.
- Final p10 gate: complete both applications' current CI, formatting, locked
  offline compilation, migrations/SQLx metadata, real database suites, browser,
  contract, extraction/container, network isolation, recovery, and pilot checks.
  Reconcile commands with live CI first; never count root tests as service tests.
- No Rust/database/browser/provider test has been run or claimed as passed.
  Current changes are workflow and prerequisite documentation only.
- Documentation audit (inline Python, 2026-11-15): PASS; ten consecutively ordered
  phases, all existing reset notes and startup registrations intact, 37 local plan
  links resolve, and three workflow records present. `git diff --check`: PASS.
- `trail check`: exit 0, no conflicts. Its advisory matches “account/company”
  against an unmerged native-effect locking learning; this documentation change
  does not touch that locking protocol, so the advisory is not applicable.
- Initial skill snapshot command: FAIL on macOS BSD `realpath -m`. Retried the
  unchanged skill script with installed GNU coreutils first in the command-local
  PATH (`/opt/homebrew/opt/coreutils/libexec/gnubin`): PASS; snapshot linked above.
  No skill or machine configuration changed.
- No review evidence manifest exists because no implementation review was
  assigned or completed. No zero-match test or documentation check counts as
  fulfillment of behavioral acceptance criteria.

## Next action

Resolve role and material phase-01 choices from pending user answers. Then assign
the confirmed author and independent reviewer per skill, finish the full contract
batch, review/accept its exact artifact, and proceed sequentially through p02–p10.
