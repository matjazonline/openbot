//! Historical command actors and captured source roles survive actual membership retirement.
use super::*;

#[tokio::test]
async fn workflow_escalation_decision_raw_source_member_retirement_preserves_command_and_role_history()
 {
    let (f, decision) = Box::pin(pending(1)).await;
    let actor = f.people.users[0];
    let principal = f.people.principals[0];
    let company = f.scope.company;
    let captured: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_run_role_bindings WHERE company_id=$1 AND principal_id=$2",
    )
    .bind(company.as_uuid())
    .bind(principal.as_uuid())
    .fetch_one(f.p().pool())
    .await
    .unwrap();
    assert!(
        captured > 0,
        "exercise a principal actually sealed into the source run"
    );
    f.p()
        .submit_escalation(command(
            &f,
            decision,
            actor,
            "historical-ack",
            EscalationChoice::Acknowledge,
        ))
        .await
        .unwrap();
    sqlx::query("DELETE FROM company_members WHERE company_id=$1 AND user_id=$2")
        .bind(company.as_uuid())
        .bind(actor)
        .execute(f.p().pool())
        .await
        .unwrap();
    let remains: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM principals WHERE company_id=$1 AND id=$2)")
            .bind(company.as_uuid())
            .bind(principal.as_uuid())
            .fetch_one(f.p().pool())
            .await
            .unwrap();
    assert!(
        !remains,
        "actual raw membership retirement deleted the live principal"
    );
    let retained: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_run_role_bindings WHERE company_id=$1 AND principal_id=$2",
    )
    .bind(company.as_uuid())
    .bind(principal.as_uuid())
    .fetch_one(f.p().pool())
    .await
    .unwrap();
    assert_eq!(retained, captured);
    let owner = f.people.role.admission.binding.target.actor.user_id();
    let saved = view(&f, decision, owner).await;
    let audit = saved.command.unwrap();
    assert_eq!(
        (audit.actor_user, audit.actor_principal),
        (actor, principal)
    );
    assert_eq!(audit.choice, EscalationChoice::Acknowledge);
    assert!(
        f.p()
            .read_escalation(
                company,
                WorkflowActor::authenticated(actor).unwrap(),
                decision
            )
            .await
            .is_err()
    );
}
