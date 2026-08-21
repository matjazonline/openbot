//! Company settings, team members and invitations.

use super::*;

pub fn companies_page(companies: &[Company]) -> String {
    let list_html = company_list_fragment(companies);

    let content = format!(
        r##"
        <div class="flex items-center justify-between mb-6">
            <div>
                <h2 class="text-2xl font-bold text-white">Company Accounts</h2>
                <p class="text-slate-400 text-sm mt-1">Manage your organization profiles and indexed slugs</p>
            </div>
            <button id="company-form-toggle" type="button" aria-controls="company-form-card" aria-expanded="false"
                onclick="const card = document.getElementById('company-form-card'); const opening = card.classList.contains('hidden'); card.classList.toggle('hidden'); this.setAttribute('aria-expanded', opening);"
                class="px-4 py-2 bg-indigo-600 hover:bg-indigo-500 text-white text-sm font-semibold rounded-lg shadow-md shadow-indigo-600/30 transition cursor-pointer">
                Add Company
            </button>
        </div>

        <div id="response-message" class="mb-6"></div>

        <!-- Create Company Card -->
        <div id="company-form-card" class="hidden bg-slate-900/70 border border-slate-700/80 rounded-xl p-5 mb-8">
            <h3 class="text-md font-semibold text-white mb-3 flex items-center gap-2">
                <span class="text-indigo-400">+</span> Add New Company
            </h3>
            <form hx-post="/companies" hx-target="#company-list" hx-swap="innerHTML" class="space-y-4"
                hx-on::after-request="if(event.detail.successful && event.detail.elt === this) {{ this.reset(); document.getElementById('company-form-card').classList.add('hidden'); document.getElementById('company-form-toggle').setAttribute('aria-expanded', 'false'); }}">
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
                <p class="text-slate-400 text-sm">No companies registered yet. Use Add Company to create your first one.</p>
            </div>
        "##
        .to_string();
    }

    companies.iter().map(company_row_fragment).collect()
}

pub fn company_row_fragment(company: &Company) -> String {
    let created_at_str = super::format_date(company.created_at);
    format!(
        r##"
        <div id="company-{id}" onclick="selectCompany('{id}')" class="bg-slate-900/80 border border-slate-700/70 rounded-xl p-4 md:p-5 flex items-center justify-between hover:border-slate-600 transition shadow-sm cursor-pointer">
            <div>
                <div class="flex items-center gap-3">
                    <h4 class="text-md font-semibold text-white">{name}</h4>
                    <span class="px-2.5 py-0.5 rounded-full text-xs font-mono bg-indigo-950/90 text-indigo-300 border border-indigo-700/50">/{slug}</span>
                    <span id="selected-badge-{id}" class="hidden px-2 py-0.5 rounded text-[10px] font-semibold bg-emerald-950 text-emerald-400 border border-emerald-700/60 uppercase tracking-wider">Active</span>
                </div>
                <p class="text-xs text-slate-400 mt-1">Added {created_at_str}</p>
            </div>
            <div class="flex items-center gap-2">
                <a href="/companies/{id}/tasks" onclick="selectCompany('{id}')"
                    class="px-3 py-1.5 text-xs font-medium bg-amber-900/80 hover:bg-amber-800 text-amber-200 border border-amber-700/50 rounded-lg transition">
                    Tasks
                </a>
                <a href="/companies/{id}/agents" onclick="selectCompany('{id}')"
                    class="px-3 py-1.5 text-xs font-medium bg-sky-900/80 hover:bg-sky-800 text-sky-200 border border-sky-700/50 rounded-lg transition">
                    Agents
                </a>
                <a href="/companies/{id}/channels" onclick="selectCompany('{id}')"
                    class="px-3 py-1.5 text-xs font-medium bg-emerald-900/80 hover:bg-emerald-800 text-emerald-200 border border-emerald-700/50 rounded-lg transition">
                    Channels
                </a>
                <a href="/companies/{id}/invites" onclick="selectCompany('{id}')"
                    class="px-3 py-1.5 text-xs font-medium bg-indigo-900/80 hover:bg-indigo-800 text-indigo-200 border border-indigo-700/50 rounded-lg transition">
                    Invites & Team
                </a>
                <button hx-get="/companies/{id}/edit" hx-target="#company-{id}" hx-swap="outerHTML" onclick="event.stopPropagation()"
                    class="px-3 py-1.5 text-xs font-medium bg-slate-700 hover:bg-slate-600 text-slate-200 rounded-lg transition cursor-pointer">
                    Edit
                </button>
                <button hx-delete="/companies/{id}" hx-target="#company-{id}" hx-swap="outerHTML" hx-confirm="Are you sure you want to delete '{name}'?" onclick="event.stopPropagation()"
                    hx-on::after-request="if(event.detail.successful) clearCachedCompanyIfMatch('{id}');"
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
            <!-- A save sets every column, so the picture -- which is picked in `/ui`, not here --
                 rides along rather than being cleared by a rename. -->
            <input type="hidden" name="avatar_url" value="{avatar_url}">
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
        avatar_url = escape_html_text(company.avatar_url.as_deref().unwrap_or("")),
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
                hx-on::after-request="if(event.detail.successful && event.detail.elt === this) this.reset();">
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
    let created_at_str = super::format_date(invite.created_at);
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
    let created_at_str = super::format_date(member.created_at);
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
    let created_at_str = super::format_date(invite.created_at);

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
