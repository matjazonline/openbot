use super::*;

#[tokio::test]
async fn workflow_reviewer_principal_visible_ordinary_member_parks_and_submits_business_decision() {
    // Keep the shared fixture's state machine off the stock test-thread stack.
    let (f, _) = Box::pin(channel_fixture()).await;
    let reviewer = f.users[0];
    let owner = f.admission.binding.target.actor.user_id();
    assert_ne!(reviewer, owner);
    sqlx::query("DELETE FROM department_roles AS holder USING department_members AS member WHERE holder.company_id=member.company_id AND holder.department_id=member.department_id AND holder.principal_id=member.principal_id AND holder.role='admin' AND member.company_id=$1 AND member.root_user_id=$2")
        .bind(f.scope.company.as_uuid())
        .bind(reviewer)
        .execute(f.persistence().pool())
        .await
        .unwrap();
    let role: String = sqlx::query_scalar(
        "SELECT role FROM membership_of($2, $1) WHERE company_id=$1 AND user_id=$2",
    )
    .bind(f.scope.company.as_uuid())
    .bind(reviewer)
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert_eq!(role, "member");
    let view = f.park().await;
    assert_eq!(
        view.subject,
        crate::application::workflow::decisions::DecisionSubject::Business
    );
    assert!(view.can_submit);
    assert_eq!(
        view.reviewers,
        vec![f.admission.binding.fixture.principal(reviewer).await]
    );
    assert!(!view.reviewers.contains(&reviewer));
    let submitted = f
        .persistence()
        .submit_decision(f.submit(&view, 0, "ordinary-business-vote"))
        .await
        .unwrap();
    assert_eq!(submitted.disposition, DecisionDisposition::Applied);
    assert!(submitted.successor.is_some());
    let saved = f
        .persistence()
        .read_decision(f.scope.company, f.actor(0), view.decision)
        .await
        .unwrap();
    assert_eq!(saved.state, "completed");
    assert!(!saved.can_submit);
}
