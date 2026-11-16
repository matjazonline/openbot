# Plan coordination: attribution-service

- Authority: `/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md`
- Writer: `/root`, current Codex session in `/Users/mac02/orca/workspaces/openbot/attribution-service`
- Revision: 16

## Plan-wide instructions

Execute the ten numbered phases in order with `implement-astra-reviewed-2-3`.
Root is the Sol 6.1/xhigh author, confirmed by the user on 2026-11-15; independent
review is Astra/high without inherited conversation. No source acceptance or
production deployment is implied by these role confirmations. Existing reset and
final-verification requirements remain in force.

## Phase directory

| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |
| p01 | `/root`; accepted at f61d9149, B AS-P01-B1-N2 | `/Users/mac02/orca/workspaces/openbot/attribution-service` | `phases/p01/docs/TASK.md`, `phases/p01/docs/HANDOFF.md` |

| p02 | `/root`; accepted f4421ac9, B AS-P02-B1-N2 | `/Users/mac02/orca/workspaces/openbot/attribution-service` | `phases/p02/docs/TASK.md`, `phases/p02/docs/HANDOFF.md` |

| p03 | `/root`; accepted fd40f480, B AS-P03-B1-N2 | `/Users/mac02/orca/workspaces/openbot/attribution-service` | `phases/p03/docs/TASK.md`, `phases/p03/docs/HANDOFF.md` |

| p04 | `/root`; accepted a5e729a6, B AS-P04-B1-N2 | `/Users/mac02/orca/workspaces/openbot/attribution-service` | `phases/p04/docs/TASK.md`, `phases/p04/docs/HANDOFF.md` |

| p05 | `/root`; accepted a03abfea, B AS-P05-B1-N1 | `/Users/mac02/orca/workspaces/openbot/attribution-service` | `phases/p05/docs/TASK.md`, `phases/p05/docs/HANDOFF.md` |

p01 is accepted at `f61d9149b78e8f3cbf8876ac1362089561e0d76f2d2e67b5373a7ec801dd6a02`.
p02 is accepted at `f4421ac9eb35f50acf92d710948bf4e3ca777104821000a4c0e296d1ac53c10d`.
p03 is accepted at `fd40f4807955c2e19c00ab9d864c2a0ce84566088e99743b84f9338a7ce7d30c`.
p04 is accepted at `a5e729a610b120f50fb43acddf1c3bd7cda8c7b8a3cc5ae8d394e971607c7275`.
p05 is accepted at `a03abfeaf183952cbbf5c178939823e441b9355fcc2e151618527ee2d18d1a4d`.
p06 implementation complete, pending independent batch review; p07–p10 remain sequentially authorized, pending predecessors.

| p06 | `/root` author; B retained after p05 | `/Users/mac02/orca/workspaces/openbot/attribution-service` | `phases/p06/docs/TASK.md`, `phases/p06/docs/HANDOFF.md` |

## Cross-phase notices

### AS-C001 / r1

- State: active
- Origin: user's API-first and Openbot-login direction, 2026-11-15.
- Targets: all current and future phases.
- Instruction/change: implement a store-neutral merchant API and representative
  merchant integration fixture. Native store adapters/signature schemes and the
  real-store pilot are deferred. Login uses Openbot's existing account authority
  through a dedicated code/PKCE bridge with separate attribution sessions.
- Dependencies/order: effective immediately; no native-platform selection blocks
  the API-first phases. A bridge design must pass B before dependent implementation.
- Evidence/review: original conversation direction recorded in phase 01 decision
  register; protocol decision AS-P01-D1/r1 awaits B.
- Verification: generic API e2e against real PostgreSQL, independent application
  boundary and live authority/revocation. Never claim native platform support.
- Responses: p01 contract scope satisfied at AS-P01-B1-N2; p02 foundation scope satisfied at AS-P02-B1-N2; p03 identity/lifecycle scope satisfied at AS-P03-B1-N2; p04 setup scope satisfied at AS-P04-B1-N2; p05 satisfied at AS-P05-B1-N1; p06 acknowledged in its TASK; future phases pending.

### AS-C002 / r1

- State: active
- Origin: user's explicit answer “Use these implementation defaults, including HTTP 503”, 2026-11-15.
- Targets: all current and future phases.
- Instruction/change: owner/admin read/manage, members read, explicit browser
  consent, no raw IP storage, 30-day attribution window, five-minute maximum
  company-status freshness, and retryable HTTP 503 on click commit failure/uncertainty.
  Production region/retention/load targets remain unapproved deployment decisions.
- Dependencies/order: effective immediately; test budgets/configuration must not
  be advertised as approved production deployment policy.
- Evidence/review: phase 01 decision register and AS-P01-D1/r1 proposed protocol.
- Verification: permission/consent/freshness boundaries and commit-before-redirect
  tests in owning phases; deterministic implementation load/retention fixtures.
- Responses: p01 contract scope satisfied at AS-P01-B1-N2; p02 foundation scope satisfied at AS-P02-B1-N2; p03 identity/lifecycle scope satisfied at AS-P03-B1-N2; p04 setup scope satisfied at AS-P04-B1-N2; p05 satisfied at AS-P05-B1-N1; p06 acknowledged in its TASK; future phases pending.

### AS-C003 / r1

- State: active
- Origin: p04 TASK AS-P04-D1/r1, independent gate AS-P04-G1-N1.
- Targets: p05, p06, p07, p08, p09, p10.
- Instruction/change: reuse attribution setup's approved-destination policy,
  exact active domain/store checks, public-installation scope and merchant
  authentication/transaction entry. Merchant headers authenticate before body;
  `begin_merchant` rechecks tenant, store, key and scope under the shared lifecycle
  lock, and last use commits with the effect. Public installation IDs are not keys.
  User management receipts bind normalized payload and original timestamp; retain
  >=48 hours and reject original requests older than24 hours after pruning.
- Dependencies/order: future phases require accepted p04 implementation
  a5e729a610b120f50fb43acddf1c3bd7cda8c7b8a3cc5ae8d394e971607c7275 (AS-P04-B1-N2). No downstream implementation starts on this architecture pass alone.
- Evidence/review: p04 logs/AS-P04-D1-r1.md, AS-P04-G1-N1; implementation paths
  services/attribution/src/{application/setup.rs,adapters/setup/}.
- Verification: actual concurrent revoke/use and domain-revoke/verification tests,
  scope/expiry/replay and before-body authentication proof in p04; owning later
  phases verify their real routes use the same boundary through final p10.
- Responses: p04 origin satisfied with accepted producer; p05 satisfied at AS-P05-B1-N1; p06 acknowledged in its TASK; p07–p10 pending registration.


### AS-C004 / r1

- State: active
- Origin: p05 TASK AS-P05-D1/r1 discovery of required product-history edits and
  user/merchant management receipt consumers.
- Targets: p05, p06, p07, p08, p09, p10 (p04 affected shared-owner verification in p05).
- Instruction/change: add AdUpdate's optional nullable advertised_product_id to
  support original05A.4 through the production API/UI. Add user/merchant principal
  kind to shared receipt/audit identity while extracting one setup/catalog owner.
  Clicks freeze database time, scoped dimension/product/destination snapshots, link
  revision and bounded bot/prefetch booleans/classifier version; never raw IP/headers.
- Dependencies/order: effective after B AS-P05-G1-N1 PASS of AS-P05-D1/r1; p05 may implement.
  Future phases require accepted p05 producer, not this proposed decision.
- Evidence/review: p05/docs/TASK.md AS-P05-D1/r1; gate PASS AS-P05-G1-N1 (decision c65c8f56).
- Verification: actual product edit preserves prior click snapshot; cross-principal
  UUID collision cannot replay another actor's receipt; p04 shared-owner regressions.
- Responses: p05 satisfied at accepted AS-P05-B1-N1/a03abfea; p06 acknowledged in its TASK; future phases pending.

### AS-C005 / r1

- State: active
- Origin: p06 TASK AS-P06-D1/r2 original06 retry/privacy/checkout requirements.
- Targets: p06, p07, p08, p09, p10.
- Instruction/change: pixel requests with event_id must also carry original
  observed_at; no-ID pixels remain separate server-timed observations. Observation
  and checkout creation protocol horizon24h/30s future skew; retain observation
  uniqueness at least48h. These minima are not production retention approval.
  Preserve installation-scoped snapshot/kind meanings and frozen checkout selected
  evidence/policy; p07 must revalidate at first paid fact. Browser consent/withdrawal,
  bounded32 candidates and visibility50%/1s semantics are specified in p06 decision.
- Dependencies/order: effective after independent AS-P06-G1-N1 PASS of AS-P06-D1/r2.
  Downstream phases require accepted p06 implementation, not this decision alone.
- Evidence/review: phases/p06/docs/TASK.md AS-P06-D1/r2 and immutable decision log.
- Verification: retry after pruning rejects old pixel/checkout, current public/key
  boundary, real concurrent observation/checkout requests, exact30d selection,
  actual browser consent/navigation/cart and no secret/customer payload collection.
- Responses: p06 acknowledged for implementation; later phases pending.
