# 02WR-P05 REVIEW

## 02WR-P05-R1 — batch review of p05 B1+B2 and final plan02 integration — CHANGES REQUIRED (one minor docs defect)

2026-11-14; independent B (Claude Code subagent, Opus 5.5 `claude-opus-5-5`; xhigh requested, not visible to B). Artifact **905f05c23d9b8b0894467a6601ddf89f81d36b06f8bef29b1973fa6236efb28f** (base/HEAD `2bbe2649`; plan02 base `103004a8`; checkpoint `f4edd160`; seven exclusions as assigned), recomputed by B at entry and before return. Coordination revision 40 pinned (I001–I004/r1, C001–C010 at acknowledged revisions, revision40 "p05 final-gate corrections").

[Note 02WR-P05-R1-N1](agent-review-notes.md), [all findings](agent-review-findings.md), B evidence: [focused tests](logs/reviewer-R1-focused.txt), [boundary scripts](logs/reviewer-R1-boundaries.txt).

- **[02WR-P05-R1/F1](agent-review-findings.md) P3 actionable (docs):** two `docs/departments.md` sentences state behavior the code lacks. Agent notices *do* carry the reason code and step, and a department without a lead does not "report no holder" while an ancestor holds the role; the `no_holder` hop resolution is not surfaced anywhere.
- **[02WR-P05-R1/F2](agent-review-findings.md) P3 optional:** no adversarial database test of the receipt guard's message-binding predicate (old or new form). B verified it by SQL semantics and inspection.
- **[02WR-P05-R1/F3](agent-review-findings.md) P4 optional:** the notification worker resolves the email renderer for every projection, not only agent notices. This is inert today.

Everything else in scope is supported; see the note for coverage, the checks B ran, and the gaps. F1 is a docs-only correction, so its recheck needs no Rust or database suite.

## 02WR-P05-R2 — correction recheck of R1/F1–F3 — CHANGES REQUIRED

2026-11-14; same independent B. Artifact **c3ababd836dafa6d285e16e7d4f884254439f50cdc6b30e7efc1ba39b9a8035e** (base/HEAD `2bbe2649`, same seven exclusions), recomputed by B at entry and before return; coordination revision 40 unchanged.

[Note 02WR-P05-R2-N1](agent-review-notes.md), [findings](agent-review-findings.md), B evidence: [focused tests + boundaries](logs/reviewer-R2-focused.txt), [origin family](logs/reviewer-R2-origin.txt), [transport boundary](logs/reviewer-R2-transport-boundary.txt).

- **02WR-P05-R1/F1 RESOLVED:** both departments-doc passages now match the code.
- **02WR-P05-R1/F3 RESOLVED:** the renderer is looked up only for an agent disposition; affected worker families pass.
- **02WR-P05-R1/F2 behavior resolved:** the new adversarial guard test is discriminating and passes. Its implementation introduced:
- **[02WR-P05-R2/F4](agent-review-findings.md) P2 actionable:** `escalation_agent_guard_tests.rs:145` writes `@openbot.invalid`, which `scripts/transport-boundary-check.sh` forbids anywhere in `src` outside the email adapter. B reproduced the failure, and gate run 3 (gate of record) already reports `Transport abstraction boundary` FAIL (`logs/gate-check3.txt:38-42`). Replace the case with the old local-part prefix under the real domain, or drop it, without splitting the string; then rerun the boundary scripts with a real `rg` and the guard test.

No PASS for this digest.

## 02WR-P05-R3 — correction recheck of R2/F4 — PASS

2026-11-14; same independent B. **PASS for artifact bd5279a566cdffd7b50e30277160ecb512e6cc9f3871b431422b695391a01da7** (base/HEAD `2bbe2649`, same seven exclusions), recomputed by B at entry and before return; coordination revision 40 (EOF blank line trimmed).

[Note 02WR-P05-R3-N1](agent-review-notes.md), [findings](agent-review-findings.md), B evidence: [checks and focused tests](logs/reviewer-R3-checks.txt).

- **02WR-P05-R2/F4 RESOLVED:** no `.invalid` literal remains in `src` outside the email adapter, and the transport boundary scripts pass with a real `rg`.
- **02WR-P05-R1/F2 RESOLVED:** the retired-shape case still discriminates (`<escalation-{id}` under the real domain), and the guard test passes.
- F1 and F3 were already resolved in R2. No open findings.
- Gate 3 (digest `c3ababd8`) is reusable outside the two corrected test lines. Its Clippy, DB-suite and Stack-budget reds are the confirmed-unrelated set; root must report them as FAIL with those dispositions. The Docker, memory, doop-integration and Node-pinned package gates remain unavailable.
