//! The first-run wizard, rendered inside the same shell and component system as every `/ui` page.

use super::*;

fn onboarding_shell(
    title: &str,
    user: &MailboxUser<'_>,
    company: Option<&Company>,
    step: usize,
    pane: &str,
) -> String {
    let content = format!(
        r##"<main class="flex min-w-0 flex-1 overflow-y-auto bg-base-200/40 p-4 sm:p-8">
        <div class="mx-auto my-auto w-full max-w-4xl">
            <div class="mb-6"><div class="mb-2 flex items-center justify-between gap-4"><span class="text-xs font-semibold uppercase tracking-widest text-primary">Workspace setup</span><span class="text-xs opacity-60">Step {step} of 3</span></div>
            <progress class="progress progress-primary w-full" value="{step}" max="3" aria-label="Onboarding step {step} of 3"></progress></div>
            <section id="onboarding-pane" class="card border border-base-300 bg-base-100 shadow-xl"><div class="card-body gap-6">{pane}</div></section>
        </div>
    </main>"##
    );
    ui_shell(&UiShell {
        title,
        user,
        company,
        section: UiSection::Companies,
        content: &content,
        script: "",
    })
}

pub fn onboarding_company_page(
    user: &MailboxUser<'_>,
    app_domain_name: &str,
    error: Option<&str>,
) -> String {
    let error_html = error.map(error_alert).unwrap_or_default();
    let app_domain_name = escape_html_text(app_domain_name);
    let model_connection_fields = model_connection_fields(&ModelConnectionFields {
        agent_id_suffix: None,
        provider: "",
        model: "",
        api_key: "",
        api_key_placeholder: "API key",
    });
    let pane = format!(
        r##"<div><h1 class="card-title text-2xl">Create your workspace</h1><p class="mt-2 opacity-70">Your company groups its email channels, agents, teammates, and model settings.</p></div>
        {error_html}
        <form method="post" action="/ui/onboarding/company" class="space-y-6">
            <div class="grid grid-cols-1 gap-4 md:grid-cols-2">
                <fieldset class="fieldset"><legend class="fieldset-legend">Company name</legend><input id="onboarding_company_name" name="name" type="text" required autofocus class="input w-full" oninput="document.getElementById('onboarding_company_slug').value = this.value.toLowerCase().trim().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '')" placeholder="Acme Corporation"></fieldset>
                <fieldset class="fieldset"><legend class="fieldset-legend">Email namespace</legend><label class="input w-full font-mono"><input id="onboarding_company_slug" name="slug" type="text" required class="grow font-mono" placeholder="acme-corporation"><span class="shrink-0 opacity-60">.{app_domain_name}</span></label><p class="label opacity-60">Used in the inbound email address.</p></fieldset>
            </div>
            <div class="rounded-box border border-base-300 bg-base-200/40 p-4"><h2 class="font-semibold">Model connection <span class="badge badge-ghost badge-sm ml-1">Optional</span></h2><p class="mb-4 mt-1 text-sm opacity-60">Leave these blank when the server provides model defaults.</p>
                {model_connection_fields}
            </div>
            <div class="card-actions justify-end"><button type="submit" class="btn btn-primary">Continue to channel</button></div>
        </form>"##
    );
    onboarding_shell("Set up your company", user, None, 1, &pane)
}

pub fn onboarding_channel_page(
    user: &MailboxUser<'_>,
    company: &Company,
    library_agents: &[Agent],
    error: Option<&str>,
) -> String {
    let error_html = error.map(error_alert).unwrap_or_default();
    let library_picker = agent_library_multi_select(library_agents, &[], "library_agent_ids");
    let library_html = if library_picker.is_empty() {
        String::new()
    } else {
        format!(
            r##"{library_picker}
            <div class="divider">and / or create a custom agent</div>"##
        )
    };
    let pane = format!(
        r##"<div><h1 class="card-title text-2xl">Create your first email agent</h1><p class="mt-2 opacity-70">Choose ready-made agents, create a custom one, or do both for <strong>{company_name}</strong>.</p></div>
        {error_html}
        <form method="post" action="/ui/onboarding/companies/{company_id}/channel" class="space-y-5" onsubmit="this.setAttribute('aria-busy', 'true'); const button = this.querySelector('[type=submit]'); button.disabled = true; button.querySelector('[data-progress]').classList.remove('hidden'); button.querySelector('[data-label]').textContent = 'Creating email agents…';">
            {library_html}
            <fieldset class="fieldset"><legend class="fieldset-legend">Channel name <span class="font-normal opacity-60">(optional)</span></legend><input id="onboarding_channel_name" name="name" type="text" class="input w-full" placeholder="Customer Support"><p class="label opacity-60">For example: <span class="font-mono">customer-support@{company_slug}...</span></p></fieldset>
            <fieldset class="fieldset"><legend class="fieldset-legend">What should this custom agent do?</legend><textarea id="onboarding_instructions" name="instructions" rows="7" class="textarea w-full" placeholder="Read incoming customer emails, identify the request, draft a concise and friendly answer, ask for missing details, and clearly list any next actions."></textarea><p class="label opacity-60">Required only when creating a custom agent.</p></fieldset>
            <div class="card-actions items-center justify-between"><a href="/ui" class="btn btn-ghost">Finish later</a><button type="submit" class="btn btn-primary"><span data-progress class="loading loading-spinner loading-sm hidden" aria-hidden="true"></span><span data-label>Create email agents</span></button></div>
        </form>"##,
        company_name = escape_html_text(&company.name),
        company_slug = escape_html_text(company.slug.as_ref()),
        company_id = company.id,
        library_html = library_html,
    );
    onboarding_shell("Create your first channel", user, Some(company), 2, &pane)
}

pub fn onboarding_complete_page(
    user: &MailboxUser<'_>,
    company: &Company,
    channel: &Channel,
    app_domain_name: &str,
) -> String {
    let email_address = format!("{}@{}.{}", channel.slug, company.slug, app_domain_name);
    let pane = format!(
        r##"<div><div class="mb-3 inline-flex size-12 items-center justify-center rounded-full bg-success/15 text-success">{check}</div><h1 class="card-title text-2xl">Your email agent is ready</h1><p class="mt-2 opacity-70">Send work to <strong>{channel_name}</strong> from your regular inbox.</p></div>
        <div class="alert alert-success items-center"><div class="min-w-0 flex-1"><div class="text-xs font-semibold uppercase tracking-wider opacity-70">Send or forward email to</div><code id="onboarding-email-address" class="mt-1 block break-all text-lg">{email_address}</code></div><button type="button" class="btn btn-success btn-sm" onclick="navigator.clipboard.writeText(document.getElementById('onboarding-email-address').textContent); this.textContent = 'Copied'">Copy address</button></div>
        <div class="grid grid-cols-1 gap-4 md:grid-cols-3">
            <section class="rounded-box border border-base-300 p-4"><span class="badge badge-primary badge-outline">1 · New request</span><h2 class="mt-3 font-semibold">Send a new email</h2><p class="mt-2 text-sm opacity-70">Put the goal in the subject and include context, constraints, deadline, and desired output.</p></section>
            <section class="rounded-box border border-base-300 p-4"><span class="badge badge-warning badge-outline">2 · Existing work</span><h2 class="mt-3 font-semibold">Forward a message</h2><p class="mt-2 text-sm opacity-70">Forward any email and add your instruction at the top.</p></section>
            <section class="rounded-box border border-base-300 p-4"><span class="badge badge-success badge-outline">3 · Iterate</span><h2 class="mt-3 font-semibold">Reply in the thread</h2><p class="mt-2 text-sm opacity-70">Reply to refine the result, answer questions, or request another pass.</p></section>
        </div>
        <div class="card-actions justify-end"><a href="/ui/channels?company_id={company_id}&channel_id={channel_id}" class="btn btn-ghost">Manage channel</a><a href="mailto:{email_address}" class="btn btn-primary">Compose first email</a><a href="/ui?company_id={company_id}&channel_id={channel_id}" class="btn btn-primary">Finish</a></div>"##,
        check = icon(Icon::Check, "size-6"),
        channel_name = escape_html_text(&channel.name),
        company_id = company.id,
        channel_id = channel.id,
        email_address = escape_html_text(&email_address)
    );
    onboarding_shell("Your email agent is ready", user, Some(company), 3, &pane)
}
