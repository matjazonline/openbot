//! Structured answers through production PostgreSQL dispatch and frozen email publication.
use super::*;
use crate::{
    entities::harness::HarnessKind,
    services::{
        harness::runs::RunCheckpoint,
        test_support::{ScriptedExchange, ScriptedResponse, scripted_scenario},
    },
};
use serde_json::{Value, json};

fn contract() -> Value {
    json!({"version":1,"format":"json_schema","schema":{"type":"object","properties":{"status":{"const":"done"}},"required":["status"],"additionalProperties":false}})
}
async fn configure(fx: &Fixture) {
    sqlx::query("UPDATE agents SET response_contract = $1 WHERE id IN (SELECT agent_id FROM channel_agents WHERE channel_id = $2)")
        .bind(contract()).bind(fx.channel_a.id).execute(&fx.pool).await.unwrap();
}
#[tokio::test]
async fn structured_dispatch_preserves_json_and_exhaustion_has_no_publication() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let _guard = crate::adapters::persistence::test_support::UNSCOPED_CLAIM
        .lock()
        .await;
    for (valid, delivery_mode) in [
        (true, ReplyDelivery::Send),
        (false, ReplyDelivery::Send),
        (true, ReplyDelivery::InAppOnly),
    ] {
        let answers = if valid {
            vec!["{}", r#"{"status":"done"}"#]
        } else {
            vec!["{}", "not json", "{}"]
        };
        let count = answers.len();
        let llm = scripted_scenario(
            answers
                .into_iter()
                .enumerate()
                .map(|(index, answer)| {
                    ScriptedExchange::new(
                        move |request| {
                            if !request.body.to_string().contains("Draft 2020-12") {
                                return Err("contract instructions");
                            }
                            if index > 0
                                && request
                                    .body
                                    .get("tools")
                                    .is_some_and(|v| v.as_array().is_none_or(|v| !v.is_empty()))
                            {
                                return Err("repair exposed tools");
                            }
                            Ok(())
                        },
                        ScriptedResponse::turn(LlmTurn::text(answer), 1),
                    )
                })
                .collect(),
        )
        .await;
        let fx = fixture_for_harness(pool.clone(), Some(&llm.base_url), HarnessKind::Rig).await;
        configure(&fx).await;
        let ingest = fx
            .threads
            .ingest_test_email(inbound(
                &fx.owner_email,
                &fx.address(&fx.channel_a),
                "<structured@example.test>",
                "Return status",
            ))
            .await
            .unwrap();
        let task = ingest.task_id.unwrap();
        let lease = fx.claim(task).await;
        let result = fx
            .threads
            .execute_claimed_agent_task_and_dispatch(
                &ingest,
                delivery_mode,
                lease,
                ingest.correlation_id().unwrap(),
            )
            .await;
        assert_eq!(llm.finish().await, Ok(count));
        let saved: Value =
            sqlx::query_scalar("SELECT checkpoint FROM task_harness_runs WHERE task_id=$1")
                .bind(task)
                .fetch_one(&pool)
                .await
                .unwrap();
        let saved: RunCheckpoint = serde_json::from_value(saved).unwrap();
        assert_eq!(saved.repair_count(), count - 1);
        let messages: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM messages WHERE company_id=$1 AND role='agent'",
        )
        .bind(fx.company.id)
        .fetch_one(&pool)
        .await
        .unwrap();
        let deliveries: i64 =
            sqlx::query_scalar("SELECT count(*) FROM message_deliveries WHERE company_id=$1")
                .bind(fx.company.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        if valid {
            assert!(
                matches!(result, Ok(DispatchOutcome::Replied(_))),
                "{result:?}"
            );
            let output = saved.final_output.unwrap();
            assert_eq!(output.content, r#"{"status":"done"}"#);
            assert!(output.structured.is_some());
            assert_eq!(messages, 1);
            assert_eq!(deliveries, i64::from(delivery_mode == ReplyDelivery::Send));
            let (body,stored):(String,Value)=sqlx::query_as("SELECT clean_text_body,structured_response FROM messages WHERE company_id=$1 AND role='agent'").bind(fx.company.id).fetch_one(&pool).await.unwrap();
            assert_eq!(body, output.content);
            assert_eq!(stored["contract"], contract());
            if delivery_mode == ReplyDelivery::Send {
                let payload:Value=sqlx::query_scalar("SELECT part.payload FROM message_delivery_parts AS part JOIN message_deliveries AS delivery ON delivery.id=part.delivery_id WHERE delivery.company_id=$1").bind(fx.company.id).fetch_one(&pool).await.unwrap();
                assert!(payload.to_string().contains("status"));
            }
        } else {
            assert!(matches!(
                result,
                Err(AppError::Execution(
                    crate::app_error::ExecutionFailure::InvalidOutput
                ))
            ));
            assert!(saved.final_output.is_none());
            assert_eq!(
                saved.state,
                crate::services::harness::runs::RunState::InvalidOutput
            );
            assert_eq!((messages, deliveries), (0, 0));
            let drafts: i64 =
                sqlx::query_scalar("SELECT count(*) FROM response_drafts WHERE company_id=$1")
                    .bind(fx.company.id)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(drafts, 0);
        }
        CompanyPersistence::delete(fx.persistence.as_ref(), fx.company.id)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn structured_review_rejects_invalid_edits_and_serializes_edit_against_approval() {
    use crate::{
        entities::{
            response_draft::{ExternalResponseReview, ResponseDraftId},
            transport::PrincipalId,
        },
        use_cases::response_review::{ResponseReviewPersistence, ReviewAction, ReviewCommand},
    };
    let Some(pool) = test_pool().await else {
        return;
    };
    let _guard = crate::adapters::persistence::test_support::UNSCOPED_CLAIM
        .lock()
        .await;
    let llm = scripted_scenario(vec![ScriptedExchange::new(
        |_| Ok(()),
        ScriptedResponse::turn(LlmTurn::text(r#"{"status":"done"}"#), 1),
    )])
    .await;
    let fx = fixture_for_harness(pool.clone(), Some(&llm.base_url), HarnessKind::Rig).await;
    configure(&fx).await;
    fx.persistence
        .set_company_review_policy(fx.company.id, ExternalResponseReview::ReviewAllExternal)
        .await
        .unwrap();
    let ingest = fx
        .threads
        .ingest_test_email(inbound(
            &fx.owner_email,
            &fx.address(&fx.channel_a),
            "<structured-review@example.test>",
            "Return status for review",
        ))
        .await
        .unwrap();
    let task = ingest.task_id.unwrap();
    let lease = fx.claim(task).await;
    let result = fx
        .threads
        .execute_claimed_agent_task_and_dispatch(
            &ingest,
            ReplyDelivery::Send,
            lease,
            ingest.correlation_id().unwrap(),
        )
        .await
        .unwrap();
    assert!(matches!(result, DispatchOutcome::Suspended));
    assert_eq!(llm.finish().await, Ok(1));
    let (id,thread,reviewer):(Uuid,Uuid,Uuid)=sqlx::query_as("SELECT draft.id,draft.thread_id,review.reviewer_principal_id FROM response_drafts AS draft JOIN response_reviews AS review ON (review.company_id,review.draft_id,review.draft_version)=(draft.company_id,draft.id,draft.version) WHERE draft.task_id=$1").bind(task).fetch_one(&pool).await.unwrap();
    let draft = ResponseDraftId::new(id);
    let reviewer = PrincipalId::new(reviewer);
    let publication = fx
        .persistence
        .publication_for_reviewer(fx.company.id, draft, 1, reviewer)
        .await
        .unwrap()
        .unwrap();
    let edit = |body| crate::use_cases::thread::ResponseReviewEditDraft {
        draft_id: draft,
        current_version: 1,
        company_id: fx.company.id,
        channel_id: fx.channel_a.id,
        thread_id: thread,
        task_id: Some(task),
        source_handoff_generation: None,
        actor_principal_id: reviewer,
        subject: "Re: reviewed",
        text_body: body,
        recipient_to: fx.owner_email.clone().into(),
        recipients_cc: vec![],
        evidence: vec![],
        current_publication: &publication,
    };
    assert!(matches!(
        fx.threads.prepare_response_review_edit(edit("{}")).await,
        Err(AppError::BadRequest(_))
    ));
    let unchanged = fx
        .persistence
        .publication_for_reviewer(fx.company.id, draft, 1, reviewer)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(unchanged.message().clean_text_body, r#"{"status":"done"}"#);
    let replacement = fx
        .threads
        .prepare_response_review_edit(edit(r#" { "status": "done" } "#))
        .await
        .unwrap();
    assert_eq!(
        replacement
            .publication
            .message()
            .structured
            .as_ref()
            .unwrap()
            .contract(),
        publication
            .message()
            .structured
            .as_ref()
            .unwrap()
            .contract()
    );
    let command = |action| ReviewCommand {
        company_id: fx.company.id,
        draft_id: draft,
        expected_draft_version: 1,
        command_id: Uuid::new_v4(),
        actor_principal_id: reviewer,
        action,
    };
    let (edited, approved) = tokio::join!(
        fx.persistence
            .execute_review_command(command(ReviewAction::Edit {
                replacement: Box::new(replacement)
            })),
        fx.persistence
            .execute_review_command(command(ReviewAction::Approve { rationale: None }))
    );
    assert_eq!(
        usize::from(edited.is_ok()) + usize::from(approved.is_ok()),
        1
    );
    if edited.is_ok() {
        fx.persistence
            .execute_review_command(ReviewCommand {
                expected_draft_version: 2,
                ..command(ReviewAction::Approve { rationale: None })
            })
            .await
            .unwrap();
    }
    let stored:Value=sqlx::query_scalar("SELECT structured_response FROM messages WHERE company_id=$1 AND structured_response IS NOT NULL").bind(fx.company.id).fetch_one(&pool).await.unwrap();
    assert_eq!(stored["contract"], contract());
    assert_eq!(stored["body"], r#"{"status":"done"}"#);
    let deliveries: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM message_deliveries WHERE company_id=$1 AND purpose='reply'",
    )
    .bind(fx.company.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(deliveries, 1);
    CompanyPersistence::delete(fx.persistence.as_ref(), fx.company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn structured_scheduled_dispatch_delivers_the_validated_json_body() {
    use crate::entities::schedule::{ScheduleDeliveryMode, ScheduledRunPayload};
    let Some(pool) = test_pool().await else {
        return;
    };
    let _guard = crate::adapters::persistence::test_support::UNSCOPED_CLAIM
        .lock()
        .await;
    let llm = scripted_scenario(vec![ScriptedExchange::new(
        |_| Ok(()),
        ScriptedResponse::turn(LlmTurn::text(r#"{"status":"done"}"#), 1),
    )])
    .await;
    let fx = fixture_for_harness(pool.clone(), Some(&llm.base_url), HarnessKind::Rig).await;
    configure(&fx).await;
    sqlx::query("UPDATE agents SET granted_tool_ids='{}' WHERE id IN (SELECT agent_id FROM channel_agents WHERE channel_id=$1)")
        .bind(fx.channel_a.id).execute(&pool).await.unwrap();
    let ingest = fx
        .threads
        .ingest_test_email(inbound(
            &fx.owner_email,
            &fx.address(&fx.channel_a),
            "<structured-schedule@example.test>",
            "Scheduled status prompt",
        ))
        .await
        .unwrap();
    let task_id = ingest.task_id.unwrap();
    let task = fx
        .persistence
        .get_task_by_id(task_id)
        .await
        .unwrap()
        .unwrap();
    let thread = task.thread_id.unwrap();
    let prompt: Uuid = sqlx::query_scalar(
        "SELECT message_id FROM thread_messages WHERE thread_id=$1 ORDER BY created_at,id LIMIT 1",
    )
    .bind(thread)
    .fetch_one(&pool)
    .await
    .unwrap();
    let payload = ScheduledRunPayload {
        schedule_run_id: None,
        schedule_id: Uuid::new_v4(),
        schedule_name: "Structured report".into(),
        channel_id: fx.channel_a.id,
        company_id: fx.company.id,
        thread_id: thread,
        subject: "Status report".into(),
        prompt: "Return status".into(),
        delivery_mode: ScheduleDeliveryMode::EmailCustom,
        recipient_emails: vec![fx.owner_email.clone().into()],
        run_as: None,
        run_key: Uuid::new_v4(),
        prompt_message_id: crate::entities::message::CanonicalMessageId::new(prompt),
    };
    sqlx::query(
        "UPDATE background_tasks SET task_type='scheduled_agent_run',payload=$2 WHERE id=$1",
    )
    .bind(task_id)
    .bind(serde_json::to_value(payload).unwrap())
    .execute(&pool)
    .await
    .unwrap();
    let lease = fx.claim(task_id).await;
    let task = fx
        .persistence
        .get_task_by_id(task_id)
        .await
        .unwrap()
        .unwrap();
    let output = fx
        .threads
        .execute_claimed_scheduled_agent_task_and_dispatch(&task, lease)
        .await
        .unwrap();
    assert!(matches!(output, DispatchOutcome::Replied(_)));
    assert_eq!(llm.finish().await, Ok(1));
    let (body,snapshot):(String,Value)=sqlx::query_as("SELECT clean_text_body,structured_response FROM messages WHERE company_id=$1 AND role='agent'").bind(fx.company.id).fetch_one(&pool).await.unwrap();
    assert_eq!(body, r#"{"status":"done"}"#);
    assert_eq!(snapshot["contract"], contract());
    let payload:crate::transport::TransportPayload=sqlx::query_scalar::<_,Value>("SELECT part.payload FROM message_delivery_parts AS part JOIN message_deliveries AS delivery ON delivery.id=part.delivery_id WHERE delivery.company_id=$1").bind(fx.company.id).fetch_one(&pool).await.map(|value| serde_json::from_value(value).unwrap()).unwrap();
    let email: crate::adapters::protocols::email::OutboundEmailV1 = payload
        .decode(
            crate::entities::transport::TransportKind::Email,
            crate::adapters::protocols::email::OUTBOUND_EMAIL_VERSION,
        )
        .unwrap();
    assert_eq!(email.body_text, body);
    CompanyPersistence::delete(fx.persistence.as_ref(), fx.company.id)
        .await
        .unwrap();
}
