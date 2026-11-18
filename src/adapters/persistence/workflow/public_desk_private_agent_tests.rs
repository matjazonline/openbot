//! Production ingest/runtime/final publisher/delivery over a scripted model. Direct address
//! registration comes from real company-agent creation, alongside an explicit shared intake desk.
use super::*;
#[path = "direct_agent_history_teardown_tests.rs"]
mod direct_history_teardown;
#[path = "public_desk_private_agent_smtp_tests.rs"]
pub(in super::super) mod smtp;
#[path = "relay_system_identity_tests.rs"]
mod system_relay;
#[path = "public_desk_private_agent_webhook_tests.rs"]
pub(in super::super) mod webhook;
use crate::entities::{
    address::AddressTarget, transport::PrincipalId, value_objects::EmailAddress,
};

struct PrincipalEndpoint {
    principal: PrincipalId,
    address: EmailAddress,
}

async fn only_desk_thread(d: &Deployment) -> Uuid {
    use crate::adapters::persistence::workflow::association::AssociationColumns;
    let rows: Vec<AssociationColumns> = sqlx::query_as(
        "SELECT channel_id,principal_id,id AS thread_id FROM threads WHERE company_id=$1 LIMIT 2",
    )
    .bind(d.fixture.target.company.as_uuid())
    .fetch_all(d.p().pool())
    .await
    .unwrap();
    assert_eq!(
        rows.len(),
        1,
        "one actual protocol message created one thread"
    );
    assert_eq!(rows[0].channel_id, Some(d.channel));
    assert_eq!(rows[0].principal_id, None);
    rows[0].thread_id.unwrap()
}

async fn private_endpoint(d: &Deployment) -> PrincipalEndpoint {
    let company = d.fixture.target.company.as_uuid();
    let principal = PrincipalId::new(sqlx::query_scalar("SELECT principal.id FROM principals AS principal JOIN agents AS agent ON agent.company_id=principal.company_id AND agent.id=principal.agent_id WHERE principal.company_id=$1 AND agent.slug='assistant' AND NOT principal.open_to_public")
        .bind(company).fetch_one(d.p().pool()).await.unwrap());
    let canonical: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM addresses AS address JOIN channel_bindings AS endpoint \
           ON endpoint.company_id=address.company_id AND endpoint.principal_id=address.principal_id \
         WHERE address.company_id=$1 AND address.principal_id=$2 AND address.slug='assistant' \
           AND endpoint.transport='email' AND endpoint.status='active')",
    ).bind(company).bind(principal.as_uuid()).fetch_one(d.p().pool()).await.unwrap();
    assert!(
        canonical,
        "the production-created agent owns its canonical address and transport endpoint"
    );
    PrincipalEndpoint {
        principal,
        address: d.address("assistant").into(),
    }
}

async fn restricted_desk(d: &Deployment) -> Uuid {
    crate::adapters::persistence::test_support::create_channel(
        d.p(),
        d.fixture.target.company.as_uuid(),
        ChannelWrite {
            name: "Restricted".into(),
            slug: "restricted".into(),
            enabled: true,
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id
}

async fn assert_independent_direct_policy(d: &Deployment, endpoint: &PrincipalEndpoint) {
    let company = d.fixture.target.company.as_uuid();
    let mut direct = customer_email(d, "private-direct", None);
    direct.to = endpoint.address.to_string();
    let refused = d.threads.ingest_test_email(direct).await.unwrap();
    assert!(!refused.accepted);
    let restricted = restricted_desk(d).await;
    let mut desk = customer_email(d, "restricted-before", None);
    desk.to = d.address("restricted");
    assert!(!d.threads.ingest_test_email(desk).await.unwrap().accepted);
    sqlx::query("UPDATE principals SET open_to_public=true WHERE company_id=$1 AND id=$2")
        .bind(company)
        .bind(endpoint.principal.as_uuid())
        .execute(d.p().pool())
        .await
        .unwrap();
    let mut direct = customer_email(d, "opened-direct", None);
    direct.to = endpoint.address.to_string();
    let accepted = d.email(direct).await;
    assert_eq!(
        accepted.thread.unwrap().target,
        AddressTarget::Principal(endpoint.principal)
    );
    let mut desk = customer_email(d, "restricted-after", None);
    desk.to = d.address("restricted");
    assert!(!d.threads.ingest_test_email(desk).await.unwrap().accepted);
    assert_ne!(
        ChannelPersistence::get_by_id(d.p(), restricted)
            .await
            .unwrap()
            .unwrap()
            .access_mode,
        crate::entities::channel::ChannelAccessMode::Public
    );
}

async fn assert_desk_publication(
    d: &Deployment,
    source: Uuid,
    thread: Uuid,
    endpoint: &PrincipalEndpoint,
) {
    #[derive(sqlx::FromRow)]
    struct Reply {
        channel_id: Option<Uuid>,
        principal_id: Option<Uuid>,
        thread_id: Uuid,
        source_target: Value,
        from_address: String,
    }
    let reply: Reply = sqlx::query_as("SELECT filed.channel_id,filed.principal_id,filed.thread_id,part.payload->'body'->'relay'->'source_target' AS source_target,part.payload->'body'->>'from' AS from_address FROM channel_reply_decisions AS decision JOIN channel_reply_runs AS owner ON owner.company_id=decision.company_id AND owner.run_id=decision.run_id JOIN channel_reply_contexts AS context ON context.company_id=owner.company_id AND context.intent_id=owner.intent_id JOIN thread_messages AS filed ON filed.company_id=decision.company_id AND filed.message_id=decision.message_id JOIN message_delivery_parts AS part ON part.company_id=decision.company_id AND part.delivery_id=decision.delivery_id WHERE context.company_id=$1 AND context.message_id=$2")
        .bind(d.fixture.target.company.as_uuid()).bind(source).fetch_one(d.p().pool()).await.unwrap();
    assert_eq!(reply.channel_id, Some(d.channel));
    assert_eq!(reply.principal_id, None);
    assert_eq!(reply.thread_id, thread);
    assert_eq!(reply.source_target, json!(AddressTarget::Desk(d.channel)));
    assert_eq!(reply.from_address, d.address("support"));
    let private: bool = sqlx::query_scalar(
        "SELECT NOT open_to_public FROM principals WHERE company_id=$1 AND id=$2",
    )
    .bind(d.fixture.target.company.as_uuid())
    .bind(endpoint.principal.as_uuid())
    .fetch_one(d.p().pool())
    .await
    .unwrap();
    assert!(
        private,
        "answering desk mail must not open the agent's direct address"
    );
}

pub(in super::super) async fn open_desk(d: &Deployment) {
    ChannelPersistence::update(
        d.p(),
        crate::application::ports::channel::ChannelUpdate {
            actor: d.fixture.target.actor,
            company_id: d.fixture.target.company.as_uuid(),
            channel_id: d.channel,
            write: ChannelWrite {
                name: "Support".into(),
                slug: "support".into(),
                enabled: true,
                participant_emails: Some(vec!["@public".into()]),
                add_3rd_party: true,
                ..Default::default()
            },
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn public_desk_private_agent_real_runtime_replies_on_the_desk_and_keeps_direct_policy_independent()
 {
    let d = Box::pin(Deployment::new(vec![LlmTurn::text(
        r#"{"body":"Desk answer"}"#,
    )]))
    .await;
    let endpoint = private_endpoint(&d).await;
    open_desk(&d).await;
    let mut selected = example(0);
    model(&mut selected);
    Box::pin(bind(&d, selected)).await;
    let accepted = d.email(customer_email(&d, "public-desk", None)).await;
    let source = accepted.inbound_message.unwrap().canonical_id.as_uuid();
    let thread = accepted.thread.unwrap();
    assert_eq!(thread.target, AddressTarget::Desk(d.channel));
    let delivery = Delivery::start(&d, ProviderCase::Accept);
    Box::pin(
        d.run_until("public desk reply from a private agent", async || {
            responses_published(&d, 1).await && delivered(&d, 1).await
        }),
    )
    .await;
    let provider = delivery.stop().await;
    let sent = provider.messages();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].email.body_text, "Desk answer");
    assert_eq!(
        sent[0].email.recipients_to,
        vec![EmailAddress::from(CUSTOMER)]
    );
    assert_eq!(
        sent[0].email.in_reply_to.as_deref(),
        Some("<public-desk@partner.test>")
    );
    assert_desk_publication(&d, source, thread.id, &endpoint).await;
    assert_independent_direct_policy(&d, &endpoint).await;
    assert_eq!(d.llm.finish().await.unwrap(), 1);
}

#[tokio::test]
async fn agent_address_create_actual_default_binding_answers_a_direct_message() {
    let d = Box::pin(answered_default(vec![LlmTurn::text(
        r#"{"body":"A direct production reply"}"#,
    )]))
    .await;
    assert_eq!(d.llm.finish().await.unwrap(), 1);
}

async fn answered_default(turns: Vec<LlmTurn>) -> Deployment {
    let d = Deployment::new(turns).await;
    // The shared fixture permits unrelated snapshot-corruption tests. This runtime proof
    // exercises the production run guard as well as the direct-target lifecycle constraints.
    sqlx::query("ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable")
        .execute(d.p().pool())
        .await
        .unwrap();
    let company = d.fixture.target.company.as_uuid();
    let principal: Uuid = sqlx::query_scalar("SELECT principal.id FROM principals AS principal JOIN agents AS agent ON agent.company_id=principal.company_id AND agent.id=principal.agent_id WHERE principal.company_id=$1 AND agent.slug='assistant'")
        .bind(company).fetch_one(d.p().pool()).await.unwrap();
    let owner: String = sqlx::query_scalar("SELECT email::text FROM users WHERE id=$1")
        .bind(d.fixture.target.actor.user_id())
        .fetch_one(d.p().pool())
        .await
        .unwrap();
    let accepted = d
        .email(RawInboundPayload {
            from: owner,
            to: d.address("assistant"),
            subject: Some("Direct production request".into()),
            text: Some("Please answer on your own address".into()),
            headers: Some(format!("Message-ID: <{}@sender.test>\n", Uuid::new_v4())),
            ..Default::default()
        })
        .await;
    assert_eq!(
        accepted.thread.unwrap().target,
        crate::entities::address::AddressTarget::Principal(
            crate::entities::transport::PrincipalId::new(principal)
        )
    );
    let delivery = Delivery::start(&d, ProviderCase::Accept);
    d.run_until("the production-created agent's direct final was delivered",async || {
        d.count("SELECT count(*) FROM message_deliveries WHERE company_id=$1 AND purpose='workflow_reply' AND status='delivered'").await==1
    }).await;
    let exact:bool=sqlx::query_scalar(
        "SELECT count(*)=1 AND bool_and(run.channel_id IS NULL AND run.principal_id=$2 \
         AND run.binding_id=selected.binding_id AND run.binding_revision=selected.binding_revision \
         AND message.author_principal_id=$2 AND message.role='agent' AND message.direction='outbound') \
         FROM workflow_runs AS run JOIN channel_reply_settings AS selected \
           ON selected.company_id=run.company_id AND selected.principal_id=run.principal_id \
         JOIN channel_reply_decisions AS final ON final.company_id=run.company_id AND final.run_id=run.id AND final.state='published' \
         JOIN messages AS message ON message.company_id=final.company_id AND message.id=final.message_id \
         WHERE run.company_id=$1")
        .bind(company).bind(principal).fetch_one(d.p().pool()).await.unwrap();
    assert!(
        exact,
        "one actual default binding/run/final retains the agent principal as author"
    );
    let provider = delivery.stop().await;
    assert_eq!(provider.messages().len(), 1);
    d
}
