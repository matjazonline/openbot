//! Company ownership must remove populated direct descendants without touching another tenant.
use super::*;
use crate::application::ports::thread::ThreadPersistence;
use crate::entities::internal_note::{AddInternalNote, InternalNoteProvenance};

const DESCENDANTS: &[&str] = &[
    "principals",
    "addresses",
    "channel_bindings",
    "threads",
    "thread_messages",
    "thread_principals",
    "messages",
    "external_threads",
    "external_messages",
    "workflow_message_intents",
    "workflow_runs",
    "agent_history_sessions",
    "agent_history_conversations",
    "agent_history_exchanges",
    "internal_notes",
    "message_deliveries",
];

async fn rows(d: &Deployment, company: Uuid) -> Value {
    let mut result = serde_json::Map::new();
    for table in DESCENDANTS {
        let value: Value = sqlx::query_scalar(&format!(
            "SELECT COALESCE(jsonb_agg(to_jsonb(record) ORDER BY to_jsonb(record)::text),'[]'::jsonb) \
             FROM {table} AS record WHERE record.company_id=$1"
        )).bind(company).fetch_one(d.p().pool()).await.unwrap();
        result.insert((*table).into(), value);
    }
    result.into()
}

async fn note(d: &Deployment, thread: &crate::entities::thread::Thread) {
    let actor = d
        .threads
        .principal_access_for_user(thread.company_id, d.fixture.target.actor.user_id())
        .await
        .unwrap()
        .unwrap();
    d.threads
        .add_internal_note(
            AddInternalNote {
                company_id: thread.company_id,
                target: thread.target,
                thread_id: thread.id,
                text: "A retained direct note".into(),
                command_id: Uuid::new_v4(),
                supersedes_note_id: None,
                provenance: InternalNoteProvenance::HumanUi,
            },
            actor,
        )
        .await
        .unwrap();
}

async fn populated(d: &Deployment, thread: &crate::entities::thread::Thread) -> Value {
    let before = rows(d, thread.company_id).await;
    for table in DESCENDANTS {
        assert!(
            !before[*table].as_array().unwrap().is_empty(),
            "must populate {table} before teardown"
        );
    }
    for table in [
        "agent_history_sessions",
        "agent_history_conversations",
        "internal_notes",
    ] {
        let direct: bool = sqlx::query_scalar(&format!(
            "SELECT count(*)>0 AND bool_and(channel_id IS NULL AND principal_id=$2) FROM {table} WHERE company_id=$1"
        )).bind(thread.company_id).bind(thread.target.principal_id().unwrap().as_uuid())
            .fetch_one(d.p().pool()).await.unwrap();
        assert!(direct, "{table} must pin the actual direct principal");
    }
    before
}

async fn configured_survivor(d: &Deployment) -> crate::entities::company::Company {
    let owner = d.fixture.target.actor;
    let company = CompanyPersistence::create(
        d.p(),
        owner.user_id(),
        CompanyWrite {
            name: "Surviving tenant".into(),
            slug: format!("survivor-{}", Uuid::new_v4().simple()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    CompanyPersistence::replace_model_connections_for_user(
        d.p(),
        owner.user_id(),
        company.id,
        vec![CompanyModelConnectionWrite {
            provider: SCRIPTED_PROVIDER.into(),
            api_key: Some("scripted-key".into()),
            models: vec![SCRIPTED_MODEL.into()],
            is_default: true,
        }],
    )
    .await
    .unwrap();
    let service = crate::adapters::persistence::test_support::company_agents(d.p());
    let created = Box::pin(service.create(
        owner,
        CompanyId::new(company.id),
        AgentWrite {
            name: "Surviving assistant".into(),
            slug: "assistant".into(),
            provider: Some(SCRIPTED_PROVIDER.into()),
            model: Some(SCRIPTED_MODEL.into()),
            ..Default::default()
        },
        None,
    ))
    .await
    .unwrap();
    register_scripted_agent_base_url(created.agent.id, &d.llm.base_url);
    company
}

async fn answered_other(d: &Deployment) -> crate::entities::thread::Thread {
    let other = Box::pin(configured_survivor(d)).await;
    let owner: String = sqlx::query_scalar("SELECT email::text FROM users WHERE id=$1")
        .bind(d.fixture.target.actor.user_id())
        .fetch_one(d.p().pool())
        .await
        .unwrap();
    let accepted = d
        .email(RawInboundPayload {
            from: owner,
            to: format!("assistant@{}.{APP_DOMAIN}", other.slug),
            subject: Some("Surviving direct work".into()),
            text: Some("Please answer this direct message".into()),
            headers: Some(format!("Message-ID: <{}@sender.test>\n", Uuid::new_v4())),
            ..Default::default()
        })
        .await;
    let delivery = Delivery::start(d, ProviderCase::Accept);
    d.run_until("the surviving tenant has populated direct history and a delivered outbox", async || {
        sqlx::query_scalar::<_,i64>("SELECT count(*) FROM message_deliveries WHERE company_id=$1 AND purpose='workflow_reply' AND status='delivered'")
            .bind(other.id).fetch_one(d.p().pool()).await.unwrap()==1
    }).await;
    assert_eq!(delivery.stop().await.messages().len(), 1);
    accepted.thread.unwrap()
}

#[tokio::test]
async fn direct_company_teardown_removes_populated_agent_history_notes_and_transport() {
    // Both production agents run against the same native database and the scripted provider;
    // boxed fixtures keep the real agent/runtime boundary inside the stock stack budget.
    let d = Box::pin(answered_default(vec![
        LlmTurn::text(r#"{"body":"A direct production reply"}"#),
        LlmTurn::text(r#"{"body":"A surviving direct production reply"}"#),
    ]))
    .await;
    let company = d.fixture.target.company.as_uuid();
    let thread_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM threads WHERE company_id=$1 AND principal_id IS NOT NULL",
    )
    .bind(company)
    .fetch_one(d.p().pool())
    .await
    .unwrap();
    let thread = ThreadPersistence::get_thread_by_id(d.p(), company, thread_id)
        .await
        .unwrap()
        .unwrap();
    let other_thread = Box::pin(answered_other(&d)).await;
    note(&d, &thread).await;
    note(&d, &other_thread).await;
    populated(&d, &thread).await;
    let surviving = populated(&d, &other_thread).await;
    CompanyPersistence::delete(d.p(), company).await.unwrap();
    let after = rows(&d, company).await;
    for table in DESCENDANTS {
        assert_eq!(after[*table], json!([]), "leftover {table}");
    }
    assert_eq!(rows(&d, other_thread.company_id).await, surviving);
    assert_eq!(d.llm.finish().await.unwrap(), 2);
}
