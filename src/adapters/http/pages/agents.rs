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
            <label for="{sys_prompt_id}" class="block text-xs font-medium text-slate-300">System Prompt</label>
            <button type="button"
                onclick="let el = document.getElementById('{gen_box_id}'); if (el) {{ el.classList.toggle('hidden'); if (!el.classList.contains('hidden')) {{ const inp = document.getElementById('{gen_input_id}'); if (inp) inp.focus(); }} }} return false;"
                class="text-xs text-indigo-400 hover:text-indigo-300 font-medium transition cursor-pointer inline-flex items-center gap-1">
                <span>✨ Generate with AI</span>
            </button>
        </div>

        <div id="{gen_box_id}" class="hidden my-2 p-3 bg-slate-900/90 border border-indigo-500/40 rounded-xl space-y-2.5 shadow-inner">
            <div class="flex items-center justify-between text-xs font-semibold text-indigo-300">
                <span class="flex items-center gap-1.5">
                    <span>✨</span>
                    <span>Generate System Prompt with AI</span>
                </span>
                <button type="button" onclick="document.getElementById('{gen_box_id}').classList.add('hidden')" class="text-slate-400 hover:text-white transition cursor-pointer">&times;</button>
            </div>
            <p class="text-[11px] text-slate-400">Describe what you want this agent to do (e.g. role, responsibilities, rules, tone):</p>
            <textarea id="{gen_input_id}" name="user_instructions" rows="2"
                placeholder="e.g. A helpful support agent that answers questions about billing and refunds politely..."
                class="w-full px-2.5 py-1.5 bg-slate-950 border border-slate-700 rounded-lg text-white text-xs placeholder-slate-500 focus:outline-none focus:ring-1 focus:ring-indigo-500 font-sans"></textarea>

            <div id="{gen_status_id}" class="text-xs"></div>

            <div class="flex items-center justify-end gap-2 pt-0.5">
                <button type="button" onclick="document.getElementById('{gen_box_id}').classList.add('hidden')"
                    class="px-2.5 py-1 bg-slate-700 hover:bg-slate-600 text-slate-200 text-xs font-medium rounded transition cursor-pointer">
                    Cancel
                </button>
                <button type="button"
                    hx-post="/companies/{company_id}/agents/generate-prompt"
                    hx-target="{hx_target}"
                    hx-swap="innerHTML"
                    hx-include="{hx_include}"
                    hx-vals='{hx_vals}'
                    hx-disabled-elt="this"
                    class="px-3 py-1 bg-indigo-600 hover:bg-indigo-500 disabled:opacity-60 disabled:cursor-not-allowed text-white text-xs font-semibold rounded shadow transition cursor-pointer flex items-center gap-1.5 [.htmx-request_&]:pointer-events-none [.htmx-request_&]:opacity-80">
                    <svg class="animate-spin h-3.5 w-3.5 text-white hidden [.htmx-request_&]:inline-block shrink-0" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
                        <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                        <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                    </svg>
                    <span class="[.htmx-request_&]:hidden">Generate</span>
                    <span class="hidden [.htmx-request_&]:inline">Generating...</span>
                </button>
            </div>
        </div>
        "#,
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
        ", #agent_provider, #agent_model, #agent_api_key",
    );

    let content = format!(
        r##"
        <div>
            <div class="flex items-center justify-between mb-6 pb-4 border-b border-slate-700/50">
                <div>
                    <h2 class="text-2xl font-bold text-white">{company_name} Agents</h2>
                    <p class="text-slate-400 text-sm mt-0.5">Manage AI Agents, model providers, and configurations</p>
                </div>
                <div class="flex items-center gap-3">
                    <a href="/companies" class="text-xs text-indigo-400 hover:text-indigo-300 font-medium transition">
                        &larr; Back to Companies
                    </a>
                    <button id="agent-form-toggle" type="button" aria-controls="agent-form-card" aria-expanded="false"
                        onclick="const card = document.getElementById('agent-form-card'); const opening = card.classList.contains('hidden'); card.classList.toggle('hidden'); this.setAttribute('aria-expanded', opening);"
                        class="px-4 py-2 bg-emerald-600 hover:bg-emerald-500 text-white text-sm font-semibold rounded-lg shadow-md shadow-emerald-600/30 transition cursor-pointer">
                        Add Agent
                    </button>
                </div>
            </div>

            <!-- Create Agent Card -->
            <div id="agent-form-card" class="hidden bg-slate-800/40 border border-slate-700/60 rounded-xl p-5 mb-8 shadow-lg">
                <h3 class="text-sm font-semibold text-white mb-4 flex items-center gap-2">
                    <span class="text-emerald-400">+</span> Add New Agent
                </h3>
                <form hx-post="/companies/{company_id}/agents" hx-target="#agent-list" hx-swap="innerHTML" class="space-y-4"
                      hx-on::after-request="if(event.detail.successful && event.detail.elt === this) {{ this.reset(); document.getElementById('agent-form-card').classList.add('hidden'); document.getElementById('agent-form-toggle').setAttribute('aria-expanded', 'false'); }}"
                      onkeydown="if(event.key==='Enter' && event.target.tagName!=='TEXTAREA') event.preventDefault()">
                    <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                        <div>
                            <label for="agent_name" class="block text-xs font-medium text-slate-300 mb-1">Agent Name</label>
                            <input type="text" id="agent_name" name="name" required
                                oninput="document.getElementById('agent_slug').value = this.value.toLowerCase().trim().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '')"
                                placeholder="e.g. Triage Bot" class="w-full bg-slate-900/80 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white placeholder-slate-500 focus:outline-none focus:border-indigo-500 transition">
                        </div>
                        <div>
                            <label for="agent_slug" class="block text-xs font-medium text-slate-300 mb-1">Slug</label>
                            <input type="text" id="agent_slug" name="slug" required
                                placeholder="triage-bot" class="w-full bg-slate-900/80 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white placeholder-slate-500 focus:outline-none focus:border-indigo-500 transition">
                        </div>
                    </div>

                    <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
                        <div>
                            <label for="agent_provider" class="block text-xs font-medium text-slate-300 mb-1">Provider</label>
                            <input type="text" id="agent_provider" name="provider"
                                placeholder="openai, anthropic, google" class="w-full bg-slate-900/80 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white placeholder-slate-500 focus:outline-none focus:border-indigo-500 transition">
                        </div>
                        <div>
                            <label for="agent_model" class="block text-xs font-medium text-slate-300 mb-1">Model</label>
                            <input type="text" id="agent_model" name="model"
                                placeholder="gpt-4o, claude-3-5-sonnet" class="w-full bg-slate-900/80 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white placeholder-slate-500 focus:outline-none focus:border-indigo-500 transition">
                        </div>
                        <div>
                            <label for="agent_api_key" class="block text-xs font-medium text-slate-300 mb-1">API Key</label>
                            <input type="password" id="agent_api_key" name="api_key"
                                placeholder="sk-..." class="w-full bg-slate-900/80 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white placeholder-slate-500 focus:outline-none focus:border-indigo-500 transition">
                        </div>
                    </div>

                    <div>
                        {prompt_gen_html}
                        <textarea id="agent_system_prompt" name="system_prompt" rows="2"
                            placeholder="You are a helpful customer support agent..."
                            class="w-full bg-slate-900/80 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white placeholder-slate-500 focus:outline-none focus:border-indigo-500 transition"></textarea>
                    </div>

                    <div>
                        <label for="agent_config_json" class="block text-xs font-medium text-slate-300 mb-1">Config JSON (Optional)</label>
                        <textarea id="agent_config_json" name="config_json" rows="2"
                            placeholder='{{ "system_prompt": "You are a support agent." }}'
                            class="w-full bg-slate-900/80 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white font-mono placeholder-slate-500 focus:outline-none focus:border-indigo-500 transition"></textarea>
                    </div>

                    <div class="flex justify-end pt-2">
                        <button type="submit" class="px-4 py-2 bg-emerald-600 hover:bg-emerald-500 text-white font-medium rounded-lg text-sm transition cursor-pointer shadow-md shadow-emerald-900/20">
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
        company_name = company_name,
        company_id = company_id,
        prompt_gen_html = prompt_gen_html,
        list_html = list_html
    );

    base_layout(&format!("{company_name} Agents"), &content)
}

pub fn agent_list_fragment(company: &Company, agents: &[Agent]) -> String {
    if agents.is_empty() {
        return format!(
            r#"<div class="bg-slate-800/20 border border-slate-800 rounded-xl p-8 text-center text-slate-400">
                <p class="text-sm">No agents configured for <span class="font-semibold text-white">{}</span> yet.</p>
                <p class="text-xs text-slate-500 mt-1">Use Add Agent to create your first one.</p>
            </div>"#,
            company.name
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
    let system_prompt_display = agent.system_prompt.as_deref().unwrap_or("-");
    let api_key_badge = if agent.api_key.is_some() {
        r#"<span class="px-2 py-0.5 text-[10px] font-medium bg-emerald-500/20 text-emerald-300 border border-emerald-500/30 rounded">Key Configured</span>"#
    } else {
        r#"<span class="px-2 py-0.5 text-[10px] font-medium bg-slate-700/50 text-slate-400 rounded">No Key</span>"#
    };

    let config_display = agent
        .config_json
        .as_ref()
        .map(|c| serde_json::to_string(c).unwrap_or_default())
        .unwrap_or_else(|| "-".to_string());

    format!(
        r##"
        <div id="agent-row-{agent_id}" class="bg-slate-800/60 border border-slate-700/50 rounded-xl p-4 flex flex-col md:flex-row md:items-center justify-between gap-4 transition hover:border-slate-600">
            <div class="space-y-1">
                <div class="flex items-center gap-2">
                    <span class="font-bold text-white text-base">{name}</span>
                    <span class="text-xs font-mono text-indigo-300 bg-indigo-950/60 border border-indigo-800/50 px-2 py-0.5 rounded">@{slug}</span>
                    {api_key_badge}
                </div>
                <div class="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-slate-400 font-mono">
                    <div><span class="text-slate-500">Provider:</span> <span class="text-slate-200">{provider}</span></div>
                    <div><span class="text-slate-500">Model:</span> <span class="text-slate-200">{model}</span></div>
                    <div class="max-w-xs truncate"><span class="text-slate-500">System Prompt:</span> <span class="text-slate-300">{system_prompt_display}</span></div>
                    <div class="max-w-xs truncate"><span class="text-slate-500">Config:</span> <span class="text-slate-300">{config_display}</span></div>
                </div>
            </div>
            <div class="flex items-center gap-2">
                <button hx-get="/companies/{company_id}/agents/{agent_id}/edit" hx-target="#agent-row-{agent_id}" hx-swap="outerHTML"
                        class="px-3 py-1.5 bg-slate-700 hover:bg-slate-600 text-slate-200 text-xs font-medium rounded-lg transition cursor-pointer">
                    Edit
                </button>
                <button hx-delete="/companies/{company_id}/agents/{agent_id}" hx-target="#agent-row-{agent_id}" hx-swap="outerHTML" hx-confirm="Are you sure you want to delete agent '{name}'?"
                        class="px-3 py-1.5 bg-rose-600/20 hover:bg-rose-600/40 text-rose-300 border border-rose-500/30 text-xs font-medium rounded-lg transition cursor-pointer">
                    Delete
                </button>
            </div>
        </div>
        "##,
        agent_id = agent_id,
        company_id = company_id,
        name = name,
        slug = slug,
        provider = provider,
        model = model,
        api_key_badge = api_key_badge,
        system_prompt_display = system_prompt_display,
        config_display = config_display
    )
}

pub fn agent_edit_fragment(company: &Company, agent: &Agent) -> String {
    let company_id = company.id;
    let agent_id = agent.id;
    let name = &agent.name;
    let slug = &agent.slug;
    let provider = agent.provider.as_deref().unwrap_or("");
    let model = agent.model.as_deref().unwrap_or("");
    let api_key = agent.api_key.as_deref().unwrap_or("");
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
            ", #agent-row-{agent_id} input[name=provider], #agent-row-{agent_id} input[name=model], #agent-row-{agent_id} input[name=api_key]"
        ),
    );

    format!(
        r##"
        <div id="agent-row-{agent_id}" class="bg-slate-800 border border-indigo-500/50 rounded-xl p-5 shadow-xl">
            <form hx-put="/companies/{company_id}/agents/{agent_id}" hx-target="#agent-row-{agent_id}" hx-swap="outerHTML" class="space-y-4">
                <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                    <div>
                        <label class="block text-xs font-medium text-slate-300 mb-1">Agent Name</label>
                        <input type="text" name="name" value="{name}" required class="w-full bg-slate-900 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white focus:outline-none focus:border-indigo-500 transition">
                    </div>
                    <div>
                        <label class="block text-xs font-medium text-slate-300 mb-1">Slug</label>
                        <input type="text" name="slug" value="{slug}" required class="w-full bg-slate-900 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white focus:outline-none focus:border-indigo-500 transition">
                    </div>
                </div>

                <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
                    <div>
                        <label class="block text-xs font-medium text-slate-300 mb-1">Provider</label>
                        <input type="text" name="provider" value="{provider}" placeholder="openai, anthropic" class="w-full bg-slate-900 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white focus:outline-none focus:border-indigo-500 transition">
                    </div>
                    <div>
                        <label class="block text-xs font-medium text-slate-300 mb-1">Model</label>
                        <input type="text" name="model" value="{model}" placeholder="gpt-4o" class="w-full bg-slate-900 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white focus:outline-none focus:border-indigo-500 transition">
                    </div>
                    <div>
                        <label class="block text-xs font-medium text-slate-300 mb-1">API Key</label>
                        <input type="password" name="api_key" value="{api_key}" placeholder="sk-..." class="w-full bg-slate-900 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white focus:outline-none focus:border-indigo-500 transition">
                    </div>
                </div>

                <div>
                    {prompt_gen_html}
                    <textarea id="agent_system_prompt_{agent_id}" name="system_prompt" rows="2" class="w-full bg-slate-900 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white focus:outline-none focus:border-indigo-500 transition">{system_prompt}</textarea>
                </div>

                <div>
                    <label class="block text-xs font-medium text-slate-300 mb-1">Config JSON</label>
                    <textarea name="config_json" rows="3" class="w-full bg-slate-900 border border-slate-700 rounded-lg px-3 py-2 text-sm text-white font-mono focus:outline-none focus:border-indigo-500 transition">{config_json_str}</textarea>
                </div>

                <div class="flex justify-end gap-2 pt-2">
                    <button type="button" hx-get="/companies/{company_id}/agents/{agent_id}/cancel" hx-target="#agent-row-{agent_id}" hx-swap="outerHTML"
                            class="px-3 py-1.5 bg-slate-700 hover:bg-slate-600 text-slate-300 text-xs font-medium rounded-lg transition cursor-pointer">
                        Cancel
                    </button>
                    <button type="submit" class="px-4 py-1.5 bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-medium rounded-lg transition cursor-pointer shadow-md shadow-indigo-900/20">
                        Save Changes
                    </button>
                </div>
            </form>
        </div>
        "##,
        agent_id = agent_id,
        company_id = company_id,
        name = name,
        slug = slug,
        provider = provider,
        model = model,
        api_key = api_key,
        system_prompt = system_prompt,
        config_json_str = config_json_str,
        prompt_gen_html = prompt_gen_html
    )
}
