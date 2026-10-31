# NBV-P04 agent review findings

## Common context

Applies to every finding below unless stated otherwise.

| Field | Value |
| --- | --- |
| Date | 2026-10-31 |
| Workflow | `implement-opus-reviewed-2_2` |
| Task record | [TASK.md](TASK.md) |
| Review record | [REVIEW.md](REVIEW.md) |
| Batch | B1 (whole phase 04) |
| Stage | Batch review |
| Review round | [NBV-P04-B1-R1](agent-review-notes.md) |
| Reviewer | B, Opus 5.5/xhigh |
| Phase | NBV-P04 |
| Worktree | `/Users/mac02/.codex/worktrees/b109/openbot` |
| HEAD | `92e8c35e67dfb71e73db87895301dee74f380898` |
| Phase delta | tree `f07c076b2242a35b0bd97bf400b1bcc854d04603` → tree `7a120c2817ecca236772a033beb3e9d69a6f735c` |
| Digest | `fc4955c759c6af2f6b146644847630ab7b43416ea8806ba0bbbfa2a88f61cf57` |
| Digest exclusions | `plan/new-board-view/phases`, `plan/new-board-view/COORDINATION.md` |

---

## NBV-P04-B1-F1 — (optional) The project glyph's source is not recorded where the icon module says glyphs come from

- **Classification:** optional suggestion · Low · traceability.
- **Evidence:**
  - Plan 04 step 5 asks for the Octicons 16px `project` icon "sourced like the others
    (`src/adapters/http/pages/icon.rs`, module doc)".
  - The module doc says glyphs are "Sourced from the 16px set via the Iconstack API".
  - D3 records a different source: unpkg `@primer/octicons@19.15.0/build/svg/project-16.svg`.
  - The reviewer fetched that pinned file and confirmed the path data is byte-identical to `Icon::Project`,
    434 characters (`logs/review-b1-octicons-compare.log`). So the glyph is correct. Only the provenance
    differs from the doc, and it lives in TASK.md rather than in the code.
- **Suggested direction:** note the pinned source on the `Project` variant, or widen the module doc to
  "Octicons 16px, e.g. via Iconstack or the pinned `@primer/octicons` build".
- **Resolution history:**
  - 2026-10-31 R1: open (optional).
