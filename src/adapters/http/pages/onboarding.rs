//! The first-run wizard: create a company, then its first channel.

use super::*;

pub fn onboarding_company_page(error: Option<&str>) -> String {
    let error_html = error.map(error_alert).unwrap_or_default();
    let content = format!(
        r##"
        <div class="mb-8">
            <p class="text-xs font-semibold uppercase tracking-[0.2em] text-indigo-400 mb-2">Setup 1 of 3</p>
            <h2 class="text-3xl font-bold text-white">Create your workspace</h2>
            <p class="text-slate-400 mt-2">Your company groups its email channels, agents, teammates, and model settings.</p>
            <div class="grid grid-cols-3 gap-2 mt-6" aria-label="Onboarding progress">
                <div class="h-1.5 rounded-full bg-indigo-500"></div>
                <div class="h-1.5 rounded-full bg-slate-700"></div>
                <div class="h-1.5 rounded-full bg-slate-700"></div>
            </div>
        </div>

        {error_html}

        <form method="post" action="/onboarding/company" class="space-y-5">
            <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                <div>
                    <label for="onboarding_company_name" class="block text-sm font-medium text-slate-200 mb-1.5">Company name</label>
                    <input id="onboarding_company_name" name="name" type="text" required autofocus
                        oninput="document.getElementById('onboarding_company_slug').value = this.value.toLowerCase().trim().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '')"
                        class="w-full px-4 py-2.5 bg-slate-900 border border-slate-700 rounded-xl text-white focus:outline-none focus:ring-2 focus:ring-indigo-500"
                        placeholder="Acme Corporation">
                </div>
                <div>
                    <label for="onboarding_company_slug" class="block text-sm font-medium text-slate-200 mb-1.5">Email namespace</label>
                    <input id="onboarding_company_slug" name="slug" type="text" required
                        class="w-full px-4 py-2.5 bg-slate-900 border border-slate-700 rounded-xl text-white font-mono focus:outline-none focus:ring-2 focus:ring-indigo-500"
                        placeholder="acme-corporation">
                    <p class="text-xs text-slate-500 mt-1.5">Used in the inbound email address.</p>
                </div>
            </div>

            <div class="border border-slate-700/70 rounded-xl p-4 bg-slate-900/40">
                <h3 class="font-semibold text-white">Model connection</h3>
                <p class="text-xs text-slate-400 mt-1 mb-4">Optional if your server already provides model defaults.</p>
                <div class="grid grid-cols-1 md:grid-cols-3 gap-3">
                    <input name="provider" type="text" class="px-3.5 py-2 bg-slate-900 border border-slate-700 rounded-lg text-sm text-white" placeholder="Provider, e.g. google">
                    <input name="model" type="text" class="px-3.5 py-2 bg-slate-900 border border-slate-700 rounded-lg text-sm text-white" placeholder="Model, e.g. gemini-2.5-flash">
                    <input name="api_key" type="password" class="px-3.5 py-2 bg-slate-900 border border-slate-700 rounded-lg text-sm text-white font-mono" placeholder="API key">
                </div>
            </div>

            <div class="flex justify-end">
                <button type="submit" class="px-6 py-2.5 bg-indigo-600 hover:bg-indigo-500 text-white font-semibold rounded-xl shadow-lg shadow-indigo-600/20 transition cursor-pointer">
                    Continue to channel
                </button>
            </div>
        </form>
        "##
    );
    base_layout("Set up your company", &content)
}

pub fn onboarding_channel_page(company: &Company, error: Option<&str>) -> String {
    let error_html = error.map(error_alert).unwrap_or_default();
    let content = format!(
        r##"
        <div class="mb-8">
            <p class="text-xs font-semibold uppercase tracking-[0.2em] text-emerald-400 mb-2">Setup 2 of 3</p>
            <h2 class="text-3xl font-bold text-white">Create your first email agent</h2>
            <p class="text-slate-400 mt-2">Give <span class="text-slate-200">{company_name}</span> a channel and describe the work it should handle.</p>
            <div class="grid grid-cols-3 gap-2 mt-6" aria-label="Onboarding progress">
                <div class="h-1.5 rounded-full bg-indigo-500"></div>
                <div class="h-1.5 rounded-full bg-emerald-500"></div>
                <div class="h-1.5 rounded-full bg-slate-700"></div>
            </div>
        </div>

        {error_html}

        <form method="post" action="/onboarding/companies/{company_id}/channel" class="space-y-5">
            <div>
                <label for="onboarding_channel_name" class="block text-sm font-medium text-slate-200 mb-1.5">Channel name</label>
                <input id="onboarding_channel_name" name="name" type="text" required autofocus
                    class="w-full px-4 py-2.5 bg-slate-900 border border-slate-700 rounded-xl text-white focus:outline-none focus:ring-2 focus:ring-emerald-500"
                    placeholder="Customer Support">
                <p class="text-xs text-slate-500 mt-1.5">This becomes the first part of the email address, for example <span class="font-mono">customer-support@{company_slug}...</span></p>
            </div>
            <div>
                <label for="onboarding_instructions" class="block text-sm font-medium text-slate-200 mb-1.5">What should this agent do?</label>
                <textarea id="onboarding_instructions" name="instructions" rows="7" required
                    class="w-full px-4 py-3 bg-slate-900 border border-slate-700 rounded-xl text-white text-sm focus:outline-none focus:ring-2 focus:ring-emerald-500"
                    placeholder="Read incoming customer emails, identify the request, draft a concise and friendly answer, ask for missing details, and clearly list any next actions."></textarea>
                <p class="text-xs text-slate-500 mt-1.5">Include its role, desired output, tone, constraints, and when it should ask for clarification.</p>
            </div>
            <div class="flex items-center justify-between gap-4">
                <a href="/companies" class="text-sm text-slate-400 hover:text-white transition">Finish later</a>
                <button type="submit" class="px-6 py-2.5 bg-emerald-600 hover:bg-emerald-500 text-white font-semibold rounded-xl shadow-lg shadow-emerald-600/20 transition cursor-pointer">
                    Create email agent
                </button>
            </div>
        </form>
        "##,
        company_name = company.name,
        company_slug = company.slug,
        company_id = company.id,
    );
    base_layout("Create your first channel", &content)
}

pub fn onboarding_complete_page(
    company: &Company,
    channel: &Channel,
    app_domain_name: &str,
) -> String {
    let email_address = format!("{}@{}.{}", channel.slug, company.slug, app_domain_name);
    let content = format!(
        r##"
        <div class="mb-8">
            <p class="text-xs font-semibold uppercase tracking-[0.2em] text-emerald-400 mb-2">Setup 3 of 3</p>
            <h2 class="text-3xl font-bold text-white">Your email agent is ready</h2>
            <p class="text-slate-400 mt-2">Send work to <span class="text-slate-200">{channel_name}</span> from your regular inbox.</p>
            <div class="grid grid-cols-3 gap-2 mt-6" aria-label="Onboarding progress">
                <div class="h-1.5 rounded-full bg-indigo-500"></div>
                <div class="h-1.5 rounded-full bg-emerald-500"></div>
                <div class="h-1.5 rounded-full bg-emerald-500"></div>
            </div>
        </div>

        <div class="rounded-2xl border border-emerald-500/30 bg-emerald-500/10 p-5 mb-7">
            <p class="text-xs font-semibold uppercase tracking-wider text-emerald-300">Send or forward email to</p>
            <div class="flex flex-col sm:flex-row sm:items-center gap-3 mt-2">
                <code id="onboarding-email-address" class="text-lg md:text-xl text-white break-all">{email_address}</code>
                <button type="button" onclick="navigator.clipboard.writeText(document.getElementById('onboarding-email-address').textContent); this.textContent = 'Copied'"
                    class="sm:ml-auto px-3 py-1.5 rounded-lg bg-emerald-600 hover:bg-emerald-500 text-sm font-semibold text-white cursor-pointer transition">Copy address</button>
            </div>
        </div>

        <div class="grid grid-cols-1 md:grid-cols-3 gap-4 mb-7">
            <section class="rounded-xl border border-slate-700 bg-slate-900/50 p-4">
                <span class="text-xs font-bold text-indigo-300">01 · NEW REQUEST</span>
                <h3 class="font-semibold text-white mt-2">Send a new email</h3>
                <p class="text-sm text-slate-400 mt-2">Put the goal in the subject. In the body, include context, constraints, deadline, and the output you want.</p>
            </section>
            <section class="rounded-xl border border-slate-700 bg-slate-900/50 p-4">
                <span class="text-xs font-bold text-amber-300">02 · EXISTING WORK</span>
                <h3 class="font-semibold text-white mt-2">Forward a message</h3>
                <p class="text-sm text-slate-400 mt-2">Forward any email to the address above and add your instruction at the top, such as “summarize this and draft a reply.”</p>
            </section>
            <section class="rounded-xl border border-slate-700 bg-slate-900/50 p-4">
                <span class="text-xs font-bold text-emerald-300">03 · ITERATE</span>
                <h3 class="font-semibold text-white mt-2">Reply in the thread</h3>
                <p class="text-sm text-slate-400 mt-2">Reply to refine the result, answer questions, or request another pass. Keep the same subject and thread for context.</p>
            </section>
        </div>

        <div class="border-t border-slate-700 pt-6">
            <h3 class="font-semibold text-white">Useful ways to start</h3>
            <div class="grid grid-cols-1 sm:grid-cols-2 gap-x-6 gap-y-2 mt-3 text-sm text-slate-300">
                <p>• Summarize a long email thread and extract owners and due dates.</p>
                <p>• Forward a customer request and ask for a response draft.</p>
                <p>• Send notes and ask for a polished brief or action plan.</p>
                <p>• Include teammates on CC so they can follow the work.</p>
                <p>• Attach documents and explain exactly what to review.</p>
                <p>• Reply with corrections instead of starting over.</p>
            </div>
        </div>

        <div class="flex flex-col sm:flex-row justify-end gap-3 mt-8">
            <a href="/companies/{company_id}/channels" class="px-5 py-2.5 text-center rounded-xl bg-slate-700 hover:bg-slate-600 text-white font-semibold transition">Manage channel</a>
            <a href="mailto:{email_address}" class="px-5 py-2.5 text-center rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white font-semibold transition">Compose first email</a>
        </div>
        "##,
        channel_name = channel.name,
        company_id = company.id,
        email_address = email_address,
    );
    base_layout("Your email agent is ready", &content)
}
