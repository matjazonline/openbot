//! Each control insert succeeds before the corresponding foreign resource is substituted.
use super::*;
use crate::use_cases::company::{CompanyPersistence, CompanyWrite};

async fn assert_fk_case(
    pool: &PgPool,
    table: &'static str,
    valid: Value,
    invalid: Value,
    constraint: &'static str,
) {
    // Table identifiers are fixed test constants; all row contents remain bound parameters.
    let insert = format!("INSERT INTO {table} SELECT (jsonb_populate_record(NULL::{table},$1)).*");
    let mut tx = pool.begin().await.unwrap();
    sqlx::query(&insert)
        .bind(valid)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    let error = sqlx::query(&insert)
        .bind(invalid)
        .execute(pool)
        .await
        .unwrap_err();
    let error = error.as_database_error().unwrap();
    assert_eq!(error.code().as_deref(), Some("23503"), "{error}");
    assert_eq!(error.constraint(), Some(constraint), "{error}");
}

async fn row(pool: &PgPool, table: &'static str, id: Uuid) -> Value {
    sqlx::query_scalar(&format!(
        "SELECT to_jsonb(stored) FROM {table} AS stored WHERE id=$1"
    ))
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap()
}

fn cloned_row(mut value: Value) -> Value {
    value["id"] = json!(Uuid::new_v4());
    value
}

#[tokio::test]
async fn cloudroom_company_machine_reference_constraints_reject_foreign_rows() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let paired = f.pair(f.company).await;
    let foreign = f.pair(f.foreign).await;
    f.install_catalog(&paired).await;
    f.install_catalog(&foreign).await;
    let own_machine = paired.receipt.machine.id.as_uuid();
    let foreign_machine = foreign.receipt.machine.id.as_uuid();
    let own_install = paired.receipt.machine.installation_id.as_uuid();
    let foreign_install = foreign.receipt.machine.installation_id.as_uuid();
    let pool = &f.database.pool;
    assert_installation_constraint(&f, &paired).await;
    let credential = json!({"company_id":f.company,"machine_id":own_machine,"generation":2,
        "verifier":format!("\\x{}", "ab".repeat(32)),"created_at":Utc::now(),"revoked_at":null});
    let mut wrong = credential.clone();
    wrong["machine_id"] = json!(foreign_machine);
    assert_fk_case(
        pool,
        "local_machine_credentials",
        credential,
        wrong,
        "local_machine_credentials_company_id_machine_id_fkey",
    )
    .await;
    let capability = json!({"company_id":f.company,"machine_id":own_machine,"installation_id":own_install,
        "sequence":2,"observed_at":Utc::now(),"catalog":{}});
    let mut wrong = capability.clone();
    wrong["machine_id"] = json!(foreign_machine);
    wrong["installation_id"] = json!(foreign_install);
    assert_fk_case(
        pool,
        "local_machine_capabilities",
        capability,
        wrong,
        "local_machine_capabilities_company_id_machine_id_installat_fkey",
    )
    .await;
    let project =
        json!({"company_id":f.company,"machine_id":own_machine,"project_id":"proj_matrix"});
    let mut wrong = project.clone();
    wrong["machine_id"] = json!(foreign_machine);
    assert_fk_case(
        pool,
        "local_machine_projects",
        project,
        wrong,
        "local_machine_projects_company_id_machine_id_fkey",
    )
    .await;
    let mut pairing = cloned_row(sqlx::query_scalar::<_,Value>("SELECT to_jsonb(pairing) FROM local_pairings AS pairing WHERE company_id=$1 AND machine_id=$2")
        .bind(f.company).bind(own_machine).fetch_one(pool).await.unwrap());
    pairing["code_hash"] = json!(format!("\\x{}", "ac".repeat(32)));
    pairing["claim_verifier"] = json!(format!("\\x{}", "ad".repeat(32)));
    let mut wrong = pairing.clone();
    wrong["machine_id"] = json!(foreign_machine);
    wrong["installation_id"] = json!(foreign_install);
    assert_fk_case(
        pool,
        "local_pairings",
        pairing,
        wrong,
        "local_pairings_company_id_machine_id_installation_id_fkey",
    )
    .await;
    assert_capability_pointer_constraint(&f, &paired, &foreign).await;
}

async fn assert_installation_constraint(f: &Fixture, paired: &Paired) {
    let own_install = Uuid::new_v4();
    let foreign_install = Uuid::new_v4();
    for (company, installation) in [(f.company, own_install), (f.foreign, foreign_install)] {
        sqlx::query("INSERT INTO local_installations(company_id,installation_id) VALUES($1,$2)")
            .bind(company)
            .bind(installation)
            .execute(&f.database.pool)
            .await
            .unwrap();
    }
    let mut valid = cloned_row(
        row(
            &f.database.pool,
            "local_machines",
            paired.receipt.machine.id.as_uuid(),
        )
        .await,
    );
    valid["installation_id"] = json!(own_install);
    valid["capabilities_sequence"] = Value::Null;
    let mut invalid = valid.clone();
    invalid["installation_id"] = json!(foreign_install);
    assert_fk_case(
        &f.database.pool,
        "local_machines",
        valid,
        invalid,
        "local_machines_company_id_installation_id_fkey",
    )
    .await;
}

async fn assert_capability_pointer_constraint(f: &Fixture, own: &Paired, foreign: &Paired) {
    // Sequence 2 exists, but only for the other tenant's actual installation.
    sqlx::query("INSERT INTO local_machine_capabilities(company_id,machine_id,installation_id,sequence,catalog) VALUES($1,$2,$3,2,'{}')")
        .bind(f.foreign).bind(foreign.receipt.machine.id.as_uuid()).bind(foreign.receipt.machine.installation_id.as_uuid())
        .execute(&f.database.pool).await.unwrap();
    let mut tx = f.database.pool.begin().await.unwrap();
    sqlx::query("UPDATE local_machines SET capabilities_sequence=1 WHERE company_id=$1 AND id=$2")
        .bind(f.company)
        .bind(own.receipt.machine.id.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    let mut tx = f.database.pool.begin().await.unwrap();
    sqlx::query("UPDATE local_machines SET capabilities_sequence=2 WHERE company_id=$1 AND id=$2")
        .bind(f.company)
        .bind(own.receipt.machine.id.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    let error = tx.commit().await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23503")
    );
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("local_machine_capability_reference")
    );
}

async fn foreign_session(f: &Fixture, foreign: &Paired) -> SessionId {
    let mut profile = f.profile(foreign.receipt.machine.id);
    profile.company_id = f.foreign;
    sqlx::query("INSERT INTO local_agent_profiles(company_id,id,machine_id,name,selection) VALUES($1,$2,$3,$4,$5)")
        .bind(profile.company_id).bind(profile.id.as_uuid()).bind(foreign.receipt.machine.id.as_uuid())
        .bind(&profile.name).bind(encode(&profile.selection).unwrap()).execute(&f.database.pool).await.unwrap();
    let session = SessionId::new(Uuid::new_v4()).unwrap();
    let conversation = f.conversation(f.foreign).await;
    sqlx::query("INSERT INTO local_agent_sessions(company_id,id,machine_id,installation_id,profile_id,selection,conversation_id) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(f.foreign).bind(session.as_uuid()).bind(foreign.receipt.machine.id.as_uuid())
        .bind(foreign.receipt.machine.installation_id.as_uuid()).bind(profile.id.as_uuid())
        .bind(encode(&profile.selection).unwrap()).bind(conversation).execute(&f.database.pool).await.unwrap();
    session
}

#[tokio::test]
async fn cloudroom_company_session_operation_constraints_reject_foreign_rows() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let paired = f.pair(f.company).await;
    let foreign = f.pair(f.foreign).await;
    let setup = prepare_session(&f, &paired).await;
    let foreign_session = foreign_session(&f, &foreign).await;
    let pool = &f.database.pool;
    let mut session = cloned_row(row(pool, "local_agent_sessions", setup.id.as_uuid()).await);
    session["thread_id"] = Value::Null;
    let mut wrong = session.clone();
    wrong["machine_id"] = json!(foreign.receipt.machine.id);
    wrong["installation_id"] = json!(foreign.receipt.machine.installation_id);
    wrong["selection"]["machine_id"] = json!(foreign.receipt.machine.id);
    assert_fk_case(
        pool,
        "local_agent_sessions",
        session.clone(),
        wrong,
        "local_agent_sessions_company_id_machine_id_installation_id_fkey",
    )
    .await;
    let foreign_record = row(pool, "local_agent_sessions", foreign_session.as_uuid()).await;
    let mut wrong = session.clone();
    wrong["conversation_id"] = foreign_record["conversation_id"].clone();
    assert_fk_case(
        pool,
        "local_agent_sessions",
        session.clone(),
        wrong,
        "local_agent_sessions_company_id_conversation_id_fkey",
    )
    .await;
    let mut wrong = session.clone();
    wrong["profile_id"] = foreign_record["profile_id"].clone();
    assert_fk_case(
        pool,
        "local_agent_sessions",
        session,
        wrong,
        "local_agent_sessions_company_id_profile_id_fkey",
    )
    .await;
    assert_operation_session_constraint(&f, &paired, setup.id, foreign_session).await;
    assert_operation_machine_constraint(&f, &paired, &foreign).await;
    assert_observation_constraint(&f, &paired, &foreign).await;
}

async fn assert_operation_session_constraint(
    f: &Fixture,
    paired: &Paired,
    session: SessionId,
    foreign_session: SessionId,
) {
    let pool = &f.database.pool;
    let operation = seed_operation(
        f,
        paired,
        LocalCommand::Send {
            session_id: session,
            thread_id: CloudroomThreadId::try_from("thr_frozen".to_owned()).unwrap(),
            prompt: LocalPrompt::try_from("constraint proof".to_owned()).unwrap(),
        },
        Some(session),
        DispatchEvidence::NotDispatched {},
    )
    .await;
    let mut valid = cloned_row(row(pool, "local_agent_operations", operation.as_uuid()).await);
    valid["logical_key"] = valid["id"].clone();
    let mut invalid = valid.clone();
    invalid["session_id"] = json!(foreign_session);
    let command = LocalCommand::Send {
        session_id: foreign_session,
        thread_id: CloudroomThreadId::try_from("thr_frozen".to_owned()).unwrap(),
        prompt: LocalPrompt::try_from("constraint proof".to_owned()).unwrap(),
    };
    let changed = LocalAgentOperation::queued(
        OperationId::new(Uuid::new_v4()).unwrap(),
        command,
        Utc::now() + chrono::Duration::minutes(30),
    )
    .unwrap();
    invalid["command"] = encode(changed.command()).unwrap();
    invalid["payload_digest"] = json!(changed.payload_digest());
    invalid["deadline"] = json!(changed.deadline());
    assert_fk_case(
        pool,
        "local_agent_operations",
        valid,
        invalid,
        "local_agent_operations_company_id_machine_id_installation_fkey1",
    )
    .await;
}

async fn assert_operation_machine_constraint(f: &Fixture, own: &Paired, foreign: &Paired) {
    let id = seed_operation(
        f,
        own,
        LocalCommand::RefreshCatalog {},
        None,
        DispatchEvidence::NotDispatched {},
    )
    .await;
    let mut valid = cloned_row(row(&f.database.pool, "local_agent_operations", id.as_uuid()).await);
    valid["logical_key"] = valid["id"].clone();
    let mut invalid = valid.clone();
    invalid["machine_id"] = json!(foreign.receipt.machine.id);
    invalid["installation_id"] = json!(foreign.receipt.machine.installation_id);
    assert_fk_case(
        &f.database.pool,
        "local_agent_operations",
        valid,
        invalid,
        "local_agent_operations_company_id_machine_id_installation__fkey",
    )
    .await;
}

async fn assert_observation_constraint(f: &Fixture, own: &Paired, foreign: &Paired) {
    let own_op = seed_operation(
        f,
        own,
        LocalCommand::RefreshCatalog {},
        None,
        DispatchEvidence::NotDispatched {},
    )
    .await;
    let foreign_op = seed_operation(
        f,
        foreign,
        LocalCommand::RefreshCatalog {},
        None,
        DispatchEvidence::NotDispatched {},
    )
    .await;
    let valid = json!({"company_id":f.company,"machine_id":own.receipt.machine.id,
        "installation_id":own.receipt.machine.installation_id,"sequence":1,"operation_id":own_op,
        "digest":"0".repeat(64),"observation":{},"accepted_at":Utc::now()});
    let mut invalid = valid.clone();
    invalid["operation_id"] = json!(foreign_op);
    assert_fk_case(
        &f.database.pool,
        "local_agent_observations",
        valid,
        invalid,
        "local_agent_observations_company_id_machine_id_installatio_fkey",
    )
    .await;
}

#[tokio::test]
async fn cloudroom_company_workflow_session_constraint_rejects_existing_foreign_run() {
    // This existing fixture admits and executes a real HTTP workflow in an owned database.
    let workflow =
        crate::adapters::persistence::workflow::reconciliation_fixture::accepted_http().await;
    let pool = workflow.persistence.pool();
    let foreign_company = CompanyPersistence::create(
        &workflow.persistence,
        workflow.owner,
        CompanyWrite {
            name: "Local association constraint".into(),
            slug: format!("local-fk-{}", Uuid::new_v4().simple()),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let own = workflow_session_row(pool, workflow.company, workflow.run).await;
    let mut foreign = workflow_session_row(pool, foreign_company, workflow.run).await;
    // The foreign-machine/profile side is valid; only its workflow association crosses tenants.
    foreign["workflow_run_id"] = json!(workflow.run);
    assert_fk_case(
        pool,
        "local_agent_sessions",
        own,
        foreign,
        "local_agent_sessions_company_id_workflow_run_id_fkey",
    )
    .await;
}

async fn workflow_session_row(pool: &PgPool, company: Uuid, run: Uuid) -> Value {
    let machine = Uuid::new_v4();
    let installation = Uuid::new_v4();
    let profile = Uuid::new_v4();
    let selection = json!({"machine_id":machine,"project_id":"proj_test","provider":"codex","model":"gpt-6-luna",
        "policy":{"workspace":"project_directory","permissions":"provider_default","reasoning":"low"}});
    sqlx::query("INSERT INTO local_installations(company_id,installation_id) VALUES($1,$2)")
        .bind(company)
        .bind(installation)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO local_machines(company_id,id,installation_id,name) VALUES($1,$2,$3,'workflow constraint')")
        .bind(company).bind(machine).bind(installation).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO local_agent_profiles(company_id,id,machine_id,name,selection) VALUES($1,$2,$3,'workflow constraint',$4)")
        .bind(company).bind(profile).bind(machine).bind(&selection).execute(pool).await.unwrap();
    json!({"company_id":company,"id":Uuid::new_v4(),"machine_id":machine,"installation_id":installation,
        "profile_id":profile,"selection":selection,"conversation_id":null,"workflow_run_id":run,
        "thread_id":null,"created_at":Utc::now()})
}
