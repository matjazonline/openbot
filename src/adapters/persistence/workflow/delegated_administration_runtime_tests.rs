use super::*;
use crate::application::{
    ports::department::DepartmentWrite, use_cases::department::DepartmentUseCases,
};
use crate::entities::{department::DepartmentRole, transport::PrincipalId};

async fn principal(d: &Deployment, user: Uuid) -> PrincipalId {
    PrincipalId::new(
        sqlx::query_scalar(
            "SELECT id FROM principals WHERE company_id=$1 AND user_id=$2 AND kind='person'",
        )
        .bind(d.fixture.target.company.as_uuid())
        .bind(user)
        .fetch_one(d.p().pool())
        .await
        .unwrap(),
    )
}

async fn delegated_desk(d: &Deployment, lead: Uuid) -> Uuid {
    let company = d.fixture.target.company;
    let owner = d.fixture.target.actor.user_id();
    let departments = crate::adapters::persistence::department::PostgresDepartments::new(
        d.p().clone(),
        crate::adapters::persistence::test_support::provisioning_templates(),
    );
    let cases = DepartmentUseCases::new(Arc::new(d.p().clone()), Arc::new(departments));
    let write = |slug: &str, parent_id| DepartmentWrite {
        name: slug.into(),
        slug: slug.into(),
        parent_id,
        ..Default::default()
    };
    let sales = cases
        .create_department(owner, company.as_uuid(), write("sales", None), None)
        .await
        .unwrap();
    cases
        .assign_role(
            owner,
            company.as_uuid(),
            sales.id,
            crate::application::ports::department::DepartmentRoleAssignment {
                role: DepartmentRole::Lead,
                principal: principal(d, lead).await,
                decider: false,
            },
        )
        .await
        .unwrap();
    let field = cases
        .create_department(
            lead,
            company.as_uuid(),
            write("field", Some(sales.id)),
            None,
        )
        .await
        .unwrap();
    let agent: Uuid = sqlx::query_scalar("SELECT principal.id FROM principals AS principal JOIN agents AS agent ON agent.company_id=principal.company_id AND agent.id=principal.agent_id WHERE agent.company_id=$1 AND agent.slug='assistant'")
        .bind(company.as_uuid()).fetch_one(d.p().pool()).await.unwrap();
    let remote = cases
        .create_department(
            lead,
            company.as_uuid(),
            write("remote-unit", Some(field.id)),
            Some(PrincipalId::new(agent)),
        )
        .await
        .unwrap();
    let (requester, _) = verified_member(&d.fixture).await;
    cases
        .add_member(
            lead,
            company.as_uuid(),
            remote.id,
            principal(d, requester).await,
        )
        .await
        .unwrap();
    requester
}

#[tokio::test]
async fn delegated_admin_native_department_reply_uses_the_lead_run_as_without_human_view() {
    let d = Box::pin(Deployment::new(vec![LlmTurn::text(
        r#"{"body":"Delegated answer."}"#,
    )]))
    .await;
    let (lead, _) = verified_member(&d.fixture).await;
    let requester = delegated_desk(&d, lead).await;
    let email: String = sqlx::query_scalar("SELECT email FROM users WHERE id=$1")
        .bind(requester)
        .fetch_one(d.p().pool())
        .await
        .unwrap();
    let mut incoming = customer_email(&d, "delegated-admin", None);
    incoming.to = d.address("remote-unit");
    incoming.from = email;
    let ingress = d.email(incoming).await;
    let desk = ingress.thread.as_ref().unwrap().target.desk_id().unwrap();
    let delivery = Delivery::start(&d, ProviderCase::Accept);
    d.run_until(
        "the delegated department's native reply is published and delivered",
        async || responses_published(&d, 1).await && delivered(&d, 1).await,
    )
    .await;
    let sent = delivery.stop().await;
    let rows = finals(&d).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].output, json!({"body":"Delegated answer."}));
    let actor: Uuid =
        sqlx::query_scalar("SELECT actor_id FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(d.fixture.target.company.as_uuid())
            .bind(rows[0].run)
            .fetch_one(d.p().pool())
            .await
            .unwrap();
    assert_eq!(actor, lead);
    assert_eq!(sent.messages().len(), 1);
    assert_eq!(sent.messages()[0].email.body_text, "Delegated answer.");
    let channel = crate::application::ports::channel::ChannelPersistence::get_by_id(d.p(), desk)
        .await
        .unwrap()
        .unwrap();
    let viewer = crate::application::ports::participant::PrincipalAccessPersistence::access_context_for_user(
        d.p(), d.fixture.target.company.as_uuid(), lead,
    ).await.unwrap().unwrap();
    assert!(
        !channel.viewer_access(viewer),
        "configuration authority grants no human conversation access"
    );
    assert_eq!(d.llm.finish().await.unwrap(), 1);
}

#[derive(sqlx::FromRow)]
struct AgentDeskGrant {
    desk: Uuid,
    principal: Uuid,
}

async fn remove_agent_grant(d: &Deployment, capability: &str) {
    let grant: AgentDeskGrant = sqlx::query_as(
        "SELECT channel.id AS desk,principal.id AS principal FROM channels AS channel \
         JOIN addresses AS address ON address.company_id=channel.company_id \
           AND address.channel_id=channel.id AND address.is_primary \
         JOIN principals AS principal ON principal.company_id=channel.company_id AND principal.kind='agent' \
         JOIN agents AS agent ON agent.company_id=principal.company_id AND agent.id=principal.agent_id \
         WHERE channel.company_id=$1 AND address.slug='remote-unit' AND agent.slug='assistant'")
        .bind(d.fixture.target.company.as_uuid()).fetch_one(d.p().pool()).await.unwrap();
    let changed = sqlx::query("DELETE FROM channel_principal_grants WHERE company_id=$1 AND channel_id=$2 AND principal_id=$3 AND capability=$4")
        .bind(d.fixture.target.company.as_uuid()).bind(grant.desk).bind(grant.principal)
        .bind(capability).execute(d.p().pool()).await.unwrap().rows_affected();
    assert!(changed > 0);
}

#[tokio::test]
async fn delegated_admin_native_reply_missing_agent_grant_never_enters_the_provider() {
    for capability in ["view", "participate"] {
        let d = Box::pin(Deployment::new(vec![])).await;
        let (lead, _) = verified_member(&d.fixture).await;
        let requester = delegated_desk(&d, lead).await;
        remove_agent_grant(&d, capability).await;
        let email: String = sqlx::query_scalar("SELECT email FROM users WHERE id=$1")
            .bind(requester)
            .fetch_one(d.p().pool())
            .await
            .unwrap();
        let mut incoming = customer_email(&d, "delegated-revoked", None);
        incoming.to = d.address("remote-unit");
        incoming.from = email;
        d.email(incoming).await;
        d.run_until(
            "the owned reply refuses the revoked agent before a provider call",
            async || {
                d.count("SELECT count(*) FROM workflow_runs WHERE company_id=$1 AND state='failed'")
                    .await
                    == 1
            },
        )
        .await;
        assert_eq!(
            d.count("SELECT count(*) FROM channel_reply_runs WHERE company_id=$1")
                .await,
            1,
            "the test must reach an actual owned automatic run"
        );
        assert_eq!(d.count("SELECT count(*) FROM task_attempts AS attempt JOIN background_tasks AS job ON job.id=attempt.task_id WHERE job.company_id=$1 AND attempt.workflow_failure_code='agent.rejected'").await, 1);
        assert!(finals(&d).await.is_empty());
        assert_eq!(d.llm.finish().await.unwrap(), 0);
    }
}
