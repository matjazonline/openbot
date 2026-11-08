# p01 review findings

## VG-P01-R1 — 2026-11-07

No concrete findings for architecture decision VG-P01-D1/r1. This is not an implementation pass; see [review notes](agent-review-notes.md#vg-p01-r1).

## VG-P01-F1 — static JSON shape validation bypassed by leading whitespace

- Date/workflow/round: 2026-11-07; implement-astra-reviewed-2-2; [VG-P01-R2](agent-review-notes.md#vg-p01-r2), p01 B1 implementation review. [Task](TASK.md), [review](REVIEW.md).
- Reviewer: /root/reviewer, requested gpt-6-astra/high, actual runtime settings not separately exposed. Worktree `/Users/mac02/.codex/worktrees/1133/openbot`; base/HEAD `769a23aa6ef423e964f8221a8ee6b0743325c6ba`; digest `bd10fd0dc5972fa61a1977d7322c7eb9507ffa94059ba97893f6de050785d6a0`.
- Classification: actionable defect, **P2**, correctness / typed credential boundary. Implementation defect against VG-P01-D1/r1 point 2 and phase01A.
- Evidence: `src/application/integrations/registry.rs:103` recognizes structured JSON only if the first raw character is `{`, `[` or `"`. Thus ` {"token":42}` bypasses decoding, passes both validate and injection, and becomes the literal bearer value. Unsupported JSON shape is accepted merely by adding ordinary JSON whitespace; a valid whitespace-prefixed object is also injected as its serialized object instead of its token. The same issue affects the API-key adapter.
- Reproduction: [exact-source harness](logs/VG-P01-R2-repro.rs), [successful compilation/run](logs/VG-P01-R2-repro-success.log). Both assertions exercised the frozen source modules, not a reimplementation. Initial standalone harness dependency selection failed because cached serde artifacts were incompatible; [retained fixture log](logs/VG-P01-R2-repro.log), then corrected selection without source changes.
- Required correction: preserve an unambiguous supported string/JSON distinction and validate structured inputs independently of leading JSON whitespace; add regressions for unsupported and supported whitespace-prefixed object/array/string forms for both registered injection methods.
- Instruction lesson: existing requirement already prohibits guessed structured-secret injection. Candidate narrowly scoped test guidance: credential shape tests should vary syntactically insignificant JSON whitespace. No instruction edit authorized.
- Resolution history: 2026-11-07 **open**.

## VG-P01-F2 — registered API-key authorization header accepted from operation arguments

- Date/workflow/round: 2026-11-07; implement-astra-reviewed-2-2; [VG-P01-R2](agent-review-notes.md#vg-p01-r2), p01 B1 implementation review. [Task](TASK.md), [review](REVIEW.md).
- Reviewer/settings/worktree/base/digest: same independently reviewed frozen artifact as VG-P01-F1.
- Classification: actionable defect, **P2**, security / public execution contract. Implementation defect against VG-P01-D1/r1 point 5 and phase01B requirement that operation arguments cannot supply authorization headers or arbitrary credential injection.
- Evidence: `src/application/integrations/execution.rs:34` normalizes header names but excludes only the exact `api_key` token; `X-API-Key` becomes `x_api_key`, which is accepted. `OperationArguments::new({"headers":{"X-API-Key":"caller-supplied"}})` returns Ok, although `registry.rs:151` registers this exact header as its credential injection destination. The result is also serializable/debuggable execution input, so the contract permits caller-supplied credential material on a slot intended to be controlled by the private adapter. No transport currently dispatches it; this is an implemented contract failure before later transport integration.
- Reproduction: [harness](logs/VG-P01-R2-repro.rs), [successful run](logs/VG-P01-R2-repro-success.log).
- Required correction: reject caller-controlled registered credential headers/injection destinations, including case variants, at the execution validation boundary; cover the current X-API-Key adapter and ensure future registered headers cannot silently bypass the rule.
- Instruction lesson: existing requirements are sufficient; match boundary negative tests to the credential methods actually registered, not only generic Authorization examples.
- Resolution history: 2026-11-07 **open**.

## VG-P01-F1 — resolution update 2026-11-07

**Fixed**, independently rechecked in VG-P01-R3 against digest `48c978f0dbbcaf84db7b4901eac45b6aad1efd33a66f54ed65aef0c83f64de81` (same base/HEAD/worktree). The adapter detects structural JSON after leading whitespace and parses the original bounded input; unsupported objects/arrays fail, supported objects/strings inject their decoded values. Author regression plus [independent exact-source recheck](logs/VG-P01-R3-recheck.log) covers bearer and API-key adapters, whitespace variants, validate and injection behavior. Original finding retained above.

## VG-P01-F2 — resolution update 2026-11-07

**Fixed**, independently rechecked in VG-P01-R3 against digest `48c978f0dbbcaf84db7b4901eac45b6aad1efd33a66f54ed65aef0c83f64de81`. Operation header objects now allow only ordinary content/conditional headers; custom/registered credential headers fail regardless of case. Static API-key adapters cannot register one of those ordinary header names, keeping the allowed argument and injection sets disjoint. Author regression plus [independent recheck](logs/VG-P01-R3-recheck.log) covers X-API-Key, mixed case, custom key names, accepted ordinary headers, and rejected ordinary-header injection registration. Original finding retained above. No remaining actionable findings.
