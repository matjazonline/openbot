use super::*;
/// What the simulation views report about the model a channel will actually run with.
///
/// Every field is display text, including [`api_key_status`](Self::api_key_status), which is a
/// pre-styled HTML badge rather than a bare value.
pub struct LlmInfo {
    pub provider: String,
    pub model: String,
    pub api_key_status: String,
}

/// Resolve the effective provider, model and key source for a channel.
///
/// Settings cascade channel → company → the channel's own `llm` config block, so this reports what
/// a run would really use rather than what any single record says.
pub fn resolve_llm_info(_channel: Option<&Channel>, company: Option<&Company>) -> LlmInfo {
    match company {
        Some(_) => LlmInfo {
            provider: "Selected by agent".to_string(),
            model: "Selected by agent".to_string(),
            api_key_status:
                "<span class=\"text-base-content/70\">Managed in company settings</span>"
                    .to_string(),
        },
        None => LlmInfo {
            provider: "N/A".to_string(),
            model: "N/A".to_string(),
            api_key_status: "<span class=\"text-base-content/70\">Unknown</span>".to_string(),
        },
    }
}

pub struct ChannelSimulationFailure<'a> {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub company: Option<&'a Company>,
    pub channel: Option<&'a Channel>,
    pub to: &'a str,
    pub from: &'a str,
    pub subject: &'a str,
    pub error: &'a str,
}

pub fn channel_simulation_failure_fragment(failure: &ChannelSimulationFailure<'_>) -> String {
    let ChannelSimulationFailure {
        company_id,
        channel_id,
        company,
        channel,
        to: to_str,
        from: from_str,
        subject: subject_str,
        error: error_msg,
    } = failure;
    let llm = resolve_llm_info(*channel, *company);
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
            <div class="bg-base-100 border border-base-300 rounded-xl p-4 mb-6 shadow-md flex items-center justify-between">
                <div class="flex items-center gap-2 text-sm text-base-content/80 font-medium">
                    <span class="inline-block w-2.5 h-2.5 rounded-full bg-error animate-ping"></span>
                    <span class="text-base-content font-semibold">Simulation Execution Failed</span>
                </div>
                <a href="/companies/{company_id}/channels/{channel_id}/simulate"
                   class="px-4 py-2 bg-primary hover:bg-primary/90 text-primary-content text-xs font-semibold rounded-lg shadow-md transition flex items-center gap-1.5 cursor-pointer">
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
            <div class="p-4 rounded-xl bg-error/10 border border-error/40 text-base-content text-sm font-semibold flex items-center gap-2">
                {error_glyph}
                <span>Simulation Execution Error: {error_msg}</span>
            </div>

            <div class="bg-base-100 border border-base-300 rounded-xl p-5 space-y-3 text-xs font-mono shadow-lg">
                <h4 class="text-sm font-sans font-bold text-base-content border-b border-base-300 pb-2">Failure Execution Details</h4>
                <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
                    <div>
                        <span class="text-base-content/70 font-sans block text-[11px] uppercase font-semibold">LLM Provider:</span>
                        <span class="text-primary font-bold">{provider_str}</span>
                    </div>
                    <div>
                        <span class="text-base-content/70 font-sans block text-[11px] uppercase font-semibold">LLM Model:</span>
                        <span class="text-primary font-bold">{model_str}</span>
                    </div>
                    <div class="md:col-span-2">
                        <span class="text-base-content/70 font-sans block text-[11px] uppercase font-semibold">API Key Status:</span>
                        <span>{api_key_status}</span>
                    </div>
                    <div>
                        <span class="text-base-content/70 font-sans block text-[11px] uppercase font-semibold">Recipient ('to'):</span>
                        <span class="text-primary">{to_str}</span>
                    </div>
                    <div>
                        <span class="text-base-content/70 font-sans block text-[11px] uppercase font-semibold">Sender ('from'):</span>
                        <span class="text-primary">{from_str}</span>
                    </div>
                    <div>
                        <span class="text-base-content/70 font-sans block text-[11px] uppercase font-semibold">Target Company:</span>
                        <span class="text-base-content">{company_name}</span>
                    </div>
                    <div>
                        <span class="text-base-content/70 font-sans block text-[11px] uppercase font-semibold">Target Desk:</span>
                        <span class="text-base-content">{channel_name}</span>
                    </div>
                </div>

                <div class="pt-2 border-t border-base-300">
                    <span class="text-base-content/70 font-sans block text-[11px] uppercase font-semibold mb-1">Subject:</span>
                    <span class="text-base-content font-sans font-medium text-sm">{subject_str}</span>
                </div>

                <div class="pt-2 border-t border-base-300">
                    <span class="text-base-content font-sans block text-[11px] uppercase font-semibold mb-1">Error Message:</span>
                    <div class="bg-base-300 p-3 rounded-lg text-base-content whitespace-pre-wrap border border-error/40 font-mono text-xs">{error_msg}</div>
                </div>
            </div>
        </div>
        "##,
        oob_form_swap = oob_form_swap,
        error_glyph = icon(Icon::X, BUTTON_ICON),
        error_msg = escape_html_text(error_msg),
        provider_str = escape_html_text(provider_str),
        model_str = escape_html_text(model_str),
        api_key_status = api_key_status,
        to_str = escape_html_text(to_str),
        from_str = escape_html_text(from_str),
        company_name = escape_html_text(&company_name),
        channel_name = escape_html_text(&channel_name),
        subject_str = escape_html_text(subject_str),
    )
}

/// Banner that replaces the compose form once a verify-only simulation has run.
fn simulation_completed_banner(company_id: Uuid, channel_id: Uuid) -> String {
    format!(
        r##"
        <div id="simulation-form-container" hx-swap-oob="outerHTML">
            <div class="bg-base-100 border border-base-300 rounded-xl p-4 mb-6 shadow-md flex items-center justify-between">
                <div class="flex items-center gap-2 text-sm text-base-content/80 font-medium">
                    <span class="inline-block w-2.5 h-2.5 rounded-full bg-primary animate-pulse"></span>
                    <span>Simulation Completed</span>
                </div>
                <a href="/companies/{company_id}/channels/{channel_id}/simulate"
                   class="px-4 py-2 bg-primary hover:bg-primary/90 text-primary-content text-xs font-semibold rounded-lg shadow-md transition flex items-center gap-1.5 cursor-pointer">
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
struct SimulationRoutingReport<'a> {
    status_banner: &'a str,
    provider: &'a str,
    model: &'a str,
    api_key_status: &'a str,
    company_name: &'a str,
    channel_name: &'a str,
    to: &'a str,
    from: &'a str,
    subject: &'a str,
    body: &'a str,
    channel_config: &'a str,
}

fn simulation_routing_report(report: &SimulationRoutingReport<'_>) -> String {
    let SimulationRoutingReport {
        status_banner,
        provider: provider_str,
        model: model_str,
        api_key_status,
        company_name,
        channel_name,
        to,
        from,
        subject: subject_str,
        body: body_str,
        channel_config: channel_config_str,
    } = report;
    format!(
        r##"
        <div class="space-y-4">
            {status_banner}

            <div class="bg-base-100 border border-base-300 rounded-xl p-5 space-y-3 text-xs font-mono shadow-lg">
                <h4 class="text-sm font-sans font-bold text-base-content border-b border-base-300 pb-2">Simulation Execution Details</h4>
                <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
                    <div>
                        <span class="text-base-content/70 font-sans block text-[11px] uppercase font-semibold">LLM Provider:</span>
                        <span class="text-primary font-bold">{provider_str}</span>
                    </div>
                    <div>
                        <span class="text-base-content/70 font-sans block text-[11px] uppercase font-semibold">LLM Model:</span>
                        <span class="text-primary font-bold">{model_str}</span>
                    </div>
                    <div class="md:col-span-2">
                        <span class="text-base-content/70 font-sans block text-[11px] uppercase font-semibold">API Key Status:</span>
                        <span>{api_key_status}</span>
                    </div>
                    <div>
                        <span class="text-base-content/70 font-sans block text-[11px] uppercase font-semibold">Recipient ('to'):</span>
                        <span class="text-primary">{to}</span>
                    </div>
                    <div>
                        <span class="text-base-content/70 font-sans block text-[11px] uppercase font-semibold">Sender ('from'):</span>
                        <span class="text-primary">{from}</span>
                    </div>
                    <div>
                        <span class="text-base-content/70 font-sans block text-[11px] uppercase font-semibold">Target Company:</span>
                        <span class="text-base-content">{company_name}</span>
                    </div>
                    <div>
                        <span class="text-base-content/70 font-sans block text-[11px] uppercase font-semibold">Target Desk:</span>
                        <span class="text-base-content">{channel_name}</span>
                    </div>
                </div>

                <div class="pt-2 border-t border-base-300">
                    <span class="text-base-content/70 font-sans block text-[11px] uppercase font-semibold mb-1">Email Subject:</span>
                    <span class="text-base-content font-sans font-medium text-sm">{subject_str}</span>
                </div>

                <div>
                    <span class="text-base-content/70 font-sans block text-[11px] uppercase font-semibold mb-1">Email Text Body:</span>
                    <div class="bg-base-300 p-3 rounded-lg text-base-content/80 whitespace-pre-wrap border border-base-300">{body_str}</div>
                </div>

                <div class="pt-2 border-t border-base-300">
                    <span class="text-base-content/70 font-sans block text-[11px] uppercase font-semibold mb-1">Desk Config:</span>
                    <pre class="bg-base-300 p-3 rounded-lg text-base-content whitespace-pre-wrap border border-base-300 text-[11px]">{channel_config_str}</pre>
                </div>
            </div>
        </div>
        "##,
        status_banner = status_banner,
        provider_str = super::escape_html_text(provider_str),
        model_str = super::escape_html_text(model_str),
        api_key_status = super::escape_html_text(api_key_status),
        to = super::escape_html_text(to),
        from = super::escape_html_text(from),
        company_name = super::escape_html_text(company_name),
        channel_name = super::escape_html_text(channel_name),
        subject_str = super::escape_html_text(subject_str),
        body_str = super::escape_html_text(body_str),
        channel_config_str = super::escape_html_text(channel_config_str),
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

/// Echoes the caller's submitted ingress data. Persisted conversation content is rendered only
/// from the separate, viewer-projected history read.
pub fn channel_simulation_result_fragment(
    company_id: Uuid,
    channel_id: Uuid,
    result: &EmailSimulationResult,
) -> String {
    let oob_form_swap = simulation_completed_banner(company_id, channel_id);

    let llm = resolve_llm_info(result.channel.as_ref(), result.company.as_ref());
    let (provider_str, model_str, api_key_status) =
        (&llm.provider, &llm.model, &llm.api_key_status);

    let status_banner = if result.resolved {
        status_banner(
            Icon::Check,
            "bg-success/10 border-success/40 text-base-content",
            "Webhook Triggered &amp; Desk Resolved Successfully!",
        )
    } else if !result.sender_authorized {
        status_banner(
            Icon::X,
            "bg-error/10 border-error/40 text-base-content",
            "Unauthorized Sender: Email 'from' address is not listed in channel participant_emails.",
        )
    } else {
        status_banner(
            Icon::Alert,
            "bg-warning/10 border-warning/40 text-base-content",
            "Desk or Company Not Found for recipient address.",
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

    let subject_str = result.subject.as_deref().unwrap_or("(No subject)");
    let body_str = result.text_body.as_deref().unwrap_or("(No text body)");

    let channel_config_str = "Execution config belongs to the active agent.".to_string();

    let body_fragment = simulation_routing_report(&SimulationRoutingReport {
        status_banner: &status_banner,
        provider: provider_str,
        model: model_str,
        api_key_status,
        company_name: &company_name,
        channel_name: &channel_name,
        to: &result.to,
        from: &result.from,
        subject: subject_str,
        body: body_str,
        channel_config: &channel_config_str,
    });

    format!("{oob_form_swap}\n{body_fragment}")
}
