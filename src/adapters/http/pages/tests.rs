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
