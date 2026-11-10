# 03 Reviewers as principals and authoring UI

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

`decision.human` reviewers are principals, may be named by role, and the authoring UI offers roles
wherever it offers agents. Continues in [04](04-escalation-and-reassignment.md).

## Batch 03A Reviewers

1. `workflow_decision_reviewers` (`init_schema.sql:5113`) drops `user_id` in favor of
   `principal_id` alone (already present), with a trigger refusing non-person principals.
   `reviewer_deadline` (`src/application/workflow/decisions.rs:286`) parses principal ids and role
   strings; `static_approval_reviewers` (`registry/actions.rs:34`) accepts a role as static.
   `MAX_REVIEWERS` applies after resolution.
2. Resolution for human roles uses the same `resolve_role` with `need = People` and the same
   admission point as [02](02-admission-resolution-and-snapshots.md); the resolved principals are
   written into the run role bindings and copied into the decision's reviewer rows when the step
   parks.
3. Notices, opt-out and the decision UI already key on `principal_id`; update the member notice
   lookup to go principal → user where it still needs an account.

## Batch 03B Authoring UI

1. The agent picker in workflow authoring (`src/adapters/http/pages/workflow_authoring/`) gains a
   "Role" group listing the vocabulary, rendered the same as agents with a role badge. Choosing a
   role writes `role:` and clears `agent:`.
2. The reviewer picker gains the same group and shows resolved holders for the currently selected
   desk as a preview, read-only, labelled as "today's holders".
3. Binding activation refusals from [01](01-role-references-and-publication.md) render on the
   bindings pane with the step name.

## Focused checks

- `workflow_reviewer_principal_`: reviewer rows are principals; an agent principal is refused;
  notices still reach the right account; the decision UI shows the reviewer's display label.
- `workflow_role_reviewer_`: a `reviewer` role resolves to all holders and parks the decision for
  each; no holder refuses admission.
- Rendering tests for both pickers and the preview.
