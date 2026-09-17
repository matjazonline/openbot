//! Agent list, editor and the AI prompt generator.

use super::*;

pub fn render_ai_prompt_generator(
    company_id: Uuid,
    sys_prompt_id: &str,
    gen_box_id: &str,
    gen_input_id: &str,
    gen_status_id: &str,
    include_form_ids: &str,
) -> String {
    let hx_vals = format!(r#"{{"target_id": "{sys_prompt_id}", "gen_box_id": "{gen_box_id}"}}"#);
    let hx_target = format!("#{gen_status_id}");
    let hx_include = format!("#{gen_input_id}{include_form_ids}");

    format!(
        r#"
        <div class="flex items-center justify-between mb-1">
            <label for="{sys_prompt_id}" class="block text-xs font-medium text-base-content/80">System Prompt</label>
            <button type="button"
                data-action="toggle-prompt-generator" data-box="{gen_box_id}" data-focus="{gen_input_id}"
                class="text-xs text-primary hover:text-primary font-medium transition cursor-pointer inline-flex items-center gap-1">
                <span class="inline-flex items-center gap-1">{sparkle} Generate with AI</span>
            </button>
        </div>

        <div id="{gen_box_id}" class="hidden my-2 p-3 bg-base-100 border border-primary/40 rounded-xl space-y-2.5 shadow-inner">
            <div class="flex items-center justify-between text-xs font-semibold text-primary">
                <span class="flex items-center gap-1.5">
                    {sparkle}
                    <span>Generate System Prompt with AI</span>
                </span>
                <button type="button" data-action="hide-element" data-target="{gen_box_id}" class="text-base-content/70 hover:text-base-content transition cursor-pointer">&times;</button>
            </div>
            <p class="text-[11px] text-base-content/70">Describe what you want this agent to do (e.g. role, responsibilities, rules, tone):</p>
            <textarea id="{gen_input_id}" name="user_instructions" rows="2"
                placeholder="e.g. A helpful support agent that answers questions about billing and refunds politely..."
                class="w-full px-2.5 py-1.5 bg-base-300 border border-base-300 rounded-lg text-base-content text-xs placeholder-base-content/60 focus:outline-none focus:ring-1 focus:ring-primary font-sans"></textarea>

            <div id="{gen_status_id}" class="text-xs"></div>

            <div class="flex items-center justify-end gap-2 pt-0.5">
                <button type="button" data-action="hide-element" data-target="{gen_box_id}"
                    class="px-2.5 py-1 bg-base-300 hover:bg-base-content/15 text-base-content text-xs font-medium rounded transition cursor-pointer">
                    Cancel
                </button>
                <button type="button"
                    hx-post="/companies/{company_id}/agents/generate-prompt"
                    hx-target="{hx_target}"
                    hx-swap="innerHTML"
                    hx-include="{hx_include}"
                    hx-vals='{hx_vals}'
                    hx-disabled-elt="this"
                    class="px-3 py-1 bg-primary hover:bg-primary/90 disabled:opacity-60 disabled:cursor-not-allowed text-primary-content text-xs font-semibold rounded shadow transition cursor-pointer flex items-center gap-1.5 [.htmx-request_&]:pointer-events-none [.htmx-request_&]:opacity-80">
                    <svg class="animate-spin h-3.5 w-3.5 text-base-content hidden [.htmx-request_&]:inline-block shrink-0" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
                        <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                        <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                    </svg>
                    <span class="[.htmx-request_&]:hidden">Generate</span>
                    <span class="hidden [.htmx-request_&]:inline">Generating...</span>
                </button>
            </div>
        </div>
        "#,
        sparkle = icon(Icon::Sparkle, BUTTON_ICON),
        company_id = company_id,
        sys_prompt_id = sys_prompt_id,
        gen_box_id = gen_box_id,
        gen_input_id = gen_input_id,
        gen_status_id = gen_status_id,
        hx_target = hx_target,
        hx_include = hx_include,
        hx_vals = hx_vals
    )
}

/// The picture field on the classic Agents page.
///
/// The same picker the `/ui` workspace uses, so a picture is chosen from disk in both places and
/// there is one upload route behind them; `id_prefix` keeps the create form's field apart from
/// every agent row's.
fn legacy_agent_avatar_field(
    id_prefix: &str,
    name: &str,
    avatar_url: Option<&AvatarUrl>,
) -> String {
    super::avatar_picker(&super::AvatarPicker {
        field_id: &format!("legacy-agent-avatar-{id_prefix}"),
        avatar_url,
        name,
        label: "Agent Picture",
        error: None,
    })
}

pub fn agents_page(company: &Company, agents: &[Agent]) -> String {
    let list_html = agent_list_fragment(company, agents);
    let company_name = &company.name;
    let company_id = company.id;
    let prompt_gen_html = render_ai_prompt_generator(
        company_id,
        "agent_system_prompt",
        "agent_prompt_gen_box",
        "agent_prompt_gen_input",
        "agent_prompt_gen_status",
        ", #agent_provider, #agent_model",
    );

    let content = format!(
        r##"
        <div>
            <div class="flex items-center justify-between mb-6 pb-4 border-b border-base-300">
                <div>
                    <h2 class="text-2xl font-bold text-base-content">{company_name} Agents</h2>
                    <p class="text-base-content/70 text-sm mt-0.5">Manage AI Agents, model providers, and configurations</p>
                </div>
                <div class="flex items-center gap-3">
                    <a href="/companies" class="text-xs text-primary hover:text-primary font-medium transition">
                        &larr; Back to Companies
                    </a>
                    <button id="agent-form-toggle" type="button" aria-controls="agent-form-card" aria-expanded="false"
                        data-action="toggle-form-card" data-card="agent-form-card"
                        class="px-4 py-2 bg-success hover:bg-success/90 text-success-content text-sm font-semibold rounded-lg shadow-md shadow-success/20 transition cursor-pointer">
                        Add Agent
                    </button>
                </div>
            </div>

            <!-- Create Agent Card -->
            <div id="agent-form-card" class="hidden bg-base-200 border border-base-300 rounded-xl p-5 mb-8 shadow-lg">
                <h3 class="text-sm font-semibold text-base-content mb-4 flex items-center gap-2">
                    <span class="text-base-content">+</span> Add New Agent
                </h3>
                <form hx-post="/companies/{company_id}/agents" hx-target="#agent-list" hx-swap="innerHTML" class="space-y-4"
                      hx-params="not avatar_file"
                      data-after-request="reset-and-collapse" data-card="agent-form-card" data-toggle="agent-form-toggle"
                      data-keydown="block-enter">
                    <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                        <div>
                            <label for="agent_name" class="block text-xs font-medium text-base-content/80 mb-1">Agent Name</label>
                            <input type="text" id="agent_name" name="name" required
                                data-input="slugify" data-slug-target="agent_slug"
                                placeholder="e.g. Triage Bot" class="w-full bg-base-100 border border-base-300 rounded-lg px-3 py-2 text-sm text-base-content placeholder-base-content/60 focus:outline-none focus:border-primary transition">
                        </div>
                        <div>
                            <label for="agent_slug" class="block text-xs font-medium text-base-content/80 mb-1">Slug</label>
                            <input type="text" id="agent_slug" name="slug" required
                                placeholder="triage-bot" class="w-full bg-base-100 border border-base-300 rounded-lg px-3 py-2 text-sm text-base-content placeholder-base-content/60 focus:outline-none focus:border-primary transition">
                        </div>
                        {avatar_field}
                    </div>

                    <div class="grid grid-cols-1 md:grid-cols-4 gap-4">
                        <div>
                            <label for="agent_provider" class="block text-xs font-medium text-base-content/80 mb-1">Provider</label>
                            <input type="text" id="agent_provider" name="provider"
                                placeholder="openai, anthropic, google" class="w-full bg-base-100 border border-base-300 rounded-lg px-3 py-2 text-sm text-base-content placeholder-base-content/60 focus:outline-none focus:border-primary transition">
                        </div>
                        <div>
                            <label for="agent_model" class="block text-xs font-medium text-base-content/80 mb-1">Model</label>
                            <input type="text" id="agent_model" name="model"
                                placeholder="gpt-4o, claude-3-5-sonnet" class="w-full bg-base-100 border border-base-300 rounded-lg px-3 py-2 text-sm text-base-content placeholder-base-content/60 focus:outline-none focus:border-primary transition">
                        </div>
                        <div>
                            <label for="agent_run_timeout_secs" class="block text-xs font-medium text-base-content/80 mb-1">Run Timeout</label>
                            <input type="number" id="agent_run_timeout_secs" name="run_timeout_secs" min="1" max="3600"
                                placeholder="Default" class="w-full bg-base-100 border border-base-300 rounded-lg px-3 py-2 text-sm text-base-content placeholder-base-content/60 focus:outline-none focus:border-primary transition">
                        </div>
                    </div>

                    <div>
                        {prompt_gen_html}
                        <textarea id="agent_system_prompt" name="system_prompt" rows="2"
                            placeholder="You are a helpful customer support agent..."
                            class="w-full bg-base-100 border border-base-300 rounded-lg px-3 py-2 text-sm text-base-content placeholder-base-content/60 focus:outline-none focus:border-primary transition"></textarea>
                    </div>

                    <div>
                        <label for="agent_config_json" class="block text-xs font-medium text-base-content/80 mb-1">Config JSON (Optional)</label>
                        <textarea id="agent_config_json" name="config_json" rows="2"
                            placeholder='{{ "system_prompt": "You are a support agent." }}'
                            class="w-full bg-base-100 border border-base-300 rounded-lg px-3 py-2 text-sm text-base-content font-mono placeholder-base-content/60 focus:outline-none focus:border-primary transition"></textarea>
                    </div>

                    <div class="flex justify-end pt-2">
                        <button type="submit" class="px-4 py-2 bg-success hover:bg-success/90 text-success-content font-medium rounded-lg text-sm transition cursor-pointer shadow-md shadow-success/20">
                            Create Agent
                        </button>
                    </div>
                </form>
            </div>

            <div id="agent-list">
                {list_html}
            </div>
        </div>
        "##,
        company_name = escape_html_text(company_name),
        company_id = company_id,
        avatar_field = legacy_agent_avatar_field("new", "", None),
        prompt_gen_html = prompt_gen_html,
        list_html = list_html
    );

    base_layout(&format!("{company_name} Agents"), &content)
}

pub fn agent_list_fragment(company: &Company, agents: &[Agent]) -> String {
    if agents.is_empty() {
        return format!(
            r#"<div class="bg-base-200 border border-base-300 rounded-xl p-8 text-center text-base-content/70">
                <p class="text-sm">No agents configured for <span class="font-semibold text-base-content">{}</span> yet.</p>
                <p class="text-xs text-base-content/70 mt-1">Use Add Agent to create your first one.</p>
            </div>"#,
            escape_html_text(&company.name)
        );
    }

    let rows: String = agents
        .iter()
        .map(|agent| agent_row_fragment(company, agent))
        .collect::<Vec<_>>()
        .join("");

    format!(r#"<div class="space-y-3">{}</div>"#, rows)
}

pub fn agent_row_fragment(company: &Company, agent: &Agent) -> String {
    let company_id = company.id;
    let agent_id = agent.id;
    let name = &agent.name;
    let slug = &agent.slug;
    let provider = agent.provider.as_deref().unwrap_or("-");
    let model = agent.model.as_deref().unwrap_or("-");
    let run_timeout = agent
        .run_timeout_secs
        .map(|seconds| format!("{seconds}s"))
        .unwrap_or_else(|| "Default".to_string());
    let system_prompt_display = agent.system_prompt.as_deref().unwrap_or("-");

    let config_display = agent
        .config_json
        .as_ref()
        .map(|c| serde_json::to_string(c).unwrap_or_default())
        .unwrap_or_else(|| "-".to_string());

    format!(
        r##"
        <div id="agent-row-{agent_id}" class="bg-base-200 border border-base-300 rounded-xl p-4 flex flex-col md:flex-row md:items-center justify-between gap-4 transition hover:border-base-content/30">
            <div class="space-y-1">
                <div class="flex items-center gap-2">
                    {avatar}
                    <span class="font-bold text-base-content text-base">{name}</span>
                    <span class="text-xs font-mono text-primary bg-primary/10 border border-primary/40 px-2 py-0.5 rounded">@{slug}</span>
                </div>
                <div class="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-base-content/70 font-mono">
                    <div><span class="text-base-content/70">Provider:</span> <span class="text-base-content">{provider}</span></div>
                    <div><span class="text-base-content/70">Model:</span> <span class="text-base-content">{model}</span></div>
                    <div><span class="text-base-content/70">Run timeout:</span> <span class="text-base-content">{run_timeout}</span></div>
                    <div class="max-w-xs truncate"><span class="text-base-content/70">System Prompt:</span> <span class="text-base-content/80">{system_prompt_display}</span></div>
                    <div class="max-w-xs truncate"><span class="text-base-content/70">Config:</span> <span class="text-base-content/80">{config_display}</span></div>
                </div>
            </div>
            <div class="flex items-center gap-2">
                <button hx-get="/companies/{company_id}/agents/{agent_id}/edit" hx-target="#agent-row-{agent_id}" hx-swap="outerHTML"
                        class="px-3 py-1.5 bg-base-300 hover:bg-base-content/15 text-base-content text-xs font-medium rounded-lg transition cursor-pointer">
                    Edit
                </button>
                <button hx-delete="/companies/{company_id}/agents/{agent_id}" hx-target="#agent-row-{agent_id}" hx-swap="outerHTML" hx-confirm="Are you sure you want to delete agent '{name}'?"
                        class="px-3 py-1.5 bg-error/10 hover:bg-error/20 text-base-content border border-error/40 text-xs font-medium rounded-lg transition cursor-pointer">
                    Delete
                </button>
            </div>
        </div>
        "##,
        agent_id = agent_id,
        company_id = company_id,
        avatar = avatar_bubble(agent.avatar_url.as_ref(), name, AvatarSize::Row),
        name = escape_html_attr(name),
        slug = escape_html_text(slug),
        provider = escape_html_text(provider),
        model = escape_html_text(model),
        run_timeout = run_timeout,
        system_prompt_display = escape_html_text(system_prompt_display),
        config_display = escape_html_text(&config_display)
    )
}

pub fn agent_edit_fragment(company: &Company, agent: &Agent) -> String {
    let company_id = company.id;
    let agent_id = agent.id;
    let name = &agent.name;
    let slug = &agent.slug;
    let provider = agent.provider.as_deref().unwrap_or("");
    let model = agent.model.as_deref().unwrap_or("");
    let run_timeout = agent
        .run_timeout_secs
        .map(|seconds| seconds.to_string())
        .unwrap_or_default();
    let system_prompt = agent.system_prompt.as_deref().unwrap_or("");
    let config_json_str = agent
        .config_json
        .as_ref()
        .map(|c| serde_json::to_string_pretty(c).unwrap_or_default())
        .unwrap_or_default();
    let prompt_gen_html = render_ai_prompt_generator(
        company_id,
        &format!("agent_system_prompt_{agent_id}"),
        &format!("agent_prompt_gen_box_{agent_id}"),
        &format!("agent_prompt_gen_input_{agent_id}"),
        &format!("agent_prompt_gen_status_{agent_id}"),
        &format!(
            ", #agent-row-{agent_id} input[name=provider], #agent-row-{agent_id} input[name=model]"
        ),
    );

    format!(
        r##"
        <div id="agent-row-{agent_id}" class="bg-base-200 border border-primary/40 rounded-xl p-5 shadow-xl">
            <form hx-put="/companies/{company_id}/agents/{agent_id}" hx-target="#agent-row-{agent_id}" hx-swap="outerHTML"
                  hx-params="not avatar_file" class="space-y-4">
                <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                    <div>
                        <label class="block text-xs font-medium text-base-content/80 mb-1">Agent Name</label>
                        <input type="text" name="name" value="{name}" required class="w-full bg-base-100 border border-base-300 rounded-lg px-3 py-2 text-sm text-base-content focus:outline-none focus:border-primary transition">
                    </div>
                    <div>
                        <label class="block text-xs font-medium text-base-content/80 mb-1">Slug</label>
                        <input type="text" name="slug" value="{slug}" required class="w-full bg-base-100 border border-base-300 rounded-lg px-3 py-2 text-sm text-base-content focus:outline-none focus:border-primary transition">
                    </div>
                    {avatar_field}
                </div>

                <div class="grid grid-cols-1 md:grid-cols-4 gap-4">
                    <div>
                        <label class="block text-xs font-medium text-base-content/80 mb-1">Provider</label>
                        <input type="text" name="provider" value="{provider}" placeholder="openai, anthropic" class="w-full bg-base-100 border border-base-300 rounded-lg px-3 py-2 text-sm text-base-content focus:outline-none focus:border-primary transition">
                    </div>
                    <div>
                        <label class="block text-xs font-medium text-base-content/80 mb-1">Model</label>
                        <input type="text" name="model" value="{model}" placeholder="gpt-4o" class="w-full bg-base-100 border border-base-300 rounded-lg px-3 py-2 text-sm text-base-content focus:outline-none focus:border-primary transition">
                    </div>
                    <div>
                        <label class="block text-xs font-medium text-base-content/80 mb-1">Run Timeout</label>
                        <input type="number" name="run_timeout_secs" min="1" max="3600" value="{run_timeout}" placeholder="Default" class="w-full bg-base-100 border border-base-300 rounded-lg px-3 py-2 text-sm text-base-content focus:outline-none focus:border-primary transition">
                    </div>
                </div>

                <div>
                    {prompt_gen_html}
                    <textarea id="agent_system_prompt_{agent_id}" name="system_prompt" rows="2" class="w-full bg-base-100 border border-base-300 rounded-lg px-3 py-2 text-sm text-base-content focus:outline-none focus:border-primary transition">{system_prompt}</textarea>
                </div>

                <div>
                    <label class="block text-xs font-medium text-base-content/80 mb-1">Config JSON</label>
                    <textarea name="config_json" rows="3" class="w-full bg-base-100 border border-base-300 rounded-lg px-3 py-2 text-sm text-base-content font-mono focus:outline-none focus:border-primary transition">{config_json_str}</textarea>
                </div>

                <div class="flex justify-end gap-2 pt-2">
                    <button type="button" hx-get="/companies/{company_id}/agents/{agent_id}/cancel" hx-target="#agent-row-{agent_id}" hx-swap="outerHTML"
                            class="px-3 py-1.5 bg-base-300 hover:bg-base-content/15 text-base-content/80 text-xs font-medium rounded-lg transition cursor-pointer">
                        Cancel
                    </button>
                    <button type="submit" class="px-4 py-1.5 bg-primary hover:bg-primary/90 text-primary-content text-xs font-medium rounded-lg transition cursor-pointer shadow-md shadow-primary/20">
                        Save Changes
                    </button>
                </div>
            </form>
        </div>
        "##,
        agent_id = agent_id,
        company_id = company_id,
        name = escape_html_attr(name),
        slug = escape_html_attr(slug),
        avatar_field =
            legacy_agent_avatar_field(&agent_id.to_string(), name, agent.avatar_url.as_ref()),
        provider = escape_html_attr(provider),
        model = escape_html_attr(model),
        run_timeout = run_timeout,
        system_prompt = escape_html_text(system_prompt),
        config_json_str = escape_html_text(&config_json_str),
        prompt_gen_html = prompt_gen_html
    )
}
