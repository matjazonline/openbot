use super::*;
/// Header shown instead of the compose form once a thread is loaded: the simulator is now
/// replying within that thread, so the only action offered is starting a fresh one.
fn simulation_loaded_thread_header(company_id: Uuid, channel_id: Uuid, tid: &str) -> String {
    format!(
        r##"
            <div id="simulation-form-container">
                <div class="bg-base-100 border border-base-300 rounded-xl p-4 mb-6 shadow-md flex items-center justify-between">
                    <div class="flex items-center gap-2 text-sm text-base-content/80 font-medium">
                        <span class="inline-block w-2.5 h-2.5 rounded-full bg-success animate-pulse"></span>
                        <span>Thread Loaded & Active</span>
                        <span class="text-xs text-base-content/70 font-mono">({tid})</span>
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
                <div class="bg-base-100 border border-base-300 rounded-xl p-5 mb-6 shadow-md space-y-6">
                    <div>
                        <h3 class="text-md font-semibold text-base-content mb-4 flex items-center gap-2">
                            <span class="text-primary">{payload_glyph}</span> Simulated Webhook Payload
                        </h3>
                        <form hx-post="/companies/{company_id}/channels/{channel_id}/simulate" hx-target="#simulation-result" hx-swap="innerHTML" hx-disabled-elt="find button[type='submit']" class="space-y-4">
                            <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                                <div>
                                    <label for="to" class="block text-xs font-medium text-base-content/80 mb-1">To (Recipient Address)</label>
                                    <input type="text" id="to" name="to" value="{target_recipient}" required
                                        class="w-full px-3.5 py-2 bg-base-200 border border-base-300 rounded-lg text-base-content text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary">
                                </div>
                                <div>
                                    <label for="from" class="block text-xs font-medium text-base-content/80 mb-1">From (Sender Address)</label>
                                    <input type="text" id="from" name="from" value="{sender_email}" data-server-sender="{sender_email}" required disabled
                                        class="w-full px-3.5 py-2 bg-base-200 border border-base-300 rounded-lg text-base-content text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary disabled:opacity-60 disabled:cursor-not-allowed">
                                </div>
                            </div>
                            <div>
                                <label for="subject" class="block text-xs font-medium text-base-content/80 mb-1">Subject</label>
                                <input type="text" id="subject" name="subject" value="Simulated Webhook Trigger" required
                                    class="w-full px-3.5 py-2 bg-base-200 border border-base-300 rounded-lg text-base-content text-sm focus:outline-none focus:ring-2 focus:ring-primary">
                            </div>
                            <div>
                                <label for="text_body" class="block text-xs font-medium text-base-content/80 mb-1">Text Body</label>
                                <textarea id="text_body" name="text_body" rows="3"
                                    class="w-full px-3.5 py-2 bg-base-200 border border-base-300 rounded-lg text-base-content text-sm focus:outline-none focus:ring-2 focus:ring-primary">Who are you?</textarea>
                            </div>
                            <div>
                                <label class="block text-xs font-medium text-base-content/80 mb-2">Execution Mode</label>
                                <div class="grid grid-cols-1 md:grid-cols-3 gap-3">
                                    <label class="flex items-start p-3 bg-base-200 border border-base-300 rounded-lg cursor-pointer hover:border-primary transition">
                                        <input type="radio" name="simulation_mode" value="verify" data-action="simulation-mode" class="mt-0.5 text-primary focus:ring-primary">
                                        <div class="ml-2.5">
                                            <span class="block text-xs font-bold text-base-content">Verify</span>
                                            <span class="block text-[11px] text-base-content/70 mt-0.5">Verification only (Recipient & Sender ACL check)</span>
                                        </div>
                                    </label>
                                    <label class="flex items-start p-3 bg-base-200 border border-base-300 rounded-lg cursor-pointer hover:border-warning transition">
                                        <input type="radio" name="simulation_mode" value="run_test" checked data-action="simulation-mode" class="mt-0.5 text-base-content focus:ring-warning">
                                        <div class="ml-2.5">
                                            <span class="block text-xs font-bold text-base-content">Run_Test</span>
                                            <span class="block text-[11px] text-base-content/70 mt-0.5">Execute full channel & agent, skip email dispatch</span>
                                        </div>
                                    </label>
                                    <label class="flex items-start p-3 bg-base-200 border border-base-300 rounded-lg cursor-pointer hover:border-success transition">
                                        <input type="radio" name="simulation_mode" value="run" data-action="simulation-mode" class="mt-0.5 text-base-content focus:ring-success">
                                        <div class="ml-2.5">
                                            <span class="block text-xs font-bold text-base-content">Run</span>
                                            <span class="block text-[11px] text-base-content/70 mt-0.5">Live execution with full AI agent & outbound SMTP send</span>
                                        </div>
                                    </label>
                                </div>
                            </div>
                            <div class="flex justify-end">
                                <button type="submit"
                                    class="px-5 py-2.5 bg-primary hover:bg-primary/90 text-primary-content text-sm font-semibold rounded-lg shadow-md shadow-primary/20 transition cursor-pointer flex items-center gap-2 [.htmx-request_&]:pointer-events-none [.htmx-request_&]:opacity-80">
                                    <svg class="animate-spin h-4 w-4 text-base-content hidden [.htmx-request_&]:inline-block shrink-0" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" aria-hidden="true">
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
                        <div class="flex-grow border-t border-base-300"></div>
                        <span class="flex-shrink mx-4 text-xs font-semibold text-base-content/70 uppercase">OR</span>
                        <div class="flex-grow border-t border-base-300"></div>
                    </div>

                    <div>
                        <h3 class="text-md font-semibold text-base-content mb-2 flex items-center gap-2">
                            <span class="text-primary">{lookup_glyph}</span> Open Existing Thread by ID
                        </h3>
                        <p class="text-base-content/70 text-xs mb-3">Inspect thread history and simulate follow-up reply messages for an existing thread.</p>
                        <form hx-get="/companies/{company_id}/channels/{channel_id}/simulate/thread" hx-target="#simulation-result" hx-swap="innerHTML" hx-sync="#simulation-result:replace" class="flex flex-col sm:flex-row gap-3">
                            <input type="text" id="open_thread_id" name="thread_id" placeholder="Enter Thread ID (e.g. 550e8400-e29b-41d4-a716-446655440000)" required
                                class="flex-1 px-3.5 py-2 bg-base-200 border border-base-300 rounded-lg text-base-content text-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary">
                            <button type="submit"
                                class="px-5 py-2 bg-primary hover:bg-primary/90 text-primary-content text-sm font-semibold rounded-lg shadow-md transition cursor-pointer flex items-center justify-center gap-1.5 whitespace-nowrap">
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
        target_recipient = escape_html_attr(target_recipient),
        sender_email = escape_html_attr(sender_email),
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
    let target_recipient = channel.inbound_address(&company.slug, app_domain_name);

    let initial_result_val = initial_result_html.unwrap_or("");

    let form_container_content = match initial_thread_id.filter(|s| !s.trim().is_empty()) {
        Some(tid) => simulation_loaded_thread_header(company.id, channel.id, tid),
        None => simulation_compose_form(company.id, channel.id, &target_recipient, sender_email),
    };

    let content = format!(
        r##"
        <div class="flex items-center justify-between mb-6">
            <div>
                <a href="/companies/{company_id}/channels" class="inline-flex items-center gap-1.5 text-xs text-primary hover:text-primary font-medium mb-1">{back_glyph} Back to Channels</a>
                <h2 class="text-2xl font-bold text-base-content">Simulate Webhook: {channel_name}</h2>
                <p class="text-base-content/70 text-sm mt-0.5">Test incoming email webhook resolution for <span class="font-mono text-base-content">{target_recipient}</span></p>
            </div>
        </div>

        {form_container_content}

        <div id="simulation-result">{initial_result_val}</div>
        "##,
        back_glyph = icon(Icon::ArrowLeft, BUTTON_ICON),
        company_id = company.id,
        channel_name = escape_html_text(&channel.name),
        target_recipient = escape_html_text(&target_recipient),
        form_container_content = form_container_content,
        initial_result_val = initial_result_val,
    );

    base_layout(&format!("Simulate {}", channel.name), &content)
}
