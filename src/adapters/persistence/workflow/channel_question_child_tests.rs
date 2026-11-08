use super::resume::ingress;
use super::*;
use crate::application::workflow::{
    batch::{BatchBudget, WorkflowBatch},
    children::WorkflowChildren,
};

#[derive(sqlx::FromRow)]
struct ChildEntry {
    run_id: Uuid,
    execution_id: Uuid,
    job_id: Uuid,
}

struct ChildQuestion {
    parent: ActivationRequest,
    asked: Asked,
}

async fn child_question(s: &Intents) -> ChildQuestion {
    let original = s.message().await;
    let child = WorkflowId::new(Uuid::new_v4());
    let child_version = VersionId::new(Uuid::new_v4());
    let definitions = DefinitionService::new(
        Preflight,
        s.p().clone(),
        WorkflowSourceDecoder,
        step_builtins::StepBuiltinPublicationDirectory {
            persistence: s.p().clone(),
        },
    );
    s.fixture
        .publish_source(
            &definitions,
            child,
            source(child, original.as_uuid(), 1).to_string(),
            child_version,
        )
        .await
        .unwrap();
    let mut parent: Value =
        serde_json::from_str(&registry::example("workflow.call").unwrap().source).unwrap();
    parent["steps"]["start"]["child_workflow_id"] = json!(child.to_string());
    parent["steps"]["start"]["child_version_id"] = json!(child_version.to_string());
    parent["steps"]["start"]["with"] = json!({"input":{"literal":{}}});
    let version = Box::pin(
        s.fixture
            .publish_workflow(WorkflowId::new(Uuid::new_v4()), parent),
    )
    .await;
    let target = Box::pin(bound(&s.fixture, version)).await;
    let parent = Box::pin(admitted_as(
        &s.fixture,
        target,
        RelatedAssociation::Thread {
            channel_id: RelatedChannelId::new(s.channel),
            thread_id: RelatedThreadId::new(s.thread),
        },
        "parent-question",
    ))
    .await;
    s.p().park_child(parent).await.unwrap().unwrap();
    let row:ChildEntry=sqlx::query_as("SELECT child.id AS run_id,execution.id AS execution_id,job.id AS job_id FROM workflow_runs AS child JOIN workflow_executions AS execution ON execution.company_id=child.company_id AND execution.run_id=child.id JOIN background_tasks AS job ON job.company_id=child.company_id AND job.workflow_execution_id=execution.id WHERE child.company_id=$1 AND child.parent_run_id=$2")
        .bind(parent.company.as_uuid()).bind(parent.run.as_uuid()).fetch_one(s.pool()).await.unwrap();
    let scope = ActivationRequest {
        company: parent.company,
        run: RunId::new(row.run_id),
        execution: ExecutionId::new(row.execution_id),
        job: crate::application::workflow::activation::WorkflowJobId(row.job_id),
    };
    let claim = s
        .p()
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    let asked = Box::pin(published(s, &PendingQuestion { scope, claim })).await;
    ChildQuestion { parent, asked }
}

#[tokio::test]
async fn channel_reply_hitl_resume_child_question_returns_to_parent_without_suppression() {
    let s = Box::pin(Intents::new()).await;
    let ChildQuestion { parent, asked } = Box::pin(child_question(&s)).await;
    assert_ne!(asked.run, parent.run);
    let id = reply(&s, &asked, By::Author, "to").await;
    assert_eq!(
        s.p()
            .deliver_reply(ingress(&s, &asked, id).await)
            .await
            .unwrap(),
        ReplyIngress::Recorded(asked.run)
    );
    let WaitProgress::Completed(done) = s.p().resume_wait(asked.answer).await.unwrap() else {
        panic!("child answer");
    };
    assert!(matches!(
        s.p().resume_wait(parent).await.unwrap(),
        WaitProgress::Pending
    ));
    let finished = s
        .p()
        .advance_pure(
            done.successor.unwrap(),
            BatchBudget::new(2, Duration::from_secs(1)).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        finished.response_disposition,
        Some(crate::domain::workflow::ResponseDisposition::Suppressed)
    );
    let WaitProgress::Completed(done) = s.p().resume_wait(parent).await.unwrap() else {
        panic!("parent resumes after child success");
    };
    assert_eq!(
        done.response_disposition,
        crate::domain::workflow::ResponseDisposition::Automatic
    );
    assert_eq!(done.output, "done");
    assert_eq!(counts(&s, asked.run).await.questions, 1);
    assert_eq!(counts(&s, parent.run).await.questions, 0);
}
