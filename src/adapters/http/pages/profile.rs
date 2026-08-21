//! `/ui/profile` — the signed-in account's own details, in the shared `/ui` shell.
//!
//! Two forms, because they are two different decisions: the identity form saves who the account
//! is (its picture, its name, its address), and the password form re-authenticates before it
//! changes anything. They swap the same `#profile-pane`, so whichever one answers leaves the
//! whole pane showing what is now stored.

use super::*;

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

/// The pane for one request: the stored account, plus whatever the last submit left behind.
pub struct ProfilePane<'a> {
    pub user: &'a User,
    /// What was last typed into the identity form, when a save was rejected; `None` shows the
    /// stored account.
    pub draft: Option<&'a ProfileDraft<'a>>,
    pub outcome: ProfileOutcome<'a>,
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
        script: "",
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
            {password}
        </div>
        "##,
        avatar = avatar_bubble(user.avatar_url.as_ref(), &user.username, AvatarSize::Header),
        username = escape_html_text(&user.username),
        email = escape_html_text(&user.email),
        joined = super::format_date(user.created_at),
        identity = identity_form(draft, &pane.outcome),
        password = password_form(&pane.outcome),
    )
}

/// The picture, the name and the address: what other people see this account as.
fn identity_form(draft: &ProfileDraft<'_>, outcome: &ProfileOutcome<'_>) -> String {
    // Taken as text and parsed here rather than carried as a URL, so a tampered hidden field
    // cannot reach the `<img src>` the bubble draws.
    let picture = AvatarUrl::parse(draft.avatar_url).ok().flatten();

    format!(
        r##"
            <section class="mb-6 rounded-box border border-base-300 bg-base-200 p-6">
                <h2 class="text-lg font-bold">Account details</h2>
                <p class="mb-4 text-xs opacity-60">Your name and picture are what your team and your threads show you as; your address is what channels recognise you by.</p>
                {banner}
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
                    <p class="text-[11px] opacity-60">Changing your address changes which channels treat you as a participant, and where mail addressed to you arrives. You stay signed in.</p>
                    <div class="border-t border-base-300 pt-4">
                        <button type="submit" class="btn btn-primary">
                            <span class="loading loading-spinner loading-sm hidden [.htmx-request_&]:inline-block"></span>
                            <span class="[.htmx-request_&]:hidden">Save Details</span>
                            <span class="hidden [.htmx-request_&]:inline">Saving...</span>
                        </button>
                    </div>
                </form>
            </section>
        "##,
        banner = outcome.banner(ProfileForm::Identity),
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

/// Its own form, and never pre-filled: a password field that comes back holding what was typed is
/// a password sitting in the page's HTML.
fn password_form(outcome: &ProfileOutcome<'_>) -> String {
    format!(
        r##"
            <section class="rounded-box border border-base-300 bg-base-200 p-6">
                <h2 class="text-lg font-bold">Password</h2>
                <p class="mb-4 text-xs opacity-60">Changing it does not sign your other browsers out.</p>
                {banner}
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
            </section>
        "##,
        banner = outcome.banner(ProfileForm::Password),
        minimum = crate::use_cases::user::MIN_PASSWORD_CHARS,
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
