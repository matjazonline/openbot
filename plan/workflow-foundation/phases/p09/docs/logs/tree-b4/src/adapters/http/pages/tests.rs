use super::*;
use crate::entities::correlation::CorrelationId;
use crate::entities::delivery::{DeliveryPartEntry, DeliveryQuery};
use crate::entities::internal_note::{InternalNoteProvenance, InternalNoteView};
use crate::entities::message::CanonicalMessageId;
use crate::entities::message_view::{AuthorView, ExternalMessageRef};
use crate::entities::notification::NotificationPreferences;
use crate::entities::transport::{
    ChannelBindingId, DeliveryId, DeliveryPartId, ExternalMessageKey, FailureClass, PrincipalId,
};
use crate::use_cases::thread::test_support::{EmailMessageDraft, stored_email_view};
use crate::use_cases::user::LoginMethods;
use chrono::Utc;
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
fn legacy_agent_fields_and_confirmation_attribute_are_escaped() {
    let company = mailbox_company();
    let agent = Agent {
        system_prompt: Some("Answer <unsafe> input".into()),
        ..settings_agent(
            company.id,
            r#"Triage" autofocus onfocus="alert(1)"#,
            "triage<script>",
        )
    };

    let row = agent_row_fragment(&company, &agent);
    let edit = agent_edit_fragment(&company, &agent);
    for html in [&row, &edit] {
        assert!(!html.contains("<script>alert(1)</script>"));
        assert!(!html.contains("autofocus onfocus=\"alert(1)"));
    }
    assert!(row.contains("Triage&quot; autofocus"));
    assert!(edit.contains("Answer &lt;unsafe&gt; input"));
}

#[test]
fn simulation_attribute_values_and_message_ids_are_escaped() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let html = channel_simulation_page(
        &company,
        "example.com",
        &channel,
        r#"sender@example.com" data-injected="yes"#,
        None,
        None,
    );
    assert!(!html.contains(r#"data-injected="yes""#));
    assert!(html.contains("data-server-sender=\"sender@example.com&quot;"));

    let thread = mailbox_thread(channel.id);
    let message = stored_email_view(EmailMessageDraft {
        subject: "<id><script>alert(1)</script>".into(),
        ..mailbox_message_draft(thread.id, "body")
    });
    let rendered = channel_simulation_loaded_thread_fragment(&SimulationThreadView {
        company: &company,
        channel: &channel,
        app_domain_name: "example.com",
        thread: &thread,
        messages: &[message],
        reply_context: Some(&EmailReplyContext {
            canonical_id: CanonicalMessageId::random(),
            author_email: None,
            rfc_message_id: Some("<id><script>alert(1)</script>".into()),
            references: Vec::new(),
            cc: Vec::new(),
        }),
        outcomes: &[],
        include_oob: false,
    });
    assert!(
        !rendered.contains("<script>alert(1)</script>"),
        "{rendered}"
    );
    assert!(rendered.contains("&lt;id&gt;&lt;script&gt;alert(1)&lt;/script&gt;"));
}

#[test]
fn library_agent_fields_reuse_the_advanced_editor_and_avatar_picker() {
    let html = library_agent_fields(
        &AgentDraft {
            name: "Global helper",
            advanced: true,
            ..AgentDraft::default()
        },
        None,
    );

    assert!(html.contains("Agent Picture"));
    assert!(html.contains("/ui/uploads/avatar"));
    assert!(html.contains("/ui/agent-library/generate-prompt"));
    assert!(html.contains("Capabilities &amp; advanced options"));
    assert!(!html.contains("Overrides the company key"));
}

/// None of these fixtures has a task in flight; the activity states have their own tests below.
fn no_activity() -> &'static HashMap<Uuid, ThreadActivity> {
    static EMPTY: std::sync::OnceLock<HashMap<Uuid, ThreadActivity>> = std::sync::OnceLock::new();
    EMPTY.get_or_init(HashMap::new)
}

/// The domain the fixtures' channel addresses are built on, so a participant that is another
/// channel is distinguishable from one that is a person.
const MAILBOX_APP_DOMAIN: &str = "mailagents.test";

fn mailbox_company() -> Company {
    Company {
        channel_defaults: Default::default(),
        id: Uuid::new_v4(),
        user_id: Uuid::new_v4(),
        name: "Acme".to_string(),
        slug: "acme".into(),
        avatar_url: None,
        created_at: Utc::now(),
    }
}

fn mailbox_channel(company_id: Uuid) -> Channel {
    Channel {
        enabled: true,
        add_3rd_party: true,
        id: Uuid::new_v4(),
        company_id,
        name: "Inbox".to_string(),
        description: None,
        slug: "inbox".into(),
        alias_slugs: Vec::new(),
        participant_emails: Some(vec!["person@example.com".into()]),
        access_mode: crate::entities::channel::ChannelAccessMode::Allowlist,
        principal_grants: Vec::new(),
        created_by: crate::entities::creation::CreationProvenance::system(),
        created_at: Utc::now(),
        settings_revision: 0,
    }
}

fn mailbox_thread(channel_id: Uuid) -> Thread {
    Thread {
        id: Uuid::new_v4(),
        channel_id,
        subject: "Question <script>".to_string(),
        participant_principal_ids: Vec::new(),
        participant_projection: crate::entities::thread::ThreadParticipantProjection {
            identities: vec![
                crate::adapters::protocols::email::EmailIdentity::parse(
                    "person@example.com".into(),
                )
                .unwrap()
                .qualify_default(),
            ],
        },
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn mailbox_account_email() -> EmailAddress {
    EmailAddress::from("dana@example.com")
}

fn mailbox_user(email: &EmailAddress) -> MailboxUser<'_> {
    MailboxUser {
        id: Uuid::new_v4(),
        username: "dana",
        email,
        avatar_url: None,
        is_operator: false,
        company_membership: CompanyMembership::Owner,
    }
}

#[test]
fn mailbox_page_renders_three_columns_and_escapes_thread_subjects() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let thread = mailbox_thread(channel.id);
    let detail = empty_detail_pane("Select a thread.", FragmentSwap::Inline);
    let email = mailbox_account_email();

    let html = mailbox_page(&MailboxPage {
        user: &mailbox_user(&email),
        company: &company,
        companies: std::slice::from_ref(&company),
        app_domain_name: "example.com",
        channels: std::slice::from_ref(&channel),
        selected_channel: Some(&channel),
        threads: std::slice::from_ref(&thread),
        next_cursor: Some("next_cursor"),
        selected_thread_id: None,
        activity: no_activity(),
        detail_html: &detail,
    });

    assert!(html.contains("/assets/app.css"));
    assert!(html.contains("id=\"channel-menu\""));
    assert!(html.contains("id=\"thread-column\""));
    assert!(html.contains("id=\"detail-pane\""));
    assert!(html.contains("Question &lt;script&gt;"));
    assert!(html.contains(&format!("thread_id={}", thread.id)));
    assert!(html.contains("/ui/threads/list?company_id="));
    assert!(html.contains("cursor=next_cursor"));
    assert!(html.contains("hx-swap=\"beforeend\""));
}

#[test]
fn top_bar_carries_a_theme_controller_that_survives_a_reload() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let detail = empty_detail_pane("Select a thread.", FragmentSwap::Inline);
    let email = EmailAddress::from("dana@example.com");
    let user = MailboxUser {
        id: Uuid::new_v4(),
        username: "dana",
        email: &email,
        avatar_url: None,
        is_operator: false,
        company_membership: CompanyMembership::Owner,
    };

    let html = mailbox_page(&MailboxPage {
        user: &user,
        company: &company,
        companies: std::slice::from_ref(&company),
        app_domain_name: "example.com",
        channels: std::slice::from_ref(&channel),
        selected_channel: Some(&channel),
        threads: &[],
        next_cursor: None,
        selected_thread_id: None,
        activity: no_activity(),
        detail_html: &detail,
    });

    // daisyUI switches the theme off the checkbox alone -- it matches
    // `:root:has(input.theme-controller[value=light]:checked)`, so both the class and the value
    // are load-bearing, not decoration.
    assert!(html.contains(r##"type="checkbox" class="theme-controller" value="light""##));
    assert!(html.contains("swap-rotate"));

    // The choice has to outlive the response: written on change, re-applied before the next paint.
    let script = application_javascript();
    assert!(script.contains("localStorage.setItem('ui_theme', theme)"));
    assert!(theme_init_javascript().contains("localStorage.getItem('ui_theme')"));

    // The restore runs in `<head>`, ahead of the body, or a light-theme reload flashes dark.
    let head = &html[..html.find("<body").expect("a body")];
    assert!(head.contains("/assets/theme-init.js"));

    // ...and the box is caught up with whatever that restore chose.
    assert!(script.contains("syncThemeController();"));

    // Every `/ui` page shares the bar, so the switch is on all of them.
    assert!(mailbox_no_company_page(&user).contains(r##"class="theme-controller""##));
}

/// daisyUI 5 dropped a set of class names that daisyUI 4 needed, and dropping them is silent:
/// the markup keeps the class, the stylesheet has nothing to match, and the field just renders
/// without whatever the class used to add. Nothing catches that at build time -- these pages are
/// format strings, so a dead class is only ever a dead substring -- which is why this reads the
/// page sources back rather than any rendered output.
///
/// `-bordered` is now the default on every field, and `label`
/// styles its own text, so all of these were removed rather than replaced.
#[test]
fn no_page_reaches_for_a_class_daisyui_5_removed() {
    const REMOVED_IN_V5: &[&str] = &[
        "input-bordered",
        "select-bordered",
        "textarea-bordered",
        "file-input-bordered",
        "label-text",
    ];

    let pages = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/adapters/http/pages");
    let mut checked = 0;

    for entry in std::fs::read_dir(&pages).expect("the pages directory") {
        let path = entry.expect("a directory entry").path();
        if path.extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("a page source");
        // This file names them all, and is the one place that is allowed to.
        if path.file_name().is_some_and(|name| name == "tests.rs") {
            continue;
        }
        for dead in REMOVED_IN_V5 {
            assert!(
                !source.contains(dead),
                "{} still uses `{dead}`, which daisyUI 5 does not define",
                path.display(),
            );
        }
        checked += 1;
    }

    // A path that silently reads nothing would pass every assertion above.
    assert!(checked > 10, "only {checked} page sources were read");
}

#[test]
fn dark_theme_recuts_the_blues_at_the_logos_hue() {
    let email = EmailAddress::from("dana@example.com");
    let user = MailboxUser {
        id: Uuid::new_v4(),
        username: "dana",
        email: &email,
        avatar_url: None,
        is_operator: false,
        company_membership: CompanyMembership::None,
    };

    let html = mailbox_no_company_page(&user);

    // The override has to reach the browser after daisyUI's own themes, or it loses the cascade.
    let head = &html[..html.find("<body").expect("a body")];
    let themes = head.find("/assets/app.css").expect("vendored themes");
    let override_at = head.find("--color-primary:").expect("a primary override");
    assert!(themes < override_at);

    // Scoped to dark: the light theme shows these colours over white and never had the problem.
    assert!(head.contains(r##"[data-theme="dark"] {"##));

    // Hue 264 is `#0000ff` -- the wordmark's blue -- and 50% sits below daisyUI's stock 58%.
    assert!(head.contains("--color-primary: oklch(50% 0.19 264.05);"));
    assert!(head.contains("--color-info: oklch(66% 0.13 232.661);"));
}

#[test]
fn top_bar_shows_the_logo_and_the_signed_in_account() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let detail = empty_detail_pane("Select a thread.", FragmentSwap::Inline);
    let email = EmailAddress::from("dana<script>@example.com");
    let user = MailboxUser {
        id: Uuid::new_v4(),
        username: "dana",
        email: &email,
        avatar_url: None,
        is_operator: false,
        company_membership: CompanyMembership::Owner,
    };

    let html = mailbox_page(&MailboxPage {
        user: &user,
        company: &company,
        companies: std::slice::from_ref(&company),
        app_domain_name: "example.com",
        channels: std::slice::from_ref(&channel),
        selected_channel: Some(&channel),
        threads: &[],
        next_cursor: None,
        selected_thread_id: None,
        activity: no_activity(),
        detail_html: &detail,
    });

    // Both inks ship on every page; CSS picks the one the current theme can be read against.
    assert!(html.contains("/assets/busybots-logo-dark-hor.png"));
    assert!(html.contains("/assets/busybots-logo-light.png"));
    assert!(html.contains(r##"[data-theme="light"] .brand-logo-on-light { display: block; }"##));
    assert!(html.contains("dana&lt;script&gt;@example.com"));
    assert!(html.contains(">D</span>"));
    // Logging out goes through the confirmation dialog, not a bare post.
    assert!(html.contains(r##"data-action="confirm-logout""##));
    assert!(html.contains(r##"<dialog id="logout-modal" class="modal">"##));
    assert!(html.contains(r##"<form method="post" action="/logout">"##));

    // The top bar owns the whole width: the columns start below it, not beside it.
    assert!(html.contains(r##"<div class="app-shell flex flex-col">"##));

    // A user with no companies still gets the same bar, since it is their only way out.
    let no_company = mailbox_no_company_page(&user);
    assert!(no_company.contains("/assets/busybots-logo-dark-hor.png"));
    assert!(no_company.contains("Log out"));
}

/// The compact layout is driven entirely by markers the workspaces put on their own columns, so
/// what these assert is the contract between a page and the shell -- not how the shell draws it.
#[test]
fn a_workspace_names_its_list_and_detail_columns_for_the_compact_layout() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let email = mailbox_account_email();
    let user = mailbox_user(&email);
    let html = mailbox_page(&MailboxPage {
        user: &user,
        company: &company,
        companies: std::slice::from_ref(&company),
        app_domain_name: "mailagents.com",
        channels: std::slice::from_ref(&channel),
        selected_channel: Some(&channel),
        threads: &[],
        next_cursor: None,
        selected_thread_id: None,
        activity: no_activity(),
        detail_html: &empty_detail_pane("Select a channel to get started.", FragmentSwap::Inline),
    });

    assert!(html.contains(r##"class="ui-pane-list"##));
    assert!(html.contains(r##"class="ui-pane-detail"##));
    // The drawer and the way back out of a detail both live in the one top bar, so no workspace
    // has to grow a phone-only control of its own.
    assert!(html.contains(r##"data-action="toggle-rail""##));
    assert!(html.contains(r##"data-action="pane-back""##));
    assert!(html.contains(r##"id="rail-backdrop""##));
    // The rail's glyphs gain names when it is a drawer rather than a column.
    assert!(html.contains(r##"<span class="rail-label">Mailbox</span>"##));
}

/// Which column a phone opens on is decided by whether the detail column has anything in it, and
/// that is the server's answer rather than the browser's guess.
#[test]
fn a_detail_column_with_nothing_open_says_so() {
    let placeholder = empty_detail_pane("Select a channel to get started.", FragmentSwap::Inline);
    assert!(placeholder.contains("data-pane-empty"));

    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let thread = mailbox_thread(channel.id);
    let email = mailbox_account_email();
    let occupied = message_pane(&MessagePane {
        company_id: company.id,
        channel: &channel,
        thread: &thread,
        messages: &[],
        viewer_email: &email,
        activity: &Default::default(),
        error: None,
        viewer_membership: CompanyMembership::Owner,
    });
    assert!(!occupied.contains("data-pane-empty"));
}

#[test]
fn compose_button_lives_in_the_thread_column_of_the_selected_channel() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let detail = empty_detail_pane("Select a channel to get started.", FragmentSwap::Inline);
    let email = mailbox_account_email();

    let without_channel = mailbox_page(&MailboxPage {
        user: &mailbox_user(&email),
        company: &company,
        companies: std::slice::from_ref(&company),
        app_domain_name: "example.com",
        channels: std::slice::from_ref(&channel),
        selected_channel: None,
        threads: &[],
        next_cursor: None,
        selected_thread_id: None,
        activity: no_activity(),
        detail_html: &detail,
    });
    // Without a channel there is no thread column, so there is nowhere for Compose to sit.
    assert!(!without_channel.contains("id=\"compose-button\""));
    assert!(without_channel.contains("Select a channel to see its threads."));

    let with_channel = mailbox_page(&MailboxPage {
        user: &mailbox_user(&email),
        company: &company,
        companies: std::slice::from_ref(&company),
        app_domain_name: "example.com",
        channels: std::slice::from_ref(&channel),
        selected_channel: Some(&channel),
        threads: &[],
        next_cursor: None,
        selected_thread_id: None,
        activity: no_activity(),
        detail_html: &detail,
    });
    assert!(with_channel.contains("id=\"compose-button\""));
    assert!(with_channel.contains(&format!(
        "/ui/compose?company_id={}&channel_id={}",
        company.id, channel.id
    )));
    // The button must target the detail pane, or it swaps into its own column.
    assert!(with_channel.contains("hx-target=\"#detail-pane\""));

    // It rides along with the column, so picking a channel over htmx brings it in too.
    let column = thread_column(&ThreadColumn {
        company_id: company.id,
        channel: &channel,
        app_domain_name: MAILBOX_APP_DOMAIN,
        threads: &[],
        next_cursor: None,
        selected_thread_id: None,
        activity: no_activity(),
    });
    assert!(column.contains("id=\"compose-button\""));
}

#[test]
fn channel_sidebar_lists_addresses_and_offers_channel_actions() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let detail = empty_detail_pane("Select a thread.", FragmentSwap::Inline);
    let email = mailbox_account_email();
    let page = |selected| {
        mailbox_page(&MailboxPage {
            user: &mailbox_user(&email),
            company: &company,
            companies: std::slice::from_ref(&company),
            app_domain_name: "example.com",
            channels: std::slice::from_ref(&channel),
            selected_channel: selected,
            threads: &[],
            next_cursor: None,
            selected_thread_id: None,
            activity: no_activity(),
            detail_html: &detail,
        })
    };

    let unselected = page(None);
    assert!(unselected.contains("inbox@acme.example.com"));
    // Channel management is the /ui Channels workspace now, not the classic page.
    assert!(unselected.contains(&format!("/ui/channels?company_id={}&new=1", company.id)));
    assert!(!unselected.contains("Edit Channel"));

    let selected = page(Some(&channel));
    assert!(selected.contains("Edit Channel"));
    assert!(selected.contains(&format!(
        "/ui/channels?company_id={}&channel_id={}",
        company.id, channel.id
    )));
}

#[test]
fn channel_actions_swap_out_of_band_so_picking_a_channel_reveals_edit_channel() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);

    // The sidebar itself is never swapped when a channel is picked, so the thread-column
    // response has to carry this block out-of-band or "Edit Channel" never appears.
    let actions = channel_actions(company.id, Some(&channel), FragmentSwap::OutOfBand);
    assert!(actions.contains("id=\"channel-actions\""));
    assert!(actions.contains("hx-swap-oob=\"outerHTML\""));
    assert!(actions.contains(&format!(
        "/ui/channels?company_id={}&channel_id={}",
        company.id, channel.id
    )));

    let inline = channel_actions(company.id, None, FragmentSwap::Inline);
    assert!(!inline.contains("hx-swap-oob"));
    assert!(!inline.contains("Edit Channel"));
}

#[test]
fn the_rail_ends_on_the_company_it_is_scoped_to_rather_than_on_a_way_out() {
    let mut company = mailbox_company();
    company.name = "Acme <script>".to_string();
    let channel = mailbox_channel(company.id);
    let detail = empty_detail_pane("Select a thread.", FragmentSwap::Inline);
    let email = mailbox_account_email();
    let page = |company: &Company| {
        mailbox_page(&MailboxPage {
            user: &mailbox_user(&email),
            company,
            companies: std::slice::from_ref(company),
            app_domain_name: "example.com",
            channels: std::slice::from_ref(&channel),
            selected_channel: Some(&channel),
            threads: &[],
            next_cursor: None,
            selected_thread_id: None,
            activity: no_activity(),
            detail_html: &detail,
        })
    };

    let letter = page(&company);

    // The foot of the rail says which company you are in, and opens its settings.
    assert!(letter.contains(&format!(
        r##"<a id="rail-company" href="/ui/companies?company_id={}""##,
        company.id
    )));
    assert!(letter.contains(r##"title="Acme &lt;script&gt;""##));
    // No picture yet, so the letter shows -- and no `<img>` is left to break.
    assert!(letter.contains(">A</span>"));
    assert!(!letter.contains("cdn.example.com"));

    // Signing out is not what the bottom of the rail does any more; the account menu owns it.
    assert!(!letter.contains(r##"title="Log out""##));
    assert!(letter.contains(r##"data-action="confirm-logout""##));

    company.avatar_url = Some(AvatarUrl::from("https://cdn.example.com/acme.png"));
    let pictured = page(&company);
    assert!(pictured.contains(r##"src="https://cdn.example.com/acme.png""##));
    // The letter stays underneath, so a picture that fails to load leaves the bubble filled.
    assert!(pictured.contains(">A</span>"));
}

#[test]
fn the_rail_badge_can_be_sent_back_out_of_band_after_a_company_is_saved() {
    let mut company = mailbox_company();
    company.avatar_url = Some(AvatarUrl::from("https://cdn.example.com/acme.png"));

    let inline = rail_company_badge(&company, FragmentSwap::Inline);
    assert!(!inline.contains("hx-swap-oob"));

    // The rail is chrome no pane swap touches, so a saved picture has to ride back on its own.
    let oob = rail_company_badge(&company, FragmentSwap::OutOfBand);
    assert!(oob.contains(r##"id="rail-company""##));
    assert!(oob.contains(r##"hx-swap-oob="outerHTML""##));
}

#[test]
fn every_workspace_rail_link_closes_live_streams_before_navigation() {
    let company = mailbox_company();
    let email = mailbox_account_email();
    let user = mailbox_user(&email);
    let html = ui_shell(&UiShell {
        title: "Mailbox",
        user: &user,
        company: Some(&company),
        section: UiSection::Mailbox,
        content: "",
    });
    let rail = html
        .split_once(r#"<nav id="app-rail""#)
        .expect("shell has the workspace rail")
        .1
        .split_once("</nav>")
        .expect("workspace rail is closed")
        .0;

    assert!(rail.contains("href="));
    assert_eq!(
        rail.matches("href=").count(),
        rail.matches(r#"data-action="navigate-workspace""#).count(),
        "every full-page link in the workspace rail must release SSE connections"
    );
}

#[test]
fn workspace_navigation_cleanup_uses_the_htmx_sse_extension_lifecycle() {
    let script = application_javascript();

    assert!(script.contains("window.addEventListener('click'"));
    assert!(script.contains("event.target.closest('a[href]')"));
    assert!(script.contains("if (link) closeLiveStreamsForNavigation(event, link);"));
    assert!(script.contains("document.querySelectorAll('[sse-connect], [data-sse-connect]')"));
    assert!(script.contains("window.htmx.trigger(owner, 'htmx:beforeCleanupElement')"));
    assert!(script.contains("event.defaultPrevented || event.button !== 0"));
    assert!(script.contains("event.metaKey || event.ctrlKey || event.shiftKey || event.altKey"));
    assert!(script.contains("target.toLowerCase() !== '_self'"));
}

#[test]
fn the_top_bar_names_the_selected_company_between_the_brand_and_the_account() {
    let mut company = mailbox_company();
    company.name = "Acme Logistics".to_string();
    let email = mailbox_account_email();
    let user = mailbox_user(&email);

    let bar = ui_shell(&UiShell {
        title: "Mailbox",
        user: &user,
        company: Some(&company),
        section: UiSection::Mailbox,
        content: "",
    });
    assert!(bar.contains(r##"id="topbar-company""##));
    assert!(bar.contains("Acme Logistics"));

    // A reader with no company yet has nothing to name, so the middle of the bar stays empty
    // rather than holding a placeholder.
    let bare = ui_shell(&UiShell {
        title: "Profile",
        user: &user,
        company: None,
        section: UiSection::Mailbox,
        content: "",
    });
    assert!(!bare.contains(r##"id="topbar-company""##));

    // A rename reaches no pane swap, so the name rides back out of band beside the rail badge.
    let oob = topbar_company(&company, FragmentSwap::OutOfBand);
    assert!(oob.contains(r##"hx-swap-oob="outerHTML""##));
    assert!(!topbar_company(&company, FragmentSwap::Inline).contains("hx-swap-oob"));
}

#[test]
fn the_ui_shell_reports_live_update_interruptions_without_replacing_sse_retries() {
    let email = mailbox_account_email();
    let user = mailbox_user(&email);
    let html = ui_shell(&UiShell {
        title: "Mailbox",
        user: &user,
        company: None,
        section: UiSection::Mailbox,
        content: "",
    });

    assert!(html.contains(r#"id="live-update-status" role="status" aria-live="polite""#));
    assert!(html.contains("alert alert-warning"));
    let script = application_javascript();
    assert!(script.contains("status.classList.toggle('alert-success', restored)"));
    assert!(html.contains("Live updates paused. Reconnecting&hellip;"));
    assert!(script.contains("htmx:sseError"));
    assert!(script.contains("htmx:sseOpen"));
    assert!(script.contains("Live updates restored."));
    assert!(!script.contains("new EventSource"));
    assert!(!script.contains("data.get('api_key')"));

    // Every `/ui` page carries one place for a failed request to be reported, because htmx does
    // not swap a non-2xx answer and would otherwise leave the click looking like a no-op.
    assert!(html.contains(r#"id="request-error-toast" role="alert" aria-live="assertive""#));
    assert!(html.contains(r#"id="request-error-message""#));
    assert!(script.contains("htmx:responseError"));
    assert!(script.contains("htmx:sendError"));
    assert!(script.contains("htmx:timeout"));
    assert!(script.contains("htmx:swapError"));
    assert!(script.contains("case 'dismiss-request-error': hideRequestError(); break;"));

    // Every button that is waiting on the server says so, and the spinner it uses actually turns:
    // daisyUI's own `loading` animation is SMIL inside a `mask-image`, which Chrome never runs.
    assert!(script.contains("htmx:beforeRequest"));
    assert!(script.contains("loading loading-spinner loading-xs"));
    assert!(script.contains("data-request-spinner"));
    assert!(
        html.contains("animation: spinner-rotate"),
        "the shell must redraw the spinner on a real CSS animation"
    );
    assert!(html.contains("@keyframes spinner-rotate"));
    assert!(
        !html.contains(".loading-spinner {{"),
        "the style block is interpolated, not a format literal"
    );
}

#[test]
fn icon_rail_lights_the_workspace_the_response_belongs_to() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let detail = empty_detail_pane("Select a thread.", FragmentSwap::Inline);
    let email = mailbox_account_email();

    let mailbox = mailbox_page(&MailboxPage {
        user: &mailbox_user(&email),
        company: &company,
        companies: std::slice::from_ref(&company),
        app_domain_name: "example.com",
        channels: std::slice::from_ref(&channel),
        selected_channel: Some(&channel),
        threads: &[],
        next_cursor: None,
        selected_thread_id: None,
        activity: no_activity(),
        detail_html: &detail,
    });

    // The rail stays inside /ui: the Channels icon opens the workspace, not the classic page.
    assert!(mailbox.contains(&format!("/ui/channels?company_id={}", company.id)));
    assert!(!mailbox.contains(&format!("href=\"/companies/{}/channels\"", company.id)));
    assert!(mailbox.contains(&format!(
        r##"<a href="/ui?company_id={}" class="btn btn-square btn-md btn-primary"##,
        company.id
    )));

    let list = ChannelSettingsList {
        company: &company,
        app_domain_name: "example.com",
        channels: std::slice::from_ref(&channel),
        selected_channel_id: None,
    };
    let pane = channel_settings_empty_pane("Select a channel.", FragmentSwap::Inline);
    let channels = channel_settings_page(&ChannelSettingsPage {
        user: &mailbox_user(&email),
        companies: std::slice::from_ref(&company),
        list: &list,
        pane_html: &pane,
    });

    // Same chrome, other icon lit. Company selection lives in the Companies workspace.
    assert!(channels.contains(&format!(
        r##"<a href="/ui/channels?company_id={}" class="btn btn-square btn-md btn-primary"##,
        company.id
    )));
    assert!(channels.contains("/assets/busybots-logo-dark-hor.png"));
    assert!(!channels.contains("dropdown-bottom w-full p-3"));
    assert!(channels.contains("id=\"channel-pane\""));

    let agent = settings_agent(company.id, "Triage", "triage");
    let agent_list = AgentSettingsList {
        company: &company,
        app_domain_name: "mail.test",
        agents: std::slice::from_ref(&agent),
        selected_agent_id: None,
    };
    let agent_pane = agent_settings_empty_pane("Select an agent.", FragmentSwap::Inline);
    let agents = agent_settings_page(&AgentSettingsPage {
        user: &mailbox_user(&email),
        companies: std::slice::from_ref(&company),
        list: &agent_list,
        pane_html: &agent_pane,
    });

    // The third workspace, same chrome again: its own icon lit.
    assert!(agents.contains(&format!(
        r##"<a href="/ui/agents?company_id={}" class="btn btn-square btn-md btn-primary"##,
        company.id
    )));
    assert!(!agents.contains("dropdown-bottom w-full p-3"));
    assert!(agents.contains("id=\"agent-pane\""));
    // The rail is shared, so the other two workspaces stay one click away and unlit.
    assert!(agents.contains(&format!(
        r##"<a href="/ui/channels?company_id={}" class="btn btn-square btn-md btn-ghost"##,
        company.id
    )));
}

#[test]
fn the_icon_rail_only_advertises_company_workspaces_the_role_can_open() {
    let company = mailbox_company();
    let email = mailbox_account_email();
    let render = |membership, is_operator| {
        let user = MailboxUser {
            company_membership: membership,
            is_operator,
            ..mailbox_user(&email)
        };
        ui_shell(&UiShell {
            title: "Mailbox",
            user: &user,
            company: Some(&company),
            section: UiSection::Mailbox,
            content: "",
        })
    };
    let link = |path: &str| format!(r#"href="{path}?company_id={}""#, company.id);

    let member = render(CompanyMembership::Member, false);
    assert!(member.contains(&link("/ui")));
    assert!(member.contains(&link("/ui/companies")));
    for path in [
        "/ui/channels",
        "/ui/agents",
        "/ui/schedules",
        "/ui/deliveries",
        "/ui/dashboard",
    ] {
        assert!(!member.contains(&link(path)), "member rail exposed {path}");
    }

    let admin = render(CompanyMembership::Admin, false);
    for path in [
        "/ui",
        "/ui/channels",
        "/ui/agents",
        "/ui/schedules",
        "/ui/deliveries",
        "/ui/dashboard",
        "/ui/companies",
    ] {
        assert!(admin.contains(&link(path)), "admin rail omitted {path}");
    }

    let operator = render(CompanyMembership::None, true);
    assert!(operator.contains(&link("/ui/dashboard")));
    assert!(!operator.contains(&link("/ui")));
    assert!(!operator.contains(r#"id="rail-company""#));
}

#[test]
fn agent_settings_list_targets_the_pane_and_swaps_out_of_band() {
    let company = mailbox_company();
    let agent = Agent {
        provider: Some("openai".to_string()),
        model: Some("gpt-4o".to_string()),
        ..settings_agent(company.id, "Triage", "triage")
    };
    let list = AgentSettingsList {
        company: &company,
        app_domain_name: "mail.test",
        agents: std::slice::from_ref(&agent),
        selected_agent_id: Some(agent.id),
    };

    let inline = agent_settings_list(&list, FragmentSwap::Inline);
    assert!(inline.contains(&format!(
        "/ui/agents/{}?company_id={}",
        agent.id, company.id
    )));
    assert!(inline.contains("hx-target=\"#agent-pane\""));
    assert!(inline.contains("hx-sync=\"#agent-pane:replace\""));
    assert!(inline.contains("@triage"));
    assert!(inline.contains("openai / gpt-4o"));
    assert!(inline.contains("menu-active"));
    assert!(!inline.contains("hx-swap-oob"));

    // After a write the list rides along on the pane's response.
    let oob = agent_settings_list(&list, FragmentSwap::OutOfBand);
    assert!(oob.contains("id=\"agent-menu\""));
    assert!(oob.contains("hx-swap-oob=\"outerHTML\""));

    // An agent with no overrides answers on the company's model, and says so.
    let plain = settings_agent(company.id, "Plain", "plain");
    let defaults = AgentSettingsList {
        agents: std::slice::from_ref(&plain),
        selected_agent_id: None,
        ..list
    };
    assert!(agent_settings_list(&defaults, FragmentSwap::Inline).contains("company default model"));

    let empty = AgentSettingsList {
        agents: &[],
        selected_agent_id: None,
        ..list
    };
    assert!(agent_settings_list(&empty, FragmentSwap::Inline).contains("No agents yet"));
}

#[test]
fn agent_edit_pane_prefills_the_stored_agent_and_offers_delete() {
    let company = mailbox_company();
    let agent = Agent {
        provider: Some("openai".to_string()),
        model: Some("gpt-4o".to_string()),
        system_prompt: Some("Answer <billing> questions.".to_string()),
        description: None,
        ..settings_agent(company.id, "Triage <bot>", "triage")
    };
    let model_connections = [CompanyModelConnection {
        provider: "openai".into(),
        models: vec![ModelName::from("gpt-4o")],
        is_default: true,
        has_api_key: true,
    }];

    let html = agent_edit_pane(&AgentEditPane {
        company: &company,
        app_domain_name: "mail.test",
        model_connections: &model_connections,
        agent: &agent,
        draft: None,
        error: None,
        capability_options: AgentCapabilityOptions::default(),
    });

    assert!(html.contains(&format!(
        "hx-put=\"/ui/agents/{}?company_id={}\"",
        agent.id, company.id
    )));
    assert!(html.contains("value=\"triage\""));
    assert!(html.contains("value=\"openai\""));
    assert!(html.contains("value=\"gpt-4o\""));
    assert!(!html.contains("name=\"run_timeout_secs\""));
    assert!(html.contains("Answer &lt;billing&gt; questions.</textarea>"));
    // Overrides are set, so the collapsed section starts open.
    assert!(html.contains("bg-base-200\" open"));
    // The name is escaped everywhere it appears, including inside the confirm prompt.
    assert!(!html.contains("Triage <bot>"));
    assert!(html.contains("Triage &lt;bot&gt;"));
    assert!(html.contains(&format!(
        "hx-delete=\"/ui/agents/{}?company_id={}\"",
        agent.id, company.id
    )));
    assert!(html.contains("This cannot be undone."));

    // An agent with nothing overridden keeps the model section collapsed.
    let plain = settings_agent(company.id, "Plain", "plain");
    let plain_html = agent_edit_pane(&AgentEditPane {
        company: &company,
        app_domain_name: "mail.test",
        model_connections: &[],
        agent: &plain,
        draft: None,
        error: None,
        capability_options: AgentCapabilityOptions::default(),
    });
    assert!(!plain_html.contains("bg-base-200\" open"));
}

#[test]
fn an_agent_with_a_picture_shows_it_and_one_without_falls_back_to_its_letter() {
    let company = mailbox_company();
    let pictured = Agent {
        avatar_url: Some(AvatarUrl::from("https://example.com/triage.png")),
        ..settings_agent(company.id, "Triage", "triage")
    };
    let plain = settings_agent(company.id, "Plain", "plain");

    let html = agent_edit_pane(&AgentEditPane {
        company: &company,
        app_domain_name: "mail.test",
        model_connections: &[],
        agent: &pictured,
        draft: None,
        error: None,
        capability_options: AgentCapabilityOptions::default(),
    });
    assert!(html.contains(r#"src="https://example.com/triage.png""#));
    // The form round-trips it, so saving from this pane cannot silently clear the picture.
    assert!(html.contains(r#"name="avatar_url" value="https://example.com/triage.png""#));
    // The letter is rendered underneath, for a URL that fails to load.
    assert!(html.contains(">T</span>"));

    let plain_html = agent_edit_pane(&AgentEditPane {
        company: &company,
        app_domain_name: "mail.test",
        model_connections: &[],
        agent: &plain,
        draft: None,
        error: None,
        capability_options: AgentCapabilityOptions::default(),
    });
    assert!(!plain_html.contains("<img"));
    assert!(plain_html.contains(">P</span>"));

    // The sidebar shows the same face as the pane.
    let agents = [pictured];
    let list = agent_settings_list(
        &AgentSettingsList {
            company: &company,
            app_domain_name: "mail.test",
            agents: &agents,
            selected_agent_id: None,
        },
        FragmentSwap::Inline,
    );
    assert!(list.contains(r#"src="https://example.com/triage.png""#));
}

#[test]
fn agent_edit_pane_keeps_a_rejected_save_in_the_form() {
    let company = mailbox_company();
    let agent = settings_agent(company.id, "Triage", "triage");
    let draft = AgentDraft {
        name: "Renamed",
        slug: "renamed",
        system_prompt: "Be terse.",
        advanced: true,
        ..AgentDraft::default()
    };

    let html = agent_edit_pane(&AgentEditPane {
        company: &company,
        app_domain_name: "mail.test",
        model_connections: &[],
        agent: &agent,
        draft: Some(&draft),
        error: Some("Invalid JSON config"),
        capability_options: AgentCapabilityOptions::default(),
    });

    assert!(html.contains("alert alert-error"));
    assert!(html.contains("Invalid JSON config"));
    assert!(html.contains("value=\"Renamed\""));
    assert!(html.contains("Be terse.</textarea>"));
    // The legacy runtime configuration has no control any more (p08 B4a).
    assert!(!html.contains("{ not json</textarea>"));
    // The header still names the stored agent; only the form carries the attempt.
    assert!(html.contains("<h2 class=\"truncate text-xl font-bold\">Triage</h2>"));
}

#[test]
fn prompt_generator_names_the_pane_it_answers_into() {
    let company = mailbox_company();
    let agent = settings_agent(company.id, "Triage", "triage");

    let edit = agent_edit_pane(&AgentEditPane {
        company: &company,
        app_domain_name: "mail.test",
        model_connections: &[],
        agent: &agent,
        draft: None,
        error: None,
        capability_options: AgentCapabilityOptions::default(),
    });

    // The box is namespaced by the agent it belongs to, and pulls the overrides along so an agent
    // pointed at its own model gets its prompt written by that model.
    assert!(edit.contains("Generate with AI"));
    assert!(edit.contains(&format!(
        "hx-post=\"/ui/agents/generate-prompt?company_id={}\"",
        company.id
    )));
    assert!(edit.contains(&format!("id=\"agent-generator-{}\"", agent.id)));
    assert!(edit.contains(&format!("#agent-provider-{}", agent.id)));
    assert!(edit.contains(&format!(r##"hx-vals='{{"id_prefix": "{}"}}'"##, agent.id)));
    assert!(edit.contains(&format!(
        "hx-target=\"#agent-generator-status-{}\"",
        agent.id
    )));

    // The create pane has no agent to name; an absent id is what tells the handler so.
    let create = agent_create_pane(&AgentCreatePane {
        company: &company,
        app_domain_name: "mail.test",
        model_connections: &[],
        library_agents: &[],
        selected_library_agent_ids: &[],
        tab: AgentCreateTab::Simple,
        draft: &AgentDraft::default(),
        error: None,
        capability_options: AgentCapabilityOptions::default(),
    });
    assert!(create.contains("id=\"agent-generator-new\""));
    assert!(create.contains("id=\"agent-prompt-new\""));
    assert!(!create.contains(r##"hx-vals='{"id_prefix""##));
}

#[test]
fn a_generated_prompt_swaps_into_the_field_without_a_script() {
    let agent_id = Uuid::new_v4();
    let html = agent_prompt_generated(
        &agent_id.to_string(),
        "Be brief.</textarea><script>alert(1)</script>",
    );

    assert!(html.contains("Prompt written into the field below."));
    // The prompt rides back as the field itself, swapped out of band...
    assert!(html.contains(&format!("id=\"agent-prompt-{agent_id}\"")));
    assert!(html.contains("hx-swap-oob=\"outerHTML\""));
    assert!(html.contains("name=\"system_prompt\""));
    // ...so nothing a model writes can close the field or open a script.
    assert!(!html.contains("<script"));
    assert!(!html.contains("</textarea><"));
    assert!(html.contains("&lt;script&gt;"));

    let failed = agent_prompt_failed("Prompt generation failed: no API key");
    assert!(failed.contains("no API key"));
    assert!(!failed.contains("hx-swap-oob"));
}

#[test]
fn the_easy_tab_creates_the_picked_library_agents() {
    let company = mailbox_company();
    let triage = Agent {
        company_id: None,
        description: Some("Sorts incoming support mail".into()),
        ..settings_agent(company.id, "Triage", "triage")
    };
    let billing = Agent {
        company_id: None,
        ..settings_agent(company.id, "Billing", "billing")
    };
    let pane = agent_create_pane(&AgentCreatePane {
        company: &company,
        app_domain_name: "mail.test",
        model_connections: &[],
        library_agents: &[triage.clone(), billing.clone()],
        selected_library_agent_ids: &[billing.id],
        tab: AgentCreateTab::Easy,
        draft: &AgentDraft::default(),
        error: None,
        capability_options: AgentCapabilityOptions::default(),
    });

    // The tab opens on the picker, and the other two forms stay behind their tabs.
    assert!(pane.contains(r##"id="agent-tab-easy-btn" class="tab tab-active""##));
    assert!(pane.contains(&format!(
        r##"hx-post="/ui/agents/from-library?company_id={}""##,
        company.id
    )));
    assert!(pane.contains(r##"<form id="agent-tab-simple" class="hidden space-y-4""##));
    assert!(pane.contains(r##"<form id="agent-tab-advanced" class="hidden space-y-4""##));

    // Every definition is offered, and what was already picked comes back ticked.
    assert!(pane.contains("Sorts incoming support mail"));
    assert!(pane.contains(&format!(
        r#"value="{}" class="checkbox checkbox-primary mt-1""#,
        triage.id
    )));
    assert!(!pane.contains(&format!(
        r#"value="{}" class="checkbox checkbox-primary mt-1" checked"#,
        triage.id
    )));
    assert!(pane.contains(&format!(
        r#"value="{}" class="checkbox checkbox-primary mt-1" checked"#,
        billing.id
    )));
    assert!(pane.contains(&format!(
        r#"<input type="hidden" name="library_agent_ids" value="{}">"#,
        billing.id
    )));
}

#[test]
fn an_empty_library_leaves_no_easy_tab_to_open() {
    let company = mailbox_company();
    let pane = agent_create_pane(&AgentCreatePane {
        company: &company,
        app_domain_name: "mail.test",
        model_connections: &[],
        library_agents: &[],
        selected_library_agent_ids: &[],
        tab: AgentCreateTab::Simple,
        draft: &AgentDraft::default(),
        error: None,
        capability_options: AgentCapabilityOptions::default(),
    });

    assert!(!pane.contains("agent-tab-easy"));
    assert!(!pane.contains("/ui/agents/from-library"));
    assert!(!pane.contains(r##"<form id="agent-tab-simple" class="hidden space-y-4""##));
}

#[test]
fn agent_create_pane_opens_on_the_tab_that_was_submitted() {
    let company = mailbox_company();
    let pane = |tab| {
        agent_create_pane(&AgentCreatePane {
            company: &company,
            app_domain_name: "mail.test",
            model_connections: &[],
            library_agents: &[],
            selected_library_agent_ids: &[],
            tab,
            draft: &AgentDraft {
                name: "Triage",
                advanced: tab == AgentCreateTab::Advanced,
                ..AgentDraft::default()
            },
            error: None,
            capability_options: AgentCapabilityOptions::default(),
        })
    };

    let simple = pane(AgentCreateTab::Simple);
    assert!(simple.contains(&format!("hx-post=\"/ui/agents?company_id={}\"", company.id)));
    assert!(simple.contains("name=\"form_mode\" value=\"simple\""));
    assert!(simple.contains("name=\"form_mode\" value=\"advanced\""));
    // An agent has a handle, not an address of its own.
    assert!(simple.contains(">@agent-handle</span>"));
    assert!(!simple.contains("agent-handle@acme.mail.test"));
    assert!(simple.contains("data-input=\"agent-simple-address-preview\""));
    assert!(simple.contains("data-input=\"agent-address-preview\""));
    assert!(simple.contains(r##"<form id="agent-tab-advanced" class="hidden space-y-4""##));
    assert!(!simple.contains(r##"<form id="agent-tab-simple" class="hidden space-y-4""##));

    // p08 D4.7: Advanced creates the agent on submit too; there is no channel step.
    assert_eq!(
        simple
            .matches(&format!("hx-post=\"/ui/agents?company_id={}\"", company.id))
            .count(),
        2
    );
    assert!(!simple.contains("/ui/agents/new/channel"));
    assert!(!simple.contains(">Next: Channel<"));
    assert!(!simple.contains(r##"<li class="step step-primary">Agent</li>"##));

    // A rejected Advanced submit has to come back on the Advanced tab.
    let advanced = pane(AgentCreateTab::Advanced);
    assert!(advanced.contains(r##"<form id="agent-tab-simple" class="hidden space-y-4""##));
    assert!(!advanced.contains(r##"<form id="agent-tab-advanced" class="hidden space-y-4""##));
}

#[test]
fn channel_settings_list_targets_the_pane_and_swaps_out_of_band() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let list = ChannelSettingsList {
        company: &company,
        app_domain_name: "example.com",
        channels: std::slice::from_ref(&channel),
        selected_channel_id: Some(channel.id),
    };

    let inline = channel_settings_list(&list, FragmentSwap::Inline);
    assert!(inline.contains(&format!(
        "/ui/channels/{}?company_id={}",
        channel.id, company.id
    )));
    assert!(inline.contains("hx-target=\"#channel-pane\""));
    assert!(inline.contains("hx-sync=\"#channel-pane:replace\""));
    assert!(inline.contains("inbox@acme.example.com"));
    assert!(inline.contains("menu-active"));
    assert!(!inline.contains("hx-swap-oob"));

    // After a write the list rides along on the pane's response.
    let oob = channel_settings_list(&list, FragmentSwap::OutOfBand);
    assert!(oob.contains("id=\"channel-menu\""));
    assert!(oob.contains("hx-swap-oob=\"outerHTML\""));

    let empty = ChannelSettingsList {
        channels: &[],
        selected_channel_id: None,
        ..list
    };
    assert!(channel_settings_list(&empty, FragmentSwap::Inline).contains("No channels yet"));
}

fn settings_agent(company_id: Uuid, name: &str, slug: &str) -> Agent {
    Agent {
        id: Uuid::new_v4(),
        company_id: Some(company_id),
        name: name.to_string(),
        slug: slug.to_string(),
        provider: None,
        model: None,
        system_prompt: None,
        description: None,
        avatar_url: None,
        created_by: crate::entities::creation::CreationProvenance::system(),
        created_at: Utc::now(),
    }
}

#[test]
fn channel_edit_pane_prefills_the_stored_channel_and_offers_delete() {
    let company = mailbox_company();
    let channel = Channel {
        participant_emails: Some(vec!["person@example.com".into(), "@public".into()]),
        ..mailbox_channel(company.id)
    };

    let html = channel_edit_pane(&ChannelEditPane {
        company: &company,
        app_domain_name: "example.com",
        channel: &channel,
        schedules: &[],
        schedule_bindings: &[],
        spam_scan_enabled: true,
        draft: None,
        error: None,
    });

    assert!(html.contains(&format!(
        "hx-put=\"/ui/channels/{}?company_id={}\"",
        channel.id, company.id
    )));
    assert!(html.contains("value=\"Inbox\""));
    assert!(html.contains("value=\"inbox\""));
    assert!(html.contains("value=\"person@example.com, @public\""));
    assert!(!html.contains("name=\"provider\""));
    assert!(!html.contains("name=\"channel_config\""));

    // What answers the channel is a workflow binding; the pane picks no agent.
    assert!(!html.contains("name=\"agent_ids\""));

    assert!(html.contains(&format!(
        "hx-delete=\"/ui/channels/{}?company_id={}\"",
        channel.id, company.id
    )));
    assert!(html.contains("hx-confirm="));

    // Spam scanning is on, so the interlock has nothing to ask.
    assert!(!html.contains("confirm_spam_disabled"));
}

/// The third-party switch is a checkbox, and an unticked checkbox submits no key at all — so the
/// pane has to render its stored state, or saving an unrelated field would silently flip it.
#[test]
fn channel_edit_pane_reflects_the_stored_third_party_setting() {
    let company = mailbox_company();

    let open = channel_edit_pane(&ChannelEditPane {
        company: &company,
        app_domain_name: "example.com",
        channel: &mailbox_channel(company.id),
        schedules: &[],
        schedule_bindings: &[],
        spam_scan_enabled: true,
        draft: None,
        error: None,
    });
    assert!(open.contains(
        r#"name="add_3rd_party" value="true" class="checkbox checkbox-sm mt-0.5" checked"#
    ));

    let closed_channel = Channel {
        add_3rd_party: false,
        ..mailbox_channel(company.id)
    };
    let closed = channel_edit_pane(&ChannelEditPane {
        company: &company,
        app_domain_name: "example.com",
        channel: &closed_channel,
        schedules: &[],
        schedule_bindings: &[],
        spam_scan_enabled: true,
        draft: None,
        error: None,
    });
    assert!(
        closed
            .contains(r#"name="add_3rd_party" value="true" class="checkbox checkbox-sm mt-0.5">"#)
    );
}

#[test]
fn channel_edit_pane_keeps_a_rejected_save_in_the_form() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let draft = ChannelDraft {
        name: "Renamed",
        slug: "renamed",
        participant_emails: "@public",
        ..ChannelDraft::default()
    };

    let html = channel_edit_pane(&ChannelEditPane {
        company: &company,
        app_domain_name: "example.com",
        channel: &channel,
        schedules: &[],
        schedule_bindings: &[],
        spam_scan_enabled: false,
        draft: Some(&draft),
        error: Some("Invalid JSON config"),
    });

    assert!(html.contains("alert alert-error"));
    assert!(html.contains("Invalid JSON config"));
    assert!(html.contains("value=\"Renamed\""));
    assert!(!html.contains("name=\"channel_config\""));
    // The header still names the stored channel; only the form carries the attempt.
    assert!(html.contains("<h2 class=\"truncate text-xl font-bold\">Inbox</h2>"));
    // The draft names @public, so the confirmation the use case will demand is live.
    assert!(
        html.contains(
            "name=\"confirm_spam_disabled\" value=\"true\" class=\"checkbox checkbox-sm\">"
        )
    );
    assert!(!html.contains("opacity-40"));
}

#[test]
fn spam_confirmation_is_inert_until_the_channel_is_public() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let pane = |participant_emails, spam_scan_enabled| {
        let draft = ChannelDraft {
            name: "Inbox",
            slug: "inbox",
            participant_emails,
            ..ChannelDraft::default()
        };
        channel_edit_pane(&ChannelEditPane {
            company: &company,
            app_domain_name: "example.com",
            channel: &channel,
            schedules: &[],
            schedule_bindings: &[],
            spam_scan_enabled,
            draft: Some(&draft),
            error: None,
        })
    };

    // Scanning on: the use case never asks, so neither does the form.
    assert!(!pane("@public", true).contains("confirm_spam_disabled"));

    // Scanning off but the channel is restricted: shown, dimmed, and not submittable.
    let restricted = pane("person@example.com", false);
    assert!(restricted.contains("spam-confirm-box"));
    assert!(restricted.contains("opacity-40 pointer-events-none"));
    assert!(restricted.contains("class=\"checkbox checkbox-sm\" disabled>"));

    // Scanning off and the channel is public: this is the case the use case rejects without it.
    let public = pane("a@x.com, @PUBLIC", false);
    assert!(!public.contains("opacity-40"));
    assert!(public.contains("class=\"checkbox checkbox-sm\">"));

    // The participants field is what drives the toggle from there on.
    assert!(public.contains(r##"data-input="channel-spam-confirm""##));
}

#[test]
fn cancelling_a_channel_form_dismisses_the_pane() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let close = format!(
        "hx-get=\"/ui/channels/close?company_id={}\"\n                            hx-target=\"#channel-pane\" hx-swap=\"outerHTML\" hx-sync=\"#channel-pane:replace\"\n                            hx-push-url=\"/ui/channels?company_id={}\">Cancel</button>",
        company.id, company.id
    );

    let edit = channel_edit_pane(&ChannelEditPane {
        company: &company,
        app_domain_name: "example.com",
        channel: &channel,
        schedules: &[],
        schedule_bindings: &[],
        spam_scan_enabled: true,
        draft: None,
        error: None,
    });
    assert!(edit.contains(&close));

    // All three create tabs offer the same way out.
    let create = channel_create_pane(&ChannelCreatePane {
        company: &company,
        app_domain_name: "example.com",
        spam_scan_enabled: true,
        draft: &ChannelDraft::default(),
        error: None,
    });
    assert_eq!(create.matches(&close).count(), 1);
}

/// p08 D4.7: the create pane makes a plain channel; an agent answers it through a workflow
/// binding, so the Easy (library agents) and Simple (instructions) tabs are gone.
#[test]
fn channel_create_pane_makes_a_plain_channel_and_keeps_a_rejected_draft() {
    let company = mailbox_company();
    let pane = |draft: &ChannelDraft<'_>, error| {
        channel_create_pane(&ChannelCreatePane {
            company: &company,
            app_domain_name: "example.com",
            spam_scan_enabled: true,
            draft,
            error,
        })
    };

    let fresh = pane(&ChannelDraft::default(), None);
    assert!(fresh.contains(&format!(
        "hx-post=\"/ui/channels?company_id={}\"",
        company.id
    )));
    assert!(fresh.contains(r##"id="channel-create-form" class="space-y-4"##));
    assert!(!fresh.contains("form_mode"));
    assert!(!fresh.contains(">Easy</button>"));
    assert!(!fresh.contains("/ui/channels/easy"));
    assert!(!fresh.contains("system_prompt"));
    assert!(fresh.contains("create an assistant channel from the agent's page"));

    // A rejected submit comes back with what was typed.
    let retried = pane(
        &ChannelDraft {
            name: "Support",
            slug: "support",
            ..ChannelDraft::default()
        },
        Some("Failed to create channel: slug already taken"),
    );
    assert!(retried.contains("slug already taken"));
    assert!(retried.contains("value=\"Support\""));
}

#[test]
fn channels_page_opens_the_form_the_mailbox_asked_for() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let channels = std::slice::from_ref(&channel);
    let page = |focus| {
        channels_page(&ChannelsPage {
            company: &company,
            app_domain_name: "example.com",
            channels,
            spam_scan_enabled: true,
            focus,
        })
    };

    let collapsed = page(ChannelsPageFocus::default());
    assert!(collapsed.contains("id=\"channel-form-card\" class=\"hidden"));
    assert!(collapsed.contains("aria-expanded=\"false\""));

    let creating = page(ChannelsPageFocus {
        create_form_open: true,
        editing_channel_id: None,
    });
    assert!(!creating.contains("id=\"channel-form-card\" class=\"hidden"));
    assert!(creating.contains("aria-expanded=\"true\""));
    assert!(!creating.contains("inline_agent_api_key"));

    let editing = page(ChannelsPageFocus {
        create_form_open: false,
        editing_channel_id: Some(channel.id),
    });
    assert!(editing.contains(&format!(
        "<form id=\"channel-{}\" hx-put=\"/companies/{}/channels/{}\"",
        channel.id, company.id, channel.id
    )));

    // An id that is not in this company's list falls back to the plain row list.
    let stale = page(ChannelsPageFocus {
        create_form_open: false,
        editing_channel_id: Some(Uuid::new_v4()),
    });
    assert!(stale.contains(&format!("<div id=\"channel-{}\"", channel.id)));
}

#[test]
fn appended_thread_page_swaps_pagination_out_of_band() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let thread = mailbox_thread(channel.id);

    let fragment = thread_list_fragment(
        &ThreadColumn {
            company_id: company.id,
            channel: &channel,
            app_domain_name: MAILBOX_APP_DOMAIN,
            threads: std::slice::from_ref(&thread),
            next_cursor: None,
            selected_thread_id: None,
            activity: no_activity(),
        },
        FragmentSwap::OutOfBand,
    );

    assert!(fragment.contains("id=\"thread-pagination\" hx-swap-oob=\"outerHTML\""));
    assert!(!fragment.contains("Load older threads"));
}

#[test]
fn message_pane_puts_the_viewers_messages_on_the_right_and_everyone_else_on_the_left() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let thread = mailbox_thread(channel.id);

    let inbound = stored_email_view(EmailMessageDraft {
        id: Uuid::new_v4(),
        thread_id: thread.id,
        message_id: "<in@test.com>".into(),
        in_reply_to: None,
        references_list: vec![],
        sender: mailbox_account_email(),
        recipients_to: vec![],
        recipients_cc: vec![],
        subject: "Question".to_string(),
        clean_text_body: "Plain <b>text</b> body".to_string(),
        raw_text_body: None,
        raw_html_body: None,
        attachments: None,
        direction: MessageDirection::Inbound,
        role: MessageRole::Human,
        thread_index: None,
        created_at: Utc::now(),
    });
    let outbound = stored_email_view(EmailMessageDraft {
        id: Uuid::new_v4(),
        thread_id: thread.id,
        message_id: "<out@test.com>".into(),
        in_reply_to: Some("<in@test.com>".into()),
        references_list: vec![],
        sender: "inbox@acme.example.com".into(),
        recipients_to: vec![],
        recipients_cc: vec![],
        subject: "Re: Question".to_string(),
        clean_text_body: "**Answered**".to_string(),
        raw_text_body: None,
        raw_html_body: None,
        attachments: None,
        direction: MessageDirection::Outbound,
        role: MessageRole::Agent,
        thread_index: None,
        created_at: Utc::now(),
    });

    let html = message_pane(&MessagePane {
        company_id: company.id,
        channel: &channel,
        thread: &thread,
        messages: &[inbound, outbound],
        viewer_email: &mailbox_account_email(),
        activity: &Default::default(),
        error: None,
        viewer_membership: CompanyMembership::Owner,
    });

    assert!(html.contains("chat chat-start"));
    assert!(html.contains("chat chat-end"));
    assert_eq!(html.matches("chat chat-start").count(), 1);
    assert_eq!(html.matches("chat chat-end").count(), 1);
    assert!(html.contains("Plain &lt;b&gt;text&lt;/b&gt; body"));
    assert!(html.contains("<strong>Answered</strong>"));

    // The header offers a further message in this same thread.
    assert!(html.contains("New Message"));
    assert!(html.contains(&format!(
        "/ui/reply?company_id={}&channel_id={}&thread_id={}",
        company.id, channel.id, thread.id
    )));

    // The chat composer under the messages posts into this same thread.
    assert!(html.contains("id=\"thread-composer\""));
    assert!(html.contains("hx-post=\"/ui/reply\""));
    assert!(html.contains(&format!("name=\"thread_id\" value=\"{}\"", thread.id)));
    assert!(html.contains(&format!("name=\"channel_id\" value=\"{}\"", channel.id)));
}

/// One message, so the live-stream tests below can say what they mean without 18 lines of struct.
fn mailbox_message(thread_id: Uuid, body: &str) -> ThreadMessageView {
    stored_email_view(mailbox_message_draft(thread_id, body))
}

/// A message as a page reads it, with no mail behind it at all -- the shape a Slack turn, a
/// schedule prompt and an in-app reply share.
fn mailbox_message_view(thread_id: Uuid, body: &str) -> ThreadMessageView {
    ThreadMessageView {
        response_contract: None,
        id: Uuid::new_v4(),
        canonical_id: CanonicalMessageId::random(),
        thread_id,
        run_ids: Vec::new(),
        author: AuthorView {
            principal_id: PrincipalId::random(),
            label: "Ada Lovelace".into(),
            handle: None,
            transport: None,
        },
        subject: "Question".into(),
        body: body.to_string(),
        attachments: Vec::new(),
        direction: MessageDirection::Inbound,
        role: MessageRole::Human,
        audience: crate::entities::message::MessageAudience::ExternalConversation,
        entry_kind: crate::entities::message::ThreadEntryKind::Conversation,
        internal_note: None,
        created_at: Utc::now(),
    }
}

#[test]
fn internal_note_markup_is_unmistakable_escaped_and_hides_removed_text() {
    let thread_id = Uuid::new_v4();
    let note_id = Uuid::new_v4();
    let mut note = mailbox_message_view(
        thread_id,
        r#"<img src=x onerror="steal()"> private customer secret"#,
    );
    note.audience = crate::entities::message::MessageAudience::InternalOnly;
    note.entry_kind = crate::entities::message::ThreadEntryKind::Note;
    note.author.label = r#"Owner <script>alert(1)</script>"#.into();
    note.internal_note = Some(InternalNoteView {
        id: note_id,
        author_principal_id: note.author.principal_id,
        provenance: InternalNoteProvenance::HumanUi,
        supersedes_note_id: None,
        superseded_by_note_id: None,
        tombstoned_at: None,
    });
    let scope = MessageScope {
        company_id: Uuid::new_v4(),
        channel_id: Uuid::new_v4(),
    };

    let active = message_bubble_chat(&note, None, scope, RunLinkAccess::Label);
    assert!(active.contains("Internal only"));
    assert!(active.contains("internal_only / note"));
    assert!(!active.contains("use with agent"));
    assert!(active.contains("&lt;img src=x onerror=&quot;steal()&quot;&gt;"));
    assert!(active.contains("Owner &lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(!active.contains("<img src=x"));
    assert!(!active.contains("<script>alert"));

    note.internal_note.as_mut().unwrap().tombstoned_at = Some(Utc::now());
    let removed = message_bubble_chat(&note, None, scope, RunLinkAccess::Label);
    assert!(removed.contains("This internal note was removed"));
    assert!(removed.contains("removed</span>"));
    assert!(!removed.contains("private customer secret"));
    assert!(!removed.contains("use with agent"));
}

/// An agent's answer as a page reads it: authored by the agent's principal, over no transport.
fn agent_reply_view(thread_id: Uuid, body: &str) -> ThreadMessageView {
    ThreadMessageView {
        author: AuthorView {
            principal_id: PrincipalId::random(),
            label: "Triage Agent".into(),
            handle: None,
            transport: None,
        },
        subject: "Re: Question".into(),
        direction: MessageDirection::Outbound,
        role: MessageRole::Agent,
        ..mailbox_message_view(thread_id, body)
    }
}

/// The same message, still as a draft, for a scenario that overrides a field or two.
fn mailbox_message_draft(thread_id: Uuid, body: &str) -> EmailMessageDraft {
    EmailMessageDraft {
        id: Uuid::new_v4(),
        thread_id,
        message_id: "<live@test.com>".into(),
        in_reply_to: None,
        references_list: vec![],
        sender: "person@example.com".into(),
        recipients_to: vec![],
        recipients_cc: vec![],
        subject: "Question".to_string(),
        clean_text_body: body.to_string(),
        raw_text_body: None,
        raw_html_body: None,
        attachments: None,
        direction: MessageDirection::Inbound,
        role: MessageRole::Human,
        thread_index: None,
        created_at: Utc::now(),
    }
}

#[test]
fn message_pane_streams_new_messages_from_where_it_was_rendered() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let thread = mailbox_thread(channel.id);
    let newest = mailbox_message(thread.id, "the latest word");

    let html = message_pane(&MessagePane {
        company_id: company.id,
        channel: &channel,
        thread: &thread,
        messages: &[mailbox_message(thread.id, "older"), newest.clone()],
        viewer_email: &mailbox_account_email(),
        activity: &Default::default(),
        error: None,
        viewer_membership: CompanyMembership::Owner,
    });

    // The connection lives on the pane, so every existing pane swap tears it down and rebuilds it
    // without any lifecycle code of its own.
    assert!(html.contains("hx-ext=\"sse\""));
    assert!(html.contains(&format!(
        "sse-connect=\"/ui/events?company_id={}&channel_id={}&thread_id={}&after={}\"",
        company.id,
        channel.id,
        thread.id,
        newest.cursor()
    )));

    // Appending is what leaves a half-typed draft and the scroll position alone.
    assert!(html.contains("sse-swap=\"message\" hx-swap=\"beforeend\""));
}

/// An empty thread has no high-water mark to resume from, and streams from its start. Sending
/// `after=` empty would be a cursor the server has to reject.
#[test]
fn message_pane_omits_the_resume_cursor_for_an_empty_thread() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let thread = mailbox_thread(channel.id);

    let html = message_pane(&MessagePane {
        company_id: company.id,
        channel: &channel,
        thread: &thread,
        messages: &[],
        viewer_email: &mailbox_account_email(),
        activity: &Default::default(),
        error: None,
        viewer_membership: CompanyMembership::Owner,
    });

    assert!(html.contains(&format!(
        "sse-connect=\"/ui/events?company_id={}&channel_id={}&thread_id={}\"",
        company.id, channel.id, thread.id
    )));
    assert!(!html.contains("&after="));
    // The first streamed message clears this, so it needs an id to be found by.
    assert!(html.contains("id=\"no-messages\""));
}

#[test]
fn thread_column_streams_touched_threads_from_where_it_was_rendered() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let newest = mailbox_thread(channel.id);
    let older = Thread {
        id: Uuid::new_v4(),
        updated_at: newest.updated_at - chrono::Duration::hours(1),
        ..newest.clone()
    };

    let html = thread_column(&ThreadColumn {
        company_id: company.id,
        channel: &channel,
        app_domain_name: MAILBOX_APP_DOMAIN,
        // The column is newest-first, so the resume cursor is the *first* row, not the last.
        threads: &[newest.clone(), older],
        next_cursor: None,
        selected_thread_id: None,
        activity: no_activity(),
    });

    assert!(html.contains("hx-ext=\"sse\""));
    assert!(html.contains(&format!(
        "sse-connect=\"/ui/threads/events?company_id={}&channel_id={}&after={}\"",
        company.id,
        channel.id,
        newest.cursor()
    )));

    // A bumped thread arrives as an insert at the top; the client drops the stale copy below.
    assert!(html.contains("sse-swap=\"thread\" hx-swap=\"afterbegin\""));
    assert!(html.contains("hx-sync=\"#detail-pane:replace\""));
    assert!(html.contains(&format!("data-thread-id=\"{}\"", newest.id)));
}

/// The container is rendered twice; both must stay live. Rendering it out of band without the
/// streaming attributes is what silently killed the live column after this client's first send.
#[test]
fn the_out_of_band_thread_list_stays_live() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let thread = mailbox_thread(channel.id);

    let oob = thread_list_oob(&ThreadColumn {
        company_id: company.id,
        channel: &channel,
        app_domain_name: MAILBOX_APP_DOMAIN,
        threads: std::slice::from_ref(&thread),
        next_cursor: None,
        selected_thread_id: Some(thread.id),
        activity: no_activity(),
    });

    assert!(oob.contains("hx-swap-oob=\"outerHTML\""));
    assert!(oob.contains("sse-swap=\"thread\""));
    assert!(oob.contains("hx-swap=\"afterbegin\""));
}

/// An empty channel has no high-water mark, and streams from its start.
#[test]
fn thread_column_omits_the_resume_cursor_for_an_empty_channel() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);

    let html = thread_column(&ThreadColumn {
        company_id: company.id,
        channel: &channel,
        app_domain_name: MAILBOX_APP_DOMAIN,
        threads: &[],
        next_cursor: None,
        selected_thread_id: None,
        activity: no_activity(),
    });

    assert!(html.contains(&format!(
        "sse-connect=\"/ui/threads/events?company_id={}&channel_id={}\"",
        company.id, channel.id
    )));
    assert!(!html.contains("&after="));
    // The first streamed row clears this, so the client needs to find it.
    assert!(html.contains("no-threads"));
}

/// Same contract as the message bubbles: what streams in must match what a reload renders.
#[test]
fn a_streamed_thread_row_is_identical_to_one_rendered_with_the_column() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let thread = mailbox_thread(channel.id);

    let column = thread_column(&ThreadColumn {
        company_id: company.id,
        channel: &channel,
        app_domain_name: MAILBOX_APP_DOMAIN,
        threads: std::slice::from_ref(&thread),
        next_cursor: None,
        selected_thread_id: None,
        activity: no_activity(),
    });

    assert!(
        column.contains(
            thread_row_fragment(
                company.id,
                &channel,
                &thread,
                false,
                ThreadRowMarks::default()
            )
            .trim()
        )
    );
}

/// The stream cannot know which thread this browser has open, so it always renders unselected —
/// the client re-applies the highlight. The selected form still has to exist for the page render.
#[test]
fn thread_row_marks_only_the_selected_thread() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let thread = mailbox_thread(channel.id);

    assert!(
        thread_row_fragment(
            company.id,
            &channel,
            &thread,
            true,
            ThreadRowMarks::default()
        )
        .contains("bg-base-300")
    );
    assert!(
        !thread_row_fragment(
            company.id,
            &channel,
            &thread,
            false,
            ThreadRowMarks::default()
        )
        .contains("bg-base-300")
    );
}

#[test]
fn a_thread_row_carries_its_own_activity_slot() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let thread = mailbox_thread(channel.id);

    let idle = thread_row_fragment(
        company.id,
        &channel,
        &thread,
        false,
        ThreadRowMarks::default(),
    );
    let working = thread_row_fragment(
        company.id,
        &channel,
        &thread,
        false,
        ThreadRowMarks {
            activity: Some(ThreadActivity::Working),
            ..ThreadRowMarks::default()
        },
    );

    // Its own event name, so a status change redraws just this badge. Sharing the column's
    // `thread` event would re-insert the whole row at the top and move the thread.
    let slot = format!("sse-swap=\"activity-{}\"", thread.id);
    assert!(idle.contains(&slot));
    assert!(working.contains(&slot));
    assert!(idle.contains("hx-swap=\"innerHTML\""));
    // The slot sits inside a row button that targets `#detail-pane`, and htmx inherits attributes:
    // without pinning the target, a badge update swaps itself over the open conversation.
    assert!(idle.contains("hx-target=\"this\""));

    // An idle thread has the slot but nothing in it, so a mark can be streamed in later.
    assert!(!idle.contains("title="));

    // Every agent run goes through the worker, so a run in progress leads the reply rather than
    // trailing it -- and it is the one state that resolves on its own, so it is the one that
    // animates.
    assert_ne!(
        working, idle,
        "a working thread must be distinguishable from an idle one"
    );
    assert!(working.contains(&icon(Icon::DotFill, BUTTON_ICON)));
    assert!(working.contains(r#"title="Agent working…""#));
    assert!(working.contains("animate-pulse"));

    // Queued shares the glyph but not the animation: it is picked up within a poll interval, and
    // changing shape that fast reads as a flicker rather than as progress.
    let queued = thread_row_fragment(
        company.id,
        &channel,
        &thread,
        false,
        ThreadRowMarks {
            activity: Some(ThreadActivity::Queued),
            ..ThreadRowMarks::default()
        },
    );
    assert!(queued.contains(&icon(Icon::DotFill, BUTTON_ICON)));
    assert!(queued.contains(r#"title="Queued""#));
    assert!(!queued.contains("animate-pulse"));

    // A thread that has stalled earns its own glyph -- quiet, unanimated, wording on hover.
    let blocked = thread_row_fragment(
        company.id,
        &channel,
        &thread,
        false,
        ThreadRowMarks {
            activity: Some(ThreadActivity::AwaitingReview),
            ..ThreadRowMarks::default()
        },
    );
    assert!(blocked.contains(&icon(Icon::Hourglass, BUTTON_ICON)));
    assert!(blocked.contains(r#"title="Awaiting review""#));
    assert!(!blocked.contains("animate-pulse"));
}

/// The reply mark is client-side and only ever applies to rows arriving over the stream, so the
/// row has to say who spoke last -- but only when the stream renders it.
#[test]
fn only_streamed_rows_say_who_spoke_last() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let thread = mailbox_thread(channel.id);

    let from_page = thread_row_fragment(
        company.id,
        &channel,
        &thread,
        false,
        ThreadRowMarks::default(),
    );
    assert!(!from_page.contains("data-last-role"));
    assert!(from_page.contains(r#"<span class="thread-mark"#));

    let streamed = thread_row_fragment(
        company.id,
        &channel,
        &thread,
        false,
        ThreadRowMarks {
            last_role: Some(MessageRole::Agent),
            ..ThreadRowMarks::default()
        },
    );
    assert!(streamed.contains(r#"data-last-role="agent""#));

    let from_person = thread_row_fragment(
        company.id,
        &channel,
        &thread,
        false,
        ThreadRowMarks {
            last_role: Some(MessageRole::Human),
            ..ThreadRowMarks::default()
        },
    );
    assert!(from_person.contains(r#"data-last-role="human""#));
}

/// The column tells the client which thread is already on screen, so a reply the reader is looking
/// at is never marked as something they missed.
#[test]
fn the_open_thread_is_identifiable_from_the_pane() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let thread = mailbox_thread(channel.id);

    let html = message_pane(&MessagePane {
        company_id: company.id,
        channel: &channel,
        thread: &thread,
        messages: &[],
        viewer_email: &mailbox_account_email(),
        activity: &Default::default(),
        error: None,
        viewer_membership: CompanyMembership::Owner,
    });

    assert!(html.contains(&format!(r#"data-thread-id="{}""#, thread.id)));
}

#[test]
fn opening_or_receiving_a_message_scrolls_to_the_start_of_the_newest_bubble() {
    assert!(MAILBOX_SCRIPT.contains("function scrollToNewestMessageStart()"));
    assert!(MAILBOX_SCRIPT.contains("pane.scrollTop = newest.offsetTop - pane.offsetTop"));
    assert!(MAILBOX_SCRIPT.contains("window.addEventListener('load', scrollToNewestMessageStart)"));
    assert!(!MAILBOX_SCRIPT.contains("scrollMessagesToBottom"));
}

/// The spinner is for work actually in progress. A run parked on a decision gets a badge with
/// no spinner — something spinning forever reads as broken rather than blocked.
#[test]
fn only_a_running_agent_gets_a_spinner() {
    let working = thread_activity_strip(
        &test_thread_runs(ThreadActivity::Working),
        Uuid::nil(),
        RunLinkAccess::Label,
    );
    assert!(working.contains("loading loading-dots"));
    assert!(working.contains("Agent working"));

    for blocked in [
        ThreadActivity::Queued,
        ThreadActivity::AwaitingReview,
        ThreadActivity::NeedsAttention,
        ThreadActivity::Waiting,
        ThreadActivity::Failed,
    ] {
        let strip = thread_activity_strip(
            &test_thread_runs(blocked),
            Uuid::nil(),
            RunLinkAccess::Label,
        );
        assert!(!strip.contains("loading"), "{blocked:?} must not spin");
        assert!(strip.contains(blocked.label()));
    }

    // An idle thread renders nothing, which is what clears the strip.
    assert!(
        thread_activity_strip(&Default::default(), Uuid::nil(), RunLinkAccess::Label).is_empty()
    );
}

#[test]
fn the_message_pane_has_a_slot_for_the_activity_strip() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let thread = mailbox_thread(channel.id);

    let html = message_pane(&MessagePane {
        company_id: company.id,
        channel: &channel,
        thread: &thread,
        messages: &[],
        viewer_email: &mailbox_account_email(),
        activity: &test_thread_runs(ThreadActivity::Working),
        error: None,
        viewer_membership: CompanyMembership::Owner,
    });

    assert!(html.contains(
        "id=\"thread-activity\" sse-swap=\"activity\" hx-target=\"this\" hx-swap=\"innerHTML\""
    ));
    assert!(html.contains("loading loading-dots"));

    // The strip sits outside the scroll area: `#message-scroll` appends with `beforeend`, so a
    // strip inside it would end up above every later message instead of under all of them.
    let scroll_start = html.find("id=\"message-scroll\"").unwrap();
    let scroll_end = html.find("id=\"thread-activity\"").unwrap();
    assert!(scroll_start < scroll_end);
    assert!(html[scroll_start..scroll_end].contains("</div>"));
}

#[test]
fn an_agent_reply_is_drawn_as_its_channel_address_and_an_inbound_message_as_its_sender() {
    let company = mailbox_company();
    let thread = mailbox_thread(mailbox_channel(company.id).id);
    let scope = MessageScope {
        company_id: Uuid::new_v4(),
        channel_id: Uuid::new_v4(),
    };

    // A reply is stored with the channel's address as its sender, and that is what it shows.
    let reply = stored_email_view(EmailMessageDraft {
        role: MessageRole::Agent,
        direction: MessageDirection::Outbound,
        sender: EmailAddress::from("support@acme.mailagents.com"),
        ..mailbox_message_draft(thread.id, "The reply.")
    });
    let html = message_bubble_chat(&reply, None, scope, RunLinkAccess::Label);
    assert!(!html.contains("<img"));
    assert!(html.contains("support@acme.mailagents.com"));

    // An inbound message is its sender's.
    let inbound = mailbox_message(thread.id, "The question.");
    let inbound_html = message_bubble_chat(&inbound, None, scope, RunLinkAccess::Label);
    assert!(!inbound_html.contains("<img"));
    assert!(inbound_html.contains(inbound.author.display()));

    // Who wrote it is on the bubble, because only the agent's reply quiets the thread's row.
    assert!(html.contains(r#"data-role="agent""#));
    assert!(inbound_html.contains(r#"data-role="human""#));
}

/// A message that came in over the internal transport was written by an agent in *another*
/// channel, and is marked as such.
#[test]
fn a_message_from_another_channel_is_marked_and_keeps_its_own_sender() {
    let company = mailbox_company();
    let thread = mailbox_thread(mailbox_channel(company.id).id);
    let scope = MessageScope {
        company_id: Uuid::new_v4(),
        channel_id: Uuid::new_v4(),
    };

    // Inbound *and* agent: only the internal delivery path writes that pair.
    let delegated = stored_email_view(EmailMessageDraft {
        role: MessageRole::Agent,
        direction: MessageDirection::Inbound,
        sender: EmailAddress::from("legal@acme.mailagents.test"),
        ..mailbox_message_draft(thread.id, "Here is the contract review.")
    });
    let html = message_bubble_chat(&delegated, None, scope, RunLinkAccess::Label);

    assert!(html.contains(&icon(Icon::Hubot, BUTTON_ICON)));
    assert!(html.contains(r#"title="From an agent in another channel""#));
    // The sending channel's address.
    assert!(html.contains("legal@acme.mailagents.test"));
    assert!(!html.contains("<img"));
    // Still agent-authored, so the body is rendered as markdown and the row's mark still settles.
    assert!(html.contains(r#"data-role="agent""#));

    // This channel's own reply carries no mark.
    let own_reply = ThreadMessageView {
        direction: MessageDirection::Outbound,
        ..delegated.clone()
    };
    let own_html = message_bubble_chat(&own_reply, None, scope, RunLinkAccess::Label);
    assert!(!own_html.contains(&icon(Icon::Hubot, BUTTON_ICON)));

    // And an ordinary person's message carries no mark either.
    let from_person = message_bubble_chat(
        &mailbox_message(thread.id, "The question."),
        None,
        scope,
        RunLinkAccess::Label,
    );
    assert!(!from_person.contains(&icon(Icon::Hubot, BUTTON_ICON)));
}

/// A thread whose participant is a channel address was opened by that channel's agent delegating
/// work in -- a platform address becomes a thread principal no other way.
#[test]
fn a_thread_opened_by_another_channel_is_marked_and_one_from_a_person_is_not() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);

    let delegated = Thread {
        participant_projection: crate::entities::thread::ThreadParticipantProjection {
            identities: vec![
                crate::adapters::protocols::email::EmailIdentity::parse(EmailAddress::from(
                    format!("research@acme.{MAILBOX_APP_DOMAIN}"),
                ))
                .unwrap()
                .qualify_default(),
            ],
        },
        ..mailbox_thread(channel.id)
    };
    let row = thread_row_fragment(
        company.id,
        &channel,
        &delegated,
        false,
        ThreadRowMarks {
            from_other_channel: opened_by_another_channel(&delegated, MAILBOX_APP_DOMAIN),
            ..ThreadRowMarks::default()
        },
    );
    assert!(row.contains(&icon(Icon::Hubot, BUTTON_ICON)));
    assert!(row.contains(r#"title="Opened by an agent in another channel""#));
    // The subject still truncates beside the glyph rather than being replaced by it.
    assert!(row.contains(r#"<span class="truncate font-semibold">Question &lt;script&gt;</span>"#));

    // `mailbox_thread` is a person's, on a domain this platform does not answer for.
    let from_person = mailbox_thread(channel.id);
    assert!(!opened_by_another_channel(&from_person, MAILBOX_APP_DOMAIN));
    assert!(
        !thread_row_fragment(
            company.id,
            &channel,
            &from_person,
            false,
            ThreadRowMarks::default(),
        )
        .contains(&icon(Icon::Hubot, BUTTON_ICON))
    );
}

/// The column derives the mark itself, so a row rendered with the page says the same thing as one
/// the stream sends -- the guarantee `a_streamed_thread_row_is_identical_to_one_rendered_with_the_column`
/// makes for every other mark on the row.
#[test]
fn the_column_marks_a_delegated_thread_without_being_told() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let delegated = Thread {
        participant_projection: crate::entities::thread::ThreadParticipantProjection {
            identities: vec![
                crate::adapters::protocols::email::EmailIdentity::parse(EmailAddress::from(
                    format!("research@acme.{MAILBOX_APP_DOMAIN}"),
                ))
                .unwrap()
                .qualify_default(),
            ],
        },
        ..mailbox_thread(channel.id)
    };

    let column = thread_column(&ThreadColumn {
        company_id: company.id,
        channel: &channel,
        app_domain_name: MAILBOX_APP_DOMAIN,
        threads: std::slice::from_ref(&delegated),
        next_cursor: None,
        selected_thread_id: None,
        activity: no_activity(),
    });

    assert!(column.contains(&icon(Icon::Hubot, BUTTON_ICON)));
}

/// An agent's reply is the answer its row's dot was promising, so the row stops repeating it --
/// on the open thread and on one the reader was not watching alike, and until the column has a new
/// state to report for that thread.
#[test]
fn a_reply_in_the_open_thread_quiets_that_thread_s_activity_mark() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let thread = mailbox_thread(channel.id);

    let detail = empty_detail_pane("Select a thread.", FragmentSwap::Inline);
    let email = mailbox_account_email();

    let page = mailbox_page(&MailboxPage {
        user: &mailbox_user(&email),
        company: &company,
        companies: std::slice::from_ref(&company),
        app_domain_name: "example.com",
        channels: std::slice::from_ref(&channel),
        selected_channel: Some(&channel),
        threads: std::slice::from_ref(&thread),
        next_cursor: None,
        selected_thread_id: None,
        activity: no_activity(),
        detail_html: &detail,
    });

    // The rule that hides it, and the reply that puts the row under it.
    assert!(page.contains(".thread-row.thread-replied .thread-activity { display: none; }"));
    let script = application_javascript();
    assert!(script.contains("function quietRepliedRow(bubble)"));
    assert!(script.contains("quietRepliedRow(swapped.lastElementChild)"));
    // Only the agent's own bubble counts as the reply.
    assert!(script.contains("bubble.dataset.role !== 'agent'"));

    // A reply on a row the reader was not watching quiets it too, and each arrival is spent once
    // so a later insert cannot re-settle a thread that has since started working again.
    assert!(script.contains("row.classList.add('thread-replied')"));
    assert!(script.contains("row.removeAttribute('data-last-role')"));

    // Quiet ends on a badge that carries a state, not on the reader opening something else.
    assert!(script.contains("badgeRow.classList.remove('thread-replied')"));
    assert!(script.contains("if (swapped.firstElementChild)"));
    assert!(!script.contains("row.classList.remove('thread-replied')"));
}

/// Every shape a canonical message comes in has to render through the one bubble, without a
/// branch for the transport that carried it -- or one that has no transport at all.
///
/// The Slack-shaped case is the load-bearing one: no `To`, no `Cc`, no `Message-ID`, an author
/// named by a workspace handle. The pre-canonical bubble could not have been given that message
/// at all.
#[test]
fn a_message_renders_for_every_transport_including_none_at_all() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let thread = mailbox_thread(channel.id);
    let scope = MessageScope {
        company_id: company.id,
        channel_id: channel.id,
    };

    let email = stored_email_view(mailbox_message_draft(thread.id, "Arrived by mail."));
    let slack = ThreadMessageView {
        author: AuthorView {
            principal_id: PrincipalId::random(),
            label: "Ada Lovelace".into(),
            handle: Some("U0123ABC".into()),
            transport: Some(TransportKind::Slack),
        },
        ..mailbox_message_view(thread.id, "Posted in a conversation.")
    };
    let agent = agent_reply_view(thread.id, "The answer.");
    let schedule = ThreadMessageView {
        author: AuthorView {
            principal_id: PrincipalId::random(),
            label: "System".into(),
            handle: None,
            transport: None,
        },
        role: MessageRole::System,
        subject: "Nightly audit".into(),
        ..mailbox_message_view(thread.id, "Run the audit.")
    };

    for message in [&email, &slack, &agent, &schedule] {
        let html = message_bubble_chat(message, None, scope, RunLinkAccess::Label);
        assert!(
            html.contains(&escape_html_text(&message.body)),
            "the body has to render: {html}"
        );
        assert!(
            html.contains(&escape_html_text(message.author.display())),
            "the author has to render: {html}"
        );
        // No provider key in an ordinary read, whichever transport carried it.
        assert!(!html.contains("@test.com>"), "{html}");
        assert!(!html.contains("<live@"), "{html}");
    }

    // The badge is what says which interface a turn came from, now that the name does not.
    assert!(message_bubble_chat(&slack, None, scope, RunLinkAccess::Label).contains(">slack<"));
    assert!(message_bubble_chat(&email, None, scope, RunLinkAccess::Label).contains(">email<"));
    // Nothing carried the schedule prompt or the agent's answer, so neither claims one.
    for message in [&agent, &schedule] {
        let html = message_bubble_chat(message, None, scope, RunLinkAccess::Label);
        assert!(!html.contains(">email<"), "{html}");
        assert!(!html.contains(">slack<"), "{html}");
    }
}

/// A thread page reaches provider identifiers only through a separately authorized request, so the
/// link is in the HTML and the keys are not.
/// p08 D4.6, WF-P08-F086: the simulator is the company owner's alone, so only the owner is
/// offered the way into it; an admin, a member or a stranger would land on a refusal.
#[test]
fn only_the_company_owner_is_offered_the_simulator() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let thread = mailbox_thread(channel.id);
    let render = |viewer_membership| {
        message_pane(&MessagePane {
            company_id: company.id,
            channel: &channel,
            thread: &thread,
            messages: &[],
            viewer_email: &mailbox_account_email(),
            activity: &Default::default(),
            error: None,
            viewer_membership,
        })
    };
    let link = format!(
        "/companies/{}/channels/{}/simulate?thread_id={}",
        company.id, channel.id, thread.id
    );

    let owner = render(CompanyMembership::Owner);
    assert!(owner.contains(&link), "{owner}");
    assert!(owner.contains("Open in Simulator"));
    for reader in [
        CompanyMembership::Admin,
        CompanyMembership::Member,
        CompanyMembership::None,
    ] {
        let html = render(reader);
        assert!(!html.contains("/simulate"), "{reader:?}: {html}");
        assert!(!html.contains("Open in Simulator"), "{reader:?}");
    }
}

#[test]
fn a_thread_page_links_to_the_diagnostic_pane_rather_than_rendering_provider_keys() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let thread = mailbox_thread(channel.id);
    let message = stored_email_view(mailbox_message_draft(thread.id, "Arrived by mail."));
    let message_id = message.id;

    let html = message_pane(&MessagePane {
        company_id: company.id,
        channel: &channel,
        thread: &thread,
        messages: &[message],
        viewer_email: &mailbox_account_email(),
        activity: &Default::default(),
        error: None,
        viewer_membership: CompanyMembership::Owner,
    });

    assert!(html.contains(&format!(
        "/ui/threads/{}/messages/{message_id}/diagnostics",
        thread.id
    )));
    assert!(html.contains(r#"id="message-diagnostics-body""#));
    assert!(!html.contains("<live@test.com>"), "{html}");
}

/// The pane names the interface every key belongs to. A bare `Message-ID` identifies nothing: the
/// same text is one outbound message on the sending channel's binding and one inbound message on
/// the receiving channel's.
#[test]
fn the_diagnostic_pane_qualifies_every_provider_key_by_its_interface() {
    let audit = MessageAuditView {
        id: Uuid::new_v4(),
        canonical_id: CanonicalMessageId::random(),
        company_id: Uuid::new_v4(),
        thread_id: Uuid::new_v4(),
        channel_id: Uuid::new_v4(),
        author: AuthorView {
            principal_id: PrincipalId::random(),
            label: "Triage Agent".into(),
            handle: None,
            transport: None,
        },
        direction: MessageDirection::Outbound,
        role: MessageRole::Agent,
        audience: crate::entities::message::MessageAudience::ExternalConversation,
        entry_kind: crate::entities::message::ThreadEntryKind::Conversation,
        correlation_id: CorrelationId::new(),
        external_keys: vec![ExternalMessageRef {
            binding_id: ChannelBindingId::random(),
            transport: TransportKind::Email,
            key: ExternalMessageKey::parse("<reply@acme.example>").unwrap(),
        }],
        created_at: Utc::now(),
    };

    let html = message_diagnostics_pane(&audit);
    assert!(html.contains(&audit.canonical_id.to_string()));
    assert!(html.contains(&audit.external_keys[0].binding_id.to_string()));
    assert!(html.contains("&lt;reply@acme.example&gt;"));
    assert!(html.contains("email"));
}

/// A message no transport carried has no keys, and the pane says so rather than looking broken.
#[test]
fn the_diagnostic_pane_says_when_no_transport_carried_a_message() {
    let audit = MessageAuditView {
        id: Uuid::new_v4(),
        canonical_id: CanonicalMessageId::random(),
        company_id: Uuid::new_v4(),
        thread_id: Uuid::new_v4(),
        channel_id: Uuid::new_v4(),
        author: AuthorView {
            principal_id: PrincipalId::random(),
            label: "System".into(),
            handle: None,
            transport: None,
        },
        direction: MessageDirection::Inbound,
        role: MessageRole::System,
        audience: crate::entities::message::MessageAudience::InternalOnly,
        entry_kind: crate::entities::message::ThreadEntryKind::SystemEvent,
        correlation_id: CorrelationId::new(),
        external_keys: Vec::new(),
        created_at: Utc::now(),
    };

    assert!(message_diagnostics_pane(&audit).contains("No transport carried this message."));
}

#[test]
fn an_attachment_is_offered_as_a_download_scoped_to_its_thread() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let thread = mailbox_thread(channel.id);
    let mut message = mailbox_message(thread.id, "See attached.");
    message.attachments = vec![
        AttachmentMetadata {
            filename: "Q3 <report>.pdf".to_string(),
            content_type: "application/pdf".to_string(),
            sha256_hash: "abc123".to_string(),
            size_bytes: 2048,
            storage_key: Some(crate::entities::value_objects::ObjectKey::new(
                "attachments/abc123.pdf",
            )),
            source: None,
        },
        // Mail that arrived before there was anywhere to keep it.
        AttachmentMetadata {
            filename: "old.doc".to_string(),
            content_type: "application/msword".to_string(),
            sha256_hash: "def456".to_string(),
            size_bytes: 10,
            storage_key: None,
            source: None,
        },
    ];

    let html = message_bubble_chat(
        &message,
        None,
        MessageScope {
            company_id: company.id,
            channel_id: channel.id,
        },
        RunLinkAccess::Label,
    );

    // The link is the app's own, carrying the scope the download is authorized against -- never a
    // storage URL.
    assert!(html.contains(&format!(
        r##"href="/ui/threads/{}/attachments/abc123?company_id={}&channel_id={}""##,
        thread.id, company.id, channel.id
    )));
    assert!(!html.contains("storage.googleapis.com"));
    assert!(!html.contains("attachments/abc123.pdf"));

    // The name is shown as text, not as markup.
    assert!(html.contains("Q3 &lt;report&gt;.pdf"));
    assert!(html.contains("2 KB"));

    // The one we do not have is listed, but leads nowhere.
    assert!(html.contains("old.doc"));
    assert!(!html.contains("attachments/def456"));
    assert!(html.contains("btn-disabled"));
}

/// The stream sends bubbles rendered by `message_bubble_chat` while the page renders them inside
/// `message_pane`. If those two ever diverge, a live message would look different from the same
/// message after a reload.
#[test]
fn a_streamed_bubble_is_identical_to_one_rendered_with_the_page() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let thread = mailbox_thread(channel.id);
    let message = mailbox_message(thread.id, "Plain <b>text</b> body");

    let pane = message_pane(&MessagePane {
        company_id: company.id,
        channel: &channel,
        thread: &thread,
        messages: std::slice::from_ref(&message),
        viewer_email: &mailbox_account_email(),
        activity: &Default::default(),
        error: None,
        viewer_membership: CompanyMembership::Owner,
    });

    // Rendered with the scope the pane itself uses, so the comparison is of the markup rather
    // than of two different attachment links.
    let scope = MessageScope {
        company_id: company.id,
        channel_id: channel.id,
    };
    assert!(
        pane.contains(
            message_bubble_chat(
                &message,
                Some(&mailbox_account_email()),
                scope,
                RunLinkAccess::Label
            )
            .trim()
        )
    );
    assert!(!pane.contains("name=\"quiet\""));
    assert!(pane.contains("hx-post=\"/ui/internal-notes\""));
    assert!(pane.contains("Private context only. This does not start a run or send anything."));
}

#[test]
fn reply_pane_fixes_the_subject_to_the_thread_it_continues() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let thread = mailbox_thread(channel.id);

    let html = reply_pane(&ReplyPane {
        company_id: company.id,
        channel: &channel,
        thread: &thread,
        channel_address: "inbox@acme.example.com",
        sender_email: "owner@example.com",
        text_body: "Draft reply",
        deliver: true,
        error: Some("Channel rejected the message"),
    });

    assert!(html.contains("hx-post=\"/ui/reply\""));
    assert!(html.contains(&format!("name=\"thread_id\" value=\"{}\"", thread.id)));
    assert!(html.contains("inbox@acme.example.com"));
    assert!(html.contains("owner@example.com"));
    assert!(html.contains("value=\"Re: Question &lt;script&gt;\" readonly"));
    assert!(html.contains("Draft reply</textarea>"));
    assert!(html.contains("toggle toggle-primary toggle-sm\" checked"));
    assert!(!html.contains("name=\"quiet\""));
    assert!(html.contains("Channel rejected the message"));

    // Cancel puts the thread's messages back in the pane it replaced.
    assert!(html.contains(&format!(
        "/ui/messages?company_id={}&channel_id={}&thread_id={}",
        company.id, channel.id, thread.id
    )));
}

#[test]
fn compose_pane_shows_the_channel_address_and_errors() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);

    let html = compose_pane(&ComposePane {
        company_id: company.id,
        channel: &channel,
        channel_address: "inbox@acme.example.com",
        sender_email: "owner@example.com",
        subject: "Draft subject",
        text_body: "Draft body",
        deliver: true,
        error: Some("Channel rejected the message"),
    });

    assert!(html.contains("inbox@acme.example.com"));
    assert!(html.contains("owner@example.com"));
    assert!(html.contains("value=\"Draft subject\""));
    assert!(html.contains("Draft body</textarea>"));
    assert!(html.contains("toggle toggle-primary toggle-sm\" checked"));
    assert!(!html.contains("name=\"quiet\""));
    assert!(html.contains("alert alert-error"));
    assert!(html.contains("Channel rejected the message"));
    assert!(html.contains("hx-post=\"/ui/compose\""));
}

fn settings_company(name: &str, slug: &str) -> Company {
    Company {
        channel_defaults: Default::default(),
        name: name.to_string(),
        slug: slug.into(),
        ..mailbox_company()
    }
}

#[test]
fn company_settings_page_uses_the_ui_shell_and_lights_its_own_rail_icon() {
    let company = settings_company("Acme", "acme");
    let other = settings_company("Globex", "globex");
    let email = mailbox_account_email();

    let list = CompanySettingsList {
        companies: &[company.clone(), other.clone()],
        selected_company_id: Some(company.id),
    };
    let pane = company_settings_empty_pane("Select a company.", FragmentSwap::Inline);
    let html = company_settings_page(&CompanySettingsPage {
        user: &mailbox_user(&email),
        list: &list,
        rail_company: Some(&company),
        pane_html: &pane,
    });

    // Same chrome as the other workspaces: its own rail icon lit, the others one click away.
    assert!(html.contains(&format!(
        r##"<a href="/ui/companies?company_id={}" class="btn btn-square btn-md btn-primary"##,
        company.id
    )));
    assert!(html.contains(&format!(
        r##"<a href="/ui/channels?company_id={}" class="btn btn-square btn-md btn-ghost"##,
        company.id
    )));
    assert!(html.contains("/assets/busybots-logo-dark-hor.png"));
    assert!(html.contains("id=\"company-pane\""));
    // The sidebar is the company list itself, so there is no switcher above it.
    assert!(!html.contains("dropdown-bottom"));
    assert!(html.contains(r##"hx-get="/ui/companies/new""##));
}

#[test]
fn company_settings_page_without_a_company_still_offers_the_create_form() {
    let email = mailbox_account_email();
    let list = CompanySettingsList {
        companies: &[],
        selected_company_id: None,
    };
    let pane = company_create_pane(&CompanyCreatePane {
        draft: &CompanyDraft::default(),
        error: None,
    });
    let html = company_settings_page(&CompanySettingsPage {
        user: &mailbox_user(&email),
        list: &list,
        rail_company: None,
        pane_html: &pane,
    });

    // Nothing for the rail to point at yet, but the workspace itself still works.
    assert!(!html.contains("btn btn-square btn-lg"));
    assert!(html.contains("No companies yet"));
    assert!(html.contains(r##"hx-post="/ui/companies""##));
    assert!(html.contains(r##"hx-get="/ui/companies/close""##));
}

#[test]
fn legacy_company_form_does_not_collect_ignored_model_credentials() {
    let html = companies_page(&[]);

    assert!(!html.contains("company_api_key"));
    assert!(!html.contains("company_provider"));
    assert!(!html.contains("company_model"));
}

#[test]
fn company_settings_list_saves_selection_in_the_url_and_swaps_out_of_band() {
    let company = settings_company("Acme", "acme");
    let other = settings_company("Globex", "globex");
    let list = CompanySettingsList {
        companies: &[company.clone(), other.clone()],
        selected_company_id: Some(company.id),
    };

    let inline = company_settings_list(&list, FragmentSwap::Inline);
    assert!(inline.contains(&format!(
        r##"href="/ui/companies?company_id={}""##,
        company.id
    )));
    assert!(!inline.contains("hx-target=\"#company-pane\""));
    assert!(inline.contains("/globex"));
    assert!(inline.contains("menu-active"));
    assert!(!inline.contains("hx-swap-oob"));

    // After a write the list rides along on the pane's response.
    let oob = company_settings_list(&list, FragmentSwap::OutOfBand);
    assert!(oob.contains("id=\"company-menu\""));
    assert!(oob.contains("hx-swap-oob=\"outerHTML\""));

    let empty = CompanySettingsList {
        companies: &[],
        selected_company_id: None,
    };
    assert!(company_settings_list(&empty, FragmentSwap::Inline).contains("No companies yet"));
}

#[test]
fn company_edit_pane_prefills_the_stored_company_and_offers_delete() {
    let company = Company {
        channel_defaults: Default::default(),
        ..settings_company("Acme & Co", "acme")
    };
    let model_connections = [CompanyModelConnection {
        provider: "openai".into(),
        models: vec![ModelName::from("gpt-4o")],
        is_default: true,
        has_api_key: true,
    }];

    let html = company_edit_pane(&CompanyEditPane {
        company: &company,
        model_connections: &model_connections,
        app_domain_name: "example.com",
        counts: CompanyCounts {
            channels: 3,
            agents: 2,
            skills: 0,
        },
        draft: None,
        error: None,
        editable: true,
        resend_api_section: "",
        body: CompanyPaneBody::Settings,
    });

    assert!(html.contains(r##"value="Acme &amp; Co""##));
    assert!(html.contains(r##"value="acme""##));
    assert!(html.contains(r##"value="openai""##));
    assert!(html.contains(
        r##"name="connection_0_api_key" value="" placeholder="Leave blank to keep the stored key""##
    ));
    assert!(html.contains(r##"name="connection_0_remove" value="true""##));
    assert!(html.contains("acme.example.com"));
    // The overrides are set, so the collapse opens on them rather than hiding a live setting.
    assert!(html.contains(r##"bg-base-200" open>"##));
    assert!(html.contains(&format!(r##"hx-put="/ui/companies/{}""##, company.id)));
    assert!(html.contains(&format!(r##"hx-delete="/ui/companies/{}""##, company.id)));
    assert!(html.contains("hx-confirm=\"Delete company"));
    // Cancel clears the pane, the way it does in the Channels workspace.
    assert!(html.contains(r##"hx-get="/ui/companies/close""##));
    // The summary counts what the company holds and links into the workspace that holds it.
    assert!(html.contains(&format!(
        r##"href="/ui/channels?company_id={}""##,
        company.id
    )));
    assert!(html.contains(&format!(r##"href="/ui/agents?company_id={}""##, company.id)));
    // The team is the pane's other half rather than somewhere it links out to, so the Settings
    // tab is the lit one and Team is one click away in the same pane.
    assert!(html.contains(&format!(
        r##"<a role="tab" class="tab tab-active" href="/ui/companies?company_id={}">Settings</a>"##,
        company.id
    )));
    assert!(html.contains(&format!(
        r##"href="/ui/companies?company_id={}&tab=team">Team</a>"##,
        company.id
    )));
}

#[test]
fn rejected_company_edit_gets_key_status_only_from_stored_metadata() {
    let company = settings_company("Acme", "acme");
    let stored_connections = [CompanyModelConnection {
        provider: "openai".into(),
        models: vec![ModelName::from("gpt-5.6-terra")],
        is_default: true,
        has_api_key: true,
    }];
    let draft = CompanyDraft {
        model_connections: vec![CompanyModelConnectionDraft {
            provider: "openai".into(),
            models: "gpt-5.6-terra".into(),
            is_default: true,
            // Rejected submissions do not get to claim a key is stored. The pane must derive this
            // bit from `stored_connections`, not from the browser.
            has_api_key: false,
            remove: false,
        }],
        ..CompanyDraft::default()
    };

    let html = company_edit_pane(&CompanyEditPane {
        company: &company,
        model_connections: &stored_connections,
        app_domain_name: "example.com",
        counts: CompanyCounts::default(),
        draft: Some(&draft),
        error: Some("save refused"),
        editable: true,
        resend_api_section: "",
        body: CompanyPaneBody::Settings,
    });

    assert!(html.contains("Leave blank to keep the stored key"));
    assert!(html.contains(r#"name="connection_0_api_key" value="""#));
    assert!(html.contains(r#"name="connection_0_remove" value="true""#));
}

#[test]
fn company_member_can_open_company_without_edit_controls_or_api_key() {
    let company = Company {
        channel_defaults: Default::default(),
        ..settings_company("Acme", "acme")
    };

    let html = company_edit_pane(&CompanyEditPane {
        company: &company,
        model_connections: &[],
        app_domain_name: "example.com",
        counts: CompanyCounts {
            channels: 3,
            agents: 2,
            skills: 0,
        },
        draft: None,
        error: None,
        editable: false,
        resend_api_section: "",
        body: CompanyPaneBody::Settings,
    });

    assert!(html.contains("Only the company owner can edit these settings."));
    assert!(html.contains(&format!(r##"href="/ui?company_id={}""##, company.id)));
    assert!(!html.contains("hx-put="));
    assert!(!html.contains("hx-delete="));
    assert!(!html.contains("owner-secret"));
}

#[test]
fn the_company_form_picks_a_picture_and_saves_it_with_the_rest_of_the_settings() {
    let mut company = settings_company("Acme", "acme");
    company.avatar_url = Some(AvatarUrl::from("https://cdn.example.com/acme.png"));

    let html = company_edit_pane(&CompanyEditPane {
        company: &company,
        model_connections: &[],
        app_domain_name: "example.com",
        counts: CompanyCounts::default(),
        draft: None,
        error: None,
        editable: true,
        resend_api_section: "",
        body: CompanyPaneBody::Settings,
    });

    // The picker is the same control every other picture is set with, in this form's own field.
    assert!(html.contains(r#"id="company-avatar""#));
    assert!(html.contains(r#"hx-post="/ui/uploads/avatar""#));
    // The stored picture is what the form is holding, so saving the form keeps it.
    assert!(html.contains(
        r#"<input type="hidden" name="avatar_url" value="https://cdn.example.com/acme.png">"#
    ));
    assert!(html.contains(&format!(r##"hx-put="/ui/companies/{}""##, company.id)));

    // A company with no picture yet offers the same field, showing its letter.
    let create = company_create_pane(&CompanyCreatePane {
        draft: &CompanyDraft::default(),
        error: None,
    });
    assert!(create.contains(r#"id="company-avatar""#));
    assert!(create.contains(r#"<input type="hidden" name="avatar_url" value="">"#));
}

#[test]
fn a_rejected_company_save_keeps_the_picture_that_was_picked() {
    let company = settings_company("Acme", "acme");
    let draft = CompanyDraft {
        name: "Acme Renamed",
        slug: "acme-renamed",
        avatar_url: "https://cdn.example.com/picked.png",
        ..CompanyDraft::default()
    };

    let html = company_edit_pane(&CompanyEditPane {
        company: &company,
        model_connections: &[],
        app_domain_name: "example.com",
        counts: CompanyCounts::default(),
        draft: Some(&draft),
        error: Some("Slug is taken"),
        editable: true,
        resend_api_section: "",
        body: CompanyPaneBody::Settings,
    });

    assert!(html.contains(
        r#"<input type="hidden" name="avatar_url" value="https://cdn.example.com/picked.png">"#
    ));

    // A submitted URL is text until it is parsed, and what cannot be rendered never reaches the
    // `<img src>` the bubble draws.
    let tampered = CompanyDraft {
        avatar_url: "javascript:alert(1)",
        ..CompanyDraft::default()
    };
    let refused = company_create_pane(&CompanyCreatePane {
        draft: &tampered,
        error: None,
    });
    assert!(!refused.contains("<img src=\"javascript:"));
}

#[test]
fn company_edit_pane_keeps_a_rejected_save_in_the_form() {
    let company = settings_company("Acme", "acme");
    let draft = CompanyDraft {
        name: "Acme Renamed",
        slug: "acme-renamed",
        avatar_url: "",
        model_connections: Vec::new(),
        ..CompanyDraft::default()
    };

    let html = company_edit_pane(&CompanyEditPane {
        company: &company,
        model_connections: &[],
        app_domain_name: "example.com",
        counts: CompanyCounts::default(),
        draft: Some(&draft),
        error: Some("Slug is taken"),
        editable: true,
        resend_api_section: "",
        body: CompanyPaneBody::Settings,
    });

    assert!(html.contains("Slug is taken"));
    assert!(html.contains(r##"value="Acme Renamed""##));
    assert!(html.contains(r##"value="acme-renamed""##));
    // The header still names the stored company — only the form carries what was typed.
    assert!(html.contains(">Acme</h2>"));
}

#[test]
fn the_create_form_deselects_the_list_without_emptying_the_rail() {
    let company = settings_company("Acme", "acme");
    let email = mailbox_account_email();

    let list = CompanySettingsList {
        companies: std::slice::from_ref(&company),
        selected_company_id: None,
    };
    let pane = company_create_pane(&CompanyCreatePane {
        draft: &CompanyDraft::default(),
        error: None,
    });
    let html = company_settings_page(&CompanySettingsPage {
        user: &mailbox_user(&email),
        list: &list,
        rail_company: Some(&company),
        pane_html: &pane,
    });

    // Nothing in the sidebar is lit, but the other workspaces are still one click away.
    assert!(!company_settings_list(&list, FragmentSwap::Inline).contains("menu-active"));
    assert!(html.contains(&format!(
        r##"<a href="/ui/channels?company_id={}" class="btn btn-square btn-md btn-ghost"##,
        company.id
    )));
    assert!(html.contains(r##"hx-post="/ui/companies""##));
}

fn team_member(company_id: Uuid, user_id: Uuid, username: &str, role: &str) -> CompanyMember {
    CompanyMember {
        id: Uuid::new_v4(),
        company_id,
        user_id,
        username: Some(username.to_string()),
        email: Some(format!("{username}@example.com")),
        avatar_url: None,
        role: role.parse().unwrap_or(CompanyAccessRole::Member),
        created_at: Utc::now(),
    }
}

fn team_invite(company_id: Uuid, email: &str, status: &str) -> CompanyInvite {
    CompanyInvite {
        id: Uuid::new_v4(),
        company_id,
        company_name: Some("Acme".to_string()),
        email: email.to_string(),
        role: CompanyAccessRole::Member,
        status: status.to_string(),
        created_at: Utc::now(),
    }
}

#[test]
fn the_team_tab_sits_inside_its_company_pane_and_lights_the_company_rail_icon() {
    let company = mailbox_company();
    let member = team_member(company.id, company.user_id, "dana", "owner");
    let invite = team_invite(company.id, "kim@example.com", "pending");

    let list = TeamSettingsList {
        company: &company,
        members: std::slice::from_ref(&member),
        invites: std::slice::from_ref(&invite),
        selected: TeamSelection::None,
        role: TeamRole::Owner,
    };
    let tab = team_tab(
        &list,
        &team_settings_empty_pane("Select someone.", FragmentSwap::Inline),
    );
    let html = company_edit_pane(&CompanyEditPane {
        company: &company,
        model_connections: &[],
        app_domain_name: "example.com",
        counts: CompanyCounts::default(),
        draft: None,
        error: None,
        editable: true,
        resend_api_section: "",
        body: CompanyPaneBody::Team(&tab),
    });

    // It is the company's pane, so it is the company's tab that is lit — and the reader is still
    // in the Companies workspace rather than in one of the team's own.
    assert!(html.contains("id=\"company-pane\""));
    assert!(html.contains(&format!(
        r##"href="/ui/companies?company_id={}&tab=team">Team</a>"##,
        company.id
    )));
    assert!(html.contains(r##"class="tab tab-active" href="/ui/companies?company_id"##));
    // The team's own two columns are inside it, and its endpoints are nested under the company.
    assert!(html.contains("id=\"team-menu\""));
    assert!(html.contains("id=\"team-pane\""));
    assert!(html.contains(&format!(
        r##"hx-get="/ui/companies/{}/team/new""##,
        company.id
    )));
    // The form arrives with the list beside it, so the response pushes the URL and the button
    // must not push a second one of its own -- `&new=1` is that URL, and nothing else emits it.
    assert!(!html.contains("&new=1"));
    // The Settings tab's own form is not in the pane while the team is.
    assert!(!html.contains(r##"name="default_add_3rd_party""##));
}

#[test]
fn team_settings_list_groups_members_and_invites_and_swaps_out_of_band() {
    let company = mailbox_company();
    let owner = team_member(company.id, company.user_id, "dana", "owner");
    let sam = team_member(company.id, Uuid::new_v4(), "sam", "member");
    let accepted = team_invite(company.id, "old@example.com", "accepted");
    let pending = team_invite(company.id, "kim@example.com", "pending");

    let list = TeamSettingsList {
        company: &company,
        members: &[owner.clone(), sam.clone()],
        // Deliberately the resolved one first, so the ordering below is the renderer's doing.
        invites: &[accepted.clone(), pending.clone()],
        selected: TeamSelection::Member(sam.user_id),
        role: TeamRole::Owner,
    };

    let inline = team_settings_list(&list, FragmentSwap::Inline);
    assert!(inline.contains(&format!(
        r##"hx-get="/ui/companies/{}/team/members/{}""##,
        company.id, sam.user_id
    )));
    assert!(inline.contains(&format!(
        r##"hx-get="/ui/companies/{}/team/invites/{}""##,
        company.id, pending.id
    )));
    // Picking somebody moves the address bar to the tab they are reachable at.
    assert!(inline.contains(&format!(
        r##"hx-push-url="/ui/companies?company_id={}&tab=team&member_id={}""##,
        company.id, sam.user_id
    )));
    assert!(inline.contains("hx-target=\"#team-pane\""));
    assert!(inline.contains("hx-sync=\"#team-pane:replace\""));
    assert!(inline.contains("menu-active"));
    assert!(!inline.contains("hx-swap-oob"));
    // Members, then invites; and a pending invite outranks one that is already answered.
    assert!(inline.find(">Members</li>") < inline.find(">Invites</li>"));
    assert!(inline.find("kim@example.com") < inline.find("old@example.com"));
    assert!(inline.contains("badge-warning"));
    assert!(inline.contains("badge-success"));

    // After a write the list rides along on the pane's response.
    let oob = team_settings_list(&list, FragmentSwap::OutOfBand);
    assert!(oob.contains("id=\"team-menu\""));
    assert!(oob.contains("hx-swap-oob=\"outerHTML\""));
}

#[test]
fn a_member_sees_the_team_but_none_of_its_invites() {
    let company = mailbox_company();
    let owner = team_member(company.id, company.user_id, "dana", "owner");
    let invite = team_invite(company.id, "kim@example.com", "pending");

    let list = TeamSettingsList {
        company: &company,
        members: std::slice::from_ref(&owner),
        // The route hands a member an empty list; the renderer must not offer the section anyway.
        invites: std::slice::from_ref(&invite),
        selected: TeamSelection::None,
        role: TeamRole::Member,
    };
    let html = team_tab(
        &list,
        &team_settings_empty_pane("Select someone.", FragmentSwap::Inline),
    );

    assert!(html.contains("dana"));
    assert!(!html.contains(">Invites</li>"));
    assert!(!html.contains("kim@example.com"));
    // Nothing to manage, so no invite button either.
    assert!(!html.contains("Invite Person"));
}

#[test]
fn the_member_pane_offers_remove_to_the_owner_and_never_for_the_owner() {
    let company = mailbox_company();
    let sam = team_member(company.id, Uuid::new_v4(), "sam", "member");
    let owner = team_member(company.id, company.user_id, "dana", "member");

    let removal_endpoint = format!(
        r##"hx-post="/ui/companies/{}/team/members/{}/removal""##,
        company.id, sam.user_id
    );
    let removable = member_pane(&MemberPane {
        company: &company,
        member: &sam,
        role: TeamRole::Owner,
        viewer_id: Uuid::new_v4(),
        avatar_draft: None,
        error: None,
    });
    assert!(removable.contains(&removal_endpoint));
    assert!(removable.contains("hx-confirm=\"Remove sam"));
    assert!(removable.contains("Access Role"));
    assert!(removable.contains(&format!(
        r##"hx-put="/ui/companies/{}/team/members/{}""##,
        company.id, sam.user_id
    )));
    assert!(removable.contains(r#"<option value="member" selected>Member</option>"#));
    assert!(removable.contains(r#"<option value="admin">Admin</option>"#));

    // The owner's own row: the use case refuses it, so the pane does not offer it.
    let owner_pane = member_pane(&MemberPane {
        company: &company,
        member: &owner,
        role: TeamRole::Owner,
        viewer_id: Uuid::new_v4(),
        avatar_draft: None,
        error: None,
    });
    assert!(!owner_pane.contains("/removal"));
    assert!(!owner_pane.contains("Access Role"));
    assert!(owner_pane.contains("cannot be removed"));
    // The stored role says "member", but owning the company outranks it.
    assert!(owner_pane.contains(">owner</span>"));

    // An ordinary member looking at somebody else gets a read-only pane.
    let read_only = member_pane(&MemberPane {
        company: &company,
        member: &sam,
        role: TeamRole::Member,
        viewer_id: Uuid::new_v4(),
        avatar_draft: None,
        error: None,
    });
    assert!(!read_only.contains("/removal"));
    assert!(!read_only.contains("Access Role"));
    assert!(read_only.contains("Only the company owner"));
}

#[test]
fn the_avatar_picker_uploads_the_picked_file_and_carries_the_url_hidden() {
    let stored = AvatarUrl::from("https://cdn.example.com/avatars/a.png");
    let picked = avatar_picker(&AvatarPicker {
        field_id: "member-avatar",
        avatar_url: Some(&stored),
        name: "Dana",
        label: "Your Picture",
        error: None,
    });

    // The field is its own swap target, so the route can answer with exactly this fragment.
    assert!(picked.contains(r#"id="member-avatar""#));
    assert!(picked.contains(r#"hx-post="/ui/uploads/avatar""#));
    assert!(picked.contains(r#"hx-encoding="multipart/form-data""#));
    assert!(picked.contains(r##"hx-target="#member-avatar""##));
    // Only the picker's own fields go up with the file, not the form it happens to sit in.
    assert!(picked.contains(r#"hx-params="avatar_file,avatar_url,avatar_field_id,avatar_name""#));
    // What the surrounding form actually saves.
    assert!(picked.contains(
        r#"<input type="hidden" name="avatar_url" value="https://cdn.example.com/avatars/a.png">"#
    ));
    // Only a picture can be removed.
    assert!(picked.contains(r#"hx-post="/ui/uploads/avatar/clear""#));

    let empty = avatar_picker(&AvatarPicker {
        field_id: "member-avatar",
        avatar_url: None,
        name: "Dana",
        label: "Your Picture",
        error: None,
    });
    assert!(empty.contains(r#"<input type="hidden" name="avatar_url" value="">"#));
    assert!(!empty.contains("/ui/uploads/avatar/clear"));
    // No picture yet, so the letter is what shows.
    assert!(empty.contains(">D<"));
}

#[test]
fn the_avatar_picker_escapes_what_it_sends_back_up() {
    let picked = avatar_picker(&AvatarPicker {
        field_id: "member-avatar",
        avatar_url: None,
        name: r#"Dana" onload="alert(1)"#,
        label: "Your Picture",
        error: Some(r#"<script>alert(1)</script>"#),
    });

    // The name travels in `hx-vals` as JSON inside an attribute, so it is escaped twice over.
    assert!(!picked.contains(r#"onload="alert(1)"#));
    assert!(picked.contains("&quot;"));
    assert!(!picked.contains("<script>"));
}

#[test]
fn each_agent_form_owns_its_own_picker() {
    let company = mailbox_company();

    // Both create forms are rendered at once -- one hidden behind a tab -- so their pickers must
    // not swap each other when a file is picked.
    let create = agent_create_pane(&AgentCreatePane {
        company: &company,
        app_domain_name: "mail.test",
        model_connections: &[],
        library_agents: &[],
        selected_library_agent_ids: &[],
        tab: AgentCreateTab::Simple,
        draft: &AgentDraft::default(),
        error: None,
        capability_options: AgentCapabilityOptions::default(),
    });
    assert!(create.contains(r#"id="agent-avatar-simple""#));
    assert!(create.contains(r#"id="agent-avatar-new""#));
    assert!(!create.contains(r#"type="url" name="avatar_url""#));

    let agent = Agent {
        avatar_url: Some(AvatarUrl::from("https://cdn.example.com/bot.png")),
        ..settings_agent(company.id, "Triage", "triage")
    };
    let edit = agent_edit_pane(&AgentEditPane {
        company: &company,
        app_domain_name: "mail.test",
        model_connections: &[],
        agent: &agent,
        draft: None,
        error: None,
        capability_options: AgentCapabilityOptions::default(),
    });
    assert!(edit.contains(&format!(r#"id="agent-avatar-{}""#, agent.id)));
    assert!(edit.contains(
        r#"<input type="hidden" name="avatar_url" value="https://cdn.example.com/bot.png">"#
    ));
    // The picked file is uploaded before the form is saved, so it never rides along with it.
    assert!(edit.contains(r#"hx-params="not avatar_file""#));
}

#[test]
fn only_your_own_member_pane_offers_the_avatar_field() {
    let company = mailbox_company();
    let dana = CompanyMember {
        avatar_url: Some(AvatarUrl::from("https://example.com/dana.png")),
        ..team_member(company.id, Uuid::new_v4(), "dana", "member")
    };

    let own = member_pane(&MemberPane {
        company: &company,
        member: &dana,
        role: TeamRole::Member,
        viewer_id: dana.user_id,
        avatar_draft: None,
        error: None,
    });
    assert!(own.contains(&format!(
        r##"hx-put="/ui/companies/{}/team/members/{}/avatar""##,
        company.id, dana.user_id
    )));
    // The picture is picked from disk; the URL it was stored at rides along hidden, so the save
    // is still the same `avatar_url` field.
    assert!(own.contains(r#"type="file" name="avatar_file""#));
    assert!(!own.contains(r#"type="url" name="avatar_url""#));
    assert!(own.contains(
        r#"<input type="hidden" name="avatar_url" value="https://example.com/dana.png">"#
    ));
    assert!(own.contains(r#"src="https://example.com/dana.png""#));

    // The owner looking at somebody else sees their picture but no way to change it.
    let someone_else = member_pane(&MemberPane {
        company: &company,
        member: &dana,
        role: TeamRole::Owner,
        viewer_id: company.user_id,
        avatar_draft: None,
        error: None,
    });
    assert!(!someone_else.contains("/avatar\""));
    assert!(someone_else.contains(r#"src="https://example.com/dana.png""#));

    // A rejected save comes back with what was typed, not with what is stored.
    let rejected = member_pane(&MemberPane {
        company: &company,
        member: &dana,
        role: TeamRole::Member,
        viewer_id: dana.user_id,
        avatar_draft: Some("javascript:alert(1)"),
        error: Some("An avatar URL must start with http:// or https://."),
    });
    // ...but never as something a page would render: a draft that is not an `http` URL is shown
    // as the reason it was refused, not put back into the field it came from.
    assert!(!rejected.contains("javascript:alert(1)"));
    assert!(rejected.contains("must start with http://"));
}

#[test]
fn the_invite_pane_only_edits_an_invite_that_is_still_pending() {
    let company = mailbox_company();
    let pending = team_invite(company.id, "kim@example.com", "pending");
    let accepted = team_invite(company.id, "sam@example.com", "accepted");

    let editable = invite_pane(&InvitePane {
        company: &company,
        invite: &pending,
        role: TeamRole::Owner,
        email_draft: None,
        role_draft: None,
        error: None,
    });
    assert!(editable.contains(&format!(
        r##"hx-put="/ui/companies/{}/team/invites/{}""##,
        company.id, pending.id
    )));
    assert!(editable.contains(r##"value="kim@example.com""##));
    assert!(editable.contains(r#"<select name="role""#));
    assert!(editable.contains(r#"<option value="member" selected>Member</option>"#));
    assert!(editable.contains("Cancel Invite"));

    // An answered invite is a record: rewriting its address would rewrite what somebody accepted.
    let settled = invite_pane(&InvitePane {
        company: &company,
        invite: &accepted,
        role: TeamRole::Owner,
        email_draft: None,
        role_draft: None,
        error: None,
    });
    assert!(!settled.contains("hx-put="));
    assert!(settled.contains("already accepted"));
    assert!(settled.contains("Delete Record"));
    assert!(settled.contains("badge-success"));

    // A non-owner cannot even delete the record.
    let read_only = invite_pane(&InvitePane {
        company: &company,
        invite: &pending,
        role: TeamRole::Member,
        email_draft: None,
        role_draft: None,
        error: None,
    });
    assert!(!read_only.contains("hx-put="));
    assert!(!read_only.contains("hx-delete="));
}

#[test]
fn the_invite_forms_keep_a_rejected_submit_in_the_form() {
    let company = mailbox_company();
    let invite = team_invite(company.id, "kim@example.com", "pending");

    let rejected_edit = invite_pane(&InvitePane {
        company: &company,
        invite: &invite,
        role: TeamRole::Owner,
        email_draft: Some("typo@example"),
        role_draft: Some(CompanyAccessRole::Admin),
        error: Some("Please provide a valid email address."),
    });
    assert!(rejected_edit.contains("Please provide a valid email address."));
    assert!(rejected_edit.contains(r##"value="typo@example""##));
    assert!(rejected_edit.contains(r#"<option value="admin" selected>Admin</option>"#));
    // The header still names the stored invite — only the form carries what was typed.
    assert!(rejected_edit.contains(">kim@example.com</h2>"));

    let rejected_create = invite_create_pane(&InviteCreatePane {
        company: &company,
        email_draft: "typo@example",
        role_draft: CompanyAccessRole::Admin,
        error: Some("Please provide a valid email address."),
    });
    assert!(rejected_create.contains("Please provide a valid email address."));
    assert!(rejected_create.contains(r##"value="typo@example""##));
    assert!(rejected_create.contains(r#"<option value="admin" selected>Admin</option>"#));
    assert!(rejected_create.contains(&format!(
        r##"hx-post="/ui/companies/{}/team/invites""##,
        company.id
    )));
    assert!(rejected_create.contains(&format!(
        r##"hx-get="/ui/companies/{}/team/close""##,
        company.id
    )));
}

/// One queued delivery, as the producer would have written it.
fn queued_delivery(company_id: Uuid, channel_id: Uuid) -> DeliveryEntry {
    DeliveryEntry {
        id: DeliveryId::random(),
        company_id,
        channel_id,
        message_id: CanonicalMessageId::random(),
        correlation_id: CorrelationId::new(),
        transport: TransportKind::Email,
        purpose: DeliveryPurpose::Reply,
        status: DeliveryStatus::Pending,
        idempotency_key: "reply:task:reply:email:customer@example.com".to_string(),
        destination_label: "support@acme".to_string(),
        external_destination: Some("customer@example.com".to_string()),
        subject: "Re: order <script>".to_string(),
        attempt_count: 0,
        max_attempts: 5,
        last_error_class: None,
        last_error_detail: None,
        parts: Vec::new(),
        available_at: Utc::now(),
        delivered_at: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn delivery_filter() -> DeliveryFilter {
    DeliveryFilter::new(DeliveryQuery::default())
}

#[test]
fn deliveries_page_lists_queued_work_and_escapes_its_subject() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let entry = queued_delivery(company.id, channel.id);
    let email = mailbox_account_email();
    let filter = delivery_filter();

    let list = DeliveryList {
        company: &company,
        entries: std::slice::from_ref(&entry),
        filter: &filter,
        has_next: false,
        selected_entry_id: Some(entry.id.as_uuid()),
    };
    let pane = delivery_empty_pane("Select a delivery.", FragmentSwap::Inline);
    let html = deliveries_page(&DeliveriesPage {
        user: &mailbox_user(&email),
        companies: std::slice::from_ref(&company),
        channels: std::slice::from_ref(&channel),
        list: &list,
        pane_html: &pane,
    });

    assert!(html.contains("Re: order &lt;script&gt;"));
    // The shell carries its own <script> tag, so this pins the subject rather than the document.
    assert!(!html.contains("order <script>"));
    assert!(html.contains("customer@example.com"));
    // The transport and the purpose are columns of their own now, not payload archaeology.
    assert!(html.contains("Email · Reply · customer@example.com"));
    // A queued delivery is an undelivered one, and the summary says so before anything is clicked.
    assert!(html.contains("1 delivery · 1 undelivered · page 1"));
    assert!(html.contains("menu-active"));
    assert!(html.contains(&format!(
        r##"hx-get="/ui/deliveries/{}?company_id={}""##,
        entry.id, company.id
    )));
    // The rail lights the workspace the response belongs to.
    assert!(html.contains(r##"title="Deliveries""##));
    // The channel filter is what the channel_id column and its index exist to serve.
    assert!(html.contains(r##"<option value="">All channels</option>"##));
    assert!(html.contains(&format!(
        r##"<option value="{}">Inbox</option>"##,
        channel.id
    )));
    // And a transport filter, which is the whole reason the workspace stopped being email-shaped.
    assert!(html.contains(r##"<option value="">All transports</option>"##));
    assert!(html.contains(r##"<option value="slack">Slack</option>"##));
}

/// An ambiguous outcome is not a failure and not a success, and the pane has to say which -- a
/// reader who treats it as a failure will re-send it by hand, which is the duplicate the whole
/// state exists to prevent.
#[test]
fn delivery_pane_distinguishes_an_unconfirmed_outcome_from_a_dead_letter() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let entry = DeliveryEntry {
        status: DeliveryStatus::OutcomeUnknown,
        last_error_class: Some(FailureClass::Timeout),
        last_error_detail: Some("the relay never acknowledged".to_string()),
        ..queued_delivery(company.id, channel.id)
    };

    let html = delivery_detail_pane(&DeliveryDetailPane {
        company_id: company.id,
        entry: &entry,
        channel: Some(&channel),
    });

    assert!(html.contains("Unconfirmed"));
    assert!(html.contains("deliberately not retried"));
    assert!(html.contains("timeout"));
    assert!(html.contains("the relay never acknowledged"));
    assert!(!html.contains("Every attempt was used up"));
}

/// A multi-part delivery earns one provider key per part. The single `provider_message_id` this
/// replaces could show only the last of them.
#[test]
fn delivery_pane_lists_every_part_and_its_provider_key() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let part = |index: u16, status: DeliveryPartStatus, key: Option<&str>| DeliveryPartEntry {
        id: DeliveryPartId::random(),
        index,
        status,
        provider_message_key: key.map(str::to_string),
        attempt_count: 1,
        last_error_class: None,
        last_error_detail: None,
        delivered_at: None,
    };
    let entry = DeliveryEntry {
        parts: vec![
            part(0, DeliveryPartStatus::Delivered, Some("<one@example.com>")),
            part(1, DeliveryPartStatus::Prepared, None),
        ],
        ..queued_delivery(company.id, channel.id)
    };

    let html = delivery_detail_pane(&DeliveryDetailPane {
        company_id: company.id,
        entry: &entry,
        channel: Some(&channel),
    });

    assert!(html.contains("Provider messages · 1 of 2 confirmed"));
    assert!(html.contains("&lt;one@example.com&gt;"));
    assert!(html.contains("Part 0"));
    assert!(html.contains("Part 1"));
}

#[test]
fn delivery_list_says_when_nothing_matches_and_pages_only_when_there_is_more() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let filter = delivery_filter();

    let empty = delivery_list(
        &DeliveryList {
            company: &company,
            entries: &[],
            filter: &filter,
            has_next: false,
            selected_entry_id: None,
        },
        FragmentSwap::Inline,
    );
    assert!(empty.contains("No delivery matches these filters."));
    assert!(!empty.contains("Older <svg"));

    let entry = queued_delivery(company.id, channel.id);
    let paged = delivery_list(
        &DeliveryList {
            company: &company,
            entries: std::slice::from_ref(&entry),
            filter: &filter,
            has_next: true,
            selected_entry_id: None,
        },
        FragmentSwap::OutOfBand,
    );
    assert!(paged.contains("Older <svg"));
    assert!(paged.contains(r##"hx-swap-oob="outerHTML""##));
    assert!(paged.contains("page=2"));
    assert!(paged.contains("hx-sync=\"#delivery-list:replace\""));
}

#[test]
fn delivery_query_carries_only_what_was_chosen() {
    let company_id = Uuid::new_v4();
    let entry_id = Uuid::new_v4();

    let bare = delivery_query(company_id, &delivery_filter(), None);
    assert_eq!(bare, format!("company_id={company_id}"));

    let channel_id = Uuid::new_v4();
    let filtered = DeliveryFilter::new(DeliveryQuery {
        channel_id: Some(channel_id),
        status: Some(DeliveryStatus::DeadLetter),
        transport: Some(TransportKind::Slack),
        purpose: Some(DeliveryPurpose::Outreach),
        sort_asc: true,
        page: Some(3),
        limit: Some(10),
    });
    let full = delivery_query(company_id, &filtered, Some(entry_id));
    assert_eq!(
        full,
        format!(
            "company_id={company_id}&channel_id={channel_id}&status=dead_letter&transport=slack\
             &purpose=outreach&sort=asc&limit=10&page=3&entry_id={entry_id}"
        )
    );
}

#[test]
fn page_timestamps_preserve_utc_instants_for_local_browser_rendering() {
    let at = chrono::DateTime::parse_from_rfc3339("2026-08-19T14:48:27.123456Z")
        .expect("a valid RFC 3339 instant")
        .with_timezone(&Utc);

    assert_eq!(
        format_date(at),
        r#"<time datetime="2026-08-19T14:48:27.123456+00:00" data-local-time="date">Aug 19, 2026 UTC</time>"#
    );
    assert!(format_date_time(at).contains(r#"data-local-time="date-time""#));
    assert!(format_time(at).contains(r#"data-local-time="time""#));
}

/// A placeholder is two halves that have to meet: the attribute on the swap target, and the
/// script in the shell that reads it. Either one alone renders a workspace that goes blank while
/// it loads, which is what the placeholders exist to stop.
#[test]
fn the_ui_shell_carries_the_placeholder_machinery_for_its_swap_targets() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let thread = mailbox_thread(channel.id);
    let detail = empty_detail_pane("Select a thread.", FragmentSwap::Inline);
    let email = mailbox_account_email();

    let html = mailbox_page(&MailboxPage {
        user: &mailbox_user(&email),
        company: &company,
        companies: std::slice::from_ref(&company),
        app_domain_name: "example.com",
        channels: std::slice::from_ref(&channel),
        selected_channel: Some(&channel),
        threads: std::slice::from_ref(&thread),
        next_cursor: Some("next_cursor"),
        selected_thread_id: None,
        activity: no_activity(),
        detail_html: &detail,
    });

    assert!(html.contains(r##"id="detail-pane" data-skeleton="pane""##));
    assert!(html.contains(r##"id="thread-column" data-skeleton="thread-column""##));
    assert!(html.contains(r##"id="thread-list" data-skeleton="thread-rows""##));

    let script = application_javascript();
    assert!(script.contains("var UI_SKELETONS = {"));
    assert!(script.contains("htmx:beforeRequest"));
    assert!(script.contains("class=\\\"skeleton "));
}

/// The dashboard is the one workspace whose body is not rendered with the page. Its immediate SSE
/// snapshot must be the placeholder's only writer, or a slower load request can overwrite it.
#[test]
fn the_dashboard_shows_a_placeholder_with_one_initial_writer() {
    let company = mailbox_company();
    let email = mailbox_account_email();
    let companies = [company];

    use crate::entities::dashboard::DashboardWindow;

    let html = dashboard_page(&DashboardShell {
        user: &mailbox_user(&email),
        scope: DashboardScopeView::Company(&companies[0]),
        selected_company: Some(&companies[0]),
        companies: &companies,
        window: DashboardWindow::last_hour(),
    });

    assert!(html.contains(r##"data-skeleton="panels""##));
    assert!(html.contains(r##"sse-connect="/ui/dashboard/events?company_id="##));
    assert!(html.contains(r##"sse-swap="dashboard""##));
    assert!(!html.contains(r##"hx-get="/ui/dashboard/panels"##));
    assert!(!html.contains(r##"hx-trigger="load""##));
    assert!(
        html.contains(r##"class="skeleton h-28 w-full""##),
        "the placeholder itself must be in the response, not only paintable later"
    );
}

/// Each placeholder attribute is a constant interpolated into a `format!`. Put one in a plain
/// string literal by mistake and it renders as the constant's own name, in the markup, where the
/// reader can see it — and nothing in the compiler notices. Every pane that starts out empty is
/// checked here, because those are the ones written as bare literals.
#[test]
fn no_pane_leaks_a_placeholder_constant_into_its_markup() {
    let panes = [
        empty_thread_column(),
        empty_detail_pane("Select a thread.", FragmentSwap::Inline),
        agent_settings_empty_pane("Select an agent.", FragmentSwap::Inline),
        channel_settings_empty_pane("Select a channel.", FragmentSwap::Inline),
        company_settings_empty_pane("Select a company.", FragmentSwap::Inline),
        team_settings_empty_pane("Select a member.", FragmentSwap::Inline),
        delivery_empty_pane("Select a delivery.", FragmentSwap::Inline),
    ];

    for pane in panes {
        assert!(
            !pane.contains("_SKELETON"),
            "a placeholder constant reached the markup: {pane}"
        );
        assert!(
            pane.contains("data-skeleton="),
            "the pane opted out of a placeholder: {pane}"
        );
    }
}

/// Every /ui workspace's first column (the sidebar) carries a header with the section name
/// and description, matching the pattern established in /ui/companies.
#[test]
fn every_ui_workspace_first_column_renders_a_sidebar_header() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let email = mailbox_account_email();
    let user = mailbox_user(&email);
    let companies = [company.clone()];

    let mailbox_html = mailbox_page(&MailboxPage {
        user: &user,
        company: &company,
        companies: &companies,
        app_domain_name: "example.com",
        channels: std::slice::from_ref(&channel),
        selected_channel: Some(&channel),
        threads: &[],
        next_cursor: None,
        selected_thread_id: None,
        activity: no_activity(),
        detail_html: "",
    });
    assert!(mailbox_html.contains(
        r##"<h2 class="min-w-0 flex-1 text-base font-semibold leading-tight">Mailbox</h2>"##
    ));

    let channels_html = channel_settings_page(&ChannelSettingsPage {
        user: &user,
        companies: &companies,
        list: &ChannelSettingsList {
            company: &company,
            app_domain_name: "example.com",
            channels: &[],
            selected_channel_id: None,
        },
        pane_html: "",
    });
    assert!(channels_html.contains(
        r##"<h2 class="min-w-0 flex-1 text-base font-semibold leading-tight">Channels</h2>"##
    ));

    let agents_html = agent_settings_page(&AgentSettingsPage {
        user: &user,
        companies: &companies,
        list: &AgentSettingsList {
            company: &company,
            app_domain_name: "mail.test",
            agents: &[],
            selected_agent_id: None,
        },
        pane_html: "",
    });
    assert!(agents_html.contains(
        r##"<h2 class="min-w-0 flex-1 text-base font-semibold leading-tight">Agents</h2>"##
    ));

    let schedules_html = schedules_page(&SchedulesPage {
        user: &user,
        company: &company,
        companies: &companies,
        schedules: &[],
        selected_schedule_id: None,
        bindings: &[],
        runs_html: "",
        pane_html: "",
    });
    assert!(schedules_html.contains(
        r##"<h2 class="min-w-0 flex-1 text-base font-semibold leading-tight">Schedules</h2>"##
    ));

    let deliveries_filter = delivery_filter();
    let deliveries_html = deliveries_page(&DeliveriesPage {
        user: &user,
        companies: &companies,
        channels: &[],
        list: &DeliveryList {
            company: &company,
            entries: &[],
            filter: &deliveries_filter,
            has_next: false,
            selected_entry_id: None,
        },
        pane_html: "",
    });
    assert!(deliveries_html.contains(
        r##"<h2 class="min-w-0 flex-1 text-base font-semibold leading-tight">Deliveries</h2>"##
    ));

    let dashboard_html = dashboard_page(&DashboardShell {
        user: &user,
        scope: DashboardScopeView::Company(&company),
        selected_company: Some(&company),
        companies: &companies,
        window: crate::entities::dashboard::DashboardWindow::last_hour(),
    });
    assert!(dashboard_html.contains(
        r##"<h2 class="min-w-0 flex-1 text-base font-semibold leading-tight">Dashboard</h2>"##
    ));

    let dashboard_global_html = dashboard_page(&DashboardShell {
        user: &user,
        scope: DashboardScopeView::Global,
        selected_company: Some(&company),
        companies: &companies,
        window: crate::entities::dashboard::DashboardWindow::last_hour(),
    });
    assert!(dashboard_global_html.contains(
        r##"<h2 class="min-w-0 flex-1 text-base font-semibold leading-tight">Dashboard</h2>"##
    ));

    let companies_html = company_settings_page(&CompanySettingsPage {
        user: &user,
        list: &CompanySettingsList {
            companies: &companies,
            selected_company_id: Some(company.id),
        },
        rail_company: Some(&company),
        pane_html: "",
    });
    assert!(companies_html.contains(
        r##"<h2 class="min-w-0 flex-1 text-base font-semibold leading-tight">Companies</h2>"##
    ));
}

fn profile_account() -> User {
    User {
        id: Uuid::new_v4(),
        username: "dana".to_string(),
        email: "dana@example.com".to_string(),
        password_hash: "irrelevant".to_string(),
        avatar_url: Some(AvatarUrl::from("https://cdn.example.com/dana.png")),
        created_at: Utc::now(),
    }
}

#[test]
fn the_profile_pane_offers_the_account_its_own_details_and_never_its_password() {
    let account = profile_account();
    let html = profile_pane(&ProfilePane {
        user: &account,
        draft: None,
        pending: &[],
        methods: &LoginMethods {
            password: true,
            google: false,
            apple: false,
        },
        google_enabled: false,
        apple_enabled: false,
        notification_preferences: Default::default(),
        outcome: ProfileOutcome::Untouched,
    });

    // Both forms write to the caller's own account, so neither carries a user id to point
    // somewhere else.
    assert!(html.contains(r##"hx-put="/ui/profile""##));
    assert!(html.contains(r##"hx-put="/ui/profile/password""##));
    assert!(!html.contains(&account.id.to_string()));

    assert!(html.contains(r#"name="username" required value="dana""#));
    assert!(html.contains(r#"name="email" required value="dana@example.com""#));

    // The picture is picked from disk; the URL it was stored at rides along hidden.
    assert!(html.contains(r#"type="file" name="avatar_file""#));
    assert!(html.contains(
        r#"<input type="hidden" name="avatar_url" value="https://cdn.example.com/dana.png">"#
    ));

    // A password field that came back holding what was typed would be a password sitting in the
    // page's HTML.
    assert!(html.contains(r#"name="current_password""#));
    assert!(!html.contains("irrelevant"));
    assert!(!html.contains(r#"name="current_password" value"#));
}

/// P08-Q8: the profile offers exactly two email toggles, each showing its stored value.
#[test]
fn the_profile_offers_the_decision_and_delivery_failure_email_toggles() {
    let account = profile_account();
    let render = |preferences| {
        profile_pane(&ProfilePane {
            user: &account,
            draft: None,
            pending: &[],
            methods: &LoginMethods {
                password: true,
                google: false,
                apple: false,
            },
            google_enabled: false,
            apple_enabled: false,
            notification_preferences: preferences,
            outcome: ProfileOutcome::Untouched,
        })
    };
    let toggle = |name: &str, checked: bool| {
        format!(
            r#"name="{name}" value="true" class="toggle toggle-primary"{}>"#,
            if checked { " checked" } else { "" }
        )
    };

    let html = render(NotificationPreferences {
        delivery_failure_email_enabled: true,
        decision_email_enabled: false,
    });
    assert_eq!(html.matches(r#"class="toggle toggle-primary""#).count(), 2);
    assert!(html.contains("Email me when a workflow decision needs my review"));
    assert!(html.contains(&toggle("decision_email_enabled", false)));
    assert!(html.contains(&toggle("delivery_failure_email_enabled", true)));
    for retired in [
        "assignment_email_enabled",
        "response_review_email_enabled",
        "delegation_timeout_email_enabled",
        "task_failure_email_enabled",
    ] {
        assert!(!html.contains(retired), "{retired}");
    }

    let html = render(NotificationPreferences::default());
    assert!(html.contains(&toggle("decision_email_enabled", true)));
}

#[test]
fn an_oauth_only_profile_can_add_password_and_connect_available_providers() {
    let account = profile_account();
    let html = profile_pane(&ProfilePane {
        user: &account,
        draft: None,
        pending: &[],
        methods: &LoginMethods {
            password: false,
            google: true,
            apple: false,
        },
        google_enabled: true,
        apple_enabled: true,
        notification_preferences: Default::default(),
        outcome: ProfileOutcome::Untouched,
    });

    assert!(html.contains(r##"hx-put="/ui/profile/password/setup""##));
    assert!(!html.contains(r#"name="current_password""#));
    assert!(html.contains("Google"));
    assert!(html.contains("Connected"));
    assert!(html.contains(r##"href="/auth/apple/connect""##));
    assert!(!html.contains(r##"href="/auth/google/connect""##));
}

#[test]
fn a_rejected_profile_shows_what_was_typed_rather_than_what_is_stored() {
    let account = profile_account();
    let html = profile_pane(&ProfilePane {
        user: &account,
        draft: Some(&ProfileDraft {
            username: "<script>dana</script>",
            email: "taken@example.com",
            avatar_url: "javascript:alert(1)",
        }),
        pending: &[],
        methods: &LoginMethods {
            password: true,
            google: false,
            apple: false,
        },
        google_enabled: false,
        apple_enabled: false,
        notification_preferences: Default::default(),
        outcome: ProfileOutcome::Rejected(
            ProfileForm::Identity,
            "An account already uses the address 'taken@example.com'.",
        ),
    });

    assert!(html.contains("already uses the address"));
    assert!(html.contains(r#"value="taken@example.com""#));
    assert!(!html.contains("<script>dana</script>"));
    assert!(html.contains("&lt;script&gt;dana&lt;/script&gt;"));
    // A draft that is not an `http` URL never reaches the `<img src>` the bubble draws.
    assert!(!html.contains("javascript:alert(1)"));
}

#[test]
fn each_profile_banner_belongs_to_the_form_that_earned_it() {
    let account = profile_account();

    let saved = profile_pane(&ProfilePane {
        user: &account,
        draft: None,
        pending: &[],
        methods: &LoginMethods {
            password: true,
            google: false,
            apple: false,
        },
        google_enabled: false,
        apple_enabled: false,
        notification_preferences: Default::default(),
        outcome: ProfileOutcome::Saved(ProfileForm::Password, "Your password has been changed."),
    });
    let (details, password) = saved
        .split_once("<h2 class=\"text-lg font-bold\">Password</h2>")
        .expect("the pane renders both sections");
    assert!(password.contains("Your password has been changed."));
    assert!(!details.contains("Your password has been changed."));

    let refused = profile_pane(&ProfilePane {
        user: &account,
        draft: None,
        pending: &[],
        methods: &LoginMethods {
            password: true,
            google: false,
            apple: false,
        },
        google_enabled: false,
        apple_enabled: false,
        notification_preferences: Default::default(),
        outcome: ProfileOutcome::Rejected(
            ProfileForm::Password,
            "That is not your current password.",
        ),
    });
    assert!(refused.contains("That is not your current password."));
    // A refusal is not a confirmation: the two banners must never render together.
    assert!(!refused.contains("has been changed."));
}

#[test]
fn the_account_chip_swaps_name_address_and_face_as_one() {
    let email = mailbox_account_email();
    let user = MailboxUser {
        avatar_url: Some(&AvatarUrl::from("https://cdn.example.com/dana.png")),
        ..mailbox_user(&email)
    };

    let oob = account_chip(&user, FragmentSwap::OutOfBand);
    assert!(oob.contains(r#"id="account-chip""#));
    assert!(oob.contains(r#"hx-swap-oob="outerHTML""#));
    assert!(oob.contains("dana"));
    assert!(oob.contains(email.as_str()));
    assert!(oob.contains(r#"src="https://cdn.example.com/dana.png""#));

    // In the bar itself the same fragment is rendered inline, or every page load would try to
    // swap a chip that is not there yet.
    assert!(!account_chip(&user, FragmentSwap::Inline).contains("hx-swap-oob"));
}

#[test]
fn the_profile_page_renders_through_the_ui_shell_with_or_without_a_company() {
    let account = profile_account();
    let email = mailbox_account_email();
    let user = mailbox_user(&email);
    let pane = profile_pane(&ProfilePane {
        user: &account,
        draft: None,
        pending: &[],
        methods: &LoginMethods {
            password: true,
            google: false,
            apple: false,
        },
        google_enabled: false,
        apple_enabled: false,
        notification_preferences: Default::default(),
        outcome: ProfileOutcome::Untouched,
    });

    let company = mailbox_company();
    let with_company = profile_page(&ProfilePage {
        user: &user,
        company: Some(&company),
        pane_html: &pane,
    });
    assert!(with_company.contains("<title>Profile"));
    assert!(with_company.contains(r##"<a href="/ui/profile">Profile</a>"##));
    assert!(with_company.contains(&format!("/ui/channels?company_id={}", company.id)));

    // An account with no company yet still reaches its own settings -- there is simply no rail
    // for them to sit beside.
    let without_company = profile_page(&ProfilePage {
        user: &user,
        company: None,
        pane_html: &pane,
    });
    assert!(without_company.contains(r##"hx-put="/ui/profile""##));
    assert!(!without_company.contains("company_id="));
}

#[test]
fn the_agent_library_reaches_the_account_menu_only_for_an_operator() {
    let company = mailbox_company();
    let email = mailbox_account_email();
    let entry = r##"<a href="/ui/agent-library">Agent library</a>"##;

    let reader = mailbox_no_company_page(&mailbox_user(&email));
    assert!(!reader.contains(entry));

    // The rest of the menu is what everyone gets, operator or not -- the library is the only
    // entry the flag adds.
    assert!(reader.contains(r##"<a href="/ui/profile">Profile</a>"##));

    let operator = MailboxUser {
        is_operator: true,
        ..mailbox_user(&email)
    };
    assert!(mailbox_no_company_page(&operator).contains(entry));

    // It is the account menu that carries it, not the rail: the library is global, so it has no
    // company_id to be scoped by.
    let detail = empty_detail_pane("Select a thread.", FragmentSwap::Inline);
    let with_rail = mailbox_page(&MailboxPage {
        user: &operator,
        company: &company,
        companies: std::slice::from_ref(&company),
        app_domain_name: "example.com",
        channels: &[],
        selected_channel: None,
        threads: &[],
        next_cursor: None,
        selected_thread_id: None,
        activity: no_activity(),
        detail_html: &detail,
    });
    assert!(with_rail.contains(entry));
    assert!(!with_rail.contains("/ui/agent-library?company_id="));
}

fn in_fifteen_minutes() -> chrono::DateTime<Utc> {
    Utc::now() + chrono::Duration::minutes(15)
}

#[test]
fn a_section_waiting_on_a_code_asks_for_it_instead_of_offering_its_form_again() {
    let account = profile_account();
    let moving_to = EmailAddress::from("new@example.com");

    let html = profile_pane(&ProfilePane {
        user: &account,
        draft: None,
        pending: &[PendingChange::Email {
            new_email: moving_to.clone(),
            expires_at: in_fifteen_minutes(),
        }],
        methods: &LoginMethods {
            password: true,
            google: false,
            apple: false,
        },
        google_enabled: false,
        apple_enabled: false,
        notification_preferences: Default::default(),
        outcome: ProfileOutcome::Untouched,
    });

    // The address field is gone while its code is outstanding -- asking again would silently void
    // the code already sitting in somebody's inbox.
    assert!(!html.contains(r#"name="email""#));
    assert!(html.contains(r##"hx-post="/ui/profile/changes/email""##));
    assert!(html.contains(r##"hx-delete="/ui/profile/changes/email""##));
    assert!(html.contains(r#"name="code""#));
    // Which inbox to open is the one thing the panel has to say.
    assert!(html.contains("new@example.com"));

    // The other section is untouched: one pending change does not lock the whole pane.
    assert!(html.contains(r#"name="current_password""#));
    assert!(!html.contains(r##"hx-post="/ui/profile/changes/password""##));
}

#[test]
fn a_pending_password_change_leaves_the_account_details_form_alone() {
    let account = profile_account();

    let html = profile_pane(&ProfilePane {
        user: &account,
        draft: None,
        pending: &[PendingChange::Password {
            expires_at: in_fifteen_minutes(),
        }],
        methods: &LoginMethods {
            password: true,
            google: false,
            apple: false,
        },
        google_enabled: false,
        apple_enabled: false,
        notification_preferences: Default::default(),
        outcome: ProfileOutcome::Untouched,
    });

    assert!(html.contains(r##"hx-post="/ui/profile/changes/password""##));
    assert!(!html.contains(r#"name="current_password""#));
    // This code goes to the address the account already has, not to anything on a form.
    assert!(html.contains("dana@example.com"));

    assert!(html.contains(r#"name="email" required value="dana@example.com""#));
    assert!(!html.contains(r##"hx-post="/ui/profile/changes/email""##));
}

#[test]
fn a_pending_address_is_never_shown_as_the_account_s_own() {
    let account = profile_account();
    let html = profile_pane(&ProfilePane {
        user: &account,
        draft: None,
        pending: &[PendingChange::Email {
            new_email: EmailAddress::from("<script>x</script>@example.com"),
            expires_at: in_fifteen_minutes(),
        }],
        methods: &LoginMethods {
            password: true,
            google: false,
            apple: false,
        },
        google_enabled: false,
        apple_enabled: false,
        notification_preferences: Default::default(),
        outcome: ProfileOutcome::Saved(
            ProfileForm::Identity,
            "Your name and picture are saved. Check the new address for the code.",
        ),
    });

    // The header still reads the stored address: nothing about an unconfirmed one is the account's.
    assert!(html.contains("dana@example.com"));
    assert!(!html.contains("<script>x</script>"));
    assert!(html.contains("&lt;script&gt;x&lt;/script&gt;@example.com"));
}

#[test]
fn the_channel_form_offers_a_description_and_escapes_what_was_typed_into_it() {
    let company = mailbox_company();
    let draft = ChannelDraft {
        name: "Supplier Desk",
        description: "Answers <supplier> capacity questions.",
        ..ChannelDraft::default()
    };

    let html = channel_create_pane(&ChannelCreatePane {
        company: &company,
        app_domain_name: "mailagents.com",
        spam_scan_enabled: true,
        draft: &draft,
        error: None,
    });

    assert_eq!(html.matches(r#"name="description""#).count(), 1);
    assert!(html.contains("Answers &lt;supplier&gt; capacity questions."));
    assert!(!html.contains("<supplier>"));
}

/// Every handler the browser runs must come from `/assets/app.js`, because the response headers
/// set `script-src 'self'` with no `'unsafe-inline'` (see `adapters::http::security`). An inline
/// `onclick`, an `hx-on` expression, or an inline `<script>` block is therefore not merely untidy
/// — it is dead on arrival, silently. This walks the HTTP adapter's own source so a page added
/// later is covered without anyone remembering to list it here.
#[test]
fn rendered_markup_carries_no_inline_javascript() {
    fn collect(dir: &std::path::Path, into: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("readable source directory") {
            let path = entry.expect("readable entry").path();
            if path.is_dir() {
                collect(&path, into);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                into.push(path);
            }
        }
    }

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/adapters/http");
    let mut sources = Vec::new();
    collect(&root, &mut sources);
    assert!(
        sources.len() > 20,
        "expected to walk the HTTP adapter, found {} files",
        sources.len()
    );

    let mut offenders = Vec::new();
    for path in sources {
        // This file deliberately holds hostile markup as test input.
        if path.ends_with("tests.rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("readable source file");
        for (index, line) in source.lines().enumerate() {
            // Prose about these constructs is not an emission of them.
            if line.trim_start().starts_with("//") {
                continue;
            }
            let mut found = Vec::new();
            if inline_event_attribute(line) {
                found.push("inline event attribute");
            }
            if line.contains("hx-on") {
                found.push("hx-on expression");
            }
            // `<script src=...>` is fine; a `<script>` that carries a body is not. A `<script>`
            // that closes a Rust string literal right after the tag is hostile test input.
            if line.contains("<script>") && !line.contains("<script>\"") {
                found.push("inline <script> block");
            }
            for what in found {
                offenders.push(format!("{}:{}: {what}", path.display(), index + 1));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "inline JavaScript is blocked by the page CSP and will never run:\n{}",
        offenders.join("\n")
    );
}

/// True when the line carries an HTML event-handler attribute such as ` onclick="`.
fn inline_event_attribute(line: &str) -> bool {
    line.match_indices(" on").any(|(at, _)| {
        let rest = &line[at + 3..];
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_lowercase())
            .collect();
        !name.is_empty() && rest[name.len()..].starts_with("=\"")
    })
}

/// The delegated dispatch is the other half of the same contract: every `data-action`,
/// `data-input`, `data-keydown`, `data-submit` and `data-after-request` value the pages emit has
/// to have a branch in the bundle, or the control is inert in exactly the way the CSP made the
/// inline handlers inert.
#[test]
fn every_delegated_action_has_a_branch_in_the_bundle() {
    let bundle = application_javascript();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/adapters/http");

    let mut missing = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("readable source directory") {
            let path = entry.expect("readable entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().is_none_or(|ext| ext != "rs") || path.ends_with("tests.rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("readable source file");
            for attribute in [
                "data-action=\"",
                "data-input=\"",
                "data-keydown=\"",
                "data-submit=\"",
                "data-after-request=\"",
                "data-after-swap=\"",
            ] {
                for (at, _) in source.match_indices(attribute) {
                    let rest = &source[at + attribute.len()..];
                    let Some(value) = rest.split('"').next() else {
                        continue;
                    };
                    // Values built at runtime cannot be checked statically.
                    if value.is_empty() || value.contains('{') {
                        continue;
                    }
                    if !bundle.contains(&format!("'{value}'"))
                        && !bundle.contains(&format!("\"{value}\""))
                    {
                        missing.push(format!("{}: {attribute}{value}\"", path.display()));
                    }
                }
            }
        }
    }

    missing.sort();
    missing.dedup();
    assert!(
        missing.is_empty(),
        "these delegated actions have no branch in /assets/app.js:\n{}",
        missing.join("\n")
    );
}

/// The bundle is one flat scope shared by every workspace, so a second `function name(...)` does
/// not add a variant -- it silently replaces the first one everywhere. `selectSidebarItem` and
/// `selectThreadRow` were each declared twice, and the schedules copies won even on the mailbox
/// page that defined them, which is why picking a channel stopped closing the mailbox menu.
#[test]
fn no_function_name_is_declared_twice_in_the_bundle() {
    let bundle = application_javascript();

    let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for (at, _) in bundle.match_indices("function ") {
        let rest = &bundle[at + "function ".len()..];
        let name = rest.split('(').next().unwrap_or_default().trim();
        // Anonymous callbacks carry no name to collide on.
        if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        *counts.entry(name).or_default() += 1;
    }

    let duplicates: Vec<String> = counts
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(name, count)| format!("{name} ({count} declarations)"))
        .collect();

    assert!(
        duplicates.is_empty(),
        "these functions are declared more than once in /assets/app.js, so the last one wins on \
         every page:\n{}",
        duplicates.join("\n")
    );
}

#[test]
#[ignore = "developer aid: dumps /assets/app.js so it can be syntax-checked with node"]
fn dump_application_javascript() {
    std::fs::write(
        std::env::var("APP_JS_OUT").expect("APP_JS_OUT"),
        application_javascript(),
    )
    .expect("write bundle");
}

fn test_thread_runs(activity: ThreadActivity) -> crate::application::thread_activity::ThreadRuns {
    crate::application::thread_activity::ThreadRuns {
        inspectable: Default::default(),
        items: vec![crate::application::thread_activity::ThreadRunActivity {
            run_id: Uuid::new_v4(),
            title: "Workflow".into(),
            activity,
        }],
        has_more: false,
        message_runs: vec![],
    }
}

#[path = "workflow_conversation_tests.rs"]
mod workflow_conversation_tests;
