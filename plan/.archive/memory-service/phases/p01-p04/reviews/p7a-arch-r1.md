# Review p7a-arch-r1: phase 05 architecture gate (E1–E12)

Reviewer B, generation 4 (agent `a5916c4a63f198c6f`, nonce `run=8e7c1e85 role=B gen=4`).
Date 2026-11-08. Read-only: no project file was edited and nothing was committed.

## Artifact identity

- Full diff `tmp/memory-service-run/frozen/p7a-arch-r1.diff`: sha256
  `328b268e11bda89d4ad77167061301fd708740962f84ebe68228a44fa1490309` (recomputed, matches).
- Incremental from the accepted service tree `7043063f`:
  `tmp/memory-service-run/frozen/p7a-incremental-from-p6b.diff`: sha256
  `f604a5f7460696223e18bb74f85a7b12e8429b5d8dd38e55caeea04e50214cb5` (recomputed, matches); +300 lines
  in `plan/memory-service/implementation-decisions.md` only (section "Phase 05 decisions").
- Tree: recomputed with a temporary index, as `freeze.sh` does (base `a6bed48a`, `tmp/` excluded):
  `f723a92e9f5db2844e3eaa76d1a29238da0fa97c`. It matches the worktree.
- Mechanical checks on the record: `git diff --no-index --check` is clean and every local
  Markdown link resolves. 26 lines in the new section are over 100 characters, all of them table
  rows.

## Coverage

These originals were read in full: `plan/memory-service/README.md`, 01 (including the access-level
contract), 05, and 06 (06B.2 and the completion criteria); `AGENTS.md`; `src/AGENTS.md`;
`src/application/AGENTS.md`; `src/infra/AGENTS.md`; `src/adapters/persistence/AGENTS.md`;
`src/adapters/http/AGENTS.md`. From the service I read `services/memory/README.md`, the admin and
generation schemas in `openapi.json`, and decisions D3, D4, D5, D6, D8, D9, D11 and D13 plus the
P6b notes.

E1–E12 were then checked against code rather than taken from the record:

- Access levels and admission: `access_level.rs`, `knowledge.rs` (all of it),
  `knowledge_checkpoint.rs` (all of it), and `agent_history/frozen.rs` (all of it). In
  `agent_history/checkpoint.rs` I read eligibility, `record_on` and `accepted_on`.
- Locks and fencing: `lease.rs` (`lock_scope`, `lock_fence`) and `checkpoints.rs` (`lock_owner`).
- Release: `knowledge_release.rs` (`evaluate_on`) and the `raise_content_level_on` call graph.
- Children and actors: `children.rs` (`create_on`), message and schedule `admission_request`, and
  `lifecycle/bindings.rs` (how run-as is chosen).
- Agent history schema: the `agent_history_conversations` and `agent_history_sessions` DDL and
  guards, and the `messages_human_public` CHECK.
- Provisioning: `provisioning.rs` (application) and `knowledge_provisioning_tests.rs`.
- Waits and failure kinds: `wait_commit.rs` (effect waits) and `actions/failure.rs`.
- Company and integrations: `company.rs` (delete) and `integrations/lifecycle.rs`.
- Credentials: `credentials/envelope.rs`, `credentials/mod.rs` and `credential_rotation.rs`.
- HTTP and runtime: `workflow_jev/mod.rs`, `infra/workflow.rs` (`UNHANDLED_STEP_KINDS`) and
  `harness/rig/workflow.rs` (`run`).
- Registry: `registry/families.rs`, `schemas.rs` and `mod.rs` (bounds).
- Service tenant code: `services/memory/src/adapters/postgres/mod.rs` (tenant create, state change
  and `lock_tenant`) and `domain/admin_policy.rs`.

The remaining E anchors were checked for existence only, by printing the first line of each span.
They all exist, though three use abbreviated paths (n1).

Not covered: runtime behaviour. This is a documentation gate, so no Rust or database suite was run,
and none is required.

## Verdict

**Changes required.** The ports, adapter, credential custody, per-namespace credentials, intent
fencing and deadline alignment, and the decision to drop D11's batch split are sound. Seven
findings below are authorization- or classification-relevant and must be corrected in the record
before P7b. The run row is a valid serialization point for recall versus save, but the record
describes it wrongly (m1).

## Findings

### Major

**M1. E2: a 409 on the absent-tenant path is taken as "done", which can falsely complete a company
deletion.**

- *Location:* E2 ("Absent tenant: create, then delete … a tombstone's 409 means done").
- *Problem:* The service sends the same 409 for a tombstoned id and for a tenant managed by another
  admin (`services/memory/src/adapters/postgres/mod.rs` around 903–907; D3). `managed_by` is the
  admin *credential* id (`migrations/0001_init.sql:47`, `domain/admin_policy.rs:35`). Two cases
  therefore hit it:
  - OpenBot's provisioner token is rotated to a new credential.
  - The token is misconfigured to another provisioner.

  In either case `create` returns 409, and E2 records the deletion as complete. The tenant and all
  its memory stay live, and nothing is left that records it.
- *Required:*
  - On the absent path, always send `tenants/delete` after `create`, whether `create` returned 200
    or 409.
  - Only a 200 from delete completes the step. The service returns 200 for its own tombstoned
    tenant via the `(TenantAction::Delete, _)` arm, so a tombstone is still recognised.
  - A 404 from delete means unmanaged. Record it as a terminal `tenant_unmanageable`, surface it to
    operators, and never treat it as done.
  - Record the provisioner credential id (parsed from `msk_adm_<uuid>_…`) on each `memory_tenants`
    row when the tenant is created. When the configured token's id differs, refuse lifecycle work
    and log it.
  - Document that the service has no way to reassign `managed_by`, so the provisioner credential
    cannot yet be rotated (carry this to 06A credential rotation).
  - Add a `memory_scope_` case: a foreign-managed tenant must not complete deletion.

**M2. E2/E3: users other than the actor can reach the actor's private user scope.**

- *Location:* E2 ("user … only when the destination is the actor alone, kind `Actor` in E3"); E3
  (`Actor` includes "a … child-branch conversation" and schedule run-as); E4 (`{kind:"user"}` on
  steps).
- *Problem:* Four existing behaviours combine here:
  - `children.rs` `create_on` copies the parent's actor, channel and thread into the child.
  - A message-triggered run's actor is the binding's `run_as` (`message_admission.rs:484`).
  - A schedule's `run_as` may name another owner or admin (`lifecycle/bindings.rs:26-31`).
  - `audience_on` checks a `child_branch` conversation against the actor's level.

  So under E3:
  - A child of a thread run reads and writes the run-as user's private namespace, and its output
    returns to the thread.
  - A schedule configured by admin B with run-as A exposes A's memories to B's chosen channels.
  - A manual run of someone else's workflow containing `memory.load {kind:"user"}` can forward the
    actor's memories wherever that workflow sends output.

  Levels cannot protect any of this. Human content is always level 0 (`messages_human_public`,
  `CHECK (role <> 'human' OR access_level = 0)`), so private user memories are mostly level 0.
- *Required:* Define user-scope eligibility as "the reader set is the actor":
  - Allowed: a `private_session` conversation whose creator is the run actor (the invariant
    `guard_workflow_agent_session` enforces), for tools, recall and capture.
  - Children inherit the *root's* eligibility. A child of a message run, a schedule, or a
    non-session run is never eligible.
  - Workflow-step `{kind:"user"}` is refused unless the root is a manual or private-session run of
    that actor. Dropping it from steps for the first release is also acceptable.
  - Settle the case of a private session attached to a thread explicitly.
  - Add `memory_scope_` cases: a child of a thread run, a schedule whose run-as is another user, and
    a foreign-authored manual workflow.

**M3. E3: exactness is fail-open.**

- *Location:* E3 (`content_exact … DEFAULT true`, only lowered on the paths listed in the table).
- *Problem:* Any model-visible path the table misses stays "exact" and under-labels saves. That
  includes future step kinds, future builtins and integrations, and any path the audit missed. 05
  says "Never assume that missing evidence means level 0" and "Fix untracked input paths". A
  default of true with opt-out does the opposite.
- *Required:*
  - Put one fail-closed hook at the generic boundaries where content becomes model-visible: stored
    step output consumed by later steps, committed tool results, and wait payloads. The hook lowers
    `content_exact` unless the kind is in an explicit allowlist of level-accounted kinds.
  - Add a coverage test in the style of `production_handler_covers_every_io_kind_or_refuses_it`.
    It enumerates every registered step kind and every builtin tool and asserts that each is
    either allowlisted with a raise or lowers exactness.

**M4. E3: children do not inherit inexactness.**

- *Location:* E3 ("a child's inexact flag propagates"); `children.rs:229-285`.
- *Problem:* Child admission copies `knowledge_ceiling` and `content_level`, but E3 only carries
  exactness from child to parent. A parent that consumed an unlabelled HTTP or MCP receipt passes
  that data as child input. The child then starts exact at the parent's level and saves an exact
  label that is too low.
- *Required:*
  - Child admission copies `content_exact`, and so do workflow-tool children.
  - Add a `memory_access_level_` case: an inexact parent's child saves through the fallback.

**M5. E4/E5: the existing release evaluator would refuse every classified save, or force an
unspecified bypass.**

- *Location:* E4 (`memory.save` joins `marker_owned_effect_kind`); E5 (target kind `memory`).
- *Problem:* `knowledge_release::evaluate_on` resolves every Local target other than http, mcp and
  messaging to `AccessLevel::PUBLIC` (`knowledge_release.rs:204`). This is the arm that already
  makes provisioning a level-0 destination (`knowledge_provisioning_tests.rs`). Routed through it,
  a consumed-6 save would fail `KnowledgeRelease`. The record says nothing about how the new target
  interacts with release, so the implementer is left to invent an exemption.
- *Required:*
  - Add an explicit `memory` arm. A memory save is a labelled store: release is allowed because the
    frozen label is at least the content level by construction.
  - The arm is reachable only from the memory target.
  - Test both: a consumed-6 save passes release, and no other resource kind reaches the arm.
  - State that read queries sent to the service (recall query, tool queries) are not release-checked
    egress. The reason is that the service is the deployment's labelled store and persists no read
    text: reads write no audit events (service README), and logs carry ids only.

**M6. E8/U3: automatic thread capture crosses channel and thread boundaries.**

- *Location:* E8 ("the agent scope for thread conversations").
- *Problem:* Thread exchanges would be captured into the agent namespace. Human thread content is
  always level 0, so after capture anyone running that agent in any channel or thread can recall
  it, including channels whose members are disjoint from the original. Levels give no protection.
  Existing agent history is scoped per thread conversation (`agent_history_conversations` thread
  index). Capture would therefore be the first *automatic* cross-channel flow of human
  conversation content, and 05B.5 excludes "unrelated private activity".
- *Required:* For the first release, capture only private sessions, into the creator's user scope.
  Thread capture is either deferred or placed behind an explicit per-agent or per-channel policy
  that states the memory is shared across the agent's channels. This is a user decision; see U3.

**M7. E9: owner deletion races provisioning and enable, and can leave a live remote resource for
a removed owner.**

- *Location:* E9 and E2 (tables and lazy provisioning).
- *Problem:* E9 orders the deleting transaction, but not the competing transactions that create
  state:
  - `delete_company_with_cleanup` (`company.rs:104-142`) checks `EXISTS` and then deletes, without
    locking the company row first.
  - A concurrent memory enable, or a lazy namespace provision, can commit between the delete's
    lifecycle write and its `DELETE`. It leaves a lifecycle row with no foreign key and
    `desired=present` for a company that has been deleted.

  The same applies to agent delete or retire, member removal, and account deletion against lazy
  user and agent namespace provisioning. The worker then creates a tenant or namespace for a
  removed owner. That is not resurrection of a tombstone, but it is the same failure class.
- *Required:*
  - The deleting or removing transaction locks the owner row first (company `FOR UPDATE`,
    agent row, membership row).
  - Enable and provision lock it `FOR SHARE` and re-check that the owner is still live.
  - Writes of `absent` are upserts.
  - The worker refuses `present` when the owner is gone.
  - Competing-claimant tests in `memory_scope_`: enable against company delete, lazy provision
    against agent delete and against member removal.

### Minor

**m1. E6: the serialization point and lock order are described wrongly, and the competing test
cannot fail on the realistic regression.**

- *Problem:*
  - `lock_fence` → `lock_scope` already takes `workflow_runs … FOR UPDATE` first, after the
    authority and reply prelocks (`lease.rs:291-294`). The added
    `SELECT … FOR NO KEY UPDATE` is therefore a no-op.
  - The real order is: authority/company prelock → run `FOR UPDATE` → execution → job → namespace
    (foreign-key share on insert) → intent. "Fence/owner → run row" misstates it.
  - As written, the E6 test sees the wait at `lock_fence`, so it passes even without the new lock.
    It never exercises the mistakes that matter: freezing from claim-time `RunKnowledge` or from a
    value read before the lock, and exposing text before the raise commits.
- *Required:* Correct the text. Either drop the redundant lock or label it as documentation. Add
  both mutations to the competing test and record that the test fails under each.

**m2. E6/E9: lock order for removal and settlement is unspecified, and pending intents can hang.**

- *Problem:*
  - Namespace removal "cancels pending intents in their transaction". If that also settles effect
    waits or runs, it takes namespace → run, the reverse of save's run → namespace/intent.
  - The delivery worker's completion should write the receipt only, and let the wait poll wake the
    run (`wait_commit.rs` `effect_resume_on`), as message delivery does.
  - Save must lock the namespace `FOR SHARE` and require `ready`.
  - Memory disable and run cancellation should terminally fail pending intents and wake their waits,
    instead of letting them expire after 6 days.
- *Required:* State all of this in E6/E9.

**m3. E5: SaveMemory has no scope and no key rule.**

- *Problem:* The tool input has no scope, and E4's key covers only the step.
- *Required:*
  - Specify the target scope: agent scope, and user scope only under M2.
  - Specify a per-invocation key, for example `ob.t.<invocation uuid>`.
  - Bound `external_sources` (at most 64, chosen deterministically). These are provenance only: the
    watermark already covers every recalled level.

**m4. E8: the capture session is unspecified, and the deadline wording is inconsistent.**

- *Problem:*
  - `GenerationRequest.session_id` is a service UUID returned by `sessions:open` (`external_id` =
    conversation). "Session = conversation id" therefore needs its own idempotent write, with key
    and `expires_at`, before the generation body can be frozen.
  - Generation intents expire at the policy expiry (up to 168 h), not "6 days".
- *Required:* Either add that step or omit `session_id` in the first release (no summaries). Fix
  the deadline text.

**m5. E3: human input is classified differently on different paths.**

- *Problem:* Manual-run input is "inexact", schedule parameters are level 0, human messages are an
  exact 0 (`messages_human_public`), and `decision.human` feedback is "inexact". Yet admission
  sets content PUBLIC for both manual and schedule runs (`knowledge.rs:104-107`).
- *Required:* One rule for human-authored input (see U2).

**m6. E1/E7: capacity and fairness.**

- *Problem:*
  - One `MAX_IN_FLIGHT` semaphore (8) is shared by recall, tools and the delivery and lifecycle
    workers.
  - There is no per-company bound (src/AGENTS.md: one tenant's burst must not starve the others).
  - A background burst freezes recall as `unavailable` for that preparation, permanently.
  - Recall inherits the 12 s timeout, for up to three scopes, before every `agent.run`.
- *Required:* Separate permits for interactive and background work, a per-company cap, and a
  recall deadline well below `MEMORY_SERVICE_TIMEOUT_MS` (for example about 2 s for all scopes
  together, run concurrently).

**m7. E7: validation cadence and the ceiling re-check are unclear.**

- *Problem:*
  - "Before every provider entry except the attempt that froze it" reads as once per tool round,
    which conflicts with 05B.1 ("do not reload memory on every tool round").
  - The freeze re-checks the ceiling, but the record does not say what happens when it fails.
- *Required:* Say "once per resumed attempt, before its first provider entry". When the ceiling
  re-check at freeze fails, discard the pack unseen and freeze `unavailable{ceiling_changed}`.

**m8. E4/E1: per-company gating and orphaned lifecycles.**

- *Problem:*
  - The configured refusal set covers the whole deployment. A company without memory enabled can
    still publish memory steps and bind the tools.
  - Unsetting `MEMORY_SERVICE_URL` while lifecycles are pending, deletes included, strands remote
    data silently.
- *Required:* Refuse at publication or fail with a terminal `memory.not_enabled`. Log a startup
  warning naming every pending lifecycle when the URL is unset.

**m9. E11: test-database and schema statements are incomplete.**

- *Problem:*
  - src/AGENTS.md prescribes `localhost:5432` for `cargo sqlx prepare`, which this run forbids.
  - `test_pool` falls back to redirecting `DATABASE_URL` to its `_test` sibling, and that must not
    engage.
- *Required:*
  - Name the private-cluster URL for `prepare --check`, and require `DATABASE_URL` unset or
    pointed at the cluster.
  - State that the tables are added to `migrations/20260817000000_init_schema.sql` in place.

### Nits

- n1. Abbreviated anchors. `agent_preparation.rs` is under `src/adapters/persistence/workflow/`,
  `builtins()` is in `src/adapters/persistence/agent_authoring/catalog.rs`, and `knowledge/ports.rs`
  is under `src/application/workflow/`.
- n2. The E5 origin refusal is defense in depth, not the boundary: no company holds a client
  credential. Say so, and note that it misses alias forms (IP versus name, port defaults).
- n3. `memory.load`'s `exposed_level` comes from fetch, so it includes items later omitted for
  budget. That is conservative; state it.
- n4. No E path consumes `MemoryClient` generation status. Drop it until something does.
- n5. The record's header still describes it as an expansion for 01–04.
- n6. Agent retire makes its namespace absent, which cannot be undone. Confirm that retire is final.

## Assessment of U1–U3 (recommendations for root to put to the user)

### U1: per-company opt-in

**Recommend opt-in, as proposed.**

- Enabling creates a remote tenant over which the provisioner holds transitive full data authority
  (D3), and once an operator cap is set it opens a path to Anthropic. Consent per company keeps
  that blast radius to companies that chose it.
- Provisioning every company would create many idle tenants and lifecycles for no product benefit.
- Pair it with defined disable semantics (tenant disabled, data retained, pending intents failed,
  see m2) and a later "delete memory" path (06A).

### U2: authored configuration counts as level 0 for exact accounting

**Recommend level 0, which is OpenBot's existing semantics and not a new rule. Apply the same rule
to every human-authored input.**

The evidence:

1. Human-authored content is level 0 by schema constraint: `messages_human_public`,
   `CHECK (role <> 'human' OR access_level = 0)`.
2. Admission sets content PUBLIC for manual and schedule runs (`knowledge.rs:104-107`).
3. Agent instructions, skills, workflow literals and schedule parameters carry no level column.
   Only members, invites, connections, messages, knowledge documents and chunks, and exchanges do.
4. The one way an agent writes configuration, Agent Builder's `create_agent_channel` (it writes
   `system_prompt` and custom steps), is already a level-0 release destination. The `Local → PUBLIC`
   arm in `knowledge_release.rs` handles it, and `knowledge_provisioning_tests` refuses
   provisioning at content 5. So configuration cannot carry system-classified content without an
   earlier release violation.

Declassification risk: a cleared person can type classified facts into instructions. That is the
same risk the existing system already accepts for human messages. Memory widens persistence (notes
at level 0 in company or agent namespaces), so document it as a limitation.

Treating configuration as inexact would make every agent save fall back to the ceiling, which
leaves 05's "consumed 4 saves at 4" unreachable.

For consistency (m5), manual run input and `decision.human` feedback should also be an exact 0.
Keep the fallback for unlabelled machine inputs: HTTP and MCP receipts, `wait.event` payloads and
Jev results.

### U3: recall bound to the `SearchMemory` binding; private-session capture to the user scope with no tool results

**Recommend accepting both, with conditions, and putting one more question to the user.**

- **Recall tied to the `SearchMemory` binding: accept.** The binding is already the explicit read
  authorization, frozen at publication (06-D5, no narrowing). A separate toggle would add UI and a
  second source of truth. Condition: the recall deadline and fairness bounds in m6.
- **Private-session capture into the creator's user scope, without tool arguments or results:
  accept.** The creator is the only reader, and the exchange level already covers anything the
  output absorbed from tools. Condition: M2's eligibility rule.
- **Add a question, U3c:** should thread conversations be captured into the agent scope at all in
  the first release? I recommend no (M6): it moves human conversation content across channels,
  and levels cannot stop it.

## Assumptions challenged

- *"A tombstone's 409 means done."* False: the 409 is uniform, and `managed_by` is tied to the
  credential id (M1).
- *"A child-branch conversation is the actor alone."* That holds for the audience-level check, but
  not for privacy: the child's output returns to the parent's destination (M2).
- *"Default exact with an audited opt-out list satisfies 'never assume 0'."* No, it fails open
  (M3). The audit also missed the parent → child direction (M4).
- *"A memory save is just another marker-owned effect."* The existing release arm makes it a
  level-0 destination (M5).
- *"The run row lock is a new serialization point."* It already exists in `lock_scope`. The
  protocol holds, but the described lock order and the test's discriminating power do not (m1).
  Both orderings preserve the watermark for a deeper reason: save content is fixed before any
  later exposure, and every exposure commits its raise first.
- *"Thread capture to agent scope is authorized by levels."* Human content is always level 0, so
  levels authorize nothing here (M6).
- *"Ordering the delete transaction prevents re-creation."* Only if creating transactions take the
  same owner lock (M7).
- *Accepted after checking:*
  - Dropping D11's batch split is coherent. With 64 facts, a deterministic terminal refusal above
    3.5 MiB and one atomic key, no partial-success state exists. D11 itself allowed shrinking the
    contract.
  - The 6-day save deadline against the service's 7-day idempotency window (D5), with `expires_at`
    frozen and resent, cannot re-execute and cannot become a new generation request.
  - One client credential per namespace, `enc:v2` under a new `CredentialContext` and listed in
    `CredentialTable`, matches persistence AGENTS.
  - The DB-test target (private cluster at 55432, `own_database` for unscoped claims) follows the
    run's constraint.
  - Over-engineering is limited: n4 only. The proposed P7b–P7f split is reasonable.

## Remaining checks for the next round

Re-review the corrected E2, E3, E5, E6, E8 and E9 text against M1–M7 and m1–m9, and confirm that
the user's answers to U1, U2, U3 and U3c are reflected. No code evidence exists yet, so none is
invalidated.
