//! D1.3 decision-bound owners end to end: a protected action whose approval is rejected or
//! expires fails the run and starts the default ladder only on a binding that escalates. These
//! audit/reason pairs cannot be forged in the validator matrix, so the real owners prove them.
use super::*;
use crate::application::workflow::binding::{EscalationHops, EscalationPolicy};
use crate::application::workflow::waits::{WaitProgress, WorkflowWaits};

fn policy() -> EscalationPolicy {
    EscalationPolicy {
        notify: DepartmentRole::Reviewer,
        hops: EscalationHops::try_from(1).unwrap(),
    }
}

async fn outcome(f: &RoleAction) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_build_object('state',run.state, \
         'cases',(SELECT coalesce(jsonb_agg(jsonb_build_array(c.source_kind,c.reason_code,c.step_id)),'[]') \
                  FROM workflow_escalation_cases AS c WHERE c.company_id=run.company_id AND c.run_id=run.id), \
         'all_cases',(SELECT count(*) FROM workflow_escalation_cases WHERE company_id=run.company_id)) \
         FROM workflow_runs AS run WHERE run.company_id=$1 AND run.id=$2",
    )
    .bind(f.setup.fixture.target.company.as_uuid())
    .bind(f.setup.claim.fence.scope.run.as_uuid())
    .fetch_one(f.setup.fixture.persistence.pool())
    .await
    .unwrap()
}

fn expected(escalating: bool, reason: &str) -> Value {
    let cases = if escalating {
        json!([["run", reason, "start"]])
    } else {
        json!([])
    };
    json!({"state":"failed","cases":cases,"all_cases":usize::from(escalating)})
}

#[tokio::test]
async fn workflow_escalation_trigger_rejected_action_approval_escalates_only_an_escalating_binding()
{
    for escalating in [false, true] {
        let f = Box::pin(role_action_with(escalating.then(policy))).await;
        parked(&f.setup).await;
        let decision = view(&f.setup).await;
        let p = &f.setup.fixture.persistence;
        p.submit_decision(submission(&f.setup, &decision, "reject"))
            .await
            .unwrap();
        assert_eq!(
            outcome(&f).await,
            expected(escalating, "http.approval_rejected"),
            "escalating={escalating}"
        );
        // A replayed rejection returns its first result and opens nothing more.
        p.submit_decision(submission(&f.setup, &decision, "reject"))
            .await
            .unwrap();
        assert_eq!(
            outcome(&f).await,
            expected(escalating, "http.approval_rejected"),
            "replay escalating={escalating}"
        );
    }
}

#[tokio::test]
async fn workflow_escalation_trigger_expired_action_approval_escalates_only_an_escalating_binding()
{
    for escalating in [false, true] {
        let f = Box::pin(role_action_with(escalating.then(policy))).await;
        parked(&f.setup).await;
        let p = &f.setup.fixture.persistence;
        let scope = f.setup.claim.fence.scope;
        sqlx::query(
            "UPDATE workflow_waits SET created_at=clock_timestamp()-interval '2 seconds', \
             deadline=clock_timestamp()-interval '1 second' WHERE company_id=$1 AND run_id=$2",
        )
        .bind(scope.company.as_uuid())
        .bind(scope.run.as_uuid())
        .execute(p.pool())
        .await
        .unwrap();
        assert!(matches!(
            p.resume_wait(scope).await.unwrap(),
            WaitProgress::Expired(_)
        ));
        assert_eq!(
            outcome(&f).await,
            expected(escalating, "http.approval_expired"),
            "escalating={escalating}"
        );
        assert_eq!(view(&f.setup).await.state, "expired");
    }
}
