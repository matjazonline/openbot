# 03PA-P01 agent review findings

Common context for the findings below, all raised in round `03PA-P01-D1-R1`:

- Date: 2026-11-14. Workflow: `implement-opus-reviewed-2_3`.
- Records: [TASK](TASK.md), [REVIEW](REVIEW.md), [notes](agent-review-notes.md#03pa-p01-d1-r1).
- Batch and stage: p01 B1, architecture gate.
- Reviewer: B, Opus 5.5 (`claude-opus-5-5`). The effort setting is not visible to the reviewer; the author requested xhigh.
- Phase `03PA-P01`; worktree `/Users/mac02/.gui-cloudroom/plugins/environment-git-worktree/host-data/worktrees/thr_jt43h4qc8d-1/openbot`.
- Reviewed: decision revision **D1/r1** (`logs/D1-r1.md`, sha256 `2105b82bcaf4b06df49758152e2d68a9087f2b9312a6fe5f60105ba5a47c773c`), against HEAD `cb46c6fd`, digest `eff341d36a36dddbf9b081322d91e1cc1a9f759425ab4546d812a9948be5d991`.
- All file:line references are at `cb46c6fd`. `init_schema` means `migrations/20260817000000_init_schema.sql`.
- Every finding below originates in the gated decision. None is an implementation error, because no implementation exists yet.

---

## 03PA-P01-D1-R1/F1 — Per-recipient principal refusal with the desk still admitted is not a mechanism the ingress has, and D1/r1 does not specify one

- **Classification:** actionable defect. Severity **High**. Categories: architecture, correctness, security.
- **What D1/r1 says:**
  - D1.4: "a principal recipient is refused per recipient with a typed reason … with bounce text … Desk recipients in the same message are unaffected."
  - D1.6 makes the decisive case "a message with one desk recipient and one person recipient admits the desk and refuses only the person."
- **What the code does:**
  - `resolve_addresses` is whole-message by design (`src/application/use_cases/thread/ingest/routing.rs:129`): "A pipeline is all-or-nothing: one undeliverable hop bounces the whole message". Every unknown or disabled slug goes into one `UndeliverableSlugs` accumulator. If it is non-empty, the whole message is rejected (`:214-217`), even when other candidates were accepted.
  - `IngestRejection` carries one whole-message bounce (`ingest/policy.rs:26-104`). There is no "accepted, with these recipients refused" result.
  - SMTP answers a rejection synchronously, with `550 5.7.1`, and an acceptance with 250 (`src/adapters/smtp/server.rs:797-825`). SMTP never enqueues a bounce email. `handle_bounce_dispatch` is called only by the SendGrid webhook (`routes/webhooks/sendgrid.rs:97`) and the Resend decoder (`resend_api/inbound.rs:174`).
- **What is unspecified:**
  1. **Result shape and reporting.** No result shape is defined for a partly accepted message. Nor is it defined how SMTP would report the refused part after answering 250. That needs a new asynchronous bounce at the public edge, which `src/adapters/smtp/AGENTS.md` warns against: "Do not answer 250 and rely on a best-effort bounce … backscatter risk".
  2. **Durability.** It is not stated how that bounce is made durable in the same logical commit as the admission (`src/application/AGENTS.md`: "Durable effects form one logical commit"; "Fire-and-forget is not acceptable for bounces"). A new platform-composed message also falls under C005/r1 item 1. So the statement "C001–C005 are unaffected" would not hold.
  3. **Pipelines.** A local part can mix a desk hop and a principal hop, for example `support+alice@` (selector grammar at `adapters/protocols/email/selector.rs:73-104`). D1/r1 does not say what happens then.
  4. **All-principal messages.** When every recipient is a principal, D1/r1 does not name the `IngestRejection` variant. It also does not give that variant's mapping in each consumer: `IngestRejection::as_str`, SMTP `for_ingest`, and Resend `ignore_reason` (`resend_api/inbound.rs:282-295`).
  5. **What outsiders learn.** A non-team sender would get the distinct reason "Direct messages to people and agents are not delivered yet", where an unknown address gets "Channel address not found". That confirms a member's address. D1.3 derives that address from the login username, which the login form accepts as "Email or username" (`routes/user.rs:566-577`). The existing code deliberately withholds directory information from non-team senders (`writable_channel_directory` at `routing.rs:672`, `answer_system_addresses` at `:720`).
- **Impact:** an implementer following D1.6 literally has to invent an unreviewed partial-delivery and bounce protocol at the public SMTP edge, and p02 replaces it anyway.
- **Required:** pick one of the following and state it in D1.4 and D1.6.
  - **Recommended: extend the existing whole-message undeliverable path for p01.**
    - Add a principal category to `UndeliverableSlugs`/`UndeliverableKind`.
    - SMTP keeps its synchronous 550; the webhooks keep their durable bounce.
    - A mixed message bounces whole, naming the principal recipient.
    - A non-team sender's text is identical to an unknown address's text.
    - Change D1.6's mixed-recipient assertion to match.
  - **Alternatively,** fully specify the partial-acceptance protocol: the result type, the SMTP behaviour, a durable bounce in the commit, C005 Message-ID ownership, pipeline semantics, outsider text, and a test per transport.
- **Instruction lesson** (hypothesis): the decision restated a plan phrase ("refused per recipient") without reading the existing all-or-nothing contract on the function it changes. Candidate rule for gated decisions: when a decision changes the routing outcome of one recipient, cite the current whole-message or per-recipient semantics of `resolve_addresses` and each transport's rejection mapping.
- **Resolution history:**
  - 2026-11-14: open.

---

## 03PA-P01-D1-R1/F2 — The internal relay does not reach `classify`/`resolve_addresses` first; a principal target becomes a retryable internal error

- **Classification:** actionable defect. Severity **Medium**. Categories: correctness, requirements.
- **Evidence:**
  - D1.4 says every path converges on `accept→route→classify`, then `preflight_inbound→resolve_addresses`, "including the internal relay (`render_email`'s `internal_target`→`single_channel_routing`). So one change covers them all."
  - In fact, `relay_internal` (`ingest/relay.rs:10-37`) first calls `authorize_relay` (`:197`). That calls `resolve_internal_destination` (`:42-103`), which looks the target up with `channel_persistence.get_by_company_slug_and_channel_slug` and, when there is none, returns `AppError::Internal("Platform channel does not exist…")` (`:66`).
  - Egress maps that `Err` to `ProviderSendOutcome::Retryable { class: Internal }` (`adapters/protocols/email/egress.rs:513`).
  - `internal_target` (`egress.rs:432-441`) sends every single-selector, non-context platform address to the relay, including a person's address.
- **Impact:** an agent's mail to a person's address is retried until its attempt budget runs out, not refused definitively. D1.6 "an internal relay to a person's address is refused" cannot pass from the change D1.4 describes, and the decision gives no definite-refusal contract for the relay.
- **Required:**
  - Name `resolve_internal_destination`/`authorize_relay` as a changed site.
    - Resolve the target through the namespace.
    - A principal target returns a definite `RelayDisposition::Refused(...)`, which becomes `Terminal { DestinationUnavailable }`.
    - Lookup failures still propagate as `Err`.
  - Have D1.6's relay check assert the `RelayDisposition` and the egress outcome, not only that nothing was ingested.
  - Record separately that an unknown desk slug takes the same retryable path today. That is pre-existing and out of scope unless the author chooses to fix it with the principal branch.
- **Instruction lesson:** the inventory (§4, "Internal relay skips classification") had this right; D1.4 contradicted its own evidence. Candidate: when a decision claims every path converges, list each path's first resolution site from the inventory.
- **Resolution history:**
  - 2026-11-14: open.

---

## 03PA-P01-D1-R1/F3 — D1.1 moves scope between phases without settling p02's new dependency, and without a coordination notice

- **Classification:** actionable defect. Severity **Medium**. Categories: requirements, coordination, verification.
- **Evidence:** the resequencing itself is justified. `workflow_escalation_agent_route` (init_schema, `FUNCTION public.workflow_escalation_agent_route`) joins `command_key='agent:'||agent.id`, then the desk, its binding and its email endpoint, which I verified. It does, however, leave two gaps.
  1. **p02 depends on agent addresses that now arrive only in p03.** Several of p02's own focused checks need mail to an agent's direct address (`02-direct-threads-and-bindings.md`):
     - `direct_thread_`: an agent's thread is visible to the team, and an outsider's mail to an agent bounces unless `open_to_public`;
     - `direct_binding_`: "an agent binding admits a run for a direct message";
     - `public_desk_private_agent_`: the same sender's direct email to the agent is refused, and admitted when only the direct address is opened;
     - `direct_competing_`: "two concurrent messages to the same agent".

     Under D1.1, agent principals have no address until p03, and each agent's slug is held by its default desk.
  2. **No notice records the move.** COORDINATION revision 7 has no notice covering the moved items:
     - into p02: 01B item 2, the "People and agents" heading, keeping `ContextSuffix`, and replacing `NotYetDeliverable`;
     - into p03: agent address registration, removing the automatic desk, "renaming an agent's slug renames its address", and the C001/C003 reroute.

     These exist only in p01's TASK. `references/concurrent-phases.md` requires a change affecting another phase to be recorded in the originating TASK and published as a requested and activated notice before dependent edits.
- **Impact:** p02 will either collide with the agent's desk slug, defer its agent checks silently, or drop items it never sees.
- **Required:**
  - D1.1 states how p02 meets its agent-target obligations. For example, p02 fixtures register an agent principal address at a slug no desk holds, and the decision names what that stand-in leaves unproved (production agent creation, agent-slug equality) for p03. Or those checks move explicitly to p03.
  - The whole move is requested as a cross-phase notice targeting p02, p03 and p04, and activated after this gate passes.
- **Instruction lesson:** a candidate for the skill's architecture-gate checklist: when a decision moves scope between phases, list each receiving phase's focused checks that depend on the moved items, and open the notice in the same revision.
- **Resolution history:**
  - 2026-11-14: open.

---

## 03PA-P01-D1-R1/F4 — The kind-guard and demotion trigger pair is a concurrency protocol, but D1/r1 gives neither its locking nor the edit path's lock order, and has no competing test

- **Classification:** actionable defect. Severity **Medium**. Category: concurrency.
- **Evidence:**
  - **The precedent locks.** `guard_department_principal_kind` (init_schema `:15528-15545`) reads the principal `FOR KEY SHARE`. Its comment explains why: "waits for a demotion in flight (a key update of `kind`)". `kind` is a key column, through `principals_company_id_id_kind_key` (`:6638`).
  - **D1/r1 states no locking.** D1.2's `guard_address_principal` is "BEFORE INSERT OR UPDATE", with no lock stated. D1.3's edit is a bare `UPDATE addresses SET slug WHERE principal_id=…`.
  - **Copying the precedent's lock onto every UPDATE deadlocks.**
    - The UPDATE locks the address tuple before its BEFORE ROW trigger runs.
    - The guard then waits for the principal row, which a concurrent `remove_member` holds (`company_invite.rs:571`; its lock order is account, then company, then principal, with `lock_timeout='5s'` at `:575`).
    - That demotion's `AFTER UPDATE OF kind` delete of the address waits for the address tuple.
    - The result is 40P01 between a self-edit and a member removal.
  - **Omitting the lock leaves I3 resting on an unstated invariant.** Every address insert would then have to happen in the transaction that created or promoted the principal. That is true in p01, unstated, and inherited by p03's new agent writers (creation, rename).
  - **No competing test.** Root `AGENTS.md`: "When adding or changing a concurrency protocol, include a test with competing claimants." D1.6 has only desk creation against a member join.
- **Required:**
  - State the protocol. For example:
    - the guard takes `FOR KEY SHARE` on INSERT and on `UPDATE OF principal_id, target_kind, channel_id`, but not on a slug-only update;
    - the edit path pre-locks in the established order, or locks the principal `FOR KEY SHARE` before the UPDATE, so that a concurrent removal yields 404.
  - Add competing tests on two connections:
    - a self-edit against a member removal;
    - an address insert against a demotion.

    Each asserts no deadlock or timeout error reaches the caller and no address remains for an `external` principal.
- **Instruction lesson:** the persistence AGENTS guidance does not mention that a BEFORE ROW trigger's own lock combines with the row's tuple lock. A candidate note for `src/adapters/persistence/AGENTS.md`: "a guard that locks a parent row inside a BEFORE UPDATE trigger runs after the child tuple is locked; check this against parent-side triggers that write the child."
- **Resolution history:**
  - 2026-11-14: open.

---

## 03PA-P01-D1-R1/F5 — Slug derivation can produce a slug the format check rejects, failing company creation or joining

- **Classification:** actionable defect. Severity **Low**. Category: correctness.
- **Evidence:**
  - D1.3 says "Trim the `-` from both ends and truncate to 48 characters." Truncating after the trim can end on `-`.
  - The format check `^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$` (init_schema `:3683`) refuses a trailing `-`.
  - The fallback applies only when the result is "empty, or `validate_slug` refuses it". `validate_slug` (`application/use_cases/channel.rs:437-465`) checks emptiness and reserved suffixes only, not the format.
  - `users.username` has only a not-blank constraint (init_schema `users`), and registration checks only that it is non-empty (`routes/user.rs:470`). So a username of 49 or more characters with a separator at the cut makes the INSERT fail with 23514, inside company creation (`company.rs:177`) or invite acceptance (`company_invite.rs:409`).
- **Required:**
  - Truncate, then trim, and check the candidate against the same format predicate the database enforces before the `member` fallback.
  - Add unit cases: a long username with a separator at the cut, a non-ASCII username, and a reserved-suffix username.
- **Suggestion:** reuse onboarding's existing `custom_slug` (`application/use_cases/onboarding.rs:355-365`) through one shared helper (`src/AGENTS.md`, "One decision, one place").
- **Resolution history:**
  - 2026-11-14: open.

---

## 03PA-P01-D1-R1/F6 — The new primary key drops the index that every desk-slug read used

- **Classification:** actionable defect. Severity **Low**. Categories: architecture, performance.
- **Evidence:**
  - **Today.** The primary key `channel_slugs_pkey (channel_id, slug)` (init_schema `:6119`) and `channel_slugs_primary_idx (channel_id) WHERE is_primary` (`:7926`) serve these lookups by `channel_id` alone:
    - `CHANNEL_SELECT`'s primary-slug and alias subqueries (`adapters/persistence/channel.rs:92-100`), which run on every channel load, including each ingress `DirectoryCache`;
    - `DELETE … WHERE channel_id = $1` (`channel.rs:531`);
    - `require_channel_reply_proposal_links` (init_schema `:13275`);
    - the cascade from `channels`.
  - **Under D1.2.**
    - The primary key is `(company_id, slug)`.
    - The only channel-side index is `addresses_desk_primary_idx (company_id, channel_id) WHERE is_primary`, so alias rows have no index by channel.
    - The readers above do not filter on `company_id`.
    - The table now also holds a row for every person.
- **Impact:** every channel load scans `addresses`. This removes existing index coverage; it is not a speculative optimization, so the persistence "production evidence" rule does not apply.
- **Required:**
  - Add a non-unique index on `(company_id, channel_id) WHERE channel_id IS NOT NULL`.
  - Have the moved readers include `company_id`.
- **Resolution history:**
  - 2026-11-14: open.

---

## 03PA-P01-D1-R1/F7 — I4 overstates what the schema proves, and D1.6/D1.7 leave required coverage out

- **Classification:** actionable defect. Severity **Low**. Category: verification.
- **Evidence:**
  1. **I4 overstates.** D1.5 I4 says "exactly one address per principal". The schema proves at most one (`addresses_principal_key`). Nothing proves that a person has one: dropping `principals.slug NOT NULL` dropped that existence guarantee.
  2. **The distinctive person paths are not in D1.6.** These are where the address write and the triggers interact:
     - the promotion branch (`participant.rs:358-373`, external to person by UPDATE);
     - rejoining after removal (a fresh derived slug; the old one freed);
     - the reuse branch (keeps its address).
  3. **Resend is hedged without reason.** D1.6 says Resend is covered "if its test harness allows". The harness exists: `resend_api/inbound_tests.rs:47` (`read` over `read_mail`) and `:174` `the_recipient_is_the_address_the_mail_was_received_for`. Plan `01-unified-addresses.md` requires the "signed provider-webhook paths", plural, and Resend is signed (`webhooks/resend_api.rs:139`).
  4. **Gates and tables are missing.**
     - `cargo test --locked --offline --test source_modules` should be run for new test files.
     - Discovered test names should be compared before and after for renamed or moved tests (`references/verification-contracts.md`).
     - TASK has neither the acceptance-mapping table nor the contract-impact table that reference requires.
     - D1.7 omits consumers that need a disposition: `sender_trust` (`thread/mod.rs:201`), simulation `preview_inbound_route` (`use_cases/channel.rs:367`) and `simulation_recipient_matches` (`routes/channel/simulation.rs:489`). The rejection-mapping consumers are covered by F1.
- **Required:**
  - Restate I4 as "at most one in the database, exactly one written on every production create or promote path".
  - Add a test for each person path: fresh, promotion, reuse, and rejoin.
  - Commit to a Resend check.
  - Add the missing gates and the two tables, with a disposition per consumer.
- **Resolution history:**
  - 2026-11-14: open.

---

## Optional suggestions (non-blocking)

### 03PA-P01-D1-R1/F8 — Tell the user about the resequencing and the schema deviations

- **Classification:** optional suggestion. Severity **Info**. Category: requirements communication.
- The README calls decisions 1–7 binding. D1/r1 departs from wording the user and the plan supplied in three places:
  - it replaces decision 1's column list (`target_id`) with `channel_id`/`principal_id`;
  - it drops plan 01A item 2 (`principals.slug`);
  - it resequences plan 04's order.

  All three are well reasoned, and the first is strictly stronger under the persistence AGENTS tenant-foreign-key rule. Record them in the phase and plan completion report so the user sees them.
- Record also, for p03's gate:
  - equality between `agents.slug` and the agent's address becomes an invariant across two tables;
  - the lazy upsert `resolve_author` (`adapters/persistence/thread/message.rs:312`, which calls `create_agent_principal_on` at `:337`) can create an agent principal without an address.
- **Resolution history:**
  - 2026-11-14: open (optional).
  - 2026-11-14, noted at round 03PA-P01-B1-R1: **closed**.
    - The user accepted the two schema departures: "no literal wording necessary, would just make it more complex". This is recorded in TASK under "B1 tests and verification mapping".
    - The resequencing is published as C006/r1 (active).

### 03PA-P01-D1-R1/F9 — Resolve in one read, and note what deriving from the username exposes

- **Classification:** optional suggestion. Severity **Info**. Categories: design, security note.
- D1.4 reads the desk list and the principal-address map separately. Resolving through one `addresses` read per company avoids a slug matching both, or neither, across two snapshots during a rename.
- Deriving the address from `users.username` publishes the login identifier as a mail local part. The login accepts "email or username" (`routes/user.rs:566-577`). This is the user's decision 1; it is worth mentioning to them, and the person can edit the address.
- Onboarding, department creation and agent creation at a slug the owner's derived address holds now fail with the conflict error. Add this to D1.5's failure modes, with wording that tells the user to rename.
- **Resolution history:**
  - 2026-11-14: open (optional).

---

# Round 03PA-P01-D1-R2 (recheck of D1/r2)

Context for the R2 entries and the resolution updates below:

- Date: 2026-11-14. Workflow: `implement-opus-reviewed-2_3`.
- Round [`03PA-P01-D1-R2`](agent-review-notes.md#03pa-p01-d1-r2). Batch and stage: p01 B1, architecture-gate recheck.
- Reviewer: B, Opus 5.5 (`claude-opus-5-5`); effort not visible to me.
- Phase `03PA-P01`, same worktree.
- Reviewed: decision revision **D1/r2** (`logs/D1-r2.md`, sha256 `f51249eeb72c36f941a6b25e8ac1849281e17edbb7b69ae30246404459693a81`). HEAD `cb46c6fd`, digest `eff341d36a36dddbf9b081322d91e1cc1a9f759425ab4546d812a9948be5d991` (unchanged).
- Coordination: revision 8, sha256 `f0a604f7…daca`.

## Resolution updates for the R1 findings (2026-11-14, against D1/r2)

| Finding | Status |
| --- | --- |
| R1/F1 | Fixed in substance; residual tracked as R2/F1 |
| R1/F2 | Fixed |
| R1/F3 | Fixed |
| R1/F4 | Fixed in substance; rationale error tracked as R2/F2 |
| R1/F5 | Fixed |
| R1/F6 | Fixed |
| R1/F7 | Fixed |
| R1/F8 | Deferred to the root's report to the user |
| R1/F9 | Addressed; refinement in R2/F3 |

- **03PA-P01-D1-R1/F1: fixed in substance.**
  - D1.4 now reuses the existing whole-message undeliverable path. A principal category is added to `UndeliverableSlugs`, SMTP keeps its synchronous 550, and the webhooks keep their durable bounce with C005 Message-ID handling.
  - Mixed pipelines are refused whole, and non-team bounce text equals the unknown-address text.
  - Residual: the SMTP reply line is not yet covered by the non-team equivalence or its assertion. Tracked as **R2/F1**.
- **03PA-P01-D1-R1/F2: fixed.** `resolve_internal_destination`/`authorize_relay` is named as a changed site.
  - A principal target returns a definite `RelayDisposition::Refused`, which egress records as terminal `DestinationUnavailable`. I confirmed the mapping at `egress.rs:500-517`.
  - Lookup errors stay `Err`.
  - D1.6 asserts both the disposition and the egress outcome.
  - The pre-existing retry for an unknown desk slug is recorded as out of scope.
- **03PA-P01-D1-R1/F3: fixed.**
  - D1.1 names the p02 fixture stand-in and its unproved limits, and has p03 rerun the checks on production-created agents.
  - 03PA-C006/r1 is proposed in COORDINATION revision 8 and activates only on a D1/r2 PASS.
  - Info: C006 lists three unproved items. D1.1 lists four, adding desk removal, which is p03's own scope anyway. Aligning them is optional.
- **03PA-P01-D1-R1/F4: fixed in substance.**
  - The guard fires only on INSERT and on `UPDATE OF target_kind, channel_id, principal_id`, so a slug-only edit skips it.
  - The edit is one statement that locks only the address row.
  - Competing tests are planned for edit against removal and insert against demotion.
  - I traced both lock orders for each pair; neither forms a cycle, and the edit gets 0 rows (404) after a committed removal.
  - Residual: the stated lock-mode premise is false. Tracked as **R2/F2**.
- **03PA-P01-D1-R1/F5: fixed.**
  - There is one shared `slugify`, from onboarding's `custom_slug`, followed by truncate, then trim.
  - The candidate is checked against the exact database format predicate and `validate_slug`, falling back to `member`.
  - The `-N` and 8-hex suffixes cannot form a reserved suffix.
  - `address_slug_` unit cases are planned.
- **03PA-P01-D1-R1/F6: fixed.** `addresses_channel_idx (company_id, channel_id) WHERE channel_id IS NOT NULL` is added, and the moved readers filter on `company_id`.
- **03PA-P01-D1-R1/F7: fixed.**
  - I4 is restated.
  - Tests are planned for the fresh, promotion, reuse and rejoin person paths.
  - The Resend check is committed.
  - Each consumer has a disposition: `sender_trust`, simulation and `_help` stay desk-only.
  - `source_modules` and the before/after `--list` comparison are in the gates.
  - The D1.7 acceptance and contract-impact tables are present.
- **03PA-P01-D1-R1/F8: deferred** to the root's report to the user, per the coordinator. I did not verify it; it is optional.
- **03PA-P01-D1-R1/F9: addressed.**
  - Resolution is one read.
  - The username exposure is noted.
  - Onboarding and department collisions are listed as failure modes.
  - A refinement is suggested as R2/F3, which is optional.

---

## 03PA-P01-D1-R2/F1 — The non-team indistinguishability claim is not carried to the SMTP reply line, the outsider's main channel

- **Classification:** actionable defect. Severity **Low**. Categories: security, verification. It originates in the gated decision: a residual of R1/F1.
- **Evidence:**
  - D1/r2 §D1.3 claims: "Outsiders cannot use bounces to enumerate addresses (D1.4)."
  - §D1.4 specifies the equivalence only under "Bounce text": "A non-team sender gets exactly the text an unknown address produces".
  - SMTP sends no bounce body. Its rejection is the synchronous line `550 5.7.1 Message rejected ({rejection})` (`smtp/server.rs:819-823`). That text comes from `IngestRejection::as_str`, which is keyed on `UndeliverableKind` (`ingest/policy.rs:83-86`).
  - So if the principal category is a new `UndeliverableKind` with its own `as_str`, the 550 line differs for a person and an unknown slug, and an outsider can enumerate members over SMTP even with bounce bodies identical.
  - D1.6's equivalence assertion covers the bounce text only. The external-ingress check asserts merely that "a person gets a synchronous 550".
- **Required:**
  - State that for a non-team sender the principal category produces the same rejection kind and `as_str` as an unknown address, for example `UndeliverableKind::UnknownChannel`. Only team senders get the principal kind and text.
  - Add the decisive SMTP assertion: a non-team sender's 550 line for a person's address equals its 550 line for an unknown slug. A team sender's line names the not-yet-deliverable reason.
- **Instruction lesson:** an indistinguishability property must be asserted on every channel the observer sees. For mail that means the synchronous SMTP reply as well as the bounce body. Candidate wording for the skill's verification-contracts examples: "a refusal's observable text per transport (SMTP reply line vs bounce body)".
- **Resolution history:**
  - 2026-11-14: open.

---

## 03PA-P01-D1-R2/F2 — D1.2's lock-mode premise is false; a demotion takes `FOR UPDATE`, not `FOR NO KEY UPDATE`

- **Classification:** actionable defect. Severity **Low**. Categories: architecture, concurrency documentation. It originates in the gated decision, in the r2 correction of R1/F4.
- **Evidence:**
  - D1/r2 §D1.2 says: "A demotion `UPDATE principals SET kind=...` takes `FOR NO KEY UPDATE` on the principal. `FOR SHARE` conflicts with that and `FOR KEY SHARE` does not".
  - That is false for this schema. `kind` is a column of the non-partial, immediate unique constraint `principals_company_id_id_kind_key UNIQUE (company_id, id, kind)` (init_schema `:6638`). PostgreSQL therefore treats it as a key column, and an UPDATE that changes it takes `FOR UPDATE`, which conflicts with `FOR KEY SHARE`.
  - The existing guard relies on exactly this, as its comment says: `guard_department_principal_kind`, init_schema `:15531-15535`, "FOR KEY SHARE waits for a demotion in flight (a key update of `kind`)".
  - Probe: [logs/D1-R2-review-lock-probe.txt](logs/D1-R2-review-lock-probe.txt) (sha256 `d1bad456…4c625`); script [logs/D1-R2-review-lock-probe.sh.txt](logs/D1-R2-review-lock-probe.sh.txt) (sha256 `3a819ea5…a457`). It ran on a disposable database on PostgreSQL 18.7, created and dropped by the script, with the same constraint shape.

    | Holder lock | Concurrent statement | Outcome |
    | --- | --- | --- |
    | `FOR KEY SHARE` | `UPDATE … SET kind=…` | blocked |
    | `FOR KEY SHARE` | `UPDATE … SET display_label=…` | proceeds |
    | `FOR SHARE` | `UPDATE … SET kind=…` | blocked |
    | `FOR SHARE` | `UPDATE … SET display_label=…` | blocked |

- **Impact:**
  - The chosen `FOR SHARE` still serializes the guard against a demotion, so the protocol's behaviour is correct.
  - The recorded premise, however, implies the existing department guard is broken. It invites a mistaken "fix" to that guard, or wrong lock choices in p03's agent writers.
  - The planned "insert against demotion" competing test passes under either lock mode, so it cannot catch the premise.
- **Required:**
  - Correct the D1.2 rationale. A demotion is a key update, because of `principals_company_id_id_kind_key`, and takes `FOR UPDATE`. Both `FOR KEY SHARE` and `FOR SHARE` serialize against it.
  - Then either keep `FOR SHARE`, giving the correct reason: it does not depend on `kind` staying in a unique key. It also blocks non-key updates of the principal, which is harmless here because every inserting transaction already holds that row. Or use `FOR KEY SHARE` to match the department guard.
  - State that `guard_department_principal_kind` is unchanged.
- **Instruction lesson:** a lock-mode claim in a gated decision should cite the key-column set, or be probed. Candidate for `src/adapters/persistence/AGENTS.md`, next to the guard precedent: "`principals.kind` is a key column (via `principals_company_id_id_kind_key`); updating it takes `FOR UPDATE`."
- **Resolution history:**
  - 2026-11-14: open.

---

## 03PA-P01-D1-R2/F3 — (optional) Scope the one-read resolution to the message's slugs

- **Classification:** optional suggestion. Severity **Info**. Categories: design, bounds.
- **Evidence:** D1/r2 §D1.4 loads "the company's whole `addresses` set" on each ingress. With person rows, that work scales with company membership on every inbound message, which `src/AGENTS.md` asks to bound ("Bound work at every external boundary"). This refines my own R1/F9 suggestion.
- **Suggestion:** a single `WHERE company_id = $1 AND slug = ANY($2)` read keeps the one-snapshot property, bounded by `MAX_ADDRESSED_TARGETS` × pipeline segments. Unknown-slug suggestions keep using the desk list.
- **Resolution history:**
  - 2026-11-14: open (optional).

---

# Round 03PA-P01-D1-R3 (delta recheck of D1/r3)

Context:

- Date: 2026-11-14. Workflow: `implement-opus-reviewed-2_3`.
- Round [`03PA-P01-D1-R3`](agent-review-notes.md#03pa-p01-d1-r3). Batch and stage: p01 B1, architecture-gate delta recheck.
- Reviewer: B, Opus 5.5 (`claude-opus-5-5`); effort not visible to me.
- Phase `03PA-P01`, same worktree.
- Reviewed: decision revision **D1/r3**, identified by its immutable copy `logs/D1-r3.md`, sha256 `d5fd5109c8d559a644ca810c82c2f54fb64bd5726697935aab1ba32e8f06d7ee`. HEAD `cb46c6fd`, digest `eff341d36a36dddbf9b081322d91e1cc1a9f759425ab4546d812a9948be5d991` (unchanged).
- Coordination: revision 8, sha256 `f0a604f7…daca` (unchanged).

## Resolution updates for the R2 findings (2026-11-14, against D1/r3)

- **03PA-P01-D1-R2/F1: fixed.**
  - D1.4 now records a non-team sender's principal recipient through the existing `undeliverable.unknown(slug, find_similar_channel_slugs(..))` path, with kind `UndeliverableKind::UnknownChannel`.
  - So the `IngestRejection` kind, `as_str()`, the SMTP `550 5.7.1 Message rejected (…)` line (`smtp/server.rs:819-823`), the bounce body (`notices.rs:102-140`) and the suggestions are the same code path as for a non-existent slug.
  - The principal kinds are for team senders only. Team status comes from `access_context(..).membership`, whose lookup errors propagate with `?`.
  - D1.6 adds the byte-equality check on the outsider SMTP line and equality of kind, text, bounce and suggestions.
  - I also traced the mixed cases for a non-team sender: desk + person, disabled desk + person, and person with a context suffix. Each produces the same output as the corresponding non-existent-slug case, because `kind()` precedence is unchanged.
- **03PA-P01-D1-R2/F2: fixed.**
  - D1.2 now states the correct premise: `kind` is in `principals_company_id_id_kind_key`, so a demotion takes `FOR UPDATE`. It cites the probe log.
  - It uses `FOR KEY SHARE`, the house mode. The probe confirms this serializes against a demotion without waiting on `display_label` updates.
  - The department guard is unchanged.
  - The guard still fires only on INSERT and `UPDATE OF target_kind, channel_id, principal_id`, so the slug-only edit protocol from r2, which I reviewed in R2, is unaffected.
- **03PA-P01-D1-R2/F3 (optional): adopted.**
  - There is one `WHERE company_id=$1 AND slug = ANY($2)` read per message, bounded by the recipient count, from a single snapshot.
  - Desk details come by id from the cached channel list, which still feeds the suggestions. A missing id is treated as an unknown desk.
  - `resolve_addresses` already refuses cross-company pipelines, so one company per message holds.

---

## 03PA-P01-D1-R3/F1 — Revision labels: the TASK copy and C006/r1 still name D1/r2 where they mean r3

- **Classification:** actionable records defect, owned by root. Severity **Low**. Category: records and coordination.
- **Not a defect in the gated decision text.** The decision is identified by `logs/D1-r3.md` and its sha256, and that file's title line says D1/r3.
- **Evidence:**
  1. TASK.md now holds two different sections with the same heading, "## D1 — architecture gate (revision r2, 2026-11-14)", at lines 166 and 365. Line 365 is the r3 text: it is byte-identical to the body of `logs/D1-r3.md`.
  2. The immutable copy carries the same labels inside the r3 decision: the heading on its line 3 says "revision r2", and its intro still says "D1/r2 replaces r1 as a whole".
  3. The coordinator's message said the section was appended as "D1 — architecture gate (revision r3)", but no such heading exists.
  4. COORDINATION revision 8, C006/r1, says "It activates when 03PA-P01 D1/r2 passes" (`:86`). Its order is "p01 D1/r2 PASS, then …" (`:94`), and the revision 8 note says "only on a D1/r2 PASS" (`:108`). D1/r2 did not pass. HANDOFF plans to activate C006/r1 in revision 9 after this round.
- **Impact:** "D1/r2" in TASK is now ambiguous between two texts. One uses `FOR SHARE` and the whole-set read; the other uses `FOR KEY SHARE`, the scoped read and the outsider rule. An implementer or acceptance check that follows the TASK heading can pick the wrong one. C006's activation condition, read literally, is never met.
- **Required** (root, before activating C006 or starting dependent edits):
  - Retitle the TASK section at line 365 as revision r3, or put a label line above it that names `logs/D1-r3.md` and its sha256. Leave `logs/D1-r3.md` unchanged: its sha256 is the identity this pass names.
  - Note in TASK that the r3 copy's own heading and intro carry the r2 labels over from r2.
  - In the activating COORDINATION revision, cite this D1/r3 PASS as the trigger, publishing a new C006 revision if the protocol requires one. State that §D1.1, C006's origin, is unchanged between r2 and r3; I confirmed this by diff.
- **Closure:** a records check by B or root. It does not reopen the gate unless the decision text changes.
- **Instruction lesson:** candidate for the skill, under Architecture gate, "revise a decision": "a revised decision gets its own revision in the heading, the intro and the immutable copy. Notices conditioned on a gate pass name the decision without a revision ('D1 PASS'), or are re-issued when the passing revision differs."
- **Resolution history:**
  - 2026-11-14: open.
  - 2026-11-14, round 03PA-P01-B1-R1 records check: **fixed**.
    - TASK's r3 section is now titled "D1 — architecture gate (revision r3, 2026-11-14) — PASS 03PA-P01-D1-R3" (TASK line 371).
    - `logs/D1-r3.md` is unchanged (sha256 `d5fd5109…`).
    - COORDINATION revision 9 activates C006/r1 "on the 03PA-P01-D1-R3 PASS for D1/r3" and states that D1.1 is unchanged from r2.
    - Remaining note: the revision 8 paragraph was edited in place rather than appended to (B1-R1/F7, optional).

---

# Round 03PA-P01-B1-R1 (batch review of p01 B1 under D1/r3)

Context:

- Date: 2026-11-14. Workflow: `implement-opus-reviewed-2_3`.
- Round [`03PA-P01-B1-R1`](agent-review-notes.md#03pa-p01-b1-r1). Batch p01 B1, the whole phase. Stage: batch review.
- Reviewer: B, Opus 5.5 (`claude-opus-5-5`); effort not visible to me.
- Phase `03PA-P01`, same worktree.
- Reviewed artifact:
  - phase start and source review base: `cb46c6fd`;
  - HEAD `830561e2`, which adds only two `.trail/learnings` files;
  - digest `888f0a805e0a1fa00b37892acf298493f573eca5897be111ba3fb9d7e255416c`, with the same four exclusions.
- Gated decision: D1/r3 (`logs/D1-r3.md` sha256 `d5fd5109…d7ee`). Coordination: revision 9 (sha256 `27e266bc…f0cf`).
- None of these findings originate in the gated decision; all are implementation or verification issues.

## 03PA-P01-B1-R1/F1 — The new address form gives no progress feedback

- **Classification:** actionable defect. Severity **Low**. Categories: UI, repository rule.
- **Evidence:**
  - `src/adapters/http/pages/profile.rs:204-211`: the address `<form hx-put="/ui/profile/addresses/{company_id}">` has no `hx-disabled-elt` and no busy label.
  - Every other profile form has `hx-disabled-elt="find button[type='submit']"` (`:340`, `:422`, `:448`, `:499`) and a "Saving..." span (`:359`).
  - `src/adapters/http/pages/AGENTS.md`, "Every user action needs visible progress feedback": disable the submitted button, show a present-tense status, and prevent duplicate actions.
- **Required:** use the same `hx-disabled-elt` and "Saving..." pattern as the sibling forms.
- **Resolution history:**
  - 2026-11-14: open.
  - 2026-11-14, recheck 03PA-P01-B1-R2 (digest `dc9329df…ca23`): **fixed**.
    - `profile.rs:204-216` now has `hx-disabled-elt="find button[type='submit']"` plus the identity form's spinner, "Save address" and "Saving..." spans, the same pattern as `:339-362`.
    - Verified by inspection.
    - The source scan `rendered_markup_carries_no_inline_javascript` raises nothing for `profile.rs`.

## 03PA-P01-B1-R1/F2 — Several D1.6 checks are missing, or do not exercise what they promise

- **Classification:** actionable defect. Severity **Low**. Category: verification.
- **(a) Self-edit against removal is tested only in the removal-first order.** (Corrected before return, after my probe.)
  - `address_tests.rs:591-622` holds the demotion open and then runs the edit.
  - **Edit-first is correct in production, but has no enduring test.** My probe on the real init schema, in a disposable database (`logs/B1-R1-review-edit-first-probe.txt`, script `.sh.txt`), held the production slug-only rename open and then ran the demotion. The demotion waits, then deletes the *renamed* row: no error, and 0 addresses left. That is variant A.
  - Variant B widens the guard to every UPDATE, the R1/F4 hazard. It also passes in held mode, because the rename takes the principal's `KEY SHARE` lock before the demotion starts. The R1/F4 deadlock needs a sub-millisecond interleaving that no hold-based test reproduces.
  - Add the edit-first order as a test. It pins that the follow trigger deletes by `principal_id`, so a renamed row is still removed.
  - Guard the deadlock property deterministically, with a schema assertion that `guard_address_principal` fires only on INSERT and `UPDATE OF target_kind, channel_id, principal_id` (via `pg_trigger.tgattr` or `information_schema.triggers`). Leave a comment saying why it exists.
- **(b) Deleting a user is not tested.** D1.6 promises that "removing a member, deleting a user or deleting an agent each drops the address".
  - Member removal is tested through production `remove_member` (`:450`).
  - The agent case is a raw stand-in, which is acceptable in p01.
  - User deletion (`user_lifecycle.rs:99`, a multi-company `UPDATE … WHERE user_id`) has no test.
- **(c) A department desk at a person's address is not tested.** D1.6 promises "desk creation, alias, rename and department-desk creation against a person". The test covers the first three (`:185-199`). A department desk goes through `plain_desk_on`/`provision_guarded`/`taken_slug`, which is correct by inspection but has no test.
- **(d) The competing tests run on the shared `test_pool`** (`Fixture::new`, `:39-40`).
  - `wait_until_a_backend_is_blocked` counts `wait_event_type = 'Lock'` across the whole database (`test_support.rs:639-654`). Under the parallel suite, another test's waiter can satisfy it, so the hold is released before this test's competitor blocks.
  - The assertions still pass, because the outcomes do not depend on order, so the contended interleaving goes silently unexercised.
  - House race tests isolate with `own_database` (`department_competing_tests.rs`, `company_access_level_race_tests.rs`). Use it for the three `address_namespace_competing_*` tests.
- **(e) The SendGrid "NOT EXERCISED" gap is avoidable.** `sendgrid/inbound.rs:257-282` already drives `decode_provider_request` with a multipart request.
  - A decode-then-`EmailIngressAdapter::accept` test would cover the signed SendGrid path at the same level as the Resend check (`resend_api/inbound_tests.rs`, routing for `alice@` equals routing for `support@`). Plan 01 names both "signed provider-webhook paths".
  - I verified the composition argument: `ingest_test_email` (`thread/test_support.rs:1635-1664`) drives the same accept→preflight path that SendGrid uses. So the behavioural risk is low, and the missing piece is an enduring regression check.
- **Required:** add the tests in (a), including the trigger-scope assertion, and in (b), (c) and (e). Move the competing tests onto `own_database` (d). Record the before/after test lists again.
- **Resolution history:**
  - 2026-11-14: open.
  - 2026-11-14, recheck 03PA-P01-B1-R2: **fixed**.
    - (a) `address_namespace_competing_held_edit_then_removal_leave_no_address_on_an_outsider` holds a slug-only row lock and then runs production `remove_member` on an isolated database. It asserts the removal blocks, completes without error, and leaves no address.
    - (a) `address_namespace_kind_guard_fires_only_when_a_target_changes` asserts `pg_get_triggerdef` contains `BEFORE INSERT OR UPDATE OF target_kind, channel_id, principal_id ON public.addresses`. This is the deterministic guard I asked for.
    - (b) `address_namespace_deleting_an_account_drops_its_addresses` calls production `delete_user_account`, which runs `finish_account_deletion_on` and the `user_lifecycle.rs:99` demotion.
    - (c) `address_namespace_a_department_desk_cannot_take_a_persons_address` (`department_lead_desk_tests.rs:281-312`) asserts a Conflict naming the slug and no new department. Its stand-in, a raw address for the fixture's raw person, is named in the test.
    - (d) `Fixture::isolated()` holds an `own_database`, and all four competing tests use it (`address_tests.rs:48-51`, `:528`, `:606`, `:640`, `:720`).
    - (e) `address_external_ingress_sendgrid_names_a_person_the_way_it_names_a_desk` (`sendgrid/inbound.rs:289-338`) follows `decode_sendgrid_request`'s steps, with the DNS verdicts stated rather than looked up. It covers the path at the Resend check's level, and the stand-in is named.
    - Test names: 4,493 before, 4,517 after; 24 added, 0 removed. I recomputed this from `B1-tests-after2.txt`.
    - My run: `logs/B1-R2-review-focus.txt`.

## 03PA-P01-B1-R1/F3 — `resolve_addresses` grew past the split threshold instead of extracting the new branch

- **Classification:** actionable defect. Severity **Low**. Categories: maintainability, repository rule.
- **Evidence:**
  - `routing.rs` `resolve_addresses` was already 101 lines at HEAD and is now 117 (`:133-249`).
  - The new `match directory.address_target(..)` sits inside the two `for` loops and adds a nested `if` for team status: four levels.
  - `src/AGENTS.md`: "past ~80 lines, past 3 levels of nesting … should be split now", and "When you edit a function that already breaks one of them, don't extend the violation — extract the part you're changing into a well-shaped helper".
- **Required:** extract the per-selector target classification into a helper that returns an outcome enum, so the loop is a `match` that pushes into the accumulators. The outcomes are desk candidate, principal refusal for a team sender, unknown with suggestions, and disabled. Behaviour is unchanged, and the existing resolution tests cover it.
- **Resolution history:**
  - 2026-11-14: open.
  - 2026-11-14, recheck 03PA-P01-B1-R2: **fixed**.
    - The new synchronous `addressed_slug` returns `AddressedSlug::{Desk(Box<Channel>), DisabledDesk, Principal, Unknown}`, and `record_undeliverable` records the refusal (`routing.rs:885-937`).
    - `resolve_addresses` is now 99 lines (`:133-231`) with one level less nesting.
    - I compared the semantics with R1: the same outcomes in the same order, team status read only for a principal target and propagated with `?`, and the stranger still recorded as `unknown` with the same suggestions.
    - The resolution and thread suites pass in my run.

## Optional suggestions

### 03PA-P01-B1-R1/F4 — Consider an `AddressSlug` newtype

- **Classification:** optional suggestion. Severity **Info**. Category: typing.
- Person addresses use `ChannelSlug`: `PersonAddress.slug`, `ResolvedAddress.slug`, `rename_person_address(.., &ChannelSlug)`. D1.4 anticipated an `AddressSlug`. It is the same namespace, so the newtype is optional. If p02 starts passing person and desk slugs side by side, src/AGENTS's newtype rule applies.
- **Resolution history:**
  - 2026-11-14: open (optional).
  - 2026-11-14, recheck 03PA-P01-B1-R2: **deferred** by root ("persons reuse `ChannelSlug` as desks do"). Accepted as optional.

### 03PA-P01-B1-R1/F5 — Pin the 409/400 classification now that the route answers 200 inline

- **Classification:** optional suggestion. Severity **Info**. Category: verification.
- **Accepted deviation (disclosure b).** D1.3/D1.6 specified 409/400 statuses. The route now answers 200 with an inline banner, matching the profile convention (htmx 2 does not swap 4xx responses), and that is acceptable house style.
- **Gap:** no test pins the use case's Conflict-vs-BadRequest classification. The session test asserts only message text, and both user test stubs return `Ok(None)`. A small use-case test would keep the HTTP contract the decision named.
- **Resolution history:**
  - 2026-11-14: open (optional).
  - 2026-11-14, recheck 03PA-P01-B1-R2: **dismissed** with root's reason: the persistence Conflict and the route's inline reasons are asserted. Accepted as optional; the classification only selects which inline message appears.

### 03PA-P01-B1-R1/F6 — Note for p02/p03: the relay's principal branch skips the sender check

- **Classification:** optional suggestion. Severity **Info**. Category: forward contract (C006 consumers).
- `authorize_relay` (`relay.rs:225-248`) now returns the principal refusal before the `mail.from` check against the source desk address.
- That is fail-closed today, because nothing is delivered. But when p02 makes principals deliverable, the sender check must still come before any principal delivery, so a relayed hop cannot borrow another desk's identity.
- Suggest recording this in p01's handoff for p02 and p03.
- **Resolution history:**
  - 2026-11-14: open (optional).
  - 2026-11-14, recheck 03PA-P01-B1-R2: **closed**. COORDINATION revision 10 appends "p01 review notes for later phases", which carries the relay sender-check rule, stranger equality and `ContextSuffix` to p02 and p03.

### 03PA-P01-B1-R1/F7 — Coordination revision 9 rewrote the revision 8 note in place

- **Classification:** optional suggestion. Severity **Info**. Category: records.
- COORDINATION revision 9 rewrote the revision 8 "D1 resequencing proposal" paragraph in place: "Its activation trigger is now the D1/r3 PASS; the original wording said D1/r2". `references/concurrent-phases.md` asks to retain prior text with its identity.
- The substance is preserved and quoted, so this is only a note. In future, append rather than rewrite.
- **Resolution history:**
  - 2026-11-14: open (optional).
  - 2026-11-14, recheck 03PA-P01-B1-R2: **closed**. Revision 10 is appended and leaves the earlier entries as they were at revision 9; the revision 8 paragraph's earlier in-place edit stays acknowledged.


---

# Round 03PA-P01-B1-R2 (recheck of the B1-R1 corrections)

- Date: 2026-11-14. Workflow: `implement-opus-reviewed-2_3`.
- Round [`03PA-P01-B1-R2`](agent-review-notes.md#03pa-p01-b1-r2). Reviewer: B, Opus 5.5 (`claude-opus-5-5`).
- Artifact: HEAD `830561e2`, digest `dc9329df080197b948e37372460f55476974a800811cb217182826193400ca23`, same four exclusions. Coordination revision 10 (sha256 `284a4c60…68fe`).
- **No new findings.** Each B1-R1 finding's status is recorded in its own history above.
- **Pre-existing unrelated failure, recorded and not a finding:** `adapters::http::pages::tests::rendered_markup_carries_no_inline_javascript` fails in my run (`logs/B1-R2-review-focus.txt`). It flags `src/adapters/http/pages/workflow_authoring/sample.rs:153`.
  - That file is unchanged against HEAD; its last commit is 48461f66.
  - The test's scan logic is unchanged: the batch only adds `addresses_html: ""` to `pages/tests.rs`.
  - plan02 p05 TASK (`plan/02-workflow-roles/phases/p05/docs/TASK.md:71`, `:83`) already records the same failure at the same line as unrelated.
  - The author's correction runs did not include `pages::tests`; I added it.
