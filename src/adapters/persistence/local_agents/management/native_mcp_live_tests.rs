use super::*;
use crate::application::workflow::{
    activation::{ActivationRequest, WorkflowJobId},
    lease::{ClaimedWorkflow, LeasePolicy, WorkflowLeases, WorkflowWorkerId},
    mcp::McpActionPreparation,
    polling::WorkflowStepKind,
};
use std::time::Duration;

pub(super) fn lease() -> LeasePolicy {
    LeasePolicy::new(Duration::from_secs(30)).unwrap()
}

pub(super) async fn claim(f: &McpSetup, run: crate::domain::workflow::RunId) -> ClaimedWorkflow {
    let (execution,job):(Uuid,Uuid)=sqlx::query_as("SELECT execution.id,job.id FROM workflow_executions AS execution
        JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id
        WHERE execution.company_id=$1 AND execution.run_id=$2")
        .bind(f.s.f.company).bind(run.as_uuid()).fetch_one(&f.s.f.database.pool).await.unwrap();
    f.s.f
        .persistence
        .claim_io(
            ActivationRequest {
                company: CompanyId::new(f.s.f.company),
                run,
                execution: crate::domain::workflow::ExecutionId::new(execution),
                job: WorkflowJobId(job),
            },
            WorkflowWorkerId(Uuid::new_v4()),
            lease(),
        )
        .await
        .unwrap()
        .expect("real broker worker claim")
}

#[tokio::test]
async fn native_mcp_real_claim_and_preparation_preserve_exact_contract_and_stop_after_grant_revocation()
 {
    let f = McpSetup::new().await;
    let p = f.s.f.persistence.as_ref();
    let response = p
        .native_mcp(&f.request(json!({"query":"authorized"})))
        .await
        .unwrap();
    let claim = Box::pin(claim(&f, response.run_id)).await;
    let request = Box::pin(p.mcp_step_request(&claim, &WorkflowStepKind("mcp.call".into())))
        .await
        .unwrap();
    assert_eq!(request.arguments, json!({"query":"authorized"}));
    assert_eq!(request.context.actor, f.s.f.owner);
    assert_eq!(request.scope.run, response.run_id);
    assert!(p.validate_io(claim.fence, lease()).await.unwrap());
    sqlx::query("UPDATE agent_binding_grants SET revoked_at=clock_timestamp() WHERE company_id=$1")
        .bind(f.s.f.company)
        .execute(&f.s.f.database.pool)
        .await
        .unwrap();
    assert!(
        !p.validate_io(claim.fence, lease()).await.unwrap(),
        "committed native grant loss must end heartbeat ownership"
    );
    assert!(
        Box::pin(p.mcp_step_request(&claim, &WorkflowStepKind("mcp.call".into())))
            .await
            .is_err()
    );
    assert_eq!(p.sweep_native_mcp(16).await.unwrap(), 1);
}

#[tokio::test]
async fn native_mcp_added_grant_and_machine_owner_locks_never_wait_behind_a_competitor() {
    for table in ["agent_binding_grants", "local_machines"] {
        let f = McpSetup::new().await;
        let p = f.s.f.persistence.as_ref();
        let response = p
            .native_mcp(&f.request(json!({"query":"contended"})))
            .await
            .unwrap();
        let claim = Box::pin(claim(&f, response.run_id)).await;
        let mut competitor = f.s.f.database.pool.begin().await.unwrap();
        sqlx::query(&format!(
            "SELECT company_id FROM {table} WHERE company_id=$1 FOR UPDATE"
        ))
        .bind(f.s.f.company)
        .execute(&mut *competitor)
        .await
        .unwrap();
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            Box::pin(p.mcp_step_request(&claim, &WorkflowStepKind("mcp.call".into()))),
        )
        .await
        .expect("added owner locks must use NOWAIT");
        let error = result
            .err()
            .expect("contended native authority must refuse");
        assert!(
            matches!(&error, AppError::ServiceUnavailable(_)),
            "{error:?}"
        );
        competitor.rollback().await.unwrap();
        assert!(
            Box::pin(p.mcp_step_request(&claim, &WorkflowStepKind("mcp.call".into())))
                .await
                .is_ok()
        );
        let markers: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM workflow_action_dispatches WHERE company_id=$1",
        )
        .bind(f.s.f.company)
        .fetch_one(&f.s.f.database.pool)
        .await
        .unwrap();
        assert_eq!(markers, 0);
    }
}
#[tokio::test]
async fn native_mcp_ingress_auth_and_admission_never_wait_for_current_source_owners() {
    let f = McpSetup::new().await;
    let p = f.s.f.persistence.as_ref();
    let authentication = crate::application::local_agents::harness::NativeBundleRequest {
        verifier: f.s.paired.candidate.credential.verifier(),
        operation: f.operation,
        identity: ConnectionIdentity::from_fence(f.fence).unwrap(),
    };
    for (table, column) in [
        ("companies", "id"),
        ("threads", "company_id"),
        ("agent_binding_grants", "company_id"),
        ("local_machines", "company_id"),
    ] {
        let mut competitor = f.s.f.database.pool.begin().await.unwrap();
        sqlx::query(&format!(
            "SELECT {column} FROM {table} WHERE {column}=$1 FOR UPDATE"
        ))
        .bind(f.s.f.company)
        .execute(&mut *competitor)
        .await
        .unwrap();
        let auth = tokio::time::timeout(
            Duration::from_secs(2),
            p.authorize_native_mcp(&authentication),
        )
        .await
        .expect("pre-body source authentication must not wait");
        assert!(matches!(auth, Err(AppError::ServiceUnavailable(_))));
        let call = tokio::time::timeout(
            Duration::from_secs(2),
            p.native_mcp(&f.request(json!({"query":"busy source"}))),
        )
        .await
        .expect("native admission must not wait");
        assert!(matches!(call, Err(AppError::ServiceUnavailable(_))));
        competitor.rollback().await.unwrap();
        p.authorize_native_mcp(&authentication).await.unwrap();
    }
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM native_mcp_calls WHERE company_id=$1")
            .bind(f.s.f.company)
            .fetch_one(&f.s.f.database.pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
    p.native_mcp(&f.request(json!({"query":"busy source"})))
        .await
        .unwrap();
}
