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
        worker_id: None,
        locked_at: None,
        lock_expires_at: None,
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
        worker_id: None,
        locked_at: None,
        lock_expires_at: None,
        run_at: Utc::now().naive_utc(),
        created_at: Utc::now().naive_utc(),
        updated_at: Utc::now().naive_utc(),
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
        created_at: Utc::now().naive_utc(),
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
        created_at: Utc::now().naive_utc(),
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
        created_at: Utc::now().naive_utc(),
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
        created_at: Utc::now().naive_utc(),
    }
}

fn mailbox_channel(company_id: Uuid) -> Channel {
    Channel {
        id: Uuid::new_v4(),
        company_id,
        name: "Inbox".to_string(),
        slug: "inbox".into(),
        api_key: None,
        provider: None,
        model: None,
        participant_emails: Some(vec!["person@example.com".into()]),
        agent_ids: None,
        channel_config: None,
        created_at: Utc::now().naive_utc(),
    }
}

fn mailbox_thread(channel_id: Uuid) -> Thread {
    Thread {
        id: Uuid::new_v4(),
        channel_id,
        subject: "Question <script>".to_string(),
        participant_emails: vec!["person@example.com".into()],
        created_at: Utc::now().naive_utc(),
        updated_at: Utc::now().naive_utc(),
    }
}

fn mailbox_account_email() -> EmailAddress {
    EmailAddress::from("dana@example.com")
}

fn mailbox_user(email: &EmailAddress) -> MailboxUser<'_> {
    MailboxUser {
        username: "dana",
        email,
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
        detail_html: &detail,
    });

    assert!(html.contains("daisyui@5"));
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
fn top_bar_shows_the_logo_and_the_signed_in_account() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let detail = empty_detail_pane("Select a thread.", FragmentSwap::Inline);
    let email = EmailAddress::from("dana<script>@example.com");
    let user = MailboxUser {
        username: "dana",
        email: &email,
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
        detail_html: &detail,
    });

    assert!(html.contains("/assets/argo-inbox-logo.png"));
    assert!(html.contains("dana&lt;script&gt;@example.com"));
    assert!(html.contains(">D</span>"));
    assert!(html.contains(r##"<form method="post" action="/logout">"##));

    // The top bar owns the whole width: the columns start below it, not beside it.
    assert!(html.contains(r##"<div class="flex h-screen flex-col">"##));

    // A user with no companies still gets the same bar, since it is their only way out.
    let no_company = mailbox_no_company_page(&user);
    assert!(no_company.contains("/assets/argo-inbox-logo.png"));
    assert!(no_company.contains("Log out"));
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
            detail_html: &detail,
        })
    };

    let unselected = page(None);
    assert!(unselected.contains("inbox@acme.example.com"));
    assert!(unselected.contains(&format!(
        "/companies/{}/channels?new=1#channel-form-card",
        company.id
    )));
    assert!(!unselected.contains("Edit Channel"));

    let selected = page(Some(&channel));
    assert!(selected.contains("Edit Channel"));
    assert!(selected.contains(&format!(
        "/companies/{}/channels?edit={}#channel-{}",
        company.id, channel.id, channel.id
    )));
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
        },
        FragmentSwap::OutOfBand,
    );

    assert!(fragment.contains("id=\"thread-pagination\" hx-swap-oob=\"outerHTML\""));
    assert!(!fragment.contains("Load older threads"));
}

#[test]
fn message_pane_separates_agent_and_human_bubbles() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let thread = mailbox_thread(channel.id);

    let inbound = Message {
        id: Uuid::new_v4(),
        thread_id: thread.id,
        message_id: "<in@test.com>".into(),
        in_reply_to: None,
        references_list: vec![],
        sender: "person@example.com".into(),
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
        created_at: Utc::now().naive_utc(),
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
        created_at: Utc::now().naive_utc(),
    };

    let html = message_pane(&MessagePane {
        company_id: company.id,
        channel: &channel,
        thread: &thread,
        messages: &[inbound, outbound],
    });

    assert!(html.contains("chat chat-start"));
    assert!(html.contains("chat chat-end"));
    assert!(html.contains("Plain &lt;b&gt;text&lt;/b&gt; body"));
    assert!(html.contains("<strong>Answered</strong>"));

    // The header offers a further message in this same thread.
    assert!(html.contains("New Message"));
    assert!(html.contains(&format!(
        "/ui/reply?company_id={}&channel_id={}&thread_id={}",
        company.id, channel.id, thread.id
    )));
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
    assert!(html.contains("alert alert-error"));
    assert!(html.contains("Channel rejected the message"));
    assert!(html.contains("hx-post=\"/ui/compose\""));
}
