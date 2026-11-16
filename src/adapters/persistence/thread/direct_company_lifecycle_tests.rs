//! Competing real ingress/company writers obey company-before-principal ownership in both orders.
use super::*;
use crate::application::use_cases::agent::{AgentWrite, CompanyAgents};
use crate::application::use_cases::company::{CompanyModelConnectionWrite, CompanyPersistence};
use crate::application::workflow::{CompanyId, WorkflowActor};
use crate::entities::address::AddressTarget;
use crate::transport::TransportRenderer;
use std::time::Duration;

async fn direct_request(f: &Fixture) -> (InboundCommitRequest, Uuid) {
    sqlx::query("UPDATE users SET email_verified_at=clock_timestamp() WHERE id=$1")
        .bind(f.owner_id)
        .execute(&f.pool)
        .await
        .unwrap();
    CompanyPersistence::replace_model_connections_for_user(
        &f.persistence,
        f.owner_id,
        f.company_id,
        vec![
            CompanyModelConnectionWrite::new(
                "openai",
                Some("test-key".into()),
                vec!["gpt-4o-mini".into()],
                true,
            )
            .unwrap(),
        ],
    )
    .await
    .unwrap();
    let created = Box::pin(
        crate::adapters::persistence::company_agent::CompanyAgentService::new(
            f.persistence.clone(),
            crate::adapters::persistence::test_support::provisioning_templates(),
        )
        .create(
            WorkflowActor::authenticated(f.owner_id).unwrap(),
            CompanyId::new(f.company_id),
            AgentWrite {
                name: "Lifecycle agent".into(),
                slug: "lifecycle-agent".into(),
                ..Default::default()
            },
            None,
        ),
    )
    .await
    .unwrap();
    let principal = created.principal_id.as_uuid();
    let (endpoint,revision):(Uuid,i64)=sqlx::query_as(
        "SELECT endpoint.id,principal.settings_revision FROM principals AS principal JOIN channel_bindings AS endpoint \
         ON endpoint.company_id=principal.company_id AND endpoint.principal_id=principal.id \
         WHERE principal.company_id=$1 AND principal.id=$2 AND endpoint.transport='email' AND endpoint.status='active'")
        .bind(f.company_id).bind(principal).fetch_one(&f.pool).await.unwrap();
    let company: String = sqlx::query_scalar("SELECT slug::text FROM companies WHERE id=$1")
        .bind(f.company_id)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    let owner: String = sqlx::query_scalar("SELECT email::text FROM users WHERE id=$1")
        .bind(f.owner_id)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    let address = crate::adapters::protocols::email::EmailRenderer::new("example.test")
        .application_address(&company.into(), &created.agent.slug.into())
        .unwrap();
    let mut request = request(
        f,
        &format!("<company-race-{}@example.test>", Uuid::new_v4()),
        "Direct ingress race",
    )
    .await;
    let binding = ChannelBindingId::new(endpoint);
    let target = AddressTarget::Principal(created.principal_id);
    request.envelope.source.binding_id = binding;
    request.envelope.author = identity(&owner);
    request.envelope.addressed = BoundedVec::parse(
        "direct recipient",
        vec![crate::transport::AddressedIdentity::new(
            RecipientRole::To,
            address,
        )],
    )
    .unwrap();
    request.associations = BoundedVec::parse(
        "direct association",
        vec![ThreadAssociation {
            address_target: target,
            binding_id: binding,
            target: ThreadTarget::Create {
                subject: "Direct ingress race".into(),
            },
            role: RecipientRole::To,
            step: PipelineStep::only(),
            principals: BoundedVec::empty(),
        }],
    )
    .unwrap();
    request.intents = BoundedVec::parse(
        "direct intent",
        vec![InboundAdmissionIntent {
            target,
            role: RecipientRole::To,
            mentioned: false,
            reply_to: None,
            settings_revision: revision,
        }],
    )
    .unwrap();
    (request, principal)
}

async fn blocked_by(f: &Fixture, blocker: i32) -> i32 {
    tokio::time::timeout(Duration::from_secs(3),async {
        loop {
            let pid:Option<i32>=sqlx::query_scalar(
                "SELECT pid FROM pg_stat_activity WHERE datname=current_database() AND $1=ANY(pg_blocking_pids(pid)) ORDER BY pid LIMIT 1")
                .bind(blocker).fetch_optional(&f.pool).await.unwrap();
            if let Some(pid)=pid {return pid;}
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.expect("the competing production writer must reach the asserted native lock")
}

enum First {
    Ingress,
    Delete,
}

async fn race(first: First) {
    let Some(f) = Fixture::isolated("direct_company_race").await else {
        return;
    };
    let (request, principal) = Box::pin(direct_request(&f)).await;
    let foreign = f.foreign_company().await;
    let foreign_before: serde_json::Value =
        sqlx::query_scalar("SELECT to_jsonb(company) FROM companies AS company WHERE id=$1")
            .bind(foreign)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    let mut gate = f.pool.begin().await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM principals WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(f.company_id)
        .bind(principal)
        .execute(&mut *gate)
        .await
        .unwrap();
    let company = f.company_id;
    let p = f.persistence.clone();
    let delete = move || tokio::spawn(async move { CompanyPersistence::delete(&p, company).await });
    let p = f.persistence.clone();
    let ingest = move || tokio::spawn(async move { p.commit_inbound(request).await });
    let (ingress, deletion) = match first {
        First::Ingress => {
            let ingress = ingest();
            let ingress_pid = blocked_by(&f, blocker).await;
            let deletion = delete();
            blocked_by(&f, ingress_pid).await;
            (ingress, deletion)
        }
        First::Delete => {
            let deletion = delete();
            let deletion_pid = blocked_by(&f, blocker).await;
            let ingress = ingest();
            blocked_by(&f, deletion_pid).await;
            (ingress, deletion)
        }
    };
    gate.commit().await.unwrap();
    let result = ingress.await.unwrap();
    match first {
        First::Ingress => assert_eq!(result.unwrap().disposition, CommitDisposition::Created),
        First::Delete => assert!(matches!(result, Err(AppError::NotFound(_))), "{result:?}"),
    }
    deletion.await.unwrap().unwrap();
    for table in [
        "companies",
        "principals",
        "addresses",
        "threads",
        "messages",
        "workflow_message_intents",
        "external_threads",
        "message_deliveries",
    ] {
        let column = if table == "companies" {
            "id"
        } else {
            "company_id"
        };
        let count: i64 =
            sqlx::query_scalar(&format!("SELECT count(*) FROM {table} WHERE {column}=$1"))
                .bind(company)
                .fetch_one(&f.pool)
                .await
                .unwrap();
        assert_eq!(count, 0, "{table} cannot retain tenant orphans");
    }
    let foreign_after: serde_json::Value =
        sqlx::query_scalar("SELECT to_jsonb(company) FROM companies AS company WHERE id=$1")
            .bind(foreign)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(foreign_after, foreign_before);
}

#[tokio::test]
async fn direct_competing_company_delete_waits_for_ingress_then_cascades_every_direct_effect() {
    Box::pin(race(First::Ingress)).await;
}

#[tokio::test]
async fn direct_competing_ingress_waits_for_company_delete_then_refuses_without_effects() {
    Box::pin(race(First::Delete)).await;
}

#[path = "direct_agent_lifecycle_race_tests.rs"]
mod agent_races;
