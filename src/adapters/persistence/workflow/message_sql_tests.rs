//! 04.11 SQL-only writers: the database, not the Rust path, owns every proof (D2 §2.5, §4.3;
//! D3 §A-§C, §F; D4 §N-§P). Each forgery is rejected; a rejected statement leaves nothing.
use super::*;
use crate::application::workflow::message::MessageObservation;
use sqlx::PgConnection;

async fn source_invocation(setup: &Setup) -> Uuid {
    invocations(setup.pool(), setup.scope().run).await[0]
}

/// A new intent cloned from `source`'s.
async fn sql_intent(db: &mut PgConnection, source: Uuid) -> Result<Uuid, sqlx::Error> {
    let invocation = Uuid::new_v4();
    let digest = format!("{:032x}{:032x}", invocation.as_u128(), source.as_u128());
    sqlx::query(
        "INSERT INTO workflow_action_intents (company_id,run_id,execution_id,id,operation_key,operation_digest,argument_digest,idempotency_key,operation,policy_decision) \
         SELECT company_id,run_id,execution_id,$2,operation_key,$3,argument_digest,'workflow-action:v1:'||$3,operation,policy_decision FROM workflow_action_intents WHERE id=$1",
    )
    .bind(source).bind(invocation).bind(&digest).execute(&mut *db).await?;
    Ok(invocation)
}

/// A new intent and `message` marker cloned from `source`, under the same fence.
async fn sql_marker(db: &mut PgConnection, source: Uuid) -> Result<(Uuid, Uuid), sqlx::Error> {
    let invocation = sql_intent(db, source).await?;
    Ok((invocation, sql_marker_for(db, source, invocation).await?))
}

/// The `message` marker of an existing intent: a SQL-only writer's first write.
async fn sql_marker_for(
    db: &mut PgConnection,
    source: Uuid,
    invocation: Uuid,
) -> Result<Uuid, sqlx::Error> {
    let marker = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO workflow_action_dispatches (company_id,run_id,execution_id,invocation_id,argument_digest,id,job_id,attempt_number,execution_generation,worker_id,effect_kind,current_policy) \
         SELECT company_id,run_id,execution_id,$2,argument_digest,$3,job_id,attempt_number,execution_generation,worker_id,'message',current_policy FROM workflow_action_dispatches WHERE invocation_id=$1",
    )
    .bind(source).bind(invocation).bind(marker).execute(&mut *db).await?;
    Ok(marker)
}

/// A complete SQL-only in-app publication cloned from an in-app `source`: marker, action,
/// thread, message, destination and the atomic receipt.
async fn sql_publication(db: &mut PgConnection, source: Uuid) -> Result<Uuid, sqlx::Error> {
    let invocation = sql_intent(db, source).await?;
    sql_publication_for(db, source, invocation).await
}

async fn sql_publication_for(
    db: &mut PgConnection,
    source: Uuid,
    invocation: Uuid,
) -> Result<Uuid, sqlx::Error> {
    let marker = sql_marker_for(db, source, invocation).await?;
    let (thread, message) = (Uuid::new_v4(), Uuid::new_v4());
    sqlx::query(
        "INSERT INTO workflow_message_actions (company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,origin,destination_count) \
         SELECT company_id,run_id,execution_id,$2,argument_digest,$3,origin,destination_count FROM workflow_message_actions WHERE invocation_id=$1",
    )
    .bind(source).bind(invocation).bind(marker).execute(&mut *db).await?;
    sqlx::query(
        "INSERT INTO threads SELECT (jsonb_populate_record(NULL::threads, to_jsonb(t) || jsonb_build_object('id',$2::uuid))).* \
         FROM threads AS t JOIN workflow_message_destinations AS target ON target.thread_id=t.id WHERE target.invocation_id=$1",
    )
    .bind(source).bind(thread).execute(&mut *db).await?;
    sqlx::query(
        "INSERT INTO messages SELECT (jsonb_populate_record(NULL::messages, to_jsonb(m) || jsonb_build_object('id',$2::uuid,'thread_id',$3::uuid))).* \
         FROM messages AS m JOIN workflow_message_destinations AS target ON target.message_id=m.id WHERE target.invocation_id=$1",
    )
    .bind(source).bind(message).bind(thread).execute(&mut *db).await?;
    sqlx::query(
        "INSERT INTO thread_messages SELECT (jsonb_populate_record(NULL::thread_messages, to_jsonb(e) || jsonb_build_object('id',gen_random_uuid(),'message_id',$2::uuid,'thread_id',$3::uuid))).* \
         FROM thread_messages AS e JOIN workflow_message_destinations AS target ON target.message_id=e.message_id WHERE target.invocation_id=$1",
    )
    .bind(source).bind(message).bind(thread).execute(&mut *db).await?;
    sqlx::query(
        "INSERT INTO workflow_message_destinations (company_id,run_id,invocation_id,ordinal,kind,destination_id,thread_id,message_id) \
         SELECT company_id,run_id,$2,ordinal,kind,destination_id,$3,$4 FROM workflow_message_destinations WHERE invocation_id=$1",
    )
    .bind(source).bind(invocation).bind(thread).bind(message).execute(&mut *db).await?;
    sqlx::query(
        "INSERT INTO workflow_action_receipts (company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,effect_kind,result) \
         SELECT company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,'message',(SELECT result FROM workflow_message_aggregate(company_id,invocation_id)) \
         FROM workflow_message_actions WHERE invocation_id=$1",
    )
    .bind(invocation).execute(&mut *db).await?;
    sqlx::query("UPDATE workflow_message_actions SET state='accepted' WHERE invocation_id=$1")
        .bind(invocation)
        .execute(&mut *db)
        .await?;
    Ok(invocation)
}

fn code(error: &sqlx::Error) -> String {
    error
        .as_database_error()
        .and_then(|error| error.code())
        .map(|code| code.to_string())
        .unwrap_or_default()
}

/// An in-app step publication (marker 1) plus `extra` SQL-only clones under its fence.
async fn markers(extra: usize) -> (Setup, Uuid) {
    let setup = Box::pin(send(&[To::Channel])).await;
    assert!(
        setup
            .runtime()
            .execute(&setup.kind, &setup.claim)
            .await
            .is_completed_ok()
    );
    let source = source_invocation(&setup).await;
    let mut tx = setup.pool().begin().await.unwrap();
    for _ in 0..extra {
        sql_publication(&mut tx, source).await.unwrap();
    }
    tx.commit().await.unwrap();
    (setup, source)
}
trait CompletedOk {
    fn is_completed_ok(&self) -> bool;
}
impl CompletedOk for WorkflowHandlerOutcome {
    fn is_completed_ok(&self) -> bool {
        matches!(self, WorkflowHandlerOutcome::Completed(Ok(_)))
    }
}

/// F002/F009 at N and N+1 through the service: the 128th marker is admitted, the 129th is
/// refused before any write with the distinct ActionLimit observation.
#[tokio::test]
async fn workflow_message_publish_admission_admits_128_and_refuses_129() {
    let (setup, _) = Box::pin(markers(126)).await;
    let run = setup.scope().run;
    assert_eq!(rows(setup.pool(), run).await.markers, 127);
    let admitted = Box::pin(tool(
        &setup,
        "the-128th",
        send_arguments(&[("channel", setup.other)], "128"),
    ))
    .await
    .unwrap();
    assert!(matches!(admitted, MessageObservation::Committed(_)));
    let before = rows(setup.pool(), run).await;
    assert_eq!(before.markers, 128);
    let refused = Box::pin(tool(
        &setup,
        "the-129th",
        send_arguments(&[("channel", setup.other)], "129"),
    ))
    .await
    .unwrap();
    assert!(matches!(refused, MessageObservation::ActionLimit));
    assert_eq!(rows(setup.pool(), run).await, before);
    let threads: i64 = sqlx::query_scalar("SELECT count(*) FROM threads WHERE channel_id=$1")
        .bind(setup.other)
        .fetch_one(setup.pool())
        .await
        .unwrap();
    assert_eq!(threads, 128);
}

/// The step path maps the refusal to Terminal `message.action_limit` (FinalError, no wait).
#[tokio::test]
async fn workflow_message_publish_step_at_the_limit_is_a_terminal_action_limit() {
    let setup = Box::pin(send(&[To::Channel])).await;
    let first = Box::pin(tool(
        &setup,
        "seed",
        send_arguments(&[("channel", setup.other)], "seed"),
    ))
    .await
    .unwrap();
    assert!(matches!(first, MessageObservation::Committed(_)));
    let source = source_invocation(&setup).await;
    let mut tx = setup.pool().begin().await.unwrap();
    for _ in 0..127 {
        sql_publication(&mut tx, source).await.unwrap();
    }
    tx.commit().await.unwrap();
    let before = rows(setup.pool(), setup.scope().run).await;
    assert_eq!(before.markers, 128);
    let WorkflowHandlerOutcome::Completed(Err(failure)) =
        setup.runtime().execute(&setup.kind, &setup.claim).await
    else {
        panic!("the step is refused");
    };
    assert_eq!(failure.failure.code().as_str(), "message.action_limit");
    assert_eq!(
        failure.failure.class(),
        crate::domain::workflow::FailureClass::Terminal
    );
    assert_eq!(rows(setup.pool(), setup.scope().run).await, before);
    assert_eq!(state(setup.pool(), setup.scope()).await.wait, None);
}

/// D3 §A: two SQL-only writers under one fence at 127 markers serialize on the attempt lock;
/// exactly one commits and the other raises 54000.
#[tokio::test]
async fn workflow_message_sql_admission_backstop_serializes_competing_writers() {
    let (setup, source) = Box::pin(markers(126)).await;
    // Intents exist first, so each writer's first write is its marker: the admission trigger
    // is where they meet, before either holds any foreign-key lock.
    let mut intents = setup.pool().begin().await.unwrap();
    let (a, b) = (
        sql_intent(&mut intents, source).await.unwrap(),
        sql_intent(&mut intents, source).await.unwrap(),
    );
    intents.commit().await.unwrap();
    let mut first = setup.pool().begin().await.unwrap();
    sql_publication_for(&mut first, source, a).await.unwrap();
    let pool = setup.pool().clone();
    let second = tokio::spawn(async move {
        let mut tx = pool.begin().await.unwrap();
        let result = sql_publication_for(&mut tx, source, b).await;
        match result {
            Ok(_) => tx.commit().await.map(|_| ()),
            Err(error) => Err(error),
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(
        !second.is_finished(),
        "the second writer waits on the attempt lock"
    );
    first.commit().await.unwrap();
    let error = second.await.unwrap().unwrap_err();
    assert_eq!(code(&error), "54000", "{error}");
    assert_eq!(rows(setup.pool(), setup.scope().run).await.markers, 128);
}

/// F007: fence capture and the step-origin close identity are database-derived.
#[tokio::test]
async fn workflow_message_sql_fence_capture_and_close_identity_reject_forgeries() {
    let (setup, source) = Box::pin(markers(0)).await;
    // A writer cannot choose the fence expiry.
    let mut tx = setup.pool().begin().await.unwrap();
    let (invocation, marker) = sql_marker(&mut tx, source).await.unwrap();
    let forged = sqlx::query(
        "INSERT INTO workflow_message_actions (company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,origin,destination_count,fence_expires_at) \
         SELECT company_id,run_id,execution_id,$2,argument_digest,$3,origin,1,clock_timestamp()+interval '1 year' FROM workflow_message_actions WHERE invocation_id=$1",
    )
    .bind(source).bind(invocation).bind(marker).execute(&mut *tx).await.unwrap_err();
    assert!(forged.to_string().contains("database owned"), "{forged}");
    tx.rollback().await.unwrap();
    // No linkage at all: the deferred guard refuses the marker at COMMIT.
    let mut tx = setup.pool().begin().await.unwrap();
    sql_marker(&mut tx, source).await.unwrap();
    let error = tx.commit().await.unwrap_err();
    assert!(
        error.to_string().contains("requires its linkage"),
        "{error}"
    );
    assert_eq!(rows(setup.pool(), setup.scope().run).await.markers, 1);
}

/// D3 §B: a step park whose attempt was closed with any other cause is rejected at COMMIT.
#[tokio::test]
async fn workflow_message_sql_park_requires_the_effect_parked_close_cause() {
    let setup = Box::pin(send(&[To::User])).await;
    sqlx::raw_sql(
        "CREATE FUNCTION fixture_other_close() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
           IF NEW.stop_reason='effect_parked' THEN NEW.stop_reason:='completed'; END IF; RETURN NEW; END $$; \
         CREATE TRIGGER fixture_other_close BEFORE UPDATE ON task_attempts FOR EACH ROW EXECUTE FUNCTION fixture_other_close();",
    )
    .execute(setup.pool())
    .await
    .unwrap();
    let outcome = setup.runtime().execute(&setup.kind, &setup.claim).await;
    assert!(matches!(outcome, WorkflowHandlerOutcome::Completed(Err(_))));
    assert_eq!(
        rows(setup.pool(), setup.scope().run).await,
        Rows {
            markers: 0,
            actions: 0,
            destinations: 0,
            deliveries: 0,
            receipts: 0,
            waits: 0,
            messages: 0
        }
    );
    let state = state(setup.pool(), setup.scope()).await;
    assert_eq!(
        (state.job.as_str(), state.attempt.as_str()),
        ("processing", "processing")
    );
}

/// F007 and D3 §F: receipts and action states equal the SQL aggregate over final statuses.
#[tokio::test]
async fn workflow_message_sql_receipt_and_state_guards_reject_forgeries() {
    let setup = Box::pin(send(&[To::Channel, To::User])).await;
    parked(&setup).await;
    let (pool, run) = (setup.pool(), setup.scope().run);
    let invocation = source_invocation(&setup).await;
    let receipt = |result: Value| {
        sqlx::query(
            "INSERT INTO workflow_action_receipts (company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,effect_kind,result) \
             SELECT company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,'message',$2 FROM workflow_message_actions WHERE invocation_id=$1",
        )
        .bind(invocation)
        .bind(result)
    };
    let aggregate = || async {
        sqlx::query_scalar::<_, Value>("SELECT aggregate.result FROM workflow_message_actions AS action, LATERAL workflow_message_aggregate(action.company_id, action.invocation_id) AS aggregate WHERE action.invocation_id=$1")
            .bind(invocation).fetch_one(pool).await.unwrap()
    };
    // Premature: one delivery is still pending.
    assert!(receipt(aggregate().await).execute(pool).await.is_err());
    let forged_state = |state: &'static str| {
        sqlx::query("UPDATE workflow_message_actions SET state=$2 WHERE invocation_id=$1")
            .bind(invocation)
            .bind(state)
    };
    for state in ["accepted", "failed", "uncertain", "stalled"] {
        assert!(forged_state(state).execute(pool).await.is_err(), "{state}");
    }
    let unknown = delivery(pool, run, 1).await;
    deliver(pool, unknown, "unknown").await;
    // A failed receipt over an unknown delivery is refused: unknown dominates failed.
    let mut failed = aggregate().await;
    failed["status"] = json!("failed");
    assert!(receipt(failed).execute(pool).await.is_err());
    for statement in [
        "UPDATE workflow_message_actions SET consumed_through=10 WHERE invocation_id=$1",
        "UPDATE workflow_message_actions SET consumed_through=5 WHERE invocation_id=$1",
        "UPDATE workflow_message_actions SET origin='model_tool', wait_id=NULL WHERE invocation_id=$1",
        "UPDATE workflow_message_actions SET destination_count=3 WHERE invocation_id=$1",
        "DELETE FROM workflow_message_actions WHERE invocation_id=$1",
    ] {
        let _ = sqlx::query(statement).bind(invocation).execute(pool).await;
    }
    let (through, origin, count): (i64, String, i16) = sqlx::query_as(
        "SELECT consumed_through, origin, destination_count FROM workflow_message_actions WHERE invocation_id=$1",
    )
    .bind(invocation).fetch_one(pool).await.unwrap();
    assert_eq!(
        (through, origin.as_str(), count),
        (10, "step", 2),
        "only the monotone cursor advance applied"
    );
    // A mismatched result is refused even when every status is final.
    sqlx::query("UPDATE workflow_message_actions SET consumed_through=0 WHERE invocation_id=$1")
        .bind(invocation)
        .execute(pool)
        .await
        .unwrap_err();
    let fresh = Box::pin(send(&[To::User])).await;
    parked(&fresh).await;
    let fresh_delivery = delivery(fresh.pool(), fresh.scope().run, 0).await;
    deliver(fresh.pool(), fresh_delivery, "delivered").await;
    let fresh_invocation = source_invocation(&fresh).await;
    let mismatch = sqlx::query(
        "INSERT INTO workflow_action_receipts (company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,effect_kind,result) \
         SELECT company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,'message','{\"status\":\"accepted\",\"messages\":[]}'::jsonb FROM workflow_message_actions WHERE invocation_id=$1",
    )
    .bind(fresh_invocation).execute(fresh.pool()).await;
    assert!(mismatch.is_err());
    assert_eq!(rows(fresh.pool(), fresh.scope().run).await.receipts, 0);
}

/// Destination links are composite: tenant, message and delivery must agree.
#[tokio::test]
async fn workflow_message_sql_destination_links_reject_foreign_rows() {
    let setup = Box::pin(send(&[To::User])).await;
    parked(&setup).await;
    let (pool, run) = (setup.pool(), setup.scope().run);
    let invocation = source_invocation(&setup).await;
    for (delivery, message, expected) in [
        // No such delivery or message in this tenant: the composite keys refuse it.
        ("gen_random_uuid()", "gen_random_uuid()", "23503"),
        // An existing delivery with another message: one link per delivery.
        ("target.delivery_id", "gen_random_uuid()", "23505"),
    ] {
        let result = sqlx::query(&format!(
            "INSERT INTO workflow_message_destinations (company_id,run_id,invocation_id,ordinal,kind,destination_id,thread_id,message_id,delivery_id,principal_id,recipient_address) \
             SELECT company_id,run_id,invocation_id,1,'user',gen_random_uuid(),thread_id,{message},{delivery},principal_id,recipient_address FROM workflow_message_destinations AS target WHERE invocation_id=$1"
        ))
        .bind(invocation)
        .execute(pool)
        .await;
        assert_eq!(code(&result.unwrap_err()), expected);
    }
    // Destinations are immutable facts.
    assert!(sqlx::query("UPDATE workflow_message_destinations SET recipient_address='x@example.test' WHERE invocation_id=$1").bind(invocation).execute(pool).await.is_err());
    assert_eq!(rows(pool, run).await.destinations, 1);
}

/// D3 §C: delivered and dead_letter are final; outcome_unknown leaves only by evidence; a
/// part re-enters unknown only by an expired reopen proof.
#[tokio::test]
async fn workflow_message_sql_delivery_provenance_rejects_unproven_exits() {
    let setup = Box::pin(send(&[To::User, To::Owner])).await;
    parked(&setup).await;
    let (pool, run) = (setup.pool(), setup.scope().run);
    let (unknown, delivered) = (delivery(pool, run, 0).await, delivery(pool, run, 1).await);
    deliver(pool, unknown, "unknown").await;
    deliver(pool, delivered, "delivered").await;
    for (delivery, statement) in [
        (
            unknown,
            "UPDATE message_deliveries SET status='delivered', delivered_at=clock_timestamp() WHERE id=$1",
        ),
        (
            unknown,
            "UPDATE message_deliveries SET status='retryable' WHERE id=$1",
        ),
        (
            unknown,
            "UPDATE message_deliveries SET status='dead_letter' WHERE id=$1",
        ),
        (
            unknown,
            "UPDATE message_delivery_parts SET status='retryable' WHERE delivery_id=$1",
        ),
        (
            delivered,
            "UPDATE message_deliveries SET status='retryable', delivered_at=NULL WHERE id=$1",
        ),
        (
            delivered,
            "UPDATE message_delivery_parts SET status='dead', delivered_at=NULL WHERE delivery_id=$1",
        ),
    ] {
        let error = sqlx::query(statement)
            .bind(delivery)
            .execute(pool)
            .await
            .unwrap_err();
        assert_eq!(code(&error), "23514", "{statement}: {error}");
    }
    // A part that never went back through a reopen cannot re-enter unknown.
    let fresh = Box::pin(send(&[To::User])).await;
    parked(&fresh).await;
    let pending = delivery(fresh.pool(), fresh.scope().run, 0).await;
    sqlx::query("UPDATE message_delivery_parts SET status='retryable' WHERE delivery_id=$1")
        .bind(pending)
        .execute(fresh.pool())
        .await
        .unwrap();
    let error = sqlx::query("UPDATE message_delivery_parts SET status='outcome_unknown', last_error_class='reconciliation_proof_expired' WHERE delivery_id=$1")
        .bind(pending).execute(fresh.pool()).await.unwrap_err();
    assert_eq!(code(&error), "23514");
    assert_eq!(events(pool, run).await.len(), 2);
}

/// D2 §2.5: an effect wait's deadline is exactly the run deadline and it belongs to an action.
#[tokio::test]
async fn workflow_message_sql_effect_wait_must_match_its_run_and_action() {
    let setup = Box::pin(send(&[To::Channel])).await;
    let scope = setup.scope();
    for deadline in [
        "(SELECT deadline FROM workflow_runs WHERE id=$2)",
        "(SELECT deadline - interval '1 second' FROM workflow_runs WHERE id=$2)",
    ] {
        let error = sqlx::query(&format!(
            "INSERT INTO workflow_waits (company_id,run_id,execution_id,id,reason,deadline,created_at) VALUES ($1,$2,$3,gen_random_uuid(),'effect',{deadline},clock_timestamp())"
        ))
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
        .execute(setup.pool()).await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("must belong to one message action"),
            "{error}"
        );
    }
}

/// 06-F020: a step execution keeps exactly one effect wait (a saved model tool call may park
/// many, one per call). The fence-capture trigger is disabled inside the rolled-back
/// transaction only so that the index itself is what refuses the second step wait.
#[tokio::test]
async fn workflow_message_sql_step_execution_keeps_one_effect_wait() {
    let setup = Box::pin(send(&[To::User])).await;
    assert!(matches!(
        setup.runtime().execute(&setup.kind, &setup.claim).await,
        WorkflowHandlerOutcome::Suspended(_)
    ));
    let mut tx = setup.pool().begin().await.unwrap();
    sqlx::query(
        "ALTER TABLE workflow_message_actions DISABLE TRIGGER workflow_message_action_capture",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    let error = sqlx::query(
        "INSERT INTO workflow_message_actions (company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,origin,wait_id,fence_expires_at,destination_count) \
         SELECT company_id,run_id,execution_id,gen_random_uuid(),argument_digest,gen_random_uuid(),origin,gen_random_uuid(),fence_expires_at,destination_count \
         FROM workflow_message_actions WHERE run_id=$1 AND origin='step'",
    )
    .bind(setup.scope().run.as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap_err();
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.constraint()),
        Some("workflow_message_step_one_wait"),
        "{error}"
    );
    tx.rollback().await.unwrap();
}
