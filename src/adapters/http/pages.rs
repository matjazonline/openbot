use pulldown_cmark::{Options, Parser, html};
use uuid::Uuid;

use crate::entities::{
    agent::Agent,
    approval::{ApprovalStatus, HumanApproval},
    channel::Channel,
    company::Company,
    company_invite::CompanyInvite,
    company_member::CompanyMember,
    message::{Message, MessageDirection, MessageRole},
    task::{BackgroundTask, TaskStatus},
    thread::Thread,
};
use crate::use_cases::channel::InboundEmailResult;
use crate::use_cases::thread::{SimulationExecutionResult, SimulationMode};

const MARKDOWN_CONTENT_STYLES: &str = "[&_p]:mb-2 [&_p:last-child]:mb-0 [&_h1]:mb-2 [&_h1]:text-base [&_h1]:font-bold [&_h2]:mb-2 [&_h2]:text-sm [&_h2]:font-bold [&_h3]:mb-2 [&_h3]:font-bold [&_ul]:list-disc [&_ul]:pl-5 [&_ol]:list-decimal [&_ol]:pl-5 [&_li]:my-1 [&_blockquote]:my-2 [&_blockquote]:border-l-2 [&_blockquote]:border-slate-600 [&_blockquote]:pl-3 [&_blockquote]:text-slate-300 [&_a]:underline [&_a]:text-indigo-300 [&_strong]:font-bold [&_code]:rounded [&_code]:bg-slate-800 [&_code]:px-1 [&_pre]:my-2 [&_pre]:overflow-x-auto [&_pre]:rounded [&_pre]:bg-slate-900 [&_pre]:p-3 [&_pre_code]:bg-transparent [&_pre_code]:p-0 [&_table]:my-2 [&_table]:w-full [&_th]:border [&_th]:border-slate-700 [&_th]:p-2 [&_th]:text-left [&_td]:border [&_td]:border-slate-800 [&_td]:p-2";

fn render_markdown(markdown: &str) -> String {
    let options =
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS;
    let parser = Parser::new_ext(markdown, options);
    let mut rendered = String::new();
    html::push_html(&mut rendered, parser);

    ammonia::Builder::default()
        .link_rel(Some("noopener noreferrer"))
        .clean(&rendered)
        .to_string()
}

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
                <a id="nav-agents" href="#" class="hidden text-slate-300 hover:text-white transition">Agents</a>
                <a id="nav-channels" href="#" class="hidden text-slate-300 hover:text-white transition">Channels</a>
                <a href="/invites" class="text-slate-300 hover:text-white transition">My Invites</a>
                <a href="/login" class="text-slate-300 hover:text-white transition">Sign In</a>
                <a href="/register" class="px-3 py-1.5 bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg transition">Sign Up</a>
            </div>
        </nav>
        <div class="bg-slate-800/80 backdrop-blur-md border border-slate-700/60 rounded-2xl shadow-2xl p-6 md:p-8">
            {content}
        </div>
    </div>
    <script>
        function getCachedCompanyId() {{
            return localStorage.getItem('cached_company_id');
        }}

        function selectCompany(companyId) {{
            if (!companyId) return;
            localStorage.setItem('cached_company_id', companyId);
            updateNavChannels();
        }}

        function clearCachedCompanyIfMatch(companyId) {{
            if (getCachedCompanyId() === companyId) {{
                localStorage.removeItem('cached_company_id');
                updateNavChannels();
            }}
        }}

        function updateNavChannels() {{
            const navChannels = document.getElementById('nav-channels');
            const navAgents = document.getElementById('nav-agents');
            const companyId = getCachedCompanyId();
            if (navChannels) {{
                if (companyId) {{
                    navChannels.href = '/companies/' + companyId + '/channels';
                    navChannels.classList.remove('hidden');
                }} else {{
                    navChannels.classList.add('hidden');
                }}
            }}
            if (navAgents) {{
                if (companyId) {{
                    navAgents.href = '/companies/' + companyId + '/agents';
                    navAgents.classList.remove('hidden');
                }} else {{
                    navAgents.classList.add('hidden');
                }}
            }}
            document.querySelectorAll('[id^="selected-badge-"]').forEach(el => {{
                if (el.id === 'selected-badge-' + companyId) {{
                    el.classList.remove('hidden');
                }} else {{
                    el.classList.add('hidden');
                }}
            }});
        }}

        function autoDetectAndSyncCompany() {{
            const match = window.location.pathname.match(/\/companies\/([a-f0-9\-]{{36}})/i);
            if (match && match[1]) {{
                selectCompany(match[1]);
            }} else {{
                updateNavChannels();
            }}
        }}

        document.addEventListener('DOMContentLoaded', autoDetectAndSyncCompany);
        document.addEventListener('htmx:afterSettle', autoDetectAndSyncCompany);
        autoDetectAndSyncCompany();

        function toggleSpamWarning(input) {{
            var form = input.closest('form');
            if (!form) return;
            var box = form.querySelector('.spam-disabled-box');
            if (!box) return;
            var checkbox = box.querySelector('input[type="checkbox"]');
            var isPublic = input.value.toLowerCase().includes('@public');
            if (isPublic) {{
                box.classList.remove('opacity-40', 'pointer-events-none', 'grayscale');
                if (checkbox) {{
                    checkbox.disabled = false;
                }}
            }} else {{
                box.classList.add('opacity-40', 'pointer-events-none', 'grayscale');
                if (checkbox) {{
                    checkbox.disabled = true;
                    checkbox.checked = false;
                }}
            }}
        }}

        window.companyTeamCache = window.companyTeamCache || {{}};

        async function fetchCompanyTeam(companyId) {{
            if (!companyId) return [];
            if (window.companyTeamCache[companyId]) {{
                return window.companyTeamCache[companyId];
            }}
            try {{
                const res = await fetch('/api/companies/' + companyId + '/team', {{ credentials: 'same-origin' }});
                if (!res.ok) return [];
                const data = await res.json();
                if (data && data.success && Array.isArray(data.members)) {{
                    const members = data.members.filter(m => m.email && m.email.trim().length > 0);
                    window.companyTeamCache[companyId] = members;
                    return members;
                }}
            }} catch (e) {{
                console.error('Error fetching team members:', e);
            }}
            return [];
        }}

        async function initTeamAutocomplete() {{
            const inputs = document.querySelectorAll('input[name="participant_emails"]');
            if (!inputs.length) return;

            for (const input of inputs) {{
                if (input.dataset.teamAutocompleteInitialized === 'true') continue;
                input.dataset.teamAutocompleteInitialized = 'true';
                input.setAttribute('autocomplete', 'off');

                let companyId = input.dataset.companyId ||
                                input.closest('[data-company-id]')?.dataset.companyId;
                if (!companyId) {{
                    const match = window.location.pathname.match(/\/companies\/([a-f0-9\-]{{36}})/i);
                    if (match && match[1]) companyId = match[1];
                }}
                if (!companyId && typeof getCachedCompanyId === 'function') {{
                    companyId = getCachedCompanyId();
                }}

                if (!companyId) continue;

                const members = await fetchCompanyTeam(companyId);
                if (!members.length) continue;

                const parent = input.parentElement;
                if (!parent) continue;

                let wrapper = parent.querySelector('.team-autocomplete-wrapper');
                if (!wrapper) {{
                    wrapper = document.createElement('div');
                    wrapper.className = 'team-autocomplete-wrapper relative';
                    input.parentNode.insertBefore(wrapper, input);
                    wrapper.appendChild(input);
                }}

                let chipsContainer = parent.querySelector('.team-chips-container');
                if (!chipsContainer) {{
                    chipsContainer = document.createElement('div');
                    chipsContainer.className = 'team-chips-container mt-2 flex flex-wrap items-center gap-1.5 text-xs';
                    parent.appendChild(chipsContainer);
                }}

                let dropdown = wrapper.querySelector('.team-dropdown');
                if (!dropdown) {{
                    dropdown = document.createElement('div');
                    dropdown.className = 'team-dropdown absolute left-0 right-0 top-full mt-1 z-50 bg-slate-800 border border-slate-700 rounded-lg shadow-xl hidden max-h-48 overflow-y-auto font-sans';
                    wrapper.appendChild(dropdown);
                }}

                function getParsedEmails() {{
                    return input.value
                        .split(',')
                        .map(s => s.trim().toLowerCase())
                        .filter(Boolean);
                }}

                function updateChips() {{
                    const currentEmails = getParsedEmails();
                    chipsContainer.innerHTML = '<span class="text-[11px] font-medium text-slate-400 mr-1">Team:</span>';
                    members.forEach(member => {{
                        const emailLower = member.email.toLowerCase();
                        const isSelected = currentEmails.includes(emailLower);
                        const btn = document.createElement('button');
                        btn.type = 'button';
                        btn.className = isSelected
                            ? 'px-2 py-0.5 rounded-md bg-emerald-500/20 text-emerald-300 border border-emerald-500/40 text-[11px] font-mono cursor-pointer hover:bg-emerald-500/30 transition flex items-center gap-1'
                            : 'px-2 py-0.5 rounded-md bg-slate-800 text-slate-300 border border-slate-700 text-[11px] font-mono cursor-pointer hover:bg-slate-700 hover:text-white transition flex items-center gap-1';
                        btn.innerHTML = (isSelected ? '✓ ' : '+ ') + member.email;
                        btn.title = (member.username ? member.username + ' (' + member.role + ')' : member.role);
                        btn.addEventListener('click', (e) => {{
                            e.preventDefault();
                            let emails = input.value.split(',').map(s => s.trim()).filter(Boolean);
                            if (isSelected) {{
                                emails = emails.filter(e => e.toLowerCase() !== emailLower);
                            }} else {{
                                if (!emails.some(e => e.toLowerCase() === emailLower)) {{
                                    emails.push(member.email);
                                }}
                            }}
                            input.value = emails.join(', ');
                            input.dispatchEvent(new Event('input', {{ bubbles: true }}));
                            updateChips();
                        }});
                        chipsContainer.appendChild(btn);
                    }});
                }}

                function getCurrentToken() {{
                    const val = input.value;
                    const pos = input.selectionStart || val.length;
                    const left = val.slice(0, pos);
                    const lastComma = left.lastIndexOf(',');
                    const token = lastComma >= 0 ? left.slice(lastComma + 1) : left;
                    return {{ token: token.trim(), pos, lastComma }};
                }}

                function renderDropdown() {{
                    const {{ token, lastComma }} = getCurrentToken();
                    if (!token) {{
                        dropdown.classList.add('hidden');
                        return;
                    }}
                    const tokenLower = token.toLowerCase();
                    const currentEmails = getParsedEmails();
                    const matches = members.filter(m => {{
                        const emailLower = m.email.toLowerCase();
                        const nameLower = (m.username || '').toLowerCase();
                        return (emailLower.includes(tokenLower) || nameLower.includes(tokenLower)) &&
                               !currentEmails.includes(emailLower);
                    }});

                    if (!matches.length) {{
                        dropdown.classList.add('hidden');
                        return;
                    }}

                    dropdown.innerHTML = '';
                    matches.forEach(m => {{
                        const item = document.createElement('div');
                        item.className = 'px-3 py-2 hover:bg-slate-700/80 cursor-pointer text-xs flex items-center justify-between border-b border-slate-700/50 last:border-b-0 text-slate-200';
                        item.innerHTML = `<span class="font-mono text-emerald-400 font-medium">${{m.email}}</span>` +
                            (m.username ? `<span class="text-slate-400 text-[11px]">${{m.username}} (${{m.role}})</span>` : `<span class="text-slate-400 text-[11px]">${{m.role}}</span>`);

                        item.addEventListener('mousedown', (e) => {{
                            e.preventDefault();
                            const val = input.value;
                            const before = lastComma >= 0 ? val.slice(0, lastComma + 1) : '';
                            const after = val.slice(input.selectionStart || val.length);
                            const afterClean = after.replace(/^[^,]*/, '');
                            const prefix = before ? before.trim() + ' ' : '';
                            input.value = prefix + m.email + ', ' + afterClean.trim();
                            input.dispatchEvent(new Event('input', {{ bubbles: true }}));
                            dropdown.classList.add('hidden');
                            updateChips();
                        }});
                        dropdown.appendChild(item);
                    }});

                    dropdown.classList.remove('hidden');
                }}

                input.addEventListener('input', () => {{
                    updateChips();
                    renderDropdown();
                }});

                input.addEventListener('focus', () => {{
                    updateChips();
                    renderDropdown();
                }});

                input.addEventListener('blur', () => {{
                    setTimeout(() => dropdown.classList.add('hidden'), 200);
                }});

                updateChips();
            }}
        }}

        document.addEventListener('DOMContentLoaded', initTeamAutocomplete);
        document.addEventListener('htmx:afterSettle', initTeamAutocomplete);
    </script>
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
    let created_at_str = company.created_at.format("%b %d, %Y").to_string();
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

pub fn render_agents_selection(
    company_id: Uuid,
    agents: &[Agent],
    selected_ids: Option<&[Uuid]>,
    container_id: &str,
) -> String {
    render_agents_selection_full(company_id, agents, selected_ids, container_id, None)
}

pub fn render_agents_selection_full(
    company_id: Uuid,
    agents: &[Agent],
    selected_ids: Option<&[Uuid]>,
    container_id: &str,
    error_msg: Option<&str>,
) -> String {
    let initial_id = match selected_ids {
        Some(ids) if !ids.is_empty() => ids[0].to_string(),
        _ => String::new(),
    };

    let group_name = format!("agent_radio_{}_{}", container_id, Uuid::new_v4().simple());
    let agents_selection_id = format!("agents-selection-{container_id}");
    let inline_form_id = format!("inline-agent-form-{container_id}");
    let hx_include_val = format!("#{inline_form_id}");
    let hx_target_val = format!("#{agents_selection_id}");
    let hx_post_val = format!("/companies/{company_id}/agents/inline?container_id={container_id}");

    let none_checked = if initial_id.is_empty() { "checked" } else { "" };

    let mut agent_cards = format!(
        r#"
        <label class="flex items-center gap-2 p-2 bg-slate-800/80 border border-slate-700/80 rounded-lg cursor-pointer hover:bg-slate-700/60 transition">
            <input type="radio" name="{group_name}" value="" {none_checked}
                onchange="let parent = this.closest('#{agents_selection_id}'); if (parent) {{ let target = parent.querySelector('input[name=agent_ids]'); if (target) target.value = ''; }}"
                class="border-slate-700 text-indigo-600 focus:ring-indigo-500">
            <div class="text-xs flex flex-col">
                <span class="font-medium text-slate-300">None</span>
                <span class="text-slate-500 font-mono text-[10px]">Use channel fallback / custom agent</span>
            </div>
        </label>
        "#,
        group_name = group_name,
        agents_selection_id = agents_selection_id,
        none_checked = none_checked
    );

    for agent in agents {
        let checked = match selected_ids {
            Some(ids) if ids.contains(&agent.id) => "checked",
            _ => "",
        };
        agent_cards.push_str(&format!(
            r#"
            <label class="flex items-center gap-2 p-2 bg-slate-800/80 border border-slate-700/80 rounded-lg cursor-pointer hover:bg-slate-700/60 transition">
                <input type="radio" name="{group_name}" value="{id}" {checked}
                    onchange="let parent = this.closest('#{agents_selection_id}'); if (parent) {{ let target = parent.querySelector('input[name=agent_ids]'); if (target) target.value = this.value; }}"
                    class="border-slate-700 text-indigo-600 focus:ring-indigo-500">
                <div class="text-xs flex flex-col">
                    <span class="font-medium text-white">{name}</span>
                    <span class="text-slate-400 font-mono text-[10px]">@{slug}</span>
                </div>
            </label>
            "#,
            group_name = group_name,
            agents_selection_id = agents_selection_id,
            id = agent.id,
            name = agent.name,
            slug = agent.slug,
            checked = checked
        ));
    }

    let form_hidden = if error_msg.is_some() { "" } else { "hidden" };
    let error_html = match error_msg {
        Some(msg) => format!(
            r#"<div class="p-2 mb-2 bg-red-500/10 border border-red-500/30 rounded text-red-400 text-xs">{msg}</div>"#
        ),
        None => String::new(),
    };

    let inline_prompt_gen_html = render_ai_prompt_generator(
        company_id,
        &format!("inline_agent_system_prompt_{container_id}"),
        &format!("inline_prompt_gen_box_{container_id}"),
        &format!("inline_prompt_gen_input_{container_id}"),
        &format!("inline_prompt_gen_status_{container_id}"),
        &format!(
            ", #inline_agent_provider_{container_id}, #inline_agent_model_{container_id}, #inline_agent_api_key_{container_id}"
        ),
    );

    format!(
        r#"
        <div id="{agents_selection_id}" class="space-y-3">
            <input type="hidden" name="agent_ids" value="{initial_id}">
            <div class="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 gap-2 mt-1">
                {agent_cards}
            </div>

            <div>
                <button type="button"
                    onclick="let el = document.getElementById('{inline_form_id}'); if (el) el.classList.toggle('hidden'); return false;"
                    class="text-xs text-emerald-400 hover:text-emerald-300 font-medium cursor-pointer inline-flex items-center gap-1">
                    <span>+ Create New Agent Inline</span>
                </button>
            </div>

            <div id="{inline_form_id}" class="{form_hidden} bg-slate-800/90 border border-indigo-500/50 p-3.5 rounded-xl space-y-3 mt-2 shadow-inner">
                <div class="flex items-center justify-between text-xs font-semibold text-indigo-300 border-b border-slate-700/60 pb-1.5">
                    <span>Create Agent Inline</span>
                    <span class="text-[10px] font-normal text-slate-400">Selected automatically after creation</span>
                </div>
                {error_html}
                <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
                    <div>
                        <label class="block text-[11px] font-medium text-slate-300 mb-0.5">Agent Name</label>
                        <input type="text" id="inline_agent_name_{container_id}" name="inline_agent_name"
                            oninput="document.getElementById('inline_agent_slug_{container_id}').value = this.value.toLowerCase().trim().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '')"
                            class="w-full px-2.5 py-1.5 bg-slate-900 border border-slate-700 rounded text-white text-xs placeholder-slate-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
                            placeholder="Support Specialist">
                    </div>
                    <div>
                        <label class="block text-[11px] font-medium text-slate-300 mb-0.5">Slug</label>
                        <input type="text" id="inline_agent_slug_{container_id}" name="inline_agent_slug"
                            class="w-full px-2.5 py-1.5 bg-slate-900 border border-slate-700 rounded text-white text-xs font-mono placeholder-slate-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
                            placeholder="support-specialist">
                    </div>
                </div>
                <div class="grid grid-cols-1 sm:grid-cols-3 gap-2">
                    <div>
                        <label class="block text-[11px] font-medium text-slate-300 mb-0.5">Provider (Optional)</label>
                        <input type="text" id="inline_agent_provider_{container_id}" name="inline_agent_provider"
                            class="w-full px-2.5 py-1.5 bg-slate-900 border border-slate-700 rounded text-white text-xs font-mono placeholder-slate-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
                            placeholder="google, openai">
                    </div>
                    <div>
                        <label class="block text-[11px] font-medium text-slate-300 mb-0.5">Model (Optional)</label>
                        <input type="text" id="inline_agent_model_{container_id}" name="inline_agent_model"
                            class="w-full px-2.5 py-1.5 bg-slate-900 border border-slate-700 rounded text-white text-xs font-mono placeholder-slate-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
                            placeholder="gemini-2.5-flash">
                    </div>
                    <div>
                        <label class="block text-[11px] font-medium text-slate-300 mb-0.5">API Key (Optional)</label>
                        <input type="password" id="inline_agent_api_key_{container_id}" name="inline_agent_api_key"
                            class="w-full px-2.5 py-1.5 bg-slate-900 border border-slate-700 rounded text-white text-xs font-mono placeholder-slate-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
                            placeholder="Key override">
                    </div>
                </div>
                <div>
                    {inline_prompt_gen_html}
                    <textarea id="inline_agent_system_prompt_{container_id}" name="inline_agent_system_prompt" rows="2"
                        class="w-full px-2.5 py-1.5 bg-slate-900 border border-slate-700 rounded text-white text-xs placeholder-slate-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
                        placeholder="You are a helpful support agent..."></textarea>
                </div>
                <div class="flex justify-end gap-2 pt-1">
                    <button type="button"
                        onclick="let el = document.getElementById('{inline_form_id}'); if (el) el.classList.add('hidden'); return false;"
                        class="px-2.5 py-1 bg-slate-700 hover:bg-slate-600 text-slate-200 text-xs font-medium rounded transition cursor-pointer">
                        Cancel
                    </button>
                    <button type="button"
                        hx-post="{hx_post_val}"
                        hx-target="{hx_target_val}"
                        hx-swap="outerHTML"
                        hx-include="{hx_include_val}"
                        hx-novalidate="true"
                        formnovalidate
                        class="px-3 py-1 bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-semibold rounded shadow transition cursor-pointer">
                        Create & Select Agent
                    </button>
                </div>
            </div>
        </div>
        "#,
        agents_selection_id = agents_selection_id,
        inline_form_id = inline_form_id,
        container_id = container_id,
        initial_id = initial_id,
        agent_cards = agent_cards,
        form_hidden = form_hidden,
        error_html = error_html,
        hx_post_val = hx_post_val,
        hx_target_val = hx_target_val,
        hx_include_val = hx_include_val,
        inline_prompt_gen_html = inline_prompt_gen_html
    )
}

fn render_spam_disabled_warning(spam_scan_enabled: bool, initial_disabled: bool) -> String {
    if spam_scan_enabled {
        String::new()
    } else {
        let (box_class, checkbox_attr) = if initial_disabled {
            ("opacity-40 pointer-events-none grayscale", "disabled")
        } else {
            ("", "")
        };
        format!(
            r#"
        <div class="spam-disabled-box p-3 bg-amber-500/10 border border-amber-500/30 rounded-lg text-amber-300 text-xs space-y-2 transition-all duration-200 {box_class}">
            <div class="font-semibold flex items-center gap-1.5 text-amber-400">
                <svg class="w-4 h-4 shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"/>
                </svg>
                Spam scanning is disabled in server configuration
            </div>
            <div>Channels without participant email restrictions will receive incoming emails without spam filtering.</div>
            <label class="flex items-center gap-2 cursor-pointer mt-1 font-medium text-amber-200">
                <input type="checkbox" name="confirm_spam_disabled" value="true" {checkbox_attr} class="rounded bg-slate-800 border-slate-700 text-amber-500 focus:ring-amber-500">
                <span>I am aware that spam scanning is disabled and confirm saving without participant restrictions.</span>
            </label>
        </div>
        "#
        )
    }
}

pub fn channels_page(
    company: &Company,
    app_domain_name: &str,
    channels: &[Channel],
    agents: &[Agent],
    spam_scan_enabled: bool,
) -> String {
    let list_html = channel_list_fragment(company, app_domain_name, channels, agents);
    let agents_selection_html = render_agents_selection(company.id, agents, None, "new");
    let spam_warning_html = render_spam_disabled_warning(spam_scan_enabled, true);
    let content = format!(
        r##"
        <div class="flex items-center justify-between mb-6">
            <div>
                <a href="/companies" class="text-xs text-indigo-400 hover:text-indigo-300 font-medium mb-1 inline-block">&larr; Back to Companies</a>
                <h2 class="text-2xl font-bold text-white">{company_name} Channels</h2>
                <p class="text-slate-400 text-sm mt-0.5">Manage channels for <span class="font-mono text-indigo-300">@{slug}.{app_domain_name}</span></p>
            </div>
            <button id="channel-form-toggle" type="button" aria-controls="channel-form-card" aria-expanded="false"
                onclick="const card = document.getElementById('channel-form-card'); const opening = card.classList.contains('hidden'); card.classList.toggle('hidden'); this.setAttribute('aria-expanded', opening);"
                class="px-4 py-2 bg-emerald-600 hover:bg-emerald-500 text-white text-sm font-semibold rounded-lg shadow-md shadow-emerald-600/30 transition cursor-pointer">
                Add Channel
            </button>
        </div>

        <div id="response-message" class="mb-6"></div>

        <!-- Create Channel Card -->
        <div id="channel-form-card" class="hidden bg-slate-900/70 border border-slate-700/80 rounded-xl p-5 mb-8">
            <div class="flex items-center justify-between mb-4 border-b border-slate-800 pb-3">
                <h3 class="text-md font-semibold text-white flex items-center gap-2">
                    <span class="text-emerald-400">+</span> Add New Channel
                </h3>
                <div class="flex items-center bg-slate-800/80 p-1 rounded-lg border border-slate-700/50 text-xs font-medium">
                    <button type="button" id="tab-simple-btn" onclick="showChannelFormTab('simple')"
                        class="px-3 py-1 rounded-md text-white bg-indigo-600 font-semibold transition cursor-pointer">
                        Simple
                    </button>
                    <button type="button" id="tab-advanced-btn" onclick="showChannelFormTab('advanced')"
                        class="px-3 py-1 rounded-md text-slate-400 hover:text-white transition cursor-pointer">
                        Advanced
                    </button>
                </div>
            </div>

            <!-- Simple Create Channel Form (Default) -->
            <form id="simple-channel-form" hx-post="/companies/{company_id}/channels" hx-target="#channel-list" hx-swap="innerHTML" hx-disabled-elt="find button[type='submit']" class="space-y-4" data-company-id="{company_id}"
                hx-on::after-request="if(event.detail.successful && event.detail.elt === this) {{ this.reset(); document.getElementById('channel-form-card').classList.add('hidden'); document.getElementById('channel-form-toggle').setAttribute('aria-expanded', 'false'); }}">
                <input type="hidden" name="form_mode" value="simple">
                <div>
                    <label for="simple_channel_name" class="block text-xs font-medium text-slate-300 mb-1">Channel Name</label>
                    <input type="text" id="simple_channel_name" name="name" required
                        class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-emerald-500"
                        placeholder="Inbound Email Handler">
                </div>
                <div>
                    <label for="simple_system_prompt" class="block text-xs font-medium text-slate-300 mb-1">Agent Instructions</label>
                    <textarea id="simple_system_prompt" name="system_prompt" rows="4" required
                        class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-emerald-500 font-mono text-xs"
                        placeholder="Describe the agent's role, responsibilities, rules, and tone. A complete system prompt will be generated when you create the channel."></textarea>
                </div>
                <div class="flex justify-end">
                    <button type="submit"
                        class="px-5 py-2 bg-emerald-600 hover:bg-emerald-500 text-white text-sm font-semibold rounded-lg shadow-md shadow-emerald-600/30 transition cursor-pointer flex items-center gap-2 [.htmx-request_&]:pointer-events-none [.htmx-request_&]:opacity-80">
                        <svg class="animate-spin h-4 w-4 text-white hidden [.htmx-request_&]:inline-block shrink-0" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" aria-hidden="true">
                            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                            <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                        </svg>
                        <span class="[.htmx-request_&]:hidden">Create Channel</span>
                        <span class="hidden [.htmx-request_&]:inline">Creating...</span>
                    </button>
                </div>
            </form>

            <!-- Advanced Create Channel Form (Hidden by default) -->
            <form id="advanced-channel-form" hx-post="/companies/{company_id}/channels" hx-target="#channel-list" hx-swap="innerHTML" class="hidden space-y-4" data-company-id="{company_id}"
                hx-on::after-request="if(event.detail.successful && event.detail.elt === this) {{ this.reset(); document.getElementById('channel-form-card').classList.add('hidden'); document.getElementById('channel-form-toggle').setAttribute('aria-expanded', 'false'); }}">
                <input type="hidden" name="form_mode" value="advanced">
                <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                    <div>
                        <label for="channel_name" class="block text-xs font-medium text-slate-300 mb-1">Channel Name</label>
                        <input type="text" id="channel_name" name="name" required
                            oninput="document.getElementById('channel_slug').value = this.value.toLowerCase().trim().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '')"
                            class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-emerald-500"
                            placeholder="Inbound Email Handler">
                    </div>
                    <div>
                        <label for="channel_slug" class="block text-xs font-medium text-slate-300 mb-1">Slug (@{slug}.{app_domain_name})</label>
                        <input type="text" id="channel_slug" name="slug" required
                            class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-emerald-500 font-mono"
                            placeholder="inbound-email-handler">
                    </div>
                </div>

                <div>
                    <label class="block text-xs font-medium text-slate-300 mb-1">Select Agent</label>
                    {agents_selection_html}
                </div>

                <div>
                    <label for="participant_emails" class="block text-xs font-medium text-slate-300 mb-1">Participant Emails (Optional - Defaults to Company Team)</label>
                    <input type="text" id="participant_emails" name="participant_emails" data-company-id="{company_id}" oninput="toggleSpamWarning(this)" autocomplete="off"
                        class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-emerald-500"
                        placeholder="Leave blank for Company Team, @public for open access, or comma-separated emails">
                    <p class="text-[11px] text-slate-400 mt-1">Leave blank for Company Team members. Use <code class="text-indigo-300">@public</code> to allow anyone, or specify email addresses.</p>
                </div>
                <div>
                    <a href="#" onclick="let el = this.nextElementSibling; if (el) el.classList.toggle('hidden'); return false;"
                        class="text-xs text-indigo-400 hover:text-indigo-300 font-medium cursor-pointer inline-flex items-center gap-1">
                        <span>Custom Channel Agent</span>
                        <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7"></path></svg>
                    </a>
                    <div class="hidden space-y-4 mt-3">
                        <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
                            <div>
                                <label for="channel_provider" class="block text-xs font-medium text-slate-300 mb-1">LLM Provider (Optional Override)</label>
                                <input type="text" id="channel_provider" name="provider"
                                    class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-emerald-500 font-mono"
                                    placeholder="google, openai, anthropic">
                            </div>
                            <div>
                                <label for="channel_model" class="block text-xs font-medium text-slate-300 mb-1">LLM Model (Optional Override)</label>
                                <input type="text" id="channel_model" name="model"
                                    class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-emerald-500 font-mono"
                                    placeholder="gemini-2.5-flash, gpt-4o">
                            </div>
                            <div>
                                <label for="channel_api_key" class="block text-xs font-medium text-slate-300 mb-1">LLM API Key (Optional Override)</label>
                                <input type="password" id="channel_api_key" name="api_key"
                                    class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-emerald-500 font-mono"
                                    placeholder="Overrides company key">
                            </div>
                        </div>
                        <div>
                            <label for="channel_config" class="block text-xs font-medium text-slate-300 mb-1">Channel Config (JSON, Optional)</label>
                            <textarea id="channel_config" name="channel_config" rows="3"
                                class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm font-mono placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-emerald-500"
                                placeholder='&#123; "trigger": "email", "action": "ai_reply" &#125;'></textarea>
                        </div>
                    </div>
                </div>
                {spam_warning_html}
                <div class="flex justify-end">
                    <button type="submit"
                        class="px-5 py-2 bg-emerald-600 hover:bg-emerald-500 text-white text-sm font-semibold rounded-lg shadow-md shadow-emerald-600/30 transition cursor-pointer">
                        Create Channel
                    </button>
                </div>
            </form>
        </div>

        <script>
            function showChannelFormTab(mode) {{
                const simpleForm = document.getElementById('simple-channel-form');
                const advancedForm = document.getElementById('advanced-channel-form');
                const simpleBtn = document.getElementById('tab-simple-btn');
                const advancedBtn = document.getElementById('tab-advanced-btn');
                if (mode === 'simple') {{
                    if (simpleForm) simpleForm.classList.remove('hidden');
                    if (advancedForm) advancedForm.classList.add('hidden');
                    if (simpleBtn) simpleBtn.className = 'px-3 py-1 rounded-md text-white bg-indigo-600 font-semibold transition cursor-pointer';
                    if (advancedBtn) advancedBtn.className = 'px-3 py-1 rounded-md text-slate-400 hover:text-white transition cursor-pointer';
                }} else {{
                    if (simpleForm) simpleForm.classList.add('hidden');
                    if (advancedForm) advancedForm.classList.remove('hidden');
                    if (simpleBtn) simpleBtn.className = 'px-3 py-1 rounded-md text-slate-400 hover:text-white transition cursor-pointer';
                    if (advancedBtn) advancedBtn.className = 'px-3 py-1 rounded-md text-white bg-indigo-600 font-semibold transition cursor-pointer';
                }}
            }}
        </script>

        <!-- Channels List Section -->
        <div>
            <h3 class="text-sm font-semibold uppercase tracking-wider text-slate-400 mb-3">Channels</h3>
            <div id="channel-list" class="space-y-3">
                {list_html}
            </div>
        </div>
        "##,
        company_name = company.name,
        slug = company.slug,
        app_domain_name = app_domain_name,
        company_id = company.id,
        agents_selection_html = agents_selection_html,
        list_html = list_html,
    );

    base_layout(&format!("{} Channels", company.name), &content)
}

pub fn channel_list_fragment(
    company: &Company,
    app_domain_name: &str,
    channels: &[Channel],
    agents: &[Agent],
) -> String {
    if channels.is_empty() {
        return r##"
            <div class="bg-slate-900/40 border border-dashed border-slate-700/80 rounded-xl p-8 text-center">
                <p class="text-slate-400 text-sm">No channels configured yet. Use Add Channel to create your first one.</p>
            </div>
        "##
        .to_string();
    }

    channels
        .iter()
        .map(|wf| channel_row_fragment(company, app_domain_name, wf, agents))
        .collect()
}

pub fn channel_row_fragment(
    company: &Company,
    app_domain_name: &str,
    channel: &Channel,
    agents: &[Agent],
) -> String {
    let created_at_str = channel.created_at.format("%b %d, %Y").to_string();
    let emails_str = match &channel.participant_emails {
        Some(emails) if !emails.is_empty() => emails.join(", "),
        _ => "None".to_string(),
    };
    let assigned_agents_str = match &channel.agent_ids {
        Some(ids) if !ids.is_empty() => {
            let matches: Vec<String> = agents
                .iter()
                .filter(|a| ids.contains(&a.id))
                .map(|a| format!("{} (@{})", a.name, a.slug))
                .collect();
            if matches.is_empty() {
                format!("{} agent(s)", ids.len())
            } else {
                matches.join(", ")
            }
        }
        _ => "None".to_string(),
    };
    let config_str = match &channel.channel_config {
        Some(cfg) => serde_json::to_string_pretty(cfg).unwrap_or_else(|_| cfg.to_string()),
        None => "None".to_string(),
    };
    let provider_str = channel.provider.as_deref().unwrap_or("Default (Company)");
    let model_str = channel.model.as_deref().unwrap_or("Default (Company)");
    let api_key_str = if channel.api_key.is_some() {
        "Configured (Channel Override)"
    } else {
        "Default (Company)"
    };
    let display_slug = format!("{}@{}.{}", channel.slug, company.slug, app_domain_name);

    format!(
        r##"
        <div id="channel-{channel_id}" class="bg-slate-900/80 border border-slate-700/70 rounded-xl p-4 md:p-5 flex flex-col gap-3 hover:border-slate-600 transition shadow-sm">
            <div class="flex items-center justify-between">
                <div>
                    <div class="flex items-center gap-3">
                        <h4 class="text-md font-semibold text-white">{name}</h4>
                        <span class="px-2.5 py-0.5 rounded-full text-xs font-mono bg-emerald-950/90 text-emerald-300 border border-emerald-700/50">{display_slug}</span>
                    </div>
                    <p class="text-xs text-slate-400 mt-1">Created on {created_at_str}</p>
                </div>
                <div class="flex items-center gap-2">
                    <a href="/companies/{company_id}/tasks?channel_id={channel_id}"
                        class="px-3 py-1.5 text-xs font-medium bg-amber-900/80 hover:bg-amber-800 text-amber-200 border border-amber-700/50 rounded-lg transition">
                        Tasks
                    </a>
                    <a href="/companies/{company_id}/channels/{channel_id}/simulate"
                        class="px-3 py-1.5 text-xs font-medium bg-indigo-900/80 hover:bg-indigo-800 text-indigo-200 border border-indigo-700/50 rounded-lg transition">
                        Simulate
                    </a>
                    <button hx-get="/companies/{company_id}/channels/{channel_id}/edit" hx-target="#channel-{channel_id}" hx-swap="outerHTML"
                        class="px-3 py-1.5 text-xs font-medium bg-slate-700 hover:bg-slate-600 text-slate-200 rounded-lg transition cursor-pointer">
                        Edit
                    </button>
                    <button hx-delete="/companies/{company_id}/channels/{channel_id}" hx-target="#channel-{channel_id}" hx-swap="outerHTML" hx-confirm="Are you sure you want to delete channel '{name}'?"
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
            <div class="grid grid-cols-1 md:grid-cols-3 gap-2 text-xs bg-slate-950/60 p-3 rounded-lg border border-slate-800 font-mono">
                <div>
                    <span class="text-slate-500 block font-sans text-[11px] font-semibold uppercase">Assigned Agents:</span>
                    <span class="text-indigo-300">{assigned_agents_str}</span>
                </div>
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
        channel_id = channel.id,
        name = channel.name,
        display_slug = display_slug,
        created_at_str = created_at_str,
        provider_str = provider_str,
        model_str = model_str,
        api_key_str = api_key_str,
        assigned_agents_str = assigned_agents_str,
        emails_str = emails_str,
        config_str = config_str,
    )
}

pub fn channel_edit_fragment(
    company: &Company,
    app_domain_name: &str,
    channel: &Channel,
    agents: &[Agent],
    spam_scan_enabled: bool,
) -> String {
    let emails_str = match &channel.participant_emails {
        Some(emails) => emails.join(", "),
        None => String::new(),
    };
    let config_str = match &channel.channel_config {
        Some(cfg) => serde_json::to_string_pretty(cfg).unwrap_or_else(|_| cfg.to_string()),
        None => String::new(),
    };
    let provider_val = channel.provider.as_deref().unwrap_or("");
    let model_val = channel.model.as_deref().unwrap_or("");
    let api_key_val = channel.api_key.as_deref().unwrap_or("");
    let agents_selection_html = render_agents_selection(
        company.id,
        agents,
        channel.agent_ids.as_deref(),
        &channel.id.to_string(),
    );
    let is_public = channel
        .participant_emails
        .as_ref()
        .map(|emails| {
            emails
                .iter()
                .any(|e| e.trim().eq_ignore_ascii_case("@public"))
        })
        .unwrap_or(false);
    let spam_warning_html = render_spam_disabled_warning(spam_scan_enabled, !is_public);
    let custom_config_hidden = if !provider_val.is_empty()
        || !model_val.is_empty()
        || !api_key_val.is_empty()
        || !config_str.is_empty()
    {
        ""
    } else {
        "hidden"
    };

    format!(
        r##"
        <form id="channel-{channel_id}" hx-put="/companies/{company_id}/channels/{channel_id}" hx-target="#channel-{channel_id}" hx-swap="outerHTML" data-company-id="{company_id}"
            class="bg-slate-900 border border-emerald-500/60 rounded-xl p-4 md:p-5 space-y-4 shadow-lg">
            <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                <div>
                    <label class="block text-xs font-medium text-slate-300 mb-1">Channel Name</label>
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
                <label class="block text-xs font-medium text-slate-300 mb-1">Select Agents (Multiple allowed)</label>
                {agents_selection_html}
            </div>

            <div>
                <label class="block text-xs font-medium text-slate-300 mb-1">Participant Emails (Optional - Defaults to Company Team)</label>
                <input type="text" name="participant_emails" value="{emails_str}" data-company-id="{company_id}" oninput="toggleSpamWarning(this)" autocomplete="off"
                    class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-emerald-500"
                    placeholder="Leave blank for Company Team, @public for open access, or comma-separated emails">
                <p class="text-[11px] text-slate-400 mt-1">Leave blank for Company Team members. Use <code class="text-indigo-300">@public</code> to allow anyone, or specify email addresses.</p>
            </div>
            <div>
                <a href="#" onclick="let el = this.nextElementSibling; if (el) el.classList.toggle('hidden'); return false;"
                    class="text-xs text-indigo-400 hover:text-indigo-300 font-medium cursor-pointer inline-flex items-center gap-1">
                    <span>Custom Channel Agent</span>
                    <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7"></path></svg>
                </a>
                <div class="{custom_config_hidden} space-y-4 mt-3">
                    <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
                        <div>
                            <label class="block text-xs font-medium text-slate-300 mb-1">LLM Provider (Optional Override)</label>
                            <input type="text" name="provider" value="{provider_val}"
                                class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-emerald-500 font-mono"
                                placeholder="google, openai, anthropic">
                        </div>
                        <div>
                            <label class="block text-xs font-medium text-slate-300 mb-1">LLM Model (Optional Override)</label>
                            <input type="text" name="model" value="{model_val}"
                                class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-emerald-500 font-mono"
                                placeholder="gemini-2.5-flash, gpt-4o">
                        </div>
                        <div>
                            <label class="block text-xs font-medium text-slate-300 mb-1">LLM API Key (Optional Override)</label>
                            <input type="password" name="api_key" value="{api_key_val}"
                                class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-emerald-500 font-mono"
                                placeholder="Leave empty to use Company key">
                        </div>
                    </div>
                    <div>
                        <label class="block text-xs font-medium text-slate-300 mb-1">Channel Config (JSON)</label>
                        <textarea name="channel_config" rows="3"
                            class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm font-mono focus:outline-none focus:ring-2 focus:ring-emerald-500">{config_str}</textarea>
                    </div>
                </div>
            </div>
            {spam_warning_html}
            <div class="flex items-center justify-end gap-2">
                <button type="button" hx-get="/companies/{company_id}/channels/{channel_id}/cancel" hx-target="#channel-{channel_id}" hx-swap="outerHTML"
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
        channel_id = channel.id,
        company_id = company.id,
        name = channel.name,
        slug = channel.slug,
        company_slug = company.slug,
        app_domain_name = app_domain_name,
        emails_str = emails_str,
        provider_val = provider_val,
        model_val = model_val,
        api_key_val = api_key_val,
        config_str = config_str,
        agents_selection_html = agents_selection_html,
        custom_config_hidden = custom_config_hidden,
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

    let form_container_content = if let Some(tid) =
        initial_thread_id.filter(|s| !s.trim().is_empty())
    {
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
            company_id = company.id,
            channel_id = channel.id,
            tid = tid,
        )
    } else {
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
                                    <input type="text" id="from" name="from" value="{sender_email}" data-server-sender="{sender_email}" required
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
                                        <input type="radio" name="simulation_mode" value="verify" checked onchange="this.form.elements.namedItem('from').disabled = false" class="mt-0.5 text-indigo-600 focus:ring-indigo-500">
                                        <div class="ml-2.5">
                                            <span class="block text-xs font-bold text-white">Verify</span>
                                            <span class="block text-[11px] text-slate-400 mt-0.5">Verification only (Recipient & Sender ACL check)</span>
                                        </div>
                                    </label>
                                    <label class="flex items-start p-3 bg-slate-800 border border-slate-700 rounded-lg cursor-pointer hover:border-amber-500 transition">
                                        <input type="radio" name="simulation_mode" value="run_test" onchange="const sender = this.form.elements.namedItem('from'); sender.value = sender.dataset.serverSender; sender.disabled = true" class="mt-0.5 text-amber-500 focus:ring-amber-500">
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
            company_id = company.id,
            channel_id = channel.id,
            target_recipient = target_recipient,
            sender_email = sender_email,
        )
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

pub fn resolve_llm_info(
    channel: Option<&Channel>,
    company: Option<&Company>,
) -> (String, String, String) {
    match (channel, company) {
        (Some(wf), Some(comp)) => {
            let wf_cfg = wf.channel_config.as_ref();
            let wf_llm = wf_cfg.and_then(|c| c.get("llm"));

            let provider_opt = wf
                .provider
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .or_else(|| comp.provider.as_deref().filter(|s| !s.trim().is_empty()))
                .or_else(|| {
                    wf_llm
                        .and_then(|l| l.get("provider"))
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.trim().is_empty())
                });

            let provider_name = provider_opt.unwrap_or("google").to_lowercase();
            let provider_label = provider_opt
                .map(|s| s.to_string())
                .unwrap_or_else(|| "google (default)".to_string());

            let model_opt = wf
                .model
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .or_else(|| comp.model.as_deref().filter(|s| !s.trim().is_empty()))
                .or_else(|| {
                    wf_llm
                        .and_then(|l| l.get("model"))
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.trim().is_empty())
                });

            let model_label = model_opt
                .map(|s| s.to_string())
                .unwrap_or_else(|| "gemini-2.5-flash (default)".to_string());

            let key_status = if wf
                .api_key
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .is_some()
            {
                "<span class=\"text-emerald-400 font-bold\">Configured (Channel)</span>".to_string()
            } else if comp
                .api_key
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .is_some()
            {
                "<span class=\"text-emerald-400 font-bold\">Configured (Company)</span>".to_string()
            } else if wf_llm
                .and_then(|l| l.get("api_key"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .is_some()
            {
                "<span class=\"text-emerald-400 font-bold\">Configured (Channel Config)</span>"
                    .to_string()
            } else {
                let env_vars = match provider_name.as_str() {
                    "google" | "gemini" => "GEMINI_API_KEY / GOOGLE_API_KEY",
                    "openai" => "OPENAI_API_KEY",
                    "anthropic" => "ANTHROPIC_API_KEY",
                    "groq" => "GROQ_API_KEY",
                    "mistral" => "MISTRAL_API_KEY",
                    _ => "LLM_API_KEY / API_KEY",
                };

                let has_env = match provider_name.as_str() {
                    "google" | "gemini" => {
                        std::env::var("GEMINI_API_KEY")
                            .ok()
                            .filter(|s| !s.trim().is_empty())
                            .is_some()
                            || std::env::var("GOOGLE_API_KEY")
                                .ok()
                                .filter(|s| !s.trim().is_empty())
                                .is_some()
                    }
                    "openai" => std::env::var("OPENAI_API_KEY")
                        .ok()
                        .filter(|s| !s.trim().is_empty())
                        .is_some(),
                    "anthropic" => std::env::var("ANTHROPIC_API_KEY")
                        .ok()
                        .filter(|s| !s.trim().is_empty())
                        .is_some(),
                    "groq" => std::env::var("GROQ_API_KEY")
                        .ok()
                        .filter(|s| !s.trim().is_empty())
                        .is_some(),
                    "mistral" => std::env::var("MISTRAL_API_KEY")
                        .ok()
                        .filter(|s| !s.trim().is_empty())
                        .is_some(),
                    _ => {
                        std::env::var("LLM_API_KEY")
                            .ok()
                            .filter(|s| !s.trim().is_empty())
                            .is_some()
                            || std::env::var("API_KEY")
                                .ok()
                                .filter(|s| !s.trim().is_empty())
                                .is_some()
                    }
                };

                if has_env {
                    format!("<span class=\"text-indigo-300 font-bold\">Env Var ({env_vars})</span>")
                } else {
                    format!(
                        "<span class=\"text-rose-400 font-bold\">Missing / Unset ⚠️ ({env_vars})</span>"
                    )
                }
            };

            (provider_label, model_label, key_status)
        }
        (Some(wf), None) => {
            let provider = wf.provider.as_deref().unwrap_or("google (default)");
            let model = wf.model.as_deref().unwrap_or("gemini-2.5-flash (default)");
            let key_status = if wf
                .api_key
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .is_some()
            {
                "<span class=\"text-emerald-400 font-bold\">Configured (Channel)</span>".to_string()
            } else {
                "<span class=\"text-rose-400 font-bold\">Missing / Unset ⚠️</span>".to_string()
            };
            (provider.to_string(), model.to_string(), key_status)
        }
        (None, Some(comp)) => {
            let provider = comp.provider.as_deref().unwrap_or("google (default)");
            let model = comp
                .model
                .as_deref()
                .unwrap_or("gemini-2.5-flash (default)");
            let key_status = if comp
                .api_key
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .is_some()
            {
                "<span class=\"text-emerald-400 font-bold\">Configured (Company)</span>".to_string()
            } else {
                "<span class=\"text-rose-400 font-bold\">Missing / Unset ⚠️</span>".to_string()
            };
            (provider.to_string(), model.to_string(), key_status)
        }
        _ => (
            "N/A".to_string(),
            "N/A".to_string(),
            "<span class=\"text-slate-400\">Unknown</span>".to_string(),
        ),
    }
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
    let (provider_str, model_str, api_key_status) = resolve_llm_info(channel, company);

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

pub fn channel_simulation_result_fragment(
    company_id: Uuid,
    channel_id: Uuid,
    result: &InboundEmailResult,
) -> String {
    let oob_form_swap = format!(
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
    );

    let (provider_str, model_str, api_key_status) =
        resolve_llm_info(result.channel.as_ref(), result.company.as_ref());

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
                .clone()
                .unwrap_or_else(|| "N/A".to_string())
        });

    let channel_name = result
        .channel
        .as_ref()
        .map(|w| format!("{} (/{})", w.name, w.slug))
        .unwrap_or_else(|| {
            result
                .channel_slug
                .clone()
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

    let body_fragment = format!(
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
        to = result.email.to,
        from = result.email.from,
        company_name = company_name,
        channel_name = channel_name,
        subject_str = subject_str,
        body_str = body_str,
        channel_config_str = channel_config_str,
    );

    format!("{oob_form_swap}\n{body_fragment}")
}

pub fn channel_simulation_execution_result_fragment(
    company_id: Uuid,
    channel_id: Uuid,
    sim_res: &SimulationExecutionResult,
    messages: &[Message],
    tasks: &[BackgroundTask],
) -> String {
    let ingest = &sim_res.ingest_result;
    let (provider_str, model_str, api_key_status) =
        resolve_llm_info(ingest.channel.as_ref(), ingest.company.as_ref());

    let thread_id_str = ingest
        .thread
        .as_ref()
        .map(|t| t.id.to_string())
        .unwrap_or_else(|| "N/A".to_string());

    let oob_form_swap = format!(
        r##"
        <div id="simulation-form-container" hx-swap-oob="outerHTML">
            <div class="bg-slate-900/70 border border-slate-700/80 rounded-xl p-4 mb-6 shadow-md flex items-center justify-between">
                <div class="flex items-center gap-2 text-sm text-slate-300 font-medium">
                    <span class="inline-block w-2.5 h-2.5 rounded-full bg-emerald-400 animate-pulse"></span>
                    <span>Simulation Thread Active</span>
                    <span class="text-xs text-slate-400 font-mono">({thread_id_str})</span>
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
        thread_id_str = thread_id_str,
    );

    if !ingest.accepted {
        let reason = ingest
            .reason
            .as_deref()
            .unwrap_or("Ingestion failed / unauthorized");
        return format!(
            r##"
            {oob_form_swap}
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
            "##,
            oob_form_swap = oob_form_swap,
            reason = reason,
            provider_str = provider_str,
            model_str = model_str,
            api_key_status = api_key_status,
        );
    }

    let agent_exec = sim_res.agent_execution.as_ref();
    let agent_response_text = agent_exec
        .map(|a| a.agent_response.as_str())
        .unwrap_or("(No response generated)");

    let agent_lower = agent_response_text.to_lowercase();
    let is_agent_error = agent_lower.contains("failed")
        || agent_lower.contains("error")
        || agent_lower.contains("missing");

    let mode_label = match sim_res.simulation_mode {
        SimulationMode::Verify => "Verify",
        SimulationMode::RunTest => "Run_Test (Dry-Run)",
        SimulationMode::Run => "Run (Live)",
    };

    let status_banner = if is_agent_error {
        r#"<div class="p-4 rounded-xl bg-rose-950/80 border border-rose-600/60 text-rose-200 text-sm font-semibold flex items-center gap-2">
            <span class="text-rose-400 text-lg">✕</span>
            <span>Channel Simulation Execution Failed! (Agent Error)</span>
        </div>"#
    } else {
        match sim_res.simulation_mode {
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
    };

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

    let channel_name = ingest
        .channel
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

    let outbound_msg_id = agent_exec
        .and_then(|a| a.outbound_message_id.clone())
        .unwrap_or_else(|| "N/A".to_string());

    let email_status = if is_agent_error {
        "<span class=\"text-rose-400 font-bold\">Failed (Execution Error)</span>"
    } else if sim_res.simulation_mode == SimulationMode::RunTest {
        "<span class=\"text-amber-400 font-bold\">Skipped (Run_Test Dry-Run)</span>"
    } else if sim_res.simulation_mode == SimulationMode::Run {
        "<span class=\"text-emerald-400 font-bold\">Dispatched via SMTP</span>"
    } else {
        "<span class=\"text-slate-400 font-bold\">None (Verify Only)</span>"
    };

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
    let agent_response_html = render_markdown(agent_response_text);

    let simulation_token_meter_html = if let Some(ref tu) =
        agent_exec.and_then(|a| a.token_usage.as_ref())
    {
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
    } else {
        String::new()
    };

    let simulation_metadata_html = if let Some(meta) = agent_exec.and_then(|a| a.metadata.as_ref())
    {
        let meta_pretty = serde_json::to_string_pretty(meta).unwrap_or_else(|_| meta.to_string());
        let finish_reason = meta
            .get("finish_reason")
            .or_else(|| meta.get("stop_reason"))
            .and_then(|v| v.as_str());

        let finish_badge = if let Some(reason) = finish_reason {
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
        } else {
            String::new()
        };

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
    } else {
        String::new()
    };

    let exec_details = format!(
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
                {simulation_token_meter_html}
                {simulation_metadata_html}
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
        "##,
        mode_label = mode_label,
        email_status = email_status,
        provider_str = provider_str,
        model_str = model_str,
        api_key_status = api_key_status,
        simulation_token_meter_html = simulation_token_meter_html,
        to_str = to_str,
        from_str = from_str,
        company_name = company_name,
        channel_name = channel_name,
        thread_id_str = thread_id_str,
        inbound_msg_id = inbound_msg_id,
        outbound_msg_id = outbound_msg_id,
        subject_str = subject_str,
        text_body_str = text_body_str,
        response_label = response_label,
        response_style = response_style,
        agent_response_html = agent_response_html,
    );

    let resolved_config = crate::services::agent_runner::ResolvedAgentParams::new(
        ingest.company.as_ref(),
        ingest.channel.as_ref(),
        None,
    )
    .map(|p| p.config().clone())
    .ok()
    .or_else(|| {
        ingest
            .channel
            .as_ref()
            .and_then(|w| w.channel_config.clone())
    })
    .unwrap_or_else(|| serde_json::json!({}));

    let thread_id_opt = ingest.thread.as_ref().map(|t| t.id);

    let messages_section = if messages.is_empty() {
        String::new()
    } else {
        let mut msgs_html = String::new();
        for msg in messages {
            let is_agent =
                msg.role == MessageRole::Agent || msg.direction == MessageDirection::Outbound;
            let created_at_fmt = msg.created_at.format("%Y-%m-%d %H:%M:%S UTC").to_string();

            let matched_task = find_task_for_message(msg, tasks, ingest.task_id, thread_id_opt);

            let msg_task_payload = match matched_task {
                Some(t) => t.payload.clone(),
                None => {
                    if is_agent {
                        serde_json::json!({
                            "task_type": "email_agent_dispatch",
                            "execution_parameters": {
                                "provider": provider_str,
                                "model": model_str,
                                "prompt": parsed.map(|p| p.prompt_text.as_str()).unwrap_or(""),
                                "config": resolved_config.clone(),
                                "executed_at": created_at_fmt
                            },
                            "execution_result": {
                                "response": msg.clean_text_body,
                                "outbound_message_id": msg.message_id
                            },
                            "channel": ingest.channel,
                            "company": ingest.company
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
                            "channel": ingest.channel,
                            "company": ingest.company
                        })
                    }
                }
            };
            let params_html = render_message_task_parameters_html(&msg_task_payload);

            if is_agent {
                let body_html = render_markdown(&msg.clean_text_body);
                msgs_html.push_str(&format!(
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
                    created_at = created_at_fmt,
                    msg_id = msg.message_id,
                    body = body_html,
                    markdown_styles = MARKDOWN_CONTENT_STYLES,
                    params_html = params_html,
                ));
            } else {
                msgs_html.push_str(&format!(
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
                    created_at = created_at_fmt,
                    sender = msg.sender,
                    msg_id = msg.message_id,
                    subject = msg.subject,
                    body = msg.clean_text_body,
                    params_html = params_html,
                ));
            }
        }

        let msg_count = messages.len();
        let label = if msg_count == 1 {
            "message"
        } else {
            "messages"
        };
        format!(
            r##"
            <div class="bg-slate-900/80 border border-slate-700/80 rounded-xl p-5 space-y-4 shadow-lg">
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
            "##,
            msg_count = msg_count,
            label = label,
            thread_id_str = thread_id_str,
            msgs_html = msgs_html,
        )
    };

    let last_msg_id = messages
        .last()
        .map(|m| m.message_id.clone())
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
                .map(|m| m.message_id.clone())
        })
        .unwrap_or_default();

    let reply_subject = if subject_str.to_lowercase().starts_with("re:") {
        subject_str.to_string()
    } else {
        format!("Re: {}", subject_str)
    };

    let run_test_checked = if sim_res.simulation_mode == SimulationMode::RunTest {
        "checked"
    } else {
        ""
    };
    let run_checked = if sim_res.simulation_mode == SimulationMode::Run {
        "checked"
    } else {
        ""
    };

    let reply_form = format!(
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
                        <input type="text" id="to_reply" name="to" value="{to_str}" required
                            class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm font-mono focus:outline-none focus:ring-2 focus:ring-indigo-500">
                    </div>
                    <div>
                        <label for="from_reply" class="block text-xs font-medium text-slate-300 mb-1">From (Sender Address)</label>
                        <input type="text" id="from_reply" name="from" value="{from_str}" disabled
                            class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm font-mono opacity-60 cursor-not-allowed">
                    </div>
                </div>

                <div>
                    <label for="subject_reply" class="block text-xs font-medium text-slate-300 mb-1">Subject</label>
                    <input type="text" id="subject_reply" name="subject" value="{reply_subject}" required
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
                            <input type="radio" name="simulation_mode" value="run_test" {run_test_checked} class="mt-0.5 text-amber-500 focus:ring-amber-500">
                            <div class="ml-2.5">
                                <span class="block text-xs font-bold text-amber-300">Run_Test</span>
                                <span class="block text-[11px] text-slate-400 mt-0.5">Execute full channel & agent, skip email dispatch</span>
                            </div>
                        </label>
                        <label class="flex items-start p-2.5 bg-slate-800 border border-slate-700 rounded-lg cursor-pointer hover:border-emerald-500 transition">
                            <input type="radio" name="simulation_mode" value="run" {run_checked} class="mt-0.5 text-emerald-500 focus:ring-emerald-500">
                            <div class="ml-2.5">
                                <span class="block text-xs font-bold text-emerald-400">Run</span>
                                <span class="block text-[11px] text-slate-400 mt-0.5">Live execution with full AI agent & outbound SMTP send</span>
                            </div>
                        </label>
                    </div>
                </div>

                <div class="flex justify-end pt-1">
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
        "##,
        company_id = company_id,
        channel_id = channel_id,
        thread_id_str = thread_id_str,
        last_msg_id = last_msg_id,
        to_str = to_str,
        from_str = from_str,
        reply_subject = reply_subject,
        run_test_checked = run_test_checked,
        run_checked = run_checked,
    );

    format!(
        "{oob_form_swap}\n<div class=\"space-y-6\">\n{status_banner}\n{exec_details}\n{messages_section}\n{reply_form}\n</div>"
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
    let company_id = company.id;
    let channel_id = channel.id;
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
        .unwrap_or_else(|| "sender@example.com".to_string());

    let oob_form_swap = format!(
        r##"
        <div id="simulation-form-container" hx-swap-oob="outerHTML">
            <div class="bg-slate-900/70 border border-slate-700/80 rounded-xl p-4 mb-6 shadow-md flex items-center justify-between">
                <div class="flex items-center gap-2 text-sm text-slate-300 font-medium">
                    <span class="inline-block w-2.5 h-2.5 rounded-full bg-emerald-400 animate-pulse"></span>
                    <span>Thread Loaded & Active</span>
                    <span class="text-xs text-slate-400 font-mono">({thread_id_str})</span>
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
        thread_id_str = thread_id_str,
    );

    let created_at_fmt = thread
        .created_at
        .format("%Y-%m-%d %H:%M:%S UTC")
        .to_string();
    let updated_at_fmt = thread
        .updated_at
        .format("%Y-%m-%d %H:%M:%S UTC")
        .to_string();
    let participants_str = if thread.participant_emails.is_empty() {
        "None recorded".to_string()
    } else {
        thread.participant_emails.join(", ")
    };

    let overview_card = format!(
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
        thread_id_str = thread_id_str,
        subject = thread.subject,
        target_recipient = target_recipient,
        participants_str = participants_str,
        msg_count = messages.len(),
        created_at_fmt = created_at_fmt,
        updated_at_fmt = updated_at_fmt,
    );

    let (provider_str, model_str, _api_key_status) = resolve_llm_info(Some(channel), Some(company));

    let resolved_config =
        crate::services::agent_runner::ResolvedAgentParams::new(Some(company), Some(channel), None)
            .map(|p| p.config().clone())
            .ok()
            .or_else(|| channel.channel_config.clone())
            .unwrap_or_else(|| serde_json::json!({}));

    let messages_section = if messages.is_empty() {
        r#"<div class="bg-slate-900/80 border border-slate-700/80 rounded-xl p-5 shadow-lg text-slate-400 text-xs text-center mb-6">No messages recorded in this thread yet.</div>"#.to_string()
    } else {
        let mut msgs_html = String::new();
        for msg in messages {
            let is_agent =
                msg.role == MessageRole::Agent || msg.direction == MessageDirection::Outbound;
            let msg_created_at = msg.created_at.format("%Y-%m-%d %H:%M:%S UTC").to_string();

            let matched_task = find_task_for_message(msg, tasks, None, Some(thread.id));

            let msg_task_payload = match matched_task {
                Some(t) => t.payload.clone(),
                None => {
                    if is_agent {
                        serde_json::json!({
                            "task_type": "email_agent_dispatch",
                            "execution_parameters": {
                                "provider": provider_str,
                                "model": model_str,
                                "prompt": msg.clean_text_body,
                                "config": resolved_config.clone(),
                                "executed_at": msg_created_at
                            },
                            "execution_result": {
                                "response": msg.clean_text_body,
                                "outbound_message_id": msg.message_id
                            },
                            "channel": channel,
                            "company": company
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
                            "channel": channel,
                            "company": company
                        })
                    }
                }
            };
            let params_html = render_message_task_parameters_html(&msg_task_payload);

            if is_agent {
                let body_html = render_markdown(&msg.clean_text_body);
                msgs_html.push_str(&format!(
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
                    created_at = msg_created_at,
                    msg_id = msg.message_id,
                    body = body_html,
                    markdown_styles = MARKDOWN_CONTENT_STYLES,
                    params_html = params_html,
                ));
            } else {
                msgs_html.push_str(&format!(
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
                    created_at = msg_created_at,
                    sender = msg.sender,
                    msg_id = msg.message_id,
                    subject = msg.subject,
                    body = msg.clean_text_body,
                    params_html = params_html,
                ));
            }
        }

        let msg_count = messages.len();
        let label = if msg_count == 1 {
            "message"
        } else {
            "messages"
        };
        format!(
            r##"
            <div class="bg-slate-900/80 border border-slate-700/80 rounded-xl p-5 space-y-4 shadow-lg mb-6">
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
            "##,
            msg_count = msg_count,
            label = label,
            thread_id_str = thread_id_str,
            msgs_html = msgs_html,
        )
    };

    let last_msg_id = messages
        .last()
        .map(|m| m.message_id.clone())
        .unwrap_or_default();

    let reply_subject = if thread.subject.to_lowercase().starts_with("re:") {
        thread.subject.clone()
    } else {
        format!("Re: {}", thread.subject)
    };

    let reply_form = format!(
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
                        <input type="text" id="to_reply" name="to" value="{target_recipient}" required
                            class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm font-mono focus:outline-none focus:ring-2 focus:ring-indigo-500">
                    </div>
                    <div>
                        <label for="from_reply" class="block text-xs font-medium text-slate-300 mb-1">From (Sender Address)</label>
                        <input type="text" id="from_reply" name="from" value="{default_sender}" disabled
                            class="w-full px-3.5 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white text-sm font-mono opacity-60 cursor-not-allowed">
                    </div>
                </div>

                <div>
                    <label for="subject_reply" class="block text-xs font-medium text-slate-300 mb-1">Subject</label>
                    <input type="text" id="subject_reply" name="subject" value="{reply_subject}" required
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
                            <input type="radio" name="simulation_mode" value="run_test" checked class="mt-0.5 text-amber-500 focus:ring-amber-500">
                            <div class="ml-2.5">
                                <span class="block text-xs font-bold text-amber-300">Run_Test</span>
                                <span class="block text-[11px] text-slate-400 mt-0.5">Execute full channel & agent, skip email dispatch</span>
                            </div>
                        </label>
                        <label class="flex items-start p-2.5 bg-slate-800 border border-slate-700 rounded-lg cursor-pointer hover:border-emerald-500 transition">
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
        "##,
        company_id = company_id,
        channel_id = channel_id,
        thread_id_str = thread_id_str,
        last_msg_id = last_msg_id,
        target_recipient = target_recipient,
        default_sender = default_sender,
        reply_subject = reply_subject,
    );

    if include_oob {
        format!("{oob_form_swap}\n{overview_card}\n{messages_section}\n{reply_form}")
    } else {
        format!("{overview_card}\n{messages_section}\n{reply_form}")
    }
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

pub fn company_tasks_page(
    company: &Company,
    channels: &[Channel],
    tasks: &[BackgroundTask],
    current_wf: Option<Uuid>,
    current_status: Option<TaskStatus>,
    sort_asc: bool,
) -> String {
    let task_list_html = task_list_fragment(company.id, tasks);

    let (total_prompt_tokens, total_completion_tokens, total_tokens_meter) = tasks
        .iter()
        .filter_map(|t| t.token_usage())
        .fold((0, 0, 0), |(p_acc, c_acc, t_acc), tu| {
            (
                p_acc + tu.prompt_tokens,
                c_acc + tu.completion_tokens,
                t_acc + tu.total_tokens,
            )
        });

    let mut wf_options = String::from("<option value=\"\">All Channels</option>");
    for wf in channels {
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

        <!-- Token Meter Summary Card -->
        <div class="bg-slate-900/80 border border-indigo-900/60 rounded-xl p-4 mb-6 flex flex-wrap items-center justify-between gap-4 shadow-sm">
            <div class="flex items-center gap-3">
                <div class="p-2.5 bg-indigo-950/80 border border-indigo-700/50 rounded-lg text-indigo-400 text-lg">
                    📊
                </div>
                <div>
                    <h4 class="text-xs font-semibold uppercase tracking-wider text-slate-300">Token Meter Summary</h4>
                    <p class="text-xs text-slate-400">Total tokens consumed across filtered task executions</p>
                </div>
            </div>
            <div class="flex items-center gap-3 text-xs font-mono">
                <div class="bg-slate-950/80 px-3 py-1.5 rounded-lg border border-slate-800">
                    <span class="text-slate-400">Prompt Tokens:</span>
                    <span class="text-indigo-300 font-bold ml-1.5">{total_prompt_tokens}</span>
                </div>
                <div class="bg-slate-950/80 px-3 py-1.5 rounded-lg border border-slate-800">
                    <span class="text-slate-400">Completion Tokens:</span>
                    <span class="text-indigo-300 font-bold ml-1.5">{total_completion_tokens}</span>
                </div>
                <div class="bg-indigo-950/90 px-3.5 py-1.5 rounded-lg border border-indigo-700/80">
                    <span class="text-indigo-200 font-semibold">Total Tokens:</span>
                    <span class="text-white font-extrabold text-sm ml-1.5">{total_tokens_meter}</span>
                </div>
            </div>
        </div>

        <!-- Filter & Sort Bar -->
        <div class="bg-slate-900/70 border border-slate-700/80 rounded-xl p-4 mb-6">
            <form hx-get="/companies/{company_id}/tasks/filter" hx-target="#task-list" hx-swap="innerHTML" class="grid grid-cols-1 md:grid-cols-3 gap-4">
                <div>
                    <label class="block text-xs font-medium text-slate-300 mb-1">Filter by Channel</label>
                    <select name="channel_id" onchange="this.form.requestSubmit()"
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

fn sanitize_json_payload(value: &serde_json::Value) -> serde_json::Value {
    let mut cloned = value.clone();
    sanitize_json_mut(&mut cloned);
    cloned
}

fn sanitize_json_mut(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                if k.eq_ignore_ascii_case("api_key")
                    || k.eq_ignore_ascii_case("apikey")
                    || k.eq_ignore_ascii_case("secret")
                {
                    if let serde_json::Value::String(s) = v {
                        if !s.is_empty() {
                            *v = serde_json::Value::String("***masked***".to_string());
                        }
                    }
                } else {
                    sanitize_json_mut(v);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                sanitize_json_mut(v);
            }
        }
        _ => {}
    }
}

pub fn find_task_for_message<'a>(
    msg: &Message,
    tasks: &'a [BackgroundTask],
    preferred_task_id: Option<Uuid>,
    thread_id: Option<Uuid>,
) -> Option<&'a BackgroundTask> {
    let is_agent = msg.role == MessageRole::Agent || msg.direction == MessageDirection::Outbound;

    for task in tasks {
        if let Some(tid) = thread_id {
            if task.thread_id.is_some() && task.thread_id != Some(tid) {
                continue;
            }
        }

        let payload = &task.payload;

        if is_agent {
            if let Some(outbound_id) = payload
                .get("execution_result")
                .and_then(|r| r.get("outbound_message_id"))
                .and_then(|v| v.as_str())
                .or_else(|| payload.get("outbound_message_id").and_then(|v| v.as_str()))
            {
                if outbound_id == msg.message_id {
                    return Some(task);
                }
            }

            if let Some(resp) = payload
                .get("execution_result")
                .and_then(|r| r.get("response"))
                .and_then(|v| v.as_str())
                .or_else(|| payload.get("response").and_then(|v| v.as_str()))
            {
                if !resp.is_empty() && resp == msg.clean_text_body {
                    return Some(task);
                }
            }
        } else {
            if let Some(inbound_msg_id) = payload
                .get("inbound_message")
                .and_then(|m| m.get("message_id"))
                .and_then(|v| v.as_str())
                .or_else(|| {
                    payload
                        .get("parsed_email")
                        .and_then(|p| p.get("message_id"))
                        .and_then(|v| v.as_str())
                })
                .or_else(|| payload.get("inbound_message_id").and_then(|v| v.as_str()))
            {
                if inbound_msg_id == msg.message_id {
                    return Some(task);
                }
            }

            if let Some(inbound_id_str) = payload
                .get("inbound_message")
                .and_then(|m| m.get("id"))
                .and_then(|v| v.as_str())
            {
                if inbound_id_str == msg.id.to_string() {
                    return Some(task);
                }
            }
        }
    }

    if let Some(pref_id) = preferred_task_id {
        if let Some(task) = tasks.iter().find(|t| t.id == pref_id) {
            return Some(task);
        }
    }

    if let Some(tid) = thread_id {
        let thread_tasks: Vec<&BackgroundTask> =
            tasks.iter().filter(|t| t.thread_id == Some(tid)).collect();

        if thread_tasks.len() == 1 {
            return Some(thread_tasks[0]);
        } else if !thread_tasks.is_empty() {
            let closest = thread_tasks
                .iter()
                .min_by_key(|t| (t.created_at - msg.created_at).num_seconds().abs());
            if let Some(t) = closest {
                return Some(t);
            }
        }
    }

    tasks.first()
}

pub fn render_message_task_parameters_html(payload: &serde_json::Value) -> String {
    let sanitized_payload = sanitize_json_payload(payload);
    let payload_str =
        serde_json::to_string_pretty(&sanitized_payload).unwrap_or_else(|_| payload.to_string());

    let mut badges = Vec::new();
    if let Some(exec_params) = sanitized_payload.get("execution_parameters") {
        if let Some(p) = exec_params.get("provider").and_then(|v| v.as_str()) {
            badges.push(format!(r#"<span class="px-2 py-0.5 rounded text-[11px] font-mono bg-indigo-950/80 text-indigo-300 border border-indigo-800/50">Provider: {}</span>"#, p));
        }
        if let Some(m) = exec_params.get("model").and_then(|v| v.as_str()) {
            badges.push(format!(r#"<span class="px-2 py-0.5 rounded text-[11px] font-mono bg-indigo-950/80 text-indigo-300 border border-indigo-800/50">Model: {}</span>"#, m));
        }
        if let Some(a) = exec_params.get("agent_name").and_then(|v| v.as_str()) {
            badges.push(format!(r#"<span class="px-2 py-0.5 rounded text-[11px] font-mono bg-purple-950/80 text-purple-300 border border-purple-800/50">Agent: {}</span>"#, a));
        }
    } else {
        if let Some(p) = sanitized_payload
            .get("channel")
            .and_then(|w| w.get("provider"))
            .and_then(|v| v.as_str())
            .or_else(|| {
                sanitized_payload
                    .get("company")
                    .and_then(|c| c.get("provider"))
                    .and_then(|v| v.as_str())
            })
        {
            badges.push(format!(r#"<span class="px-2 py-0.5 rounded text-[11px] font-mono bg-indigo-950/80 text-indigo-300 border border-indigo-800/50">Provider: {}</span>"#, p));
        }
        if let Some(m) = sanitized_payload
            .get("channel")
            .and_then(|w| w.get("model"))
            .and_then(|v| v.as_str())
            .or_else(|| {
                sanitized_payload
                    .get("company")
                    .and_then(|c| c.get("model"))
                    .and_then(|v| v.as_str())
            })
        {
            badges.push(format!(r#"<span class="px-2 py-0.5 rounded text-[11px] font-mono bg-indigo-950/80 text-indigo-300 border border-indigo-800/50">Model: {}</span>"#, m));
        }
    }

    if let Some(parsed) = sanitized_payload.get("parsed_email") {
        if let Some(s) = parsed.get("sender").and_then(|v| v.as_str()) {
            badges.push(format!(r#"<span class="px-2 py-0.5 rounded text-[11px] font-mono bg-slate-800 text-slate-300 border border-slate-700">Sender: {}</span>"#, s));
        }
        if let Some(subj) = parsed.get("subject").and_then(|v| v.as_str()) {
            badges.push(format!(r#"<span class="px-2 py-0.5 rounded text-[11px] font-mono bg-slate-800 text-slate-300 border border-slate-700">Subject: {}</span>"#, subj));
        }
    }

    let has_config = sanitized_payload
        .get("execution_parameters")
        .and_then(|e| e.get("config"))
        .map_or(false, |c| !c.is_null() && c != &serde_json::json!({}))
        || sanitized_payload
            .get("channel")
            .and_then(|w| w.get("channel_config"))
            .map_or(false, |c| !c.is_null() && c != &serde_json::json!({}));
    if has_config {
        badges.push(r#"<span class="px-2 py-0.5 rounded text-[11px] font-mono bg-emerald-950/80 text-emerald-300 border border-emerald-800/50">Config: Present</span>"#.to_string());
    }

    if let Some(tu) = sanitized_payload
        .get("execution_result")
        .and_then(|r| r.get("token_usage"))
        .or_else(|| sanitized_payload.get("token_usage"))
    {
        if let (Some(prompt), Some(comp), Some(total)) = (
            tu.get("prompt_tokens").and_then(|v| v.as_u64()),
            tu.get("completion_tokens").and_then(|v| v.as_u64()),
            tu.get("total_tokens").and_then(|v| v.as_u64()),
        ) {
            badges.push(format!(
                r#"<span class="px-2 py-0.5 rounded text-[11px] font-mono bg-indigo-950/90 text-indigo-200 border border-indigo-700/60 font-semibold">📊 Token Meter: {} tokens (Prompt: {} | Completion: {})</span>"#,
                total, prompt, comp
            ));
        }
    }

    let meta_val = sanitized_payload
        .get("execution_result")
        .and_then(|r| r.get("metadata"))
        .or_else(|| sanitized_payload.get("metadata"));

    if let Some(m) = meta_val {
        if let Some(diagnostics) = m.get("execution_diagnostics") {
            if let Some(duration_ms) = diagnostics.get("duration_ms").and_then(|v| v.as_u64()) {
                badges.push(format!(r#"<span class="px-2 py-0.5 rounded text-[11px] font-mono bg-cyan-950/80 text-cyan-300 border border-cyan-800/60">Duration: {} ms</span>"#, duration_ms));
            }
            if let Some(source) = diagnostics
                .get("token_usage_source")
                .and_then(|v| v.as_str())
            {
                badges.push(format!(r#"<span class="px-2 py-0.5 rounded text-[11px] font-mono bg-slate-800 text-slate-300 border border-slate-700">Token Usage: {}</span>"#, source));
            }
            if let Some(response_chars) = diagnostics
                .get("response_characters")
                .and_then(|v| v.as_u64())
            {
                badges.push(format!(r#"<span class="px-2 py-0.5 rounded text-[11px] font-mono bg-slate-800 text-slate-300 border border-slate-700">Response: {} chars</span>"#, response_chars));
            }
            if let Some(tool_calls) = diagnostics
                .get("tool_call_count")
                .and_then(|v| v.as_u64())
                .filter(|count| *count > 0)
            {
                badges.push(format!(r#"<span class="px-2 py-0.5 rounded text-[11px] font-mono bg-purple-950/80 text-purple-300 border border-purple-800/60">Tool Calls: {}</span>"#, tool_calls));
            }
        }

        if let Some(summary) = m.get("observability").and_then(|v| v.get("summary")) {
            if let (Some(events), Some(llm_calls)) = (
                summary.get("total_events").and_then(|v| v.as_u64()),
                summary.get("total_llm_calls").and_then(|v| v.as_u64()),
            ) {
                badges.push(format!(r#"<span class="px-2 py-0.5 rounded text-[11px] font-mono bg-teal-950/80 text-teal-300 border border-teal-800/60">Observed: {} events / {} LLM calls</span>"#, events, llm_calls));
            }
        }

        let finish_reason = m
            .get("finish_reason")
            .or_else(|| m.get("stop_reason"))
            .and_then(|v| v.as_str());

        if let Some(reason) = finish_reason {
            let (badge_style, reason_label) = match reason {
                "length" | "max_tokens" => (
                    r#"bg-amber-950/90 text-amber-300 border border-amber-700/80 font-bold"#,
                    format!("Finish Reason: {} (TRUNCATED MID-SENTENCE)", reason),
                ),
                "stop" | "end_turn" => (
                    r#"bg-emerald-950/90 text-emerald-300 border border-emerald-700/80 font-semibold"#,
                    format!("Finish Reason: {}", reason),
                ),
                other => (
                    r#"bg-slate-800 text-slate-300 border border-slate-700 font-semibold"#,
                    format!("Finish Reason: {}", other),
                ),
            };
            badges.push(format!(
                r#"<span class="px-2 py-0.5 rounded text-[11px] font-mono {}">🏁 {}</span>"#,
                badge_style, reason_label
            ));
        } else {
            badges.push(r#"<span class="px-2 py-0.5 rounded text-[11px] font-mono bg-purple-950/80 text-purple-300 border border-purple-800/50">Metadata: Present</span>"#.to_string());
        }
    }

    let summary_badges_html = if !badges.is_empty() {
        format!(
            r#"<div class="flex flex-wrap gap-1.5 mb-2">{}</div>"#,
            badges.join("")
        )
    } else {
        String::new()
    };

    format!(
        r##"
        <details class="mt-3 border-t border-slate-800/80 pt-3 group">
            <summary class="cursor-pointer text-xs font-semibold text-slate-400 hover:text-indigo-300 transition flex items-center gap-1.5 select-none">
                <span class="text-indigo-400 font-mono text-[11px] group-open:rotate-90 transition-transform">►</span>
                <span>Task Execution Parameters</span>
            </summary>
            <div class="mt-2.5">
                {summary_badges_html}
                <pre class="bg-slate-950 p-3 rounded-lg text-emerald-300 font-mono text-[11px] border border-slate-800/80 overflow-x-auto whitespace-pre-wrap max-h-96">{payload_str}</pre>
            </div>
        </details>
        "##,
        summary_badges_html = summary_badges_html,
        payload_str = payload_str,
    )
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
        TaskStatus::PendingApproval => {
            r#"<span class="px-2.5 py-0.5 rounded-full text-xs font-semibold bg-sky-950 text-sky-300 border border-sky-700/50">⏳ Awaiting Approval</span>"#
        }
        TaskStatus::WaitingForThirdPartyReply => {
            r#"<span class="px-2.5 py-0.5 rounded-full text-xs font-semibold bg-cyan-950 text-cyan-300 border border-cyan-700/50">⏳ Awaiting 3rd Party Reply</span>"#
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

    let simulation_link = match task.thread_id {
        Some(tid) => format!(
            r##"<a href="/companies/{company_id}/channels/{channel_id}/simulate?thread_id={tid}"
                class="px-3 py-1.5 text-xs font-semibold bg-indigo-600/90 hover:bg-indigo-500 text-white rounded-lg transition flex items-center gap-1 shadow-sm whitespace-nowrap">
                <span>⚡ Open Simulation</span>
            </a>"##,
            company_id = company_id,
            channel_id = task.channel_id,
            tid = tid
        ),
        None => String::new(),
    };

    let thread_info = match task.thread_id {
        Some(tid) => format!(
            r##" • Thread: <a href="/companies/{company_id}/channels/{channel_id}/simulate?thread_id={tid}" class="font-mono text-emerald-400 hover:text-emerald-300 underline font-medium">{tid}</a>"##,
            company_id = company_id,
            channel_id = task.channel_id,
            tid = tid
        ),
        None => String::new(),
    };

    let error_html = match &task.last_error {
        Some(err) if !err.is_empty() => format!(
            r##"<div class="mt-2 text-xs font-mono bg-slate-950/80 p-2 rounded border border-rose-900/50 text-rose-300">Error: {err}</div>"##
        ),
        _ => String::new(),
    };

    let token_meter_badge = if let Some(tu) = task.token_usage() {
        format!(
            r##"<div class="mt-2 inline-flex items-center gap-2 px-2.5 py-1 rounded-md bg-indigo-950/80 border border-indigo-800/70 text-indigo-300 font-mono text-xs shadow-sm">
                <span class="text-indigo-400 font-semibold">📊 Token Meter:</span>
                <span class="text-white font-bold">{} total</span>
                <span class="text-slate-400 text-[11px]">(Prompt: {} • Completion: {})</span>
            </div>"##,
            tu.total_tokens, tu.prompt_tokens, tu.completion_tokens
        )
    } else {
        String::new()
    };

    let parameters_html = render_message_task_parameters_html(&task.payload);

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
                    <p class="text-xs text-slate-400 mt-1">Type: <span class="font-mono text-indigo-300">{task_type}</span> • Enqueued {created_at_str}{thread_info}</p>
                    {token_meter_badge}
                </div>
                <div class="flex items-center gap-2">
                    {simulation_link}
                    {action_button}
                </div>
            </div>
            {error_html}
            {parameters_html}
        </div>
        "##,
        task_id = task.id,
        status_badge = status_badge,
        retry_count = task.retry_count,
        max_retries = task.max_retries,
        task_type = task.task_type,
        created_at_str = created_at_str,
        thread_info = thread_info,
        token_meter_badge = token_meter_badge,
        simulation_link = simulation_link,
        action_button = action_button,
        error_html = error_html,
        parameters_html = parameters_html,
    )
}

pub fn approval_result_page(title: &str, approval: &HumanApproval, message: &str) -> String {
    let status_badge = match approval.status {
        ApprovalStatus::Approved => {
            r#"<span class="px-3 py-1 text-xs font-semibold rounded-full bg-emerald-500/20 text-emerald-300 border border-emerald-500/30">✓ Approved</span>"#
        }
        ApprovalStatus::Rejected => {
            r#"<span class="px-3 py-1 text-xs font-semibold rounded-full bg-rose-500/20 text-rose-300 border border-rose-500/30">✗ Rejected</span>"#
        }
        ApprovalStatus::Expired => {
            r#"<span class="px-3 py-1 text-xs font-semibold rounded-full bg-amber-500/20 text-amber-300 border border-amber-500/30">⏱ Expired</span>"#
        }
        ApprovalStatus::Pending => {
            r#"<span class="px-3 py-1 text-xs font-semibold rounded-full bg-sky-500/20 text-sky-300 border border-sky-500/30">⏳ Pending</span>"#
        }
    };

    let content = format!(
        r##"
        <div class="max-w-xl mx-auto py-8 text-center">
            <div class="mb-6 flex justify-center">{status_badge}</div>
            <h1 class="text-2xl font-extrabold text-white mb-4">{title}</h1>
            <p class="text-slate-300 text-base leading-relaxed mb-6">{message}</p>

            <div class="bg-slate-900/60 rounded-xl p-5 border border-slate-700/50 text-left text-xs font-mono text-slate-300 space-y-2 mb-8">
                <div><span class="text-slate-500">Action Title:</span> <span class="text-indigo-300 font-bold">{action_title}</span></div>
                <div><span class="text-slate-500">Approver:</span> {approver_email}</div>
                <div><span class="text-slate-500">Action Type:</span> {action_type}</div>
                <div><span class="text-slate-500">Summary:</span> {action_summary}</div>
            </div>

            <a href="/companies" class="inline-block px-5 py-2.5 bg-indigo-600 hover:bg-indigo-500 text-white font-medium rounded-lg transition">
                Return to Dashboard
            </a>
        </div>
        "##,
        status_badge = status_badge,
        title = title,
        message = message,
        action_title = approval.action_title,
        approver_email = approval.approver_email,
        action_type = approval.action_type,
        action_summary = approval.action_summary,
    );

    base_layout(title, &content)
}

pub fn approval_details_page(approval: &HumanApproval) -> String {
    let confirm_link = format!("/approvals/{}?action=confirm", approval.token);
    let reject_link = format!("/approvals/{}?action=reject", approval.token);

    let content = format!(
        r##"
        <div class="max-w-xl mx-auto py-6">
            <div class="text-center mb-6">
                <span class="px-3 py-1 text-xs font-semibold rounded-full bg-sky-500/20 text-sky-300 border border-sky-500/30">⏳ Action Approval Required</span>
                <h1 class="text-2xl font-extrabold text-white mt-3">{action_title}</h1>
            </div>

            <p class="text-slate-300 text-sm mb-6 text-center">{action_summary}</p>

            <div class="bg-slate-900/60 rounded-xl p-5 border border-slate-700/50 text-xs font-mono text-slate-300 space-y-2 mb-8">
                <div><span class="text-slate-500">Approver Email:</span> <span class="text-indigo-300">{approver_email}</span></div>
                <div><span class="text-slate-500">Action Type:</span> {action_type}</div>
                <div><span class="text-slate-500">Expires At:</span> {expires_at}</div>
            </div>

            <div class="flex items-center justify-center gap-4">
                <a href="{confirm_link}" class="px-6 py-3 bg-emerald-600 hover:bg-emerald-500 text-white font-semibold rounded-xl transition shadow-lg shadow-emerald-900/20">
                    ✓ Confirm & Execute
                </a>
                <a href="{reject_link}" class="px-6 py-3 bg-rose-600 hover:bg-rose-500 text-white font-semibold rounded-xl transition shadow-lg shadow-rose-900/20">
                    ✗ Reject
                </a>
            </div>
        </div>
        "##,
        action_title = approval.action_title,
        action_summary = approval.action_summary,
        approver_email = approval.approver_email,
        action_type = approval.action_type,
        expires_at = approval.expires_at,
        confirm_link = confirm_link,
        reject_link = reject_link,
    );

    base_layout("Confirm Action", &content)
}

pub fn channel_approvals_fragment(approvals: &[HumanApproval]) -> String {
    if approvals.is_empty() {
        return r#"<div class="p-4 text-center text-xs text-slate-400">No human-in-the-loop approvals recorded for this channel.</div>"#.to_string();
    }

    let rows: String = approvals
        .iter()
        .map(|a| {
            let badge = match a.status {
                ApprovalStatus::Approved => r#"<span class="px-2 py-0.5 text-xs rounded bg-emerald-500/20 text-emerald-300 border border-emerald-500/30">✓ Approved</span>"#,
                ApprovalStatus::Rejected => r#"<span class="px-2 py-0.5 text-xs rounded bg-rose-500/20 text-rose-300 border border-rose-500/30">✗ Rejected</span>"#,
                ApprovalStatus::Expired => r#"<span class="px-2 py-0.5 text-xs rounded bg-amber-500/20 text-amber-300 border border-amber-500/30">⏱ Expired</span>"#,
                ApprovalStatus::Pending => r#"<span class="px-2 py-0.5 text-xs rounded bg-sky-500/20 text-sky-300 border border-sky-500/30">⏳ Pending</span>"#,
            };

            format!(
                r##"
                <div class="p-3 bg-slate-900/50 rounded-lg border border-slate-800 text-xs flex items-center justify-between">
                    <div>
                        <div class="flex items-center gap-2 mb-1">
                            <span class="font-bold text-white">{action_title}</span>
                            {badge}
                        </div>
                        <p class="text-slate-400">{action_summary} • <span class="text-slate-300">{approver}</span></p>
                    </div>
                    <span class="text-slate-500 font-mono text-[10px]">{created_at}</span>
                </div>
                "##,
                action_title = a.action_title,
                badge = badge,
                action_summary = a.action_summary,
                approver = a.approver_email,
                created_at = a.created_at,
            )
        })
        .collect();

    format!(r#"<div class="space-y-2 mt-4">{rows}</div>"#, rows = rows)
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;
    use uuid::Uuid;

    #[test]
    fn test_render_markdown_formats_content_and_removes_unsafe_html() {
        let html =
            render_markdown("## Result\n\n**Ready** with `code`.\n\n<script>alert('xss')</script>");

        assert!(html.contains("<h2>Result</h2>"));
        assert!(html.contains("<strong>Ready</strong>"));
        assert!(html.contains("<code>code</code>"));
        assert!(!html.contains("<script"));
        assert!(!html.contains("alert('xss')"));
    }

    #[test]
    fn test_find_task_for_message_multi_task_matching() {
        let thread_id = Uuid::new_v4();
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();

        let task1_id = Uuid::new_v4();
        let task2_id = Uuid::new_v4();

        let task1 = BackgroundTask {
            id: task1_id,
            company_id,
            channel_id,
            thread_id: Some(thread_id),
            task_type: "email_agent_dispatch".to_string(),
            status: TaskStatus::Completed,
            payload: json!({
                "inbound_message": {
                    "message_id": "<in1@test.com>"
                },
                "execution_result": {
                    "outbound_message_id": "<out1@test.com>",
                    "response": "Response 1"
                }
            }),
            retry_count: 0,
            max_retries: 3,
            last_error: None,
            run_at: Utc::now().naive_utc(),
            created_at: Utc::now().naive_utc(),
            updated_at: Utc::now().naive_utc(),
        };

        let task2 = BackgroundTask {
            id: task2_id,
            company_id,
            channel_id,
            thread_id: Some(thread_id),
            task_type: "email_agent_dispatch".to_string(),
            status: TaskStatus::Completed,
            payload: json!({
                "inbound_message": {
                    "message_id": "<in2@test.com>"
                },
                "execution_result": {
                    "outbound_message_id": "<out2@test.com>",
                    "response": "Response 2"
                }
            }),
            retry_count: 0,
            max_retries: 3,
            last_error: None,
            run_at: Utc::now().naive_utc(),
            created_at: Utc::now().naive_utc(),
            updated_at: Utc::now().naive_utc(),
        };

        let tasks = vec![task1, task2];

        let msg_in1 = Message {
            id: Uuid::new_v4(),
            thread_id,
            message_id: "<in1@test.com>".to_string(),
            in_reply_to: None,
            references_list: vec![],
            sender: "user@test.com".to_string(),
            recipients_to: vec![],
            recipients_cc: vec![],
            subject: "Hi".to_string(),
            clean_text_body: "Inbound 1".to_string(),
            raw_text_body: None,
            raw_html_body: None,
            attachments: None,
            direction: MessageDirection::Inbound,
            role: MessageRole::Human,
            thread_index: None,
            created_at: Utc::now().naive_utc(),
        };

        let msg_out1 = Message {
            id: Uuid::new_v4(),
            thread_id,
            message_id: "<out1@test.com>".to_string(),
            in_reply_to: Some("<in1@test.com>".to_string()),
            references_list: vec![],
            sender: "agent@test.com".to_string(),
            recipients_to: vec![],
            recipients_cc: vec![],
            subject: "Re: Hi".to_string(),
            clean_text_body: "Response 1".to_string(),
            raw_text_body: None,
            raw_html_body: None,
            attachments: None,
            direction: MessageDirection::Outbound,
            role: MessageRole::Agent,
            thread_index: None,
            created_at: Utc::now().naive_utc(),
        };

        let msg_in2 = Message {
            id: Uuid::new_v4(),
            thread_id,
            message_id: "<in2@test.com>".to_string(),
            in_reply_to: Some("<out1@test.com>".to_string()),
            references_list: vec![],
            sender: "user@test.com".to_string(),
            recipients_to: vec![],
            recipients_cc: vec![],
            subject: "Re: Hi 2".to_string(),
            clean_text_body: "Inbound 2".to_string(),
            raw_text_body: None,
            raw_html_body: None,
            attachments: None,
            direction: MessageDirection::Inbound,
            role: MessageRole::Human,
            thread_index: None,
            created_at: Utc::now().naive_utc(),
        };

        let msg_out2 = Message {
            id: Uuid::new_v4(),
            thread_id,
            message_id: "<out2@test.com>".to_string(),
            in_reply_to: Some("<in2@test.com>".to_string()),
            references_list: vec![],
            sender: "agent@test.com".to_string(),
            recipients_to: vec![],
            recipients_cc: vec![],
            subject: "Re: Hi 2".to_string(),
            clean_text_body: "Response 2".to_string(),
            raw_text_body: None,
            raw_html_body: None,
            attachments: None,
            direction: MessageDirection::Outbound,
            role: MessageRole::Agent,
            thread_index: None,
            created_at: Utc::now().naive_utc(),
        };

        let matched_in1 = find_task_for_message(&msg_in1, &tasks, None, Some(thread_id)).unwrap();
        assert_eq!(matched_in1.id, task1_id);

        let matched_out1 = find_task_for_message(&msg_out1, &tasks, None, Some(thread_id)).unwrap();
        assert_eq!(matched_out1.id, task1_id);

        let matched_in2 = find_task_for_message(&msg_in2, &tasks, None, Some(thread_id)).unwrap();
        assert_eq!(matched_in2.id, task2_id);

        let matched_out2 = find_task_for_message(&msg_out2, &tasks, None, Some(thread_id)).unwrap();
        assert_eq!(matched_out2.id, task2_id);
    }

    #[test]
    fn test_task_parameters_render_execution_diagnostics() {
        let html = render_message_task_parameters_html(&json!({
            "execution_result": {
                "metadata": {
                    "execution_diagnostics": {
                        "duration_ms": 1250,
                        "response_characters": 4096,
                        "token_usage_source": "estimated",
                        "tool_call_count": 2,
                        "tool_names": ["search", "send_email"]
                    },
                    "observability": {
                        "summary": {
                            "total_events": 3,
                            "total_llm_calls": 1
                        }
                    }
                }
            }
        }));

        assert!(html.contains("Duration: 1250 ms"));
        assert!(html.contains("Token Usage: estimated"));
        assert!(html.contains("Response: 4096 chars"));
        assert!(html.contains("Tool Calls: 2"));
        assert!(html.contains("Observed: 3 events / 1 LLM calls"));
    }
}
