use uuid::Uuid;

use crate::entities::{
    company::Company,
    company_invite::CompanyInvite,
    company_member::CompanyMember,
    task::{BackgroundTask, TaskStatus},
    workflow::Workflow,
};
use crate::use_cases::thread::{SimulationExecutionResult, SimulationMode};
use crate::use_cases::workflow::InboundEmailResult;

pub fn base_layout(title: &str, content: &str) -> String {
    format!(
        r##"<!DOCTYPE html>
<html lang="en" class="h-full bg-slate-900">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{title} - Mail Agents</title>
    <script src="https://cdn.jsdelivr.net/npm/@tailwindcss/browser@4"></script>
    <script src="https://unpkg.com/htmx.org@2.0.4"></script>
</head>
<body class="h-full font-sans antialiased text-slate-100 flex flex-col items-center p-4 md:p-8">
    <div class="w-full max-w-4xl">
        <nav class="flex items-center justify-between mb-8 pb-4 border-b border-slate-800">
            <a href="/companies" class="text-xl font-extrabold tracking-tight text-white flex items-center gap-2">
                <span class="text-indigo-500">❖</span> Mail Agents
            </a>
            <div class="flex items-center gap-4 text-sm font-medium">
                <a href="/companies" class="text-slate-300 hover:text-white transition">Companies</a>
                <a href="/invites" class="text-slate-300 hover:text-white transition">My Invites</a>
                <a href="/login" class="text-slate-300 hover:text-white transition">Sign In</a>
                <a href="/register" class="px-3 py-1.5 bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg transition">Sign Up</a>
            </div>
        </nav>
        <div class="bg-slate-800/80 backdrop-blur-md border border-slate-700/60 rounded-2xl shadow-2xl p-6 md:p-8">
            {content}
        </div>
    </div>
</body>
</html>"##
    )
}

pub fn login_page() -> String {
    let content = r##"
        <h2 class="text-2xl font-bold text-white mb-6 text-center">Welcome back</h2>

        <div id="response-message" class="mb-4"></div>

        <form hx-post="/api/user/login" hx-target="#response-message" hx-swap="innerHTML" class="space-y-5 max-w-md mx-auto">
            <div>
                <label for="email_or_username" class="block text-sm font-medium text-slate-300 mb-1">Email or Username</label>
                <input type="text" id="email_or_username" name="email_or_username" required
                    class="w-full px-4 py-2.5 bg-slate-900/60 border border-slate-700 rounded-xl text-white placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:border-transparent transition"
                    placeholder="you@example.com or username">
            </div>

            <div>
                <label for="password" class="block text-sm font-medium text-slate-300 mb-1">Password</label>
                <input type="password" id="password" name="password" required
                    class="w-full px-4 py-2.5 bg-slate-900/60 border border-slate-700 rounded-xl text-white placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:border-transparent transition"
                    placeholder="........">
            </div>

            <button type="submit"
                class="w-full py-3 px-4 bg-indigo-600 hover:bg-indigo-500 text-white font-semibold rounded-xl shadow-lg shadow-indigo-600/30 transition duration-150 ease-in-out cursor-pointer flex items-center justify-center">
                <span>Sign In</span>
            </button>
        </form>

        <div class="mt-6 text-center text-sm text-slate-400">
            Don't have an account?
            <a href="/register" class="text-indigo-400 hover:text-indigo-300 font-medium ml-1 transition">Sign up</a>
        </div>
    "##;

    base_layout("Login", content)
}

pub fn register_page() -> String {
    let content = r##"
        <h2 class="text-2xl font-bold text-white mb-6 text-center">Create an account</h2>

        <div id="response-message" class="mb-4"></div>

        <form hx-post="/api/user/register" hx-target="#response-message" hx-swap="innerHTML" class="space-y-4 max-w-md mx-auto">
            <div>
                <label for="username" class="block text-sm font-medium text-slate-300 mb-1">Username</label>
                <input type="text" id="username" name="username" required
                    class="w-full px-4 py-2.5 bg-slate-900/60 border border-slate-700 rounded-xl text-white placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:border-transparent transition"
                    placeholder="johndoe">
            </div>

            <div>
                <label for="email" class="block text-sm font-medium text-slate-300 mb-1">Email address</label>
                <input type="email" id="email" name="email" required
                    class="w-full px-4 py-2.5 bg-slate-900/60 border border-slate-700 rounded-xl text-white placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:border-transparent transition"
                    placeholder="you@example.com">
            </div>

            <div>
                <label for="password" class="block text-sm font-medium text-slate-300 mb-1">Password</label>
                <input type="password" id="password" name="password" required
                    class="w-full px-4 py-2.5 bg-slate-900/60 border border-slate-700 rounded-xl text-white placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:border-transparent transition"
                    placeholder="........">
            </div>

            <div>
                <label for="confirm_password" class="block text-sm font-medium text-slate-300 mb-1">Confirm Password</label>
                <input type="password" id="confirm_password" name="confirm_password" required
                    class="w-full px-4 py-2.5 bg-slate-900/60 border border-slate-700 rounded-xl text-white placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:border-transparent transition"
                    placeholder="........">
            </div>

            <button type="submit"
                class="w-full py-3 px-4 bg-indigo-600 hover:bg-indigo-500 text-white font-semibold rounded-xl shadow-lg shadow-indigo-600/30 transition duration-150 ease-in-out cursor-pointer flex items-center justify-center mt-2">
                <span>Create Account</span>
            </button>
        </form>

        <div class="mt-6 text-center text-sm text-slate-400">
            Already have an account?
            <a href="/login" class="text-indigo-400 hover:text-indigo-300 font-medium ml-1 transition">Sign in</a>
        </div>
    "##;

    base_layout("Register", content)
}

pub fn companies_page(companies: &[Company]) -> String {
    let list_html = company_list_fragment(companies);

    let content = format!(
        r##"
        <div class="flex items-center justify-between mb-6">
            <div>
                <h2 class="text-2xl font-bold text-white">Company Accounts</h2>
                <p class="text-slate-400 text-sm mt-1">Manage your organization profiles and indexed slugs</p>
            </div>
        </div>

        <div id="response-message" class="mb-6"></div>

        <!-- Create Company Card -->
        <div class="bg-slate-900/70 border border-slate-700/80 rounded-xl p-5 mb-8">
            <h3 class="text-md font-semibold text-white mb-3 flex items-center gap-2">
                <span class="text-indigo-400">+</span> Add New Company
            </h3>
            <form hx-post="/companies" hx-target="#company-list" hx-swap="innerHTML" class="space-y-4"
                hx-on::after-request="if(event.detail.successful) this.reset();">
                <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
                    <div>
                        <label for="company_name" class="block text-xs font-medium text-slate-300 mb-1">Company Name</label>
                        <input type="text" id="company_name" name="name" required
                            oninput="document.getElementById('company_slug').value = this.value.toLowerCase().trim().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '')"
                            class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-indigo-500"
                            placeholder="Acme Corporation">
                    </div>
                    <div>
                        <label for="company_slug" class="block text-xs font-medium text-slate-300 mb-1">Slug (Indexed)</label>
                        <input type="text" id="company_slug" name="slug" required
                            class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-indigo-500 font-mono"
                            placeholder="acme-corporation">
                    </div>
                    <div>
                        <label for="company_api_key" class="block text-xs font-medium text-slate-300 mb-1">LLM API Key (Optional)</label>
                        <input type="password" id="company_api_key" name="api_key"
                            class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-indigo-500 font-mono"
                            placeholder="AIzaSy... / sk-...">
                    </div>
                </div>
                <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                    <div>
                        <label for="company_provider" class="block text-xs font-medium text-slate-300 mb-1">LLM Provider (Optional)</label>
                        <input type="text" id="company_provider" name="provider"
                            class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-indigo-500"
                            placeholder="e.g. google, openai, anthropic">
                    </div>
                    <div>
                        <label for="company_model" class="block text-xs font-medium text-slate-300 mb-1">LLM Model (Optional)</label>
                        <input type="text" id="company_model" name="model"
                            class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-indigo-500 font-mono"
                            placeholder="e.g. gemini-2.5-flash, gpt-4o">
                    </div>
                </div>
                <div class="flex justify-end">
                    <button type="submit"
                        class="px-5 py-2 bg-indigo-600 hover:bg-indigo-500 text-white text-sm font-semibold rounded-lg shadow-md shadow-indigo-600/30 transition cursor-pointer">
                        Create Company
                    </button>
                </div>
            </form>
        </div>

        <!-- Company List Section -->
        <div>
            <h3 class="text-sm font-semibold uppercase tracking-wider text-slate-400 mb-3">Your Companies</h3>
            <div id="company-list" class="space-y-3">
                {list_html}
            </div>
        </div>
    "##
    );

    base_layout("Companies", &content)
}

pub fn company_list_fragment(companies: &[Company]) -> String {
    if companies.is_empty() {
        return r##"
            <div class="bg-slate-900/40 border border-dashed border-slate-700/80 rounded-xl p-8 text-center">
                <p class="text-slate-400 text-sm">No companies registered yet. Create your first company above!</p>
            </div>
        "##
        .to_string();
    }

    companies.iter().map(company_row_fragment).collect()
}

pub fn company_row_fragment(company: &Company) -> String {
    let created_at_str = company.created_at.format("%b %d, %Y").to_string();
    format!(
        r##"
        <div id="company-{id}" class="bg-slate-900/80 border border-slate-700/70 rounded-xl p-4 md:p-5 flex items-center justify-between hover:border-slate-600 transition shadow-sm">
            <div>
                <div class="flex items-center gap-3">
                    <h4 class="text-md font-semibold text-white">{name}</h4>
                    <span class="px-2.5 py-0.5 rounded-full text-xs font-mono bg-indigo-950/90 text-indigo-300 border border-indigo-700/50">/{slug}</span>
                </div>
                <p class="text-xs text-slate-400 mt-1">Added {created_at_str}</p>
            </div>
            <div class="flex items-center gap-2">
                <a href="/companies/{id}/tasks"
                    class="px-3 py-1.5 text-xs font-medium bg-amber-900/80 hover:bg-amber-800 text-amber-200 border border-amber-700/50 rounded-lg transition">
                    Tasks
                </a>
                <a href="/companies/{id}/workflows"
                    class="px-3 py-1.5 text-xs font-medium bg-emerald-900/80 hover:bg-emerald-800 text-emerald-200 border border-emerald-700/50 rounded-lg transition">
                    Workflows
                </a>
                <a href="/companies/{id}/invites"
                    class="px-3 py-1.5 text-xs font-medium bg-indigo-900/80 hover:bg-indigo-800 text-indigo-200 border border-indigo-700/50 rounded-lg transition">
                    Invites & Team
                </a>
                <button hx-get="/companies/{id}/edit" hx-target="#company-{id}" hx-swap="outerHTML"
                    class="px-3 py-1.5 text-xs font-medium bg-slate-700 hover:bg-slate-600 text-slate-200 rounded-lg transition cursor-pointer">
                    Edit
                </button>
                <button hx-delete="/companies/{id}" hx-target="#company-{id}" hx-swap="outerHTML" hx-confirm="Are you sure you want to delete '{name}'?"
                    class="px-3 py-1.5 text-xs font-medium bg-rose-950/80 hover:bg-rose-900/90 text-rose-300 border border-rose-800/50 rounded-lg transition cursor-pointer">
                    Delete
                </button>
            </div>
        </div>
        "##,
        id = company.id,
        name = company.name,
        slug = company.slug,
        created_at_str = created_at_str,
    )
}

pub fn company_edit_fragment(company: &Company) -> String {
    let api_key_val = company.api_key.as_deref().unwrap_or("");
    let provider_val = company.provider.as_deref().unwrap_or("");
    let model_val = company.model.as_deref().unwrap_or("");
    format!(
        r##"
        <form id="company-{id}" hx-put="/companies/{id}" hx-target="#company-{id}" hx-swap="outerHTML"
            class="bg-slate-900 border border-indigo-500/60 rounded-xl p-4 md:p-5 space-y-4 shadow-lg">
            <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
                <div>
                    <label class="block text-xs font-medium text-slate-300 mb-1">Company Name</label>
                    <input type="text" name="name" value="{name}" required
                        oninput="this.form.slug.value = this.value.toLowerCase().trim().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '')"
                        class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-indigo-500">
                </div>
                <div>
                    <label class="block text-xs font-medium text-slate-300 mb-1">Slug (Indexed)</label>
                    <input type="text" name="slug" value="{slug}" required
                        class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-indigo-500 font-mono">
                </div>
                <div>
                    <label class="block text-xs font-medium text-slate-300 mb-1">LLM API Key (Optional)</label>
                    <input type="password" name="api_key" value="{api_key}"
                        class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-indigo-500 font-mono"
                        placeholder="AIzaSy... / sk-...">
                </div>
            </div>
            <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                <div>
                    <label class="block text-xs font-medium text-slate-300 mb-1">LLM Provider (Optional)</label>
                    <input type="text" name="provider" value="{provider}"
                        class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-indigo-500"
                        placeholder="e.g. google, openai">
                </div>
                <div>
                    <label class="block text-xs font-medium text-slate-300 mb-1">LLM Model (Optional)</label>
                    <input type="text" name="model" value="{model}"
                        class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-indigo-500 font-mono"
                        placeholder="e.g. gemini-2.5-flash">
                </div>
            </div>
            <div class="flex items-center justify-end gap-2">
                <button type="button" hx-get="/companies/{id}/cancel" hx-target="#company-{id}" hx-swap="outerHTML"
                    class="px-3 py-1.5 text-xs font-medium bg-slate-700 hover:bg-slate-600 text-slate-200 rounded-lg transition cursor-pointer">
                    Cancel
                </button>
                <button type="submit"
                    class="px-4 py-1.5 text-xs font-semibold bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg transition cursor-pointer">
                    Save Changes
                </button>
            </div>
        </form>
        "##,
        id = company.id,
        name = company.name,
        slug = company.slug,
        api_key = api_key_val,
        provider = provider_val,
        model = model_val,
    )
}

pub fn success_alert(message: &str, redirect_url: Option<(&str, &str)>) -> String {
    let redirect_html = match redirect_url {
        Some((url, label)) => format!(
            r##"<div class="mt-3"><a href="{url}" class="inline-block text-xs font-semibold uppercase tracking-wider text-emerald-300 hover:text-white underline transition">{label} &rarr;</a></div>"##
        ),
        None => String::new(),
    };

    format!(
        r##"<div class="p-4 mb-4 rounded-xl bg-emerald-950/60 border border-emerald-600/40 text-emerald-200 text-sm">
            <div class="flex items-center gap-2 font-medium">
                <svg class="w-5 h-5 text-emerald-400 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7"></path></svg>
                <span>{message}</span>
            </div>
            {redirect_html}
        </div>"##
    )
}

pub fn error_alert(message: &str) -> String {
    format!(
        r##"<div class="p-4 mb-4 rounded-xl bg-rose-950/60 border border-rose-600/40 text-rose-200 text-sm flex items-center gap-2 font-medium">
            <svg class="w-5 h-5 text-rose-400 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 19 9 0 0118 0z"></path></svg>
            <span>{message}</span>
        </div>"##
    )
}

pub fn company_invites_page(
    company: &Company,
    invites: &[CompanyInvite],
    members: &[CompanyMember],
) -> String {
    let invites_html = company_invite_list_fragment(company.id, invites);
    let team_html = company_team_list_fragment(company.id, members);

    let content = format!(
        r##"
        <div class="flex items-center justify-between mb-6">
            <div>
                <a href="/companies" class="text-xs text-indigo-400 hover:text-indigo-300 font-medium mb-1 inline-block">&larr; Back to Companies</a>
                <h2 class="text-2xl font-bold text-white">{company_name} Management</h2>
                <p class="text-slate-400 text-sm mt-0.5">Manage email invites and view team members for <span class="font-mono text-indigo-300">/{slug}</span></p>
            </div>
        </div>

        <div id="response-message" class="mb-6"></div>

        <!-- Create Invite Card -->
        <div class="bg-slate-900/70 border border-slate-700/80 rounded-xl p-5 mb-8">
            <h3 class="text-md font-semibold text-white mb-3 flex items-center gap-2">
                <span class="text-indigo-400">+</span> Invite User by Email
            </h3>
            <form hx-post="/companies/{company_id}/invites" hx-target="#invite-list" hx-swap="innerHTML" class="flex gap-3"
                hx-on::after-request="if(event.detail.successful) this.reset();">
                <input type="email" name="email" required placeholder="colleague@example.com"
                    class="flex-1 px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-indigo-500">
                <button type="submit"
                    class="px-5 py-2 bg-indigo-600 hover:bg-indigo-500 text-white text-sm font-semibold rounded-lg shadow-md shadow-indigo-600/30 transition cursor-pointer">
                    Send Invite
                </button>
            </form>
        </div>

        <!-- Invites List Section -->
        <div class="mb-10">
            <h3 class="text-sm font-semibold uppercase tracking-wider text-slate-400 mb-3">Email Invites</h3>
            <div id="invite-list" class="space-y-3">
                {invites_html}
            </div>
        </div>

        <!-- Team Members Section -->
        <div>
            <h3 class="text-sm font-semibold uppercase tracking-wider text-slate-400 mb-3">Company Team</h3>
            <div id="team-list" class="space-y-3">
                {team_html}
            </div>
        </div>
        "##,
        company_name = company.name,
        slug = company.slug,
        company_id = company.id,
        invites_html = invites_html,
        team_html = team_html,
    );

    base_layout(&format!("Manage {}", company.name), &content)
}

pub fn company_invite_list_fragment(company_id: Uuid, invites: &[CompanyInvite]) -> String {
    if invites.is_empty() {
        return r##"
            <div class="bg-slate-900/40 border border-dashed border-slate-700/80 rounded-xl p-6 text-center">
                <p class="text-slate-400 text-sm">No email invites sent yet.</p>
            </div>
        "##
        .to_string();
    }

    invites
        .iter()
        .map(|inv| company_invite_row_fragment(company_id, inv))
        .collect()
}

pub fn company_invite_row_fragment(company_id: Uuid, invite: &CompanyInvite) -> String {
    let created_at_str = invite.created_at.format("%b %d, %Y").to_string();
    let status_badge = match invite.status.as_str() {
        "accepted" => {
            r#"<span class="px-2.5 py-0.5 rounded-full text-xs font-medium bg-emerald-950 text-emerald-300 border border-emerald-700/50">Accepted</span>"#
        }
        "declined" => {
            r#"<span class="px-2.5 py-0.5 rounded-full text-xs font-medium bg-rose-950 text-rose-300 border border-rose-700/50">Declined</span>"#
        }
        _ => {
            r#"<span class="px-2.5 py-0.5 rounded-full text-xs font-medium bg-amber-950 text-amber-300 border border-amber-700/50">Pending</span>"#
        }
    };

    format!(
        r##"
        <div id="invite-{invite_id}" class="bg-slate-900/80 border border-slate-700/70 rounded-xl p-4 md:p-5 flex items-center justify-between hover:border-slate-600 transition shadow-sm">
            <div>
                <div class="flex items-center gap-3">
                    <span class="text-md font-semibold text-white">{email}</span>
                    {status_badge}
                </div>
                <p class="text-xs text-slate-400 mt-1">Invited on {created_at_str}</p>
            </div>
            <div class="flex items-center gap-2">
                <button hx-get="/companies/{company_id}/invites/{invite_id}/edit" hx-target="#invite-{invite_id}" hx-swap="outerHTML"
                    class="px-3 py-1.5 text-xs font-medium bg-slate-700 hover:bg-slate-600 text-slate-200 rounded-lg transition cursor-pointer">
                    Edit
                </button>
                <button hx-delete="/companies/{company_id}/invites/{invite_id}" hx-target="#invite-{invite_id}" hx-swap="outerHTML" hx-confirm="Are you sure you want to cancel invite for '{email}'?"
                    class="px-3 py-1.5 text-xs font-medium bg-rose-950/80 hover:bg-rose-900/90 text-rose-300 border border-rose-800/50 rounded-lg transition cursor-pointer">
                    Delete
                </button>
            </div>
        </div>
        "##,
        company_id = company_id,
        invite_id = invite.id,
        email = invite.email,
        status_badge = status_badge,
        created_at_str = created_at_str,
    )
}

pub fn company_invite_edit_fragment(company_id: Uuid, invite: &CompanyInvite) -> String {
    format!(
        r##"
        <form id="invite-{invite_id}" hx-put="/companies/{company_id}/invites/{invite_id}" hx-target="#invite-{invite_id}" hx-swap="outerHTML"
            class="bg-slate-900 border border-indigo-500/60 rounded-xl p-4 md:p-5 flex items-center gap-3 shadow-lg">
            <input type="email" name="email" value="{email}" required
                class="flex-1 px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-indigo-500">
            <div class="flex items-center gap-2">
                <button type="button" hx-get="/companies/{company_id}/invites/{invite_id}/cancel" hx-target="#invite-{invite_id}" hx-swap="outerHTML"
                    class="px-3 py-1.5 text-xs font-medium bg-slate-700 hover:bg-slate-600 text-slate-200 rounded-lg transition cursor-pointer">
                    Cancel
                </button>
                <button type="submit"
                    class="px-4 py-1.5 text-xs font-semibold bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg transition cursor-pointer">
                    Save
                </button>
            </div>
        </form>
        "##,
        company_id = company_id,
        invite_id = invite.id,
        email = invite.email,
    )
}

pub fn company_team_list_fragment(company_id: Uuid, members: &[CompanyMember]) -> String {
    if members.is_empty() {
        return r##"
            <div class="bg-slate-900/40 border border-dashed border-slate-700/80 rounded-xl p-6 text-center">
                <p class="text-slate-400 text-sm">No team members joined yet.</p>
            </div>
        "##
        .to_string();
    }

    members
        .iter()
        .map(|m| company_team_row_fragment(company_id, m))
        .collect()
}

pub fn company_team_row_fragment(company_id: Uuid, member: &CompanyMember) -> String {
    let created_at_str = member.created_at.format("%b %d, %Y").to_string();
    let username_display = member.username.as_deref().unwrap_or("Unknown User");
    let email_display = member.email.as_deref().unwrap_or("");

    format!(
        r##"
        <div id="member-{user_id}" class="bg-slate-900/80 border border-slate-700/70 rounded-xl p-4 md:p-5 flex items-center justify-between hover:border-slate-600 transition shadow-sm">
            <div>
                <div class="flex items-center gap-3">
                    <h4 class="text-md font-semibold text-white">{username}</h4>
                    <span class="text-xs text-slate-400">({email})</span>
                    <span class="px-2.5 py-0.5 rounded-full text-xs font-medium bg-indigo-950 text-indigo-300 border border-indigo-700/50 uppercase">{role}</span>
                </div>
                <p class="text-xs text-slate-400 mt-1">Joined {created_at_str}</p>
            </div>
            <div class="flex items-center gap-2">
                <button hx-delete="/companies/{company_id}/team/{user_id}" hx-target="#member-{user_id}" hx-swap="outerHTML" hx-confirm="Remove '{username}' from company team?"
                    class="px-3 py-1.5 text-xs font-medium bg-rose-950/80 hover:bg-rose-900/90 text-rose-300 border border-rose-800/50 rounded-lg transition cursor-pointer">
                    Remove
                </button>
            </div>
        </div>
        "##,
        company_id = company_id,
        user_id = member.user_id,
        username = username_display,
        email = email_display,
        role = member.role,
        created_at_str = created_at_str,
    )
}

pub fn user_invites_page(user_email: &str, invites: &[CompanyInvite]) -> String {
    let list_html = user_invite_list_fragment(invites);

    let content = format!(
        r##"
        <div class="flex items-center justify-between mb-6">
            <div>
                <h2 class="text-2xl font-bold text-white">Your Invitations</h2>
                <p class="text-slate-400 text-sm mt-1">Company invitations sent to <span class="font-mono text-indigo-300">{user_email}</span></p>
            </div>
        </div>

        <div id="response-message" class="mb-6"></div>

        <div>
            <div id="user-invites-list" class="space-y-3">
                {list_html}
            </div>
        </div>
        "##,
        user_email = user_email,
        list_html = list_html,
    );

    base_layout("My Invites", &content)
}

pub fn user_invite_list_fragment(invites: &[CompanyInvite]) -> String {
    if invites.is_empty() {
        return r##"
            <div class="bg-slate-900/40 border border-dashed border-slate-700/80 rounded-xl p-8 text-center">
                <p class="text-slate-400 text-sm">You have no pending or past company invitations.</p>
            </div>
        "##
        .to_string();
    }

    invites.iter().map(user_invite_row_fragment).collect()
}

pub fn user_invite_row_fragment(invite: &CompanyInvite) -> String {
    let company_name = invite.company_name.as_deref().unwrap_or("Unknown Company");
    let created_at_str = invite.created_at.format("%b %d, %Y").to_string();

    let action_buttons = match invite.status.as_str() {
        "accepted" => r#"<span class="px-3 py-1.5 rounded-lg text-xs font-semibold bg-emerald-950 text-emerald-300 border border-emerald-700/50">Accepted</span>"#.to_string(),
        "declined" => r#"<span class="px-3 py-1.5 rounded-lg text-xs font-semibold bg-rose-950 text-rose-300 border border-rose-700/50">Declined</span>"#.to_string(),
        _ => format!(
            r##"
            <button hx-post="/invites/{invite_id}/accept" hx-target="#user-invite-{invite_id}" hx-swap="outerHTML"
                class="px-3.5 py-1.5 text-xs font-semibold bg-emerald-600 hover:bg-emerald-500 text-white rounded-lg transition cursor-pointer shadow-md shadow-emerald-600/30">
                Accept
            </button>
            <button hx-post="/invites/{invite_id}/decline" hx-target="#user-invite-{invite_id}" hx-swap="outerHTML"
                class="px-3.5 py-1.5 text-xs font-semibold bg-rose-950 hover:bg-rose-900 text-rose-300 border border-rose-800/50 rounded-lg transition cursor-pointer">
                Decline
            </button>
            "##,
            invite_id = invite.id
        ),
    };

    format!(
        r##"
        <div id="user-invite-{invite_id}" class="bg-slate-900/80 border border-slate-700/70 rounded-xl p-4 md:p-5 flex items-center justify-between hover:border-slate-600 transition shadow-sm">
            <div>
                <div class="flex items-center gap-3">
                    <h4 class="text-md font-semibold text-white">{company_name}</h4>
                </div>
                <p class="text-xs text-slate-400 mt-1">Invited to {email} on {created_at_str}</p>
            </div>
            <div class="flex items-center gap-2">
                {action_buttons}
            </div>
        </div>
        "##,
        invite_id = invite.id,
        company_name = company_name,
        email = invite.email,
        created_at_str = created_at_str,
        action_buttons = action_buttons,
    )
}

pub fn workflows_page(company: &Company, app_domain_name: &str, workflows: &[Workflow]) -> String {
    let list_html = workflow_list_fragment(company, app_domain_name, workflows);

    let content = format!(
        r##"
        <div class="flex items-center justify-between mb-6">
            <div>
                <a href="/companies" class="text-xs text-indigo-400 hover:text-indigo-300 font-medium mb-1 inline-block">&larr; Back to Companies</a>
                <h2 class="text-2xl font-bold text-white">{company_name} Workflows</h2>
                <p class="text-slate-400 text-sm mt-0.5">Manage automated workflows for <span class="font-mono text-indigo-300">@{slug}.{app_domain_name}</span></p>
            </div>
        </div>

        <div id="response-message" class="mb-6"></div>

        <!-- Create Workflow Card -->
        <div class="bg-slate-900/70 border border-slate-700/80 rounded-xl p-5 mb-8">
            <h3 class="text-md font-semibold text-white mb-3 flex items-center gap-2">
                <span class="text-emerald-400">+</span> Add New Workflow
            </h3>
            <form hx-post="/companies/{company_id}/workflows" hx-target="#workflow-list" hx-swap="innerHTML" class="space-y-4"
                hx-on::after-request="if(event.detail.successful) this.reset();">
                <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                    <div>
                        <label for="workflow_name" class="block text-xs font-medium text-slate-300 mb-1">Workflow Name</label>
                        <input type="text" id="workflow_name" name="name" required
                            oninput="document.getElementById('workflow_slug').value = this.value.toLowerCase().trim().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '')"
                            class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-emerald-500"
                            placeholder="Inbound Email Handler">
                    </div>
                    <div>
                        <label for="workflow_slug" class="block text-xs font-medium text-slate-300 mb-1">Slug (@{slug}.{app_domain_name})</label>
                        <input type="text" id="workflow_slug" name="slug" required
                            class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-emerald-500 font-mono"
                            placeholder="inbound-email-handler">
                    </div>
                </div>
                <div>
                    <label for="participant_emails" class="block text-xs font-medium text-slate-300 mb-1">Participant Emails (Comma-separated, Optional)</label>
                    <input type="text" id="participant_emails" name="participant_emails"
                        class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-emerald-500"
                        placeholder="agent1@example.com, agent2@example.com">
                </div>
                <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
                    <div>
                        <label for="workflow_provider" class="block text-xs font-medium text-slate-300 mb-1">LLM Provider (Optional Override)</label>
                        <input type="text" id="workflow_provider" name="provider"
                            class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-emerald-500 font-mono"
                            placeholder="google, openai, anthropic">
                    </div>
                    <div>
                        <label for="workflow_model" class="block text-xs font-medium text-slate-300 mb-1">LLM Model (Optional Override)</label>
                        <input type="text" id="workflow_model" name="model"
                            class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-emerald-500 font-mono"
                            placeholder="gemini-2.5-flash, gpt-4o">
                    </div>
                    <div>
                        <label for="workflow_api_key" class="block text-xs font-medium text-slate-300 mb-1">LLM API Key (Optional Override)</label>
                        <input type="password" id="workflow_api_key" name="api_key"
                            class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-emerald-500 font-mono"
                            placeholder="Overrides company key">
                    </div>
                </div>
                <div>
                    <label for="workflow_config" class="block text-xs font-medium text-slate-300 mb-1">Workflow Config (JSON, Optional)</label>
                    <textarea id="workflow_config" name="workflow_config" rows="3"
                        class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm font-mono placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-emerald-500"
                        placeholder='&#123; "trigger": "email", "action": "ai_reply" &#125;'></textarea>
                </div>
                <div class="flex justify-end">
                    <button type="submit"
                        class="px-5 py-2 bg-emerald-600 hover:bg-emerald-500 text-white text-sm font-semibold rounded-lg shadow-md shadow-emerald-600/30 transition cursor-pointer">
                        Create Workflow
                    </button>
                </div>
            </form>
        </div>

        <!-- Workflows List Section -->
        <div>
            <h3 class="text-sm font-semibold uppercase tracking-wider text-slate-400 mb-3">Workflows</h3>
            <div id="workflow-list" class="space-y-3">
                {list_html}
            </div>
        </div>
        "##,
        company_name = company.name,
        slug = company.slug,
        app_domain_name = app_domain_name,
        company_id = company.id,
        list_html = list_html,
    );

    base_layout(&format!("{} Workflows", company.name), &content)
}

pub fn workflow_list_fragment(
    company: &Company,
    app_domain_name: &str,
    workflows: &[Workflow],
) -> String {
    if workflows.is_empty() {
        return r##"
            <div class="bg-slate-900/40 border border-dashed border-slate-700/80 rounded-xl p-8 text-center">
                <p class="text-slate-400 text-sm">No workflows configured yet. Create your first workflow above!</p>
            </div>
        "##
        .to_string();
    }

    workflows
        .iter()
        .map(|wf| workflow_row_fragment(company, app_domain_name, wf))
        .collect()
}

pub fn workflow_row_fragment(
    company: &Company,
    app_domain_name: &str,
    workflow: &Workflow,
) -> String {
    let created_at_str = workflow.created_at.format("%b %d, %Y").to_string();
    let emails_str = match &workflow.participant_emails {
        Some(emails) if !emails.is_empty() => emails.join(", "),
        _ => "None".to_string(),
    };
    let config_str = match &workflow.workflow_config {
        Some(cfg) => serde_json::to_string_pretty(cfg).unwrap_or_else(|_| cfg.to_string()),
        None => "None".to_string(),
    };
    let provider_str = workflow.provider.as_deref().unwrap_or("Default (Company)");
    let model_str = workflow.model.as_deref().unwrap_or("Default (Company)");
    let api_key_str = if workflow.api_key.is_some() {
        "Configured (Workflow Override)"
    } else {
        "Default (Company)"
    };
    let display_slug = format!("{}@{}.{}", workflow.slug, company.slug, app_domain_name);

    format!(
        r##"
        <div id="workflow-{workflow_id}" class="bg-slate-900/80 border border-slate-700/70 rounded-xl p-4 md:p-5 flex flex-col gap-3 hover:border-slate-600 transition shadow-sm">
            <div class="flex items-center justify-between">
                <div>
                    <div class="flex items-center gap-3">
                        <h4 class="text-md font-semibold text-white">{name}</h4>
                        <span class="px-2.5 py-0.5 rounded-full text-xs font-mono bg-emerald-950/90 text-emerald-300 border border-emerald-700/50">{display_slug}</span>
                    </div>
                    <p class="text-xs text-slate-400 mt-1">Created on {created_at_str}</p>
                </div>
                <div class="flex items-center gap-2">
                    <a href="/companies/{company_id}/workflows/{workflow_id}/simulate"
                        class="px-3 py-1.5 text-xs font-medium bg-indigo-900/80 hover:bg-indigo-800 text-indigo-200 border border-indigo-700/50 rounded-lg transition">
                        Simulate
                    </a>
                    <button hx-get="/companies/{company_id}/workflows/{workflow_id}/edit" hx-target="#workflow-{workflow_id}" hx-swap="outerHTML"
                        class="px-3 py-1.5 text-xs font-medium bg-slate-700 hover:bg-slate-600 text-slate-200 rounded-lg transition cursor-pointer">
                        Edit
                    </button>
                    <button hx-delete="/companies/{company_id}/workflows/{workflow_id}" hx-target="#workflow-{workflow_id}" hx-swap="outerHTML" hx-confirm="Are you sure you want to delete workflow '{name}'?"
                        class="px-3 py-1.5 text-xs font-medium bg-rose-950/80 hover:bg-rose-900/90 text-rose-300 border border-rose-800/50 rounded-lg transition cursor-pointer">
                        Delete
                    </button>
                </div>
            </div>
            <div class="grid grid-cols-1 md:grid-cols-3 gap-2 text-xs bg-slate-950/60 p-3 rounded-lg border border-slate-800 font-mono">
                <div>
                    <span class="text-slate-500 block font-sans text-[11px] font-semibold uppercase">Provider:</span>
                    <span class="text-slate-300">{provider_str}</span>
                </div>
                <div>
                    <span class="text-slate-500 block font-sans text-[11px] font-semibold uppercase">Model:</span>
                    <span class="text-slate-300">{model_str}</span>
                </div>
                <div>
                    <span class="text-slate-500 block font-sans text-[11px] font-semibold uppercase">API Key:</span>
                    <span class="text-slate-300">{api_key_str}</span>
                </div>
            </div>
            <div class="grid grid-cols-1 md:grid-cols-2 gap-2 text-xs bg-slate-950/60 p-3 rounded-lg border border-slate-800 font-mono">
                <div>
                    <span class="text-slate-500 block font-sans text-[11px] font-semibold uppercase">Participants:</span>
                    <span class="text-slate-300">{emails_str}</span>
                </div>
                <div>
                    <span class="text-slate-500 block font-sans text-[11px] font-semibold uppercase">Config:</span>
                    <pre class="text-slate-300 whitespace-pre-wrap text-[11px]">{config_str}</pre>
                </div>
            </div>
        </div>
        "##,
        company_id = company.id,
        workflow_id = workflow.id,
        name = workflow.name,
        display_slug = display_slug,
        created_at_str = created_at_str,
        provider_str = provider_str,
        model_str = model_str,
        api_key_str = api_key_str,
        emails_str = emails_str,
        config_str = config_str,
    )
}

pub fn workflow_edit_fragment(
    company: &Company,
    app_domain_name: &str,
    workflow: &Workflow,
) -> String {
    let emails_str = match &workflow.participant_emails {
        Some(emails) => emails.join(", "),
        None => String::new(),
    };
    let config_str = match &workflow.workflow_config {
        Some(cfg) => serde_json::to_string_pretty(cfg).unwrap_or_else(|_| cfg.to_string()),
        None => String::new(),
    };
    let provider_val = workflow.provider.as_deref().unwrap_or("");
    let model_val = workflow.model.as_deref().unwrap_or("");
    let api_key_val = workflow.api_key.as_deref().unwrap_or("");

    format!(
        r##"
        <form id="workflow-{workflow_id}" hx-put="/companies/{company_id}/workflows/{workflow_id}" hx-target="#workflow-{workflow_id}" hx-swap="outerHTML"
            class="bg-slate-900 border border-emerald-500/60 rounded-xl p-4 md:p-5 space-y-4 shadow-lg">
            <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                <div>
                    <label class="block text-xs font-medium text-slate-300 mb-1">Workflow Name</label>
                    <input type="text" name="name" value="{name}" required
                        oninput="this.form.slug.value = this.value.toLowerCase().trim().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '')"
                        class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-emerald-500">
                </div>
                <div>
                    <label class="block text-xs font-medium text-slate-300 mb-1">Slug (@{company_slug}.{app_domain_name})</label>
                    <input type="text" name="slug" value="{slug}" required
                        class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-emerald-500 font-mono">
                </div>
            </div>
            <div>
                <label class="block text-xs font-medium text-slate-300 mb-1">Participant Emails (Comma-separated)</label>
                <input type="text" name="participant_emails" value="{emails_str}"
                    class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-emerald-500">
            </div>
            <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
                <div>
                    <label class="block text-xs font-medium text-slate-300 mb-1">LLM Provider (Optional Override)</label>
                    <input type="text" name="provider" value="{provider}"
                        class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-emerald-500 font-mono"
                        placeholder="google, openai, anthropic">
                </div>
                <div>
                    <label class="block text-xs font-medium text-slate-300 mb-1">LLM Model (Optional Override)</label>
                    <input type="text" name="model" value="{model}"
                        class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-emerald-500 font-mono"
                        placeholder="gemini-2.5-flash, gpt-4o">
                </div>
                <div>
                    <label class="block text-xs font-medium text-slate-300 mb-1">LLM API Key (Optional Override)</label>
                    <input type="password" name="api_key" value="{api_key}"
                        class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-emerald-500 font-mono"
                        placeholder="Leave empty to use Company key">
                </div>
            </div>
            <div>
                <label class="block text-xs font-medium text-slate-300 mb-1">Workflow Config (JSON)</label>
                <textarea name="workflow_config" rows="3"
                    class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm font-mono focus:outline-none focus:ring-2 focus:ring-emerald-500">{config_str}</textarea>
            </div>
            <div class="flex items-center justify-end gap-2">
                <button type="button" hx-get="/companies/{company_id}/workflows/{workflow_id}/cancel" hx-target="#workflow-{workflow_id}" hx-swap="outerHTML"
                    class="px-3 py-1.5 text-xs font-medium bg-slate-700 hover:bg-slate-600 text-slate-200 rounded-lg transition cursor-pointer">
                    Cancel
                </button>
                <button type="submit"
                    class="px-4 py-1.5 text-xs font-semibold bg-emerald-600 hover:bg-emerald-500 text-white rounded-lg transition cursor-pointer">
                    Save Changes
                </button>
            </div>
        </form>
        "##,
        company_id = company.id,
        company_slug = company.slug,
        app_domain_name = app_domain_name,
        workflow_id = workflow.id,
        name = workflow.name,
        slug = workflow.slug,
        emails_str = emails_str,
        provider = provider_val,
        model = model_val,
        api_key = api_key_val,
        config_str = config_str,
    )
}

pub fn workflow_simulation_page(
    company: &Company,
    app_domain_name: &str,
    workflow: &Workflow,
) -> String {
    let target_recipient = format!("{}@{}.{}", workflow.slug, company.slug, app_domain_name);

    let default_sender = match &workflow.participant_emails {
        Some(emails) if !emails.is_empty() => emails[0].clone(),
        _ => "sender@example.com".to_string(),
    };

    let content = format!(
        r##"
        <div class="flex items-center justify-between mb-6">
            <div>
                <a href="/companies/{company_id}/workflows" class="text-xs text-indigo-400 hover:text-indigo-300 font-medium mb-1 inline-block">&larr; Back to Workflows</a>
                <h2 class="text-2xl font-bold text-white">Simulate Webhook: {workflow_name}</h2>
                <p class="text-slate-400 text-sm mt-0.5">Test incoming email webhook resolution for <span class="font-mono text-emerald-300">{target_recipient}</span></p>
            </div>
        </div>

        <div class="bg-slate-900/70 border border-slate-700/80 rounded-xl p-5 mb-6 shadow-md">
            <h3 class="text-md font-semibold text-white mb-4 flex items-center gap-2">
                <span class="text-indigo-400">⚡</span> Simulated Webhook Payload
            </h3>
            <form hx-post="/companies/{company_id}/workflows/{workflow_id}/simulate" hx-target="#simulation-result" hx-swap="innerHTML" class="space-y-4">
                <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                    <div>
                        <label for="to" class="block text-xs font-medium text-slate-300 mb-1">To (Recipient Address)</label>
                        <input type="text" id="to" name="to" value="{target_recipient}" required
                            class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm font-mono focus:outline-none focus:ring-2 focus:ring-indigo-500">
                    </div>
                    <div>
                        <label for="from" class="block text-xs font-medium text-slate-300 mb-1">From (Sender Address)</label>
                        <input type="text" id="from" name="from" value="{default_sender}" required
                            class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm font-mono focus:outline-none focus:ring-2 focus:ring-indigo-500">
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
                            <input type="radio" name="simulation_mode" value="verify" checked class="mt-0.5 text-indigo-600 focus:ring-indigo-500">
                            <div class="ml-2.5">
                                <span class="block text-xs font-bold text-white">Verify</span>
                                <span class="block text-[11px] text-slate-400 mt-0.5">Verification only (Recipient & Sender ACL check)</span>
                            </div>
                        </label>
                        <label class="flex items-start p-3 bg-slate-800 border border-slate-700 rounded-lg cursor-pointer hover:border-amber-500 transition">
                            <input type="radio" name="simulation_mode" value="run_test" class="mt-0.5 text-amber-500 focus:ring-amber-500">
                            <div class="ml-2.5">
                                <span class="block text-xs font-bold text-amber-300">Run_Test</span>
                                <span class="block text-[11px] text-slate-400 mt-0.5">Execute full workflow & agent, skip email dispatch</span>
                            </div>
                        </label>
                        <label class="flex items-start p-3 bg-slate-800 border border-slate-700 rounded-lg cursor-pointer hover:border-emerald-500 transition">
                            <input type="radio" name="simulation_mode" value="run" class="mt-0.5 text-emerald-500 focus:ring-emerald-500">
                            <div class="ml-2.5">
                                <span class="block text-xs font-bold text-emerald-400">Run</span>
                                <span class="block text-[11px] text-slate-400 mt-0.5">Live execution with full AI agent & outbound SMTP send</span>
                            </div>
                        </label>
                    </div>
                </div>
                <div class="flex justify-end">
                    <button type="submit"
                        class="px-5 py-2.5 bg-indigo-600 hover:bg-indigo-500 text-white text-sm font-semibold rounded-lg shadow-md shadow-indigo-600/30 transition cursor-pointer flex items-center gap-2">
                        <span>Trigger Webhook Simulation</span>
                        <span>&rarr;</span>
                    </button>
                </div>
            </form>
        </div>

        <div id="simulation-result"></div>
        "##,
        company_id = company.id,
        workflow_id = workflow.id,
        workflow_name = workflow.name,
        target_recipient = target_recipient,
        default_sender = default_sender,
    );

    base_layout(&format!("Simulate {}", workflow.name), &content)
}

pub fn workflow_simulation_result_fragment(result: &InboundEmailResult) -> String {
    let status_banner = if result.resolved {
        r#"<div class="p-4 rounded-xl bg-emerald-950/80 border border-emerald-600/60 text-emerald-200 text-sm font-semibold flex items-center gap-2">
            <span class="text-emerald-400 text-lg">✓</span>
            <span>Webhook Triggered & Workflow Resolved Successfully!</span>
        </div>"#
    } else if !result.sender_authorized {
        r#"<div class="p-4 rounded-xl bg-rose-950/80 border border-rose-600/60 text-rose-200 text-sm font-semibold flex items-center gap-2">
            <span class="text-rose-400 text-lg">✕</span>
            <span>Unauthorized Sender: Email 'from' address is not listed in workflow participant_emails.</span>
        </div>"#
    } else {
        r#"<div class="p-4 rounded-xl bg-amber-950/80 border border-amber-600/60 text-amber-200 text-sm font-semibold flex items-center gap-2">
            <span class="text-amber-400 text-lg">⚠</span>
            <span>Workflow or Company Not Found for recipient address.</span>
        </div>"#
    };

    let company_name = result
        .company
        .as_ref()
        .map(|c| format!("{} (/{})", c.name, c.slug))
        .unwrap_or_else(|| {
            result
                .company_slug
                .clone()
                .unwrap_or_else(|| "N/A".to_string())
        });

    let workflow_name = result
        .workflow
        .as_ref()
        .map(|w| format!("{} (/{})", w.name, w.slug))
        .unwrap_or_else(|| {
            result
                .workflow_slug
                .clone()
                .unwrap_or_else(|| "N/A".to_string())
        });

    let subject_str = result.email.subject.as_deref().unwrap_or("(No subject)");
    let body_str = result
        .email
        .text_body
        .as_deref()
        .unwrap_or("(No text body)");

    let workflow_config_str = match &result.workflow {
        Some(wf) => match &wf.workflow_config {
            Some(cfg) => serde_json::to_string_pretty(cfg).unwrap_or_else(|_| cfg.to_string()),
            None => "None".to_string(),
        },
        None => "None".to_string(),
    };

    format!(
        r##"
        <div class="space-y-4">
            {status_banner}

            <div class="bg-slate-900 border border-slate-700/80 rounded-xl p-5 space-y-3 text-xs font-mono shadow-lg">
                <h4 class="text-sm font-sans font-bold text-white border-b border-slate-800 pb-2">Simulation Execution Details</h4>
                <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
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
                        <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold">Target Workflow:</span>
                        <span class="text-slate-200">{workflow_name}</span>
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
                    <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold mb-1">Workflow Config:</span>
                    <pre class="bg-slate-950 p-3 rounded-lg text-emerald-300 whitespace-pre-wrap border border-slate-800 text-[11px]">{workflow_config_str}</pre>
                </div>
            </div>
        </div>
        "##,
        status_banner = status_banner,
        to = result.email.to,
        from = result.email.from,
        company_name = company_name,
        workflow_name = workflow_name,
        subject_str = subject_str,
        body_str = body_str,
        workflow_config_str = workflow_config_str,
    )
}

pub fn workflow_simulation_execution_result_fragment(
    sim_res: &SimulationExecutionResult,
) -> String {
    let ingest = &sim_res.ingest_result;

    if !ingest.accepted {
        let reason = ingest
            .reason
            .as_deref()
            .unwrap_or("Ingestion failed / unauthorized");
        return format!(
            r#"<div class="p-4 rounded-xl bg-rose-950/80 border border-rose-600/60 text-rose-200 text-sm font-semibold flex items-center gap-2">
                <span class="text-rose-400 text-lg">✕</span>
                <span>Webhook Ingestion Rejected: {}</span>
            </div>"#,
            reason
        );
    }

    let mode_label = match sim_res.simulation_mode {
        SimulationMode::Verify => "Verify",
        SimulationMode::RunTest => "Run_Test (Dry-Run)",
        SimulationMode::Run => "Run (Live)",
    };

    let status_banner = match sim_res.simulation_mode {
        SimulationMode::RunTest => {
            r#"<div class="p-4 rounded-xl bg-amber-950/80 border border-amber-600/60 text-amber-200 text-sm font-semibold flex items-center gap-2">
                <span class="text-amber-400 text-lg">⚡</span>
                <span>Workflow Executed Successfully in Run_Test Mode! (Outbound email send was skipped / dry-run)</span>
            </div>"#
        }
        SimulationMode::Run => {
            r#"<div class="p-4 rounded-xl bg-emerald-950/80 border border-emerald-600/60 text-emerald-200 text-sm font-semibold flex items-center gap-2">
                <span class="text-emerald-400 text-lg">✓</span>
                <span>Workflow Executed & Outbound Email Dispatched Successfully!</span>
            </div>"#
        }
        SimulationMode::Verify => {
            r#"<div class="p-4 rounded-xl bg-indigo-950/80 border border-indigo-600/60 text-indigo-200 text-sm font-semibold flex items-center gap-2">
                <span class="text-indigo-400 text-lg">✓</span>
                <span>Verification Check Passed!</span>
            </div>"#
        }
    };

    let thread_id_str = ingest
        .thread
        .as_ref()
        .map(|t| t.id.to_string())
        .unwrap_or_else(|| "N/A".to_string());

    let inbound_msg_id = ingest
        .inbound_message
        .as_ref()
        .map(|m| m.message_id.clone())
        .unwrap_or_else(|| "N/A".to_string());

    let company_name = ingest
        .company
        .as_ref()
        .map(|c| format!("{} (/{})", c.name, c.slug))
        .unwrap_or_else(|| "N/A".to_string());

    let workflow_name = ingest
        .workflow
        .as_ref()
        .map(|w| format!("{} (/{})", w.name, w.slug))
        .unwrap_or_else(|| "N/A".to_string());

    let parsed = ingest.parsed_email.as_ref();
    let to_str = parsed
        .and_then(|p| p.recipients_to.first().map(|s| s.as_str()))
        .unwrap_or("N/A");
    let from_str = parsed.map(|p| p.sender.as_str()).unwrap_or("N/A");
    let subject_str = parsed.map(|p| p.subject.as_str()).unwrap_or("(No subject)");
    let text_body_str = parsed
        .map(|p| p.clean_text_body.as_str())
        .unwrap_or("(No text body)");

    let agent_exec = sim_res.agent_execution.as_ref();
    let outbound_msg_id = agent_exec
        .and_then(|a| a.outbound_message_id.clone())
        .unwrap_or_else(|| "N/A".to_string());

    let agent_response_text = agent_exec
        .map(|a| a.agent_response.as_str())
        .unwrap_or("(No response generated)");

    let email_status = if sim_res.simulation_mode == SimulationMode::RunTest {
        "<span class=\"text-amber-400 font-bold\">Skipped (Run_Test Dry-Run)</span>"
    } else if sim_res.simulation_mode == SimulationMode::Run {
        "<span class=\"text-emerald-400 font-bold\">Dispatched via SMTP</span>"
    } else {
        "<span class=\"text-slate-400 font-bold\">None (Verify Only)</span>"
    };

    format!(
        r##"
        <div class="space-y-4">
            {status_banner}

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
                        <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold">Target Workflow:</span>
                        <span class="text-slate-200">{workflow_name}</span>
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
                    <span class="text-slate-500 font-sans block text-[11px] uppercase font-semibold mb-1">Generated AI Agent Response:</span>
                    <div class="bg-slate-950 p-3 rounded-lg text-emerald-300 whitespace-pre-wrap border border-slate-800 font-sans">{agent_response_text}</div>
                </div>
            </div>
        </div>
        "##,
        status_banner = status_banner,
        mode_label = mode_label,
        email_status = email_status,
        to_str = to_str,
        from_str = from_str,
        company_name = company_name,
        workflow_name = workflow_name,
        thread_id_str = thread_id_str,
        inbound_msg_id = inbound_msg_id,
        outbound_msg_id = outbound_msg_id,
        subject_str = subject_str,
        text_body_str = text_body_str,
        agent_response_text = agent_response_text,
    )
}

pub fn company_tasks_page(
    company: &Company,
    workflows: &[Workflow],
    tasks: &[BackgroundTask],
    current_wf: Option<Uuid>,
    current_status: Option<TaskStatus>,
    sort_asc: bool,
) -> String {
    let task_list_html = task_list_fragment(company.id, tasks);

    let mut wf_options = String::from("<option value=\"\">All Workflows</option>");
    for wf in workflows {
        let selected = if current_wf == Some(wf.id) {
            "selected"
        } else {
            ""
        };
        wf_options.push_str(&format!(
            "<option value=\"{}\" {}>{} (/{})</option>",
            wf.id, selected, wf.name, wf.slug
        ));
    }

    let status_options_vec = vec![
        ("", "All Statuses"),
        ("pending", "Pending"),
        ("processing", "Processing"),
        ("completed", "Completed"),
        ("failed", "Failed"),
        ("dead_letter", "Dead Letter"),
        ("stopped", "Stopped"),
    ];

    let mut status_options = String::new();
    let current_status_str = current_status.as_ref().map(|s| s.as_str()).unwrap_or("");
    for (val, label) in status_options_vec {
        let selected = if current_status_str == val {
            "selected"
        } else {
            ""
        };
        status_options.push_str(&format!(
            "<option value=\"{}\" {}>{}</option>",
            val, selected, label
        ));
    }

    let sort_desc_selected = if !sort_asc { "selected" } else { "" };
    let sort_asc_selected = if sort_asc { "selected" } else { "" };

    let content = format!(
        r##"
        <div class="flex items-center justify-between mb-6">
            <div>
                <a href="/companies" class="text-xs text-indigo-400 hover:text-indigo-300 font-medium mb-1 inline-block">&larr; Back to Companies</a>
                <h2 class="text-2xl font-bold text-white">{company_name} Background Tasks</h2>
                <p class="text-slate-400 text-sm mt-0.5">Monitor, stop, or resume background processing tasks for <span class="font-mono text-indigo-300">/{slug}</span></p>
            </div>
        </div>

        <div id="response-message" class="mb-6"></div>

        <!-- Filter & Sort Bar -->
        <div class="bg-slate-900/70 border border-slate-700/80 rounded-xl p-4 mb-6">
            <form hx-get="/companies/{company_id}/tasks/filter" hx-target="#task-list" hx-swap="innerHTML" class="grid grid-cols-1 md:grid-cols-3 gap-4">
                <div>
                    <label class="block text-xs font-medium text-slate-300 mb-1">Filter by Workflow</label>
                    <select name="workflow_id" onchange="this.form.requestSubmit()"
                        class="w-full px-3 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-indigo-500">
                        {wf_options}
                    </select>
                </div>
                <div>
                    <label class="block text-xs font-medium text-slate-300 mb-1">Filter by Status</label>
                    <select name="status" onchange="this.form.requestSubmit()"
                        class="w-full px-3 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-indigo-500">
                        {status_options}
                    </select>
                </div>
                <div>
                    <label class="block text-xs font-medium text-slate-300 mb-1">Sort by Time</label>
                    <select name="sort" onchange="this.form.requestSubmit()"
                        class="w-full px-3 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-indigo-500">
                        <option value="desc" {sort_desc_selected}>Newest First</option>
                        <option value="asc" {sort_asc_selected}>Oldest First</option>
                    </select>
                </div>
            </form>
        </div>

        <!-- Task List Section -->
        <div>
            <h3 class="text-sm font-semibold uppercase tracking-wider text-slate-400 mb-3">Tasks</h3>
            <div id="task-list" class="space-y-3">
                {task_list_html}
            </div>
        </div>
        "##,
        company_name = company.name,
        slug = company.slug,
        company_id = company.id,
        wf_options = wf_options,
        status_options = status_options,
        sort_desc_selected = sort_desc_selected,
        sort_asc_selected = sort_asc_selected,
        task_list_html = task_list_html,
    );

    base_layout(&format!("{} Tasks", company.name), &content)
}

pub fn task_list_fragment(company_id: Uuid, tasks: &[BackgroundTask]) -> String {
    if tasks.is_empty() {
        return r##"
            <div class="bg-slate-900/40 border border-dashed border-slate-700/80 rounded-xl p-8 text-center">
                <p class="text-slate-400 text-sm">No tasks matching the selected filters.</p>
            </div>
        "##
        .to_string();
    }

    tasks
        .iter()
        .map(|t| task_row_fragment(company_id, t))
        .collect()
}

pub fn task_row_fragment(company_id: Uuid, task: &BackgroundTask) -> String {
    let created_at_str = task.created_at.format("%b %d, %H:%M:%S").to_string();
    let status_badge = match task.status {
        TaskStatus::Pending => {
            r#"<span class="px-2.5 py-0.5 rounded-full text-xs font-semibold bg-amber-950 text-amber-300 border border-amber-700/50">Pending</span>"#
        }
        TaskStatus::Processing => {
            r#"<span class="px-2.5 py-0.5 rounded-full text-xs font-semibold bg-indigo-950 text-indigo-300 border border-indigo-700/50 animate-pulse">Processing</span>"#
        }
        TaskStatus::Completed => {
            r#"<span class="px-2.5 py-0.5 rounded-full text-xs font-semibold bg-emerald-950 text-emerald-300 border border-emerald-700/50">Completed</span>"#
        }
        TaskStatus::Failed => {
            r#"<span class="px-2.5 py-0.5 rounded-full text-xs font-semibold bg-rose-950 text-rose-300 border border-rose-700/50">Failed</span>"#
        }
        TaskStatus::DeadLetter => {
            r#"<span class="px-2.5 py-0.5 rounded-full text-xs font-semibold bg-purple-950 text-purple-300 border border-purple-700/50">Dead Letter</span>"#
        }
        TaskStatus::Stopped => {
            r#"<span class="px-2.5 py-0.5 rounded-full text-xs font-semibold bg-slate-800 text-slate-400 border border-slate-600">Stopped</span>"#
        }
    };

    let action_button = match task.status {
        TaskStatus::Pending | TaskStatus::Processing | TaskStatus::Failed => format!(
            r##"<button hx-post="/companies/{company_id}/tasks/{task_id}/stop" hx-target="#task-{task_id}" hx-swap="outerHTML"
                class="px-3 py-1.5 text-xs font-semibold bg-rose-950 hover:bg-rose-900 text-rose-300 border border-rose-800/50 rounded-lg transition cursor-pointer">
                Stop Task
            </button>"##,
            company_id = company_id,
            task_id = task.id
        ),
        TaskStatus::Stopped | TaskStatus::DeadLetter => format!(
            r##"<button hx-post="/companies/{company_id}/tasks/{task_id}/resume" hx-target="#task-{task_id}" hx-swap="outerHTML"
                class="px-3 py-1.5 text-xs font-semibold bg-emerald-600 hover:bg-emerald-500 text-white rounded-lg transition cursor-pointer shadow-md shadow-emerald-600/30">
                Resume Task
            </button>"##,
            company_id = company_id,
            task_id = task.id
        ),
        _ => String::new(),
    };

    let error_html = match &task.last_error {
        Some(err) if !err.is_empty() => format!(
            r##"<div class="mt-2 text-xs font-mono bg-slate-950/80 p-2 rounded border border-rose-900/50 text-rose-300">Error: {err}</div>"##
        ),
        _ => String::new(),
    };

    format!(
        r##"
        <div id="task-{task_id}" class="bg-slate-900/80 border border-slate-700/70 rounded-xl p-4 md:p-5 hover:border-slate-600 transition shadow-sm">
            <div class="flex items-center justify-between">
                <div>
                    <div class="flex items-center gap-3">
                        <span class="font-mono text-xs text-slate-300 font-semibold">{task_id}</span>
                        {status_badge}
                        <span class="text-xs text-slate-400 font-mono">Retries: {retry_count}/{max_retries}</span>
                    </div>
                    <p class="text-xs text-slate-400 mt-1">Type: <span class="font-mono text-indigo-300">{task_type}</span> • Enqueued {created_at_str}</p>
                </div>
                <div class="flex items-center gap-2">
                    {action_button}
                </div>
            </div>
            {error_html}
        </div>
        "##,
        task_id = task.id,
        status_badge = status_badge,
        retry_count = task.retry_count,
        max_retries = task.max_retries,
        task_type = task.task_type,
        created_at_str = created_at_str,
        action_button = action_button,
        error_html = error_html,
    )
}
