# Plan coordination: jev-evaluate

- Authority: /Users/mac02/dev/openbot/plan/jev-evaluate/COORDINATION.md
- Writer: /root, /Users/mac02/dev/openbot (initial file established by /root/phase_01; ownership handed to /root)
- Revision: 2

## Plan-wide instructions
- JEV-I001 / r1: Implement phases 01–05 sequentially in this checkout. No deployment or backward compatibility. Preserve pre-existing src/main.rs, plan files, and implement-plan-phases skill.
- JEV-I002 / r1: Each phase runs focused checks and independent Astra review. Coordinator runs the full formatting, offline compilation, migrations, and database-backed suite after all phases.

## Phase directory
| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |
| p01 | /root/phase_01 | /Users/mac02/dev/openbot | phases/p01/docs/TASK.md, phases/p01/docs/HANDOFF.md |

## Cross-phase notices

### JEV-C001 / r1
- State: active
- Origin: p01, /root/phase_01, phases/p01/docs/TASK.md, "Implementation decisions and coordination request"
- Targets: p02, p03, p04, p05
- Instruction/change: Use the p01 `registry::jev` contract and catalogue v1.6. Keep Jev separate from agent-only `ModelStep`. Preserve the 64 KiB serialized request/response bounds and exact compiled completion validation. p04 extends the existing question contract for Box and authorized images. p05 removes `jev.evaluate` from `infra::workflow::UNHANDLED_STEP_KINDS` and updates temporary help/language text only when the full production handler exists. Keep compiler semantic revision 6 unless a later change to existing semantics updates its database budget classifier together.
- Dependencies/order: Effective for consumers after p01 acceptance; p02–p05 start only after their preceding phase is accepted. Partial runtime must remain refused in production.
- Evidence/review: p01 architecture decision JEV-P01-D1/r1 passed in JEV-P01-D1-R1; final p01 implementation artifact and review are pending.
- Verification: Each consumer checks its affected contract and the handled-or-refused I/O-kind invariant; p05 checks final production activation. Coordinator runs the complete suite after all phases.
- Responses: p02 pending; p03 pending; p04 pending; p05 pending.
