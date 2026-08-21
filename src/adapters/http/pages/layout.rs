//! Page chrome shared by every view: the HTML shell, markdown rendering and alerts.

use super::*;

pub(crate) const MARKDOWN_CONTENT_STYLES: &str = "[&_p]:mb-2 [&_p:last-child]:mb-0 [&_h1]:mb-2 [&_h1]:text-base [&_h1]:font-bold [&_h2]:mb-2 [&_h2]:text-sm [&_h2]:font-bold [&_h3]:mb-2 [&_h3]:font-bold [&_ul]:list-disc [&_ul]:pl-5 [&_ol]:list-decimal [&_ol]:pl-5 [&_li]:my-1 [&_blockquote]:my-2 [&_blockquote]:border-l-2 [&_blockquote]:border-slate-600 [&_blockquote]:pl-3 [&_blockquote]:text-slate-300 [&_a]:underline [&_a]:text-indigo-300 [&_strong]:font-bold [&_code]:rounded [&_code]:bg-slate-800 [&_code]:px-1 [&_pre]:my-2 [&_pre]:overflow-x-auto [&_pre]:rounded [&_pre]:bg-slate-900 [&_pre]:p-3 [&_pre_code]:bg-transparent [&_pre_code]:p-0 [&_table]:my-2 [&_table]:w-full [&_th]:border [&_th]:border-slate-700 [&_th]:p-2 [&_th]:text-left [&_td]:border [&_td]:border-slate-800 [&_td]:p-2";

pub(crate) fn render_markdown(markdown: &str) -> String {
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
    layout(title, content, true)
}

pub(crate) fn public_layout(title: &str, content: &str) -> String {
    format!(
        r##"<!DOCTYPE html>
<html lang="en" data-theme="dark" class="h-full">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{title} - Mail Agents</title>
    <link href="https://cdn.jsdelivr.net/npm/daisyui@5" rel="stylesheet" type="text/css" />
    <link href="https://cdn.jsdelivr.net/npm/daisyui@5/themes.css" rel="stylesheet" type="text/css" />
    <style>
        [data-theme="dark"] {{
            --color-primary: oklch(50% 0.19 264.05);
            --color-primary-content: oklch(96% 0.02 264.05);
        }}
        :root, [data-theme] {{ --radius-field: .5rem !important; }}
        .input:focus, .input:focus-within {{ outline-offset: -1px; }}
    </style>
    <script src="https://cdn.jsdelivr.net/npm/@tailwindcss/browser@4"></script>
    <script src="https://unpkg.com/htmx.org@2.0.4"></script>
</head>
<body class="min-h-full bg-base-100 text-base-content">
    <div class="mx-auto flex min-h-screen w-full max-w-5xl flex-col px-4 py-6 sm:px-6 lg:px-8">
        <nav class="navbar min-h-0 px-0">
            <div class="flex-1">
                <a href="/login" title="BusyBots">
                    <img src="/assets/busybots-logo-light.png" alt="BusyBots" class="h-10 w-auto">
                </a>
            </div>
            <div class="flex-none gap-2">
                <a href="/login" class="btn btn-ghost btn-sm">Sign in</a>
                <a href="/register" class="btn btn-primary btn-sm">Sign up</a>
            </div>
        </nav>

        <main class="flex flex-1 items-center justify-center py-10">
            <div class="card w-full max-w-md border border-base-300 bg-base-200 shadow-2xl">
                <div class="card-body p-6 sm:p-8">
                    {content}
                </div>
            </div>
        </main>
    </div>
</body>
</html>"##
    )
}

/// Navigation shown once a session exists: company-scoped shortcuts plus the account menu.
const AUTHENTICATED_NAV: &str = r##"
                <a id="nav-channels" href="#" class="hidden text-slate-300 hover:text-white transition">Channels</a>
                <a id="nav-agents" href="#" class="hidden text-slate-300 hover:text-white transition">Agents</a>
                <details class="relative">
                    <summary class="list-none text-slate-300 hover:text-white transition cursor-pointer [&::-webkit-details-marker]:hidden">Account</summary>
                    <div class="absolute right-0 z-50 mt-2 w-36 overflow-hidden rounded-lg border border-slate-700 bg-slate-800 shadow-xl">
                        <a href="/companies" class="block px-4 py-2.5 text-slate-300 hover:bg-slate-700 hover:text-white transition">Companies</a>
                        <a href="/ui/invites" class="block px-4 py-2.5 text-slate-300 hover:bg-slate-700 hover:text-white transition">My Invites</a>
                        <form method="post" action="/logout" class="border-t border-slate-700">
                            <button type="submit" class="w-full px-4 py-2.5 text-left text-slate-300 hover:bg-slate-700 hover:text-white transition cursor-pointer">Log Out</button>
                        </form>
                    </div>
                </details>
        "##;

/// Navigation for signed-out visitors.
const PUBLIC_NAV: &str = r##"
                <a href="/login" class="text-slate-300 hover:text-white transition">Sign In</a>
                <a href="/register" class="px-3 py-1.5 bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg transition">Sign Up</a>
        "##;

/// Client-side behaviour shared by every page: remembering the selected company, the spam-warning
/// interlock, and the participant/team autocomplete.
///
/// Kept out of the `format!` above so its braces need no escaping and it reads as plain JavaScript.
const APP_SCRIPT: &str = r##"        function getCachedCompanyId() {
            return localStorage.getItem('cached_company_id');
        }

        function selectCompany(companyId) {
            if (!companyId) return;
            localStorage.setItem('cached_company_id', companyId);
            updateNavChannels();
        }

        function clearCachedCompanyIfMatch(companyId) {
            if (getCachedCompanyId() === companyId) {
                localStorage.removeItem('cached_company_id');
                updateNavChannels();
            }
        }

        function updateNavChannels() {
            const navChannels = document.getElementById('nav-channels');
            const navAgents = document.getElementById('nav-agents');
            const companyId = getCachedCompanyId();
            if (navChannels) {
                if (companyId) {
                    navChannels.href = '/companies/' + companyId + '/channels';
                    navChannels.classList.remove('hidden');
                } else {
                    navChannels.classList.add('hidden');
                }
            }
            if (navAgents) {
                if (companyId) {
                    navAgents.href = '/companies/' + companyId + '/agents';
                    navAgents.classList.remove('hidden');
                } else {
                    navAgents.classList.add('hidden');
                }
            }
            document.querySelectorAll('[id^="selected-badge-"]').forEach(el => {
                if (el.id === 'selected-badge-' + companyId) {
                    el.classList.remove('hidden');
                } else {
                    el.classList.add('hidden');
                }
            });
        }

        function autoDetectAndSyncCompany() {
            const match = window.location.pathname.match(/\/companies\/([a-f0-9\-]{36})/i);
            if (match && match[1]) {
                selectCompany(match[1]);
            } else {
                updateNavChannels();
            }
        }

        document.addEventListener('DOMContentLoaded', autoDetectAndSyncCompany);
        document.addEventListener('htmx:afterSettle', autoDetectAndSyncCompany);
        autoDetectAndSyncCompany();

        function toggleSpamWarning(input) {
            var form = input.closest('form');
            if (!form) return;
            var box = form.querySelector('.spam-disabled-box');
            if (!box) return;
            var checkbox = box.querySelector('input[type="checkbox"]');
            var isPublic = input.value.toLowerCase().includes('@public');
            if (isPublic) {
                box.classList.remove('opacity-40', 'pointer-events-none', 'grayscale');
                if (checkbox) {
                    checkbox.disabled = false;
                }
            } else {
                box.classList.add('opacity-40', 'pointer-events-none', 'grayscale');
                if (checkbox) {
                    checkbox.disabled = true;
                    checkbox.checked = false;
                }
            }
        }

        window.companyTeamCache = window.companyTeamCache || {};

        async function fetchCompanyTeam(companyId) {
            if (!companyId) return [];
            if (window.companyTeamCache[companyId]) {
                return window.companyTeamCache[companyId];
            }
            try {
                const res = await fetch('/api/companies/' + companyId + '/team', { credentials: 'same-origin' });
                if (!res.ok) return [];
                const data = await res.json();
                if (data && data.success && Array.isArray(data.members)) {
                    const members = data.members.filter(m => m.email && m.email.trim().length > 0);
                    window.companyTeamCache[companyId] = members;
                    return members;
                }
            } catch (e) {
                console.error('Error fetching team members:', e);
            }
            return [];
        }

        async function initTeamAutocomplete() {
            const inputs = document.querySelectorAll('input[name="participant_emails"]');
            if (!inputs.length) return;

            for (const input of inputs) {
                if (input.dataset.teamAutocompleteInitialized === 'true') continue;
                input.dataset.teamAutocompleteInitialized = 'true';
                input.setAttribute('autocomplete', 'off');

                let companyId = input.dataset.companyId ||
                                input.closest('[data-company-id]')?.dataset.companyId;
                if (!companyId) {
                    const match = window.location.pathname.match(/\/companies\/([a-f0-9\-]{36})/i);
                    if (match && match[1]) companyId = match[1];
                }
                if (!companyId && typeof getCachedCompanyId === 'function') {
                    companyId = getCachedCompanyId();
                }

                if (!companyId) continue;

                const members = await fetchCompanyTeam(companyId);
                if (!members.length) continue;

                const parent = input.parentElement;
                if (!parent) continue;

                let wrapper = parent.querySelector('.team-autocomplete-wrapper');
                if (!wrapper) {
                    wrapper = document.createElement('div');
                    wrapper.className = 'team-autocomplete-wrapper relative';
                    input.parentNode.insertBefore(wrapper, input);
                    wrapper.appendChild(input);
                }

                let chipsContainer = parent.querySelector('.team-chips-container');
                if (!chipsContainer) {
                    chipsContainer = document.createElement('div');
                    chipsContainer.className = 'team-chips-container mt-2 flex flex-wrap items-center gap-1.5 text-xs';
                    parent.appendChild(chipsContainer);
                }

                let dropdown = wrapper.querySelector('.team-dropdown');
                if (!dropdown) {
                    dropdown = document.createElement('div');
                    dropdown.className = 'team-dropdown absolute left-0 right-0 top-full mt-1 z-50 bg-slate-800 border border-slate-700 rounded-lg shadow-xl hidden max-h-48 overflow-y-auto font-sans';
                    wrapper.appendChild(dropdown);
                }

                function getParsedEmails() {
                    return input.value
                        .split(',')
                        .map(s => s.trim().toLowerCase())
                        .filter(Boolean);
                }

                function updateChips() {
                    const currentEmails = getParsedEmails();
                    chipsContainer.innerHTML = '<span class="text-[11px] font-medium text-slate-400 mr-1">Team:</span>';
                    members.forEach(member => {
                        const emailLower = member.email.toLowerCase();
                        const isSelected = currentEmails.includes(emailLower);
                        const btn = document.createElement('button');
                        btn.type = 'button';
                        btn.className = isSelected
                            ? 'px-2 py-0.5 rounded-md bg-emerald-500/20 text-emerald-300 border border-emerald-500/40 text-[11px] font-mono cursor-pointer hover:bg-emerald-500/30 transition flex items-center gap-1'
                            : 'px-2 py-0.5 rounded-md bg-slate-800 text-slate-300 border border-slate-700 text-[11px] font-mono cursor-pointer hover:bg-slate-700 hover:text-white transition flex items-center gap-1';
                        // The glyph is markup, the email is not: it goes in as text so an
                        // address containing angle brackets stays an address.
                        btn.innerHTML = (isSelected ? CHIP_SELECTED_MARK : CHIP_ADD_MARK) + '<span></span>';
                        btn.lastChild.textContent = member.email;
                        btn.title = (member.username ? member.username + ' (' + member.role + ')' : member.role);
                        btn.addEventListener('click', (e) => {
                            e.preventDefault();
                            let emails = input.value.split(',').map(s => s.trim()).filter(Boolean);
                            if (isSelected) {
                                emails = emails.filter(e => e.toLowerCase() !== emailLower);
                            } else {
                                if (!emails.some(e => e.toLowerCase() === emailLower)) {
                                    emails.push(member.email);
                                }
                            }
                            input.value = emails.join(', ');
                            input.dispatchEvent(new Event('input', { bubbles: true }));
                            updateChips();
                        });
                        chipsContainer.appendChild(btn);
                    });
                }

                function getCurrentToken() {
                    const val = input.value;
                    const pos = input.selectionStart || val.length;
                    const left = val.slice(0, pos);
                    const lastComma = left.lastIndexOf(',');
                    const token = lastComma >= 0 ? left.slice(lastComma + 1) : left;
                    return { token: token.trim(), pos, lastComma };
                }

                function renderDropdown() {
                    const { token, lastComma } = getCurrentToken();
                    if (!token) {
                        dropdown.classList.add('hidden');
                        return;
                    }
                    const tokenLower = token.toLowerCase();
                    const currentEmails = getParsedEmails();
                    const matches = members.filter(m => {
                        const emailLower = m.email.toLowerCase();
                        const nameLower = (m.username || '').toLowerCase();
                        return (emailLower.includes(tokenLower) || nameLower.includes(tokenLower)) &&
                               !currentEmails.includes(emailLower);
                    });

                    if (!matches.length) {
                        dropdown.classList.add('hidden');
                        return;
                    }

                    dropdown.innerHTML = '';
                    matches.forEach(m => {
                        const item = document.createElement('div');
                        item.className = 'px-3 py-2 hover:bg-slate-700/80 cursor-pointer text-xs flex items-center justify-between border-b border-slate-700/50 last:border-b-0 text-slate-200';
                        item.innerHTML = `<span class="font-mono text-emerald-400 font-medium">${m.email}</span>` +
                            (m.username ? `<span class="text-slate-400 text-[11px]">${m.username} (${m.role})</span>` : `<span class="text-slate-400 text-[11px]">${m.role}</span>`);

                        item.addEventListener('mousedown', (e) => {
                            e.preventDefault();
                            const val = input.value;
                            const before = lastComma >= 0 ? val.slice(0, lastComma + 1) : '';
                            const after = val.slice(input.selectionStart || val.length);
                            const afterClean = after.replace(/^[^,]*/, '');
                            const prefix = before ? before.trim() + ' ' : '';
                            input.value = prefix + m.email + ', ' + afterClean.trim();
                            input.dispatchEvent(new Event('input', { bubbles: true }));
                            dropdown.classList.add('hidden');
                            updateChips();
                        });
                        dropdown.appendChild(item);
                    });

                    dropdown.classList.remove('hidden');
                }

                input.addEventListener('input', () => {
                    updateChips();
                    renderDropdown();
                });

                input.addEventListener('focus', () => {
                    updateChips();
                    renderDropdown();
                });

                input.addEventListener('blur', () => {
                    setTimeout(() => dropdown.classList.add('hidden'), 200);
                });

                updateChips();
            }
        }

        document.addEventListener('DOMContentLoaded', initTeamAutocomplete);
        document.addEventListener('htmx:afterSettle', initTeamAutocomplete);"##;

pub(crate) fn layout(title: &str, content: &str, authenticated: bool) -> String {
    let home_href = if authenticated {
        "/companies"
    } else {
        "/login"
    };
    let navigation = if authenticated {
        AUTHENTICATED_NAV
    } else {
        PUBLIC_NAV
    };

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
            <a href="{home_href}" class="flex items-center" title="BusyBots">
                <img src="/assets/busybots-logo-light.png" alt="BusyBots" class="h-9 w-auto">
            </a>
            <div class="flex items-center gap-4 text-sm font-medium">
                {navigation}
            </div>
        </nav>
        <div class="bg-slate-800/80 backdrop-blur-md border border-slate-700/60 rounded-2xl shadow-2xl p-6 md:p-8">
            {content}
        </div>
    </div>
    <script>
        var CHIP_SELECTED_MARK = '{chip_selected_mark}';
        var CHIP_ADD_MARK = '{chip_add_mark}';
{APP_SCRIPT}
{LOCAL_TIME_SCRIPT}
    </script>
</body>
</html>"##,
        chip_selected_mark = icon(Icon::Check, BUTTON_ICON),
        chip_add_mark = icon(Icon::Plus, BUTTON_ICON),
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

/// How big an avatar bubble is drawn, by where it sits rather than by a raw class: the three
/// places `/ui` shows a face should not each pick their own size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AvatarSize {
    /// A sidebar row -- a channel's agents, a company's team.
    Row,
    /// The account button in the top bar.
    Bar,
    /// The header of an open pane.
    Header,
}

impl AvatarSize {
    fn bubble_class(self) -> &'static str {
        match self {
            AvatarSize::Row => "w-8",
            AvatarSize::Bar => "w-9",
            AvatarSize::Header => "w-12",
        }
    }

    fn letter_class(self) -> &'static str {
        match self {
            AvatarSize::Row => "text-xs",
            AvatarSize::Bar => "text-sm",
            AvatarSize::Header => "text-lg",
        }
    }
}

/// The face shown for a user or an agent: their picture when they have one, the first letter of
/// their name when they do not.
///
/// The letter is always rendered underneath, and the picture sits on top of it -- an avatar URL
/// that 404s or is blocked drops its `<img>` and reveals the letter, rather than leaving the hole
/// a bare broken image would.
pub(crate) fn avatar_bubble(
    avatar_url: Option<&AvatarUrl>,
    name: &str,
    size: AvatarSize,
) -> String {
    let picture = match avatar_url {
        Some(url) => format!(
            r##"<img src="{url}" alt="" loading="lazy" onerror="this.remove()"
                            class="absolute inset-0 h-full w-full rounded-full object-cover">"##,
            url = escape_html_text(url.as_str()),
        ),
        None => String::new(),
    };

    format!(
        r##"<div class="avatar avatar-placeholder shrink-0">
                    <div class="relative {bubble_class} rounded-full bg-primary text-primary-content">
                        <span class="{letter_class} font-bold">{initial}</span>
                        {picture}
                    </div>
                </div>"##,
        bubble_class = size.bubble_class(),
        letter_class = size.letter_class(),
        initial = escape_html_text(&avatar_initial(name)),
    )
}

/// The letter in an avatar bubble: the first character of whatever the thing is called.
fn avatar_initial(name: &str) -> String {
    name.trim()
        .chars()
        .next()
        .map(|first| first.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string())
}

pub(crate) fn escape_html_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
