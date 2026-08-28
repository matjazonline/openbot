use super::*;
use crate::use_cases::user::LoginMethods;
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
    assert!(html.contains("Used by this library agent"));
    assert!(!html.contains("Overrides the company key"));
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
        worker_id: None,
        execution_generation: None,
        locked_at: None,
        lock_expires_at: None,
        run_at: Utc::now(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
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
        worker_id: None,
        execution_generation: None,
        locked_at: None,
        lock_expires_at: None,
        run_at: Utc::now(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let tasks = vec![task1, task2];

    let msg_in1 = Message {
        id: Uuid::new_v4(),
        thread_id,
        message_id: "<in1@test.com>".into(),
        in_reply_to: None,
        references_list: vec![],
        sender: "user@test.com".into(),
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
        created_at: Utc::now(),
    };

    let msg_out1 = Message {
        id: Uuid::new_v4(),
        thread_id,
        message_id: "<out1@test.com>".into(),
        in_reply_to: Some("<in1@test.com>".into()),
        references_list: vec![],
        sender: "agent@test.com".into(),
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
        created_at: Utc::now(),
    };

    let msg_in2 = Message {
        id: Uuid::new_v4(),
        thread_id,
        message_id: "<in2@test.com>".into(),
        in_reply_to: Some("<out1@test.com>".into()),
        references_list: vec![],
        sender: "user@test.com".into(),
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
        created_at: Utc::now(),
    };

    let msg_out2 = Message {
        id: Uuid::new_v4(),
        thread_id,
        message_id: "<out2@test.com>".into(),
        in_reply_to: Some("<in2@test.com>".into()),
        references_list: vec![],
        sender: "agent@test.com".into(),
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
        created_at: Utc::now(),
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

/// None of these fixtures has a task in flight; the activity states have their own tests below.
fn no_activity() -> &'static HashMap<Uuid, ThreadActivity> {
    static EMPTY: std::sync::OnceLock<HashMap<Uuid, ThreadActivity>> = std::sync::OnceLock::new();
    EMPTY.get_or_init(HashMap::new)
}

fn mailbox_company() -> Company {
    Company {
        id: Uuid::new_v4(),
        user_id: Uuid::new_v4(),
        name: "Acme".to_string(),
        slug: "acme".into(),
        api_key: None,
        provider: None,
        model: None,
        enable_llm_spam_guardrail: None,
        avatar_url: None,
        memory_provider: None,
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
        api_key: None,
        provider: None,
        model: None,
        participant_emails: Some(vec!["person@example.com".into()]),
        agent_ids: None,
        channel_config: None,
        retrieve_company_memory: false,
        retrieve_agent_memory: false,
        retrieve_user_memory: false,
        persist_company_memory: false,
        persist_agent_memory: false,
        persist_user_memory: false,
        memory_persistence_mode: crate::entities::memory::MemoryPersistenceMode::AudienceOnly,
        memory_recall_mode: crate::entities::memory::MemoryRecallMode::Fast,
        memory_max_results: 5,
        created_by: crate::entities::creation::CreationProvenance::system(),
        created_at: Utc::now(),
    }
}

fn mailbox_thread(channel_id: Uuid) -> Thread {
    Thread {
        id: Uuid::new_v4(),
        channel_id,
        subject: "Question <script>".to_string(),
        participant_emails: vec!["person@example.com".into()],
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
        agent: None,
        viewer_email: &email,
        activity: None,
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
        "/ui/tasks",
        "/ui/outbox",
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
        "/ui/tasks",
        "/ui/outbox",
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
        api_key: Some("sk-test".to_string()),
        system_prompt: Some("Answer <billing> questions.".to_string()),
        description: None,
        config_json: Some(serde_json::json!({ "temperature": 0.2 })),
        ..settings_agent(company.id, "Triage <bot>", "triage")
    };

    let html = agent_edit_pane(&AgentEditPane {
        company: &company,
        agent: &agent,
        used_by: &[],
        draft: None,
        error: None,
    });

    assert!(html.contains(&format!(
        "hx-put=\"/ui/agents/{}?company_id={}\"",
        agent.id, company.id
    )));
    assert!(html.contains("value=\"triage\""));
    assert!(html.contains("value=\"openai\""));
    assert!(html.contains("value=\"gpt-4o\""));
    assert!(html.contains("Answer &lt;billing&gt; questions.</textarea>"));
    assert!(html.contains("\"temperature\": 0.2"));
    // Overrides are set, so the collapsed section starts open.
    assert!(html.contains("bg-base-200\" open"));
    // The name is escaped everywhere it appears, including inside the confirm prompt.
    assert!(!html.contains("Triage <bot>"));
    assert!(html.contains("Triage &lt;bot&gt;"));
    assert!(html.contains(&format!(
        "hx-delete=\"/ui/agents/{}?company_id={}\"",
        agent.id, company.id
    )));
    assert!(html.contains("No channel is running it."));

    // An agent with nothing overridden keeps the model section collapsed.
    let plain = settings_agent(company.id, "Plain", "plain");
    let plain_html = agent_edit_pane(&AgentEditPane {
        company: &company,
        agent: &plain,
        used_by: &[],
        draft: None,
        error: None,
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
        agent: &pictured,
        used_by: &[],
        draft: None,
        error: None,
    });
    assert!(html.contains(r#"src="https://example.com/triage.png""#));
    // The form round-trips it, so saving from this pane cannot silently clear the picture.
    assert!(html.contains(r#"name="avatar_url" value="https://example.com/triage.png""#));
    // The letter is rendered underneath, for a URL that fails to load.
    assert!(html.contains(">T</span>"));

    let plain_html = agent_edit_pane(&AgentEditPane {
        company: &company,
        agent: &plain,
        used_by: &[],
        draft: None,
        error: None,
    });
    assert!(!plain_html.contains("<img"));
    assert!(plain_html.contains(">P</span>"));

    // The sidebar shows the same face as the pane.
    let agents = [pictured];
    let list = agent_settings_list(
        &AgentSettingsList {
            company: &company,
            agents: &agents,
            selected_agent_id: None,
        },
        FragmentSwap::Inline,
    );
    assert!(list.contains(r#"src="https://example.com/triage.png""#));
}

#[test]
fn agent_edit_pane_lists_the_channels_running_the_agent() {
    let company = mailbox_company();
    let agent = settings_agent(company.id, "Triage", "triage");
    let channel = mailbox_channel(company.id);

    let html = agent_edit_pane(&AgentEditPane {
        company: &company,
        agent: &agent,
        used_by: &[&channel],
        draft: None,
        error: None,
    });

    assert!(html.contains("Run by"));
    assert!(html.contains(&format!(
        "/ui/channels?company_id={}&channel_id={}",
        company.id, channel.id
    )));
    // Deleting it is not free, and the confirmation has to say so.
    assert!(html.contains("1 channel is running it and will stop."));
}

#[test]
fn agent_edit_pane_keeps_a_rejected_save_in_the_form() {
    let company = mailbox_company();
    let agent = settings_agent(company.id, "Triage", "triage");
    let draft = AgentDraft {
        name: "Renamed",
        slug: "renamed",
        system_prompt: "Be terse.",
        config_json: "{ not json",
        advanced: true,
        ..AgentDraft::default()
    };

    let html = agent_edit_pane(&AgentEditPane {
        company: &company,
        agent: &agent,
        used_by: &[],
        draft: Some(&draft),
        error: Some("Invalid JSON config"),
    });

    assert!(html.contains("alert alert-error"));
    assert!(html.contains("Invalid JSON config"));
    assert!(html.contains("value=\"Renamed\""));
    assert!(html.contains("Be terse.</textarea>"));
    assert!(html.contains("{ not json</textarea>"));
    // The header still names the stored agent; only the form carries the attempt.
    assert!(html.contains("<h2 class=\"truncate text-xl font-bold\">Triage</h2>"));
}

#[test]
fn prompt_generator_names_the_pane_it_answers_into() {
    let company = mailbox_company();
    let agent = settings_agent(company.id, "Triage", "triage");

    let edit = agent_edit_pane(&AgentEditPane {
        company: &company,
        agent: &agent,
        used_by: &[],
        draft: None,
        error: None,
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
        draft: &AgentDraft::default(),
        error: None,
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
fn agent_create_pane_opens_on_the_tab_that_was_submitted() {
    let company = mailbox_company();
    let pane = |advanced| {
        agent_create_pane(&AgentCreatePane {
            company: &company,
            draft: &AgentDraft {
                name: "Triage",
                advanced,
                ..AgentDraft::default()
            },
            error: None,
        })
    };

    let simple = pane(false);
    assert!(simple.contains(&format!("hx-post=\"/ui/agents?company_id={}\"", company.id)));
    assert!(simple.contains("name=\"form_mode\" value=\"simple\""));
    assert!(simple.contains("name=\"form_mode\" value=\"advanced\""));
    assert!(simple.contains(r##"<form id="agent-tab-advanced" class="hidden space-y-4""##));
    assert!(!simple.contains(r##"<form id="agent-tab-simple" class="hidden space-y-4""##));

    // A rejected Advanced submit has to come back on the Advanced tab.
    let advanced = pane(true);
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
        api_key: None,
        system_prompt: None,
        description: None,
        config_json: None,
        avatar_url: None,
        created_by: crate::entities::creation::CreationProvenance::system(),
        created_at: Utc::now(),
    }
}

#[test]
fn channel_edit_pane_prefills_the_stored_channel_and_offers_delete() {
    let company = mailbox_company();
    let triage = settings_agent(company.id, "Triage <bot>", "triage");
    let unused = settings_agent(company.id, "Unused", "unused");
    let channel = Channel {
        participant_emails: Some(vec!["person@example.com".into(), "@public".into()]),
        agent_ids: Some(vec![triage.id]),
        channel_config: Some(serde_json::json!({ "mode": "async" })),
        provider: Some("openai".to_string()),
        ..mailbox_channel(company.id)
    };

    let html = channel_edit_pane(&ChannelEditPane {
        company: &company,
        app_domain_name: "example.com",
        channel: &channel,
        agents: &[triage.clone(), unused.clone()],
        schedules: &[],
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
    assert!(html.contains("value=\"openai\""));
    assert!(html.contains("&quot;mode&quot;: &quot;async&quot;\n}</textarea>"));

    // Only the assigned agent is checked, and the hidden field carries the submitted order.
    assert!(html.contains(&format!("value=\"{}\" checked", triage.id)));
    assert!(html.contains(&format!("value=\"{}\" ", unused.id)));
    assert!(!html.contains(&format!("value=\"{}\" checked", unused.id)));
    assert!(html.contains(&format!(
        "<input type=\"hidden\" name=\"agent_ids\" value=\"{}\">",
        triage.id
    )));
    assert!(html.contains("Triage &lt;bot&gt;"));

    // Overrides are already set, so their section arrives open rather than collapsed.
    assert!(html.contains("bg-base-200\" open>"));

    assert!(html.contains(&format!(
        "hx-delete=\"/ui/channels/{}?company_id={}\"",
        channel.id, company.id
    )));
    assert!(html.contains("hx-confirm="));

    // Spam scanning is on, so the interlock has nothing to ask.
    assert!(!html.contains("confirm_spam_disabled"));
}

/// A library definition is picked on what it does, so the pane opens a modal of cards rather than
/// a dropdown that could only carry names.
#[test]
fn library_agents_are_picked_from_a_modal_of_cards() {
    let company = mailbox_company();
    let scheduler = Agent {
        company_id: None,
        description: Some("Books meetings from the thread.".to_string()),
        ..settings_agent(company.id, "Scheduler", "scheduler")
    };
    let researcher = Agent {
        company_id: None,
        ..settings_agent(company.id, "Researcher", "researcher")
    };
    let channel = Channel {
        agent_ids: Some(vec![scheduler.id]),
        ..mailbox_channel(company.id)
    };

    let html = channel_edit_pane(&ChannelEditPane {
        company: &company,
        app_domain_name: "example.com",
        channel: &channel,
        agents: &[scheduler.clone(), researcher.clone()],
        schedules: &[],
        spam_scan_enabled: true,
        draft: None,
        error: None,
    });

    // The button names the current pick and opens the modal; the choice itself rides a hidden
    // field, since a card is a button and cannot be submitted.
    assert!(html.contains(r##"data-action="open-dialog""##));
    assert!(html.contains(&format!(
        "<input type=\"hidden\" class=\"channel-library-agent-field\" value=\"{}\">",
        scheduler.id
    )));
    assert!(html.contains("data-placeholder=\"Choose a library agent…\">Scheduler</span>"));

    // Both definitions are cards, and only the assigned one is marked.
    assert!(html.contains(&format!("data-agent-id=\"{}\"", scheduler.id)));
    assert!(html.contains(&format!("data-agent-id=\"{}\"", researcher.id)));
    assert_eq!(html.matches("channel-library-agent-card").count(), 2);
    assert_eq!(html.matches("border-2 border-primary").count(), 1);

    // The card carries what the dropdown could not.
    assert!(html.contains("Books meetings from the thread."));
    assert!(html.contains("@scheduler"));

    // A nested <form> would be dropped by the parser and could submit the pane, so the backdrop
    // closes the dialog itself.
    assert!(!html.contains("modal-backdrop\">\n"));
    assert!(html.contains("class=\"modal-backdrop\""));
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
        agents: &[],
        schedules: &[],
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
        agents: &[],
        schedules: &[],
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
        channel_config: "{ not json",
        advanced: true,
        ..ChannelDraft::default()
    };

    let html = channel_edit_pane(&ChannelEditPane {
        company: &company,
        app_domain_name: "example.com",
        channel: &channel,
        agents: &[],
        schedules: &[],
        spam_scan_enabled: false,
        draft: Some(&draft),
        error: Some("Invalid JSON config"),
    });

    assert!(html.contains("alert alert-error"));
    assert!(html.contains("Invalid JSON config"));
    assert!(html.contains("value=\"Renamed\""));
    assert!(html.contains("{ not json</textarea>"));
    // The header still names the stored channel; only the form carries the attempt.
    assert!(html.contains("<h2 class=\"truncate text-xl font-bold\">Inbox</h2>"));
    // The draft names @public, so the confirmation the use case will demand is live.
    assert!(
        html.contains(
            "name=\"confirm_spam_disabled\" value=\"true\" class=\"checkbox checkbox-sm\">"
        )
    );
    assert!(!html.contains("opacity-40"));
    // No agents exist yet, so the field still submits and points at where to make one.
    assert!(html.contains("<input type=\"hidden\" name=\"agent_ids\" value=\"\">"));
    assert!(html.contains(&format!("/ui/agents?company_id={}&amp;new=1", company.id)));
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
            advanced: true,
            ..ChannelDraft::default()
        };
        channel_edit_pane(&ChannelEditPane {
            company: &company,
            app_domain_name: "example.com",
            channel: &channel,
            agents: &[],
            schedules: &[],
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
        agents: &[],
        schedules: &[],
        spam_scan_enabled: true,
        draft: None,
        error: None,
    });
    assert!(edit.contains(&close));

    // All three create tabs offer the same way out.
    let create = channel_create_pane(&ChannelCreatePane {
        company: &company,
        app_domain_name: "example.com",
        agents: &[],
        spam_scan_enabled: true,
        draft: &ChannelDraft::default(),
        easy: false,
        error: None,
    });
    assert_eq!(create.matches(&close).count(), 3);
}

#[test]
fn channel_create_pane_opens_on_the_tab_that_was_submitted() {
    let company = mailbox_company();
    let pane = |draft: &ChannelDraft<'_>, error| {
        channel_create_pane(&ChannelCreatePane {
            company: &company,
            app_domain_name: "example.com",
            agents: &[],
            spam_scan_enabled: true,
            draft,
            easy: false,
            error,
        })
    };

    let fresh = pane(&ChannelDraft::default(), None);
    assert!(fresh.contains(&format!(
        "hx-post=\"/ui/channels?company_id={}\"",
        company.id
    )));
    assert!(fresh.contains("<input type=\"hidden\" name=\"form_mode\" value=\"simple\">"));
    assert!(fresh.contains("<input type=\"hidden\" name=\"form_mode\" value=\"advanced\">"));
    assert!(fresh.contains(">Easy</button>"));
    assert!(fresh.contains(&format!(
        "hx-post=\"/ui/channels/easy?company_id={}\"",
        company.id
    )));
    assert!(fresh.contains(r##"id="channel-tab-easy" class="hidden space-y-4"##));
    assert!(fresh.contains(r##"id="channel-tab-simple" class=" space-y-4"##));
    assert!(fresh.contains(r##"id="channel-tab-advanced" class="hidden space-y-4"##));

    // A rejected Advanced submit comes back on the Advanced tab with what was typed.
    let retried = pane(
        &ChannelDraft {
            name: "Support",
            slug: "support",
            advanced: true,
            ..ChannelDraft::default()
        },
        Some("Failed to create channel: slug already taken"),
    );
    assert!(retried.contains(r##"id="channel-tab-simple" class="hidden space-y-4"##));
    assert!(retried.contains(r##"id="channel-tab-advanced" class=" space-y-4"##));
    assert!(retried.contains("slug already taken"));
    assert!(retried.contains("value=\"Support\""));

    // Simple mode keeps its instructions instead.
    let simple_retry = pane(
        &ChannelDraft {
            name: "Support",
            system_prompt: "Answer billing questions",
            ..ChannelDraft::default()
        },
        Some("Failed to generate agent prompt"),
    );
    assert!(simple_retry.contains("Answer billing questions</textarea>"));
    assert!(simple_retry.contains(r##"id="channel-tab-simple" class=" space-y-4"##));

    let scheduler = Agent {
        company_id: None,
        description: Some("Books meetings from email.".to_string()),
        ..settings_agent(company.id, "Scheduler", "scheduler")
    };
    let selected = [scheduler.id];
    let easy_draft = ChannelDraft {
        agent_ids: &selected,
        ..ChannelDraft::default()
    };
    let easy_retry = channel_create_pane(&ChannelCreatePane {
        company: &company,
        app_domain_name: "example.com",
        agents: &[scheduler.clone()],
        spam_scan_enabled: true,
        draft: &easy_draft,
        easy: true,
        error: Some("channel already exists"),
    });
    assert!(easy_retry.contains(r##"id="channel-tab-easy" class=" space-y-4"##));
    assert!(easy_retry.contains(&format!("value=\"{}\"", scheduler.id)));
    assert!(easy_retry.contains("class=\"checkbox checkbox-primary mt-1\" checked"));
    assert!(easy_retry.contains("Books meetings from email."));
    assert!(easy_retry.contains("channel already exists"));
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
            agents: &[],
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

    let inbound = Message {
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
    };
    let outbound = Message {
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
    };

    let html = message_pane(&MessagePane {
        company_id: company.id,
        channel: &channel,
        thread: &thread,
        messages: &[inbound, outbound],
        agent: None,
        viewer_email: &mailbox_account_email(),
        activity: None,
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
fn mailbox_message(thread_id: Uuid, body: &str) -> Message {
    Message {
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
        agent: None,
        viewer_email: &mailbox_account_email(),
        activity: None,
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
        agent: None,
        viewer_email: &mailbox_account_email(),
        activity: None,
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
        threads: std::slice::from_ref(&thread),
        next_cursor: None,
        selected_thread_id: None,
        activity: no_activity(),
    });

    assert!(
        column
            .contains(thread_row_fragment(company.id, &channel, &thread, false, None, None).trim())
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
        thread_row_fragment(company.id, &channel, &thread, true, None, None)
            .contains("bg-base-300")
    );
    assert!(
        !thread_row_fragment(company.id, &channel, &thread, false, None, None)
            .contains("bg-base-300")
    );
}

#[test]
fn a_thread_row_carries_its_own_activity_slot() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let thread = mailbox_thread(channel.id);

    let idle = thread_row_fragment(company.id, &channel, &thread, false, None, None);
    let working = thread_row_fragment(
        company.id,
        &channel,
        &thread,
        false,
        Some(ThreadActivity::Working),
        None,
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
    assert!(working.contains(r#"title="Agent replying…""#));
    assert!(working.contains("animate-pulse"));

    // Queued shares the glyph but not the animation: it is picked up within a poll interval, and
    // changing shape that fast reads as a flicker rather than as progress.
    let queued = thread_row_fragment(
        company.id,
        &channel,
        &thread,
        false,
        Some(ThreadActivity::Queued),
        None,
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
        Some(ThreadActivity::WaitingApproval),
        None,
    );
    assert!(blocked.contains(&icon(Icon::Hourglass, BUTTON_ICON)));
    assert!(blocked.contains(r#"title="Waiting for approval""#));
    assert!(!blocked.contains("animate-pulse"));
}

/// The reply mark is client-side and only ever applies to rows arriving over the stream, so the
/// row has to say who spoke last -- but only when the stream renders it.
#[test]
fn only_streamed_rows_say_who_spoke_last() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let thread = mailbox_thread(channel.id);

    let from_page = thread_row_fragment(company.id, &channel, &thread, false, None, None);
    assert!(!from_page.contains("data-last-role"));
    assert!(from_page.contains(r#"<span class="thread-mark"#));

    let streamed = thread_row_fragment(
        company.id,
        &channel,
        &thread,
        false,
        None,
        Some(MessageRole::Agent),
    );
    assert!(streamed.contains(r#"data-last-role="agent""#));

    let from_person = thread_row_fragment(
        company.id,
        &channel,
        &thread,
        false,
        None,
        Some(MessageRole::Human),
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
        agent: None,
        viewer_email: &mailbox_account_email(),
        activity: None,
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

/// The spinner is for work actually in progress. A thread parked on an approval gets a badge with
/// no spinner — something spinning forever reads as broken rather than blocked.
#[test]
fn only_a_running_agent_gets_a_spinner() {
    let working = thread_activity_strip(Some(ThreadActivity::Working));
    assert!(working.contains("loading loading-dots"));
    assert!(working.contains("Agent replying"));

    for blocked in [
        ThreadActivity::Queued,
        ThreadActivity::WaitingApproval,
        ThreadActivity::WaitingReply,
        ThreadActivity::Failed,
    ] {
        let strip = thread_activity_strip(Some(blocked));
        assert!(!strip.contains("loading"), "{blocked:?} must not spin");
        assert!(strip.contains(blocked.label()));
    }

    // An idle thread renders nothing, which is what clears the strip.
    assert!(thread_activity_strip(None).is_empty());
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
        agent: None,
        viewer_email: &mailbox_account_email(),
        activity: Some(ThreadActivity::Working),
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
fn an_agent_reply_is_drawn_as_the_agent_and_an_inbound_message_as_its_sender() {
    let company = mailbox_company();
    let thread = mailbox_thread(mailbox_channel(company.id).id);
    let agent = Agent {
        avatar_url: Some(AvatarUrl::from("https://example.com/triage.png")),
        ..settings_agent(company.id, "Triage", "triage")
    };

    let reply = Message {
        role: MessageRole::Agent,
        direction: MessageDirection::Outbound,
        sender: EmailAddress::from("support@acme.mailagents.com"),
        ..mailbox_message(thread.id, "The reply.")
    };
    let html = message_bubble_chat(
        &reply,
        Some(&agent),
        None,
        MessageScope {
            company_id: Uuid::new_v4(),
            channel_id: Uuid::new_v4(),
        },
    );
    assert!(html.contains("chat-image"));
    assert!(html.contains(r#"src="https://example.com/triage.png""#));
    assert!(html.contains("Triage"));

    // With no single agent behind the channel, the address it was sent from stands in.
    let anonymous = message_bubble_chat(
        &reply,
        None,
        None,
        MessageScope {
            company_id: Uuid::new_v4(),
            channel_id: Uuid::new_v4(),
        },
    );
    assert!(!anonymous.contains("<img"));
    assert!(anonymous.contains("support@acme.mailagents.com"));

    // An inbound message is its sender's, whichever agent answers the channel.
    let inbound = mailbox_message(thread.id, "The question.");
    let inbound_html = message_bubble_chat(
        &inbound,
        Some(&agent),
        None,
        MessageScope {
            company_id: Uuid::new_v4(),
            channel_id: Uuid::new_v4(),
        },
    );
    assert!(!inbound_html.contains("<img"));
    assert!(inbound_html.contains(inbound.sender.as_str()));

    // Who wrote it is on the bubble, because only the agent's reply quiets the thread's row.
    assert!(html.contains(r#"data-role="agent""#));
    assert!(inbound_html.contains(r#"data-role="human""#));
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

#[test]
fn an_attachment_is_offered_as_a_download_scoped_to_its_thread() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let thread = mailbox_thread(channel.id);
    let mut message = mailbox_message(thread.id, "See attached.");
    message.attachments = Some(vec![
        AttachmentMetadata {
            filename: "Q3 <report>.pdf".to_string(),
            content_type: "application/pdf".to_string(),
            sha256_hash: "abc123".to_string(),
            size_bytes: 2048,
            storage_key: Some(crate::entities::value_objects::ObjectKey::new(
                "attachments/abc123.pdf",
            )),
        },
        // Mail that arrived before there was anywhere to keep it.
        AttachmentMetadata {
            filename: "old.doc".to_string(),
            content_type: "application/msword".to_string(),
            sha256_hash: "def456".to_string(),
            size_bytes: 10,
            storage_key: None,
        },
    ]);

    let html = message_bubble_chat(
        &message,
        None,
        None,
        MessageScope {
            company_id: company.id,
            channel_id: channel.id,
        },
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
        agent: None,
        viewer_email: &mailbox_account_email(),
        activity: None,
    });

    // Rendered with the scope the pane itself uses, so the comparison is of the markup rather
    // than of two different attachment links.
    let scope = MessageScope {
        company_id: company.id,
        channel_id: channel.id,
    };
    assert!(pane.contains(
        message_bubble_chat(&message, None, Some(&mailbox_account_email()), scope).trim()
    ));
    assert!(pane.contains("name=\"quiet\" value=\"true\""));
    assert!(pane.contains("save to history without running the agent"));
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
        quiet: true,
        error: Some("Channel rejected the message"),
    });

    assert!(html.contains("hx-post=\"/ui/reply\""));
    assert!(html.contains(&format!("name=\"thread_id\" value=\"{}\"", thread.id)));
    assert!(html.contains("inbox@acme.example.com"));
    assert!(html.contains("owner@example.com"));
    assert!(html.contains("value=\"Re: Question &lt;script&gt;\" readonly"));
    assert!(html.contains("Draft reply</textarea>"));
    assert!(html.contains("toggle toggle-primary toggle-sm\" checked"));
    assert!(html.contains(
        "name=\"quiet\" value=\"true\" class=\"checkbox checkbox-primary checkbox-sm\" checked"
    ));
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
        quiet: true,
        error: Some("Channel rejected the message"),
    });

    assert!(html.contains("inbox@acme.example.com"));
    assert!(html.contains("owner@example.com"));
    assert!(html.contains("value=\"Draft subject\""));
    assert!(html.contains("Draft body</textarea>"));
    assert!(html.contains("toggle toggle-primary toggle-sm\" checked"));
    assert!(html.contains(
        "name=\"quiet\" value=\"true\" class=\"checkbox checkbox-primary checkbox-sm\" checked"
    ));
    assert!(html.contains("alert alert-error"));
    assert!(html.contains("Channel rejected the message"));
    assert!(html.contains("hx-post=\"/ui/compose\""));
}

fn monitored_task(company_id: Uuid, channel_id: Uuid, status: TaskStatus) -> BackgroundTask {
    BackgroundTask {
        id: Uuid::new_v4(),
        company_id,
        channel_id,
        thread_id: None,
        task_type: "email_agent_dispatch".to_string(),
        status,
        payload: json!({
            "execution_result": {
                "token_usage": { "prompt_tokens": 120, "completion_tokens": 45, "total_tokens": 165 }
            }
        }),
        retry_count: 1,
        max_retries: 3,
        last_error: None,
        worker_id: None,
        execution_generation: None,
        locked_at: None,
        lock_expires_at: None,
        run_at: Utc::now(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

#[test]
fn task_monitor_page_uses_the_ui_shell_and_lights_its_own_rail_icon() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let task = monitored_task(company.id, channel.id, TaskStatus::Completed);
    let filter = TaskFilter::new(
        Some(channel.id),
        Some(TaskStatus::Completed),
        false,
        None,
        None,
    );
    let email = mailbox_account_email();

    let list = TaskMonitorList {
        company: &company,
        tasks: std::slice::from_ref(&task),
        filter: &filter,
        has_next: false,
        selected_task_id: None,
    };
    let pane = task_monitor_empty_pane("Select a task.", FragmentSwap::Inline);
    let html = task_monitor_page(&TaskMonitorPage {
        user: &mailbox_user(&email),
        companies: std::slice::from_ref(&company),
        channels: std::slice::from_ref(&channel),
        list: &list,
        pane_html: &pane,
    });

    // Same chrome as the other workspaces: its own rail icon lit, the others one click away.
    assert!(html.contains(&format!(
        r##"<a href="/ui/tasks?company_id={}" class="btn btn-square btn-md btn-primary"##,
        company.id
    )));
    assert!(html.contains(&format!(
        r##"<a href="/ui/agents?company_id={}" class="btn btn-square btn-md btn-ghost"##,
        company.id
    )));
    assert!(!html.contains("dropdown-bottom w-full p-3"));
    assert!(html.contains("id=\"task-pane\""));
    // The filter form keeps what the request was made with, and it sits outside the swapped list.
    assert!(html.contains(&format!(
        r##"<option value="{}" selected>Inbox</option>"##,
        channel.id
    )));
    assert!(html.contains(r##"<option value="completed" selected>Completed</option>"##));
    assert!(html.contains(r##"<option value="desc" selected>Newest first</option>"##));
}

#[test]
fn task_monitor_list_targets_the_pane_and_swaps_out_of_band() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let task = monitored_task(company.id, channel.id, TaskStatus::Processing);
    let filter = TaskFilter::new(None, None, false, Some(2), None);

    let list = TaskMonitorList {
        company: &company,
        tasks: std::slice::from_ref(&task),
        filter: &filter,
        has_next: true,
        selected_task_id: Some(task.id),
    };

    let inline = task_monitor_list(&list, FragmentSwap::Inline);
    assert!(inline.contains(&format!("/ui/tasks/{}?company_id={}", task.id, company.id)));
    assert!(inline.contains("hx-target=\"#task-pane\""));
    assert!(inline.contains("hx-sync=\"#task-pane:replace\""));
    assert!(inline.contains("menu-active"));
    assert!(inline.contains("Processing"));
    assert!(inline.contains("165 tokens"));
    assert!(!inline.contains("hx-swap-oob"));
    // Paging keeps the filters it was made under, and stays on the list alone.
    assert!(inline.contains(&format!("/ui/tasks/list?company_id={}&page=3", company.id)));
    // Newest-first, so "back" is towards the newer end -- and the arrow leads the word.
    assert!(inline.contains("</svg> Newer"));

    // After a stop or a resume the list rides along on the pane's response.
    let oob = task_monitor_list(&list, FragmentSwap::OutOfBand);
    assert!(oob.contains("id=\"task-list\""));
    assert!(oob.contains("hx-swap-oob=\"outerHTML\""));

    let empty = TaskMonitorList {
        tasks: &[],
        has_next: false,
        selected_task_id: None,
        ..list
    };
    let empty_html = task_monitor_list(&empty, FragmentSwap::Inline);
    assert!(empty_html.contains("No tasks match these filters"));
    assert!(!empty_html.contains("Older <svg"));
}

#[test]
fn task_detail_pane_offers_the_action_the_status_allows() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let thread_id = Uuid::new_v4();
    let task = BackgroundTask {
        thread_id: Some(thread_id),
        last_error: Some("provider timed out".to_string()),
        ..monitored_task(company.id, channel.id, TaskStatus::Failed)
    };

    let html = task_detail_pane(&TaskDetailPane {
        deliveries: &[],
        company_id: company.id,
        task: &task,
        channel: Some(&channel),
        error: Some("Failed to stop task: worker unreachable"),
    });

    assert!(html.contains("id=\"task-pane\""));
    assert!(html.contains(&format!(
        "hx-post=\"/ui/tasks/{}/stop?company_id={}\"",
        task.id, company.id
    )));
    assert!(!html.contains("/resume?"));
    // A task with a thread is read in the mailbox, not in the classic simulator.
    assert!(html.contains(&format!(
        "/ui?company_id={}&channel_id={}&thread_id={}",
        company.id, channel.id, thread_id
    )));
    assert!(html.contains("provider timed out"));
    assert!(html.contains("Failed to stop task: worker unreachable"));
    assert!(html.contains("Retries"));
    assert!(html.contains("165"));
    // The payload block is the one the simulator already renders, secrets scrubbed and all.
    assert!(html.contains("Task Execution Parameters"));

    // A stopped task is the other way round: resume, and nothing to stop.
    let stopped = BackgroundTask {
        status: TaskStatus::Stopped,
        ..monitored_task(company.id, channel.id, TaskStatus::Stopped)
    };
    let stopped_html = task_detail_pane(&TaskDetailPane {
        company_id: company.id,
        task: &stopped,
        channel: None,
        deliveries: &[],
        error: None,
    });
    assert!(stopped_html.contains(&format!("/ui/tasks/{}/resume", stopped.id)));
    assert!(!stopped_html.contains("/stop?"));
    // A task whose channel is gone still renders, falling back to the raw id.
    assert!(stopped_html.contains(&stopped.channel_id.to_string()));
}

#[test]
fn task_filter_clamps_paging_and_probes_for_a_next_page() {
    let filter = TaskFilter::new(None, None, false, Some(0), Some(1000));
    assert_eq!(filter.page(), 1);
    assert_eq!(filter.limit(), TaskFilter::MAX_PAGE_SIZE);
    assert_eq!(filter.offset(), 0);

    let third = TaskFilter::new(None, None, false, Some(3), Some(25));
    assert_eq!(third.offset(), 50);
    assert_eq!(third.probe_limit(), 26);
    assert_eq!(third.on_page(0).page(), 1);

    let company_id = Uuid::new_v4();
    let channel_id = Uuid::new_v4();
    let probed: Vec<BackgroundTask> = (0..26)
        .map(|_| monitored_task(company_id, channel_id, TaskStatus::Completed))
        .collect();
    let (page, has_next) = third.split_probe(probed);
    assert_eq!(page.len(), 25);
    assert!(has_next);

    let (short_page, no_next) = third.split_probe(vec![monitored_task(
        company_id,
        channel_id,
        TaskStatus::Completed,
    )]);
    assert_eq!(short_page.len(), 1);
    assert!(!no_next);
}

fn settings_company(name: &str, slug: &str) -> Company {
    Company {
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
        provider: Some("openai".into()),
        model: Some("gpt-4o".into()),
        enable_llm_spam_guardrail: Some(true),
        ..settings_company("Acme & Co", "acme")
    };

    let html = company_edit_pane(&CompanyEditPane {
        company: &company,
        app_domain_name: "example.com",
        counts: CompanyCounts {
            channels: 3,
            agents: 2,
        },
        draft: None,
        error: None,
        editable: true,
        body: CompanyPaneBody::Settings,
    });

    assert!(html.contains(r##"value="Acme &amp; Co""##));
    assert!(html.contains(r##"value="acme""##));
    assert!(html.contains(r##"value="openai""##));
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
fn company_member_can_open_company_without_edit_controls_or_api_key() {
    let company = Company {
        api_key: Some("owner-secret".into()),
        ..settings_company("Acme", "acme")
    };

    let html = company_edit_pane(&CompanyEditPane {
        company: &company,
        app_domain_name: "example.com",
        counts: CompanyCounts {
            channels: 3,
            agents: 2,
        },
        draft: None,
        error: None,
        editable: false,
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
        app_domain_name: "example.com",
        counts: CompanyCounts::default(),
        draft: None,
        error: None,
        editable: true,
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
        app_domain_name: "example.com",
        counts: CompanyCounts::default(),
        draft: Some(&draft),
        error: Some("Slug is taken"),
        editable: true,
        body: CompanyPaneBody::Settings,
    });

    assert!(html.contains(
        r#"<input type="hidden" name="avatar_url" value="https://cdn.example.com/picked.png">"#
    ));

    // A submitted URL is text until it is parsed, and what cannot be rendered never reaches the
    // `<img src>` the bubble draws.
    let tampered = CompanyDraft {
        avatar_url: "javascript:alert(1)",
        memory_provider: "",
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
        provider: "",
        model: "",
        api_key: "",
        spam_guardrail: SpamGuardrail::Disabled,
        avatar_url: "",
        memory_provider: "",
    };

    let html = company_edit_pane(&CompanyEditPane {
        company: &company,
        app_domain_name: "example.com",
        counts: CompanyCounts::default(),
        draft: Some(&draft),
        error: Some("Slug is taken"),
        editable: true,
        body: CompanyPaneBody::Settings,
    });

    assert!(html.contains("Slug is taken"));
    assert!(html.contains(r##"value="Acme Renamed""##));
    assert!(html.contains(r##"value="acme-renamed""##));
    // The header still names the stored company — only the form carries what was typed.
    assert!(html.contains(">Acme</h2>"));
}

#[test]
fn the_spam_guardrail_select_opens_on_the_state_it_was_given() {
    // The three states a checkbox cannot express, each round-tripping through the form value.
    for (stored, marked) in [
        (None, r##"<option value="" selected>"##),
        (Some(true), r##"<option value="true" selected>"##),
        (Some(false), r##"<option value="false" selected>"##),
    ] {
        let guardrail = SpamGuardrail::from_stored(stored);
        let draft = CompanyDraft {
            spam_guardrail: guardrail,
            ..CompanyDraft::default()
        };
        let html = company_create_pane(&CompanyCreatePane {
            draft: &draft,
            error: None,
        });

        assert!(html.contains(marked), "expected {marked} for {stored:?}");
        assert_eq!(guardrail.stored(), stored);
    }

    assert_eq!(
        SpamGuardrail::from_form(Some("false")),
        SpamGuardrail::Disabled
    );
    assert_eq!(
        SpamGuardrail::from_form(Some("")),
        SpamGuardrail::ServerDefault
    );
    assert_eq!(SpamGuardrail::from_form(None), SpamGuardrail::ServerDefault);
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
        app_domain_name: "example.com",
        counts: CompanyCounts::default(),
        draft: None,
        error: None,
        editable: true,
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
    assert!(!html.contains(r##"name="enable_llm_spam_guardrail""##));
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

    let removable = member_pane(&MemberPane {
        company: &company,
        member: &sam,
        role: TeamRole::Owner,
        viewer_id: Uuid::new_v4(),
        avatar_draft: None,
        error: None,
    });
    assert!(removable.contains(&format!(
        r##"hx-delete="/ui/companies/{}/team/members/{}""##,
        company.id, sam.user_id
    )));
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
    assert!(!owner_pane.contains("hx-delete="));
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
    assert!(!read_only.contains("hx-delete="));
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
        draft: &AgentDraft::default(),
        error: None,
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
        agent: &agent,
        used_by: &[],
        draft: None,
        error: None,
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

#[test]
fn task_pane_surfaces_a_dead_lettered_delivery_against_a_completed_task() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let task = BackgroundTask {
        ..monitored_task(company.id, channel.id, TaskStatus::Completed)
    };

    let failed = OutboxEntry {
        status: OutboxStatus::Failed,
        retry_count: 5,
        last_error: Some("connection refused".to_string()),
        ..queued_email(company.id, Some(channel.id), Some(task.id))
    };

    let html = task_detail_pane(&TaskDetailPane {
        deliveries: std::slice::from_ref(&failed),
        company_id: company.id,
        task: &task,
        channel: Some(&channel),
        error: None,
    });

    // The task reads as completed, so the delivery section is the only thing that can tell an
    // operator the reply never actually went out.
    assert!(html.contains("Delivery"));
    assert!(html.contains("badge-error"));
    assert!(html.contains("gave up after every attempt"));
    assert!(html.contains("connection refused"));
    assert!(html.contains("5 failed attempt(s)"));
    // And it is a way in, not just a report: the row opens the email in the Outbox workspace.
    assert!(html.contains(&format!(
        r##"href="/ui/outbox?company_id={}&entry_id={}""##,
        company.id, failed.id
    )));

    // A task that sent nothing must not grow an empty section.
    let quiet = task_detail_pane(&TaskDetailPane {
        deliveries: &[],
        company_id: company.id,
        task: &task,
        channel: Some(&channel),
        error: None,
    });
    assert!(!quiet.contains("Delivery"));
}

/// One queued email, as the poller would have stored it.
fn queued_email(company_id: Uuid, channel_id: Option<Uuid>, task_id: Option<Uuid>) -> OutboxEntry {
    OutboxEntry {
        id: Uuid::new_v4(),
        company_id,
        channel_id,
        task_id,
        status: OutboxStatus::Pending,
        idempotency_key: "task:reply".to_string(),
        payload: json!({
            "channel_name": "Support",
            "recipient_to": "customer@example.com",
            "recipients_cc": ["cc@example.com"],
            "subject": "Re: order <script>",
            "body_text": "On its way.",
            "api_key": "sk-do-not-render-me",
        }),
        retry_count: 0,
        last_error: None,
        provider_message_id: None,
        available_at: Utc::now(),
        sent_at: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn outbox_filter() -> OutboxFilter {
    OutboxFilter::new(None, None, false, None, None)
}

#[test]
fn outbox_page_lists_queued_email_and_escapes_its_subject() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let entry = queued_email(company.id, Some(channel.id), None);
    let email = mailbox_account_email();
    let filter = outbox_filter();

    let list = OutboxList {
        company: &company,
        entries: std::slice::from_ref(&entry),
        filter: &filter,
        has_next: false,
        selected_entry_id: Some(entry.id),
    };
    let pane = outbox_empty_pane("Select an email.", FragmentSwap::Inline);
    let html = outbox_page(&OutboxPage {
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
    // A queued email is an unsent one, and the summary says so before anything is clicked.
    assert!(html.contains("1 email · 1 unsent · page 1"));
    assert!(html.contains("menu-active"));
    assert!(html.contains(&format!(
        r##"hx-get="/ui/outbox/{}?company_id={}""##,
        entry.id, company.id
    )));
    // The rail lights the workspace the response belongs to.
    assert!(html.contains(r##"title="Outbox""##));
    // The channel filter is what the channel_id column and its index exist to serve.
    assert!(html.contains(r##"<option value="">All channels</option>"##));
    assert!(html.contains(&format!(
        r##"<option value="{}">Inbox</option>"##,
        channel.id
    )));
}

#[test]
fn outbox_pane_links_a_queued_email_to_the_task_that_wrote_it() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let task = monitored_task(company.id, channel.id, TaskStatus::Completed);
    let entry = queued_email(company.id, Some(channel.id), Some(task.id));

    let html = outbox_detail_pane(&OutboxDetailPane {
        company_id: company.id,
        entry: &entry,
        task: Some(&task),
        channel: Some(&channel),
    });

    let task_url = format!("/ui/tasks?company_id={}&task_id={}", company.id, task.id);
    assert!(html.contains(&task_url));
    assert!(html.contains("Open Task"));
    // The task completed; the email has still not gone out, which is the whole point of the join.
    assert!(html.contains(task_status_label(TaskStatus::Completed)));
    assert!(html.contains("Queued · next attempt"));
    // The live channel names itself, rather than the stale name in the payload.
    assert!(html.contains(">Inbox<"));
    assert!(!html.contains(">Support<"));
    assert!(html.contains("cc@example.com"));
    assert!(html.contains("task:reply"));
    // The payload is shown, but secrets in it are not.
    assert!(html.contains("***masked***"));
    assert!(!html.contains("sk-do-not-render-me"));
}

#[test]
fn outbox_pane_without_a_task_offers_no_link_to_one() {
    let company = mailbox_company();
    let entry = OutboxEntry {
        status: OutboxStatus::Sent,
        sent_at: Some(Utc::now()),
        ..queued_email(company.id, None, None)
    };

    let html = outbox_detail_pane(&OutboxDetailPane {
        company_id: company.id,
        entry: &entry,
        task: None,
        channel: None,
    });

    assert!(!html.contains("Open Task"));
    assert!(!html.contains("/ui/tasks?"));
    assert!(html.contains("Delivered"));
    assert!(html.contains("alert-success"));
    // With no channel to resolve, the pane falls back to the name the payload recorded.
    assert!(html.contains(">Support<"));
}

#[test]
fn outbox_pane_names_a_task_it_cannot_read() {
    let company = mailbox_company();
    let task_id = Uuid::new_v4();
    let entry = queued_email(company.id, None, Some(task_id));

    // The row points at a task, but the caller could not load it — the pane says so rather than
    // pretending the email came from nowhere.
    let html = outbox_detail_pane(&OutboxDetailPane {
        company_id: company.id,
        entry: &entry,
        task: None,
        channel: None,
    });

    assert!(html.contains("Open Task"));
    assert!(html.contains("Unavailable"));
    assert!(html.contains(&task_id.to_string()));
}

#[test]
fn outbox_list_says_when_nothing_matches_and_pages_only_when_there_is_more() {
    let company = mailbox_company();
    let filter = outbox_filter();

    let empty = outbox_list(
        &OutboxList {
            company: &company,
            entries: &[],
            filter: &filter,
            has_next: false,
            selected_entry_id: None,
        },
        FragmentSwap::Inline,
    );
    assert!(empty.contains("No queued email matches these filters."));
    assert!(!empty.contains("Older <svg"));

    let entry = queued_email(company.id, None, None);
    let paged = outbox_list(
        &OutboxList {
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
    assert!(paged.contains("hx-sync=\"#outbox-list:replace\""));
}

#[test]
fn outbox_query_carries_only_what_was_chosen() {
    let company_id = Uuid::new_v4();
    let entry_id = Uuid::new_v4();

    let bare = outbox_query(company_id, &outbox_filter(), None);
    assert_eq!(bare, format!("company_id={company_id}"));

    let channel_id = Uuid::new_v4();
    let filtered = OutboxFilter::new(
        Some(channel_id),
        Some(OutboxStatus::Failed),
        true,
        Some(3),
        Some(10),
    );
    let full = outbox_query(company_id, &filtered, Some(entry_id));
    assert_eq!(
        full,
        format!(
            "company_id={company_id}&channel_id={channel_id}&status=failed&sort=asc&limit=10&page=3&entry_id={entry_id}"
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
        task_monitor_empty_pane("Select a task.", FragmentSwap::Inline),
        outbox_empty_pane("Select an email.", FragmentSwap::Inline),
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
    assert!(
        mailbox_html
            .contains(r##"<h2 class="text-base font-semibold leading-tight">Mailbox</h2>"##)
    );

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
    assert!(
        channels_html
            .contains(r##"<h2 class="text-base font-semibold leading-tight">Channels</h2>"##)
    );

    let agents_html = agent_settings_page(&AgentSettingsPage {
        user: &user,
        companies: &companies,
        list: &AgentSettingsList {
            company: &company,
            agents: &[],
            selected_agent_id: None,
        },
        pane_html: "",
    });
    assert!(
        agents_html.contains(r##"<h2 class="text-base font-semibold leading-tight">Agents</h2>"##)
    );

    let schedules_html = schedules_page(&SchedulesPage {
        user: &user,
        company: &company,
        companies: &companies,
        schedules: &[],
        selected_schedule_id: None,
        runs_html: "",
        pane_html: "",
    });
    assert!(
        schedules_html
            .contains(r##"<h2 class="text-base font-semibold leading-tight">Schedules</h2>"##)
    );

    let tasks_filter = TaskFilter::new(None, None, false, None, None);
    let tasks_html = task_monitor_page(&TaskMonitorPage {
        user: &user,
        companies: &companies,
        channels: &[],
        list: &TaskMonitorList {
            company: &company,
            tasks: &[],
            filter: &tasks_filter,
            has_next: false,
            selected_task_id: None,
        },
        pane_html: "",
    });
    assert!(
        tasks_html.contains(r##"<h2 class="text-base font-semibold leading-tight">Tasks</h2>"##)
    );

    let outbox_filter_val = outbox_filter();
    let outbox_html = outbox_page(&OutboxPage {
        user: &user,
        companies: &companies,
        channels: &[],
        list: &OutboxList {
            company: &company,
            entries: &[],
            filter: &outbox_filter_val,
            has_next: false,
            selected_entry_id: None,
        },
        pane_html: "",
    });
    assert!(
        outbox_html.contains(r##"<h2 class="text-base font-semibold leading-tight">Outbox</h2>"##)
    );

    let dashboard_html = dashboard_page(&DashboardShell {
        user: &user,
        scope: DashboardScopeView::Company(&company),
        selected_company: Some(&company),
        companies: &companies,
        window: crate::entities::dashboard::DashboardWindow::last_hour(),
    });
    assert!(
        dashboard_html
            .contains(r##"<h2 class="text-base font-semibold leading-tight">Dashboard</h2>"##)
    );

    let dashboard_global_html = dashboard_page(&DashboardShell {
        user: &user,
        scope: DashboardScopeView::Global,
        selected_company: Some(&company),
        companies: &companies,
        window: crate::entities::dashboard::DashboardWindow::last_hour(),
    });
    assert!(
        dashboard_global_html
            .contains(r##"<h2 class="text-base font-semibold leading-tight">Dashboard</h2>"##)
    );

    let companies_html = company_settings_page(&CompanySettingsPage {
        user: &user,
        list: &CompanySettingsList {
            companies: &companies,
            selected_company_id: Some(company.id),
        },
        rail_company: Some(&company),
        pane_html: "",
    });
    assert!(
        companies_html
            .contains(r##"<h2 class="text-base font-semibold leading-tight">Companies</h2>"##)
    );
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
        agents: &[],
        spam_scan_enabled: true,
        draft: &draft,
        easy: false,
        error: None,
    });

    assert_eq!(
        html.matches(r#"name="description""#).count(),
        2,
        "both the simple and the advanced tab collect a description"
    );
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

#[test]
#[ignore = "developer aid: dumps /assets/app.js so it can be syntax-checked with node"]
fn dump_application_javascript() {
    std::fs::write(
        std::env::var("APP_JS_OUT").expect("APP_JS_OUT"),
        application_javascript(),
    )
    .expect("write bundle");
}
