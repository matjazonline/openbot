# VG-P06-B1-R1 — CHANGES REQUIRED

Whole06A–C review completed against original requirements and accepted D1/r1. The frozen artifact is **not accepted**.

Base/HEAD `ace526fd8259ec53c5199ffa7555a1a99dcba58e`; digest `a781a861b6d39b7abccfdc0706619a7cfcc5573a440f5026ae0dbb492218c658`; coordination revision18. Exact exclusions: `plan/vault-google/COORDINATION.md`, `plan/vault-google/phases/p06/docs` only. D1/r1 SHA `8fc6432c81d63527f34ab3dbe5f628683b7d705b636d093bd3f93d85af18ba45` unchanged.

| Finding | Severity | Required correction |
|---|---|---|
| VG-P06-B1-F1 | Medium | Keep host invocation projection rejection typed as pre-dispatch; native-valid requests above the gateway bound currently become retryable unresolved errors. |
| VG-P06-B1-F2 | Medium | Cancel and await actual pending MCP initialization; dropping connect currently permits later initialized traffic. |
| VG-P06-B1-F3 | High | Recheck genuine authority/generation after MCP admission/DNS/connect waits before the next authenticated request. |

All evidence, counterexamples and correction expectations are in [findings](agent-review-findings.md); whole100-path coverage and limitations in [notes](agent-review-notes.md#vg-p06-b1-r1). Independent focused checks: MCP10/10 and HTTP5/5 pass. Two independent probes reproduce F1 and F2; F3 is actual source/caller evidence. No full/runtime/stack/lease suite or external provider call. Author checks do not close these findings.

[Entry identity](logs/reviewer-B1-R1-entry.txt), [exit identity](logs/reviewer-B1-R1-exit.txt), [inventory](logs/reviewer-B1-R1-inventory.json), [coordination pin](logs/coordination-B1-R1-r18.md). Root retains ownership of source/TASK/HANDOFF;07 remains unstarted pending06 acceptance.

Prior architecture acceptance VG-P06-A1-R1 remains preserved in [retained review](logs/reviewer-A1-REVIEW-retained.md) and dated notes. It was not code acceptance and did not waive these requirements.
