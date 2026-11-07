//! Atomic question publication and trusted response continuation against PostgreSQL.
use super::*;
use crate::application::agent::ToolCallOrigin;
use crate::application::workflow::lease::{
    ClaimedWorkflow, LeasePolicy, WorkflowLeases, WorkflowWorkerId,
};
use crate::application::workflow::message::{
    MessageActionPreparation, MessageKind, MessageToolCall,
};
use crate::application::workflow::worker::WorkflowHandler;

#[path = "channel_question_approval_tests.rs"]
mod approval;
#[path = "channel_question_child_tests.rs"]
mod child;
#[path = "channel_question_commit_tests.rs"]
mod commit;
#[path = "channel_question_integrity_tests.rs"]
mod integrity;
#[path = "channel_question_lifecycle_tests.rs"]
mod lifecycle;
#[path = "channel_question_release_tests.rs"]
mod release;
#[path = "channel_question_resume_tests.rs"]
mod resume;

struct PendingQuestion {
    scope: ActivationRequest,
    claim: ClaimedWorkflow,
}

fn source(workflow: WorkflowId, original: Uuid, rounds: usize) -> Value {
    let mut source: Value =
        serde_json::from_str(&registry::example("message.ask").unwrap().source).unwrap();
    source["workflow_id"] = json!(workflow.to_string());
    let question = json!({"type":"message.ask","with":{
        "source_message":{"literal":original},"content":{"literal":{"body":"Which option?"}},
        "deadline":{"literal":(Utc::now()+chrono::Duration::minutes(10)).to_rfc3339()},
        "payload_schema":{"literal":MessageTriggerInput::schema()}},"routes":{"success":"finish"}});
    source["steps"]["start"] = question.clone();
    if rounds == 2 {
        source["steps"]["start"]["routes"] = json!({"success":"again"});
        source["steps"]["again"] = question;
    }
    source["steps"]["finish"] = json!({"type":"channel.no_reply","with":{
        "value":{"literal":"done"},"output_schema":{"literal":{"type":"string"}}},
        "routes":{"success":"$end"}});
    source
}

async fn pending(s: &Intents, rounds: usize) -> PendingQuestion {
    Box::pin(pending_with(
        s,
        rounds,
        |_| {},
        crate::application::workflow::DeliveryPolicy::Deliver,
    ))
    .await
}

async fn pending_with(
    s: &Intents,
    rounds: usize,
    edit: impl FnOnce(&mut Value),
    policy: crate::application::workflow::DeliveryPolicy,
) -> PendingQuestion {
    Box::pin(pending_using(s, rounds, edit, policy, lease())).await
}

async fn pending_using(
    s: &Intents,
    rounds: usize,
    edit: impl FnOnce(&mut Value),
    policy: crate::application::workflow::DeliveryPolicy,
    lease: LeasePolicy,
) -> PendingQuestion {
    let original = s.message().await;
    let workflow = WorkflowId::new(Uuid::new_v4());
    let version = VersionId::new(Uuid::new_v4());
    let definitions = DefinitionService::new(
        Preflight,
        s.p().clone(),
        WorkflowSourceDecoder,
        message::MessagePublicationDirectory {
            persistence: s.p().clone(),
        },
    );
    let mut source = source(workflow, original.as_uuid(), rounds);
    edit(&mut source);
    s.fixture
        .publish_source(&definitions, workflow, source.to_string(), version)
        .await
        .unwrap();
    let target = Box::pin(bound(&s.fixture, version)).await;
    let scope = Box::pin(
        crate::adapters::persistence::workflow::tests::message_tests::support::admitted_with(
            &s.fixture,
            target,
            RelatedAssociation::Thread {
                channel_id: RelatedChannelId::new(s.channel),
                thread_id: RelatedThreadId::new(s.thread),
            },
            &format!("atomic-question-{workflow}"),
            policy,
        ),
    )
    .await;
    let claim = s
        .p()
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease)
        .await
        .unwrap()
        .unwrap();
    PendingQuestion { scope, claim }
}

#[derive(sqlx::FromRow)]
struct PromptRow {
    message_id: Uuid,
    respondent_id: Uuid,
    thread_id: Uuid,
    channel_id: Uuid,
}

async fn published(s: &Intents, pending: &PendingQuestion) -> Asked {
    let outcome = runtime(s.p())
        .execute(&WorkflowStepKind("message.ask".into()), &pending.claim)
        .await;
    let WorkflowHandlerOutcome::Suspended(suspended) = outcome else {
        panic!("{outcome:?}");
    };
    assert!(
        s.p()
            .confirm_suspension(pending.claim.fence, suspended.wait)
            .await
            .unwrap()
    );
    let row: PromptRow = sqlx::query_as("SELECT question.message_id,question.respondent_id,destination.thread_id,destination.channel_id FROM workflow_channel_questions AS question JOIN workflow_message_destinations AS destination ON destination.company_id=question.company_id AND destination.invocation_id=question.invocation_id WHERE question.company_id=$1 AND question.execution_id=$2")
        .bind(pending.scope.company.as_uuid()).bind(pending.scope.execution.as_uuid())
        .fetch_one(s.pool()).await.unwrap();
    Asked {
        run: pending.scope.run,
        answer: pending.scope,
        question: row.message_id,
        thread: row.thread_id,
        channel: row.channel_id,
        expected: Some(row.respondent_id),
    }
}

async fn managed(s: &Intents, rounds: usize) -> Asked {
    let pending = Box::pin(pending(s, rounds)).await;
    Box::pin(published(s, &pending)).await
}

#[derive(Debug, sqlx::FromRow)]
struct Counts {
    questions: i64,
    waits: i64,
    destinations: i64,
    dispatches: i64,
}
async fn counts(s: &Intents, run: RunId) -> Counts {
    sqlx::query_as("SELECT (SELECT count(*) FROM workflow_channel_questions WHERE run_id=$1) AS questions,(SELECT count(*) FROM workflow_waits WHERE run_id=$1) AS waits,(SELECT count(*) FROM workflow_message_destinations WHERE run_id=$1) AS destinations,(SELECT count(*) FROM workflow_action_dispatches WHERE run_id=$1) AS dispatches")
        .bind(run.as_uuid()).fetch_one(s.pool()).await.unwrap()
}

#[tokio::test]
async fn channel_reply_hitl_prompt_commits_wait_and_delivery_together() {
    let s = Box::pin(Intents::new()).await;
    let asked = Box::pin(managed(&s, 1)).await;
    let c = counts(&s, asked.run).await;
    assert_eq!(c.questions, 1);
    assert_eq!(c.waits, 1);
    assert_eq!(c.destinations, 1);
    assert_eq!(c.dispatches, 1);
    assert!(matches!(
        s.p().resume_wait(asked.answer).await.unwrap(),
        WaitProgress::Pending
    ));
    let delivered = delivery(s.pool(), asked.run, 0).await;
    deliver(s.pool(), delivered, "delivered").await;
    assert_eq!(
        s.p()
            .consume_message_receipts(asked.answer, lease())
            .await
            .unwrap(),
        1
    );
    assert!(matches!(
        s.p().resume_wait(asked.answer).await.unwrap(),
        WaitProgress::Pending
    ));
    let state: String = sqlx::query_scalar("SELECT state FROM workflow_runs WHERE id=$1")
        .bind(asked.run.as_uuid())
        .fetch_one(s.pool())
        .await
        .unwrap();
    assert_eq!(state, "waiting");
}

#[tokio::test]
async fn channel_reply_hitl_prompt_competing_publishers_create_one_occurrence() {
    let s = Box::pin(Intents::new()).await;
    let pending = Box::pin(pending(&s, 1)).await;
    let first = runtime(s.p());
    let second = runtime(s.p());
    let kind = WorkflowStepKind("message.ask".into());
    let (a, b) = tokio::join!(
        first.execute(&kind, &pending.claim),
        second.execute(&kind, &pending.claim)
    );
    assert!(
        matches!(a, WorkflowHandlerOutcome::Suspended(_))
            || matches!(b, WorkflowHandlerOutcome::Suspended(_)),
        "{a:?}/{b:?}"
    );
    let c = counts(&s, pending.scope.run).await;
    assert_eq!(c.questions, 1);
    assert_eq!(c.waits, 1);
    assert_eq!(c.destinations, 1);
    assert_eq!(c.dispatches, 1);
}

#[tokio::test]
async fn channel_reply_hitl_prompt_cannot_be_an_agent_or_ad_hoc_tool() {
    let s = Box::pin(Intents::new()).await;
    let pending = Box::pin(pending(&s, 1)).await;
    let call = MessageToolCall {
        origin: ToolCallOrigin::Workflow,
        kind: MessageKind::Ask,
        call: crate::application::workflow::actions::ModelToolCallId::parse("forged-question")
            .unwrap(),
        arguments: json!({}),
    };
    let scope = crate::application::workflow::actions::ActionScope {
        company: pending.scope.company,
        run: pending.scope.run,
        execution: pending.scope.execution,
    };
    assert!(s.p().message_tool_request(scope, &call).await.is_err());
    assert_eq!(counts(&s, pending.scope.run).await.questions, 0);
}

#[tokio::test]
async fn channel_reply_hitl_prompt_in_app_acceptance_keeps_the_human_wait_open() {
    let s = Box::pin(Intents::new()).await;
    let pending = Box::pin(pending_with(
        &s,
        1,
        |_| {},
        crate::application::workflow::DeliveryPolicy::InAppOnly,
    ))
    .await;
    let asked = Box::pin(published(&s, &pending)).await;
    let receipts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_action_receipts WHERE run_id=$1")
            .bind(asked.run.as_uuid())
            .fetch_one(s.pool())
            .await
            .unwrap();
    let deliveries:i64=sqlx::query_scalar("SELECT count(*) FROM workflow_message_destinations WHERE run_id=$1 AND delivery_id IS NOT NULL")
        .bind(asked.run.as_uuid()).fetch_one(s.pool()).await.unwrap();
    assert_eq!(receipts, 1);
    assert_eq!(deliveries, 0);
    assert!(matches!(
        s.p().resume_wait(asked.answer).await.unwrap(),
        WaitProgress::Pending
    ));
}

#[tokio::test]
async fn channel_reply_hitl_prompt_uuid_case_and_nanosecond_deadline_keep_the_same_witness() {
    use chrono::Timelike;
    let s = Box::pin(Intents::new()).await;
    let pending=Box::pin(pending_with(&s,1,|source| {
        let original=source["steps"]["start"]["with"]["source_message"]["literal"].as_str().unwrap().to_uppercase();
        source["steps"]["start"]["with"]["source_message"]=json!({"literal":original});
        source["steps"]["start"]["with"]["deadline"]=json!({"literal":
            (Utc::now()+chrono::Duration::minutes(10)).with_nanosecond(123456789).unwrap().to_rfc3339()});
    },crate::application::workflow::DeliveryPolicy::Deliver)).await;
    let asked = Box::pin(published(&s, &pending)).await;
    assert_eq!(counts(&s, asked.run).await.questions, 1);
}
