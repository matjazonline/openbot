# P07 review record

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Reviewer-owned (B). Details: `agent-review-notes.md` (rounds) and `agent-review-findings.md`
(findings and dispositions).

## Rounds

| Round | Date | Stage | Artifact | Outcome | Findings |
| --- | --- | --- | --- | --- | --- |
| WF-P07-R001 | 2026-10-21 | B1 architecture gate, decision P07-D1 rev 1 | `logs/decision-D1.md` sha256 `7c3245e7…8f3c92`; base/HEAD `921079d`; digest `e2714983…e3fc` (exclusions `plan/workflow-foundation/phases/p07/docs plan/workflow-foundation/COORDINATION.md marketing website`); COORDINATION r1, no notices | **Changes required** | Actionable: F001 (High), F002, F003, F004 (Medium), F005, F006 (Low). Optional: F007 |

## Open findings

| ID | Severity | Summary | Status |
| --- | --- | --- | --- |
| WF-P07-F001 | High | Child runs capture a fresh thread-history snapshot (implicit, later conversation context) | open |
| WF-P07-F002 | Medium | No classification/settlement for deterministic child-creation failures; park/round transition can be re-polled forever | open |
| WF-P07-F003 | Medium | Rerun-with-link requirement deferred without an agreed disposition | open |
| WF-P07-F004 | Medium | Binding-based child admission kept, violating phase-07 child invariants | open |
| WF-P07-F005 | Low | Phase-03 `control_retry` obligation for children not addressed | open |
| WF-P07-F006 | Low | Cancel cascade child lookup unindexed under 1 s statement timeout; cancel vs child-creation race test missing | open |
| WF-P07-F007 | Optional | Typed child error detail, ChildContract scope, self-reference resolution, provenance labelling, explicit resources, `exit_when` errors | open (non-blocking) |
