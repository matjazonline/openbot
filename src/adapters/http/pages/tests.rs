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
        created_at: Utc::now(),
    }
}

fn mailbox_channel(company_id: Uuid) -> Channel {
    Channel {
        enabled: true,
        id: Uuid::new_v4(),
        company_id,
        name: "Inbox".to_string(),
        slug: "inbox".into(),
        alias_slugs: Vec::new(),
        api_key: None,
        provider: None,
        model: None,
        participant_emails: Some(vec!["person@example.com".into()]),
        agent_ids: None,
        channel_config: None,
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
    // Logging out goes through the confirmation dialog, not a bare post.
    assert!(html.contains(r##"onclick="confirmLogout()""##));
    assert!(html.contains(r##"<dialog id="logout-modal" class="modal">"##));
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
        detail_html: &detail,
    });

    // The rail stays inside /ui: the Channels icon opens the workspace, not the classic page.
    assert!(mailbox.contains(&format!("/ui/channels?company_id={}", company.id)));
    assert!(!mailbox.contains(&format!("href=\"/companies/{}/channels\"", company.id)));
    assert!(mailbox.contains(&format!(
        r##"<a href="/ui?company_id={}" class="btn btn-square btn-lg btn-primary"##,
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

    // Same chrome, other icon lit — and the company switcher stays in this workspace.
    assert!(channels.contains(&format!(
        r##"<a href="/ui/channels?company_id={}" class="btn btn-square btn-lg btn-primary"##,
        company.id
    )));
    assert!(channels.contains("/assets/argo-inbox-logo.png"));
    assert!(channels.contains(&format!(
        r##"<li><a href="/ui/channels?company_id={}""##,
        company.id
    )));
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

    // The third workspace, same chrome again: its own icon lit, its own switcher targets.
    assert!(agents.contains(&format!(
        r##"<a href="/ui/agents?company_id={}" class="btn btn-square btn-lg btn-primary"##,
        company.id
    )));
    assert!(agents.contains(&format!(
        r##"<li><a href="/ui/agents?company_id={}""##,
        company.id
    )));
    assert!(agents.contains("id=\"agent-pane\""));
    // The rail is shared, so the other two workspaces stay one click away and unlit.
    assert!(agents.contains(&format!(
        r##"<a href="/ui/channels?company_id={}" class="btn btn-square btn-lg btn-ghost"##,
        company.id
    )));
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
    assert!(edit.contains("✨ Generate with AI"));
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
    assert!(!create.contains("hx-vals"));
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
        company_id,
        name: name.to_string(),
        slug: slug.to_string(),
        provider: None,
        model: None,
        api_key: None,
        system_prompt: None,
        config_json: None,
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
    assert!(public.contains(r##"oninput="toggleChannelSpamConfirm(this)""##));
}

#[test]
fn cancelling_a_channel_form_dismisses_the_pane() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let close = format!(
        "hx-get=\"/ui/channels/close?company_id={}\"\n                            hx-target=\"#channel-pane\" hx-swap=\"outerHTML\"\n                            hx-push-url=\"/ui/channels?company_id={}\">Cancel</button>",
        company.id, company.id
    );

    let edit = channel_edit_pane(&ChannelEditPane {
        company: &company,
        app_domain_name: "example.com",
        channel: &channel,
        agents: &[],
        spam_scan_enabled: true,
        draft: None,
        error: None,
    });
    assert!(edit.contains(&close));

    // Both create tabs offer the same way out, so backing out of a new channel is possible at all.
    let create = channel_create_pane(&ChannelCreatePane {
        company: &company,
        app_domain_name: "example.com",
        agents: &[],
        spam_scan_enabled: true,
        draft: &ChannelDraft::default(),
        error: None,
    });
    assert_eq!(create.matches(&close).count(), 2);
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

    // The chat composer under the messages posts into this same thread.
    assert!(html.contains("id=\"thread-composer\""));
    assert!(html.contains("hx-post=\"/ui/reply\""));
    assert!(html.contains(&format!("name=\"thread_id\" value=\"{}\"", thread.id)));
    assert!(html.contains(&format!("name=\"channel_id\" value=\"{}\"", channel.id)));
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
        r##"<a href="/ui/tasks?company_id={}" class="btn btn-square btn-lg btn-primary"##,
        company.id
    )));
    assert!(html.contains(&format!(
        r##"<a href="/ui/agents?company_id={}" class="btn btn-square btn-lg btn-ghost"##,
        company.id
    )));
    // The company switcher stays in this workspace.
    assert!(html.contains(&format!(
        r##"<li><a href="/ui/tasks?company_id={}""##,
        company.id
    )));
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
    assert!(inline.contains("menu-active"));
    assert!(inline.contains("Processing"));
    assert!(inline.contains("165 tokens"));
    assert!(!inline.contains("hx-swap-oob"));
    // Paging keeps the filters it was made under, and stays on the list alone.
    assert!(inline.contains(&format!("/ui/tasks/list?company_id={}&page=3", company.id)));
    assert!(inline.contains("← Newer"));

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
    assert!(!empty_html.contains("Older →"));
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
        rail_company_id: Some(company.id),
        pane_html: &pane,
    });

    // Same chrome as the other workspaces: its own rail icon lit, the others one click away.
    assert!(html.contains(&format!(
        r##"<a href="/ui/companies?company_id={}" class="btn btn-square btn-lg btn-primary"##,
        company.id
    )));
    assert!(html.contains(&format!(
        r##"<a href="/ui/channels?company_id={}" class="btn btn-square btn-lg btn-ghost"##,
        company.id
    )));
    assert!(html.contains("/assets/argo-inbox-logo.png"));
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
        rail_company_id: None,
        pane_html: &pane,
    });

    // Nothing for the rail to point at yet, but the workspace itself still works.
    assert!(!html.contains("btn btn-square btn-lg"));
    assert!(html.contains("No companies yet"));
    assert!(html.contains(r##"hx-post="/ui/companies""##));
    assert!(html.contains(r##"hx-get="/ui/companies/close""##));
}

#[test]
fn company_settings_list_targets_the_pane_and_swaps_out_of_band() {
    let company = settings_company("Acme", "acme");
    let other = settings_company("Globex", "globex");
    let list = CompanySettingsList {
        companies: &[company.clone(), other.clone()],
        selected_company_id: Some(company.id),
    };

    let inline = company_settings_list(&list, FragmentSwap::Inline);
    assert!(inline.contains(&format!(r##"hx-get="/ui/companies/{}""##, company.id)));
    assert!(inline.contains("hx-target=\"#company-pane\""));
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
    assert!(html.contains(&format!(r##"href="/ui/team?company_id={}""##, company.id)));
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
    };

    let html = company_edit_pane(&CompanyEditPane {
        company: &company,
        app_domain_name: "example.com",
        counts: CompanyCounts::default(),
        draft: Some(&draft),
        error: Some("Slug is taken"),
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
        rail_company_id: Some(company.id),
        pane_html: &pane,
    });

    // Nothing in the sidebar is lit, but the other workspaces are still one click away.
    assert!(!company_settings_list(&list, FragmentSwap::Inline).contains("menu-active"));
    assert!(html.contains(&format!(
        r##"<a href="/ui/channels?company_id={}" class="btn btn-square btn-lg btn-ghost"##,
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
        role: role.to_string(),
        created_at: Utc::now(),
    }
}

fn team_invite(company_id: Uuid, email: &str, status: &str) -> CompanyInvite {
    CompanyInvite {
        id: Uuid::new_v4(),
        company_id,
        company_name: Some("Acme".to_string()),
        email: email.to_string(),
        status: status.to_string(),
        created_at: Utc::now(),
    }
}

#[test]
fn team_settings_page_uses_the_ui_shell_and_lights_its_own_rail_icon() {
    let company = mailbox_company();
    let member = team_member(company.id, company.user_id, "dana", "owner");
    let invite = team_invite(company.id, "kim@example.com", "pending");
    let email = mailbox_account_email();

    let list = TeamSettingsList {
        company: &company,
        members: std::slice::from_ref(&member),
        invites: std::slice::from_ref(&invite),
        selected: TeamSelection::None,
        role: TeamRole::Owner,
    };
    let pane = team_settings_empty_pane("Select someone.", FragmentSwap::Inline);
    let html = team_settings_page(&TeamSettingsPage {
        user: &mailbox_user(&email),
        companies: std::slice::from_ref(&company),
        list: &list,
        pane_html: &pane,
    });

    // Same chrome as the other workspaces: its own rail icon lit, the others one click away.
    assert!(html.contains(&format!(
        r##"<a href="/ui/team?company_id={}" class="btn btn-square btn-lg btn-primary"##,
        company.id
    )));
    assert!(html.contains(&format!(
        r##"<a href="/ui/companies?company_id={}" class="btn btn-square btn-lg btn-ghost"##,
        company.id
    )));
    // The team icon sits directly below the companies one.
    assert!(
        html.find(r##"title="Companies""##) < html.find(r##"title="Team""##),
        "the team icon belongs below the companies icon"
    );
    assert!(html.contains("id=\"team-pane\""));
    // The company switcher stays in this workspace.
    assert!(html.contains(&format!(
        r##"<li><a href="/ui/team?company_id={}""##,
        company.id
    )));
    assert!(html.contains(r##"hx-get="/ui/team/new?company_id="##));
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
        "/ui/team/members/{}?company_id={}",
        sam.user_id, company.id
    )));
    assert!(inline.contains(&format!(
        "/ui/team/invites/{}?company_id={}",
        pending.id, company.id
    )));
    assert!(inline.contains("hx-target=\"#team-pane\""));
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
    let email = mailbox_account_email();

    let list = TeamSettingsList {
        company: &company,
        members: std::slice::from_ref(&owner),
        // The route hands a member an empty list; the renderer must not offer the section anyway.
        invites: std::slice::from_ref(&invite),
        selected: TeamSelection::None,
        role: TeamRole::Member,
    };
    let pane = team_settings_empty_pane("Select someone.", FragmentSwap::Inline);
    let html = team_settings_page(&TeamSettingsPage {
        user: &mailbox_user(&email),
        companies: std::slice::from_ref(&company),
        list: &list,
        pane_html: &pane,
    });

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
        error: None,
    });
    assert!(removable.contains(&format!(
        r##"hx-delete="/ui/team/members/{}?company_id={}""##,
        sam.user_id, company.id
    )));
    assert!(removable.contains("hx-confirm=\"Remove sam"));

    // The owner's own row: the use case refuses it, so the pane does not offer it.
    let owner_pane = member_pane(&MemberPane {
        company: &company,
        member: &owner,
        role: TeamRole::Owner,
        error: None,
    });
    assert!(!owner_pane.contains("hx-delete="));
    assert!(owner_pane.contains("cannot be removed"));
    // The stored role says "member", but owning the company outranks it.
    assert!(owner_pane.contains(">owner</span>"));

    // An ordinary member looking at somebody else gets a read-only pane.
    let read_only = member_pane(&MemberPane {
        company: &company,
        member: &sam,
        role: TeamRole::Member,
        error: None,
    });
    assert!(!read_only.contains("hx-delete="));
    assert!(read_only.contains("Only the company owner"));
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
        draft: None,
        error: None,
    });
    assert!(editable.contains(&format!(
        r##"hx-put="/ui/team/invites/{}?company_id={}""##,
        pending.id, company.id
    )));
    assert!(editable.contains(r##"value="kim@example.com""##));
    assert!(editable.contains("Cancel Invite"));

    // An answered invite is a record: rewriting its address would rewrite what somebody accepted.
    let settled = invite_pane(&InvitePane {
        company: &company,
        invite: &accepted,
        role: TeamRole::Owner,
        draft: None,
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
        draft: None,
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
        draft: Some("typo@example"),
        error: Some("Please provide a valid email address."),
    });
    assert!(rejected_edit.contains("Please provide a valid email address."));
    assert!(rejected_edit.contains(r##"value="typo@example""##));
    // The header still names the stored invite — only the form carries what was typed.
    assert!(rejected_edit.contains(">kim@example.com</h2>"));

    let rejected_create = invite_create_pane(&InviteCreatePane {
        company: &company,
        draft: "typo@example",
        error: Some("Please provide a valid email address."),
    });
    assert!(rejected_create.contains("Please provide a valid email address."));
    assert!(rejected_create.contains(r##"value="typo@example""##));
    assert!(rejected_create.contains(&format!(
        r##"hx-post="/ui/team/invites?company_id={}""##,
        company.id
    )));
    assert!(rejected_create.contains(r##"hx-get="/ui/team/close?company_id="##));
}

#[test]
fn task_pane_surfaces_a_dead_lettered_delivery_against_a_completed_task() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let task = BackgroundTask {
        ..monitored_task(company.id, channel.id, TaskStatus::Completed)
    };

    let failed = TaskDelivery {
        id: Uuid::new_v4(),
        status: "failed".to_string(),
        retry_count: 5,
        last_error: Some("connection refused".to_string()),
        provider_message_id: None,
        available_at: chrono::Utc::now(),
        sent_at: None,
        created_at: chrono::Utc::now(),
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
