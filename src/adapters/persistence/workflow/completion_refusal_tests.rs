use super::*;

fn wrong_fences(fence: WorkflowFence, other: ActivationRequest) -> Vec<WorkflowFence> {
    vec![
        WorkflowFence {
            worker: worker(),
            ..fence
        },
        WorkflowFence {
            generation: WorkflowGeneration(Uuid::new_v4()),
            ..fence
        },
        WorkflowFence {
            attempt: WorkflowAttempt(fence.attempt.0 + 1),
            ..fence
        },
        WorkflowFence {
            scope: ActivationRequest {
                company: CompanyId::new(Uuid::new_v4()),
                ..fence.scope
            },
            ..fence
        },
        WorkflowFence {
            scope: ActivationRequest {
                run: other.run,
                ..fence.scope
            },
            ..fence
        },
        WorkflowFence {
            scope: ActivationRequest {
                execution: other.execution,
                ..fence.scope
            },
            ..fence
        },
        WorkflowFence {
            scope: ActivationRequest {
                job: other.job,
                ..fence.scope
            },
            ..fence
        },
    ]
}

async fn sibling(f: &AdmissionFixture) -> ActivationRequest {
    let command = f.prepare(f.request("completion-sibling", f.manual())).await;
    f.persistence().admit(&command).await.unwrap();
    let job = sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
        .bind(command.first_execution_id().as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    ActivationRequest {
        company: command.company_id(),
        run: command.proposed_run_id(),
        execution: command.first_execution_id(),
        job: WorkflowJobId(job),
    }
}

#[tokio::test]
async fn workflow_completion_mixed_existing_id_refusal_before_and_after_commit() {
    let (f, claim) = claimed(chain()).await;
    let other = sibling(&f).await;
    for phase in ["live", "completed"] {
        if phase == "completed" {
            f.persistence()
                .complete_io(result(claim.fence))
                .await
                .unwrap()
                .unwrap();
        }
        let before = state(&f).await;
        for wrong in wrong_fences(claim.fence, other) {
            assert!(
                f.persistence()
                    .complete_io(result(wrong))
                    .await
                    .unwrap()
                    .is_none(),
                "{phase}: {wrong:?}"
            );
            assert_eq!(state(&f).await, before);
        }
    }
    sqlx::query("UPDATE workflow_runs SET state='cancelled' WHERE id=$1")
        .bind(claim.fence.scope.run.as_uuid())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    let before = state(&f).await;
    assert_eq!(
        f.persistence()
            .complete_io(result(claim.fence))
            .await
            .unwrap()
            .unwrap()
            .disposition,
        CommitDisposition::Replayed
    );
    assert_eq!(state(&f).await, before);
    sqlx::query("UPDATE workflow_runs SET deadline=clock_timestamp()+interval '30 milliseconds' WHERE id=$1").bind(claim.fence.scope.run.as_uuid()).execute(f.persistence().pool()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(40)).await;
    let before = state(&f).await;
    assert_eq!(
        f.persistence()
            .complete_io(result(claim.fence))
            .await
            .unwrap()
            .unwrap()
            .disposition,
        CommitDisposition::Replayed
    );
    assert_eq!(state(&f).await, before);
}

#[tokio::test]
async fn workflow_completion_unclaimed_pure_and_legacy_refuse() {
    for kind in ["context.load", "data.map"] {
        let source = serde_json::from_str(&registry::example(kind).unwrap().source).unwrap();
        let (f, scope) = fixture_source(source).await;
        let fence = WorkflowFence {
            scope,
            worker: worker(),
            generation: WorkflowGeneration(Uuid::new_v4()),
            attempt: WorkflowAttempt(1),
        };
        let before = state(&f).await;
        assert!(
            f.persistence()
                .complete_io(result(fence))
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(state(&f).await, before);
    }
    let (f, claim) = claimed(chain()).await;
    use crate::application::use_cases::channel::{ChannelPersistence, ChannelWrite};
    let channel = ChannelPersistence::create(
        f.persistence(),
        claim.fence.scope.company.as_uuid(),
        ChannelWrite {
            name: "Legacy completion fixture".into(),
            slug: "legacy-completion".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let legacy = Uuid::new_v4();
    sqlx::query("INSERT INTO background_tasks (id,company_id,channel_id,thread_id,correlation_id,task_type,payload) SELECT $2,company_id,$3,thread_id,correlation_id,'reply','{}' FROM background_tasks WHERE id=$1")
        .bind(claim.fence.scope.job.0).bind(legacy).bind(channel.id).execute(f.persistence().pool()).await.unwrap();
    let before = state(&f).await;
    let fence = WorkflowFence {
        scope: ActivationRequest {
            job: WorkflowJobId(legacy),
            ..claim.fence.scope
        },
        ..claim.fence
    };
    assert!(
        f.persistence()
            .complete_io(result(fence))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(state(&f).await, before);
}
