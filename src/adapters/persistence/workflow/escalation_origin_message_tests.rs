//! Genuine native message and final-reply publication preserve origin through producer retention.
use super::*;
use crate::adapters::persistence::workflow::agent_history_retention_fixture as history_clock;
use crate::application::workflow::activation::{ActivationRequest, WorkflowJobId};
use crate::application::workflow::channel_reply::*;
use crate::application::workflow::message::{
    MessageArguments, MessagePlanner, MessagePublicationIdentity,
};
use crate::domain::workflow::ExecutionId;
use crate::services::test_support::{LlmTurn, register_scripted_agent_base_url, scripted_llm};
use std::time::Duration;

#[derive(sqlx::FromRow)]
struct Output {
    run_id: Uuid,
    message_id: Uuid,
    origin_id: Uuid,
}

async fn output(f: &AgentCase) -> Output {
    sqlx::query_as("SELECT destination.run_id,destination.message_id,origin.origin_id \
        FROM workflow_message_destinations AS destination JOIN workflow_message_notification_origins AS origin \
          ON origin.company_id=destination.company_id AND origin.message_id=destination.message_id \
        JOIN workflow_message_producer_seals AS producer ON producer.company_id=destination.company_id \
          AND producer.message_id=destination.message_id AND producer.producer_run_id=destination.run_id \
        WHERE destination.company_id=$1")
        .bind(f.people.role.company().as_uuid()).fetch_one(f.people.role.p().pool()).await.unwrap()
}

async fn reject_unrelated_message(f: &AgentCase, output: &Output) {
    #[derive(sqlx::FromRow)]
    struct Input {
        message_id: Uuid,
        channel_id: Uuid,
        thread_id: Uuid,
    }
    ordinary(f).await;
    let p = f.people.role.p();
    let input:Input=sqlx::query_as("SELECT message.id AS message_id,membership.channel_id,membership.thread_id \
        FROM messages AS message JOIN thread_messages AS membership ON membership.company_id=message.company_id AND membership.message_id=message.id \
        WHERE message.company_id=$1 AND message.role='human' ORDER BY message.created_at DESC LIMIT 1")
        .bind(f.people.role.company().as_uuid()).fetch_one(p.pool()).await.unwrap();
    sqlx::query("UPDATE workflow_message_intents SET available_at=clock_timestamp()+interval '1 hour' WHERE company_id=$1 AND message_id=$2")
        .bind(f.people.role.company().as_uuid()).bind(input.message_id).execute(p.pool()).await.unwrap();
    let error=sqlx::query("UPDATE workflow_message_producer_seals SET producer_run_id=$2 WHERE company_id=$1 AND message_id=$3")
        .bind(f.people.role.company().as_uuid()).bind(output.run_id).bind(input.message_id).execute(p.pool()).await.unwrap_err();
    assert_eq!(
        error.as_database_error().and_then(|e| e.code()).as_deref(),
        Some("23514")
    );
    let error=sqlx::query("INSERT INTO workflow_message_destinations(company_id,run_id,invocation_id,ordinal,kind,destination_channel_id, \
        destination_user_id,replied_message_id,channel_id,thread_id,message_id,delivery_id,principal_id,recipient_address) \
        SELECT company_id,run_id,invocation_id,ordinal+1,'channel',$4,NULL,NULL,$4,$5,$3,NULL,NULL,NULL \
        FROM workflow_message_destinations WHERE company_id=$1 AND message_id=$2")
        .bind(f.people.role.company().as_uuid()).bind(output.message_id).bind(input.message_id)
        .bind(input.channel_id).bind(input.thread_id).execute(p.pool()).await.unwrap_err();
    assert_eq!(
        error.as_database_error().and_then(|e| e.code()).as_deref(),
        Some("23514")
    );
    assert!(
        error.to_string().contains("exact canonical producer"),
        "{error}"
    );
    let unmarked:bool=sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM workflow_message_notification_origins WHERE company_id=$1 AND message_id=$2)")
        .bind(f.people.role.company().as_uuid()).bind(input.message_id).fetch_one(p.pool()).await.unwrap();
    assert!(
        unmarked,
        "a raw unrelated canonical message never acquired workflow provenance"
    );
}

async fn send(f: &AgentCase) -> Output {
    let llm=scripted_llm(vec![
        LlmTurn::tool_call("message__send",json!({"content":{"body":"A bounded follow-up"},"destinations":[{"kind":"channel","id":f.people.role.channel}]})),
        LlmTurn::text(r#"{"body":"Follow-up sent."}"#),
    ]).await;
    register_scripted_agent_base_url(f.created.agent.id, &llm.base_url);
    let after = Box::pin(super::super::super::tool_tests::execute(f)).await;
    let model = llm.finish().await;
    assert_eq!(
        after["handler_state"], "succeeded",
        "{after}; model: {model:?}"
    );
    assert_eq!(model.unwrap(), 2);
    assert_eq!(after["progress"], "waiting");
    assert_eq!(after["commands"], 0);
    let output = output(f).await;
    let original: Uuid = sqlx::query_scalar(
        "SELECT notice_id FROM workflow_escalation_notice_targets WHERE company_id=$1",
    )
    .bind(f.people.role.company().as_uuid())
    .fetch_one(f.people.role.p().pool())
    .await
    .unwrap();
    assert_eq!(output.origin_id, original);
    output
}

async fn retire_producer(f: &AgentCase, run: RunId) {
    #[derive(sqlx::FromRow)]
    struct HistorySource {
        execution_id: Uuid,
        job_id: Uuid,
    }
    let sources: Vec<HistorySource> = sqlx::query_as(
        "SELECT execution_id,job_id FROM agent_history_exchanges WHERE company_id=$1 AND run_id=$2 AND payload_retained",
    ).bind(f.people.role.company().as_uuid()).bind(run.as_uuid())
        .fetch_all(f.people.role.p().pool()).await.unwrap();
    assert_eq!(
        sources.len(),
        1,
        "the genuine native agent run owns one retained history exchange"
    );
    let source = &sources[0];
    let p = f.people.role.p();
    let scope = ActivationRequest {
        company: f.people.role.company(),
        run,
        execution: ExecutionId::new(source.execution_id),
        job: WorkflowJobId(source.job_id),
    };
    history_clock::age(p, scope).await;
    let candidate = history_clock::candidate(p, scope).await;
    if !p.expire_history(candidate).await.unwrap() {
        let facts: Value = sqlx::query_scalar("SELECT jsonb_build_object('state',exchange.state,'job',job.status, \
            'old',exchange.finished_at<=clock_timestamp()-interval '90 days', \
            'active',(SELECT active_exchange_id=exchange.id FROM agent_history_conversations WHERE company_id=exchange.company_id AND id=exchange.conversation_id), \
            'attempts',(SELECT count(*) FROM task_attempts WHERE task_id=job.id AND status='processing'), \
            'retry_safe',workflow_action_retry_safe(exchange.company_id,exchange.execution_id), \
            'messages',(SELECT jsonb_agg(state) FROM workflow_message_actions WHERE company_id=exchange.company_id AND run_id=exchange.run_id), \
            'finals',(SELECT jsonb_agg(state) FROM channel_reply_decisions WHERE company_id=exchange.company_id AND run_id=exchange.run_id)) \
            FROM agent_history_exchanges AS exchange JOIN background_tasks AS job ON job.company_id=exchange.company_id AND job.id=exchange.job_id \
            WHERE exchange.company_id=$1 AND exchange.run_id=$2")
            .bind(scope.company.as_uuid()).bind(run.as_uuid()).fetch_one(p.pool()).await.unwrap();
        panic!("settled producer payload must expire through its actual owner: {facts}");
    }
    history_clock::retire_source(p, scope).await;
}

fn publisher(f: &AgentCase) -> ChannelReplyPublisher<PostgresPersistence> {
    let p = f.people.role.p();
    ChannelReplyPublisher::new(
        p.clone(),
        MessagePlanner::new(
            crate::adapters::persistence::workflow::tests::approval_test_support::composer(p),
            "agents.test".into(),
        ),
    )
}

async fn finish_final(f: &AgentCase, run: Uuid) {
    let p = f.people.role.p();
    let claims = p
        .claim_replies(
            ReplyPublicationWorker(Uuid::new_v4()),
            ReplyPublicationPage::try_from(1).unwrap(),
        )
        .await
        .unwrap();
    for claim in claims {
        assert_eq!(claim.company, f.people.role.company());
        assert_eq!(claim.run.as_uuid(), run);
        assert!(matches!(
            publisher(f).publish_claim(claim).await.unwrap(),
            ReplyPublicationOutcome::Published(_)
        ));
    }
    let published: bool = sqlx::query_scalar(
        "SELECT state='published' FROM channel_reply_decisions WHERE company_id=$1 AND run_id=$2",
    )
    .bind(f.people.role.company().as_uuid())
    .bind(run)
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert!(
        published,
        "sending to another channel does not settle the original notice's automatic final reply"
    );
}

#[tokio::test]
async fn workflow_escalation_origin_native_message_keeps_origin_after_actual_producer_retention() {
    let (f, source) = Box::pin(pending_agent_from_run_with_tools(&[
        AgentBuiltinTool::MessageSend,
    ]))
    .await;
    let output = Box::pin(send(&f)).await;
    Box::pin(finish_final(&f, output.run_id)).await;
    Box::pin(reject_unrelated_message(&f, &output)).await;
    retire(&f, source).await;
    retire_producer(&f, RunId::new(output.run_id)).await;
    let retained: bool = sqlx::query_scalar("SELECT origin.origin_id=$3 AND producer.producer_run_id=$4 \
        AND NOT EXISTS(SELECT 1 FROM workflow_runs WHERE company_id=$1 AND id=$4) \
        FROM workflow_message_notification_origins AS origin JOIN workflow_message_producer_seals AS producer \
          ON producer.company_id=origin.company_id AND producer.message_id=origin.message_id \
        WHERE origin.company_id=$1 AND origin.message_id=$2")
        .bind(f.people.role.company().as_uuid()).bind(output.message_id).bind(output.origin_id)
        .bind(output.run_id).fetch_one(f.people.role.p().pool()).await.unwrap();
    assert!(
        retained,
        "the genuine native tool message retains exact suppression after its real producing run retires"
    );
    assert_eq!(counts(&f).await["cases"], 0);
    assert_eq!(counts(&f).await["roots"], 1);
    assert_eq!(counts(&f).await["messages"], 3);
}

async fn publish_after_omission(f: &AgentCase) -> ReplyPublicationReceipt {
    let p = f.people.role.p();
    let claims = p
        .claim_replies(
            ReplyPublicationWorker(Uuid::new_v4()),
            ReplyPublicationPage::try_from(1).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(claims.len(), 1);
    let claim = claims[0];
    let publisher = publisher(f);
    let ReplyPublicationLoad::Ready(draft) = p.load_reply_publication(claim).await.unwrap() else {
        panic!("the genuine pending reply must load its exact frozen draft");
    };
    let plan = MessagePlanner::new(
        crate::adapters::persistence::workflow::tests::approval_test_support::composer(p),
        "agents.test".into(),
    )
    .plan(
        MessagePublicationIdentity {
            company: claim.company,
            invocation: draft.invocation,
        },
        &MessageArguments::Reply {
            source: draft.source,
            content: draft.content,
        },
        &draft.routes,
    )
    .await
    .unwrap();
    let before = counts(f).await;
    sqlx::raw_sql("CREATE SEQUENCE final_origin_commit_reached; \
        CREATE FUNCTION omit_final_origin() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NULL; END $$; \
        CREATE TRIGGER aa_omit_final_origin BEFORE INSERT ON workflow_message_notification_origins \
          FOR EACH ROW EXECUTE FUNCTION omit_final_origin(); \
        CREATE FUNCTION mark_final_origin_commit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
          IF NEW.state='published' THEN PERFORM nextval('final_origin_commit_reached'); END IF; RETURN NEW; END $$; \
        CREATE TRIGGER zz_mark_final_origin_commit AFTER UPDATE ON channel_reply_decisions \
          FOR EACH ROW EXECUTE FUNCTION mark_final_origin_commit();")
        .execute(p.pool()).await.unwrap();
    let error = p.commit_reply_publication(claim, &plan).await.unwrap_err();
    assert!(
        error.to_string().contains("producer origin")
            || error.to_string().contains("producing run origin"),
        "{error}"
    );
    let reached: bool = sqlx::query_scalar("SELECT is_called FROM final_origin_commit_reached")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert!(
        reached,
        "the actual publication fence completed before deferred sealing refused commit"
    );
    assert_eq!(
        counts(f).await,
        before,
        "canonical publication and origin roll back together"
    );
    sqlx::query("DROP TRIGGER aa_omit_final_origin ON workflow_message_notification_origins")
        .execute(p.pool())
        .await
        .unwrap();
    let result = publisher.publish_claim(claim).await.unwrap();
    let ReplyPublicationOutcome::Published(receipt) = result else {
        panic!("{result:?}")
    };
    assert_eq!(publisher.publish_claim(claim).await.unwrap(), result);
    assert!(
        receipt.delivery.is_none(),
        "internal final reply does not create external delivery"
    );
    receipt
}

async fn wait_for_final(f: &AgentCase, producer: Uuid) {
    let p = f.people.role.p();
    let state: String = sqlx::query_scalar(
        "SELECT state FROM channel_reply_decisions WHERE company_id=$1 AND run_id=$2",
    )
    .bind(f.people.role.company().as_uuid())
    .bind(producer)
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(
        state, "pending",
        "the genuine notice handler produced one pending final reply"
    );
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let due: bool = sqlx::query_scalar("SELECT available_at<=clock_timestamp() FROM channel_reply_decisions WHERE company_id=$1 AND run_id=$2")
                .bind(f.people.role.company().as_uuid()).bind(producer).fetch_one(p.pool()).await.unwrap();
            if due { break; }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }).await.expect("actual database clock must reach final publication scheduling");
}

#[tokio::test]
async fn workflow_escalation_origin_real_final_publication_seals_atomically_and_survives_actual_producer_retention()
 {
    let (f, source) = Box::pin(pending_agent_from_run()).await;
    // Keep the real final publisher out until the native handler and all its
    // workers have stopped; this changes only queue scheduling in this database.
    sqlx::raw_sql(
        "CREATE FUNCTION hold_origin_final() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
        NEW.available_at=clock_timestamp()+interval '15 seconds'; RETURN NEW; END $$; \
        CREATE TRIGGER hold_origin_final BEFORE INSERT ON channel_reply_decisions \
          FOR EACH ROW EXECUTE FUNCTION hold_origin_final();",
    )
    .execute(f.people.role.p().pool())
    .await
    .unwrap();
    let llm = scripted_llm(vec![LlmTurn::text(r#"{"body":"Final result"}"#)]).await;
    register_scripted_agent_base_url(f.created.agent.id, &llm.base_url);
    let after = Box::pin(super::super::super::tool_tests::execute(&f)).await;
    let model = llm.finish().await;
    assert_eq!(
        after["handler_state"], "succeeded",
        "{after}; model: {model:?}"
    );
    assert_eq!(model.unwrap(), 1);
    assert_eq!(
        after["progress"], "waiting",
        "success is not acknowledgement"
    );
    let p = f.people.role.p();
    let (producer, origin): (Uuid, Uuid) = sqlx::query_as(
        "SELECT ownership.run_id,ownership.notice_id FROM workflow_escalation_handling_roots AS ownership WHERE company_id=$1")
        .bind(f.people.role.company().as_uuid()).fetch_one(p.pool()).await.unwrap();
    retire(&f, source).await;
    wait_for_final(&f, producer).await;
    let receipt = Box::pin(publish_after_omission(&f)).await;
    let exact: bool = sqlx::query_scalar("SELECT origin.origin_id=$3 AND producer.producer_run_id=$4 \
        AND message.clean_text_body='Final result' AND response.state='published' AND response.run_id=$4 \
        FROM workflow_message_notification_origins AS origin JOIN workflow_message_producer_seals AS producer \
          ON producer.company_id=origin.company_id AND producer.message_id=origin.message_id \
        JOIN messages AS message ON message.company_id=origin.company_id AND message.id=origin.message_id \
        JOIN channel_reply_decisions AS response ON response.company_id=origin.company_id AND response.message_id=origin.message_id \
        WHERE origin.company_id=$1 AND origin.message_id=$2")
        .bind(f.people.role.company().as_uuid()).bind(receipt.message.as_uuid()).bind(origin)
        .bind(producer).fetch_one(p.pool()).await.unwrap();
    assert!(
        exact,
        "the real final reply inherited only its exact producing run's origin"
    );
    retire_producer(&f, RunId::new(producer)).await;
    let retained: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_message_notification_origins \
        WHERE company_id=$1 AND message_id=$2 AND origin_id=$3) AND NOT EXISTS(SELECT 1 FROM workflow_runs WHERE company_id=$1 AND id=$4)")
        .bind(f.people.role.company().as_uuid()).bind(receipt.message.as_uuid()).bind(origin)
        .bind(producer).fetch_one(p.pool()).await.unwrap();
    assert!(
        retained,
        "the canonical final reply keeps suppression after actual producer retention"
    );
    assert_eq!(counts(&f).await["cases"], 0);
    assert_eq!(counts(&f).await["roots"], 1);
}
