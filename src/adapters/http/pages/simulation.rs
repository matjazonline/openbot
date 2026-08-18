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
                        <span>🔄 Simulate New Thread</span>
                    </a>
                </div>
            </div>
            "##,
        company_id = company_id,
        channel_id = channel_id,
        tid = tid,
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
                            <span class="text-indigo-400">⚡</span> Simulated Webhook Payload
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
                            <span class="text-indigo-400">🔍</span> Open Existing Thread by ID
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
                <a href="/companies/{company_id}/channels" class="text-xs text-indigo-400 hover:text-indigo-300 font-medium mb-1 inline-block">&larr; Back to Channels</a>
                <h2 class="text-2xl font-bold text-white">Simulate Webhook: {channel_name}</h2>
                <p class="text-slate-400 text-sm mt-0.5">Test incoming email webhook resolution for <span class="font-mono text-emerald-300">{target_recipient}</span></p>
            </div>
        </div>

        {form_container_content}

        <div id="simulation-result">{initial_result_val}</div>
        "##,
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
    match env_vars {
        Some(names) => {
            format!("<span class=\"text-rose-400 font-bold\">Missing / Unset ⚠️ ({names})</span>")
        }
        None => "<span class=\"text-rose-400 font-bold\">Missing / Unset ⚠️</span>".to_string(),
    }
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
                    <span>🔄 Simulate New Thread</span>
                </a>
            </div>
        </div>
        "##,
        company_id = company_id,
        channel_id = channel_id,
    );

    format!(
        r##"
        {oob_form_swap}
        <div class="space-y-4">
            <div class="p-4 rounded-xl bg-rose-950/80 border border-rose-600/60 text-rose-200 text-sm font-semibold flex items-center gap-2">
                <span class="text-rose-400 text-lg">✕</span>
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
                    <span>🔄 Simulate New Thread</span>
                </a>
            </div>
        </div>
        "##,
        company_id = company_id,
        channel_id = channel_id,
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
        r#"<div class="p-4 rounded-xl bg-emerald-950/80 border border-emerald-600/60 text-emerald-200 text-sm font-semibold flex items-center gap-2">
            <span class="text-emerald-400 text-lg">✓</span>
            <span>Webhook Triggered & Channel Resolved Successfully!</span>
        </div>"#
    } else if !result.sender_authorized {
        r#"<div class="p-4 rounded-xl bg-rose-950/80 border border-rose-600/60 text-rose-200 text-sm font-semibold flex items-center gap-2">
            <span class="text-rose-400 text-lg">✕</span>
            <span>Unauthorized Sender: Email 'from' address is not listed in channel participant_emails.</span>
        </div>"#
    } else {
        r#"<div class="p-4 rounded-xl bg-amber-950/80 border border-amber-600/60 text-amber-200 text-sm font-semibold flex items-center gap-2">
            <span class="text-amber-400 text-lg">⚠</span>
            <span>Channel or Company Not Found for recipient address.</span>
        </div>"#
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
        status_banner,
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
                    <span>🔄 Simulate New Thread</span>
                </a>
            </div>
        </div>
        "##
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
    let created_at = msg.created_at.format("%Y-%m-%d %H:%M:%S UTC").to_string();
    let params_html =
        render_message_task_parameters_html(&message_task_payload(msg, is_agent, &created_at, ctx));

    if is_agent {
        let body = render_markdown(&msg.clean_text_body);
        format!(
            r##"
                    <div class="bg-indigo-950/40 border border-indigo-500/30 rounded-xl p-4 space-y-2 shadow-sm">
                        <div class="flex items-center justify-between border-b border-indigo-500/20 pb-2 text-xs">
                            <div class="flex items-center gap-2 font-semibold text-indigo-300">
                                <span>🤖</span>
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
                                <span>👤</span>
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
                        <span>💬</span> Thread History ({msg_count} {label})
                    </h4>
                    <span class="text-xs font-mono text-emerald-400">Thread ID: {thread_id_str}</span>
                </div>
                <div class="space-y-3">
                    {msgs_html}
                </div>
            </div>
            "##
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
                    <span class="text-indigo-400 text-base">↩️</span> Simulate Reply Webhook Call
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
                        <span class="[.htmx-request_&]:hidden">&rarr;</span>
                    </button>
                </div>
            </form>
        </div>
        "##
    )
}

/// The rejection view: no agent ran, so only the routing/LLM context is worth showing.
pub(crate) fn simulation_rejection_view(
    banner: &str,
    reason: &str,
    provider_str: &str,
    model_str: &str,
    api_key_status: &str,
) -> String {
    format!(
        r##"
            {banner}
            <div class="space-y-4">
                <div class="p-4 rounded-xl bg-rose-950/80 border border-rose-600/60 text-rose-200 text-sm font-semibold flex items-center gap-2">
                    <span class="text-rose-400 text-lg">✕</span>
                    <span>Webhook Ingestion Rejected: {reason}</span>
                </div>

                <div class="bg-slate-900 border border-slate-700/80 rounded-xl p-5 space-y-3 text-xs font-mono shadow-lg">
                    <h4 class="text-sm font-sans font-bold text-white border-b border-slate-800 pb-2">Rejection Execution Details</h4>
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
                    </div>
                </div>
            </div>
            "##
    )
}

pub(crate) fn simulation_status_banner(mode: SimulationMode, is_agent_error: bool) -> &'static str {
    if is_agent_error {
        return r#"<div class="p-4 rounded-xl bg-rose-950/80 border border-rose-600/60 text-rose-200 text-sm font-semibold flex items-center gap-2">
            <span class="text-rose-400 text-lg">✕</span>
            <span>Channel Simulation Execution Failed! (Agent Error)</span>
        </div>"#;
    }
    match mode {
        SimulationMode::RunTest => {
            r#"<div class="p-4 rounded-xl bg-amber-950/80 border border-amber-600/60 text-amber-200 text-sm font-semibold flex items-center gap-2">
                    <span class="text-amber-400 text-lg">⚡</span>
                    <span>Channel Executed Successfully in Run_Test Mode! (Outbound email send was skipped / dry-run)</span>
                </div>"#
        }
        SimulationMode::Run => {
            r#"<div class="p-4 rounded-xl bg-emerald-950/80 border border-emerald-600/60 text-emerald-200 text-sm font-semibold flex items-center gap-2">
                    <span class="text-emerald-400 text-lg">✓</span>
                    <span>Channel Executed & Outbound Email Dispatched Successfully!</span>
                </div>"#
        }
        SimulationMode::Verify => {
            r#"<div class="p-4 rounded-xl bg-indigo-950/80 border border-indigo-600/60 text-indigo-200 text-sm font-semibold flex items-center gap-2">
                    <span class="text-indigo-400 text-lg">✓</span>
                    <span>Verification Check Passed!</span>
                </div>"#
        }
    }
}

pub(crate) fn token_meter_html(token_usage: Option<&crate::entities::task::TokenUsage>) -> String {
    let Some(tu) = token_usage else {
        return String::new();
    };
    format!(
        r#"
            <div class="md:col-span-2 pt-2 border-t border-slate-800">
                <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold mb-1">📊 Token Meter:</span>
                <div class="flex items-center gap-2 text-xs font-mono">
                    <span class="bg-indigo-950/80 text-indigo-300 border border-indigo-800/80 px-2 py-0.5 rounded font-bold">Total: {}</span>
                    <span class="text-slate-400">(Prompt: {} • Completion: {})</span>
                </div>
            </div>
            "#,
        tu.total_tokens, tu.prompt_tokens, tu.completion_tokens
    )
}

pub(crate) fn execution_metadata_html(metadata: Option<&serde_json::Value>) -> String {
    let Some(meta) = metadata else {
        return String::new();
    };
    let meta_pretty = serde_json::to_string_pretty(meta).unwrap_or_else(|_| meta.to_string());
    let finish_badge = meta
        .get("finish_reason")
        .or_else(|| meta.get("stop_reason"))
        .and_then(|v| v.as_str())
        .map(|reason| {
            let style = match reason {
                "length" | "max_tokens" => "bg-amber-950 text-amber-300 border-amber-700 font-bold",
                "stop" | "end_turn" => {
                    "bg-emerald-950 text-emerald-300 border-emerald-700 font-semibold"
                }
                _ => "bg-slate-800 text-slate-300 border-slate-700 font-semibold",
            };
            format!(
                r#"<span class="px-2 py-0.5 rounded text-[11px] font-mono border {}">Finish Reason: {}</span>"#,
                style, reason
            )
        })
        .unwrap_or_default();

    format!(
        r#"
            <div class="md:col-span-2 pt-2 border-t border-slate-800">
                <div class="flex items-center justify-between mb-1">
                    <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold">🔍 Execution Metadata:</span>
                    {}
                </div>
                <pre class="bg-slate-950 p-2.5 rounded-lg text-indigo-300 font-mono text-[11px] border border-slate-800/80 overflow-x-auto whitespace-pre-wrap max-h-48">{}</pre>
            </div>
            "#,
        finish_badge, meta_pretty
    )
}

/// Field-by-field account of what the simulated run did.
pub(crate) struct ExecutionDetails<'a> {
    mode_label: &'a str,
    email_status: &'a str,
    provider_str: &'a str,
    model_str: &'a str,
    api_key_status: &'a str,
    token_meter_html: &'a str,
    metadata_html: &'a str,
    to_str: &'a str,
    from_str: &'a str,
    company_name: &'a str,
    channel_name: &'a str,
    thread_id_str: &'a str,
    inbound_msg_id: &'a str,
    outbound_msg_id: &'a str,
    subject_str: &'a str,
    text_body_str: &'a str,
    is_agent_error: bool,
    agent_response_html: &'a str,
}

pub(crate) fn execution_details_card(details: &ExecutionDetails<'_>) -> String {
    let ExecutionDetails {
        mode_label,
        email_status,
        provider_str,
        model_str,
        api_key_status,
        token_meter_html,
        metadata_html,
        to_str,
        from_str,
        company_name,
        channel_name,
        thread_id_str,
        inbound_msg_id,
        outbound_msg_id,
        subject_str,
        text_body_str,
        is_agent_error,
        agent_response_html,
    } = *details;

    let response_label = if is_agent_error {
        "<span class=\"text-rose-400 font-sans block text-[11px] uppercase font-semibold mb-1\">Execution Error Details:</span>"
    } else {
        "<span class=\"text-slate-500 font-sans block text-[11px] uppercase font-semibold mb-1\">Generated AI Agent Response:</span>"
    };
    let response_style = if is_agent_error {
        format!(
            "bg-slate-950 p-3 rounded-lg text-rose-300 border border-rose-800/80 font-mono text-xs max-h-60 overflow-y-auto {MARKDOWN_CONTENT_STYLES}"
        )
    } else {
        format!(
            "bg-slate-950 p-3 rounded-lg text-emerald-300 border border-slate-800 font-sans text-xs max-h-60 overflow-y-auto {MARKDOWN_CONTENT_STYLES}"
        )
    };

    format!(
        r##"
        <div class="bg-slate-900 border border-slate-700/80 rounded-xl p-5 space-y-3 text-xs font-mono shadow-lg">
            <h4 class="text-sm font-sans font-bold text-white border-b border-slate-800 pb-2">Full Execution Details</h4>
            <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
                <div>
                    <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold">Mode:</span>
                    <span class="text-indigo-300 font-bold">{mode_label}</span>
                </div>
                <div>
                    <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold">Email Dispatch Status:</span>
                    <span>{email_status}</span>
                </div>
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
                {token_meter_html}
                {metadata_html}
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
                <div>
                    <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold">Thread ID:</span>
                    <span class="text-emerald-300 font-mono">{thread_id_str}</span>
                </div>
                <div>
                    <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold">Inbound Message ID:</span>
                    <span class="text-slate-300 font-mono">{inbound_msg_id}</span>
                </div>
                <div class="md:col-span-2">
                    <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold">Outbound Agent Message ID:</span>
                    <span class="text-indigo-300 font-mono">{outbound_msg_id}</span>
                </div>
            </div>

            <div class="pt-2 border-t border-slate-800">
                <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold mb-1">Subject:</span>
                <span class="text-slate-200 font-sans font-medium text-sm">{subject_str}</span>
            </div>

            <div>
                <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold mb-1">Inbound Text Body:</span>
                <div class="bg-slate-950 p-3 rounded-lg text-slate-300 whitespace-pre-wrap border border-slate-800">{text_body_str}</div>
            </div>

            <div class="pt-2 border-t border-slate-800">
                {response_label}
                <div class="{response_style}">{agent_response_html}</div>
            </div>
        </div>
        "##
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

/// The execution-details card for a simulated run: mode, dispatch status, model, the addresses
/// involved and the agent's own answer.
fn simulation_execution_details(
    sim_res: &SimulationExecutionResult,
    provider_str: &str,
    model_str: &str,
    api_key_status: &str,
    thread_id_str: &str,
    is_agent_error: bool,
    agent_response_text: &str,
    to_str: &str,
    from_str: &str,
    subject_str: &str,
    text_body_str: &str,
    email_status: &str,
) -> String {
    let ingest = &sim_res.ingest_result;
    let agent_exec = sim_res.agent_execution.as_ref();
    execution_details_card(&ExecutionDetails {
        mode_label: match sim_res.simulation_mode {
            SimulationMode::Verify => "Verify",
            SimulationMode::RunTest => "Run_Test (Dry-Run)",
            SimulationMode::Run => "Run (Live)",
        },
        email_status,
        provider_str: &provider_str,
        model_str: &model_str,
        api_key_status: &api_key_status,
        token_meter_html: &token_meter_html(agent_exec.and_then(|a| a.token_usage.as_ref())),
        metadata_html: &execution_metadata_html(agent_exec.and_then(|a| a.metadata.as_ref())),
        to_str,
        from_str,
        company_name: &ingest
            .company
            .as_ref()
            .map(|c| format!("{} (/{})", c.name, c.slug))
            .unwrap_or_else(|| "N/A".to_string()),
        channel_name: &ingest
            .channel
            .as_ref()
            .map(|w| format!("{} (/{})", w.name, w.slug))
            .unwrap_or_else(|| "N/A".to_string()),
        thread_id_str: &thread_id_str,
        inbound_msg_id: &ingest
            .inbound_message
            .as_ref()
            .map(|m| m.message_id.to_string())
            .unwrap_or_else(|| "N/A".to_string()),
        outbound_msg_id: &agent_exec
            .and_then(|a| a.outbound_message_id.clone())
            .unwrap_or_else(|| "N/A".to_string()),
        subject_str,
        text_body_str,
        is_agent_error,
        agent_response_html: &render_markdown(agent_response_text),
    })
}

/// Thread history for a simulated run, attributing agent messages to the resolved channel config.
fn simulation_thread_history(
    ingest: &crate::use_cases::thread::InboundIngestResult,
    messages: &[Message],
    tasks: &[BackgroundTask],
    thread_id_str: &str,
    provider_str: &str,
    model_str: &str,
) -> String {
    let parsed = ingest.parsed_email.as_ref();
    thread_history_section(
        messages,
        &thread_id_str,
        "",
        "",
        &MessageTaskContext {
            tasks,
            task_id: ingest.task_id,
            thread_id: ingest.thread.as_ref().map(|t| t.id),
            company: ingest.company.as_ref(),
            channel: ingest.channel.as_ref(),
            provider: &provider_str,
            model: &model_str,
            resolved_config: resolved_agent_config(
                ingest.company.as_ref(),
                ingest.channel.as_ref(),
            ),
            agent_prompt: Some(parsed.map(|p| p.prompt_text.as_str()).unwrap_or("")),
        },
    )
}

/// Reply form for a simulated run, pre-addressed to continue the same thread.
fn simulation_reply_form(
    sim_res: &SimulationExecutionResult,
    company_id: Uuid,
    channel_id: Uuid,
    messages: &[Message],
    thread_id_str: &str,
    to_str: &str,
    from_str: &str,
    subject_str: &str,
) -> String {
    let ingest = &sim_res.ingest_result;
    let last_msg_id = messages
        .last()
        .map(|m| m.message_id.to_string())
        .or_else(|| {
            sim_res
                .agent_execution
                .as_ref()
                .and_then(|a| a.outbound_message_id.clone())
        })
        .or_else(|| {
            ingest
                .inbound_message
                .as_ref()
                .map(|m| m.message_id.to_string())
        })
        .unwrap_or_default();

    simulate_reply_form(&ReplyFormFields {
        company_id,
        channel_id,
        thread_id_str: &thread_id_str,
        last_msg_id: &last_msg_id,
        to_value: to_str,
        from_value: from_str,
        subject: &if subject_str.to_lowercase().starts_with("re:") {
            subject_str.to_string()
        } else {
            format!("Re: {}", subject_str)
        },
        run_test_checked: sim_res.simulation_mode == SimulationMode::RunTest,
        run_checked: sim_res.simulation_mode == SimulationMode::Run,
        submit_row_class: " pt-1",
    })
}

pub fn channel_simulation_execution_result_fragment(
    company_id: Uuid,
    channel_id: Uuid,
    sim_res: &SimulationExecutionResult,
    messages: &[Message],
    tasks: &[BackgroundTask],
) -> String {
    let ingest = &sim_res.ingest_result;
    let llm = resolve_llm_info(ingest.channel.as_ref(), ingest.company.as_ref());
    let (provider_str, model_str, api_key_status) =
        (&llm.provider, &llm.model, &llm.api_key_status);

    let thread_id_str = ingest
        .thread
        .as_ref()
        .map(|t| t.id.to_string())
        .unwrap_or_else(|| "N/A".to_string());

    let banner = simulation_active_banner(
        company_id,
        channel_id,
        &thread_id_str,
        "Simulation Thread Active",
    );

    if !ingest.accepted {
        let reason = ingest
            .reason
            .as_deref()
            .unwrap_or("Ingestion failed / unauthorized");
        return simulation_rejection_view(
            &banner,
            reason,
            &provider_str,
            &model_str,
            &api_key_status,
        );
    }

    let agent_exec = sim_res.agent_execution.as_ref();
    let agent_response_text = agent_exec
        .map(|a| a.agent_response.as_str())
        .unwrap_or("(No response generated)");

    // The runner reports failures in-band as response text, so the wording is all we have to go on.
    let agent_lower = agent_response_text.to_lowercase();
    let is_agent_error = agent_lower.contains("failed")
        || agent_lower.contains("error")
        || agent_lower.contains("missing");

    let parsed = ingest.parsed_email.as_ref();
    let to_str = parsed
        .and_then(|p| p.recipients_to.first().map(|s| s.as_str()))
        .unwrap_or("N/A");
    let from_str = parsed.map(|p| p.sender.as_str()).unwrap_or("N/A");
    let subject_str = parsed.map(|p| p.subject.as_str()).unwrap_or("(No subject)");
    let text_body_str = parsed
        .map(|p| p.clean_text_body.as_str())
        .unwrap_or("(No text body)");

    let email_status = if is_agent_error {
        "<span class=\"text-rose-400 font-bold\">Failed (Execution Error)</span>"
    } else if sim_res.simulation_mode == SimulationMode::RunTest {
        "<span class=\"text-amber-400 font-bold\">Skipped (Run_Test Dry-Run)</span>"
    } else if sim_res.simulation_mode == SimulationMode::Run {
        "<span class=\"text-emerald-400 font-bold\">Dispatched via SMTP</span>"
    } else {
        "<span class=\"text-slate-400 font-bold\">None (Verify Only)</span>"
    };

    let exec_details = simulation_execution_details(
        sim_res,
        provider_str,
        model_str,
        api_key_status,
        &thread_id_str,
        is_agent_error,
        agent_response_text,
        to_str,
        from_str,
        subject_str,
        text_body_str,
        email_status,
    );
    let messages_section = simulation_thread_history(
        ingest,
        messages,
        tasks,
        &thread_id_str,
        provider_str,
        model_str,
    );
    let reply_form = simulation_reply_form(
        sim_res,
        company_id,
        channel_id,
        messages,
        &thread_id_str,
        to_str,
        from_str,
        subject_str,
    );

    let status_banner = simulation_status_banner(sim_res.simulation_mode, is_agent_error);
    format!(
        "{banner}\n<div class=\"space-y-6\">\n{status_banner}\n{exec_details}\n{messages_section}\n{reply_form}\n</div>"
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
        created_at_fmt = thread.created_at.format("%Y-%m-%d %H:%M:%S UTC"),
        updated_at_fmt = thread.updated_at.format("%Y-%m-%d %H:%M:%S UTC"),
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
                    <span>🔄 Simulate New Thread</span>
                </a>
            </div>
        </div>
        "##,
        company_id = company_id,
        channel_id = channel_id,
    );

    let error_body = format!(
        r##"
        <div class="space-y-4">
            <div class="p-4 rounded-xl bg-rose-950/80 border border-rose-600/60 text-rose-200 text-sm font-semibold flex items-center gap-2">
                <span class="text-rose-400 text-lg">✕</span>
                <span>Error Loading Thread ({thread_id_input}): {error_msg}</span>
            </div>
        </div>
        "##,
        thread_id_input = thread_id_input,
        error_msg = error_msg,
    );

    if include_oob {
        format!("{oob_form_swap}\n{error_body}")
    } else {
        error_body
    }
}
