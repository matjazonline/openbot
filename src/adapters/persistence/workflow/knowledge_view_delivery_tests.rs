//! Classified delivery read models and the actual generic notification projector.
use super::*;
use crate::adapters::persistence::workflow::knowledge;
use crate::application::workflow::polling::WorkflowStepKind;
use crate::application::workflow::run_views::RunViews;
use crate::entities::access_level::AccessLevel;
use crate::use_cases::delivery::{DeliveryReadScope, DeliveryReader};

const SUBJECT: &str = "classified-subject-canary";
const BODY: &str = "classified-body-canary";

async fn classified_delivery() -> Setup {
    let mut fixture = Fixture::new().await;
    let owner = fixture.target.actor.user_id();
    let user = member(&fixture, "admin").await;
    sqlx::query("UPDATE company_members SET access_level=5 WHERE company_id=$1 AND user_id=$2")
        .bind(fixture.target.company.as_uuid())
        .bind(user)
        .execute(fixture.persistence.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE users SET email_verified_at=clock_timestamp() WHERE id=$1")
        .bind(user)
        .execute(fixture.persistence.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO participant_identities(id,company_id,principal_id,transport,namespace,subject,status,claim_metadata,provenance) SELECT $1,$2,principal.id,'email','email',account.email,'verified','{\"version\":1,\"kind\":\"account\"}'::jsonb,'account' FROM principals AS principal JOIN users AS account ON account.id=principal.user_id WHERE principal.company_id=$2 AND principal.user_id=$3")
        .bind(Uuid::new_v4()).bind(fixture.target.company.as_uuid()).bind(user).execute(fixture.persistence.pool()).await.unwrap();
    // The admin is the real command actor at publication/admission, not a rewritten run snapshot.
    fixture.target.actor = WorkflowActor::authenticated(user).unwrap();
    sqlx::query("ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable")
        .execute(fixture.persistence.pool())
        .await
        .unwrap();
    let channel = new_channel(&fixture, "runs").await;
    let other = new_channel(&fixture, "notices").await;
    let with = json!({"content":{"literal":{"subject":SUBJECT,"body":BODY}},"destinations":{"literal":[{"kind":"user","id":user.to_string()}]}});
    // Keep the real publication/admission runtime seams boxed on the stock test stack.
    let version = Box::pin(published(&fixture, "message.send", with)).await;
    let target = Box::pin(bound(&fixture, version)).await;
    let claim = Box::pin(claimed(&fixture, target, channel)).await;
    let setup = Setup {
        fixture,
        channel,
        other,
        user,
        owner,
        claim,
        kind: WorkflowStepKind("message.send".into()),
    };
    let mut tx = setup.pool().begin().await.unwrap();
    knowledge::raise_content_level_on(
        &mut tx,
        setup.scope().company,
        setup.scope().run,
        AccessLevel::L5,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert!(matches!(
        setup.runtime().execute(&setup.kind, &setup.claim).await,
        WorkflowHandlerOutcome::Suspended(_)
    ));
    setup
}

fn read_scope(setup: &Setup, user: Uuid) -> DeliveryReadScope {
    DeliveryReadScope {
        company_id: setup.scope().company.as_uuid(),
        viewer: WorkflowActor::authenticated(user).unwrap(),
    }
}
async fn demote(setup: &Setup) {
    sqlx::query("UPDATE company_members SET access_level=0 WHERE company_id=$1 AND user_id=$2")
        .bind(setup.scope().company.as_uuid())
        .bind(setup.user)
        .execute(setup.pool())
        .await
        .unwrap();
}

#[tokio::test]
async fn knowledge_view_delivery_subjects_use_current_level_and_foreign_id_is_concealed() {
    use crate::entities::delivery::{DeliveryFilter, DeliveryQuery};
    use crate::entities::transport::DeliveryId;
    let setup = Box::pin(classified_delivery()).await;
    let id = DeliveryId::new(latest_delivery(setup.pool(), setup.scope().run).await);
    let p = &setup.fixture.persistence;
    let full = p
        .get_delivery(read_scope(&setup, setup.user), id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(full.subject, SUBJECT);
    let mut diagnostic = full.clone();
    diagnostic.last_error_detail = Some("private-diagnostic-canary".into());
    for part in &mut diagnostic.parts {
        part.last_error_detail = Some("private-part-canary".into());
        part.provider_message_key = Some("private-provider-key-canary".into());
    }
    crate::use_cases::delivery::project_delivery(
        &mut diagnostic,
        crate::application::knowledge::PayloadVisibility {
            viewer: AccessLevel::PUBLIC,
            content: AccessLevel::L5,
        },
    );
    assert!(diagnostic.last_error_detail.is_none());
    assert!(
        diagnostic
            .parts
            .iter()
            .all(|part| part.last_error_detail.is_none() && part.provider_message_key.is_none())
    );
    demote(&setup).await;
    let scope = read_scope(&setup, setup.user);
    let entry = p.get_delivery(scope, id).await.unwrap().unwrap();
    assert_eq!(entry.subject, "Restricted message");
    let list = p
        .list_company_deliveries(scope, &DeliveryFilter::new(DeliveryQuery::default()))
        .await
        .unwrap();
    assert!(
        list.iter()
            .any(|entry| entry.id == id && entry.subject == "Restricted message")
    );
    assert_eq!(
        p.get_delivery(read_scope(&setup, setup.owner), id)
            .await
            .unwrap()
            .unwrap()
            .subject,
        SUBJECT
    );
    assert!(
        p.get_delivery(
            DeliveryReadScope {
                company_id: Uuid::new_v4(),
                ..scope
            },
            id
        )
        .await
        .unwrap()
        .is_none()
    );
}

#[tokio::test]
async fn knowledge_view_notification_email_and_in_app_projection_never_quote_classified_message() {
    use crate::adapters::monitoring::InMemoryMonitor;
    use crate::application::services::notification_worker::NotificationWorker;
    use crate::application::settings::NotificationMailSettings;
    use crate::application::transport::{DeliveryComposer, TransportRenderers};
    let setup = Box::pin(classified_delivery()).await;
    let delivery = latest_delivery(setup.pool(), setup.scope().run).await;
    demote(&setup).await;
    deliver(setup.pool(), delivery, "dead").await;
    let store = Arc::new(setup.fixture.persistence.clone());
    let renderers = Arc::new(
        TransportRenderers::new()
            .register(Arc::new(
                crate::adapters::protocols::email::EmailRenderer::new(APP_DOMAIN),
            ))
            .unwrap(),
    );
    let worker = NotificationWorker::new(
        store.clone(),
        DeliveryComposer::new(renderers, store),
        NotificationMailSettings {
            mail_domain: APP_DOMAIN.into(),
            public_base_url: "https://agents.test".into(),
        },
        Arc::new(InMemoryMonitor::new()),
    );
    // This fixture owns its database: the projector's unscoped claim cannot steal another test's event.
    assert!(worker.drain_once().await.unwrap() > 0);
    let mail: Vec<String> = sqlx::query_scalar("SELECT part.payload::text FROM message_delivery_parts AS part JOIN message_deliveries AS delivery ON delivery.id=part.delivery_id WHERE delivery.company_id=$1 AND delivery.purpose='notification'")
        .bind(setup.scope().company.as_uuid()).fetch_all(setup.pool()).await.unwrap();
    assert!(!mail.is_empty());
    assert!(
        mail.iter()
            .any(|payload| payload.contains("Action required"))
    );
    let in_app: Vec<String> = sqlx::query_scalar("SELECT to_jsonb(notification)::text FROM notifications AS notification WHERE notification.company_id=$1 AND notification.recipient_user_id=$2")
        .bind(setup.scope().company.as_uuid()).bind(setup.user).fetch_all(setup.pool()).await.unwrap();
    assert!(!in_app.is_empty());
    for value in mail.iter().chain(in_app.iter()) {
        assert!(!value.contains(SUBJECT) && !value.contains(BODY));
    }
    // The actual current recipient is now below the message stamp.
    assert_eq!(
        setup
            .fixture
            .persistence
            .run_visibility(crate::application::workflow::run_views::RunReadScope {
                company: setup.scope().company,
                run: setup.scope().run,
                viewer: WorkflowActor::authenticated(setup.user).unwrap()
            })
            .await
            .unwrap()
            .unwrap()
            .viewer,
        AccessLevel::PUBLIC
    );
}
