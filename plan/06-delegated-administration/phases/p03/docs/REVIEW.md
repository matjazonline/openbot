# Review record — 06DA-P03

## 06DA-P03-B1-R1 — 03A docs + 03B final gate

- **Outcome: CHANGES REQUIRED** at `61b33200230c82b9b1c12e746326ceb6c6d14f44dc9b5823e4bce65e44c5c57d`. HEAD `a93cd388` plus three docs edits and the 03 completion record; exclusions `phases/p03/docs`, `phases/p02/docs`, `phases/p01/docs`, `COORDINATION.md`. Digest and file list reproduced by me. Note [06DA-P03-B1-R1-N1](agent-review-notes.md).
- **Coordination:** r10 (committed), no notices.
- **Actionable:** [B1-F1](agent-review-findings.md), low — doc accuracy:
  - root-lead authority over root edits and adding root agents is understated;
  - company settings are owner-only, not "owner and root admins";
  - the history card cannot show a deletion;
  - auth-ops says "all refusals 404", ignoring the F10 403s;
  - the departments intro is loose;
  - three stale "admin" statements remain in `departments.md` (lead-change run-as, the "admin cannot see the desk" refusal, the Builder `list_company_agents`).
- **Optional:** [B1-F2](agent-review-findings.md) — the completion record omits the gates that passed only on rerun (browser hang; tool-gap transport and attribution), the hang's "pre-existing" label is unsupported, and the single-file rerun has no log.
- **Accepted:**
  - every 03B gate disposition: plugin pin, Docker images and the onnxruntime 1.24.4 memory gates unavailable (the 404s reproduced by me); tool gaps rerun; Doop deferred and visible; main.rs and source_modules run separately; stack budget with no overflow; lease PASS; Linux scope NOT EXERCISED;
  - the CI comparison, SQLx regeneration, `trail build` and startup plan paths;
  - the combined p01 + p02 full suite: only the inherited Doop failure; the C-locale pair is green on UTF-8;
  - the completion record otherwise states no more than the evidence supports.
- **Not complete:** Plan06 still cannot be declared complete; required checks remain unavailable.
- **Next:** A fixes F1 (and optionally F2), then requests a recheck.

## 06DA-P03-B1-R2 — recheck of B1-F1/F2

- **PASS for p03 B1** at `f8bfaccd7dadb06a774f1bde3dffd5001be0b3fb5246f67f9804df5d73f2d160`. HEAD `a93cd388` plus the four docs/record files; same four exclusions; digest and file list reproduced by me. Note [06DA-P03-B1-R2-N1](agent-review-notes.md).
- [B1-F1](agent-review-findings.md) FIXED: all nine doc items now match the code (root and root-lead edits, root removal rules, owner-only company writes, history card, 404/403, intro, lead-change actor, Builder tool); integration docs PASS.
- [B1-F2](agent-review-findings.md) APPLIED: the completion record names the rerun-only gates, the hang is "cause not established", and the single-file rerun log is retained.
- **Evidence:** both manifests validate. The 03B evidence from R1 is reused; no source or threshold changed.
- **Scope:** batch acceptance of docs and the gate record only. Plan06 is **not** complete: required checks remain unavailable, and the Doop deferral keeps the DB suite and stack budget red.
