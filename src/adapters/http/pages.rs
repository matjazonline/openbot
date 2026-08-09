use crate::entities::company::Company;

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
    <div class="w-full max-w-3xl">
        <nav class="flex items-center justify-between mb-8 pb-4 border-b border-slate-800">
            <a href="/companies" class="text-xl font-extrabold tracking-tight text-white flex items-center gap-2">
                <span class="text-indigo-500">❖</span> Mail Agents
            </a>
            <div class="flex items-center gap-4 text-sm font-medium">
                <a href="/companies" class="text-slate-300 hover:text-white transition">Companies</a>
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
                <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
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
    format!(
        r##"
        <form id="company-{id}" hx-put="/companies/{id}" hx-target="#company-{id}" hx-swap="outerHTML"
            class="bg-slate-900 border border-indigo-500/60 rounded-xl p-4 md:p-5 space-y-4 shadow-lg">
            <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
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
