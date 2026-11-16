//! Historical command actors and captured source roles survive actual membership retirement.
use super::*;
use crate::adapters::persistence::PostgresPersistence;

async fn retire_source(
    p: &PostgresPersistence,
    company: CompanyId,
    actor: Uuid,
    principal: PrincipalId,
) {
    sqlx::query("UPDATE principals SET kind='external', user_id=NULL, updated_at=clock_timestamp() WHERE company_id=$1 AND user_id=$2")
        .bind(company.as_uuid()).bind(actor).execute(p.pool()).await.unwrap();
    let retired: Value = sqlx::query_scalar(
        "SELECT jsonb_build_object( \
           'kind', principal.kind, 'user', principal.user_id, \
           'historical_user', principal.historical_person_user_id, \
           'memberships', (SELECT count(*) FROM department_members AS member \
              WHERE member.company_id=principal.company_id AND member.principal_id=principal.id), \
           'roles', (SELECT count(*) FROM department_roles AS role \
              WHERE role.company_id=principal.company_id AND role.principal_id=principal.id), \
           'live_access', EXISTS(SELECT 1 FROM membership_of($3,$1))) \
         FROM principals AS principal WHERE principal.company_id=$1 AND principal.id=$2",
    )
    .bind(company.as_uuid())
    .bind(principal.as_uuid())
    .bind(actor)
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(
        retired,
        json!({"kind":"external","user":null,"historical_user":actor,
            "memberships":0,"roles":0,"live_access":false}),
        "retirement retains the lifetime principal and removes every live membership and role"
    );
}

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
    // Box the database probe so this nested workflow test does not duplicate its future frame.
    Box::pin(retire_source(f.p(), company, actor, principal)).await;
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
