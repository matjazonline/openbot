//! The signed-in user's company invitations in the shared `/ui` shell.

use super::*;

pub struct InviteSettingsPage<'a> {
    pub user: &'a MailboxUser<'a>,
    pub company: Option<&'a Company>,
    pub invites: &'a [CompanyInvite],
}

pub fn invite_settings_page(page: &InviteSettingsPage<'_>) -> String {
    let content = format!(
        r##"
        <main class="min-w-0 flex-1 overflow-y-auto bg-base-100 p-6 lg:p-10">
            <div class="mx-auto max-w-4xl">
                <div class="mb-6">
                    <h1 class="text-2xl font-bold">My Invites</h1>
                    <p class="mt-1 text-sm opacity-60">Company invitations sent to {email}</p>
                </div>
                <div id="user-invites-list" class="space-y-3">{invites}</div>
            </div>
        </main>
        "##,
        email = escape_html_text(page.user.email),
        invites = invite_settings_list(page.invites),
    );

    ui_shell(&UiShell {
        title: "My Invites",
        user: page.user,
        company: page.company,
        section: UiSection::Invites,
        content: &content,
        script: "",
    })
}

pub fn invite_settings_list(invites: &[CompanyInvite]) -> String {
    if invites.is_empty() {
        return r##"<div class="rounded-box border border-dashed border-base-300 p-8 text-center text-sm opacity-60">You have no pending or past company invitations.</div>"##.to_string();
    }

    invites.iter().map(invite_settings_row).collect()
}

pub fn invite_settings_row(invite: &CompanyInvite) -> String {
    let company_name =
        escape_html_text(invite.company_name.as_deref().unwrap_or("Unknown Company"));
    let email = escape_html_text(&invite.email);
    let created_at = format_date(invite.created_at);
    let actions = match invite.status.as_str() {
        "accepted" => r#"<span class="badge badge-success">Accepted</span>"#.to_string(),
        "declined" => r#"<span class="badge badge-error">Declined</span>"#.to_string(),
        _ => format!(
            r##"<button class="btn btn-success btn-sm" hx-post="/ui/invites/{id}/accept" hx-target="#user-invite-{id}" hx-swap="outerHTML">Accept</button>
                <button class="btn btn-error btn-outline btn-sm" hx-post="/ui/invites/{id}/decline" hx-target="#user-invite-{id}" hx-swap="outerHTML">Decline</button>"##,
            id = invite.id,
        ),
    };

    format!(
        r##"<article id="user-invite-{id}" class="card border border-base-300 bg-base-200 shadow-sm">
            <div class="card-body flex-row items-center justify-between gap-4 p-5">
                <div class="min-w-0">
                    <h2 class="truncate font-semibold">{company_name}</h2>
                    <p class="mt-1 text-xs opacity-60">Invited to {email} on {created_at}</p>
                </div>
                <div class="flex shrink-0 gap-2">{actions}</div>
            </div>
        </article>"##,
        id = invite.id,
    )
}
