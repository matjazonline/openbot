use super::*;
use crate::application::notification::NotificationPersistence;
use crate::entities::notification::NotificationId;
use crate::use_cases::company_invite::CompanyInvitePersistence;

fn worker(
    f: &PeopleFixture,
) -> crate::application::services::notification_worker::NotificationWorker {
    crate::adapters::persistence::test_support::notification_worker(f.role.p(), Default::default())
}

async fn pending(count: usize) -> PeopleFixture {
    let f = Box::pin(PeopleFixture::new(count)).await;
    let binding = &f.role.admission.binding;
    let state = binding.state().await;
    let mut request = binding.request(Some(state.revision));
    request.association = RelatedAssociation::Channel(RelatedChannelId::new(f.role.channel));
    request.escalation = Some(EscalationPolicy {
        notify: DepartmentRole::Reviewer,
        hops: EscalationHops::try_from(2).unwrap(),
    });
    binding.service().configure(request).await.unwrap();
    let prepared = f.role.prepared("person-escalation").await;
    clear_reviewers(&f).await;
    assert!(matches!(
        f.role.p().admit(&prepared).await,
        Err(AppError::WorkflowRole(_))
    ));
    for principal in &f.principals {
        assign(&f, f.role.department, *principal).await;
    }
    assert_eq!(
        worker(&f).drain_once().await.unwrap(),
        1,
        "materialize first hop"
    );
    f
}

async fn outcomes(f: &PeopleFixture) -> Vec<String> {
    sqlx::query_scalar("SELECT outcome FROM workflow_escalation_person_receipts WHERE company_id=$1 ORDER BY outcome")
        .bind(f.role.company().as_uuid()).fetch_all(f.role.p().pool()).await.unwrap()
}

async fn counts(f: &PeopleFixture) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('notifications',(SELECT count(*) FROM notifications WHERE company_id=$1), \
        'emails',(SELECT count(*) FROM message_deliveries WHERE company_id=$1 AND purpose='notification'), \
        'failures',(SELECT count(*) FROM workflow_escalation_recipient_failures WHERE company_id=$1), \
        'runs',(SELECT count(*) FROM workflow_runs WHERE company_id=$1), \
        'hops',(SELECT count(*) FROM workflow_run_escalations WHERE company_id=$1))")
        .bind(f.role.company().as_uuid()).fetch_one(f.role.p().pool()).await.unwrap()
}

async fn no_run_attention(f: &PeopleFixture, decision: Uuid) {
    use crate::application::attention::AttentionPersistence;
    use crate::entities::attention::{AttentionQuery, AttentionSourceKind, AttentionView};
    let channels = [f.role.channel];
    let page = f
        .role
        .p()
        .list_attention(AttentionQuery {
            company_id: f.role.company().as_uuid(),
            principal_id: f.principals[0],
            visible_channel_ids: &channels,
            manager: true,
            company_items: false,
            view: AttentionView::MyWork,
            cursor: None,
            limit: 100,
        })
        .await
        .unwrap();
    let rows: Vec<_> = page
        .items
        .iter()
        .filter(|item| item.source_kind == AttentionSourceKind::EscalationDecision)
        .collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].source_id, decision);
    assert_eq!(
        rows[0].run_id, None,
        "the genuine refused admission never fabricated a run"
    );
    assert_eq!(
        rows[0].href.as_deref(),
        Some(format!("/ui/workflow-decisions/escalations/{decision}").as_str())
    );
    assert_eq!(
        f.role
            .p()
            .operational_summary(f.role.company().as_uuid(), &channels)
            .await
            .unwrap()
            .open_decision_count,
        1
    );
}

#[tokio::test]
async fn workflow_escalation_person_actual_worker_proves_outbox_opt_out_and_secure_open_without_a_run()
 {
    let f = Box::pin(pending(2)).await;
    sqlx::query("INSERT INTO user_notification_preferences(user_id,decision_email_enabled) VALUES ($1,false) \
        ON CONFLICT(user_id) DO UPDATE SET decision_email_enabled=false")
        .bind(f.users[1]).execute(f.role.p().pool()).await.unwrap();
    assert_eq!(worker(&f).drain_once().await.unwrap(), 2);
    assert_eq!(outcomes(&f).await, vec!["email_queued", "opted_out"]);
    assert_eq!(
        counts(&f).await,
        json!({"notifications":2,"emails":1,"failures":0,"runs":0,"hops":1})
    );
    #[derive(sqlx::FromRow)]
    struct Link {
        id: Uuid,
        decision_id: Uuid,
    }
    let link: Link = sqlx::query_as("SELECT notification.id,source.decision_id FROM notifications AS notification \
        JOIN workflow_escalation_notices AS source ON source.company_id=notification.company_id AND source.event_id=notification.event_id \
        WHERE notification.company_id=$1 AND notification.recipient_user_id=$2")
        .bind(f.role.company().as_uuid()).bind(f.users[0]).fetch_one(f.role.p().pool()).await.unwrap();
    no_run_attention(&f, link.decision_id).await;
    let href = f
        .role
        .p()
        .open_notification(
            f.role.company().as_uuid(),
            f.users[0],
            f.principals[0],
            &[f.role.channel],
            NotificationId::new(link.id),
        )
        .await
        .unwrap();
    assert_eq!(
        href,
        format!("/ui/workflow-decisions/escalations/{}", link.decision_id)
    );
    assert!(
        f.role
            .p()
            .open_notification(
                f.role.company().as_uuid(),
                f.users[1],
                f.principals[1],
                &[f.role.channel],
                NotificationId::new(link.id)
            )
            .await
            .is_err()
    );
    assert_eq!(worker(&f).drain_once().await.unwrap(), 0);
    assert_eq!(
        outcomes(&f).await.len(),
        2,
        "replay cannot send a second notice"
    );
}

#[tokio::test]
async fn workflow_escalation_person_one_retired_recipient_preserves_the_capable_recipients_window()
{
    let f = Box::pin(pending(2)).await;
    f.role
        .p()
        .remove_member(f.role.company().as_uuid(), f.users[0])
        .await
        .unwrap();
    assert_eq!(worker(&f).drain_once().await.unwrap(), 2);
    assert_eq!(outcomes(&f).await, vec!["email_queued", "failed"]);
    assert_eq!(
        counts(&f).await,
        json!({"notifications":1,"emails":1,"failures":1,"runs":0,"hops":1})
    );
    assert_eq!(
        worker(&f).drain_once().await.unwrap(),
        0,
        "a capable recipient keeps the original deadline"
    );
    let waiting: bool = sqlx::query_scalar("SELECT state='waiting' AND current_hop=1 FROM workflow_escalation_progress WHERE company_id=$1")
        .bind(f.role.company().as_uuid()).fetch_one(f.role.p().pool()).await.unwrap();
    assert!(waiting);
}

#[tokio::test]
async fn workflow_escalation_person_all_retired_recipients_expedite_the_existing_control_to_owner_attention()
 {
    let f = Box::pin(pending(2)).await;
    for user in &f.users {
        f.role
            .p()
            .remove_member(f.role.company().as_uuid(), *user)
            .await
            .unwrap();
    }
    assert_eq!(
        worker(&f).drain_once().await.unwrap(),
        3,
        "two failed notices and the control made due by retirement"
    );
    assert_eq!(outcomes(&f).await, vec!["failed", "failed"]);
    for _ in 0..3 {
        if worker(&f).drain_once().await.unwrap() == 0 {
            break;
        }
    }
    let owner: bool = sqlx::query_scalar("SELECT state='owner_waiting' AND current_hop=2 FROM workflow_escalation_progress WHERE company_id=$1")
        .bind(f.role.company().as_uuid()).fetch_one(f.role.p().pool()).await.unwrap();
    assert!(owner);
    assert_eq!(
        counts(&f).await,
        json!({"notifications":0,"emails":0,"failures":2,"runs":0,"hops":2})
    );
    assert_eq!(
        worker(&f).drain_once().await.unwrap(),
        0,
        "there is no third automatic hop"
    );
}

#[tokio::test]
async fn workflow_escalation_person_receipts_reject_forged_opt_out_and_source_rebinding() {
    let f = Box::pin(pending(2)).await;
    assert_eq!(worker(&f).drain_once().await.unwrap(), 2);
    for statement in [
        "UPDATE workflow_escalation_person_receipts SET outcome='opted_out',email_delivery_id=NULL WHERE company_id=$1",
        "DELETE FROM workflow_escalation_person_receipts WHERE company_id=$1",
        "UPDATE notifications SET recipient_principal_id=$2 WHERE company_id=$1 AND recipient_principal_id<>$2",
        "UPDATE notifications SET channel_id=NULL WHERE company_id=$1",
        "INSERT INTO workflow_escalation_person_receipts(company_id,notice_id,event_id,outcome) SELECT company_id,source_id,id,'opted_out' FROM notification_events WHERE company_id=$1 AND source_kind='escalation_notice' ON CONFLICT DO NOTHING",
    ] {
        let mut tx = f.role.p().pool().begin().await.unwrap();
        let mut query = sqlx::query(statement).bind(f.role.company().as_uuid());
        if statement.contains("$2") {
            query = query.bind(f.principals[0].as_uuid());
        }
        let error = query.execute(&mut *tx).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514"),
            "{statement}: {error}"
        );
        tx.rollback().await.unwrap();
    }
    assert_eq!(outcomes(&f).await, vec!["email_queued", "email_queued"]);
}

#[tokio::test]
async fn workflow_escalation_person_competing_actual_workers_commit_one_receipt_and_email_per_person()
 {
    let f = Box::pin(pending(4)).await;
    let first = worker(&f);
    let second = worker(&f);
    let (a, b) = tokio::join!(first.drain_once(), second.drain_once());
    assert_eq!(a.unwrap() + b.unwrap(), 4);
    assert_eq!(outcomes(&f).await, vec!["email_queued"; 4]);
    assert_eq!(
        counts(&f).await,
        json!({"notifications":4,"emails":4,"failures":0,"runs":0,"hops":1})
    );
}

#[tokio::test]
async fn workflow_escalation_person_post_projection_retirement_cancels_email_and_expedites_without_rewriting_receipt()
 {
    let f = Box::pin(pending(1)).await;
    assert_eq!(worker(&f).drain_once().await.unwrap(), 1);
    f.role
        .p()
        .remove_member(f.role.company().as_uuid(), f.users[0])
        .await
        .unwrap();
    let cancelled: bool = sqlx::query_scalar("SELECT bool_and(cancellation_reason='recipient_revoked' AND cancellation_requested_at IS NOT NULL) \
        FROM message_deliveries WHERE company_id=$1 AND purpose='notification'")
        .bind(f.role.company().as_uuid()).fetch_one(f.role.p().pool()).await.unwrap();
    assert!(cancelled, "actual membership removal cancels queued email");
    for _ in 0..3 {
        if worker(&f).drain_once().await.unwrap() == 0 {
            break;
        }
    }
    let owner: bool = sqlx::query_scalar("SELECT state='owner_waiting' AND current_hop=2 FROM workflow_escalation_progress WHERE company_id=$1")
        .bind(f.role.company().as_uuid()).fetch_one(f.role.p().pool()).await.unwrap();
    assert!(
        owner,
        "a delivered notice losing its last capable person expedites its existing control"
    );
    assert_eq!(
        outcomes(&f).await,
        vec!["email_queued"],
        "retirement retains the original durable outbox history"
    );
    let failures: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_escalation_recipient_failures WHERE company_id=$1",
    )
    .bind(f.role.company().as_uuid())
    .fetch_one(f.role.p().pool())
    .await
    .unwrap();
    assert_eq!(failures, 1);
}

#[tokio::test]
async fn workflow_escalation_person_lost_lease_during_receipt_rolls_back_every_projection_effect() {
    use crate::application::notification::{
        NotificationProjectionCommit, NotificationProjectionCommitOutcome,
    };
    use crate::transport::WorkerId;
    let f = Box::pin(pending(1)).await;
    sqlx::query("INSERT INTO user_notification_preferences(user_id,decision_email_enabled) VALUES ($1,false)")
        .bind(f.users[0]).execute(f.role.p().pool()).await.unwrap();
    sqlx::raw_sql("CREATE SEQUENCE escalation_person_receipt_reached; \
        CREATE FUNCTION slow_person_receipt_test() RETURNS trigger LANGUAGE plpgsql AS $$ \
        BEGIN PERFORM nextval('escalation_person_receipt_reached'); PERFORM pg_sleep(1.1); RETURN NEW; END $$; \
        CREATE TRIGGER slow_person_receipt_test AFTER INSERT ON workflow_escalation_person_receipts \
        FOR EACH ROW EXECUTE FUNCTION slow_person_receipt_test()")
        .execute(f.role.p().pool()).await.unwrap();
    let claim = f
        .role
        .p()
        .claim_notification_events(WorkerId::random(), std::time::Duration::from_secs(1), 1)
        .await
        .unwrap()
        .pop()
        .unwrap();
    let expected = f
        .role
        .p()
        .resolve_notification_projection(&claim.event)
        .await
        .unwrap();
    assert!(!expected.should_email());
    let (result, _) = f
        .role
        .p()
        .commit_notification_projection(NotificationProjectionCommit {
            fence: &claim.lease,
            expected: &expected,
            email: None,
            agent_message_key: None,
            escalation_ack_hours: Default::default(),
        })
        .await
        .unwrap();
    assert_eq!(result, NotificationProjectionCommitOutcome::LeaseLost);
    let reached: bool =
        sqlx::query_scalar("SELECT is_called FROM escalation_person_receipt_reached")
            .fetch_one(f.role.p().pool())
            .await
            .unwrap();
    assert!(
        reached,
        "expiry before receipt insertion does not prove rollback after its write"
    );
    assert!(outcomes(&f).await.is_empty());
    assert_eq!(
        counts(&f).await,
        json!({"notifications":0,"emails":0,"failures":0,"runs":0,"hops":1})
    );
}

#[tokio::test]
async fn workflow_escalation_person_source_uuid_and_public_stamp_validation_agree_in_rust_and_sql()
{
    use crate::adapters::persistence::workflow::admission_binding::{
        AdmissionPayloadSource, SourceKey,
    };
    let f = Box::pin(PeopleFixture::new(0)).await;
    let id = Uuid::new_v4();
    let canonical = id.to_string();
    for identity in [
        canonical.clone(),
        id.simple().to_string(),
        format!("{{{id}}}"),
        format!("urn:uuid:{id}"),
        "broken".into(),
        canonical.replace('-', "--"),
    ] {
        for kind in ["manual", "schedule", "message", "child"] {
            let body = if kind == "schedule" {
                json!([kind, identity, id])
            } else {
                json!([kind, identity])
            };
            let key = format!("v1:{body}");
            let expected = match SourceKey::payload_source(&key) {
                Ok(AdmissionPayloadSource::Public) => Some(0_i16),
                Ok(AdmissionPayloadSource::Message(_)) | Err(_) => None,
            };
            let actual: Option<i16> =
                sqlx::query_scalar("SELECT workflow_refusal_content_level($1,$2)")
                    .bind(f.role.company().as_uuid())
                    .bind(&key)
                    .fetch_one(f.role.p().pool())
                    .await
                    .unwrap();
            assert_eq!(actual, expected, "{key}");
        }
    }
}

#[tokio::test]
async fn workflow_escalation_person_receipt_cannot_commit_without_fenced_event_completion() {
    use crate::transport::WorkerId;
    let f = Box::pin(pending(1)).await;
    sqlx::query("INSERT INTO user_notification_preferences(user_id,decision_email_enabled) VALUES ($1,false)")
        .bind(f.users[0]).execute(f.role.p().pool()).await.unwrap();
    let claim = f
        .role
        .p()
        .claim_notification_events(WorkerId::random(), std::time::Duration::from_secs(30), 1)
        .await
        .unwrap()
        .pop()
        .unwrap();
    let mut tx = f.role.p().pool().begin().await.unwrap();
    sqlx::query("INSERT INTO notifications(id,company_id,recipient_user_id,recipient_principal_id,event_id,source_kind,source_id,action_kind,source_generation,channel_id) \
        SELECT event.notification_id,event.company_id,$2,source.principal_id,event.id,event.source_kind,event.source_id,event.action_kind,event.source_generation,$3 \
        FROM notification_events AS event JOIN workflow_escalation_notices AS source ON source.company_id=event.company_id AND source.event_id=event.id \
        WHERE event.id=$1")
        .bind(claim.event.id.as_uuid()).bind(f.users[0]).bind(f.role.channel).execute(&mut *tx).await.unwrap();
    sqlx::query(
        "INSERT INTO workflow_escalation_person_receipts(company_id,notice_id,event_id,outcome) \
        SELECT company_id,source_id,id,'opted_out' FROM notification_events WHERE id=$1",
    )
    .bind(claim.event.id.as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap();
    let error = tx.commit().await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("notification_receipt_completion"),
        "{error}"
    );
    assert!(outcomes(&f).await.is_empty());
    assert_eq!(
        counts(&f).await,
        json!({"notifications":0,"emails":0,"failures":0,"runs":0,"hops":1})
    );
}
