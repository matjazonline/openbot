//! Valid legacy auxiliary owners; fixed table/column names only.
use super::*;

pub(super) const OWNERS: [&str; 11] = [
    "task_outreaches",
    "task_harness_runs",
    "task_agent_instructions",
    "start_agent_task_commands",
    "task_channel_targets",
    "task_ownership_events",
    "task_status_events",
    "delegation_control_commands",
    "human_task_completions",
    "task_approval_waits",
    "thread_handoff_runs",
];

pub(super) async fn insert_record(
    db: &mut PgConnection,
    table: &str,
    record: &serde_json::Value,
) -> Result<(), sqlx::Error> {
    let columns = record
        .as_object()
        .unwrap()
        .keys()
        .map(|key| format!("\"{key}\""))
        .collect::<Vec<_>>()
        .join(", ");
    sqlx::query(&format!("INSERT INTO {table} ({columns}) SELECT {columns} FROM jsonb_populate_record(NULL::{table}, $1)"))
        .bind(record).execute(db).await?;
    Ok(())
}

pub(super) async fn seed(f: &JobFixture, db: &mut PgConnection, task: Uuid) {
    let company = f.binding.target.company.as_uuid();
    let worker = agent(f, db).await;
    let principal: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 AND agent_id = $2")
            .bind(company)
            .bind(worker)
            .fetch_one(&mut *db)
            .await
            .unwrap();
    let thread = Uuid::new_v4();
    insert_record(
        db,
        "threads",
        &json!({"id":thread,"company_id":company,"channel_id":f.channel,"subject":"Auxiliary"}),
    )
    .await
    .unwrap();
    let sources = notification_fixtures::auxiliary_sources(f, db, Some(task)).await;
    let message: Uuid =
        sqlx::query_scalar("SELECT message_id FROM message_deliveries WHERE id = $1")
            .bind(sources[2])
            .fetch_one(&mut *db)
            .await
            .unwrap();
    let outreach = outreach(f, db, task).await;
    seed_status_sources(db, company, task, sources[0], outreach).await;
    seed_outreach_descendants(db, company, f.channel, thread, message, outreach).await;
    seed_harness(db, company, task, worker, principal).await;
    insert_record(
        db,
        "task_agent_instructions",
        &json!({"id":Uuid::new_v4(),"company_id":company,
        "channel_id":f.channel,"thread_id":thread,"task_id":task,"command_id":Uuid::new_v4(),
        "command_fingerprint":"test","requested_by_principal_id":principal,
        "requested_ownership_version":1,"wake_outcome":"queued"}),
    )
    .await
    .unwrap();
    insert_record(
        db,
        "start_agent_task_commands",
        &json!({"company_id":company,"task_id":task,
        "command_id":Uuid::new_v4(),"command_fingerprint":"test"}),
    )
    .await
    .unwrap();
    insert_record(
        db,
        "task_channel_targets",
        &json!({"company_id":company,"task_id":task,
        "channel_id":f.channel,"thread_id":thread,"recipient_role":"to","position":0}),
    )
    .await
    .unwrap();
    insert_record(
        db,
        "delegation_control_commands",
        &json!({"company_id":company,"task_id":task,
        "outreach_id":outreach,"command_id":Uuid::new_v4(),"command_fingerprint":"test",
        "operation":"cancel_outreach","actor_principal_id":principal,"actor_kind":"agent",
        "authority":"owning_agent","reason":"no_longer_needed","from_version":1,"to_version":2,
        "result":{"version":1}}),
    )
    .await
    .unwrap();
    insert_record(
        db,
        "human_task_completions",
        &json!({"company_id":company,"task_id":task,
        "command_id":Uuid::new_v4(),"command_fingerprint":"test","owner_principal_id":principal,
        "ownership_version":1,"message_id":message}),
    )
    .await
    .unwrap();
    insert_record(db,"task_approval_waits", &json!({"company_id":company,"task_id":task,
        "approval_id":sources[0],"cycle_id":Uuid::new_v4(),"ownership_version":1,"state":"waiting"})).await.unwrap();
    let handoff = Uuid::new_v4();
    let generation = Uuid::new_v4();
    insert_record(db,"thread_handoffs", &json!({"id":handoff,"company_id":company,
        "channel_id":f.channel,"thread_id":thread,"generation":generation,"source_message_id":message})).await.unwrap();
    insert_record(db,"thread_handoff_runs", &json!({"company_id":company,"task_id":task,
        "handoff_id":handoff,"generation":generation,"requested_by_principal_id":principal,"command_id":Uuid::new_v4()})).await.unwrap();
}

async fn seed_outreach_descendants(
    db: &mut PgConnection,
    company: Uuid,
    channel: Uuid,
    thread: Uuid,
    message: Uuid,
    outreach: Uuid,
) {
    let association = Uuid::new_v4();
    insert_record(
        db,
        "thread_messages",
        &json!({"id":association,"company_id":company,
        "channel_id":channel,"thread_id":thread,"message_id":message,"entry_kind":"delegation"}),
    )
    .await
    .unwrap();
    let target = Uuid::new_v4();
    insert_record(
        db,
        "task_outreach_targets",
        &json!({"id":target,"company_id":company,
        "outreach_id":outreach,"email":"reply@example.test","external_transport":"email",
        "external_namespace":"email","external_subject":"reply@example.test"}),
    )
    .await
    .unwrap();
    insert_record(
        db,
        "task_outreach_replies",
        &json!({"company_id":company,"outreach_id":outreach,
        "target_id":target,"response_association_id":association,"disposition":"late"}),
    )
    .await
    .unwrap();
}

async fn seed_status_sources(
    db: &mut PgConnection,
    company: Uuid,
    task: Uuid,
    approval: Uuid,
    outreach: Uuid,
) {
    sqlx::query("UPDATE task_status_events SET related_approval_id = $2 WHERE task_id = $1")
        .bind(task)
        .bind(approval)
        .execute(&mut *db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO task_status_events (id,company_id,task_id,correlation_id,sequence,to_status,reason,actor_kind,retry_count,run_at,related_outreach_id) VALUES (gen_random_uuid(),$1,$2,$2,100,'pending','outreach_started','system',0,CURRENT_TIMESTAMP,$3)")
        .bind(company).bind(task).bind(outreach).execute(db).await.unwrap();
}

async fn seed_harness(
    db: &mut PgConnection,
    company: Uuid,
    task: Uuid,
    worker: Uuid,
    principal: Uuid,
) {
    let run = Uuid::new_v4();
    let checkpoint = json!({"schema_version":1,"run_id":run,"revision":0,"state":"active",
        "identity":{"company_id":company,"task_id":task,"agent_id":worker,"harness":"rig","response_contract":null},
        "contract_fingerprint":null,"policy":{},"messages":[{}],"reservations":[],"turns":[],"invocations":[],"executions":[]});
    insert_record(
        db,
        "task_harness_runs",
        &json!({"id":run,"company_id":company,"task_id":task,
        "agent_id":worker,"owner_principal_id":principal,"ownership_version":1,"schema_version":1,
        "revision":0,"state":"active","checkpoint":checkpoint}),
    )
    .await
    .unwrap();
}
