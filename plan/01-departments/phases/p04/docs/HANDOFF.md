# p04 HANDOFF

- Phase p04 (`plan/01-departments/04-ui-and-docs.md`), workflow implement-opus-reviewed-2_2.
- Records `plan/01-departments/phases/p04/docs/`; start commit b1026003 (merge of origin/main
  6091c3cd into 30654e3c); snapshot exclusions `plan/01-departments/phases/p04/docs`
  `plan/01-departments/COORDINATION.md`.
- Root = A (session 68933786, Opus 5.5/xhigh, user confirmed). B = agent `ad409b2d3d149c2c6`
  (Opus 5.5/xhigh, resumed from p01–p03).
- Coordination rev 7 pinned `logs/coordination-r7.md` (sha256 3b556f5a…5414): I001 r1, I002 r2
  (user "pull main and continue": merge, then p04, then p05), I003 r2 (full gate in p05). C001–C004
  plan 04 documentation targets are B3's.
- Authorizations: local commit per accepted batch; no push.
- Resources: test DB `openbot_dept_p04_test` (drop at phase end); scratch CSS build dir under the
  session scratchpad; Tailwind from `/Users/mac02/dev/openbot/node_modules` (read only).

## State

- Merge committed b1026003 (checks in TASK "Merge checks").
- **B1 accepted in phase** (04A + 04B.1–3 + p02 B2-F2/F3): B1-R2 PASS at `9cad3b57…3848`;
  committed after b1026003. Behaviour changes: the agents directory JSON `desk` is
  `{id, address}`; desk/channel error texts say desk; the assistant desk action is "Create
  additional desk"; the desk form can set a department and members-only.
- **B2 accepted in phase** (04B.4 + 04C.1–4): B2-R2 PASS at `a97fdbeb…58ba`; committed after
  399efe2c. Behaviour changes: department filters on `/ui/work`, the run board and the dashboard
  (unknown or unseen department → 404); "Waiting on a person" view; model usage `group=department`;
  `_help`/bounce desks under department headings; agent JSON `departments`/`more_departments`;
  `AttentionQuery.company_items`. Found and fixed a p03 regression in a test p03 never ran
  (`picking_a_library_agent_copies_its_skills_with_the_agent_and_channel`).
- B3 (04C.5 + 04D docs): not started.
- Known unrelated failure: `rendered_markup_carries_no_inline_javascript` (pre-existing,
  `pages/workflow_authoring/sample.rs`).

## Next action

B3: 04C.5 + 04D docs (draft of docs/departments.md in the session scratchpad) and B2-F6.
