//! The channel simulator: compose a message, run it, inspect the execution.

use super::*;

/// Header shown instead of the compose form once a thread is loaded: the simulator is now
/// replying within that thread, so the only action offered is starting a fresh one.
fn simulation_loaded_thread_header(company_id: Uuid, channel_id: Uuid, tid: &str) -> String {
    format!(
        r##"
            <div id="simulation-form-container">
                <div class="bg-slate-900/70 border border-slate-700/80 rounded-xl p-4 mb-6 shadow-md flex items-center justify-between">
                    <div class="flex items-center gap-2 text-sm text-slate-300 font-medium">
                        <span class="inline-block w-2.5 h-2.5 rounded-full bg-emerald-400 animate-pulse"></span>
                        <span>Thread Loaded & Active</span>
                        <span class="text-xs text-slate-400 font-mono">({tid})</span>
                    </div>
                    <a href="/companies/{company_id}/channels/{channel_id}/simulate"
                       class="px-4 py-2 bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-semibold rounded-lg shadow-md transition flex items-center gap-1.5 cursor-pointer">
                        <span class="inline-flex items-center gap-1.5">{sync_glyph} Simulate New Thread</span>
                    </a>
                </div>
            </div>
            "##,
        company_id = company_id,
        channel_id = channel_id,
        tid = tid,
        sync_glyph = icon(Icon::Sync, BUTTON_ICON),
    )
}

/// The compose form: a synthetic inbound webhook payload plus the controls for loading an
/// existing thread by id.
fn simulation_compose_form(
    company_id: Uuid,
    channel_id: Uuid,
    target_recipient: &str,
    sender_email: &str,
) -> String {
    format!(
        r##"
            <div id="simulation-form-container">
                <div class="bg-slate-900/70 border border-slate-700/80 rounded-xl p-5 mb-6 shadow-md space-y-6">
                    <div>
                        <h3 class="text-md font-semibold text-white mb-4 flex items-center gap-2">
                            <span class="text-indigo-400">{payload_glyph}</span> Simulated Webhook Payload
                        </h3>
                        <form hx-post="/companies/{company_id}/channels/{channel_id}/simulate" hx-target="#simulation-result" hx-swap="innerHTML" hx-disabled-elt="find button[type='submit']" class="space-y-4">
                            <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                                <div>
                                    <label for="to" class="block text-xs font-medium text-slate-300 mb-1">To (Recipient Address)</label>
                                    <input type="text" id="to" name="to" value="{target_recipient}" required
                                        class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm font-mono focus:outline-none focus:ring-2 focus:ring-indigo-500">
                                </div>
                                <div>
                                    <label for="from" class="block text-xs font-medium text-slate-300 mb-1">From (Sender Address)</label>
                                    <input type="text" id="from" name="from" value="{sender_email}" data-server-sender="{sender_email}" required disabled
                                        class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm font-mono focus:outline-none focus:ring-2 focus:ring-indigo-500 disabled:opacity-60 disabled:cursor-not-allowed">
                                </div>
                            </div>
                            <div>
                                <label for="subject" class="block text-xs font-medium text-slate-300 mb-1">Subject</label>
                                <input type="text" id="subject" name="subject" value="Simulated Webhook Trigger" required
                                    class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-indigo-500">
                            </div>
                            <div>
                                <label for="text_body" class="block text-xs font-medium text-slate-300 mb-1">Text Body</label>
                                <textarea id="text_body" name="text_body" rows="3"
                                    class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-indigo-500">Who are you?</textarea>
                            </div>
                            <div>
                                <label class="block text-xs font-medium text-slate-300 mb-2">Execution Mode</label>
                                <div class="grid grid-cols-1 md:grid-cols-3 gap-3">
                                    <label class="flex items-start p-3 bg-slate-800 border border-slate-700 rounded-lg cursor-pointer hover:border-indigo-500 transition">
                                        <input type="radio" name="simulation_mode" value="verify" onchange="this.form.elements.namedItem('from').disabled = false" class="mt-0.5 text-indigo-600 focus:ring-indigo-500">
                                        <div class="ml-2.5">
                                            <span class="block text-xs font-bold text-white">Verify</span>
                                            <span class="block text-[11px] text-slate-400 mt-0.5">Verification only (Recipient & Sender ACL check)</span>
                                        </div>
                                    </label>
                                    <label class="flex items-start p-3 bg-slate-800 border border-slate-700 rounded-lg cursor-pointer hover:border-amber-500 transition">
                                        <input type="radio" name="simulation_mode" value="run_test" checked onchange="const sender = this.form.elements.namedItem('from'); sender.value = sender.dataset.serverSender; sender.disabled = true" class="mt-0.5 text-amber-500 focus:ring-amber-500">
                                        <div class="ml-2.5">
                                            <span class="block text-xs font-bold text-amber-300">Run_Test</span>
                                            <span class="block text-[11px] text-slate-400 mt-0.5">Execute full channel & agent, skip email dispatch</span>
                                        </div>
                                    </label>
                                    <label class="flex items-start p-3 bg-slate-800 border border-slate-700 rounded-lg cursor-pointer hover:border-emerald-500 transition">
                                        <input type="radio" name="simulation_mode" value="run" onchange="const sender = this.form.elements.namedItem('from'); sender.value = sender.dataset.serverSender; sender.disabled = true" class="mt-0.5 text-emerald-500 focus:ring-emerald-500">
                                        <div class="ml-2.5">
                                            <span class="block text-xs font-bold text-emerald-400">Run</span>
                                            <span class="block text-[11px] text-slate-400 mt-0.5">Live execution with full AI agent & outbound SMTP send</span>
                                        </div>
                                    </label>
                                </div>
                            </div>
                            <div class="flex justify-end">
                                <button type="submit"
                                    class="px-5 py-2.5 bg-indigo-600 hover:bg-indigo-500 text-white text-sm font-semibold rounded-lg shadow-md shadow-indigo-600/30 transition cursor-pointer flex items-center gap-2 [.htmx-request_&]:pointer-events-none [.htmx-request_&]:opacity-80">
                                    <svg class="animate-spin h-4 w-4 text-white hidden [.htmx-request_&]:inline-block shrink-0" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" aria-hidden="true">
                                        <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                                        <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                                    </svg>
                                    <span class="[.htmx-request_&]:hidden">Trigger Webhook Simulation</span>
                                    <span class="hidden [.htmx-request_&]:inline">Simulating...</span>
                                    <span class="[.htmx-request_&]:hidden">&rarr;</span>
                                </button>
                            </div>
                        </form>
                    </div>

                    <div class="relative flex py-2 items-center">
                        <div class="flex-grow border-t border-slate-800"></div>
                        <span class="flex-shrink mx-4 text-xs font-semibold text-slate-500 uppercase">OR</span>
                        <div class="flex-grow border-t border-slate-800"></div>
                    </div>

                    <div>
                        <h3 class="text-md font-semibold text-white mb-2 flex items-center gap-2">
                            <span class="text-indigo-400">{lookup_glyph}</span> Open Existing Thread by ID
                        </h3>
                        <p class="text-slate-400 text-xs mb-3">Inspect thread history and simulate follow-up reply messages for an existing thread.</p>
                        <form hx-get="/companies/{company_id}/channels/{channel_id}/simulate/thread" hx-target="#simulation-result" hx-swap="innerHTML" class="flex flex-col sm:flex-row gap-3">
                            <input type="text" id="open_thread_id" name="thread_id" placeholder="Enter Thread ID (e.g. 550e8400-e29b-41d4-a716-446655440000)" required
                                class="flex-1 px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm font-mono focus:outline-none focus:ring-2 focus:ring-indigo-500">
                            <button type="submit"
                                class="px-5 py-2 bg-indigo-600 hover:bg-indigo-500 text-white text-sm font-semibold rounded-lg shadow-md transition cursor-pointer flex items-center justify-center gap-1.5 whitespace-nowrap">
                                <span>Open Thread</span>
                                <span>&rarr;</span>
                            </button>
                        </form>
                    </div>
                </div>
            </div>
            "##,
        company_id = company_id,
        channel_id = channel_id,
        target_recipient = target_recipient,
        sender_email = sender_email,
        payload_glyph = icon(Icon::Zap, BUTTON_ICON),
        lookup_glyph = icon(Icon::Search, BUTTON_ICON),
    )
}

pub fn channel_simulation_page(
    company: &Company,
    app_domain_name: &str,
    channel: &Channel,
    sender_email: &str,
    initial_thread_id: Option<&str>,
    initial_result_html: Option<&str>,
) -> String {
    let target_recipient = format!("{}@{}.{}", channel.slug, company.slug, app_domain_name);

    let initial_result_val = initial_result_html.unwrap_or("");

    let form_container_content = match initial_thread_id.filter(|s| !s.trim().is_empty()) {
        Some(tid) => simulation_loaded_thread_header(company.id, channel.id, tid),
        None => simulation_compose_form(company.id, channel.id, &target_recipient, sender_email),
    };

    let content = format!(
        r##"
        <div class="flex items-center justify-between mb-6">
            <div>
                <a href="/companies/{company_id}/channels" class="inline-flex items-center gap-1.5 text-xs text-indigo-400 hover:text-indigo-300 font-medium mb-1">{back_glyph} Back to Channels</a>
                <h2 class="text-2xl font-bold text-white">Simulate Webhook: {channel_name}</h2>
                <p class="text-slate-400 text-sm mt-0.5">Test incoming email webhook resolution for <span class="font-mono text-emerald-300">{target_recipient}</span></p>
            </div>
        </div>

        {form_container_content}

        <div id="simulation-result">{initial_result_val}</div>
        "##,
        back_glyph = icon(Icon::ArrowLeft, BUTTON_ICON),
        company_id = company.id,
        channel_name = channel.name,
        target_recipient = target_recipient,
        form_container_content = form_container_content,
        initial_result_val = initial_result_val,
    );

    base_layout(&format!("Simulate {}", channel.name), &content)
}

/// What the simulation views report about the model a channel will actually run with.
///
/// Every field is display text, including [`api_key_status`](Self::api_key_status), which is a
/// pre-styled HTML badge rather than a bare value.
pub struct LlmInfo {
    pub provider: String,
    pub model: String,
    pub api_key_status: String,
}

const DEFAULT_PROVIDER_LABEL: &str = "google (default)";
const DEFAULT_MODEL_LABEL: &str = "gemini-2.5-flash (default)";

/// Resolve the effective provider, model and key source for a channel.
///
/// Settings cascade channel → company → the channel's own `llm` config block, so this reports what
/// a run would really use rather than what any single record says.
pub fn resolve_llm_info(channel: Option<&Channel>, company: Option<&Company>) -> LlmInfo {
    match (channel, company) {
        (Some(channel), Some(company)) => {
            let channel_llm = channel
                .channel_config
                .as_ref()
                .and_then(|config| config.get("llm"));
            let config_value = |key: &str| {
                channel_llm
                    .and_then(|llm| llm.get(key))
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.trim().is_empty())
            };

            let provider = non_blank(channel.provider.as_deref())
                .or_else(|| non_blank(company.provider.as_deref()))
                .or_else(|| config_value("provider"));
            let model = non_blank(channel.model.as_deref())
                .or_else(|| non_blank(company.model.as_deref()))
                .or_else(|| config_value("model"));

            let provider_name = provider.unwrap_or("google").to_lowercase();
            LlmInfo {
                provider: provider.unwrap_or(DEFAULT_PROVIDER_LABEL).to_string(),
                model: model.unwrap_or(DEFAULT_MODEL_LABEL).to_string(),
                api_key_status: api_key_status(
                    non_blank(channel.api_key.as_deref()).is_some(),
                    non_blank(company.api_key.as_deref()).is_some(),
                    config_value("api_key").is_some(),
                    &provider_name,
                ),
            }
        }
        (Some(channel), None) => LlmInfo {
            provider: channel
                .provider
                .as_deref()
                .unwrap_or(DEFAULT_PROVIDER_LABEL)
                .to_string(),
            model: channel
                .model
                .as_deref()
                .unwrap_or(DEFAULT_MODEL_LABEL)
                .to_string(),
            api_key_status: match non_blank(channel.api_key.as_deref()) {
                Some(_) => configured_badge("Channel"),
                None => missing_badge(None),
            },
        },
        (None, Some(company)) => LlmInfo {
            provider: company
                .provider
                .as_deref()
                .unwrap_or(DEFAULT_PROVIDER_LABEL)
                .to_string(),
            model: company
                .model
                .as_deref()
                .unwrap_or(DEFAULT_MODEL_LABEL)
                .to_string(),
            api_key_status: match non_blank(company.api_key.as_deref()) {
                Some(_) => configured_badge("Company"),
                None => missing_badge(None),
            },
        },
        (None, None) => LlmInfo {
            provider: "N/A".to_string(),
            model: "N/A".to_string(),
            api_key_status: "<span class=\"text-slate-400\">Unknown</span>".to_string(),
        },
    }
}

/// Where the API key for a run would come from, most specific source first, falling back to the
/// provider's environment variables.
fn api_key_status(
    on_channel: bool,
    on_company: bool,
    in_channel_config: bool,
    provider_name: &str,
) -> String {
    if on_channel {
        return configured_badge("Channel");
    }
    if on_company {
        return configured_badge("Company");
    }
    if in_channel_config {
        return configured_badge("Channel Config");
    }

    let env_vars = provider_env_vars(provider_name);
    let names = env_vars.join(" / ");
    if env_vars.iter().any(|name| env_var_set(name)) {
        format!("<span class=\"text-indigo-300 font-bold\">Env Var ({names})</span>")
    } else {
        missing_badge(Some(&names))
    }
}

fn configured_badge(source: &str) -> String {
    format!("<span class=\"text-emerald-400 font-bold\">Configured ({source})</span>")
}

fn missing_badge(env_vars: Option<&str>) -> String {
    let names = match env_vars {
        Some(names) => format!(" ({names})"),
        None => String::new(),
    };

    format!(
        r##"<span class="inline-flex items-center gap-1.5 text-rose-400 font-bold">{glyph} Missing / Unset{names}</span>"##,
        glyph = icon(Icon::Alert, BUTTON_ICON),
    )
}

/// Environment variables that can supply a key for this provider, in the order they're reported.
fn provider_env_vars(provider_name: &str) -> &'static [&'static str] {
    match provider_name {
        "google" | "gemini" => &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
        "openai" => &["OPENAI_API_KEY"],
        "anthropic" => &["ANTHROPIC_API_KEY"],
        "groq" => &["GROQ_API_KEY"],
        "mistral" => &["MISTRAL_API_KEY"],
        _ => &["LLM_API_KEY", "API_KEY"],
    }
}

fn env_var_set(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| !value.trim().is_empty())
}

fn non_blank(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.trim().is_empty())
}

pub fn channel_simulation_failure_fragment(
    company_id: Uuid,
    channel_id: Uuid,
    company: Option<&Company>,
    channel: Option<&Channel>,
    to_str: &str,
    from_str: &str,
    subject_str: &str,
    error_msg: &str,
) -> String {
    let llm = resolve_llm_info(channel, company);
    let (provider_str, model_str, api_key_status) =
        (&llm.provider, &llm.model, &llm.api_key_status);

    let company_name = company
        .map(|c| format!("{} (/{})", c.name, c.slug))
        .unwrap_or_else(|| "N/A".to_string());

    let channel_name = channel
        .map(|w| format!("{} (/{})", w.name, w.slug))
        .unwrap_or_else(|| "N/A".to_string());

    let oob_form_swap = format!(
        r##"
        <div id="simulation-form-container" hx-swap-oob="outerHTML">
            <div class="bg-slate-900/70 border border-slate-700/80 rounded-xl p-4 mb-6 shadow-md flex items-center justify-between">
                <div class="flex items-center gap-2 text-sm text-slate-300 font-medium">
                    <span class="inline-block w-2.5 h-2.5 rounded-full bg-rose-500 animate-ping"></span>
                    <span class="text-rose-400 font-semibold">Simulation Execution Failed</span>
                </div>
                <a href="/companies/{company_id}/channels/{channel_id}/simulate"
                   class="px-4 py-2 bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-semibold rounded-lg shadow-md transition flex items-center gap-1.5 cursor-pointer">
                    <span class="inline-flex items-center gap-1.5">{sync_glyph} Simulate New Thread</span>
                </a>
            </div>
        </div>
        "##,
        company_id = company_id,
        channel_id = channel_id,
        sync_glyph = icon(Icon::Sync, BUTTON_ICON),
    );

    format!(
        r##"
        {oob_form_swap}
        <div class="space-y-4">
            <div class="p-4 rounded-xl bg-rose-950/80 border border-rose-600/60 text-rose-200 text-sm font-semibold flex items-center gap-2">
                {error_glyph}
                <span>Simulation Execution Error: {error_msg}</span>
            </div>

            <div class="bg-slate-900 border border-slate-700/80 rounded-xl p-5 space-y-3 text-xs font-mono shadow-lg">
                <h4 class="text-sm font-sans font-bold text-white border-b border-slate-800 pb-2">Failure Execution Details</h4>
                <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
                    <div>
                        <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold">LLM Provider:</span>
                        <span class="text-indigo-300 font-bold">{provider_str}</span>
                    </div>
                    <div>
                        <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold">LLM Model:</span>
                        <span class="text-indigo-300 font-bold">{model_str}</span>
                    </div>
                    <div class="md:col-span-2">
                        <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold">API Key Status:</span>
                        <span>{api_key_status}</span>
                    </div>
                    <div>
                        <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold">Recipient ('to'):</span>
                        <span class="text-indigo-300">{to_str}</span>
                    </div>
                    <div>
                        <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold">Sender ('from'):</span>
                        <span class="text-indigo-300">{from_str}</span>
                    </div>
                    <div>
                        <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold">Target Company:</span>
                        <span class="text-slate-200">{company_name}</span>
                    </div>
                    <div>
                        <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold">Target Channel:</span>
                        <span class="text-slate-200">{channel_name}</span>
                    </div>
                </div>

                <div class="pt-2 border-t border-slate-800">
                    <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold mb-1">Subject:</span>
                    <span class="text-slate-200 font-sans font-medium text-sm">{subject_str}</span>
                </div>

                <div class="pt-2 border-t border-slate-800">
                    <span class="text-rose-400 font-sans block text-[11px] uppercase font-semibold mb-1">Error Message:</span>
                    <div class="bg-slate-950 p-3 rounded-lg text-rose-300 whitespace-pre-wrap border border-rose-800/80 font-mono text-xs">{error_msg}</div>
                </div>
            </div>
        </div>
        "##,
        oob_form_swap = oob_form_swap,
        error_glyph = icon(Icon::X, BUTTON_ICON),
        error_msg = error_msg,
        provider_str = provider_str,
        model_str = model_str,
        api_key_status = api_key_status,
        to_str = to_str,
        from_str = from_str,
        company_name = company_name,
        channel_name = channel_name,
        subject_str = subject_str,
    )
}

/// Banner that replaces the compose form once a verify-only simulation has run.
fn simulation_completed_banner(company_id: Uuid, channel_id: Uuid) -> String {
    format!(
        r##"
        <div id="simulation-form-container" hx-swap-oob="outerHTML">
            <div class="bg-slate-900/70 border border-slate-700/80 rounded-xl p-4 mb-6 shadow-md flex items-center justify-between">
                <div class="flex items-center gap-2 text-sm text-slate-300 font-medium">
                    <span class="inline-block w-2.5 h-2.5 rounded-full bg-indigo-400 animate-pulse"></span>
                    <span>Simulation Completed</span>
                </div>
                <a href="/companies/{company_id}/channels/{channel_id}/simulate"
                   class="px-4 py-2 bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-semibold rounded-lg shadow-md transition flex items-center gap-1.5 cursor-pointer">
                    <span class="inline-flex items-center gap-1.5">{sync_glyph} Simulate New Thread</span>
                </a>
            </div>
        </div>
        "##,
        company_id = company_id,
        channel_id = channel_id,
        sync_glyph = icon(Icon::Sync, BUTTON_ICON),
    )
}

/// Routing report for a verify run: which company/channel the address resolved to, the model that
/// would have answered, and the payload as received.
fn simulation_routing_report(
    status_banner: &str,
    provider_str: &str,
    model_str: &str,
    api_key_status: &str,
    company_name: &str,
    channel_name: &str,
    to: &str,
    from: &str,
    subject_str: &str,
    body_str: &str,
    channel_config_str: &str,
) -> String {
    format!(
        r##"
        <div class="space-y-4">
            {status_banner}

            <div class="bg-slate-900 border border-slate-700/80 rounded-xl p-5 space-y-3 text-xs font-mono shadow-lg">
                <h4 class="text-sm font-sans font-bold text-white border-b border-slate-800 pb-2">Simulation Execution Details</h4>
                <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
                    <div>
                        <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold">LLM Provider:</span>
                        <span class="text-indigo-300 font-bold">{provider_str}</span>
                    </div>
                    <div>
                        <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold">LLM Model:</span>
                        <span class="text-indigo-300 font-bold">{model_str}</span>
                    </div>
                    <div class="md:col-span-2">
                        <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold">API Key Status:</span>
                        <span>{api_key_status}</span>
                    </div>
                    <div>
                        <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold">Recipient ('to'):</span>
                        <span class="text-indigo-300">{to}</span>
                    </div>
                    <div>
                        <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold">Sender ('from'):</span>
                        <span class="text-indigo-300">{from}</span>
                    </div>
                    <div>
                        <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold">Target Company:</span>
                        <span class="text-slate-200">{company_name}</span>
                    </div>
                    <div>
                        <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold">Target Channel:</span>
                        <span class="text-slate-200">{channel_name}</span>
                    </div>
                </div>

                <div class="pt-2 border-t border-slate-800">
                    <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold mb-1">Email Subject:</span>
                    <span class="text-slate-200 font-sans font-medium text-sm">{subject_str}</span>
                </div>

                <div>
                    <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold mb-1">Email Text Body:</span>
                    <div class="bg-slate-950 p-3 rounded-lg text-slate-300 whitespace-pre-wrap border border-slate-800">{body_str}</div>
                </div>

                <div class="pt-2 border-t border-slate-800">
                    <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold mb-1">Channel Config:</span>
                    <pre class="bg-slate-950 p-3 rounded-lg text-emerald-300 whitespace-pre-wrap border border-slate-800 text-[11px]">{channel_config_str}</pre>
                </div>
            </div>
        </div>
        "##,
        status_banner = status_banner,
        provider_str = provider_str,
        model_str = model_str,
        api_key_status = api_key_status,
        to = to,
        from = from,
        company_name = company_name,
        channel_name = channel_name,
        subject_str = subject_str,
        body_str = body_str,
        channel_config_str = channel_config_str,
    )
}

/// The one-line verdict at the top of a simulation result: how the run went, in a tinted strip.
///
/// The three outcomes differ only in glyph, tint and sentence, so those are the parameters and the
/// shape is not -- a verdict that was laid out differently from its siblings would read as a
/// different kind of thing rather than as a different answer.
fn status_banner(glyph: Icon, tint: &str, sentence: &str) -> String {
    format!(
        r##"<div class="p-4 rounded-xl border {tint} text-sm font-semibold flex items-center gap-2">
            {glyph}
            <span>{sentence}</span>
        </div>"##,
        glyph = icon(glyph, BUTTON_ICON),
    )
}

pub fn channel_simulation_result_fragment(
    company_id: Uuid,
    channel_id: Uuid,
    result: &InboundEmailResult,
) -> String {
    let oob_form_swap = simulation_completed_banner(company_id, channel_id);

    let llm = resolve_llm_info(result.channel.as_ref(), result.company.as_ref());
    let (provider_str, model_str, api_key_status) =
        (&llm.provider, &llm.model, &llm.api_key_status);

    let status_banner = if result.resolved {
        status_banner(
            Icon::Check,
            "bg-emerald-950/80 border-emerald-600/60 text-emerald-200",
            "Webhook Triggered &amp; Channel Resolved Successfully!",
        )
    } else if !result.sender_authorized {
        status_banner(
            Icon::X,
            "bg-rose-950/80 border-rose-600/60 text-rose-200",
            "Unauthorized Sender: Email 'from' address is not listed in channel participant_emails.",
        )
    } else {
        status_banner(
            Icon::Alert,
            "bg-amber-950/80 border-amber-600/60 text-amber-200",
            "Channel or Company Not Found for recipient address.",
        )
    };

    let company_name = result
        .company
        .as_ref()
        .map(|c| format!("{} (/{})", c.name, c.slug))
        .unwrap_or_else(|| {
            result
                .company_slug
                .as_ref()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "N/A".to_string())
        });

    let channel_name = result
        .channel
        .as_ref()
        .map(|w| format!("{} (/{})", w.name, w.slug))
        .unwrap_or_else(|| {
            result
                .channel_slug
                .as_ref()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "N/A".to_string())
        });

    let subject_str = result.email.subject.as_deref().unwrap_or("(No subject)");
    let body_str = result
        .email
        .text_body
        .as_deref()
        .unwrap_or("(No text body)");

    let channel_config_str = match &result.channel {
        Some(wf) => match &wf.channel_config {
            Some(cfg) => serde_json::to_string_pretty(cfg).unwrap_or_else(|_| cfg.to_string()),
            None => "None".to_string(),
        },
        None => "None".to_string(),
    };

    let body_fragment = simulation_routing_report(
        &status_banner,
        provider_str,
        model_str,
        api_key_status,
        &company_name,
        &channel_name,
        &result.email.to,
        &result.email.from,
        subject_str,
        &body_str,
        &channel_config_str,
    );

    format!("{oob_form_swap}\n{body_fragment}")
}

/// Sticky header that replaces the "new simulation" form once a thread is live.
pub(crate) fn simulation_active_banner(
    company_id: Uuid,
    channel_id: Uuid,
    thread_id_str: &str,
    status_label: &str,
) -> String {
    format!(
        r##"
        <div id="simulation-form-container" hx-swap-oob="outerHTML">
            <div class="bg-slate-900/70 border border-slate-700/80 rounded-xl p-4 mb-6 shadow-md flex items-center justify-between">
                <div class="flex items-center gap-2 text-sm text-slate-300 font-medium">
                    <span class="inline-block w-2.5 h-2.5 rounded-full bg-emerald-400 animate-pulse"></span>
                    <span>{status_label}</span>
                    <span class="text-xs text-slate-400 font-mono">({thread_id_str})</span>
                </div>
                <a href="/companies/{company_id}/channels/{channel_id}/simulate"
                   class="px-4 py-2 bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-semibold rounded-lg shadow-md transition flex items-center gap-1.5 cursor-pointer">
                    <span class="inline-flex items-center gap-1.5">{sync_glyph} Simulate New Thread</span>
                </a>
            </div>
        </div>
        "##,
        sync_glyph = icon(Icon::Sync, BUTTON_ICON),
    )
}

/// What the thread history needs in order to describe a message that has no recorded task: the
/// task list to search, and the channel configuration to attribute an agent reply to.
pub(crate) struct MessageTaskContext<'a> {
    tasks: &'a [BackgroundTask],
    task_id: Option<Uuid>,
    thread_id: Option<Uuid>,
    company: Option<&'a Company>,
    channel: Option<&'a Channel>,
    provider: &'a str,
    model: &'a str,
    resolved_config: serde_json::Value,
    /// Prompt to report for agent messages with no task; `None` reuses the message body.
    agent_prompt: Option<&'a str>,
}

/// The real task payload when one matches, otherwise a synthesized stand-in so the message still
/// renders its execution parameters.
pub(crate) fn message_task_payload(
    msg: &Message,
    is_agent: bool,
    created_at_fmt: &str,
    ctx: &MessageTaskContext<'_>,
) -> serde_json::Value {
    if let Some(task) = find_task_for_message(msg, ctx.tasks, ctx.task_id, ctx.thread_id) {
        return task.payload.clone();
    }
    if is_agent {
        serde_json::json!({
            "task_type": "email_agent_dispatch",
            "execution_parameters": {
                "provider": ctx.provider,
                "model": ctx.model,
                "prompt": ctx.agent_prompt.unwrap_or(msg.clean_text_body.as_str()),
                "config": ctx.resolved_config.clone(),
                "executed_at": created_at_fmt
            },
            "execution_result": {
                "response": msg.clean_text_body,
                "outbound_message_id": msg.message_id
            },
            "channel": ctx.channel,
            "company": ctx.company
        })
    } else {
        serde_json::json!({
            "task_type": "email_agent_dispatch",
            "parsed_email": {
                "sender": msg.sender,
                "subject": msg.subject,
                "prompt_text": msg.clean_text_body,
                "message_id": msg.message_id
            },
            "inbound_message": {
                "id": msg.id,
                "message_id": msg.message_id,
                "direction": "inbound",
                "role": "human",
                "clean_text_body": msg.clean_text_body
            },
            "channel": ctx.channel,
            "company": ctx.company
        })
    }
}

/// One message in the thread history: an indigo agent card or a slate inbound card.
pub(crate) fn message_bubble(msg: &Message, ctx: &MessageTaskContext<'_>) -> String {
    let is_agent = msg.role == MessageRole::Agent || msg.direction == MessageDirection::Outbound;
    let created_at = super::format_date_time(msg.created_at);
    let created_at_utc = msg.created_at.to_rfc3339();
    let params_html = render_message_task_parameters_html(&message_task_payload(
        msg,
        is_agent,
        &created_at_utc,
        ctx,
    ));

    if is_agent {
        let body = render_markdown(&msg.clean_text_body);
        format!(
            r##"
                    <div class="bg-indigo-950/40 border border-indigo-500/30 rounded-xl p-4 space-y-2 shadow-sm">
                        <div class="flex items-center justify-between border-b border-indigo-500/20 pb-2 text-xs">
                            <div class="flex items-center gap-2 font-semibold text-indigo-300">
                                {author_glyph}
                                <span>AI Agent Response</span>
                                <span class="px-1.5 py-0.5 rounded bg-indigo-900/60 text-indigo-200 text-[10px] uppercase font-mono">Outbound</span>
                            </div>
                            <span class="text-slate-400 font-mono text-[11px]">{created_at}</span>
                        </div>
                        <div class="text-xs font-mono text-slate-400">
                            <span>Message ID: </span><span class="text-indigo-200">{msg_id}</span>
                        </div>
                        <div class="bg-slate-950 p-3 rounded-lg text-emerald-300 border border-slate-800 text-xs font-sans max-h-60 overflow-y-auto {markdown_styles}">
                            {body}
                        </div>
                        {params_html}
                    </div>
                    "##,
            author_glyph = icon(Icon::Hubot, BUTTON_ICON),
            created_at = created_at,
            msg_id = msg.message_id,
            body = body,
            markdown_styles = MARKDOWN_CONTENT_STYLES,
            params_html = params_html,
        )
    } else {
        format!(
            r##"
                    <div class="bg-slate-900 border border-slate-800 rounded-xl p-4 space-y-2 shadow-sm">
                        <div class="flex items-center justify-between border-b border-slate-800 pb-2 text-xs">
                            <div class="flex items-center gap-2 font-semibold text-slate-200">
                                {author_glyph}
                                <span>Inbound Email</span>
                                <span class="px-1.5 py-0.5 rounded bg-slate-800 text-slate-300 text-[10px] uppercase font-mono">Inbound</span>
                            </div>
                            <span class="text-slate-400 font-mono text-[11px]">{created_at}</span>
                        </div>
                        <div class="grid grid-cols-1 md:grid-cols-2 gap-2 text-xs font-mono text-slate-400">
                            <div>From: <span class="text-indigo-300">{sender}</span></div>
                            <div>Message ID: <span class="text-slate-300">{msg_id}</span></div>
                        </div>
                        <div class="text-xs font-medium text-slate-200 pt-1">
                            Subject: <span class="font-normal text-slate-300">{subject}</span>
                        </div>
                        <div class="bg-slate-950 p-3 rounded-lg text-slate-300 whitespace-pre-wrap border border-slate-800 text-xs">
                            {body}
                        </div>
                        {params_html}
                    </div>
                    "##,
            author_glyph = icon(Icon::Person, BUTTON_ICON),
            created_at = created_at,
            sender = msg.sender,
            msg_id = msg.message_id,
            subject = msg.subject,
            body = msg.clean_text_body,
            params_html = params_html,
        )
    }
}

/// The full "Thread History" card, or `empty_state` when the thread has no messages yet.
pub(crate) fn thread_history_section(
    messages: &[Message],
    thread_id_str: &str,
    wrapper_extra_class: &str,
    empty_state: &str,
    ctx: &MessageTaskContext<'_>,
) -> String {
    if messages.is_empty() {
        return empty_state.to_string();
    }
    let msgs_html: String = messages
        .iter()
        .map(|msg| message_bubble(msg, ctx))
        .collect();
    let msg_count = messages.len();
    let label = if msg_count == 1 {
        "message"
    } else {
        "messages"
    };

    format!(
        r##"
            <div class="bg-slate-900/80 border border-slate-700/80 rounded-xl p-5 space-y-4 shadow-lg{wrapper_extra_class}">
                <div class="flex items-center justify-between border-b border-slate-800 pb-3">
                    <h4 class="text-sm font-sans font-bold text-white flex items-center gap-2">
                        {history_glyph} Thread History ({msg_count} {label})
                    </h4>
                    <span class="text-xs font-mono text-emerald-400">Thread ID: {thread_id_str}</span>
                </div>
                <div class="space-y-3">
                    {msgs_html}
                </div>
            </div>
            "##,
        history_glyph = icon(Icon::CommentDiscussion, BUTTON_ICON),
    )
}

/// Everything that varies between the two "simulate a reply" forms.
pub(crate) struct ReplyFormFields<'a> {
    company_id: Uuid,
    channel_id: Uuid,
    thread_id_str: &'a str,
    last_msg_id: &'a str,
    to_value: &'a str,
    from_value: &'a str,
    subject: &'a str,
    run_test_checked: bool,
    run_checked: bool,
    /// The execution-result form pads its submit row; the loaded-thread form doesn't.
    submit_row_class: &'a str,
}

pub(crate) fn simulate_reply_form(fields: &ReplyFormFields<'_>) -> String {
    let ReplyFormFields {
        company_id,
        channel_id,
        thread_id_str,
        last_msg_id,
        to_value,
        from_value,
        subject,
        run_test_checked,
        run_checked,
        submit_row_class,
    } = *fields;
    let run_test_checked = if run_test_checked { " checked" } else { "" };
    let run_checked = if run_checked { " checked" } else { "" };

    format!(
        r##"
        <div class="bg-slate-900/90 border border-indigo-500/40 rounded-xl p-5 shadow-xl space-y-4">
            <div class="flex items-center justify-between border-b border-slate-800 pb-3">
                <h3 class="text-sm font-bold text-white flex items-center gap-2">
                    <span class="text-indigo-400">{reply_glyph}</span> Simulate Reply Webhook Call
                </h3>
                <span class="text-xs text-slate-400">Simulate next message in Thread <span class="font-mono text-indigo-300">{thread_id_str}</span></span>
            </div>

            <form hx-post="/companies/{company_id}/channels/{channel_id}/simulate"
                  hx-target="#simulation-result"
                  hx-swap="innerHTML"
                  hx-disabled-elt="find button[type='submit']"
                  class="space-y-4">
                <input type="hidden" name="in_reply_to" value="{last_msg_id}">

                <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                    <div>
                        <label for="to_reply" class="block text-xs font-medium text-slate-300 mb-1">To (Recipient Address)</label>
                        <input type="text" id="to_reply" name="to" value="{to_value}" required
                            class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm font-mono focus:outline-none focus:ring-2 focus:ring-indigo-500">
                    </div>
                    <div>
                        <label for="from_reply" class="block text-xs font-medium text-slate-300 mb-1">From (Sender Address)</label>
                        <input type="text" id="from_reply" name="from" value="{from_value}" disabled
                            class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm font-mono opacity-60 cursor-not-allowed">
                    </div>
                </div>

                <div>
                    <label for="subject_reply" class="block text-xs font-medium text-slate-300 mb-1">Subject</label>
                    <input type="text" id="subject_reply" name="subject" value="{subject}" required
                        class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-indigo-500">
                </div>

                <div>
                    <label for="text_body_reply" class="block text-xs font-medium text-slate-300 mb-1">Reply Text Body</label>
                    <textarea id="text_body_reply" name="text_body" rows="3" required placeholder="Type your reply message here..."
                        class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-indigo-500"></textarea>
                </div>

                <div>
                    <label class="block text-xs font-medium text-slate-300 mb-2">Execution Mode</label>
                    <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
                        <label class="flex items-start p-2.5 bg-slate-800 border border-slate-700 rounded-lg cursor-pointer hover:border-amber-500 transition">
                            <input type="radio" name="simulation_mode" value="run_test"{run_test_checked} class="mt-0.5 text-amber-500 focus:ring-amber-500">
                            <div class="ml-2.5">
                                <span class="block text-xs font-bold text-amber-300">Run_Test</span>
                                <span class="block text-[11px] text-slate-400 mt-0.5">Execute full channel & agent, skip email dispatch</span>
                            </div>
                        </label>
                        <label class="flex items-start p-2.5 bg-slate-800 border border-slate-700 rounded-lg cursor-pointer hover:border-emerald-500 transition">
                            <input type="radio" name="simulation_mode" value="run"{run_checked} class="mt-0.5 text-emerald-500 focus:ring-emerald-500">
                            <div class="ml-2.5">
                                <span class="block text-xs font-bold text-emerald-400">Run</span>
                                <span class="block text-[11px] text-slate-400 mt-0.5">Live execution with full AI agent & outbound SMTP send</span>
                            </div>
                        </label>
                    </div>
                </div>

                <div class="flex justify-end{submit_row_class}">
                    <button type="submit"
                        class="px-5 py-2.5 bg-indigo-600 hover:bg-indigo-500 text-white text-sm font-semibold rounded-lg shadow-md shadow-indigo-600/30 transition cursor-pointer flex items-center gap-2 [.htmx-request_&]:pointer-events-none [.htmx-request_&]:opacity-80">
                        <svg class="animate-spin h-4 w-4 text-white hidden [.htmx-request_&]:inline-block shrink-0" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" aria-hidden="true">
                            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                            <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                        </svg>
                        <span class="[.htmx-request_&]:hidden">Trigger Reply Webhook Simulation</span>
                        <span class="hidden [.htmx-request_&]:inline">Simulating...</span>
                        <span class="[.htmx-request_&]:hidden">{submit_glyph}</span>
                    </button>
                </div>
            </form>
        </div>
        "##,
        reply_glyph = icon(Icon::Reply, BUTTON_ICON),
        submit_glyph = icon(Icon::ArrowRight, BUTTON_ICON),
    )
}

/// The channel configuration an agent reply would have used, for messages with no recorded task.
pub(crate) fn resolved_agent_config(
    company: Option<&Company>,
    channel: Option<&Channel>,
) -> serde_json::Value {
    crate::services::agent_runner::ResolvedAgentParams::new(company, channel, None)
        .map(|p| p.config().clone())
        .ok()
        .or_else(|| channel.and_then(|c| c.channel_config.clone()))
        .unwrap_or_else(|| serde_json::json!({}))
}

/// Wrap a simulated thread view in its own live connection.
///
/// A simulation is queued for the worker like any other message, so this view starts out showing
/// only the message that was sent and fills in as the run progresses. The wrapper stays put and
/// only its contents are replaced -- swapping the element itself would tear down the `EventSource`
/// and reopen it on every update.
///
/// `hx-target="this"` is not decoration. htmx attributes are inherited, and this sits inside the
/// simulation form's target; without pinning it, each update swaps itself over the whole page.
pub fn simulation_live_view(
    company_id: Uuid,
    channel_id: Uuid,
    thread_id: Uuid,
    inner: &str,
) -> String {
    format!(
        r#"<div id="simulation-live" hx-ext="sse"
            sse-connect="/companies/{company_id}/channels/{channel_id}/simulate/events?thread_id={thread_id}"
            sse-swap="simulation" hx-target="this" hx-swap="innerHTML">{inner}</div>"#
    )
}

pub fn channel_simulation_loaded_thread_fragment(
    company: &Company,
    channel: &Channel,
    app_domain_name: &str,
    thread: &Thread,
    messages: &[Message],
    tasks: &[BackgroundTask],
    include_oob: bool,
) -> String {
    let thread_id_str = thread.id.to_string();
    let target_recipient = format!("{}@{}.{}", channel.slug, company.slug, app_domain_name);

    let default_sender = thread
        .participant_emails
        .first()
        .cloned()
        .or_else(|| {
            channel
                .participant_emails
                .as_ref()
                .and_then(|e| e.first().cloned())
        })
        .unwrap_or_else(|| {
            crate::entities::value_objects::EmailAddress::from("sender@example.com")
        });

    let overview_card =
        loaded_thread_overview_card(thread, &thread_id_str, &target_recipient, messages.len());

    let llm = resolve_llm_info(Some(channel), Some(company));
    let (provider_str, model_str) = (&llm.provider, &llm.model);
    let messages_section = thread_history_section(
        messages,
        &thread_id_str,
        " mb-6",
        r#"<div class="bg-slate-900/80 border border-slate-700/80 rounded-xl p-5 shadow-lg text-slate-400 text-xs text-center mb-6">No messages recorded in this thread yet.</div>"#,
        &MessageTaskContext {
            tasks,
            task_id: None,
            thread_id: Some(thread.id),
            company: Some(company),
            channel: Some(channel),
            provider: &provider_str,
            model: &model_str,
            resolved_config: resolved_agent_config(Some(company), Some(channel)),
            agent_prompt: None,
        },
    );

    let reply_form = simulate_reply_form(&ReplyFormFields {
        company_id: company.id,
        channel_id: channel.id,
        thread_id_str: &thread_id_str,
        last_msg_id: &messages
            .last()
            .map(|m| m.message_id.to_string())
            .unwrap_or_default(),
        to_value: &target_recipient,
        from_value: &default_sender,
        subject: &if thread.subject.to_lowercase().starts_with("re:") {
            thread.subject.clone()
        } else {
            format!("Re: {}", thread.subject)
        },
        run_test_checked: true,
        run_checked: false,
        submit_row_class: "",
    });

    if include_oob {
        let banner = simulation_active_banner(
            company.id,
            channel.id,
            &thread_id_str,
            "Thread Loaded & Active",
        );
        format!("{banner}\n{overview_card}\n{messages_section}\n{reply_form}")
    } else {
        format!("{overview_card}\n{messages_section}\n{reply_form}")
    }
}

pub(crate) fn loaded_thread_overview_card(
    thread: &Thread,
    thread_id_str: &str,
    target_recipient: &str,
    msg_count: usize,
) -> String {
    let participants_str = if thread.participant_emails.is_empty() {
        "None recorded".to_string()
    } else {
        thread.participant_emails.join(", ")
    };

    format!(
        r##"
        <div class="bg-slate-900 border border-slate-700/80 rounded-xl p-5 space-y-3 text-xs font-mono shadow-lg mb-6">
            <h4 class="text-sm font-sans font-bold text-white border-b border-slate-800 pb-2 flex items-center justify-between">
                <span>Loaded Thread Details</span>
                <span class="text-emerald-400 text-xs font-mono">ID: {thread_id_str}</span>
            </h4>
            <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
                <div>
                    <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold">Subject:</span>
                    <span class="text-slate-200 font-bold">{subject}</span>
                </div>
                <div>
                    <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold">Target Channel Address:</span>
                    <span class="text-indigo-300">{target_recipient}</span>
                </div>
                <div>
                    <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold">Participants:</span>
                    <span class="text-slate-300">{participants_str}</span>
                </div>
                <div>
                    <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold">Total Messages:</span>
                    <span class="text-emerald-300 font-bold">{msg_count}</span>
                </div>
                <div>
                    <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold">Created At:</span>
                    <span class="text-slate-400">{created_at_fmt}</span>
                </div>
                <div>
                    <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold">Last Updated:</span>
                    <span class="text-slate-400">{updated_at_fmt}</span>
                </div>
            </div>
        </div>
        "##,
        subject = thread.subject,
        created_at_fmt = super::format_date_time(thread.created_at),
        updated_at_fmt = super::format_date_time(thread.updated_at),
    )
}

pub fn channel_simulation_thread_error_fragment(
    company_id: Uuid,
    channel_id: Uuid,
    thread_id_input: &str,
    error_msg: &str,
    include_oob: bool,
) -> String {
    let oob_form_swap = format!(
        r##"
        <div id="simulation-form-container" hx-swap-oob="outerHTML">
            <div class="bg-slate-900/70 border border-slate-700/80 rounded-xl p-4 mb-6 shadow-md flex items-center justify-between">
                <div class="flex items-center gap-2 text-sm text-slate-300 font-medium">
                    <span class="inline-block w-2.5 h-2.5 rounded-full bg-rose-500 animate-ping"></span>
                    <span class="text-rose-400 font-semibold">Failed to Load Thread</span>
                </div>
                <a href="/companies/{company_id}/channels/{channel_id}/simulate"
                   class="px-4 py-2 bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-semibold rounded-lg shadow-md transition flex items-center gap-1.5 cursor-pointer">
                    <span class="inline-flex items-center gap-1.5">{sync_glyph} Simulate New Thread</span>
                </a>
            </div>
        </div>
        "##,
        company_id = company_id,
        channel_id = channel_id,
        sync_glyph = icon(Icon::Sync, BUTTON_ICON),
    );

    let error_body = format!(
        r##"
        <div class="space-y-4">
            <div class="p-4 rounded-xl bg-rose-950/80 border border-rose-600/60 text-rose-200 text-sm font-semibold flex items-center gap-2">
                {error_glyph}
                <span>Error Loading Thread ({thread_id_input}): {error_msg}</span>
            </div>
        </div>
        "##,
        error_glyph = icon(Icon::X, BUTTON_ICON),
        thread_id_input = thread_id_input,
        error_msg = error_msg,
    );

    if include_oob {
        format!("{oob_form_swap}\n{error_body}")
    } else {
        error_body
    }
}
