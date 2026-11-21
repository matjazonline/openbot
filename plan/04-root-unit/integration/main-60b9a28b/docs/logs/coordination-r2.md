# Plan coordination: 04-root-unit

- Authority: /Users/mac02/orca/workspaces/openbot/plan03-04/plan/04-root-unit/COORDINATION.md
- Writer: /root in /Users/mac02/orca/workspaces/openbot/plan03-04
- Revision: 2

## Plan-wide instructions

- Execute original Plan04 documents in order; stop before Plan05. No backward compatibility.
- Plan03 prerequisite published to main at `8fdb892b6e279ed0c09150e2336bbdc7462f877d`.
  Accepted material artifact c1ccbc73 / d27557291a01be97a3856d6cf1867e06900df104736a9c69f07754b3000f88fb,
  independent review 03PA-P04-B1-R17/N1. Later publication commits change only records.
- Acknowledge Plan03 C008/r1: remaining Doop execution is deferred to plan/fix-doop.md,
  does not block Plan04, and must remain visible as failed/incomplete. Whole CI is not green.
  Do not blindly defer new affected failures.
- Run focused root/membership/concurrency checks and HTTP authorization suites per batch.
  Run the complete repository/Plan04 final gate once after integration.

## Phase directory

| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |
| p01 (accepted) | /root; A root; B /root/p04_reviewer | /Users/mac02/orca/workspaces/openbot/plan03-04 | phases/p01/docs/{TASK,HANDOFF}.md |
| integration-main | /root; A root; B /root/p04_reviewer | /Users/mac02/orca/workspaces/openbot/plan03-04 | integration/main-60b9a28b/docs/{TASK,HANDOFF}.md |

## Cross-phase notices

### 04RU-C001 / r1
- State: active
- Origin: main integration60b9a28b, root writer; integration/main-60b9a28b/docs/TASK.md
- Targets: integration-main, p02, p03; p01 historical acceptance is retained.
- Instruction/change: imported attribution authority must read canonical root membership,
  retain account/company/root-row authorization fences and current permissions; no old
  company_members table is reintroduced. Owning deletion retains attribution terminal
  lifecycle/outbox evidence alongside global installation reservations, with exact proof.
- Dependencies/order: reviewed p01 commitd4bbde66/source2a5714f5…7ab793 plus imported
  origin/main03f0bda2, merged60b9a28b. Complete affected integration before p02 edits.
- Evidence/review: p01 independent04RU-P01-B1-R2/N1 PASS; published attribution contract
  plan/attribution-service/03-single-sign-on-and-company-access.md. This carries established
  owners/protocols, not a new architectural decision. Integration review remains required.
- Verification: attribution identity/access/lifecycle/competing status cases, real history-rich
  company/account teardown with exact retained terminal records, all21HTTP authorization cases.
- Responses: integration-main -> acknowledged r1 in its TASK; p02/p03 pending at startup.

