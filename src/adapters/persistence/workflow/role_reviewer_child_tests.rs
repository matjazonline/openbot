use super::*;
use crate::application::workflow::history::WorkflowHistoryRuntime;
use crate::application::workflow::lease::{LeasePolicy, WorkflowWorkerId};
use crate::application::workflow::polling::{PollPolicy, WorkflowFairPolling};
use crate::application::workflow::worker::WorkflowWorker;

async fn configure_child(f: &PeopleFixture) -> VersionId {
    let fixture = &f.role.admission.binding.fixture;
    let child_id = WorkflowId::new(Uuid::new_v4());
    let mut child: Value =
        serde_json::from_str(&registry::example("decision.human").unwrap().source).unwrap();
    child["input_schema"] = json!(true);
    child["output_schema"] = json!(true);
    child["steps"]["start"]["with"]["reviewers"] = json!({"literal":["reviewer"]});
    child["steps"]["start"]["with"]["data_schema"] = json!({"literal":true});
    child["steps"]["start"]["with"]["feedback_required"] = json!({"literal":[]});
    let child_version = fixture.publish_workflow(child_id, child).await;
    let mut parent: Value =
        serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
    parent["output_schema"] = json!(true);
    parent["steps"]["start"]["routes"]["success"] = json!("call_child");
    parent["steps"]["call_child"] = json!({"type":"workflow.call","child_workflow_id":child_id.to_string(),"child_version_id":child_version.to_string(),"with":{"input":{"literal":{}},"params":{"literal":{}}},"routes":{"success":"$end"}});
    let version = republish(f, parent).await;
    let binding = &f.role.admission.binding;
    let mut request = binding.request(Some(binding.state().await.revision));
    request.version = version;
    request.association = RelatedAssociation::Channel(RelatedChannelId::new(f.role.channel));
    binding.service().configure(request).await.unwrap();
    child_version
}

async fn drive(f: &PeopleFixture, ready: impl AsyncFn() -> bool) {
    let p = f.role.p();
    let history = WorkflowHistoryRuntime::new(p.clone());
    let notices = notices(p);
    let worker = WorkflowWorker {
        port: p,
        handler: &history,
        notices: Some(&notices),
        worker: WorkflowWorkerId(Uuid::new_v4()),
        lease: LeasePolicy::new(std::time::Duration::from_secs(30)).unwrap(),
        poll: PollPolicy::new(std::time::Duration::from_millis(50), 8).unwrap(),
        active_jobs: None,
        monitoring: None,
    };
    let cancel = tokio_util::sync::CancellationToken::new();
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let page = p.poll_fair(worker.worker, 8).await.unwrap();
            for candidate in page.candidates {
                // Keep the generic worker state machine off the stock test-thread stack.
                Box::pin(worker.process(candidate, &cancel)).await.unwrap();
            }
            if ready().await {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("child reviewer workflow reaches its actual wait/completion");
}

async fn replace_people(f: &PeopleFixture) -> PrincipalId {
    let departments = crate::adapters::persistence::test_support::department_use_cases(f.role.p());
    let owner = f.role.admission.binding.target.actor.user_id();
    for principal in &f.principals {
        departments
            .clear_role(
                owner,
                f.role.company().as_uuid(),
                f.role.department,
                DepartmentRole::Reviewer,
                *principal,
            )
            .await
            .unwrap();
    }
    let today = PrincipalId::new(f.role.admission.binding.fixture.principal(owner).await);
    departments
        .assign_role(
            owner,
            f.role.company().as_uuid(),
            f.role.department,
            DepartmentRole::Reviewer,
            today,
        )
        .await
        .unwrap();
    today
}

async fn child_decision(
    f: &PeopleFixture,
    root: RunId,
    child_version: VersionId,
) -> crate::application::workflow::decisions::DecisionView {
    let child: Uuid = sqlx::query_scalar(
        "SELECT id FROM workflow_runs WHERE company_id=$1 AND parent_run_id=$2 AND version_id=$3",
    )
    .bind(f.role.company().as_uuid())
    .bind(root.as_uuid())
    .bind(child_version.as_uuid())
    .fetch_one(f.role.p().pool())
    .await
    .unwrap();
    let decision: Uuid =
        sqlx::query_scalar("SELECT id FROM workflow_decisions WHERE company_id=$1 AND run_id=$2")
            .bind(f.role.company().as_uuid())
            .bind(child)
            .fetch_one(f.role.p().pool())
            .await
            .unwrap();
    f.role
        .p()
        .read_decision(
            f.role.company(),
            WorkflowActor::authenticated(f.users[0]).unwrap(),
            DecisionId(decision),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn workflow_role_reviewer_pinned_child_reused_step_reads_its_admission_capture() {
    let f = Box::pin(PeopleFixture::new(2)).await;
    let child_version = Box::pin(configure_child(&f)).await;
    let root = f.role.admitted("people-child").await;
    let today = replace_people(&f).await;
    Box::pin(drive(&f, async || {
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_decisions AS decision JOIN workflow_runs AS child ON child.company_id=decision.company_id AND child.id=decision.run_id WHERE child.company_id=$1 AND child.parent_run_id=$2)")
            .bind(f.role.company().as_uuid()).bind(root.as_uuid()).fetch_one(f.role.p().pool()).await.unwrap()
    })).await;
    let root_state: Value = sqlx::query_scalar(
        "SELECT to_jsonb(run) FROM workflow_runs AS run WHERE company_id=$1 AND id=$2",
    )
    .bind(f.role.company().as_uuid())
    .bind(root.as_uuid())
    .fetch_one(f.role.p().pool())
    .await
    .unwrap();
    assert_eq!(root_state["state"], "waiting", "{root_state}");
    assert_eq!(root_state["waiting_reason"], "child_run", "{root_state}");
    let view = child_decision(&f, root, child_version).await;
    assert_eq!(view.step.as_str(), "start");
    assert_eq!(view.reviewers.len(), 2);
    assert!(
        view.reviewers
            .iter()
            .all(|id| f.principals.contains(&PrincipalId::new(*id)))
    );
    assert!(!view.reviewers.contains(&today.as_uuid()));
    f.role
        .p()
        .submit_decision(SubmitDecision {
            company: f.role.company(),
            actor: WorkflowActor::authenticated(f.users[0]).unwrap(),
            decision: view.decision,
            command_key: DecisionCommandKey::parse("child-vote").unwrap(),
            expected_revision: view.revision,
            expected_artifact: view.artifact_digest,
            choice: view.choices[0].clone(),
            data: json!(null),
            feedback: String::new(),
        })
        .await
        .unwrap();
    Box::pin(drive(&f, async || {
        sqlx::query_scalar(
            "SELECT state='succeeded' FROM workflow_runs WHERE company_id=$1 AND id=$2",
        )
        .bind(f.role.company().as_uuid())
        .bind(root.as_uuid())
        .fetch_one(f.role.p().pool())
        .await
        .unwrap()
    }))
    .await;
    let succeeded: bool = sqlx::query_scalar(
        "SELECT state='succeeded' FROM workflow_runs WHERE company_id=$1 AND id=$2",
    )
    .bind(f.role.company().as_uuid())
    .bind(root.as_uuid())
    .fetch_one(f.role.p().pool())
    .await
    .unwrap();
    assert!(succeeded);
}
