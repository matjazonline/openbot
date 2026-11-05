//! Exercise the real application projection and production renderer, including non-body sinks.
use super::*;
use crate::entities::access_level::{AccessLevel, visible_to};
use crate::use_cases::thread::project_message;

#[test]
fn knowledge_view_visibility_covers_all_levels() {
    for viewer in AccessLevel::ALL {
        for stamp in AccessLevel::ALL {
            assert_eq!(visible_to(viewer, stamp), viewer.as_i16() >= stamp.as_i16());
        }
    }
}

#[test]
fn knowledge_view_placeholder_contains_only_safe_attribution_and_position() {
    let mut message = mailbox_message_view(Uuid::new_v4(), "secret-body-canary");
    message.access_level = AccessLevel::L5;
    message.subject = "secret-subject-canary".into();
    message
        .attachments
        .push(crate::entities::message::AttachmentMetadata {
            filename: "secret-attachment-canary.pdf".into(),
            content_type: "application/pdf".into(),
            sha256_hash: "a".repeat(64),
            size_bytes: 12,
            storage_key: None,
            source: None,
        });
    let cursor = message.cursor();
    project_message(&mut message, AccessLevel::PUBLIC);
    assert!(message.restricted);
    assert_eq!(message.cursor(), cursor);
    assert!(
        message.body.is_empty() && message.subject.is_empty() && message.attachments.is_empty()
    );
    let html = super::super::message_visibility::restricted_message(&message);
    assert!(html.contains("Restricted message"));
    assert!(html.contains(&message.canonical_id.to_string()));
    for secret in [
        "secret-body-canary",
        "secret-subject-canary",
        "secret-attachment-canary",
        "href=",
        "download",
        "textarea",
    ] {
        assert!(!html.contains(secret), "restricted sink: {secret}");
    }
}

#[test]
fn knowledge_view_matching_level_and_owner_keep_full_message() {
    for level in [AccessLevel::L5, AccessLevel::MAX] {
        let mut message = mailbox_message_view(Uuid::new_v4(), "visible-body");
        message.access_level = AccessLevel::L5;
        message.subject = "visible-subject".into();
        project_message(&mut message, level);
        assert!(!message.restricted);
        assert_eq!(message.body, "visible-body");
        assert_eq!(message.subject, "visible-subject");
    }
}

pub(super) fn restricted_browser_fixture() -> String {
    let mut message = mailbox_message_view(Uuid::new_v4(), "private-body-canary");
    message.access_level = AccessLevel::L5;
    message.subject = "private-subject-canary".into();
    message.author.handle = Some("writer<img/src=x/onerror=fetch('/exfil')>".into());
    message
        .attachments
        .push(crate::entities::message::AttachmentMetadata {
            filename: "private-attachment-canary.pdf".into(),
            content_type: "application/pdf".into(),
            sha256_hash: "a".repeat(64),
            size_bytes: 12,
            storage_key: None,
            source: None,
        });
    project_message(&mut message, AccessLevel::PUBLIC);
    let bubble = crate::adapters::http::pages::message_bubble_chat(
        &message,
        None,
        crate::adapters::http::pages::MessageScope {
            company_id: Uuid::new_v4(),
            channel_id: Uuid::new_v4(),
        },
        crate::adapters::http::pages::RunLinkAccess::Label,
    );
    base_layout("Restricted message", &bubble)
}

#[test]
fn knowledge_view_simulation_outcomes_remove_run_choices_and_model_metadata() {
    use crate::entities::message_outcome::{AdmittedRun, RunExecutionSummary, RunModelUsage};
    let now = chrono::Utc::now();
    let mut run = AdmittedRun {
        payload_restricted: false,
        run_id: Uuid::new_v4(),
        state: crate::domain::workflow::RunState::Running,
        executions: vec![RunExecutionSummary {
            step_id: "choose".into(),
            activation: 1,
            route: Some("private-route-canary".into()),
            created_at: now,
            activated_at: Some(now),
            completed_at: Some(now),
        }],
        model_usage: vec![RunModelUsage {
            provider: "private-provider-canary".into(),
            model: "private-model-canary".into(),
            calls: 1,
            input_tokens: 10,
            output_tokens: 20,
        }],
    };
    crate::application::message_outcomes::project_outcome(
        &mut run,
        crate::application::knowledge::PayloadVisibility {
            viewer: AccessLevel::PUBLIC,
            content: AccessLevel::L5,
        },
    );
    assert!(
        run.payload_restricted && run.executions[0].route.is_none() && run.model_usage.is_empty()
    );
    assert_eq!(run.executions[0].step_id, "choose");
    assert_eq!(run.executions[0].completed_at, Some(now));
}
