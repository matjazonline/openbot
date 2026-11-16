//! Structural FK controls accompany, rather than replace, actual native-origin teardown.
use super::{FkExpectation, PostgresPersistence, finish_fk_probe, foreign_company};
use crate::{
    application::ports::{channel::*, thread::ThreadPersistence},
    domain::workflow::CompanyId,
    entities::address::AddressTarget,
};
use serde_json::{Value, json};
use uuid::Uuid;

const PROFILE: &str = "local_agent_sessions_company_id_profile_id_fkey";
const CONVERSATION: &str = "local_agent_sessions_company_id_conversation_id_fkey";
const WORKFLOW: &str = "local_agent_sessions_company_id_workflow_run_id_fkey";
const OPERATION_SESSION: &str = "local_agent_operations_company_id_machine_id_installation_fkey1";

#[derive(Clone, sqlx::FromRow)]
struct Session {
    company_id: Uuid,
    machine_id: Uuid,
    installation_id: Uuid,
    profile_id: Uuid,
    selection: Value,
    conversation_id: Option<Uuid>,
    workflow_run_id: Option<Uuid>,
}

async fn insert_session(db: &mut sqlx::PgConnection, session: &Session) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO local_agent_sessions(id,company_id,machine_id,installation_id,profile_id,selection,conversation_id,workflow_run_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(id).bind(session.company_id).bind(session.machine_id).bind(session.installation_id)
        .bind(session.profile_id).bind(&session.selection).bind(session.conversation_id)
        .bind(session.workflow_run_id).execute(db).await.unwrap();
    id
}

async fn foreign_session(p: &PostgresPersistence, owner: Uuid) -> (Session, Uuid) {
    let company = foreign_company(p, owner).await;
    let machine = Uuid::new_v4();
    let installation = Uuid::new_v4();
    let profile = Uuid::new_v4();
    let selection = json!({"machine_id":machine});
    let mut tx = p.pool().begin().await.unwrap();
    sqlx::query("INSERT INTO local_installations(installation_id,company_id) VALUES($1,$2)")
        .bind(installation)
        .bind(company.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO local_machines(id,company_id,installation_id,name) VALUES($1,$2,$3,'Foreign FK control')")
        .bind(machine).bind(company.as_uuid()).bind(installation).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO local_agent_profiles(id,company_id,machine_id,name,selection) VALUES($1,$2,$3,'Foreign FK control',$4)")
        .bind(profile).bind(company.as_uuid()).bind(machine).bind(&selection).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let channel = ChannelPersistence::create(
        p,
        company.as_uuid(),
        ChannelWrite {
            name: "Foreign conversation".into(),
            slug: "foreign-conversation".into(),
            enabled: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let conversation = p
        .create_thread(
            company.as_uuid(),
            AddressTarget::Desk(channel.id),
            "Foreign FK control",
            &[],
        )
        .await
        .unwrap();
    let foreign = Session {
        company_id: company.as_uuid(),
        machine_id: machine,
        installation_id: installation,
        profile_id: profile,
        selection,
        conversation_id: Some(conversation.id),
        workflow_run_id: None,
    };
    let mut tx = p.pool().begin().await.unwrap();
    let id = insert_session(&mut tx, &foreign).await;
    tx.commit().await.unwrap();
    (foreign, id)
}

async fn probe_session(p: &PostgresPersistence, session: Session, expected: FkExpectation) {
    let mut tx = p.pool().begin().await.unwrap();
    let id = insert_session(&mut tx, &session).await;
    finish_fk_probe(tx, expected).await;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM local_agent_sessions WHERE company_id=$1 AND id=$2)",
    )
    .bind(session.company_id)
    .bind(id)
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert!(!exists, "each FK probe rolls back its entire row");
}

async fn probe_operation(
    p: &PostgresPersistence,
    context: &Session,
    session: Uuid,
    expected: FkExpectation,
) {
    let mut tx = p.pool().begin().await.unwrap();
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO local_agent_operations(id,company_id,machine_id,installation_id,session_id,logical_key,origin,command,payload_digest,deadline) VALUES($1,$2,$3,$4,$5,$6,'direct',$7,repeat('0',64),clock_timestamp()+interval '1 minute')")
        .bind(id).bind(context.company_id).bind(context.machine_id).bind(context.installation_id)
        .bind(session).bind(format!("fk-control-{id}"))
        .bind(json!({"kind":"inspect","session_id":session})).execute(&mut *tx).await.unwrap();
    finish_fk_probe(tx, expected).await;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM local_agent_operations WHERE company_id=$1 AND id=$2)",
    )
    .bind(context.company_id)
    .bind(id)
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert!(!exists);
}

async fn session_edges(p: &PostgresPersistence, original: &Session, foreign: &Session, run: Uuid) {
    probe_session(p, original.clone(), FkExpectation::Valid).await;
    let mut invalid = original.clone();
    invalid.profile_id = Uuid::new_v4();
    probe_session(p, invalid, FkExpectation::Refused(PROFILE)).await;
    let mut invalid = original.clone();
    invalid.profile_id = foreign.profile_id;
    probe_session(p, invalid, FkExpectation::Refused(PROFILE)).await;
    let mut invalid = original.clone();
    invalid.workflow_run_id = None;
    invalid.conversation_id = Some(Uuid::new_v4());
    probe_session(p, invalid, FkExpectation::Refused(CONVERSATION)).await;
    let mut invalid = original.clone();
    invalid.workflow_run_id = None;
    invalid.conversation_id = foreign.conversation_id;
    probe_session(p, invalid, FkExpectation::Refused(CONVERSATION)).await;
    let mut valid = original.clone();
    valid.conversation_id = None;
    valid.workflow_run_id = Some(run);
    probe_session(p, valid.clone(), FkExpectation::Valid).await;
    valid.workflow_run_id = Some(Uuid::new_v4());
    probe_session(p, valid, FkExpectation::Refused(WORKFLOW)).await;
    let mut invalid = foreign.clone();
    invalid.conversation_id = None;
    invalid.workflow_run_id = Some(run);
    probe_session(p, invalid, FkExpectation::Refused(WORKFLOW)).await;
}

pub(crate) async fn assert_relationships(
    p: &PostgresPersistence,
    company: CompanyId,
    owner: Uuid,
) -> CompanyId {
    let (session_id,): (Uuid,) = sqlx::query_as(
        "SELECT id FROM local_agent_sessions WHERE company_id=$1 ORDER BY id LIMIT 1",
    )
    .bind(company.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    let original: Session = sqlx::query_as("SELECT company_id,machine_id,installation_id,profile_id,selection,conversation_id,workflow_run_id FROM local_agent_sessions WHERE company_id=$1 AND id=$2")
        .bind(company.as_uuid()).bind(session_id).fetch_one(p.pool()).await.unwrap();
    let run =
        sqlx::query_scalar("SELECT id FROM workflow_runs WHERE company_id=$1 ORDER BY id LIMIT 1")
            .bind(company.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    let (foreign, foreign_id) = foreign_session(p, owner).await;
    session_edges(p, &original, &foreign, run).await;
    probe_operation(p, &original, session_id, FkExpectation::Valid).await;
    probe_operation(
        p,
        &original,
        Uuid::new_v4(),
        FkExpectation::Refused(OPERATION_SESSION),
    )
    .await;
    probe_operation(
        p,
        &original,
        foreign_id,
        FkExpectation::Refused(OPERATION_SESSION),
    )
    .await;
    let retained: Uuid =
        sqlx::query_scalar("SELECT id FROM local_agent_sessions WHERE company_id=$1 AND id=$2")
            .bind(company.as_uuid())
            .bind(session_id)
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(retained, session_id);
    CompanyId::new(foreign.company_id)
}
