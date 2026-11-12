# 02WR-P04-D1-R2 independent correction inspection

2026-11-12. Architecture only; no implementation or database execution. Original r1 coverage reused where unchanged. Source remains accepted p03, no p04 implementation.

Read complete immutable D1-r2 and correction patch, original04 requirements, coordination20/C005-r2, skill review/acceptance and records references. Rechecked the affected source through graft and exact spans:

- activation.rs127–171: existing input resolution body reads saved binding/context/dependency outputs and calls the same activation evaluator. The proposed reuse avoids a second evaluator; actual preview purity and postlock equivalence remain implementation obligations.
- token_budget.rs214–241 and checkpoint_models reserve/settle callers: current late reviewer authority calls are explicitly covered by r2's merged prelock disposition.
- notification.rs402–507: href and mutating open currently share projection resolution; r2 now distinguishes lock-free preview from authorized ordered mutation. Old event/source-first commit is explicitly replaced.
- decisions/read.rs37–103: owner-authorized historical projection reads immutable workflow_decision_reviewers principal_id/reviewer_label; it does not require all historical reviewers to remain live person accounts.
- decision_authority_tests.rs262–299 and433–455: production removal preserves open/settled history, and raw membership deletion preserves settled whole-view equality. Tests inspected, not rerun. These are retained inherited obligations, not missing p04 execution claims.
- message_intents.rs132–150,245–270: candidate_bindings passes NULL for the optional binding restriction, enumerating all eligible matching channel/thread bindings. Capturing a desired binding elsewhere does not restrict this query.
- bindings.rs224–255 and domain/workflow/trigger.rs10: production writes cap active message bindings per channel at eight; fan-out is bounded but not one root per notice. Multiple candidates are valid production data.
- Existing captured-department retention is expressly retained by r2; no lifecycle relaxation is implied by defensive missing-context handling.

F1's original lock edges are now explicitly dispositioned, with all synchronous Business/action/token/final-response/legacy notification equivalents and terminal owners covered. A remaining decision ambiguity concerns operation/state-specific hints and missing mappings for historical/non-applicable reviewers. This is the same shared-consumer contract finding, not a new implementation defect.

F2's durable authenticated origin and recursive-case suppression address the original unlimited-case cycle. Its one admitted handling root/notice and 64 roots/case assertion remains unsupported by unchanged ordinary multi-binding candidate admission; root cardinality also affects which result settles the original recipient. Need an explicit targeting/fan-out disposition and actual multi-binding production cases.

F3 is resolved at architecture level: neutral resolver, complete mixed/multiple-agent set, nearest populated shadowing, per-recipient ineligibility,16/17 bound, complete failure/advance reasons and discriminating worker assertions are explicit. Existing Agent/People modes stay unchanged.

No Cargo/SQLx/DB/browser checks were run. Read-only commands only. One shell call successfully printed the message_intents graft skeleton then returned127 for an accidental trailing `g raft`; this had no side effects and the required source inspection was completed in the next command. No source changes.
