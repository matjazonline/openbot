# 03PA-P01 — Unified addresses

- Phase ID `03PA-P01`; workflow `implement-opus-reviewed-2_3` (skill `/Users/mac02/.gui-cloudroom/plugins/environment-git-worktree/host-data/worktrees/thr_jt43h4qc8d-1/openbot/.claude/skills/implement-opus-reviewed-2_3`).
- Record root: `/Users/mac02/.gui-cloudroom/plugins/environment-git-worktree/host-data/worktrees/thr_jt43h4qc8d-1/openbot/plan/03-principal-addresses/phases/p01/docs/`.
- Root/A: Claude Code main session d139d6f0, Opus 5.5 `claude-opus-5-5`, xhigh (user-confirmed). B: a fresh general-purpose Opus subagent with xhigh requested, not a fork. It is spawned for the first review; its agent ID is recorded here when that happens.
- Worktree `/Users/mac02/.gui-cloudroom/plugins/environment-git-worktree/host-data/worktrees/thr_jt43h4qc8d-1/openbot`, branch `room/check-last-handoff-md-under-plan-02-work-qc8d` (upstream `origin/codex/departments-addressing-plans`).
- Phase starting commit: `cb46c6fd`, plan02 complete and pushed.
- Preserved pre-existing user changes (never stage, stash, reset or discard): `.claude/helpers/graft-*.cjs` (deleted), `.claude/skills/graft/SKILL.md` (deleted), `.claude/settings.json`, `.mcp.json` (deleted), root `AGENTS.md`, `.trail/`. `.claude/settings.local.json` is local and never committed.
- Plan coordination: `plan/03-principal-addresses/COORDINATION.md`, revision 7 (adopted this session). p01 acknowledges I001/r1, I003/r1, C001/r1–C005/r1; I002/r1 is superseded per revision 7.
- Prerequisites:
  - plan01 (departments) complete;
  - plan02 (workflow roles) complete: p05 `bd5279a5…1da7`, commit `cb46c6fd`.
- Authorizations:
  - implementation of plan03 (user: "continue with @plan/03-principal-addresses");
  - no push, merge or deployment is authorized for plan03 yet. I004 covered plan02 only.

## Original requirements (`plan/03-principal-addresses/README.md`, `01-unified-addresses.md`)

See the plan files; decisions 1–7 (user, 2026-11-09) are binding. Batch 01A (schema), 01B (resolution), focused checks `address_namespace_`, `address_resolution_`, `address_external_ingress_`.

## Status

- 2026-11-14: startup done. D1/r1 is written and frozen (`logs/D1-r1.md`, sha256 `2105b82b…773c`). The snapshot is `logs/D1-R1-snapshot.txt`: base `cb46c6fd`, digest `eff341d36a36dddbf9b081322d91e1cc1a9f759425ab4546d812a9948be5d991`, with exclusions `plan/03-principal-addresses/phases/p01/docs plan/03-principal-addresses/COORDINATION.md plan/02-workflow-roles/COORDINATION.md plan/02-workflow-roles/phases/p05/docs`.
- B for 03PA-P01 is a fresh general-purpose Opus subagent with xhigh requested, not a fork. Agent ID ae8aa8649634fe3c9.
- Gate rounds:
  - R1 on D1/r1: F1–F7, F8, F9.
  - R2 on D1/r2: two Low residuals plus an optional one.
  - **R3: PASS for D1/r3** (`logs/D1-r3.md`, sha256 `d5fd5109c8d559a644ca810c82c2f54fb64bd5726697935aab1ba32e8f06d7ee`), on digest `eff341d3…d991`.
  - R3/F1, the records labels, is fixed here and in COORDINATION revision 9.
- **D1/r3 is the gated decision for B1.** Any change to it reopens the gate.

## D1 — architecture gate (revision r1, 2026-11-14)

Evidence: `logs/D1-inventory.md` is a read-only inventory of every `channel_slugs` site, every automatic-agent-desk site and every address resolution and ingress site, at HEAD `cb46c6fd`. Line numbers in the plan text are stale; the inventory has the current ones.

### D1.1 Phase sequencing (same end state as the plan)

Plan 01 item 3 and plan 04 have p01 disable the automatic agent desk "before registering principal addresses at the same slugs", with the binding supplied in p03. The code now has a consumer the plan predates. Plan02's agent escalation notices route only through that desk: `workflow_escalation_agent_route` joins `workflow_provisioning_commands.command_key='agent:'||agent_id`, the desk, its binding and its email endpoint. So do `agent_notice_route.rs`, `escalation_assignment::target_on`/`reassign_on` and the receipt guard.

About 78 tests assert automatic desks or go through them: 23 `agent_default_desk_*`, 34 escalation agent/origin tests, and about 21 others. The replacement needs p02A (direct threads), p02B (principal bindings) and p03A (provisioning). The interim would be dead or failing, and C001 forbids a parallel default-desk path. Therefore:

- **p01 (this phase):**
  - The `addresses` namespace for desks and principals.
  - **Person** principal addresses: derived at first membership, editable on the profile.
  - Namespace resolution on every ingress path. A principal recipient is classified, then refused as not yet deliverable.
  - A shared `address_for`.
  - The automatic agent desk stays exactly as it is, and still holds the agent's slug as a desk address. Agent principals get no address row yet.
- **Moved to p03, as one change:**
  - registering **agent** principal addresses;
  - disabling the automatic-desk call and removing its dependents (inventory §2);
  - the principal binding;
  - rerouting agent notices (C001/C003).

  The plan's constraint holds, because the desk call stops in the same change that registers the agent address.
- **Moved to p02** (both need decision-5 access or direct threads):
  - 01B item 2, one intent per principal recipient;
  - the "People and agents" heading in `_help` and in the bounce directory (01B item 3, second half).

### D1.2 Schema (init schema, fresh databases)

```sql
CREATE TABLE public.addresses (
    company_id uuid NOT NULL,
    slug public.citext NOT NULL,
    target_kind text NOT NULL,
    channel_id uuid,
    principal_id uuid,
    is_primary boolean DEFAULT false NOT NULL,
    created_at timestamptz DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT addresses_pkey PRIMARY KEY (company_id, slug),
    CONSTRAINT addresses_slug_format CHECK (<the channel_slugs_format expression, unchanged>),
    CONSTRAINT addresses_target_kind_check CHECK (target_kind IN ('desk','principal')),
    CONSTRAINT addresses_target_check CHECK (
        (target_kind='desk' AND channel_id IS NOT NULL AND principal_id IS NULL)
     OR (target_kind='principal' AND principal_id IS NOT NULL AND channel_id IS NULL AND is_primary)),
    CONSTRAINT addresses_channel_fk FOREIGN KEY (company_id, channel_id) REFERENCES public.channels(company_id,id) ON DELETE CASCADE,
    CONSTRAINT addresses_principal_fk FOREIGN KEY (company_id, principal_id) REFERENCES public.principals(company_id,id) ON DELETE CASCADE
);
CREATE UNIQUE INDEX addresses_desk_primary_idx ON public.addresses (company_id, channel_id) WHERE is_primary AND channel_id IS NOT NULL;
CREATE UNIQUE INDEX addresses_principal_key ON public.addresses (company_id, principal_id) WHERE principal_id IS NOT NULL;
```

- `channel_slugs` is removed; there is no view and no compatibility shim. Every reader and writer in inventory §1 moves to `addresses` with `target_kind='desk'`. That includes `require_channel_reply_proposal_links` and the `department_roster`, `department.rs`, `message.rs` and provisioning reads.
- **Deviation from the plan's single `target_id` column with trigger-checked FKs.** Two kind-typed columns give real composite tenant FKs with cascade, and `addresses_target_check` ties each column to its kind. That is the persistence AGENTS rule "tenant identifier in the FK", with no trigger emulating an FK.
- **Deviation: no `principals.slug` column.** A principal's address row is its slug, and `addresses_principal_key` makes it exactly one per principal. Plan 01A item 2's `slug NOT NULL` would force slugs onto `external`/`system` principals, which must have none, and would duplicate the address in a second column that a guard must keep in sync. Plan item 3, "the agent slug and principal slug are the same value by construction", becomes in p03: agent creation writes `agents.slug` and the principal's address row in one transaction, and an agent rename updates both.
- `guard_address_principal`, BEFORE INSERT OR UPDATE ON addresses: a principal target must be of kind `person` or `agent`. Otherwise it raises 23514 with constraint `addresses_principal_kind`. The schema allows `agent` now; p01 code writes only `person`.
- `addresses_follow_principal_kind`, AFTER UPDATE OF kind ON principals: when a principal leaves `person`/`agent` it deletes that principal's address, in the same transaction. All three demotion paths are covered: member removal (`company_invite.rs:594`), user deletion (`user_lifecycle.rs:99`) and agent deletion (`agent.rs:488`). The slug is freed; someone who rejoins gets a fresh derived slug.
- "One primary per target": a principal's single row is always primary (`addresses_target_check`). For desks, `addresses_desk_primary_idx` keeps today's rule of at most one primary, with the application writing exactly one.

### D1.3 Person slugs

- Derived wherever a person principal is created or promoted: `create_person_principal_on`, which covers company creation (`company.rs:177`), invite acceptance (`company_invite.rs:409`) and promotion of an external principal to person.
  - The input is `users.username`, the only name the account has.
  - Lowercase it. Map each run of characters outside `[a-z0-9]` to `-`. Trim the `-` from both ends and truncate to 48 characters.
  - If the result is empty, or `validate_slug` refuses it (for example a reserved suffix), use `member`.
  - Insert with `ON CONFLICT (company_id, slug) DO NOTHING RETURNING`, trying `base`, then `base-2` through `base-50`, then a single `base-<8 hex>`. If all of those are taken, return `AppError::Conflict`.
  - Joining never fails just because the slug is taken; the bound is explicit.
  - An existing person (the reuse branch) keeps their address.
- Editing:
  - The person's profile page lists one address per company membership.
  - The edit goes through `validate_slug` and the format check, then `UPDATE addresses SET slug WHERE principal_id=<own principal>`.
  - The caller is resolved from the session: only that person, for their own principal. Anyone else gets 404.
  - Error 23505 becomes a 409 Conflict, "Address … is already in use in this company".
  - Routing revisions (`settings_revision` on principals) arrive in p02 together with direct admission. In p01 nothing is delivered to a principal, so no stale route can exist yet.

### D1.4 Resolution

- `EmailChannelSelectorParser` becomes `EmailAddressParser`, and `ChannelSelector` becomes `AddressSelector`, keeping `CurrentCompany`/`Qualified`, the local slug and the stripped context suffix. The serde shape changes in the outbound `internal_target`; no compatibility is needed.
  - Parsing stays **syntactic**. The adapter cannot tell a desk from a principal without the namespace, so the plan's "`classify` maps a principal selector to `AddressedTarget::Principal`" is implemented as two steps.
  - First, `AddressedTarget::Channels(Vec<ChannelSelector>)` becomes `AddressedTarget::Platform(Vec<AddressSelector>)`.
  - Then `ThreadUseCases::resolve_addresses` resolves each selector through the namespace, using the directory cache's desk list plus a per-company principal-address map. Each one becomes a desk (the existing pipeline, unchanged), a principal, or unknown (the existing bounce with suggestions).
- In p01 a principal recipient is refused per recipient with a typed reason:
  - `PrincipalRecipientRefusal::NotYetDeliverable`, with bounce text "Direct messages to people and agents are not delivered yet";
  - `PrincipalRecipientRefusal::ContextSuffix`, for example `alice+quiet@`, because a context suffix is not accepted for a principal.

  It never becomes an intent, a thread or a run. p02 replaces `NotYetDeliverable` with direct-thread admission and keeps `ContextSuffix`. Desk recipients in the same message are unaffected.
- Every path converges on `EmailIngressAdapter::accept`→`route`→`classify`, then `preflight_inbound`→`resolve_addresses`. That covers SMTP (`finish_data_transaction`), the SendGrid signed webhook, the Resend API event decode, and the internal relay (`render_email`'s `internal_target`→`single_channel_routing`). So one change covers them all. `sender_trust` and `sender_needs_spam_scan` stay desk-only in p01, because nothing is delivered to a principal yet; p02 owns principal spam policy.
- `Channel::address_for` becomes a domain-level `address_for(&AddressSlug|&ChannelSlug, &CompanySlug, domain)` shared by desks and, later, principals. `Channel::inbound_address` delegates to it. System addresses (`_help` and the others) are unchanged.

### D1.5 Invariants and failure modes

- I1: a slug names exactly one target per company (primary key), and may repeat across companies.
- I2: every target is FK-proven to be in the same tenant; deleting the target cascades to its address.
- I3: only `person`/`agent` principals have an address. A demotion drops it in the same transaction, and inserting an `external`/`system` address is refused (23514, `addresses_principal_kind`).
- I4: exactly one address per principal; at most one primary per desk; the application writes exactly one desk primary, as today.
- Creating a desk, an alias or a rename, or the automatic agent desk, at a slug a person holds is refused through the existing conflict mapping. `slug_conflict_error` and `taken_slug` now say the address is in use by "a desk, person or agent".
- Races:
  - A concurrent desk creation and member join at one slug: the primary key decides. The joiner takes the next suffix, and the desk creator gets the existing conflict.
  - Two joins with the same username: suffixes.
  - Running out of suffixes gives a bounded Conflict.
- A principal recipient never reaches admission in p01, so no plan02 consumer changes. The agent desk, the notice route and the escalation suite are untouched.

### D1.6 Verification plan

Focused checks for B1, all on fresh disposable databases:

- `address_namespace_`:
  - a desk vs person collision, in both directions: desk creation, alias and rename against a person; person derivation suffixing against a desk; a person's edit refused;
  - the same slug in another company is allowed;
  - inserting an `external`/`system` address is refused (23514 plus the constraint name);
  - a second address for one principal is refused;
  - one primary per desk;
  - removing a member, deleting a user or deleting an agent drops the address;
  - a cross-tenant `channel_id`/`principal_id` row is rejected (rollback-only);
  - a competing desk creation and member join at one slug leave exactly one owner, with the outcomes above.
- `address_resolution_`:
  - the parser grammar;
  - resolution to desk, principal, system or nobody;
  - a reserved suffix is stripped for a desk and refused for a principal;
  - an internal relay to a person's address is refused;
  - a message with one desk recipient and one person recipient admits the desk and refuses only the person.
- `address_external_ingress_`: the SMTP path and the signed SendGrid webhook each resolve desk and person recipients through the same namespace. The desk is admitted as before; the person is refused with `NotYetDeliverable`. The Resend decode is covered if its test harness allows; otherwise it is recorded as a gap.
- Profile route: editing your own address round-trips; another user gets 404; a taken slug gives 409; an invalid or reserved slug gives 400.
- Regressions:
  - every existing slug, alias and rename test, plus `provisioning_losing_an_address_race_is_a_conflict_refusal` and other `taken_slug` tests;
  - department desk tests and `agent_default_desk_*` (they still pass, since desks stay);
  - the escalation agent and origin suites;
  - `no_channel_has_an_email_binding_that_disagrees_with_its_primary_slug`, the reply-proposal link guard tests, the address-grouping tests and the simulation route tests.
- Gates: `cargo fmt --check`, `git diff --check`, diagnostic clippy, both transport boundary scripts with a real `rg` (C005), the CI extra boundary scripts and `cargo sqlx prepare` (`.sqlx`).

### D1.7 Affected contracts

- the `ChannelSelector` serde shape in the outbound email `internal_target`;
- the `AddressedTarget` variants (inventory §3);
- the `slug_conflict_error` wording;
- the SQL function `require_channel_reply_proposal_links`;
- the `channel_slugs` table name, at every site in inventory §1;
- the `.sqlx` macro files, if any query references slugs (inventory: none).

C001–C005 are unaffected in p01 (D1.1). C005's boundary rules apply to every new test: no `.invalid` identities and no hand-built `OutboundEmailV1`.

## D1 — architecture gate (revision r2, 2026-11-14)

D1/r2 replaces r1 as a whole and answers 03PA-P01-D1-R1/F1–F7 and F9. F8 is reported to the user. Unchanged r1 text is restated where it still applies, so r2 stands on its own. The inventory is `logs/D1-inventory.md`.

### D1.1 Phase sequencing (same end state as the plan)

Rationale as in r1. Plan02's agent escalation notices route only through the automatic agent desk: `workflow_escalation_agent_route` joins command key `'agent:'||agent_id`, then the desk, binding and email endpoint, and so do `agent_notice_route.rs`, `escalation_assignment::target_on`/`reassign_on` and the receipt guard. About 78 tests depend on it. The replacement needs p02A, p02B and p03A, and C001 forbids a parallel default-desk path.

- **p01:**
  - the `addresses` namespace;
  - **person** addresses;
  - namespace resolution on every ingress path including the internal relay, with principal recipients refused as not yet deliverable (D1.4);
  - a shared `address_for`.

  The automatic agent desk is unchanged and still holds the agent's slug. Agent principals get no address row.
- **p02:**
  - 01B item 2 (one intent per principal recipient);
  - the "People and agents" directory heading (01B item 3, second half);
  - its own direct-thread, binding and access work.

  p02's **agent-target** checks (`direct_thread_`, `direct_binding_`, `public_desk_private_agent_`, `direct_competing_`) use a **fixture stand-in**. A test helper registers an address for a real agent principal at a slug no desk holds; the agent's own slug is still occupied by its automatic desk. The stand-in leaves four things unproved until p03: production agent creation writing that address; equality of the agent slug and the address; rename keeping them in sync; removal of the desk.
- **p03, one change:**
  - registering agent addresses at the agent's slug, inside the agent-creation transaction;
  - disabling the automatic-desk call and removing its dependents (inventory §2);
  - the principal binding;
  - rerouting agent notices (C001/C003);
  - **rerunning p02's agent-target checks against agents created through production.**

  Plan 04's constraint holds: the desk call stops in the same change that registers the agent address.
- The move is published as **03PA-C006/r1** in plan03 COORDINATION, targeting p02, p03 and p04. It is proposed now and activated only after D1/r2 passes.

### D1.2 Schema (init schema, fresh databases)

As in r1, plus F6 and the F4 locking:

```sql
CREATE TABLE public.addresses (
    company_id uuid NOT NULL,
    slug public.citext NOT NULL,
    target_kind text NOT NULL,
    channel_id uuid,
    principal_id uuid,
    is_primary boolean DEFAULT false NOT NULL,
    created_at timestamptz DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT addresses_pkey PRIMARY KEY (company_id, slug),
    CONSTRAINT addresses_slug_format CHECK (<channel_slugs_format expression, unchanged>),
    CONSTRAINT addresses_target_kind_check CHECK (target_kind IN ('desk','principal')),
    CONSTRAINT addresses_target_check CHECK (
        (target_kind='desk' AND channel_id IS NOT NULL AND principal_id IS NULL)
     OR (target_kind='principal' AND principal_id IS NOT NULL AND channel_id IS NULL AND is_primary)),
    CONSTRAINT addresses_channel_fk FOREIGN KEY (company_id, channel_id) REFERENCES public.channels(company_id,id) ON DELETE CASCADE,
    CONSTRAINT addresses_principal_fk FOREIGN KEY (company_id, principal_id) REFERENCES public.principals(company_id,id) ON DELETE CASCADE
);
CREATE UNIQUE INDEX addresses_desk_primary_idx ON public.addresses (company_id, channel_id) WHERE is_primary AND channel_id IS NOT NULL;
CREATE INDEX addresses_channel_idx ON public.addresses (company_id, channel_id) WHERE channel_id IS NOT NULL;
CREATE UNIQUE INDEX addresses_principal_key ON public.addresses (company_id, principal_id) WHERE principal_id IS NOT NULL;
```

- `channel_slugs` is removed. Every reader and writer in inventory §1 moves to `addresses` with `target_kind='desk'`. They all filter on `company_id` together with `channel_id`, so `addresses_channel_idx` serves them (F6): the `CHANNEL_SELECT` primary and alias subqueries, the desk-settings delete and re-insert, `require_channel_reply_proposal_links`, the roster and department reads, `channel_route_on`, provisioning `taken_slug`, and the cascades.
- The two deviations from the plan wording stand, and their rationale is as in r1: kind-typed FK columns instead of `target_id`, and no `principals.slug`. F8 says to tell the user; this is reported.
  - In p03, "agent slug == agent address" becomes a two-table invariant, `agents.slug` and the principal's address row. p03 writes both in one transaction on create and rename, and proves it with a test.
- **F4 locking protocol:**
  - `guard_address_principal` is BEFORE INSERT OR UPDATE OF `target_kind, channel_id, principal_id` ON addresses, so a slug-only update does not fire it. For a principal target it reads `principals.kind ... FOR SHARE` and refuses anything other than `person`/`agent` with 23514 and constraint `addresses_principal_kind`.
    - A demotion `UPDATE principals SET kind=...` takes `FOR NO KEY UPDATE` on the principal. `FOR SHARE` conflicts with that and `FOR KEY SHARE` does not, so `FOR SHARE` is what serializes the guard against a demotion.
    - If the guard locks first, the demotion waits. Its AFTER trigger then sees the committed address and deletes it.
    - If the demotion commits first, the guard re-reads `external` and refuses.
  - `addresses_follow_principal_kind`, AFTER UPDATE OF kind ON principals: when `NEW.kind` is not `person`/`agent` and `OLD.kind` is, it deletes the principal's address. The demotion transaction locks the principal, then the address row.
  - The **slug-only edit** (D1.3) is one statement, `UPDATE addresses ... FROM principals WHERE principal.user_id=<session user> AND principal.kind='person' AND ...`. It locks only the address row and takes no principal lock.
    - If a removal is committing concurrently, the edit waits on the address row lock. It then finds the row deleted and updates 0 rows, which the route answers 404.
    - There is no lock cycle: the edit holds only one row, and the demotion takes principal, then address, in a fixed order.
  - The create and promote paths take their principal row, insert or update it, then insert the address. The guard's `FOR SHARE` is on a row the transaction already holds.

### D1.3 Person slugs

- **One derivation helper (F5, `src/AGENTS.md` "One decision, one place").**
  - Onboarding's `custom_slug`, a lowercase ASCII-alphanumeric map with runs of anything else collapsed to `-`, becomes the shared `slugify(name)`. It lives in the application module that owns `validate_slug`, and onboarding calls it.
  - Person derivation: `slugify(username)`, then truncate to 48 characters, **then** trim `-` again.
  - The candidate must then pass the database's exact format predicate (`^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$`) and `validate_slug`; otherwise it becomes `member`.
  - Insert with `ON CONFLICT (company_id, slug) DO NOTHING RETURNING`, trying `base`, `base-2` through `base-50`, then one `base-<8 hex>`. If those are all taken, return `AppError::Conflict`.
- **Every production path** goes through `create_person_principal_on`:
  - fresh insert: company creation (`company.rs:177`) or invite acceptance (`company_invite.rs:409`);
  - **promotion** of an external principal: insert the derived address after the kind update;
  - **reuse** of an existing person: keep that person's address;
  - **rejoin** after removal: the principal was demoted to external and lost its address, so this is a promotion and derives a fresh address.
  - Identity: at most one address per principal in the database (F7), and exactly one written on every production create or promote path. Each path has a test.
- Editing: as in r1. Only the person themselves, from the profile page, gets the 404/409/400 mapping described there, using the F4 single-statement protocol.
- **Exposure (F9):**
  - A derived address publishes a form of the login username to anyone the address is shown to. The user's decision 1 and plan 01A item 2 choose this: "derived from the account name".
  - Outsiders cannot use bounces to enumerate addresses (D1.4).
  - The person can change it at any time.

### D1.4 Resolution

- `EmailChannelSelectorParser` becomes `EmailAddressParser`, `ChannelSelector` becomes `AddressSelector`, and `AddressedTarget::Channels` becomes `AddressedTarget::Platform(Vec<AddressSelector>)`.
  - Parsing stays syntactic, as in r1. That includes the serde shape of the outbound `internal_target`, which needs no compatibility.
  - **One read (F9):** the ingress directory cache loads the company's whole `addresses` set (slug → desk or principal) in a single query. A slug resolves to exactly one target from one snapshot, so a concurrent rename cannot make it match both or neither.
- **External and webhook ingress (F1): whole-message refusal, reusing the existing undeliverable path.**
  - `resolve_addresses` stays all-or-nothing. `UndeliverableSlugs` gains a principal category with two reasons: `NotYetDeliverable`, and `ContextSuffix` (a context suffix such as `alice+quiet@` addressed to a principal).
  - A message with any principal recipient is refused as a whole, through the same mechanisms as an unknown desk today. SMTP keeps its synchronous 550. The SendGrid and Resend webhooks keep their durable bounce email, with the existing Message-ID handling (C005).
  - **Bounce text:**
    - A **team** sender gets "Direct messages to people and agents are not delivered yet", naming the address, or the context-suffix sentence.
    - A **non-team** sender gets exactly the text an unknown address produces, so an outsider cannot tell a member's address from a non-existent one.
  - Mixed pipelines such as `support+alice@` resolve their segments through the same namespace. A principal segment makes the message undeliverable as a whole.
  - In p02 the principal category is replaced by admission, except `ContextSuffix`.
- **Internal relay (F2).** `resolve_internal_destination`/`authorize_relay` (`relay.rs:42-103`) is a changed site.
  - The target resolves through the same namespace read. A principal target returns a definite `RelayDisposition::Refused("Direct messages to people and agents are not delivered yet")`, which egress records as terminal `DestinationUnavailable` (`egress.rs:~508`), not as a retry.
  - Lookup failures still propagate as `Err`.
  - Pre-existing and out of scope: an unknown desk slug still takes today's retryable `AppError::Internal` path. It is recorded and not changed.
- **Consumer dispositions (F7):**
  - `sender_trust`/`sender_needs_spam_scan` (SMTP spam pre-check) stay desk-only. A principal recipient counts as a stranger there, which only affects scanning, and the message is refused anyway. p02 owns principal spam policy.
  - The simulation routes (`preview_inbound_route`, `preview_recipient`, `simulation_recipient_matches`) simulate a *desk's* inbound and stay desk-only. A person's slug previews as "no such desk", unchanged in meaning. Revisited in p02.
  - `_help` and the bounce directory stay desk-only in p01.
- `address_for` becomes domain-level, as in r1.

### D1.5 Invariants and failure modes

- I1: a slug names exactly one target per company, and may repeat across companies.
- I2: every target is in the same tenant through its FK; deletion cascades.
- I3: only `person`/`agent` principals have an address. This holds under concurrency through the D1.2 F4 protocol.
- I4 (restated, F7): at most one address per principal in the database; exactly one written on every production person create or promote path; at most one primary per desk, with the application writing exactly one as today.
- Collisions:
  - A desk creation, alias or rename, or an automatic agent desk, at a slug a person holds is refused through the existing conflict mapping. The wording becomes "in use by a desk, person or agent".
  - **Onboarding:** company creation derives the owner's address first. An agent or desk the owner then names after their own username conflicts; onboarding rolls back whole, as for any taken address, and the user picks another name.
  - **Department creation** at a person's slug is refused the same way (F9).
- Races, as in r1:
  - desk creation against a member joining at one slug: the joiner takes the next suffix and the desk creator gets the conflict;
  - two joins with the same username: the suffix ladder;
  - suffixes exhausted: a bounded Conflict;
  - edit against removal, and insert against demotion: per D1.2 F4.
- No plan02 consumer changes in p01 (D1.1).

### D1.6 Verification plan

All focused checks run on fresh disposable databases.

- **`address_namespace_`:**
  - a desk vs person collision in both directions: desk creation, alias, rename and department-desk creation against a person; person derivation against a desk; a person's edit refused;
  - the same slug is allowed in another company;
  - inserting an `external`/`system` address is refused (23514 plus the constraint name);
  - a second address for one principal is refused;
  - one primary per desk;
  - removing a member, deleting a user or deleting an agent each drops the address;
  - a cross-tenant `channel_id`/`principal_id` row is rejected (rollback-only);
  - the person paths: fresh, promotion, reuse and rejoin each leave exactly one address;
  - **competing, on two connections:**
    - a desk creation and a member join at one slug;
    - a self-edit against a member removal;
    - an address insert against a demotion.

    Each asserts no deadlock or lock-timeout error reaches the caller, and that no address remains for an `external` principal.
- **`address_slug_`** (unit): a long username with a separator at the cut point, a non-ASCII username, a reserved-suffix username, an empty result, and onboarding's slugs unchanged through the shared helper.
- **`address_resolution_`:**
  - the parser grammar;
  - one-read resolution to desk, principal, system or nobody;
  - a reserved suffix is stripped for a desk and gives `ContextSuffix` for a principal;
  - a desk-plus-person message is refused whole;
  - team vs non-team bounce text, where the non-team text equals the unknown-address text;
  - an internal relay to a person's address returns `RelayDisposition::Refused` and the egress outcome is terminal `DestinationUnavailable`.
- **`address_external_ingress_`:** desk and person recipients resolve through the same namespace on SMTP (a person gets a synchronous 550), the signed SendGrid webhook (durable bounce) and the Resend decode (committed, using its existing test harness).
- **Profile route:** your own edit round-trips; another user gets 404; a taken slug gives 409; an invalid or reserved slug gives 400.
- **Regressions:**
  - every slug, alias and rename test, plus `provisioning_losing_an_address_race_is_a_conflict_refusal` and the other `taken_slug` tests;
  - department-desk tests, `agent_default_desk_*`, and the escalation agent and origin suites (desks stay);
  - `no_channel_has_an_email_binding_that_disagrees_with_its_primary_slug`, the reply-proposal link guard tests, the address-grouping tests, the simulation route tests, and the onboarding and invite tests.
- **Gates:**
  - `cargo fmt --check` and `git diff --check`;
  - diagnostic clippy;
  - both transport boundary scripts with a real `rg` (C005);
  - the CI extra boundary scripts;
  - the Rust source-module reachability test (`tests/source_modules.rs`);
  - `cargo sqlx prepare` (`.sqlx`);
  - **the discovered test names before and after** (`--list`), with each removed name accounted for.

### D1.7 Acceptance mapping and contract impact (F7)

| Criterion (plan 01, as resequenced) | Planned assertion | Status |
| --- | --- | --- |
| 01A.1 `addresses` replaces `channel_slugs` | namespace tests; regressions on every moved reader | planned |
| 01A.2 principal addresses (persons) | person-path tests, slug unit tests, profile route | planned |
| 01A.2 external/system have none | guard refusal test, demotion tests | planned |
| 01A.3 agent slug == address; disable the automatic desk | moved to p03 (C006) | deferred by D1.1 |
| 01B.1 parser and classification | resolution and ingress tests | planned |
| 01B.2 one intent per principal | moved to p02 (C006) | deferred by D1.1 |
| 01B.3 shared `address_for` / directory heading | `address_for` in p01; heading in p02 | split |
| 01B.4 SMTP, webhook and relay through one namespace | external ingress and relay tests | planned |

| Contract / consumer | Disposition |
| --- | --- |
| `channel_slugs` (inventory §1, every site) | moved to `addresses` (desk) |
| the `ChannelSelector` serde shape in the outbound `internal_target` | renamed `AddressSelector`; no compatibility needed |
| the `AddressedTarget` variants | `Channels` becomes `Platform`; every match site in inventory §3 |
| `UndeliverableSlugs`/`UndeliverableKind` and the bounce renderers | principal category added; non-team text unchanged |
| `RelayDisposition` (relay) | principal becomes `Refused` |
| `slug_conflict_error`, `taken_slug` wording | "a desk, person or agent" |
| the `require_channel_reply_proposal_links` SQL | reads `addresses` |
| `sender_trust`, simulation, `_help` | desk-only in p01 (D1.4) |
| plan02 agent-notice route, escalation assignment | unchanged in p01; p03 (C001/C003/C006) |
| `.sqlx` | regenerated; expected unchanged (inventory: no macro query names `channel_slugs`) |

## D1 — architecture gate (revision r3, 2026-11-14) — PASS 03PA-P01-D1-R3

Label correction for 03PA-P01-D1-R3/F1: this section is **D1/r3**. Its immutable copy, `logs/D1-r3.md` (sha256 `d5fd5109…d7ee`), is left unchanged because the PASS names that hash; its title and intro still say r2 by mistake. r3 is r2 plus the R2/F1, R2/F2 and R2/F3 changes in D1.2, D1.4, D1.6 and D1.7. D1.1 is unchanged from r2.

### D1.1 Phase sequencing (same end state as the plan)

Rationale as in r1. Plan02's agent escalation notices route only through the automatic agent desk: `workflow_escalation_agent_route` joins command key `'agent:'||agent_id`, then the desk, binding and email endpoint, and so do `agent_notice_route.rs`, `escalation_assignment::target_on`/`reassign_on` and the receipt guard. About 78 tests depend on it. The replacement needs p02A, p02B and p03A, and C001 forbids a parallel default-desk path.

- **p01:**
  - the `addresses` namespace;
  - **person** addresses;
  - namespace resolution on every ingress path including the internal relay, with principal recipients refused as not yet deliverable (D1.4);
  - a shared `address_for`.

  The automatic agent desk is unchanged and still holds the agent's slug. Agent principals get no address row.
- **p02:**
  - 01B item 2 (one intent per principal recipient);
  - the "People and agents" directory heading (01B item 3, second half);
  - its own direct-thread, binding and access work.

  p02's **agent-target** checks (`direct_thread_`, `direct_binding_`, `public_desk_private_agent_`, `direct_competing_`) use a **fixture stand-in**. A test helper registers an address for a real agent principal at a slug no desk holds; the agent's own slug is still occupied by its automatic desk. The stand-in leaves four things unproved until p03: production agent creation writing that address; equality of the agent slug and the address; rename keeping them in sync; removal of the desk.
- **p03, one change:**
  - registering agent addresses at the agent's slug, inside the agent-creation transaction;
  - disabling the automatic-desk call and removing its dependents (inventory §2);
  - the principal binding;
  - rerouting agent notices (C001/C003);
  - **rerunning p02's agent-target checks against agents created through production.**

  Plan 04's constraint holds: the desk call stops in the same change that registers the agent address.
- The move is published as **03PA-C006/r1** in plan03 COORDINATION, targeting p02, p03 and p04. It is proposed now and activated only after D1/r2 passes.

### D1.2 Schema (init schema, fresh databases)

As in r1, plus F6 and the F4 locking:

```sql
CREATE TABLE public.addresses (
    company_id uuid NOT NULL,
    slug public.citext NOT NULL,
    target_kind text NOT NULL,
    channel_id uuid,
    principal_id uuid,
    is_primary boolean DEFAULT false NOT NULL,
    created_at timestamptz DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT addresses_pkey PRIMARY KEY (company_id, slug),
    CONSTRAINT addresses_slug_format CHECK (<channel_slugs_format expression, unchanged>),
    CONSTRAINT addresses_target_kind_check CHECK (target_kind IN ('desk','principal')),
    CONSTRAINT addresses_target_check CHECK (
        (target_kind='desk' AND channel_id IS NOT NULL AND principal_id IS NULL)
     OR (target_kind='principal' AND principal_id IS NOT NULL AND channel_id IS NULL AND is_primary)),
    CONSTRAINT addresses_channel_fk FOREIGN KEY (company_id, channel_id) REFERENCES public.channels(company_id,id) ON DELETE CASCADE,
    CONSTRAINT addresses_principal_fk FOREIGN KEY (company_id, principal_id) REFERENCES public.principals(company_id,id) ON DELETE CASCADE
);
CREATE UNIQUE INDEX addresses_desk_primary_idx ON public.addresses (company_id, channel_id) WHERE is_primary AND channel_id IS NOT NULL;
CREATE INDEX addresses_channel_idx ON public.addresses (company_id, channel_id) WHERE channel_id IS NOT NULL;
CREATE UNIQUE INDEX addresses_principal_key ON public.addresses (company_id, principal_id) WHERE principal_id IS NOT NULL;
```

- `channel_slugs` is removed. Every reader and writer in inventory §1 moves to `addresses` with `target_kind='desk'`. They all filter on `company_id` together with `channel_id`, so `addresses_channel_idx` serves them (F6): the `CHANNEL_SELECT` primary and alias subqueries, the desk-settings delete and re-insert, `require_channel_reply_proposal_links`, the roster and department reads, `channel_route_on`, provisioning `taken_slug`, and the cascades.
- The two deviations from the plan wording stand, and their rationale is as in r1: kind-typed FK columns instead of `target_id`, and no `principals.slug`. F8 says to tell the user; this is reported.
  - In p03, "agent slug == agent address" becomes a two-table invariant, `agents.slug` and the principal's address row. p03 writes both in one transaction on create and rename, and proves it with a test.
- **F4 locking protocol:**
  - `guard_address_principal` is BEFORE INSERT OR UPDATE OF `target_kind, channel_id, principal_id` ON addresses, so a slug-only update does not fire it. For a principal target it reads `principals.kind ... FOR KEY SHARE` and refuses anything other than `person`/`agent` with 23514 and constraint `addresses_principal_kind`.
    - **Corrected rationale (R2/F2).** `kind` is a column of the unique key `principals_company_id_id_kind_key`. An UPDATE that changes it therefore takes `FOR UPDATE` on the principal, and `FOR KEY SHARE` conflicts with that. B's probe on PostgreSQL 18.7 (`logs/D1-R2-review-lock-probe.txt`) showed `FOR KEY SHARE` blocks a `kind` update but not a `display_label` update.
    - The existing `guard_department_principal_kind` relies on the same fact, so `FOR KEY SHARE` is the house mode and serializes the guard against a demotion. It does not wait on unrelated label updates. r2 used `FOR SHARE`, which is also correct but blocks those label updates needlessly.
    - The department guard is unchanged.
    - The guard never fires on the slug-only edit, so R1/F4's deadlock concern (a guard lock on every UPDATE) does not arise.
    - If the guard locks first, the demotion waits. Its AFTER trigger then sees the committed address and deletes it.
    - If the demotion commits first, the guard re-reads `external` and refuses.
  - `addresses_follow_principal_kind`, AFTER UPDATE OF kind ON principals: when `NEW.kind` is not `person`/`agent` and `OLD.kind` is, it deletes the principal's address. The demotion transaction locks the principal, then the address row.
  - The **slug-only edit** (D1.3) is one statement, `UPDATE addresses ... FROM principals WHERE principal.user_id=<session user> AND principal.kind='person' AND ...`. It locks only the address row and takes no principal lock.
    - If a removal is committing concurrently, the edit waits on the address row lock. It then finds the row deleted and updates 0 rows, which the route answers 404.
    - There is no lock cycle: the edit holds only one row, and the demotion takes principal, then address, in a fixed order.
  - The create and promote paths take their principal row, insert or update it, then insert the address. The guard's `FOR KEY SHARE` is on a row the transaction already holds.

### D1.3 Person slugs

- **One derivation helper (F5, `src/AGENTS.md` "One decision, one place").**
  - Onboarding's `custom_slug`, a lowercase ASCII-alphanumeric map with runs of anything else collapsed to `-`, becomes the shared `slugify(name)`. It lives in the application module that owns `validate_slug`, and onboarding calls it.
  - Person derivation: `slugify(username)`, then truncate to 48 characters, **then** trim `-` again.
  - The candidate must then pass the database's exact format predicate (`^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$`) and `validate_slug`; otherwise it becomes `member`.
  - Insert with `ON CONFLICT (company_id, slug) DO NOTHING RETURNING`, trying `base`, `base-2` through `base-50`, then one `base-<8 hex>`. If those are all taken, return `AppError::Conflict`.
- **Every production path** goes through `create_person_principal_on`:
  - fresh insert: company creation (`company.rs:177`) or invite acceptance (`company_invite.rs:409`);
  - **promotion** of an external principal: insert the derived address after the kind update;
  - **reuse** of an existing person: keep that person's address;
  - **rejoin** after removal: the principal was demoted to external and lost its address, so this is a promotion and derives a fresh address.
  - Identity: at most one address per principal in the database (F7), and exactly one written on every production create or promote path. Each path has a test.
- Editing: as in r1. Only the person themselves, from the profile page, gets the 404/409/400 mapping described there, using the F4 single-statement protocol.
- **Exposure (F9):**
  - A derived address publishes a form of the login username to anyone the address is shown to. The user's decision 1 and plan 01A item 2 choose this: "derived from the account name".
  - Outsiders cannot use bounces to enumerate addresses (D1.4).
  - The person can change it at any time.

### D1.4 Resolution

- `EmailChannelSelectorParser` becomes `EmailAddressParser`, `ChannelSelector` becomes `AddressSelector`, and `AddressedTarget::Channels` becomes `AddressedTarget::Platform(Vec<AddressSelector>)`.
  - Parsing stays syntactic, as in r1. That includes the serde shape of the outbound `internal_target`, which needs no compatibility.
  - **One read (F9; scoped by R2/F3):** per message and company, one query, `SELECT slug, target_kind, channel_id, principal_id FROM addresses WHERE company_id=$1 AND slug = ANY($2)`, over that message's own recipient slugs. The recipient count is already bounded. A slug resolves to exactly one target from one snapshot, so a concurrent rename cannot make it match both or neither, and the work is bounded by the recipient count, not the company's size.
    - Desk targets then take their channel details by id from the existing cached company channel list, which also still feeds the unknown-slug suggestions.
    - A desk id missing from that list resolves exactly as an unknown desk does today.
- **External and webhook ingress (F1): whole-message refusal, reusing the existing undeliverable path.**
  - `resolve_addresses` stays all-or-nothing. `UndeliverableSlugs` gains a principal category with two reasons: `NotYetDeliverable`, and `ContextSuffix` (a context suffix such as `alice+quiet@` addressed to a principal).
  - A message with any principal recipient is refused as a whole, through the same mechanisms as an unknown desk today. SMTP keeps its synchronous 550. The SendGrid and Resend webhooks keep their durable bounce email, with the existing Message-ID handling (C005).
  - **Bounce text:**
    - A **team** sender gets "Direct messages to people and agents are not delivered yet", naming the address, or the context-suffix sentence.
    - **A non-team sender gets an unknown slug (R2/F1).** The principal recipient is recorded with the existing unknown-desk kind (`UndeliverableKind::UnknownChannel`), and its suggestions come from the same desk-list computation. So the `IngestRejection` kind, its `as_str()` and therefore SMTP's synchronous `550 Message rejected ({rejection})` line, the webhook bounce email, and the suggestions are all identical to those for a slug that does not exist. An outsider cannot tell a member's address from a non-existent one on any transport.
    - The principal kinds (`NotYetDeliverable`, `ContextSuffix`) exist only for team senders. Team membership comes from the sender authorization ingress already performs. A lookup failure there propagates as `Err`, never as "outsider" (`src/AGENTS.md`).
  - Mixed pipelines such as `support+alice@` resolve their segments through the same namespace. A principal segment makes the message undeliverable as a whole.
  - In p02 the principal category is replaced by admission, except `ContextSuffix`.
- **Internal relay (F2).** `resolve_internal_destination`/`authorize_relay` (`relay.rs:42-103`) is a changed site.
  - The target resolves through the same namespace read. A principal target returns a definite `RelayDisposition::Refused("Direct messages to people and agents are not delivered yet")`, which egress records as terminal `DestinationUnavailable` (`egress.rs:~508`), not as a retry.
  - Lookup failures still propagate as `Err`.
  - Pre-existing and out of scope: an unknown desk slug still takes today's retryable `AppError::Internal` path. It is recorded and not changed.
- **Consumer dispositions (F7):**
  - `sender_trust`/`sender_needs_spam_scan` (SMTP spam pre-check) stay desk-only. A principal recipient counts as a stranger there, which only affects scanning, and the message is refused anyway. p02 owns principal spam policy.
  - The simulation routes (`preview_inbound_route`, `preview_recipient`, `simulation_recipient_matches`) simulate a *desk's* inbound and stay desk-only. A person's slug previews as "no such desk", unchanged in meaning. Revisited in p02.
  - `_help` and the bounce directory stay desk-only in p01.
- `address_for` becomes domain-level, as in r1.

### D1.5 Invariants and failure modes

- I1: a slug names exactly one target per company, and may repeat across companies.
- I2: every target is in the same tenant through its FK; deletion cascades.
- I3: only `person`/`agent` principals have an address. This holds under concurrency through the D1.2 F4 protocol.
- I4 (restated, F7): at most one address per principal in the database; exactly one written on every production person create or promote path; at most one primary per desk, with the application writing exactly one as today.
- Collisions:
  - A desk creation, alias or rename, or an automatic agent desk, at a slug a person holds is refused through the existing conflict mapping. The wording becomes "in use by a desk, person or agent".
  - **Onboarding:** company creation derives the owner's address first. An agent or desk the owner then names after their own username conflicts; onboarding rolls back whole, as for any taken address, and the user picks another name.
  - **Department creation** at a person's slug is refused the same way (F9).
- Races, as in r1:
  - desk creation against a member joining at one slug: the joiner takes the next suffix and the desk creator gets the conflict;
  - two joins with the same username: the suffix ladder;
  - suffixes exhausted: a bounded Conflict;
  - edit against removal, and insert against demotion: per D1.2 F4.
- No plan02 consumer changes in p01 (D1.1).

### D1.6 Verification plan

All focused checks run on fresh disposable databases.

- **`address_namespace_`:**
  - a desk vs person collision in both directions: desk creation, alias, rename and department-desk creation against a person; person derivation against a desk; a person's edit refused;
  - the same slug is allowed in another company;
  - inserting an `external`/`system` address is refused (23514 plus the constraint name);
  - a second address for one principal is refused;
  - one primary per desk;
  - removing a member, deleting a user or deleting an agent each drops the address;
  - a cross-tenant `channel_id`/`principal_id` row is rejected (rollback-only);
  - the person paths: fresh, promotion, reuse and rejoin each leave exactly one address;
  - **competing, on two connections:**
    - a desk creation and a member join at one slug;
    - a self-edit against a member removal;
    - an address insert against a demotion.

    Each asserts no deadlock or lock-timeout error reaches the caller, and that no address remains for an `external` principal.
- **`address_slug_`** (unit): a long username with a separator at the cut point, a non-ASCII username, a reserved-suffix username, an empty result, and onboarding's slugs unchanged through the shared helper.
- **`address_resolution_`:**
  - the parser grammar;
  - one-read resolution to desk, principal, system or nobody;
  - a reserved suffix is stripped for a desk and gives `ContextSuffix` for a principal;
  - a desk-plus-person message is refused whole;
  - team vs non-team bounce text, where for a non-team sender the person address and a non-existent slug give equal `IngestRejection` kinds and `as_str()`, equal bounce emails, and equal suggestions;
  - an internal relay to a person's address returns `RelayDisposition::Refused` and the egress outcome is terminal `DestinationUnavailable`.
- **`address_external_ingress_`:** desk and person recipients resolve through the same namespace on SMTP (a person gets a synchronous 550), the signed SendGrid webhook (durable bounce) and the Resend decode (committed, using its existing test harness).
  - **SMTP enumeration check (R2/F1):** from an outsider, the 550 reply line for a member's address equals the 550 line for a non-existent slug, byte for byte.
- **Profile route:** your own edit round-trips; another user gets 404; a taken slug gives 409; an invalid or reserved slug gives 400.
- **Regressions:**
  - every slug, alias and rename test, plus `provisioning_losing_an_address_race_is_a_conflict_refusal` and the other `taken_slug` tests;
  - department-desk tests, `agent_default_desk_*`, and the escalation agent and origin suites (desks stay);
  - `no_channel_has_an_email_binding_that_disagrees_with_its_primary_slug`, the reply-proposal link guard tests, the address-grouping tests, the simulation route tests, and the onboarding and invite tests.
- **Gates:**
  - `cargo fmt --check` and `git diff --check`;
  - diagnostic clippy;
  - both transport boundary scripts with a real `rg` (C005);
  - the CI extra boundary scripts;
  - the Rust source-module reachability test (`tests/source_modules.rs`);
  - `cargo sqlx prepare` (`.sqlx`);
  - **the discovered test names before and after** (`--list`), with each removed name accounted for.

### D1.7 Acceptance mapping and contract impact (F7)

| Criterion (plan 01, as resequenced) | Planned assertion | Status |
| --- | --- | --- |
| 01A.1 `addresses` replaces `channel_slugs` | namespace tests; regressions on every moved reader | planned |
| 01A.2 principal addresses (persons) | person-path tests, slug unit tests, profile route | planned |
| 01A.2 external/system have none | guard refusal test, demotion tests | planned |
| 01A.3 agent slug == address; disable the automatic desk | moved to p03 (C006) | deferred by D1.1 |
| 01B.1 parser and classification | resolution and ingress tests | planned |
| 01B.2 one intent per principal | moved to p02 (C006) | deferred by D1.1 |
| 01B.3 shared `address_for` / directory heading | `address_for` in p01; heading in p02 | split |
| 01B.4 SMTP, webhook and relay through one namespace | external ingress and relay tests | planned |

| Contract / consumer | Disposition |
| --- | --- |
| `channel_slugs` (inventory §1, every site) | moved to `addresses` (desk) |
| the `ChannelSelector` serde shape in the outbound `internal_target` | renamed `AddressSelector`; no compatibility needed |
| the `AddressedTarget` variants | `Channels` becomes `Platform`; every match site in inventory §3 |
| `UndeliverableSlugs`/`UndeliverableKind`, `IngestRejection::as_str` and the bounce renderers | principal kinds for team senders only; a non-team sender gets `UnknownChannel`, so the 550 line, bounce text and suggestions are unchanged (R2/F1) |
| `RelayDisposition` (relay) | principal becomes `Refused` |
| `slug_conflict_error`, `taken_slug` wording | "a desk, person or agent" |
| the `require_channel_reply_proposal_links` SQL | reads `addresses` |
| `sender_trust`, simulation, `_help` | desk-only in p01 (D1.4) |
| plan02 agent-notice route, escalation assignment | unchanged in p01; p03 (C001/C003/C006) |
| `.sqlx` | regenerated; expected unchanged (inventory: no macro query names `channel_slugs`) |

## B1 implementation (in progress, 2026-11-14)

Baseline test names: `logs/B1-tests-before.txt` (4,493), taken at `cb46c6fd` + records. The `channel.rs` SQL-string edit made during that build changed no test function, so the names are unaffected. Implemented so far; `cargo check --all-targets` is clean:

- **Schema:** `channel_slugs` is replaced by `addresses`, exactly as D1.2 specifies: constraints, three indexes, the FKs, `guard_address_principal` (`FOR KEY SHARE`, fired by INSERT or a target-column UPDATE) and `addresses_follow_principal_kind`. `require_channel_reply_proposal_links` now reads `addresses`.
- **Desk readers and writers moved** (inventory §1):
  - `CHANNEL_SELECT`, `insert_desk_addresses` (renamed from `insert_channel_slugs`), the alias lookup and the settings rewrite;
  - `taken_slug` (any kind of target);
  - `channel_route_on`, the roster joins and `department.rs::desks`;
  - test SQL and comments, plus `docs/system_addresses.md`'s constraint name.
  - The `slug_conflict_error` wording is "a desk, person or agent".
- **Slugs:** `slugify` (onboarding's `custom_slug`, now shared), `person_address_base` (slugify, truncate to 48, trim, format and `validate_slug` check, else `member`), `is_address_slug` and `SlugKind::PersonAddress`.
- **Person addresses:** `ensure_person_address_on` runs at the end of `create_person_principal_on` on every branch. It keeps an existing address; otherwise it walks the `ON CONFLICT DO NOTHING` ladder `base`, `-2` … `-50`, then one `-<8 hex>`, then returns Conflict.
- **Resolution:**
  - Renamed: `EmailAddressParser`, `EmailAddressSelection`, `AddressSelector`, `InvalidAddressSelector`, `AddressedTarget::Platform` and `platform_pipelines`.
  - New `entities::address::{AddressTarget, ResolvedAddress, address_for}`; `Channel::address_for` delegates to it.
  - New port method `ChannelPersistence::address_targets`, a single `slug = ANY($2::citext[])` read. The Postgres implementation errors if a row names neither exactly one desk nor one principal. The four mocks use `desk_addresses`, a test-only helper.
  - `DirectoryCache::load_addresses` and `address_target`, with `ThreadUseCases::load_addressed_namespace` reading once per company per message.
  - `resolve_addresses`:
    - a desk target is matched by id in the cached channel list;
    - a principal target is `UndeliverableSlugs::principal` for a team sender (`PrincipalRefusal::{NotYetDeliverable, ContextSuffix}`, the latter when the recipient's disposition is `FileOnly`), and `unknown(…)` with the same suggestions for anyone else;
    - anything else stays unknown.
  - New `UndeliverableKind::Principal`, and `as_str` gives the refusal sentence.
  - `BounceInfo.principals`, a "Person or agent" line in the body, and the `bounce_cause` principal cases. When there are no principals, the existing strings are unchanged.
- **Relay:**
  - `resolve_internal_destination` returns `InternalDestination::{Desk, Principal}`. It reads the source company first; when the selector is in the source's own company it reads the namespace, and a principal there means a definite principal refusal.
  - `authorize_relay` returns `RelayAuthorization::{Refused, Desk}`, and `relay_internal` maps `Refused` to `RelayDisposition::Refused`.
  - The old `Option`/`NotInternal` arm was unreachable (the destination never returned `None`) and is removed.
- **Profile:**
  - New `UserPersistence::{person_addresses, rename_person_address}`. The rename is one `UPDATE … FROM principals` statement that touches only the address row; 23505 becomes Conflict. Implemented in Postgres and in both stubs (empty or `None`).
  - `UserUseCases::rename_person_address` validates through `validate_slug`, then `is_address_slug`, which gives BadRequest. `None` from persistence gives NotFound.
  - The route `PUT /ui/profile/addresses/{company_id}` renders the `#profile-addresses` fragment beside `#profile-pane`; the vault page passes it empty.
  - **Convention note:** profile forms answer a rejected save with 200 and an inline banner, because htmx 2 does not swap 4xx responses. So Conflict and BadRequest are rendered in the section, while NotFound and other errors propagate (404). The use-case error mapping is the D1.3 409/400 mapping.

### B1 tests and verification mapping

| D1.6 check | Test(s) | Decisive assertion |
| --- | --- | --- |
| `address_namespace_`: desk vs person collision, both directions | `address_tests::address_namespace_a_slug_names_one_desk_or_person_per_company` | A desk create, alias or rename at the owner's address is refused with "a desk, person or agent". A joiner at a desk's slug gets `-2`. A person's rename onto a desk's slug is Conflict. The rename round-trips. |
| the same slug in another company | same test | A desk at `dana-own` in another company is created. The owner has the same derived slug in both companies. |
| external and system principals refused; one address per principal; one primary per desk | `address_namespace_refuses_external_and_system_principals_and_a_second_address` | `23514 addresses_principal_kind` (external and system), `23505 addresses_principal_key`, `23505 addresses_desk_primary_idx`, `23514 addresses_target_check` (a non-primary principal row) |
| cross-tenant rows rejected | `address_namespace_rejects_a_target_from_another_company` | `23503 addresses_channel_fk` / `addresses_principal_fk`, rollback-only |
| person paths: fresh, promotion, reuse, rejoin | `address_namespace_person_paths_leave_exactly_one_address` (fresh is the owner, in the first test) | The promoted principal id equals the outsider's id, with one address. Reuse keeps it. `remove_member` drops it and frees the slug. Rejoin derives it afresh. |
| demotion drops the address (agent principal) | `address_namespace_demotion_of_an_agent_principal_drops_its_address` | A raw agent address is gone after `kind` becomes `external` |
| competing: desk vs join | `address_namespace_competing_desk_and_join_at_one_slug_leave_one_owner` | The join waits on the desk's uncommitted primary key, then takes `-2`; the desk keeps the slug |
| competing: self-edit vs removal | `address_namespace_competing_self_edit_and_removal_leave_no_address_on_an_outsider` | The edit blocks, then returns `Ok(None)` (404) with no lock error; no address remains |
| competing: insert vs demotion, both orders | `address_namespace_competing_insert_and_demotion_leave_no_address_on_an_outsider` | Insert first: the demotion waits, then the trigger deletes the address. Demotion first: the guard waits, then `23514 addresses_principal_kind`. |
| `address_slug_` | `use_cases::channel::tests::address_slug_{derivation_keeps_the_database_shape, shape_matches_the_format_constraint, onboarding_names_are_unchanged_by_the_shared_helper}` | A separator at the cut is trimmed, non-ASCII becomes separators, empty or reserved becomes `member`, and onboarding slugs are unchanged |
| `address_resolution_`: desk admitted; person refused (team) | `thread::tests::address_resolution::address_resolution_a_desk_is_admitted_and_a_person_is_refused_for_a_teammate` | The reason is "Direct messages … not delivered yet", `bounce.principals = [alice/NotYetDeliverable]`, and the body has the "Person or agent" line |
| a context suffix strips for a desk and is refused for a principal | `…a_context_suffix_strips_for_a_desk_and_is_refused_for_a_person` | `support+quiet` is accepted; `alice+quiet` gives `ContextSuffix` |
| desk and person together refused whole | `…a_desk_and_a_person_together_are_refused_whole` | `support+alice` is refused and the durable counts are unchanged |
| a stranger cannot tell a person from nobody | `…a_stranger_cannot_tell_a_person_from_nobody` | For `alice`, `alice+quiet` and `support+alice` against `nobody…`: equal `IngestRejection` (slug and message key normalized), no principals, and equal bounce bodies |
| internal relay to a person | `…an_internal_relay_to_a_person_is_refused_definitely` | `RelayDisposition::Refused("Direct messages …")`. Egress maps `Refused` to terminal `DestinationUnavailable` (`egress.rs:504`, unchanged). |
| `address_external_ingress_` SMTP | `smtp::server::tests::address_external_ingress_smtp_does_not_confirm_a_members_address` (`own_database`, real Postgres namespace) | A stranger to `owner@` gets a 550 line byte-equal to `nobody@`. A teammate gets the "not delivered yet" 550. A desk gets 250. This is `SmtpAnswer::for_ingest(..).line()`, the line `finish_data_transaction` writes. |
| `address_external_ingress_` Resend | `resend_api::inbound_tests::address_external_ingress_resend_names_a_person_the_way_it_names_a_desk` | The `read_mail` routing for `alice@` equals the routing for `support@` with the slug swapped (one `Platform` selector) |
| `address_external_ingress_` SendGrid signed webhook | **NOT EXERCISED at the route level.** No harness drives `sendgrid_inbound_webhook`; the repository has no route test for it. | Covered by composition: `decode_sendgrid_request` gives a `RawInboundPayload` (existing `the_envelope_is_the_routing_source_of_truth`), then the same `EmailIngressAdapter::accept`→`preflight_inbound` path the SMTP and resolution tests drive, then the unchanged `handle_bounce_dispatch`. This is a verification gap. |
| profile route | `ui_profile::session_tests::address_profile_renames_only_the_callers_own_address` | Own rename gives 200 with "Address saved." and the stored slug; the company name is escaped. `taken`, `bad slug!` and `front-quiet` give an inline reason and no change. Another company gives `Err(NotFound)`, and that company's address is unchanged. |

- 2026-11-14: the user approved committing the two trail learnings ("yes, commit the trail learnings"). They are committed as `830561e2`, containing only the two `.trail/learnings/*.md` files; the rest of `.trail/` stays uncommitted and preserved. HEAD is now `830561e2`; the B1 review base for the source delta is still `cb46c6fd` (the learnings commit holds no source). Not pushed: no push was asked for.
- 2026-11-14, user: "I'm OK with the two places the address table departs from my decision 1's wording (no literal wording necessary, would just make it more complex)". This accepts the two departures from decision 1's wording: the kind-typed `channel_id`/`principal_id` columns instead of `target_id`, and no `principals.slug`. It settles 03PA-P01-D1-R1/F8; no `target_id` computed column will be added.

### B1 checks and review round 03PA-P01-B1-R1 inputs (2026-11-14)

- Fixes during checks:
  - The new SMTP test's fixture desk was disabled by the `ChannelWrite` default, so it was set `enabled: true`. The production answer, "Channel is disabled", was correct.
  - Clippy `large_enum_variant` on the new `InternalDestination`: the variant is now `Desk(Box<RelaySource>)` (`src/AGENTS.md` boxing rule). The relay and resolution tests were rerun (relay1, 20 PASS).
- Results, all in `logs/03PA-P01-B1-R1.evidence.json` (validated: 10 PASS, 2 FAIL kept as history, 1 NOT EXERCISED):
  - namespace: 8 PASS;
  - slug and resolution: 8 PASS;
  - focus1: 794 PASS plus 1 fixture FAIL, fixed;
  - focus2: 532 PASS;
  - ingress: 2 PASS;
  - relay: 20 PASS;
  - diagnostic clippy: PASS after one fix;
  - fresh migrate and `.sqlx` unchanged;
  - all static and boundary gates;
  - source modules: 8 PASS;
  - test names 4,493 → 4,512, with 19 added and 0 removed.
- Gap: the signed SendGrid route test is not exercised (see the mapping table).
- Artifact: phase start `cb46c6fd`, review base `cb46c6fd` (source), HEAD `830561e2` (that commit holds only the two trail learnings). Digest `888f0a805e0a1fa00b37892acf298493f573eca5897be111ba3fb9d7e255416c` (`logs/B1-R1-snapshot.txt`), with exclusions `plan/03-principal-addresses/phases/p01/docs plan/03-principal-addresses/COORDINATION.md plan/02-workflow-roles/COORDINATION.md plan/02-workflow-roles/phases/p05/docs`. Coordination revision 9.

### B1-R1 result and corrections (2026-11-14)

B (round 03PA-P01-B1-R1) found no production behaviour defect. It returned F1–F3 (Low, actionable) and F4–F7 (Info). Corrections:

- **F1:** the address form gets `hx-disabled-elt="find button[type='submit']"` and the spinner/"Saving..." spans of the identity form (`pages/profile.rs`).
- **F2:**
  - **(a)** `address_namespace_competing_held_edit_then_removal_leave_no_address_on_an_outsider`: an edit's address-row lock is held while `remove_member` waits, then completes with no address and no error. Also `address_namespace_kind_guard_fires_only_when_a_target_changes`, which asserts `pg_get_triggerdef` reads `BEFORE INSERT OR UPDATE OF target_kind, channel_id, principal_id`.
  - **(b)** `address_namespace_deleting_an_account_drops_its_addresses` uses the production `HostAccountDeletion::delete_user_account`.
  - **(c)** `department::tests::lead_desks::address_namespace_a_department_desk_cannot_take_a_persons_address`: creating a department at a person's address is Conflict and adds no department. The fixture writes principals raw, so the test gives the member person an address first.
  - **(d)** The four competing tests use `Fixture::isolated()`, an `own_database` held by the fixture.
  - **(e)** `sendgrid::inbound::tests::address_external_ingress_sendgrid_names_a_person_the_way_it_names_a_desk` runs the `decode_sendgrid_request` steps, except the DNS-backed authentication lookup whose verdicts it states instead: `decode_provider_request`, `parse_raw_mime_to_payload`, then `EmailIngressAdapter::accept`. Person routing equals desk routing with the slug swapped. This replaces the earlier NOT EXERCISED entry.
- **F3:** the per-slug decision is extracted as the synchronous `addressed_slug(..) -> AddressedSlug::{Desk(Box<Channel>), DisabledDesk, Principal, Unknown}` plus `record_undeliverable`. A sync helper adds no async frame (`src/AGENTS.md`). `resolve_addresses` is 99 lines (101 at the base) with one less nesting level. Its behaviour is unchanged: a desk is matched by id, disabled is decided before the ACL, a stranger's principal is unknown with the same suggestions, and the sender is read only for a principal target.
- Not taken (Info):
  - F4, an `AddressSlug` newtype: deferred; persons reuse `ChannelSlug` as desks do.
  - F5, a use-case Conflict/BadRequest test: the persistence Conflict and the route inline reasons are already asserted.
  - F6: carried to p02/p03 in COORDINATION revision 10.
  - F7: revision 10 is appended, not edited in place.
- Checks:
  - corr1: 192 PASS, 1 FAIL (the department fixture, fixed); corr2: 88 PASS; corr3: 193 PASS after `AddressedSlug::Desk` was boxed.
  - Clippy: the clippy3 FAIL (`large_enum_variant`, `needless_borrow`) was fixed; clippy4 PASS.
  - Static gates: 9 PASS. Source modules: PASS.
  - Test names: 4,493 → 4,517, with 24 added and 0 removed.
- Artifact for R2: HEAD `830561e2`, digest `dc9329df080197b948e37372460f55476974a800811cb217182826193400ca23` (`logs/B1-R2-snapshot.txt`), same four exclusions, coordination revision 10. Manifest `logs/03PA-P01-B1-R2.evidence.json`, validated: 7 PASS, 2 FAIL kept as history, 1 NOT EXERCISED (the F1 markup, verified by inspection).

## Acceptance (2026-11-14)

- B passed round **03PA-P01-B1-R2** for digest `dc9329df…ca23`, under D1/r3.
- Root recomputed the digest: `dc9329df080197b948e37372460f55476974a800811cb217182826193400ca23`, HEAD `830561e2` (`logs/B1-acceptance-snapshot.txt`). Coordination revision 10 has no new notices, and C001–C006 are respected. The evidence manifest validates.
- **03PA-P01 is accepted in phase.** It is not yet integrated; the plan04 final gate does that.
- Known and reported:
  - The SendGrid and Resend checks are adapter-level: stated auth verdicts, no route harness.
  - Agent-address paths are deferred to p03 under C006.
  - B's run hit `rendered_markup_carries_no_inline_javascript` failing on `workflow_authoring/sample.rs:153`. This is the same pre-existing failure plan02 p05 recorded as unrelated; the batch doesn't touch that file.
- No commit for plan03 yet. The user's instruction committed plan02 when it was complete; plan03 has no commit or push instruction, so its work stays in the worktree.
