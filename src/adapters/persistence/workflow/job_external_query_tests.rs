//! Mixed jobs use the fully migrated schema in an isolated disposable database.
use super::super::notification_fixtures;
use super::*;
use crate::{
    application::{attention::AttentionPersistence, notification::NotificationPersistence},
    entities::{attention::*, notification::*, transport::PrincipalId},
};

#[tokio::test]
async fn workflow_job_external_attention_preserves_legacy_and_taskless_sources() {
    let (f, workflow, legacy) = mixed_fixture().await;
    let persistence = &f.binding.fixture.persistence;
    let company = f.binding.target.company.as_uuid();
    let actor: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 AND user_id = $2")
            .bind(company)
            .bind(f.binding.target.actor.user_id())
            .fetch_one(persistence.pool())
            .await
            .unwrap();
    let visible = [f.channel];
    sqlx::query("UPDATE background_tasks SET status = 'dead_letter' WHERE id = $1")
        .bind(legacy)
        .execute(persistence.pool())
        .await
        .unwrap();
    let query = || AttentionQuery {
        company_id: company,
        principal_id: PrincipalId::new(actor),
        visible_channel_ids: &visible,
        view: AttentionView::TeamWork,
        all_owned: false,
        cursor: None,
        limit: 100,
    };
    let page = persistence.list_attention(query()).await.unwrap();
    assert_eq!(
        page.items
            .iter()
            .map(|item| item.source_id)
            .collect::<Vec<_>>(),
        vec![legacy]
    );
    let summary = persistence
        .operational_summary(company, &visible)
        .await
        .unwrap();
    assert_eq!(summary.unassigned_count, 1);
    let mut connection = persistence.pool().acquire().await.unwrap();
    let taskless = notification_fixtures::auxiliary_sources(&f, &mut connection, None).await;
    let linked = notification_fixtures::auxiliary_sources(&f, &mut connection, Some(legacy)).await;
    drop(connection);
    let page = persistence.list_attention(query()).await.unwrap();
    let ids: Vec<_> = page.items.iter().map(|item| item.source_id).collect();
    assert!(!ids.contains(&workflow));
    for id in taskless.iter().chain(&linked) {
        assert!(ids.contains(id), "missing source {id}");
    }
    assert_eq!(ids.len(), 6);
    let summary = persistence
        .operational_summary(company, &visible)
        .await
        .unwrap();
    assert_eq!(summary.unassigned_count, 2);
    assert_eq!(summary.permanent_delivery_failure_count, 2);
    let version: i64 =
        sqlx::query_scalar("SELECT attention_version FROM background_tasks WHERE id = $1")
            .bind(legacy)
            .fetch_one(persistence.pool())
            .await
            .unwrap();
    for task_id in [workflow, legacy] {
        let command = AttentionSourceCommand {
            company_id: company,
            source_kind: AttentionSourceKind::Task,
            source_id: task_id,
            command_id: Uuid::new_v4(),
            expected_version: version as u64,
            actor_principal_id: PrincipalId::new(actor),
            visible_channel_ids: visible.to_vec(),
            priority: BusinessPriority::High,
            due_at: None,
            responsible_principal_id: None,
        };
        if task_id == workflow {
            let fingerprint =
                crate::adapters::persistence::attention::command_fingerprint(&command).unwrap();
            sqlx::query("INSERT INTO attention_source_events (company_id,source_kind,source_id,command_id,command_fingerprint,operation,actor_principal_id,from_version,to_version,previous_priority,new_priority) VALUES ($1,'task',$2,$3,$4,'attributes_changed',$5,1,2,'normal','high')")
                .bind(company).bind(workflow).bind(command.command_id).bind(fingerprint).bind(actor)
                .execute(persistence.pool()).await.unwrap();
        }
        let result = persistence.change_source_attributes(command.clone()).await;
        if task_id == workflow {
            assert!(result.is_err());
        } else {
            assert_eq!(result.unwrap(), version as u64 + 1);
            assert_eq!(
                persistence.change_source_attributes(command).await.unwrap(),
                version as u64 + 1
            );
        }
    }
}

#[tokio::test]
async fn workflow_job_external_notification_sources_exclude_workflow_jobs() {
    let (f, workflow, legacy) = mixed_fixture().await;
    let persistence = &f.binding.fixture.persistence;
    let company = f.binding.target.company.as_uuid();
    let actor: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 AND user_id = $2")
            .bind(company)
            .bind(f.binding.target.actor.user_id())
            .fetch_one(persistence.pool())
            .await
            .unwrap();
    sqlx::query("UPDATE background_tasks SET owner_principal_id = $2, owner_principal_kind = 'person' WHERE id = $1")
        .bind(legacy).bind(actor).execute(persistence.pool()).await.unwrap();
    // Without the source discriminator this terminal workflow row resolves the event.
    sqlx::query("UPDATE background_tasks SET status = 'completed' WHERE id = $1")
        .bind(workflow)
        .execute(persistence.pool())
        .await
        .unwrap();
    for task in [workflow, legacy] {
        let event = NotificationEvent {
            id: NotificationEventId::new(Uuid::new_v4()),
            notification_id: NotificationId::new(Uuid::new_v4()),
            company_id: company,
            source_kind: NotificationSourceKind::Task,
            source_id: task,
            action_kind: NotificationActionKind::Assignment,
            source_generation: 1,
            actor_principal_id: None,
            occurred_at: chrono::Utc::now(),
        };
        let projection = persistence
            .resolve_notification_projection(&event)
            .await
            .unwrap();
        if task == workflow {
            assert!(matches!(
                projection.disposition,
                NotificationDisposition::Withdrawn
            ));
        } else {
            assert!(matches!(
                projection.disposition,
                NotificationDisposition::Active(_)
            ));
        }
    }
}

#[tokio::test]
async fn workflow_job_external_review_evidence_rejects_shared_workflow_attempts() {
    use crate::{
        adapters::persistence::{
            response_review::create_review_draft_on,
            test_support::{DeliveryFixtureRequest, delivery_fixture},
        },
        entities::{
            correlation::CorrelationId,
            message::{MessageDirection, MessageRole},
            response_draft::*,
        },
        use_cases::{
            response_review::{DraftPublicationSnapshot, PreparedReviewDraft},
            thread::{MessageAuthorWrite, MessageWrite},
        },
    };
    let (f, workflow, legacy) = mixed_fixture().await;
    let p = &f.binding.fixture.persistence;
    let company = f.binding.target.company.as_uuid();
    let actor: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 AND user_id = $2")
            .bind(company)
            .bind(f.binding.target.actor.user_id())
            .fetch_one(p.pool())
            .await
            .unwrap();
    let actor = PrincipalId::new(actor);
    let thread = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO threads (id,company_id,channel_id,subject) VALUES ($1,$2,$3,'Evidence')",
    )
    .bind(thread)
    .bind(company)
    .bind(f.channel)
    .execute(p.pool())
    .await
    .unwrap();
    let generation = Uuid::new_v4();
    for task in [workflow, legacy] {
        sqlx::query("INSERT INTO task_attempts (id,task_id,attempt_number,execution_generation,status,worker_id,machine_id) VALUES (gen_random_uuid(),$1,1,$2,'processing',gen_random_uuid(),'test')")
            .bind(task).bind(generation).execute(p.pool()).await.unwrap();
        let queued = delivery_fixture(
            p,
            DeliveryFixtureRequest::new(company, f.channel, thread, "evidence"),
        )
        .await;
        let mut message = MessageWrite::internal(
            thread,
            MessageAuthorWrite::Platform,
            "Re: evidence".to_string(),
            "Answer".to_string(),
            MessageDirection::Outbound,
            MessageRole::Agent,
            CorrelationId::new(),
        )
        .external_conversation();
        message.id = queued.message_id;
        let publication = DraftPublicationSnapshot::new(message, queued.delivery).unwrap();
        let draft = PreparedReviewDraft::new(
            ResponseDraftId::new(Uuid::new_v4()),
            1,
            company,
            f.channel,
            thread,
            None,
            actor,
            actor,
            DraftRecipientSnapshot::email("customer@example.com".into(), vec![]),
            vec![ResponseEvidence {
                id: Uuid::new_v4(),
                source: EvidenceSource::DelegatedResult {
                    task_id: task,
                    execution_generation: generation,
                },
                source_version: "1".into(),
                content_digest: "digest".into(),
                audience: EvidenceAudience::InternalOnly,
                support: EvidenceSupport::DirectEvidence,
            }],
            publication,
        )
        .unwrap();
        let mut tx = p.pool().begin().await.unwrap();
        let result = create_review_draft_on(&mut tx, &draft, Some(actor)).await;
        if task == workflow {
            assert!(
                result.is_err(),
                "shared workflow attempt is not legacy evidence"
            );
        } else {
            assert_eq!(result.unwrap(), actor);
        }
        tx.rollback().await.unwrap();
    }
}
