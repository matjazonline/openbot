# Phase 5 — Dispatch wiring

Read [`general_plan_instructions.md`](general_plan_instructions.md) first. Depends on phases 1–4.

**Goal.** Make the stored capability actually run: build an `AgentCapabilitySpec` per agent, resolve
its harness from the registry, and apply the sub-agent scope in the two places that can reach a
sibling.

Small phase, but it contains the one authorization change in the plan.

---

## 1. Build the spec

`resolve_agent_params` (`agent_runner/params.rs` after the phase-3 split, `agent_runner.rs:377`
before it) becomes:

```rust
/// Load exactly the credential this agent selected, plus everything it may do, then discard the
/// persistence boundary before constructing the spec.
pub async fn resolve_agent_capabilities(
    company_persistence: &dyn CompanyPersistence,
    capabilities: &dyn AgentCapabilityReader,
    company: &Company,
    agent_id: Uuid,
) -> AppResult<ResolvedAgentCapabilities>;

pub struct ResolvedAgentCapabilities {
    pub spec: Box<AgentCapabilitySpec>,
    /// Stable identity for logs and authorization context; never substitute the user-authored name.
    pub agent_id: Uuid,
    /// Platform-native bounds/authorization policy consumed by native tool contexts, not compiled
    /// into a harness dialect.
    pub native_tool_policy: NativeToolPolicy,
    /// Never logged; passed to `sanitize_text` as the literal to redact.
    pub api_key: String,
}
```

`AgentCapabilityReader::load_for_execution(company.id, agent_id)` returns the agent row, skills and
resolved `SubAgentScope` from one consistent snapshot. Credential resolution is otherwise unchanged: the
company's default model connection, an agent-selected model only within a provider the company
enabled, and the hardcoded `google | openai | anthropic | groq` allowlist. The spec receives the
validated harness-specific config, stored harness kind, grants, skills and scope from that snapshot.
An absent or malformed snapshot fails the run explicitly; it never becomes an empty capability.

`AgentRunner::tool_policy` stops reading the `ai-agents`-shaped `extra_config`. It receives
`NativeToolPolicy` from `ResolvedAgentCapabilities` and gives each native tool its typed slice. This
is required for dependency direction: application authorization must not know a harness's
`tool_security.tools.<id>.config` path.

`ResolvedAgentParams` was `{provider, model, api_key, config}` with the merged config already
computed. That merge now happens in the phase-3 compiler, so this function no longer calls
`base_agent_config()`, `merge_json`, `ensure_config_fields` or `append_base_context_prompt` — all
four moved. If any of them is still called here, the layering did not land.

The reader may implement the snapshot as one query or a short read-only transaction, but dispatch
must see one version of the agent and both relationship sets. Each relationship is bounded by its
SQL position constraint. Do not add an inline `HashMap` for skills or sub-agents in `run_agents`;
if profiling later justifies caching, use the `LookupCache` shape from `src/AGENTS.md` and invalidate
the whole snapshot as one value.

## 2. Resolve the harness

`run_agents` (`use_cases/thread/dispatch.rs:791`) currently ends with:

```rust
match tokio::time::timeout(run_timeout, Box::pin(runner.execute())).await
```

`AgentRunner::execute` already keeps prompt composition, the spam guardrail, harness lookup and the
metrics record after phase 3. Phase 5 changes only the requested kind from
`HarnessKind::default()` to the stored snapshot and carries `agent_id` into `AgentRun` for structured
warnings:

```rust
let harness = self.harnesses.require(self.spec.harness)?;
// The seam into the harness runtime, whose own async chain is not ours to shrink. Boxing here
// caps what this side of the boundary contributes to it.
let output = Box::pin(harness.run(AgentRun { … })).await?;
```

`require` returns a typed error rather than `Option` for a reason worth restating at the call site:
an agent whose `harness_kind` has no registered implementation must **fail the run loudly**. A
fallback to the default harness would mean a deployment that drops a harness silently downgrades
every agent using it — the same class of bug as `.unwrap_or(false)` on an authorization path.

Do not rebuild or expose the registry through `AppState`. Phase 3 already constructs it in
`infra/setup.rs` and gives it to `ThreadUseCases`; a second path would allow runtime and UI views of
registered harnesses to drift. Startup registration remains unconditional for the in-process
harness.

Unchanged around it: the `tokio::time::timeout(agent.run_timeout(global), …)` wall clock, the
`while_leased` heartbeat in `task_worker.rs`, `AgentExecutionDisposition::Suspended` leaving the task
open, and dispatch's "if any matched agent failed, none of them may land" (`dispatch.rs:718`).

## 3. The sub-agent scope — the authorization change

`SubAgentScope::allows` (phase 1) is called by the one shared callable-target decision used in the
two externally visible places, and both matter:

1. **`agent_directory_tool.rs`** — `ListCompanyAgentsTool` lists sibling channels. `Restricted(ids)`
   filters the listing; `AllCompanySiblings` returns today's result.
2. **`outreach_tool.rs`** — `resolve_internal_target` / `InternalTargetOutcome`
   (`use_cases/channel.rs`) resolves a submitted selector to an internal channel. The same scope
   applies here before any outreach row or delivery is created.

Filtering only the listing would be the bug: a model that has seen a sibling's selector in an
earlier turn, or guessed it from the naming convention, could still call it. The candidate id is the
target channel's first configured agent, because `dispatch::first_agent_for` is the responder that
actually runs. A channel whose first agent is absent from `Restricted(ids)` is refused even if a
later configured agent is allowed. Document and test this ordering rule; changing channel dispatch
to run more than the first agent requires revisiting the authorization model first.

A direct global-library first responder also cannot appear in a company-owned allowlist, so a
restricted caller does not see or reach that channel. Copy the library agent into the company when
it must be an allowlisted target; do not special-case the global id past the composite-FK invariant.

Pass the scope into the shared resolved-target decision, not independently into two presentation
tools. The directory, outreach send path, and `InternalDelegationPolicy` classification must obtain
the same outcome. In particular, an excluded target must not be classified as an allowed internal
delegation for approval purposes and then refused only later.

From `src/AGENTS.md`, two rules land squarely here:

- **"Don't collapse errors into defaults on authorization paths."** The scope load propagates with
  `?`. No `.ok().flatten()`, no `.unwrap_or(false)`. If the deliberate fallback is "on error, refuse",
  write the `match` with an explicit `Err(e) => { warn!(…); refuse }` arm so the choice is visible.
- **"One decision, one place."** Both call sites go through `SubAgentScope::allows`. Do not
  re-implement the empty-means-everything rule at the second site; that is exactly the
  `is_trusted_participant` three-copies problem the rule was written from.

An empty allowlist means "every company sibling", so no existing agent changes behaviour. Say that
in the doc comment on `SubAgentScope`, because "empty means unrestricted" is the kind of inversion
someone later reads as a bug and "fixes".

`create_agent_channel` is not an authorization side door. It creates a channel and tells the model
to contact it in a later tool call; creation itself does not mutate either the persisted allowlist
or the immutable capability snapshot. Phase 4 therefore rejects the currently contradictory
combination of `Restricted` scope and an effective `create_agent_channel` grant. Do not "fix" that
validation by automatically adding the new agent id. Supporting restricted dynamic children later
requires a typed, auditable ephemeral-child policy and tests across creation, directory listing,
outreach and approval classification.

## 4. `agent_directory_tool` reads the catalogue too

The directory tool's output describes what a sibling is for, from `agents.description`. Nothing to
change there — but note that with skills stored, a sibling's *capabilities* are now knowable. Do not
add them to the listing in this phase; it widens the prompt for every run and is a product decision,
not a wiring one.

---

## Tests

**Pure**:
- `an_empty_sub_agent_scope_allows_every_sibling`
- `a_restricted_scope_refuses_a_sibling_not_on_the_list`

**Behavioural**, with the existing hand-written mocks in `services/test_support.rs`:
- `an_agent_with_no_registered_harness_fails_the_run` — assert the error names the kind, and assert
  it did **not** run on the default harness
- `a_restricted_agent_sees_only_its_allowlisted_siblings_in_the_directory`
- `a_restricted_agent_cannot_outreach_to_an_excluded_sibling_by_selector` — the one that would catch
  filtering the listing alone
- `a_spec_carries_its_agents_skills_in_position_order`
- `a_scope_read_error_fails_the_run_instead_of_becoming_unrestricted`
- `a_channel_is_authorized_against_the_first_agent_that_dispatch_will_run`
- `an_allowed_later_agent_does_not_authorize_an_excluded_first_responder`
- `a_restricted_agent_cannot_reach_a_direct_global_library_responder`
- `a_capability_snapshot_never_mixes_an_agent_row_with_a_different_relationship_version`
- `a_newly_created_agent_never_extends_a_restricted_scope_implicitly`

**Regression**: preserve the existing dispatch and `thread/*_tests.rs` behavior assertions; their
fixtures may need the new capability reader wired explicitly. An agent with no skills, no grants and
no allowlist has to behave exactly as it did before phase 4.

## Done when

An agent with a stored skill and a stored tool grant runs it end to end — see step 4 of the
verification in `general_plan_instructions.md` — and the whole suite is green with
`./scripts/stack-budget.sh` still passing.
