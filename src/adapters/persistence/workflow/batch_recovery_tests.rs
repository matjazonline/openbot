use super::*;
use crate::adapters::persistence::workflow::batch::advance_on;

#[tokio::test]
async fn workflow_batch_deferred_deadline_and_lost_ack_are_atomic() {
    let (f, request) = fixture(source()).await;
    sqlx::query("UPDATE workflow_runs SET deadline = clock_timestamp() + interval '350 milliseconds' WHERE id = $1")
        .bind(request.run.as_uuid()).execute(f.persistence().pool()).await.unwrap();
    let before = snapshot(&f).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    advance_on(&mut tx, request, budget(64)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(360)).await;
    assert!(tx.commit().await.is_err());
    assert_eq!(snapshot(&f).await, before);
    sqlx::query(
        "UPDATE workflow_runs SET deadline = clock_timestamp() + interval '1 hour' WHERE id = $1",
    )
    .bind(request.run.as_uuid())
    .execute(f.persistence().pool())
    .await
    .unwrap();
    let mut tx = f.persistence().pool().begin().await.unwrap();
    let saved = advance_on(&mut tx, request, budget(1)).await.unwrap();
    tx.commit().await.unwrap();
    // Simulate losing the commit response and reconnecting through a fresh pool.
    let options = f.persistence().pool().connect_options();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect_with((*options).clone())
        .await
        .unwrap();
    let reconnect = PostgresPersistence::new(pool.clone());
    let before = snapshot(&f).await;
    let replay = reconnect.advance_pure(request, budget(64)).await.unwrap();
    assert_eq!(replay.disposition, BatchDisposition::Replay);
    assert_eq!(replay.continuation, saved.continuation);
    assert_eq!(snapshot(&f).await, before);
    pool.close().await;
}

#[tokio::test]
async fn workflow_batch_rule_replay_ignores_new_available_predecessor() {
    let mut source = source();
    source["steps"]["rule"]["rule"]["cases"] =
        json!([{"when":{"eq":[{"ref":"/steps/start/output"},{"literal":2}]},"choice":"yes"}]);
    let (f, request) = fixture(source).await;
    f.persistence()
        .advance_pure(request, budget(1))
        .await
        .unwrap();
    // Create a later logical rule activation; a gap is available for a newly
    // committed earlier execution, as in repeated-step recovery fixtures.
    let later = ActivationRequest {
        execution: ExecutionId::new(Uuid::new_v4()),
        job: WorkflowJobId(Uuid::new_v4()),
        ..request
    };
    sqlx::query("INSERT INTO workflow_executions (company_id,run_id,id,step_id,activation) VALUES ($1,$2,$3,'rule',4)")
        .bind(request.company.as_uuid()).bind(request.run.as_uuid()).bind(later.execution.as_uuid()).execute(f.persistence().pool()).await.unwrap();
    sqlx::query("INSERT INTO background_tasks (id,company_id,channel_id,thread_id,correlation_id,task_type,payload,queue_kind,workflow_execution_id) SELECT $1,run.company_id,run.channel_id,run.thread_id,run.correlation_id,'workflow_execution',$3,'workflow',$4 FROM workflow_runs AS run WHERE run.company_id = $2 AND run.id = $5")
        .bind(later.job.0).bind(request.company.as_uuid()).bind(job_payload(later.execution)).bind(later.execution.as_uuid()).bind(request.run.as_uuid()).execute(f.persistence().pool()).await.unwrap();
    let frozen = f.persistence().activate(later).await.unwrap();
    assert_eq!(frozen.choice.as_ref().unwrap().as_str(), "no");
    sqlx::query("INSERT INTO workflow_executions (company_id,run_id,id,step_id,activation,activated_at,frozen_inputs,completed_at,committed_output) VALUES ($1,$2,gen_random_uuid(),'start',3,clock_timestamp(),'{}',clock_timestamp(),'2')")
        .bind(request.company.as_uuid()).bind(request.run.as_uuid()).execute(f.persistence().pool()).await.unwrap();
    assert_eq!(f.persistence().activate(later).await.unwrap(), frozen);
    let result = f
        .persistence()
        .advance_pure(later, budget(64))
        .await
        .unwrap();
    assert_eq!(result.completed, 1);
    let route: String =
        sqlx::query_scalar("SELECT committed_route FROM workflow_executions WHERE id = $1")
            .bind(later.execution.as_uuid())
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert_eq!(route, "choice:no");
}

#[tokio::test]
async fn workflow_batch_hard_transaction_timeout_releases_run_wait() {
    let (f, request) = fixture(source()).await;
    let before = snapshot(&f).await;
    let mut blocker = f.persistence().pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM workflow_runs WHERE id = $1 FOR UPDATE")
        .bind(request.run.as_uuid())
        .execute(&mut *blocker)
        .await
        .unwrap();
    let tiny = BatchBudget::new(1, Duration::from_millis(1)).unwrap();
    assert!(
        tokio::time::timeout(
            Duration::from_secs(3),
            f.persistence().advance_pure(request, tiny)
        )
        .await
        .unwrap()
        .is_err()
    );
    blocker.rollback().await.unwrap();
    assert_eq!(snapshot(&f).await, before);
    assert_eq!(
        f.persistence()
            .advance_pure(request, budget(64))
            .await
            .unwrap()
            .completed,
        3
    );
}

#[tokio::test]
async fn workflow_batch_first_successor_link_enforces_run_and_company_scope() {
    let (f, request) = fixture(source()).await;
    let foreign = CompanyPersistence::create(
        f.persistence(),
        f.binding.target.actor.user_id(),
        CompanyWrite {
            name: "Foreign workflow".into(),
            slug: "foreign-workflow".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let mut draft = f.binding.fixture.save(None);
    draft.target.company = CompanyId::new(foreign.id);
    draft.content.source = source().to_string();
    f.binding.fixture.service().save(draft).await.unwrap();
    let mut publication = f.binding.fixture.publish();
    publication.target.company = CompanyId::new(foreign.id);
    publication.version = crate::domain::workflow::VersionId::new(
        sqlx::query_scalar(
            "SELECT version_id FROM workflow_runs WHERE company_id = $1 AND id = $2",
        )
        .bind(request.company.as_uuid())
        .bind(request.run.as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap(),
    );
    f.binding
        .fixture
        .service()
        .publish(publication)
        .await
        .unwrap();
    let mut tx = f.persistence().pool().begin().await.unwrap();
    // All targets are real, valid executions. Only ownership differs; IDs are
    // never mutated after a committed route has made them immutable.
    let mut targets = Vec::new();
    for company in [request.company.as_uuid(), foreign.id] {
        let run = Uuid::new_v4();
        sqlx::query("INSERT INTO workflow_runs SELECT (jsonb_populate_record(NULL::workflow_runs,to_jsonb(original) || jsonb_build_object('id',$2::uuid,'company_id',$3::uuid))).* FROM workflow_runs AS original WHERE original.id = $1")
            .bind(request.run.as_uuid()).bind(run).bind(company).execute(&mut *tx).await.unwrap();
        let execution = Uuid::new_v4();
        sqlx::query("INSERT INTO workflow_executions (company_id,run_id,id,step_id,activation) VALUES ($1,$2,$3,'rule',2)")
            .bind(company).bind(run).bind(execution).execute(&mut *tx).await.unwrap();
        targets.push(execution);
    }
    let local = Uuid::new_v4();
    sqlx::query("INSERT INTO workflow_executions (company_id,run_id,id,step_id,activation) VALUES ($1,$2,$3,'rule',2)")
        .bind(request.company.as_uuid()).bind(request.run.as_uuid()).bind(local).execute(&mut *tx).await.unwrap();
    for target in targets {
        sqlx::query("SAVEPOINT foreign_successor")
            .execute(&mut *tx)
            .await
            .unwrap();
        let error = first_link(&mut tx, request, target).await.unwrap_err();
        assert!(matches!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514" | "23503")
        ));
        sqlx::query("ROLLBACK TO SAVEPOINT foreign_successor")
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    assert_eq!(
        first_link(&mut tx, request, local)
            .await
            .unwrap()
            .rows_affected(),
        1
    );
    tx.rollback().await.unwrap();
}

async fn first_link(
    db: &mut sqlx::PgConnection,
    request: ActivationRequest,
    successor: Uuid,
) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    sqlx::query("UPDATE workflow_executions SET activated_at = clock_timestamp(),frozen_inputs = '{}',completed_at = clock_timestamp(),committed_output = '1',committed_route = 'success',route_target = 'rule',successor_execution_id = $2 WHERE id = $1")
        .bind(request.execution.as_uuid()).bind(successor).execute(db).await
}
