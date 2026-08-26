//! The Team tab of the `/ui` Companies workspace: the selected company's people — who has
//! joined, and who has been invited but has not — drawn inside that company's own pane rather
//! than in a workspace of their own.
//!
//! The list column picks a person and their pane is swapped into `#team-pane` over htmx, the way
//! picking a channel swaps the Channels pane. Every write re-renders the pane and sends the list
//! along out of band, so an invite, a rename or a removal shows up immediately.

use super::*;

/// What the signed-in user may do with this company's team.
///
/// Only the owner can invite, edit an invite or remove a member — [`CompanyInviteUseCases`]
/// rejects everyone else — so the pane renders those actions only for them rather than offering
/// buttons the server will refuse.
///
/// [`CompanyInviteUseCases`]: crate::use_cases::company_invite::CompanyInviteUseCases
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamRole {
    Owner,
    Member,
}

impl TeamRole {
    /// The owner's view is the only one with anything to manage.
    pub fn manages(self) -> bool {
        self == TeamRole::Owner
    }
}

/// Which sidebar entry is open, if any.
///
/// A member and an invite are different things keyed by different ids — a member by the user it
/// is, an invite by the invite it is — so the selection is one enum rather than two `Option`s
/// that could both be set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TeamSelection {
    #[default]
    None,
    /// Keyed by the member's `user_id`, which is what removing one takes.
    Member(Uuid),
    Invite(Uuid),
}

/// The people list in the sidebar — the only part of the workspace a write has to refresh.
pub struct TeamSettingsList<'a> {
    pub company: &'a Company,
    pub members: &'a [CompanyMember],
    pub invites: &'a [CompanyInvite],
    pub selected: TeamSelection,
    pub role: TeamRole,
}

/// The pane for someone who has joined.
pub struct MemberPane<'a> {
    pub company: &'a Company,
    pub member: &'a CompanyMember,
    pub role: TeamRole,
    /// The signed-in user, so the pane can tell "this is you" from "this is a colleague".
    pub viewer_id: Uuid,
    /// What the user last typed in the avatar field, when a save was rejected; `None` shows the
    /// stored URL.
    pub avatar_draft: Option<&'a str>,
    pub error: Option<&'a str>,
}

impl MemberPane<'_> {
    /// Whether this pane is the viewer looking at themselves.
    ///
    /// An avatar is a property of an account, not of a membership, so it is editable in exactly
    /// one pane: your own. Nobody -- owner included -- sets somebody else's picture.
    fn is_self(&self) -> bool {
        self.member.user_id == self.viewer_id
    }

    /// Whether this member can be removed here.
    ///
    /// The owner is the one member nobody can remove — `remove_company_team_member` refuses it —
    /// so their pane shows no button rather than one that always fails.
    fn removable(&self) -> bool {
        self.role.manages() && self.member.user_id != self.company.user_id
    }
}

/// The pane for someone who has been invited.
pub struct InvitePane<'a> {
    pub company: &'a Company,
    pub invite: &'a CompanyInvite,
    pub role: TeamRole,
    /// What the user last submitted when a save was rejected; `None` shows the stored invite.
    pub email_draft: Option<&'a str>,
    pub role_draft: Option<CompanyAccessRole>,
    pub error: Option<&'a str>,
}

/// The pane for an invite that does not exist yet.
pub struct InviteCreatePane<'a> {
    pub company: &'a Company,
    pub email_draft: &'a str,
    pub role_draft: CompanyAccessRole,
    pub error: Option<&'a str>,
}

/// Where an invite stands, as the sidebar and the pane both badge it.
///
/// The status arrives from the database as a string; parsing it here once means the badge, the
/// wording and the ordering cannot disagree about what "pending" means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InviteStatus {
    Pending,
    Accepted,
    Declined,
}

impl InviteStatus {
    fn parse(status: &str) -> Self {
        match status.trim().to_lowercase().as_str() {
            "accepted" => InviteStatus::Accepted,
            "declined" => InviteStatus::Declined,
            _ => InviteStatus::Pending,
        }
    }

    fn label(self) -> &'static str {
        match self {
            InviteStatus::Pending => "Pending",
            InviteStatus::Accepted => "Accepted",
            InviteStatus::Declined => "Declined",
        }
    }

    fn badge_class(self) -> &'static str {
        match self {
            InviteStatus::Pending => "badge-warning",
            InviteStatus::Accepted => "badge-success",
            InviteStatus::Declined => "badge-error",
        }
    }

    /// Pending invites are the ones still waiting on somebody, so they sort to the top.
    fn sort_key(self) -> u8 {
        match self {
            InviteStatus::Pending => 0,
            InviteStatus::Accepted => 1,
            InviteStatus::Declined => 2,
        }
    }
}

/// One team endpoint, as every fragment on the tab addresses it.
///
/// The team is no longer a workspace of its own, so its URLs are nested under the company whose
/// team it is: there is no way to write one without saying whose people are being listed,
/// invited or removed.
fn team_endpoint(company_id: Uuid, rest: &str) -> String {
    format!("/ui/companies/{company_id}/team{rest}")
}

/// Where the address bar points for one team selection: the Companies workspace, with this
/// company open on its Team tab.
pub fn team_url(company_id: Uuid, selected: TeamSelection) -> String {
    let selection = match selected {
        TeamSelection::None => String::new(),
        TeamSelection::Member(user_id) => format!("&member_id={user_id}"),
        TeamSelection::Invite(invite_id) => format!("&invite_id={invite_id}"),
    };

    format!("/ui/companies?company_id={company_id}&tab=team{selection}")
}

/// The address bar for the invite form, which belongs to nobody yet and so is not a selection.
///
/// Sent as the response's own `HX-Push-Url` rather than written onto the button that asks for the
/// form: the form arrives with the list beside it, so the pane and the URL are one answer.
pub fn team_invite_form_url(company_id: Uuid) -> String {
    format!("/ui/companies?company_id={company_id}&tab=team&new=1")
}

/// The Team tab's body: the company's people in a column, whoever is open beside them.
///
/// It is embedded in the company's own pane rather than rendered through [`ui_shell`], so the
/// Companies workspace keeps its sidebar and the reader stays on the company they picked.
///
/// [`ui_shell`]: super::ui_shell
pub fn team_tab(list: &TeamSettingsList<'_>, pane_html: &str) -> String {
    let company_id = list.company.id;
    let invite_button = if list.role.manages() {
        format!(
            r##"
                    <div class="border-t border-base-300 p-2">
                        <button type="button" class="btn btn-primary btn-sm btn-block justify-start"
                            hx-get="{new_endpoint}"
                            hx-target="#team-pane" hx-swap="outerHTML">{plus_glyph} Invite Person</button>
                    </div>
            "##,
            new_endpoint = team_endpoint(company_id, "/new"),
            plus_glyph = icon(Icon::Plus, BUTTON_ICON),
        )
    } else {
        String::new()
    };

    format!(
        r##"
            <div class="flex min-h-0 flex-1">
                <div class="flex min-h-0 w-64 shrink-0 flex-col border-r border-base-300 bg-base-200">
                    {list_html}
                    {invite_button}
                </div>
                {pane_html}
            </div>
        "##,
        list_html = team_settings_list(list, FragmentSwap::Inline),
    )
}

/// The tab's list column: everyone who has joined, then everyone who was invited.
///
/// Keyed `#team-menu` so the mailbox's selection highlighting applies here unchanged, and
/// rendered out of band after a write so an invite or a removal shows up without the pane having
/// to reload the whole workspace.
pub fn team_settings_list(list: &TeamSettingsList<'_>, swap: FragmentSwap) -> String {
    let members = if list.members.is_empty() {
        r##"<li class="px-3 py-3 text-xs opacity-60">Nobody has joined yet.</li>"##.to_string()
    } else {
        list.members
            .iter()
            .map(|member| {
                member_entry(
                    list.company,
                    member,
                    list.selected == TeamSelection::Member(member.user_id),
                )
            })
            .collect()
    };

    // Only the owner can see invites at all, so a member's sidebar is the joined list and nothing
    // else — no empty "Invites" heading implying something is being withheld.
    let invites = if list.role.manages() {
        let entries = if list.invites.is_empty() {
            r##"<li class="px-3 py-3 text-xs opacity-60">No invites sent yet.</li>"##.to_string()
        } else {
            sorted_invites(list.invites)
                .into_iter()
                .map(|invite| {
                    invite_entry(
                        list.company.id,
                        invite,
                        list.selected == TeamSelection::Invite(invite.id),
                    )
                })
                .collect()
        };

        format!(r##"<li class="menu-title">Invites</li>{entries}"##)
    } else {
        String::new()
    };

    format!(
        r##"<ul id="team-menu" class="menu w-full flex-1 flex-nowrap gap-1 overflow-y-auto px-2"{oob}>
                <li class="menu-title">Members</li>
                {members}
                {invites}
            </ul>"##,
        oob = swap.oob_attribute(),
    )
}

/// The invites the sidebar shows, pending first, so what still needs an answer is at the top.
fn sorted_invites(invites: &[CompanyInvite]) -> Vec<&CompanyInvite> {
    let mut sorted: Vec<&CompanyInvite> = invites.iter().collect();
    sorted.sort_by_key(|invite| {
        (
            InviteStatus::parse(&invite.status).sort_key(),
            invite.created_at,
        )
    });
    sorted
}

fn member_entry(company: &Company, member: &CompanyMember, selected: bool) -> String {
    let role = if member.user_id == company.user_id {
        "owner"
    } else {
        member.role.as_str()
    };

    format!(
        r##"
                <li>
                    <a class="flex items-center gap-3 {active}"
                        hx-get="{endpoint}"
                        hx-target="#team-pane" hx-swap="outerHTML"
                        hx-push-url="{push_url}"
                        data-action="select-sidebar-item">
                        {avatar}
                        <span class="flex min-w-0 flex-col items-start gap-0.5">
                            <span class="flex w-full items-center gap-2">
                                <span class="min-w-0 truncate">{username}</span>
                                <span class="badge badge-ghost badge-sm shrink-0">{role}</span>
                            </span>
                            <span class="w-full truncate font-mono text-[11px] opacity-60">{email}</span>
                        </span>
                    </a>
                </li>
        "##,
        active = if selected { "menu-active" } else { "" },
        endpoint = team_endpoint(company.id, &format!("/members/{}", member.user_id)),
        push_url = team_url(company.id, TeamSelection::Member(member.user_id)),
        avatar = avatar_bubble(
            member.avatar_url.as_ref(),
            member_name(member),
            AvatarSize::Row
        ),
        username = escape_html_text(member_name(member)),
        role = escape_html_text(role),
        email = escape_html_text(member.email.as_deref().unwrap_or("")),
    )
}

fn invite_entry(company_id: Uuid, invite: &CompanyInvite, selected: bool) -> String {
    let status = InviteStatus::parse(&invite.status);

    format!(
        r##"
                <li>
                    <a class="flex w-full items-center gap-2 {active}"
                        hx-get="{endpoint}"
                        hx-target="#team-pane" hx-swap="outerHTML"
                        hx-push-url="{push_url}"
                        data-action="select-sidebar-item">
                        <span class="min-w-0 truncate font-mono text-[13px]">{email}</span>
                        <span class="badge badge-ghost badge-sm ml-auto shrink-0">{role}</span>
                        <span class="badge {badge} badge-sm shrink-0">{label}</span>
                    </a>
                </li>
        "##,
        active = if selected { "menu-active" } else { "" },
        endpoint = team_endpoint(company_id, &format!("/invites/{}", invite.id)),
        push_url = team_url(company_id, TeamSelection::Invite(invite.id)),
        email = escape_html_text(&invite.email),
        role = invite.role.label(),
        badge = status.badge_class(),
        label = status.label(),
    )
}

/// What to call a member whose account has no username on it.
fn member_name(member: &CompanyMember) -> &str {
    match member.username.as_deref() {
        Some(username) if !username.trim().is_empty() => username,
        _ => member.email.as_deref().unwrap_or("Unknown user"),
    }
}

/// The pane before anyone is picked.
pub fn team_settings_empty_pane(message: &str, swap: FragmentSwap) -> String {
    format!(
        r##"
        <section id="team-pane"{PANE_SKELETON} class="flex flex-1 items-center justify-center bg-base-100 p-8"{oob}>
            <p class="text-center text-sm opacity-60">{message}</p>
        </section>
        "##,
        oob = swap.oob_attribute(),
        message = escape_html_text(message),
    )
}

pub fn member_pane(pane: &MemberPane<'_>) -> String {
    let member = pane.member;
    let is_owner = member.user_id == pane.company.user_id;

    let remove_button = if pane.removable() {
        format!(
            r##"<button type="button" class="btn btn-error btn-outline"
                            hx-delete="{endpoint}"
                            hx-target="#team-pane" hx-swap="outerHTML"
                            hx-confirm="Remove {username} from the {company_name} team? They lose access to its channels and threads."
                            hx-push-url="{cleared_url}">Remove from Team</button>"##,
            endpoint = team_endpoint(pane.company.id, &format!("/members/{}", member.user_id)),
            cleared_url = team_url(pane.company.id, TeamSelection::None),
            username = escape_html_text(member_name(member)),
            company_name = escape_html_text(&pane.company.name),
        )
    } else {
        String::new()
    };

    let footnote = if is_owner {
        "The company owner cannot be removed from their own team."
    } else if pane.role.manages() {
        ""
    } else {
        "Only the company owner can change who is on the team."
    };

    format!(
        r##"
        <section id="team-pane"{PANE_SKELETON} class="flex flex-1 flex-col bg-base-100">
            <div class="flex items-start justify-between gap-3 border-b border-base-300 px-6 py-4">
                <div class="flex min-w-0 items-center gap-3">
                    {avatar}
                    <div class="min-w-0">
                        <h2 class="truncate text-xl font-bold">{username}</h2>
                        <p class="truncate font-mono text-xs opacity-60">{email}</p>
                    </div>
                </div>
                <span class="badge badge-primary shrink-0">{role}</span>
            </div>
            <div class="flex-1 overflow-y-auto px-6 py-4">
                {error_html}
                {avatar_form}
                {access_role_form}
                <dl class="mb-6 grid grid-cols-1 gap-3 sm:grid-cols-2">
                    <div class="rounded-box bg-base-200 px-4 py-3">
                        <dt class="text-[11px] uppercase tracking-wider opacity-60">Joined</dt>
                        <dd class="text-sm font-medium">{joined}</dd>
                    </div>
                    <div class="rounded-box bg-base-200 px-4 py-3">
                        <dt class="text-[11px] uppercase tracking-wider opacity-60">Company</dt>
                        <dd class="truncate text-sm font-medium">{company_name}</dd>
                    </div>
                </dl>
                <p class="mb-4 text-xs opacity-70">A team member is trusted by every channel in this company that has no participant list of its own.</p>
                <div class="flex items-center gap-3 border-t border-base-300 pt-4">
                    <button type="button" class="btn btn-ghost"
                        hx-get="{close_endpoint}"
                        hx-target="#team-pane" hx-swap="outerHTML"
                        hx-push-url="{cleared_url}">Close</button>
                    <div class="ml-auto">{remove_button}</div>
                </div>
                <p class="mt-3 text-[11px] opacity-60">{footnote}</p>
            </div>
        </section>
        "##,
        avatar = avatar_bubble(
            member.avatar_url.as_ref(),
            member_name(member),
            AvatarSize::Header
        ),
        username = escape_html_text(member_name(member)),
        email = escape_html_text(member.email.as_deref().unwrap_or("")),
        role = escape_html_text(if is_owner {
            "owner"
        } else {
            member.role.as_str()
        }),
        error_html = form_error_banner(pane.error),
        avatar_form = avatar_form(pane),
        access_role_form = member_access_role_form(pane),
        joined = super::format_date(member.created_at),
        company_name = escape_html_text(&pane.company.name),
        close_endpoint = team_endpoint(pane.company.id, "/close"),
        cleared_url = team_url(pane.company.id, TeamSelection::None),
    )
}

/// The owner's control for changing a joined user's company access.
fn member_access_role_form(pane: &MemberPane<'_>) -> String {
    if !pane.removable() {
        return String::new();
    }

    format!(
        r##"
                <form class="mb-6 rounded-box bg-base-200 px-4 py-3"
                    hx-put="{endpoint}"
                    hx-target="#team-pane" hx-swap="outerHTML"
                    hx-disabled-elt="find button[type='submit']">
                    <label class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">Access Role</span></div>
                        <select name="role" class="select w-full">{options}</select>
                        <div class="label"><span class="text-[11px] opacity-60">Admins can manage channels, agents and schedules. Members can work in the inbox.</span></div>
                    </label>
                    <button type="submit" class="btn btn-primary btn-sm">
                        <span class="loading loading-spinner loading-xs hidden [.htmx-request_&]:inline-block"></span>
                        <span class="[.htmx-request_&]:hidden">Save Access</span>
                        <span class="hidden [.htmx-request_&]:inline">Saving...</span>
                    </button>
                </form>
        "##,
        endpoint = team_endpoint(
            pane.company.id,
            &format!("/members/{}", pane.member.user_id),
        ),
        options = access_role_options(pane.member.role),
    )
}

fn access_role_options(selected: CompanyAccessRole) -> String {
    CompanyAccessRole::ALL
        .into_iter()
        .map(|role| {
            format!(
                r#"<option value="{}"{}>{}</option>"#,
                role.as_str(),
                if role == selected { " selected" } else { "" },
                role.label(),
            )
        })
        .collect()
}

/// The picture field, shown only in your own pane.
///
/// Picking a file uploads it and swaps the field around what was stored; the button is what
/// attaches the result to the account, and "Remove" then Save is how a picture goes away.
fn avatar_form(pane: &MemberPane<'_>) -> String {
    if !pane.is_self() {
        return String::new();
    }

    // A rejected save re-renders with what was submitted rather than what is stored, so the pane
    // does not appear to have kept a picture it refused.
    let draft = pane
        .avatar_draft
        .and_then(|draft| AvatarUrl::parse(draft).ok().flatten());
    let showing = draft.as_ref().or(pane.member.avatar_url.as_ref());

    format!(
        r##"
                <form class="mb-6 rounded-box bg-base-200 px-4 py-3"
                    hx-put="{endpoint}"
                    hx-target="#team-pane" hx-swap="outerHTML"
                    hx-params="avatar_url"
                    hx-disabled-elt="find button[type='submit']">
                    {picker}
                    <button type="submit" class="btn btn-primary btn-sm">
                        <span class="loading loading-spinner loading-xs hidden [.htmx-request_&]:inline-block"></span>
                        <span class="[.htmx-request_&]:hidden">Save Picture</span>
                        <span class="hidden [.htmx-request_&]:inline">Saving...</span>
                    </button>
                </form>
        "##,
        endpoint = team_endpoint(
            pane.company.id,
            &format!("/members/{}/avatar", pane.member.user_id),
        ),
        picker = avatar_picker(&AvatarPicker {
            field_id: "member-avatar",
            avatar_url: showing,
            name: member_name(pane.member),
            label: "Your Picture",
            error: None,
        }),
    )
}

pub fn invite_pane(pane: &InvitePane<'_>) -> String {
    let invite = pane.invite;
    let status = InviteStatus::parse(&invite.status);
    let email = pane.email_draft.unwrap_or(&invite.email);
    let role = pane.role_draft.unwrap_or(invite.role);
    let company_id = pane.company.id;
    let invite_endpoint = team_endpoint(company_id, &format!("/invites/{}", invite.id));
    let close_endpoint = team_endpoint(company_id, "/close");
    let cleared_url = team_url(company_id, TeamSelection::None);

    // An invite that has already been answered is a record, not a form: re-sending it to another
    // address would silently rewrite what somebody already accepted or declined.
    let body = if status == InviteStatus::Pending && pane.role.manages() {
        format!(
            r##"
                <form hx-put="{invite_endpoint}" hx-target="#team-pane" hx-swap="outerHTML" class="space-y-4">
                    <label class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">Invited Email</span></div>
                        <input type="email" name="email" required value="{email}" placeholder="colleague@example.com"
                            class="input w-full font-mono">
                        <div class="label"><span class="text-[11px] opacity-60">They join the team by accepting this invite from their own account.</span></div>
                    </label>
                    <label class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">Access Role</span></div>
                        <select name="role" class="select w-full">{role_options}</select>
                    </label>
                    <div class="flex items-center gap-3 border-t border-base-300 pt-4">
                        <button type="submit" class="btn btn-primary">
                            <span class="loading loading-spinner loading-sm hidden [.htmx-request_&]:inline-block"></span>
                            <span class="[.htmx-request_&]:hidden">Save Changes</span>
                            <span class="hidden [.htmx-request_&]:inline">Saving...</span>
                        </button>
                        <button type="button" class="btn btn-ghost"
                            hx-get="{close_endpoint}"
                            hx-target="#team-pane" hx-swap="outerHTML"
                            hx-push-url="{cleared_url}">Cancel</button>
                        <button type="button" class="btn btn-error btn-outline ml-auto"
                            hx-delete="{invite_endpoint}"
                            hx-target="#team-pane" hx-swap="outerHTML"
                            hx-confirm="Cancel the invite for {email}?"
                            hx-push-url="{cleared_url}">Cancel Invite</button>
                    </div>
                </form>
            "##,
            email = escape_html_text(email),
            role_options = access_role_options(role),
        )
    } else {
        let delete_button = if pane.role.manages() {
            format!(
                r##"<button type="button" class="btn btn-error btn-outline ml-auto"
                            hx-delete="{invite_endpoint}"
                            hx-target="#team-pane" hx-swap="outerHTML"
                            hx-confirm="Delete the {label} invite for {email}? It only removes the record."
                            hx-push-url="{cleared_url}">Delete Record</button>"##,
                label = status.label().to_lowercase(),
                email = escape_html_text(&invite.email),
            )
        } else {
            String::new()
        };

        format!(
            r##"
                <p class="mb-4 text-sm opacity-70">This invite was already {label}, so its address is fixed. Send a new invite to bring somebody else in.</p>
                <div class="flex items-center gap-3 border-t border-base-300 pt-4">
                    <button type="button" class="btn btn-ghost"
                        hx-get="{close_endpoint}"
                        hx-target="#team-pane" hx-swap="outerHTML"
                        hx-push-url="{cleared_url}">Close</button>
                    {delete_button}
                </div>
            "##,
            label = status.label().to_lowercase(),
        )
    };

    format!(
        r##"
        <section id="team-pane"{PANE_SKELETON} class="flex flex-1 flex-col bg-base-100">
            <div class="flex items-start justify-between gap-3 border-b border-base-300 px-6 py-4">
                <div class="min-w-0">
                    <h2 class="truncate font-mono text-xl font-bold">{stored_email}</h2>
                    <p class="text-xs opacity-60">Invited {created_at} as {access_role}</p>
                </div>
                <span class="badge {badge} shrink-0">{label}</span>
            </div>
            <div class="flex-1 overflow-y-auto px-6 py-4">
                {error_html}
                {body}
            </div>
        </section>
        "##,
        stored_email = escape_html_text(&invite.email),
        created_at = super::format_date(invite.created_at),
        access_role = invite.role.label(),
        badge = status.badge_class(),
        label = status.label(),
        error_html = form_error_banner(pane.error),
    )
}

pub fn invite_create_pane(pane: &InviteCreatePane<'_>) -> String {
    format!(
        r##"
        <section id="team-pane"{PANE_SKELETON} class="flex flex-1 flex-col bg-base-100">
            <div class="border-b border-base-300 px-6 py-4">
                <h2 class="text-xl font-bold">Invite someone to {company_name}</h2>
                <p class="text-xs opacity-70">They join by accepting the invite from their own account, and are then trusted by every channel with no participant list of its own.</p>
            </div>
            <div class="flex-1 overflow-y-auto px-6 py-4">
                {error_html}
                <form class="space-y-4" hx-post="{invites_endpoint}" hx-target="#team-pane" hx-swap="outerHTML"
                    hx-disabled-elt="find button[type='submit']">
                    <label class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">Email Address</span></div>
                        <input type="email" name="email" required value="{email}" placeholder="colleague@example.com"
                            class="input w-full font-mono">
                    </label>
                    <label class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">Access Role</span></div>
                        <select name="role" class="select w-full">{role_options}</select>
                        <div class="label"><span class="text-[11px] opacity-60">Choose the access they receive as soon as they accept.</span></div>
                    </label>
                    <div class="flex items-center gap-3">
                        <button type="submit" class="btn btn-primary">
                            <span class="loading loading-spinner loading-sm hidden [.htmx-request_&]:inline-block"></span>
                            <span class="[.htmx-request_&]:hidden">Send Invite</span>
                            <span class="hidden [.htmx-request_&]:inline">Sending...</span>
                        </button>
                        <button type="button" class="btn btn-ghost"
                            hx-get="{close_endpoint}"
                            hx-target="#team-pane" hx-swap="outerHTML"
                            hx-push-url="{cleared_url}">Cancel</button>
                    </div>
                </form>
            </div>
        </section>
        "##,
        company_name = escape_html_text(&pane.company.name),
        invites_endpoint = team_endpoint(pane.company.id, "/invites"),
        close_endpoint = team_endpoint(pane.company.id, "/close"),
        cleared_url = team_url(pane.company.id, TeamSelection::None),
        error_html = form_error_banner(pane.error),
        email = escape_html_text(pane.email_draft),
        role_options = access_role_options(pane.role_draft),
    )
}
