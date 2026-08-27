//! Login and registration pages.

use super::*;

pub fn login_page(google_enabled: bool, apple_enabled: bool) -> String {
    let providers = provider_buttons("login", "Sign in", google_enabled, apple_enabled);
    let content = format!(
        r##"
        <div class="mb-6 text-center">
            <h1 class="text-2xl font-bold">Welcome back</h1>
            <p class="mt-2 text-sm text-base-content/60">Sign in to continue to BusyBots.</p>
        </div>

        <div id="response-message"></div>

        {providers}

        <form hx-post="/api/user/login" hx-target="#response-message" hx-swap="innerHTML" class="space-y-5">
            <fieldset class="fieldset">
                <label for="email_or_username" class="fieldset-legend">Email or username</label>
                <input type="text" id="email_or_username" name="email_or_username" required
                    autocomplete="username" class="input w-full" placeholder="you@example.com or username">
            </fieldset>

            <fieldset class="fieldset">
                <label for="password" class="fieldset-legend">Password</label>
                <input type="password" id="password" name="password" required
                    autocomplete="current-password" class="input w-full" placeholder="Enter your password">
            </fieldset>

            <button type="submit" class="btn btn-primary w-full">Sign in</button>
        </form>

        <div class="mt-6 text-center text-sm text-base-content/60">
            Don't have an account?
            <a href="/register" class="link link-primary ml-1 font-medium">Sign up</a>
        </div>
    "##
    );

    public_layout("Login", &content)
}

pub fn register_page(google_enabled: bool, apple_enabled: bool) -> String {
    let providers = provider_buttons("register", "Register", google_enabled, apple_enabled);
    let content = format!(
        r##"
        <div class="mb-6 text-center">
            <h1 class="text-2xl font-bold">Create an account</h1>
            <p class="mt-2 text-sm text-base-content/60">Get started with BusyBots.</p>
        </div>

        <div id="response-message"></div>

        {providers}

        <form hx-post="/api/user/register" hx-target="#response-message" hx-swap="innerHTML" class="space-y-4">
            <fieldset class="fieldset">
                <label for="username" class="fieldset-legend">Username</label>
                <input type="text" id="username" name="username" required
                    autocomplete="username" class="input w-full" placeholder="johndoe">
            </fieldset>

            <fieldset class="fieldset">
                <label for="email" class="fieldset-legend">Email address</label>
                <input type="email" id="email" name="email" required
                    autocomplete="email" class="input w-full" placeholder="you@example.com">
            </fieldset>

            <fieldset class="fieldset">
                <label for="password" class="fieldset-legend">Password</label>
                <input type="password" id="password" name="password" required
                    autocomplete="new-password" class="input w-full" placeholder="Create a password">
            </fieldset>

            <fieldset class="fieldset">
                <label for="confirm_password" class="fieldset-legend">Confirm password</label>
                <input type="password" id="confirm_password" name="confirm_password" required
                    autocomplete="new-password" class="input w-full" placeholder="Repeat your password">
            </fieldset>

            <button type="submit" class="btn btn-primary mt-2 w-full">Create account</button>
        </form>

        <div class="mt-6 text-center text-sm text-base-content/60">
            Already have an account?
            <a href="/login" class="link link-primary ml-1 font-medium">Sign in</a>
        </div>
    "##
    );

    public_layout("Register", &content)
}

fn provider_buttons(action: &str, verb: &str, google_enabled: bool, apple_enabled: bool) -> String {
    if !google_enabled && !apple_enabled {
        return String::new();
    }
    let google = google_enabled
        .then(|| format!(r##"<a href="/auth/google/{action}" class="btn btn-outline w-full">{verb} with Google</a>"##))
        .unwrap_or_default();
    let apple = apple_enabled
        .then(|| format!(r##"<a href="/auth/apple/{action}" class="btn btn-neutral w-full">{verb} with Apple</a>"##))
        .unwrap_or_default();
    format!(
        r##"<div class="space-y-2">{google}{apple}</div>
        <div class="divider text-xs text-base-content/50">OR</div>"##
    )
}

pub fn confirmation_form(email: &str) -> String {
    let email = escape_html_text(email);
    format!(
        r##"<div class="rounded-lg border border-blue-200 bg-blue-50 p-4 text-blue-900">
            <p class="mb-3">We sent a 6-digit confirmation code to <strong>{email}</strong>.</p>
            <form hx-post="/api/user/register/confirm" hx-target="#response-message" hx-swap="innerHTML" class="space-y-3">
                <input type="hidden" name="email" value="{email}">
                <label for="confirmation_code" class="block text-sm font-medium">Confirmation code</label>
                <input type="text" id="confirmation_code" name="code" required inputmode="numeric"
                    autocomplete="one-time-code" pattern="[0-9]{{6}}" maxlength="6"
                    class="w-full rounded-lg border border-slate-300 px-3 py-2" placeholder="000000">
                <button type="submit" class="w-full rounded-lg bg-blue-600 px-4 py-2 font-medium text-white">Confirm email</button>
            </form>
        </div>"##,
    )
}
