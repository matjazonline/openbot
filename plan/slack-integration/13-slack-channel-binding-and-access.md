# Step 13 — Link Private Slack Conversations with an Explicit Access Grant

## Outcome

Let a company manager bind a private Slack conversation to a business channel only after verifying
the bot can read/post there and explicitly accepting the disclosure/participation grant.

## Product/security policy

Slack v1 supports only a conversation for which all of these are true at link time:

- private channel, not DM or MPIM;
- not Slack Connect/shared, externally shared, org-wide/default, archived, frozen, or pending share;
- threadable;
- the installed bot is a member; and
- the token has the required read/history/write scopes.

Linking means: every current or future conversation member may read all messages mirrored into that
conversation and may submit messages to the bound business channel. This is an audited
`conversation_members_read_and_participate` policy—not an assertion that Slack workspace
membership grants global app access. The UI warns that a Slack member can invite another workspace
member to many private channels, so membership changes can widen the audience without an app-side
ACL edit.

## Manager flow

- Add `src/adapters/http/routes/ui_integrations.rs` and
  `src/adapters/http/pages/integrations.rs`, linked from company/channel settings. Scope every
  installation, channel, and binding ID through the active company and existing manager
  authorization.
- List only conversations visible to the bot via bounded cursor pagination, with server-side result
  and page caps. Do not load every member or channel into one request.
- On selection, re-fetch `conversations.info` immediately and validate the complete policy. Fetch a
  bounded member count/snapshot for audit; do not treat cached list data as authorization.
- Render conversation ID/name, workspace, privacy/shared flags, bot membership, member count,
  channel data classification warning, and exact access policy. Require an explicit confirmation
  field protected by the existing browser CSRF/Origin defenses.
- In one transaction, create/enable the binding, save the validation snapshot, and append the audit
  event. A unique constraint, not the UI, prevents double binding.
- Support pause and unlink. Pause stops new ingress/delivery but preserves maps and queued work;
  unlink defaults to pause plus audit and requires a separate retention decision before deletion.

## Provider drift

- Process `app_uninstalled`, `tokens_revoked`, and loss-of-membership/provider errors by marking the
  installation or binding `reauthorization_required`/`orphaned`, never deleting history.
- Before each send, definitive `not_in_channel`, `channel_not_found`, archived, or restricted errors
  disable/pause the binding and dead-letter affected delivery with a typed reason.
- A low-frequency reconciler revalidates conversation type and bot membership. Because membership
  itself is the chosen grant, it records member-count drift but does not disable merely because the
  set changed. A transition to public/shared/unsupported does disable immediately.
- Add an operator-visible “last verified” time and stale threshold. Do not claim continuous
  verification when the provider call has not run.

## Identity-link management

- Show Slack user ID/display claim and explicitly linked app principal separately.
- Allow a manager to link/unlink an observed Slack identity to a same-company person principal with
  an audit event and collision check. Linking affects attribution/ACL identity; it never changes
  Slack membership.
- Never offer “verified email” language for `users.info` profile data and never auto-merge by it.

## Tests

- Reject public, shared/Slack Connect, DM, MPIM, archived, frozen, non-threadable, and bot-not-member
  conversations.
- Cross-company installation/channel/binding IDs look not found and cannot create an audit row.
- Two concurrent links for one endpoint produce one binding and one effective grant.
- Confirmation is mandatory and its snapshot is auditable without message/member PII beyond safe
  IDs/count.
- Paused/orphaned bindings disappear from ingress/delivery selection but history remains readable
  through authorized app UI.
- Provider drift to public/shared disables the binding; member-count-only drift does not pretend to
  enforce the channel's app-side viewer list.

## Acceptance criteria

- The implemented behavior and UI describe the same access model.
- No Slack conversation receives channel data before a manager's explicit audited confirmation.
- Unsupported conversation kinds fail closed and cannot be forced through caller-supplied flags.
