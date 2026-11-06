//! fix-app 02 §1: a model-tool email parks its job on a delivery wait, and only a continuation can
//! reopen it. A send the job has no continuation left for is refused before anything is written,
//! and the SQL park guard refuses a park at the limit from any writer.
use super::*;
use crate::adapters::persistence::workflow::WORKFLOW_JOB_CONTINUATION_LIMIT;
use crate::application::workflow::message::MessageObservation;

/// Drive the claimed job through `continuations` real park-and-continue cycles: each model-tool
/// email parks the job on its own delivery wait, the delivery settles, and the receipt reopens the
/// job (one continuation), which is then reclaimed under a fresh fence.
async fn continued(continuations: i16) -> Setup {
    let mut setup = Box::pin(send(&[To::Channel])).await;
    let run = setup.scope().run;
    for call in 0..continuations {
        let observed = Box::pin(tool(
            &setup,
            &format!("mail-{call}"),
            send_arguments(&[("user", setup.user)], &format!("Body {call}")),
        ))
        .await
        .unwrap();
        assert!(
            matches!(observed, MessageObservation::DeliveryParked(_)),
            "call {call} parks"
        );
        let delivery = latest_delivery(setup.pool(), run).await;
        Box::pin(continue_delivery(&mut setup, delivery, "delivered")).await;
    }
    assert_eq!(job_continuations(&setup).await, continuations);
    setup
}

async fn job_continuations(setup: &Setup) -> i16 {
    sqlx::query_scalar("SELECT continuation_count FROM background_tasks WHERE id=$1")
        .bind(setup.scope().job.0)
        .fetch_one(setup.pool())
        .await
        .unwrap()
}

/// The Rust constant is the SQL function: one limit, written once on each side.
#[tokio::test]
async fn message_park_continuation_limit_matches_the_sql_function() {
    let Some(pool) = crate::adapters::persistence::test_support::test_pool().await else {
        return;
    };
    let limit: i16 = sqlx::query_scalar("SELECT workflow_job_continuation_limit()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(limit, WORKFLOW_JOB_CONTINUATION_LIMIT);
    // The harness's execution ceiling is one more than the job's continuations (the first
    // execution is not a continuation).
    assert_eq!(
        i32::from(crate::services::harness::runs::MAX_CONTINUATIONS) - 1,
        i32::from(WORKFLOW_JOB_CONTINUATION_LIMIT)
    );
}

/// One below the limit the send parks, the receipt resumes the job with its last continuation,
/// and the step continues under a fresh claim.
#[tokio::test]
async fn message_park_continuation_one_below_the_limit_parks_and_resumes() {
    let mut setup = continued(WORKFLOW_JOB_CONTINUATION_LIMIT - 1).await;
    let observed = Box::pin(tool(
        &setup,
        "the-last-park",
        send_arguments(&[("user", setup.user)], "Last"),
    ))
    .await
    .unwrap();
    assert!(matches!(observed, MessageObservation::DeliveryParked(_)));
    let run = setup.scope().run;
    let delivery = latest_delivery(setup.pool(), run).await;
    Box::pin(continue_delivery(&mut setup, delivery, "delivered")).await;
    assert_eq!(
        job_continuations(&setup).await,
        WORKFLOW_JOB_CONTINUATION_LIMIT
    );
    let after = state(setup.pool(), setup.scope()).await;
    assert_eq!(
        (
            after.run.as_str(),
            after.job.as_str(),
            after.attempt.as_str()
        ),
        ("running", "processing", "processing"),
        "the continued step runs again"
    );
}

/// At the limit the send is refused with the workflow tool-limit observation before any write:
/// no marker, action, destination, message, delivery, receipt or wait, and the job keeps running.
/// (The action intent is prepared before publication, as for the action-limit refusal.)
#[tokio::test]
async fn message_park_continuation_at_the_limit_is_refused_before_any_write() {
    let setup = continued(WORKFLOW_JOB_CONTINUATION_LIMIT).await;
    let run = setup.scope().run;
    let before = rows(setup.pool(), run).await;
    let refused = Box::pin(tool(
        &setup,
        "one-too-many",
        send_arguments(&[("user", setup.user)], "Refused"),
    ))
    .await
    .unwrap();
    assert!(matches!(refused, MessageObservation::ContinuationLimit));
    assert_eq!(rows(setup.pool(), run).await, before);
    let after = state(setup.pool(), setup.scope()).await;
    assert_eq!(
        (
            after.run.as_str(),
            after.job.as_str(),
            after.attempt.as_str()
        ),
        ("running", "processing", "processing")
    );
    assert_eq!(
        job_continuations(&setup).await,
        WORKFLOW_JOB_CONTINUATION_LIMIT
    );
    // An in-app send needs no continuation and still goes out at the limit.
    let in_app = Box::pin(tool(
        &setup,
        "in-app",
        send_arguments(&[("channel", setup.other)], "Fine"),
    ))
    .await
    .unwrap();
    assert!(matches!(in_app, MessageObservation::Committed(_)));
}

/// The invocation of the run's most recent message action: the model-tool send to clone.
async fn latest_invocation(setup: &Setup) -> Uuid {
    sqlx::query_scalar(
        "SELECT invocation_id FROM workflow_message_actions WHERE run_id=$1 \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(setup.scope().run.as_uuid())
    .fetch_one(setup.pool())
    .await
    .unwrap()
}

/// A complete model-tool email publication and its effect park, written in SQL alone under the
/// current claim's fence: every row `publish_on` writes (intent, marker, action, thread, message,
/// delivery and its part, destination, wait) cloned from `source`, then the park's own statements.
/// No Rust continuation check runs, so only the database decides whether the park may commit.
async fn sql_effect_park(setup: &Setup, source: Uuid) -> Result<(), sqlx::Error> {
    let fence = setup.claim.fence;
    let scope = fence.scope;
    let (invocation, marker, wait, thread, message, delivery) = (
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    );
    let digest = format!("{:032x}{:032x}", invocation.as_u128(), source.as_u128());
    let mut tx = setup.pool().begin().await?;
    sqlx::query(
        "INSERT INTO workflow_action_intents (company_id,run_id,execution_id,id,operation_key,operation_digest,argument_digest,idempotency_key,operation,policy_decision,content_level_at_freeze,destination_level_at_freeze,origin_kind,agent_manifest_id,agent_id,agent_revision_id,agent_binding_id) \
         SELECT company_id,run_id,execution_id,$2,operation_key,$3,argument_digest,'workflow-action:v1:'||$3,operation,policy_decision,content_level_at_freeze,destination_level_at_freeze,origin_kind,agent_manifest_id,agent_id,agent_revision_id,agent_binding_id FROM workflow_action_intents WHERE id=$1",
    )
    .bind(source)
    .bind(invocation)
    .bind(&digest)
    .execute(&mut *tx)
    .await?;
    let statements: Vec<(&str, Vec<Uuid>)> = vec![
        (
            "INSERT INTO workflow_action_dispatches (company_id,run_id,execution_id,invocation_id,argument_digest,id,job_id,attempt_number,execution_generation,worker_id,effect_kind,current_policy) \
          SELECT company_id,run_id,execution_id,$2,argument_digest,$3,job_id,$5,$4,$6,'message',current_policy FROM workflow_action_dispatches WHERE invocation_id=$1",
            vec![source, invocation, marker],
        ),
        (
            "INSERT INTO workflow_message_actions (company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,origin,wait_id,destination_count) \
          SELECT company_id,run_id,execution_id,$2,argument_digest,$3,origin,$4,destination_count FROM workflow_message_actions WHERE invocation_id=$1",
            vec![source, invocation, marker, wait],
        ),
        (
            "INSERT INTO threads SELECT (jsonb_populate_record(NULL::threads, to_jsonb(t) || jsonb_build_object('id',$2::uuid))).* \
          FROM threads AS t JOIN workflow_message_destinations AS target ON target.thread_id=t.id WHERE target.invocation_id=$1",
            vec![source, thread],
        ),
        (
            "INSERT INTO thread_principals SELECT (jsonb_populate_record(NULL::thread_principals, to_jsonb(p) || jsonb_build_object('thread_id',$2::uuid))).* \
          FROM thread_principals AS p JOIN workflow_message_destinations AS target ON target.thread_id=p.thread_id WHERE target.invocation_id=$1",
            vec![source, thread],
        ),
        (
            "INSERT INTO messages SELECT (jsonb_populate_record(NULL::messages, to_jsonb(m) || jsonb_build_object('id',$2::uuid))).* \
          FROM messages AS m JOIN workflow_message_destinations AS target ON target.message_id=m.id WHERE target.invocation_id=$1",
            vec![source, message],
        ),
        (
            "INSERT INTO thread_messages SELECT (jsonb_populate_record(NULL::thread_messages, to_jsonb(e) || jsonb_build_object('id',gen_random_uuid(),'message_id',$2::uuid,'thread_id',$3::uuid))).* \
          FROM thread_messages AS e JOIN workflow_message_destinations AS target ON target.message_id=e.message_id WHERE target.invocation_id=$1",
            vec![source, message, thread],
        ),
        (
            "INSERT INTO message_deliveries SELECT (jsonb_populate_record(NULL::message_deliveries, to_jsonb(d) || jsonb_build_object( \
             'id',$2::uuid,'message_id',$3::uuid,'idempotency_key',d.idempotency_key||':'||$2::text,'status','pending','attempt_count',0, \
             'available_at',clock_timestamp(),'last_error_class',NULL,'last_error_detail',NULL,'delivered_at',NULL, \
             'settled_evidence_id',NULL,'reopen_id',NULL,'created_at',clock_timestamp(),'updated_at',clock_timestamp()))).* \
          FROM message_deliveries AS d JOIN workflow_message_destinations AS target ON target.delivery_id=d.id WHERE target.invocation_id=$1",
            vec![source, delivery, message],
        ),
        (
            "INSERT INTO message_delivery_parts SELECT (jsonb_populate_record(NULL::message_delivery_parts, to_jsonb(part) || jsonb_build_object( \
             'id',gen_random_uuid(),'delivery_id',$2::uuid,'status','prepared','attempt_count',0,'provider_message_key',NULL, \
             'last_error_class',NULL,'last_error_detail',NULL,'request_started_at',NULL,'delivered_at',NULL, \
             'created_at',clock_timestamp(),'updated_at',clock_timestamp()))).* \
          FROM message_delivery_parts AS part JOIN workflow_message_destinations AS target ON target.delivery_id=part.delivery_id WHERE target.invocation_id=$1",
            vec![source, delivery],
        ),
        (
            "INSERT INTO workflow_message_destinations (company_id,run_id,invocation_id,ordinal,kind,destination_channel_id,destination_user_id,replied_message_id,channel_id,thread_id,message_id,delivery_id,principal_id,recipient_address) \
          SELECT company_id,run_id,$2,ordinal,kind,destination_channel_id,destination_user_id,replied_message_id,channel_id,$3,$4,$5,principal_id,recipient_address FROM workflow_message_destinations WHERE invocation_id=$1",
            vec![source, invocation, thread, message, delivery],
        ),
        // `park_on`, statement for statement.
        (
            "INSERT INTO workflow_waits (company_id,run_id,execution_id,id,reason,deadline,created_at) \
          SELECT company_id,id,(SELECT execution_id FROM workflow_message_actions WHERE invocation_id=$1),$2,'effect',deadline,clock_timestamp() \
          FROM workflow_runs WHERE id=(SELECT run_id FROM workflow_message_actions WHERE invocation_id=$1)",
            vec![source, wait],
        ),
    ];
    for (sql, ids) in statements {
        let mut query = sqlx::query(sql);
        for id in &ids {
            query = query.bind(*id);
        }
        if sql.contains("$4,$6") {
            query = query
                .bind(fence.generation.0)
                .bind(fence.attempt.0)
                .bind(fence.worker.0);
        }
        query.execute(&mut *tx).await?;
    }
    sqlx::query("UPDATE task_attempts SET status='completed',stop_reason='effect_parked',finished_at=clock_timestamp() WHERE task_id=$1 AND attempt_number=$2 AND worker_id=$3 AND execution_generation=$4 AND status='processing'")
        .bind(scope.job.0).bind(fence.attempt.0).bind(fence.worker.0).bind(fence.generation.0)
        .execute(&mut *tx).await?;
    sqlx::query("UPDATE background_tasks SET status='completed',updated_at=clock_timestamp(),worker_id=NULL,execution_generation=NULL,locked_at=NULL,lock_expires_at=NULL WHERE company_id=$1 AND id=$2 AND status='processing'")
        .bind(scope.company.as_uuid()).bind(scope.job.0)
        .execute(&mut *tx).await?;
    sqlx::query("UPDATE workflow_runs SET state='waiting',waiting_reason='effect' WHERE company_id=$1 AND id=$2 AND state IN ('queued','running')")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid())
        .execute(&mut *tx).await?;
    tx.commit().await
}

/// The SQL guard refuses an effect park at the limit whatever wrote it. The same hand-written
/// park commits one below the limit (so it is otherwise valid, and its receipt continues the job
/// to the limit) and is refused at the limit, leaving nothing behind.
#[tokio::test]
async fn message_park_continuation_guard_rejects_a_hand_written_park_at_the_limit() {
    let mut setup = continued(WORKFLOW_JOB_CONTINUATION_LIMIT - 1).await;
    let run = setup.scope().run;
    let source = latest_invocation(&setup).await;
    sql_effect_park(&setup, source)
        .await
        .expect("one below the limit the hand-written park commits");
    let parked = state(setup.pool(), setup.scope()).await;
    assert_eq!(
        (
            parked.run.as_str(),
            parked.job.as_str(),
            parked.stop.as_deref()
        ),
        ("waiting", "completed", Some("effect_parked"))
    );
    let delivery = latest_delivery(setup.pool(), run).await;
    Box::pin(continue_delivery(&mut setup, delivery, "delivered")).await;
    assert_eq!(
        job_continuations(&setup).await,
        WORKFLOW_JOB_CONTINUATION_LIMIT
    );

    let before = rows(setup.pool(), run).await;
    let source = latest_invocation(&setup).await;
    let error = sql_effect_park(&setup, source)
        .await
        .expect_err("at the limit the park is refused at commit");
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("23514"),
        "{error}"
    );
    assert!(
        error
            .to_string()
            .contains("workflow completion lost ownership before commit"),
        "{error}"
    );
    assert_eq!(rows(setup.pool(), run).await, before);
    let after = state(setup.pool(), setup.scope()).await;
    assert_eq!(
        (
            after.run.as_str(),
            after.job.as_str(),
            after.attempt.as_str()
        ),
        ("running", "processing", "processing")
    );
}
