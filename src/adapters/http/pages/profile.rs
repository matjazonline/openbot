//! `/ui/profile` — the signed-in account's own details, in the shared `/ui` shell.
//!
//! Two sections, because they are two different decisions: the identity form saves who the account
//! is (its picture, its name, its address), and the password form re-authenticates before it
//! changes anything. They swap the same `#profile-pane`, so whichever one answers leaves the whole
//! pane showing what is now stored.
//!
//! Either can be waiting on a code rather than offering its form: a new address and a new password
//! are both mailed one before they take effect, and a section with a request outstanding shows
//! [`code_panel`] in place of its form until the code comes back or the request is cancelled.

use super::*;
use crate::use_cases::user::LoginMethods;

/// The Profile workspace for one request.
pub struct ProfilePage<'a> {
    pub user: &'a MailboxUser<'a>,
    /// Which company the rail points at; `None` for an account with no company yet, which is the
    /// one `/ui` page that still has to work — a new account can reach its own settings before it
    /// has anywhere to put them.
    pub company: Option<&'a Company>,
    pub pane_html: &'a str,
}

/// What the identity form was last submitted with, so a rejected save comes back filled in.
#[derive(Debug)]
pub struct ProfileDraft<'a> {
    pub username: &'a str,
    pub email: &'a str,
    /// The picture as the picker is holding it, blank for the letter bubble. Kept as the submitted
    /// text so a rejected save comes back showing what was picked -- see [`identity_form`], which
    /// is where it becomes an [`AvatarUrl`] again.
    pub avatar_url: &'a str,
}

/// Which of the pane's two forms an answer belongs to.
///
/// A message means nothing without it: "Saved" under the password form and "Saved" under the
/// identity form are different claims, and a wrong current password must not look like a rejected
/// email address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileForm {
    Identity,
    Password,
}

/// How a submit went, attached to the form that made it.
///
/// One value rather than a `(ProfileForm, Option<&str>, Option<&str>)`: an error and a
/// confirmation are mutually exclusive, and the pair form lets both be set at once.
#[derive(Debug)]
pub enum ProfileOutcome<'a> {
    /// Nothing was submitted -- the pane as it opens.
    Untouched,
    Saved(ProfileForm, &'a str),
    Rejected(ProfileForm, &'a str),
}

impl ProfileOutcome<'_> {
    /// The banner shown above `form`, empty for the form that was not submitted.
    fn banner(&self, form: ProfileForm) -> String {
        match self {
            ProfileOutcome::Saved(submitted, message) if *submitted == form => {
                success_alert(message, None)
            }
            ProfileOutcome::Rejected(submitted, message) if *submitted == form => {
                error_alert(message)
            }
            _ => String::new(),
        }
    }
}

/// The pane for one request: the stored account, whatever is waiting on a mailed code, and
/// whatever the last submit left behind.
pub struct ProfilePane<'a> {
    pub user: &'a User,
    /// What was last typed into the identity form, when a save was rejected; `None` shows the
    /// stored account.
    pub draft: Option<&'a ProfileDraft<'a>>,
    /// Changes this account has asked for and not yet proved. A section with one waiting asks for
    /// the code instead of offering its form again -- see [`code_panel`].
    pub pending: &'a [PendingChange],
    pub methods: &'a LoginMethods,
    pub google_enabled: bool,
    pub apple_enabled: bool,
    pub outcome: ProfileOutcome<'a>,
}

/// What is waiting on a code for `kind`, if anything.
fn pending_of(pending: &[PendingChange], kind: AccountChangeKind) -> Option<&PendingChange> {
    pending.iter().find(|change| change.kind() == kind)
}

pub fn profile_page(page: &ProfilePage<'_>) -> String {
    let content = format!(
        r##"
        <main class="min-w-0 flex-1 overflow-y-auto bg-base-100">
            {pane_html}
        </main>
        "##,
        pane_html = page.pane_html,
    );

    ui_shell(&UiShell {
        title: "Profile",
        user: page.user,
        company: page.company,
        section: UiSection::Profile,
        content: &content,
    })
}

/// Everything the account may change about itself, as one swappable fragment.
pub fn profile_pane(pane: &ProfilePane<'_>) -> String {
    let user = pane.user;
    let stored = stored_draft(user);
    let draft = pane.draft.unwrap_or(&stored);

    format!(
        r##"
        <div id="profile-pane" class="mx-auto max-w-3xl p-6 lg:p-10">
            <div class="mb-8 flex items-center gap-4">
                {avatar}
                <div class="min-w-0">
                    <h1 class="truncate text-2xl font-bold">{username}</h1>
                    <p class="truncate font-mono text-xs opacity-60">{email} &middot; joined {joined}</p>
                </div>
            </div>
            {identity}
            {login_methods}
            {password}
        </div>
        "##,
        avatar = avatar_bubble(user.avatar_url.as_ref(), &user.username, AvatarSize::Header),
        username = escape_html_text(&user.username),
        email = escape_html_text(&user.email),
        joined = super::format_date(user.created_at),
        identity = identity_section(draft, pane.pending, &pane.outcome),
        login_methods = login_methods_section(pane),
        password = password_section(pane.user, pane.pending, pane.methods, &pane.outcome),
    )
}

fn login_methods_section(pane: &ProfilePane<'_>) -> String {
    let google = provider_row(
        "Google",
        pane.methods.google,
        pane.google_enabled,
        "/auth/google/connect",
    );
    let apple = provider_row(
        "Apple",
        pane.methods.apple,
        pane.apple_enabled,
        "/auth/apple/connect",
    );
    format!(
        r##"
            <section class="mb-6 rounded-box border border-base-300 bg-base-200 p-6">
                <h2 class="text-lg font-bold">Login methods</h2>
                <p class="mb-4 text-xs opacity-60">Connected methods sign in to this same account. Provider emails must match your account email.</p>
                <div class="space-y-3">{google}{apple}</div>
            </section>
        "##
    )
}

fn provider_row(name: &str, connected: bool, enabled: bool, connect_url: &str) -> String {
    let action = if connected {
        r#"<span class="badge badge-success badge-outline">Connected</span>"#.to_string()
    } else if enabled {
        format!(r##"<a class="btn btn-sm btn-outline" href="{connect_url}">Connect</a>"##)
    } else {
        r#"<span class="text-xs opacity-50">Unavailable</span>"#.to_string()
    };
    format!(
        r#"<div class="flex items-center justify-between rounded-box border border-base-300 bg-base-100 px-4 py-3"><span class="font-medium">{name}</span>{action}</div>"#,
        name = escape_html_text(name),
    )
}

/// The account's own details, or the code that has to come back before its new address is real.
fn identity_section(
    draft: &ProfileDraft<'_>,
    pending: &[PendingChange],
    outcome: &ProfileOutcome<'_>,
) -> String {
    let body = match pending_of(pending, AccountChangeKind::Email) {
        Some(PendingChange::Email {
            new_email,
            expires_at,
        }) => code_panel(&CodePanel {
            sent_to: new_email,
            explanation: "Enter it to move your account to that address. Until you do, your account keeps the address it has.",
            kind: AccountChangeKind::Email,
            expires_at: *expires_at,
        }),
        _ => identity_form(draft),
    };

    format!(
        r##"
            <section class="mb-6 rounded-box border border-base-300 bg-base-200 p-6">
                <h2 class="text-lg font-bold">Account details</h2>
                <p class="mb-4 text-xs opacity-60">Your name and picture are what your team and your threads show you as; your address is what channels recognise you by.</p>
                {banner}
                {body}
            </section>
        "##,
        banner = outcome.banner(ProfileForm::Identity),
    )
}

/// The picture, the name and the address: what other people see this account as.
fn identity_form(draft: &ProfileDraft<'_>) -> String {
    // Taken as text and parsed here rather than carried as a URL, so a tampered hidden field
    // cannot reach the `<img src>` the bubble draws.
    let picture = AvatarUrl::parse(draft.avatar_url).ok().flatten();

    format!(
        r##"
                <form class="space-y-4" hx-put="/ui/profile" hx-target="#profile-pane" hx-swap="outerHTML"
                    hx-disabled-elt="find button[type='submit']">
                    {picture_field}
                    <div class="grid grid-cols-1 gap-4 md:grid-cols-2">
                        <label class="form-control w-full">
                            <div class="label"><span class="text-xs opacity-70">Username</span></div>
                            <input type="text" name="username" required value="{username}" placeholder="dana"
                                class="input w-full">
                        </label>
                        <label class="form-control w-full">
                            <div class="label"><span class="text-xs opacity-70">Email Address</span></div>
                            <input type="email" name="email" required value="{email}" placeholder="dana@example.com"
                                class="input w-full font-mono">
                        </label>
                    </div>
                    <p class="text-[11px] opacity-60">A new address is mailed a code before it becomes yours, so it is not your account's until you confirm it. Changing it changes which channels treat you as a participant, and where mail addressed to you arrives. You stay signed in.</p>
                    <div class="border-t border-base-300 pt-4">
                        <button type="submit" class="btn btn-primary">
                            <span class="loading loading-spinner loading-sm hidden [.htmx-request_&]:inline-block"></span>
                            <span class="[.htmx-request_&]:hidden">Save Details</span>
                            <span class="hidden [.htmx-request_&]:inline">Saving...</span>
                        </button>
                    </div>
                </form>
        "##,
        picture_field = avatar_picker(&AvatarPicker {
            field_id: "profile-avatar",
            avatar_url: picture.as_ref(),
            name: draft.username,
            label: "Your Picture",
            error: None,
        }),
        username = escape_html_text(draft.username),
        email = escape_html_text(draft.email),
    )
}

/// The password form, or the code that has to come back before a new password takes effect.
fn password_section(
    user: &User,
    pending: &[PendingChange],
    methods: &LoginMethods,
    outcome: &ProfileOutcome<'_>,
) -> String {
    let account_email = EmailAddress::from(user.email.as_str());
    let body = match pending_of(pending, AccountChangeKind::Password) {
        Some(PendingChange::Password { expires_at }) => code_panel(&CodePanel {
            sent_to: &account_email,
            explanation: if methods.password {
                "Enter it to finish the change. Until you do, your current password still works."
            } else {
                "Enter it to activate password login. Until you do, use your connected provider."
            },
            kind: AccountChangeKind::Password,
            expires_at: *expires_at,
        }),
        _ if methods.password => password_form(),
        _ => password_setup_form(),
    };

    format!(
        r##"
            <section class="rounded-box border border-base-300 bg-base-200 p-6">
                <h2 class="text-lg font-bold">Password</h2>
                <p class="mb-4 text-xs opacity-60">{description}</p>
                {banner}
                {body}
            </section>
        "##,
        banner = outcome.banner(ProfileForm::Password),
        description = if methods.password {
            "Changing it does not sign your other browsers out."
        } else {
            "Add email or username and password as another way to sign in."
        },
    )
}

fn password_setup_form() -> String {
    format!(
        r##"
                <p class="mb-4 text-xs opacity-60">Add a password so you can also sign in with your email or username.</p>
                <form class="space-y-4" hx-put="/ui/profile/password/setup" hx-target="#profile-pane" hx-swap="outerHTML"
                    hx-disabled-elt="find button[type='submit']">
                    <div class="grid grid-cols-1 gap-4 md:grid-cols-2">
                        <label class="form-control w-full">
                            <div class="label"><span class="text-xs opacity-70">New Password</span></div>
                            <input type="password" name="new_password" required minlength="{MIN_PASSWORD_CHARS}"
                                autocomplete="new-password" class="input w-full">
                        </label>
                        <label class="form-control w-full">
                            <div class="label"><span class="text-xs opacity-70">Confirm Password</span></div>
                            <input type="password" name="confirm_password" required minlength="{MIN_PASSWORD_CHARS}"
                                autocomplete="new-password" class="input w-full">
                        </label>
                    </div>
                    <button type="submit" class="btn btn-primary">Add Password</button>
                </form>
        "##,
        MIN_PASSWORD_CHARS = crate::use_cases::user::MIN_PASSWORD_CHARS,
    )
}

/// Never pre-filled: a password field that comes back holding what was typed is a password sitting
/// in the page's HTML.
fn password_form() -> String {
    format!(
        r##"
                <form class="space-y-4" hx-put="/ui/profile/password" hx-target="#profile-pane" hx-swap="outerHTML"
                    hx-disabled-elt="find button[type='submit']">
                    <label class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">Current Password</span></div>
                        <input type="password" name="current_password" required autocomplete="current-password"
                            class="input w-full">
                    </label>
                    <div class="grid grid-cols-1 gap-4 md:grid-cols-2">
                        <label class="form-control w-full">
                            <div class="label"><span class="text-xs opacity-70">New Password</span></div>
                            <input type="password" name="new_password" required autocomplete="new-password"
                                minlength="{minimum}" class="input w-full">
                        </label>
                        <label class="form-control w-full">
                            <div class="label"><span class="text-xs opacity-70">Confirm New Password</span></div>
                            <input type="password" name="confirm_password" required autocomplete="new-password"
                                minlength="{minimum}" class="input w-full">
                        </label>
                    </div>
                    <div class="border-t border-base-300 pt-4">
                        <button type="submit" class="btn btn-primary">
                            <span class="loading loading-spinner loading-sm hidden [.htmx-request_&]:inline-block"></span>
                            <span class="[.htmx-request_&]:hidden">Change Password</span>
                            <span class="hidden [.htmx-request_&]:inline">Changing...</span>
                        </button>
                    </div>
                </form>
        "##,
        minimum = crate::use_cases::user::MIN_PASSWORD_CHARS,
    )
}

/// A change that has been asked for and is waiting on the code mailed out for it.
pub struct CodePanel<'a> {
    /// Where the code went -- the *new* address for an address change, the account's own for a
    /// password one. Named because it is the only thing that tells the reader which inbox to open.
    pub sent_to: &'a EmailAddress,
    /// What confirming the code will do, and what holds until it is confirmed.
    pub explanation: &'a str,
    pub kind: AccountChangeKind,
    pub expires_at: DateTime<Utc>,
}

/// The one control that turns a mailed code into a change, wherever a change is confirmed.
///
/// It replaces its section's form rather than sitting beside it: the account is in one state or
/// the other, and offering both invites a second request that silently voids the code already in
/// somebody's inbox. Cancel is how you get the form back.
fn code_panel(panel: &CodePanel<'_>) -> String {
    format!(
        r##"
                <form class="space-y-4" hx-post="/ui/profile/changes/{kind}" hx-target="#profile-pane" hx-swap="outerHTML"
                    hx-disabled-elt="find button[type='submit']">
                    <div class="rounded-box bg-base-300 px-4 py-3 text-sm">
                        <p>We sent a 6-digit code to <span class="font-mono font-semibold">{sent_to}</span>.</p>
                        <p class="mt-1 text-xs opacity-70">{explanation}</p>
                        <p class="mt-1 text-xs opacity-60">The code expires at {expires_at}.</p>
                    </div>
                    <label class="form-control w-full max-w-xs">
                        <div class="label"><span class="text-xs opacity-70">Confirmation Code</span></div>
                        <input type="text" name="code" required inputmode="numeric" autocomplete="one-time-code"
                            pattern="[0-9]{{6}}" maxlength="6" placeholder="000000"
                            class="input w-full font-mono tracking-[0.4em]">
                    </label>
                    <div class="flex items-center gap-3 border-t border-base-300 pt-4">
                        <button type="submit" class="btn btn-primary">
                            <span class="loading loading-spinner loading-sm hidden [.htmx-request_&]:inline-block"></span>
                            <span class="[.htmx-request_&]:hidden">Confirm</span>
                            <span class="hidden [.htmx-request_&]:inline">Confirming...</span>
                        </button>
                        <button type="button" class="btn btn-ghost"
                            hx-delete="/ui/profile/changes/{kind}"
                            hx-target="#profile-pane" hx-swap="outerHTML">Cancel</button>
                    </div>
                </form>
        "##,
        kind = panel.kind.as_str(),
        sent_to = escape_html_text(panel.sent_to),
        explanation = escape_html_text(panel.explanation),
        expires_at = super::format_date_time(panel.expires_at),
    )
}

/// A stored account as the identity form sees it.
fn stored_draft(user: &User) -> ProfileDraft<'_> {
    ProfileDraft {
        username: &user.username,
        email: &user.email,
        avatar_url: user.avatar_url.as_deref().unwrap_or(""),
    }
}
