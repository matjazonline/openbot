# Plan coordination: attribution-service

- Authority: `/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md`
- Writer: `/root`, current Codex session in `/Users/mac02/orca/workspaces/openbot/attribution-service`
- Revision: 4

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

| p03 | `/root` author; B reused after p02 | `/Users/mac02/orca/workspaces/openbot/attribution-service` | `phases/p03/docs/TASK.md`, `phases/p03/docs/HANDOFF.md` |

p01 is accepted at `f61d9149b78e8f3cbf8876ac1362089561e0d76f2d2e67b5373a7ec801dd6a02`.
p02 is accepted at `f4421ac9eb35f50acf92d710948bf4e3ca777104821000a4c0e296d1ac53c10d`.
p03 is active; p04–p10 remain sequentially authorized, unstarted pending predecessors.

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
- Responses: p01 contract scope satisfied at AS-P01-B1-N2; p02 foundation scope satisfied at AS-P02-B1-N2; p03 acknowledged in its TASK; future phases pending.

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
- Responses: p01 contract scope satisfied at AS-P01-B1-N2; p02 foundation scope satisfied at AS-P02-B1-N2; p03 acknowledged in its TASK; future phases pending.
