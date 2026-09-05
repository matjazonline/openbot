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
    skills: &dyn SkillPersistence,
    company: &Company,
    agent: &AgentEntity,
) -> AppResult<ResolvedAgentCapabilities>;

pub struct ResolvedAgentCapabilities {
    pub spec: Box<AgentCapabilitySpec>,
    /// Never logged; passed to `sanitize_text` as the literal to redact.
    pub api_key: String,
}
```

Credential resolution is unchanged: the company's default model connection, an agent-selected model
only within a provider the company enabled, and the hardcoded `google | openai | anthropic | groq`
allowlist (`:306`). Only the config assembly changes — `extra_config` is the agent's raw
`config_json`, and `skills` / `granted_tools` / `sub_agents` come from the new columns and tables.

`ResolvedAgentParams` was `{provider, model, api_key, config}` with the merged config already
computed. That merge now happens in the phase-3 compiler, so this function no longer calls
`base_agent_config()`, `merge_json`, `ensure_config_fields` or `append_base_context_prompt` — all
four moved. If any of them is still called here, the layering did not land.

Loading skills is one extra query per agent run. `list_for_agent` is indexed by
`agent_skills_skill_idx` and returns at most `MAX_AGENT_SKILLS` (16) rows; it does not need a cache.
If profiling later says otherwise, the `LookupCache` shape from `src/AGENTS.md` — one small struct
holding the persistence handles and its maps — is the answer, not an inline `HashMap` in
`run_agents`.

## 2. Resolve the harness

`run_agents` (`use_cases/thread/dispatch.rs:791`) currently ends with:

```rust
match tokio::time::timeout(run_timeout, Box::pin(runner.execute())).await
```

`AgentRunner::execute` keeps prompt composition, the spam guardrail and the metrics record, and
gains one step in the middle:

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

`infra/setup.rs` builds the registry beside `MemoryProviderRegistry`:

```rust
let harnesses = Arc::new(
    HarnessRegistry::default()
        .register(Arc::new(AiAgentsHarness::new()))?,
);
```

`AppState` gains `harnesses: Arc<HarnessRegistry>`. Registration is unconditional — unlike the
memory providers, which are conditional on deployment credentials being present. The in-process
harness needs no credentials of its own.

Unchanged around it: the `tokio::time::timeout(agent.run_timeout(global), …)` wall clock, the
`while_leased` heartbeat in `task_worker.rs`, `AgentExecutionDisposition::Suspended` leaving the task
open, and dispatch's "if any matched agent failed, none of them may land" (`dispatch.rs:718`).

## 3. The sub-agent scope — the authorization change

`SubAgentScope::allows` (phase 1) is called in **two** places, and both matter:

1. **`agent_directory_tool.rs`** — `ListCompanyAgentsTool` lists sibling channels. `Restricted(ids)`
   filters the listing; `AllCompanySiblings` returns today's result.
2. **`outreach_tool.rs`** — `resolve_internal_target` / `InternalTargetOutcome`
   (`use_cases/channel.rs`) resolves a submitted address to an internal channel. The same scope
   applies here.

Filtering only the listing would be the bug: a model that has seen a sibling's address in an earlier
turn, or guessed it from the naming convention, could still mail it. The recipients are already
classified against the channel directory rather than taken on the model's word — that is what
`InternalDelegationPolicy` (`agent_runner.rs:575`) exists for — so the scope check belongs at the
same point, on the resolved target.

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
- `a_restricted_agent_cannot_outreach_to_an_excluded_sibling_by_address` — the one that would catch
  filtering the listing alone
- `a_spec_carries_its_agents_skills_in_position_order`

**Regression**: the existing dispatch and `thread/*_tests.rs` suites must pass unmodified. An agent
with no skills, no grants and no allowlist has to behave exactly as it did before phase 4.

## Done when

An agent with a stored skill and a stored tool grant runs it end to end — see step 4 of the
verification in `general_plan_instructions.md` — and the whole suite is green with
`./scripts/stack-budget.sh` still passing.
