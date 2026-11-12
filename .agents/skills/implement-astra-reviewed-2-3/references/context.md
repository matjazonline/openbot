# Codex context handling

The 75% target comes from the source workflow; it is not a claim that instructions
can schedule compaction. Codex exposes `model_auto_compact_token_limit` as a token
threshold, and `model_auto_compact_token_limit_scope` selects `total` or
`body_after_prefix`. See the [official configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference).

When configuring the target is authorized, derive the token threshold as
`floor(0.75 * effective_context_capacity)` from authoritative runtime capacity,
with scope `total`. Check support in the installed host and verify effective values
for root and each worker; do not assume a root setting propagates. Never infer
capacity from a model name or increase `model_context_window` to fit a guessed
threshold. A configuration edit is not proof that an existing session adopted it.

Use host compaction controls only when actually exposed. Do not send `/compact`
as a shell command or worker prompt and claim it compacted the session. If the
controls or effective values are unavailable, record the limitation and continue
with host defaults and current checkpoints. If the user requires strict enforcement,
resolve that requirement before dependent work. Do not edit global settings merely
because this skill was selected.

Use runtime context measurements when available, identifying the session and
sample time. Recorded request input is a past sample, not exact live occupancy;
cumulative usage is spend, not context. Missing readings remain unknown. After
compaction, reconcile records and continue in the same role and handle. No context
telemetry script is required to run this workflow.
