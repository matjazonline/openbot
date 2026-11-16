# Claude Code context handling

The 75% target comes from the source workflow; it is not a claim that instructions
can schedule compaction. Claude Code reads `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`, the
percentage (1-100) of the auto-compact window at which auto-compaction triggers,
for the main session and subagents. It can only lower the threshold: values above
the default are ignored. See the [environment variable reference](https://code.claude.com/docs/en/env-vars)
and, for the auto-compact window, [model configuration](https://code.claude.com/docs/en/model-config).

When configuring the target is authorized, add `"CLAUDE_AUTOCOMPACT_PCT_OVERRIDE": "75"`
to the `env` block of the project's `.claude/settings.local.json` (personal) or
`.claude/settings.json` (shared); the `update-config` skill makes this edit. Do not
edit global settings merely because this skill was selected. Verify the effective
value in the settings files and with `printenv CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`
in a Bash call. Whether a running session re-reads it is not documented: a
configuration edit is not proof that the current session or existing workers
adopted it, so disclose that until a session started after the edit confirms it.

The model cannot run `/compact`; it is a user command. Do not send `/compact` as a
shell command or worker prompt and claim it compacted the session. If the setting
or its effective value is unavailable, record the limitation and continue with
host defaults and current checkpoints. If the user requires strict enforcement,
resolve that requirement before dependent work.

Claude Code exposes no live context occupancy to the model; `/context` and the
status line are user-facing. Use readings the user supplies, identifying the
session and sample time. Cumulative usage is spend, not context. Missing readings
remain unknown. After compaction, reconcile records and continue in the same role
and agent ID. No context telemetry script is required to run this workflow.
