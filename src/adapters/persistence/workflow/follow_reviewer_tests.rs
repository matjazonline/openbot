use super::*;
use crate::application::workflow::reviewer_assignment::ReviewerAssignment;
use std::collections::BTreeSet;

#[path = "follow_bound_tests.rs"]
mod bounds;
#[path = "follow_escalation_tests.rs"]
mod escalations;

struct FollowFixture {
    people: PeopleFixture,
    scope: ActivationRequest,
    decision: DecisionId,
}
impl FollowFixture {
    async fn new(count: usize) -> Self {
        let people = Box::pin(PeopleFixture::with_tracking(
            count,
            json!(["reviewer"]),
            ReviewerTracking::FollowRole,
        ))
        .await;
        let run = people.role.admitted("follow").await;
        Self::park(people, run).await
    }
    async fn park(people: PeopleFixture, run: RunId) -> Self {
        let scope = people.scope(run).await;
        assert!(
            people
                .role
                .p()
                .park_decision(scope, &notices(people.role.p()))
                .await
                .unwrap()
                .is_some()
        );
        let decision = DecisionId(
            sqlx::query_scalar(
                "SELECT id FROM workflow_decisions WHERE company_id=$1 AND run_id=$2",
            )
            .bind(scope.company.as_uuid())
            .bind(run.as_uuid())
            .fetch_one(people.role.p().pool())
            .await
            .unwrap(),
        );
        Self {
            people,
            scope,
            decision,
        }
    }
    async fn assignment(&self) -> ReviewerAssignment {
        let value:Value=sqlx::query_scalar("SELECT assignment.content FROM workflow_follow_decisions AS current \
            JOIN workflow_reviewer_assignments AS assignment USING(company_id,decision_id,assignment_revision) \
            WHERE current.company_id=$1 AND current.decision_id=$2")
            .bind(self.scope.company.as_uuid()).bind(self.decision.0).fetch_one(self.people.role.p().pool()).await.unwrap();
        ReviewerAssignment::decode(&serde_json::to_vec(&value).unwrap()).unwrap()
    }
    async fn proposal(&self) -> Value {
        sqlx::query_scalar("SELECT jsonb_build_object('digest',decision.proposal_digest,'revision',decision.revision, \
            'inputs',execution.frozen_inputs,'binding_revision',run.binding_revision,'bundle_digest',run.bundle_digest, \
            'wait',decision.wait_id,'deadline',wait.deadline) FROM workflow_decisions AS decision \
            JOIN workflow_executions AS execution ON execution.company_id=decision.company_id AND execution.id=decision.execution_id \
            JOIN workflow_runs AS run ON run.company_id=decision.company_id AND run.id=decision.run_id \
            JOIN workflow_waits AS wait ON wait.company_id=decision.company_id AND wait.id=decision.wait_id \
            WHERE decision.company_id=$1 AND decision.id=$2")
            .bind(self.scope.company.as_uuid()).bind(self.decision.0).fetch_one(self.people.role.p().pool()).await.unwrap()
    }
    async fn clear(&self, index: usize) {
        crate::adapters::persistence::test_support::department_use_cases(self.people.role.p())
            .clear_role(
                self.people.role.admission.binding.target.actor.user_id(),
                self.scope.company.as_uuid(),
                self.people.role.department,
                DepartmentRole::Reviewer,
                self.people.principals[index],
            )
            .await
            .unwrap();
    }
    async fn assign(&self, index: usize) {
        crate::adapters::persistence::test_support::department_use_cases(self.people.role.p())
            .assign_role(
                self.people.role.admission.binding.target.actor.user_id(),
                self.scope.company.as_uuid(),
                self.people.role.department,
                DepartmentRole::Reviewer,
                self.people.principals[index],
            )
            .await
            .unwrap();
    }
    async fn vote(&self, index: usize) -> SubmitDecision {
        let actor = WorkflowActor::authenticated(self.people.users[index]).unwrap();
        let view = self
            .people
            .role
            .p()
            .read_decision(self.scope.company, actor, self.decision)
            .await
            .unwrap();
        SubmitDecision {
            company: self.scope.company,
            actor,
            decision: self.decision,
            command_key: DecisionCommandKey::parse(format!("vote-{index}")).unwrap(),
            expected_revision: view.revision,
            expected_artifact: view.artifact_digest,
            choice: view.choices[0].clone(),
            feedback: String::new(),
            data: json!(null),
        }
    }
}

#[tokio::test]
async fn workflow_reviewer_follow_real_role_writer_replaces_only_current_rows_and_preserves_proposal()
 {
    let f = Box::pin(FollowFixture::new(2)).await;
    let before = f.assignment().await;
    let proposal = f.proposal().await;
    assert_eq!(before.assignment_revision.get(), 1);
    assert!(
        before
            .roles
            .iter()
            .all(|reference| u64::from(reference.resolution_revision) == 0)
    );
    let retained = before
        .reviewers
        .iter()
        .find(|reviewer| reviewer.principal_id == f.people.principals[1])
        .unwrap();
    f.clear(0).await;
    let after = f.assignment().await;
    assert_eq!(after.assignment_revision.get(), 2);
    assert_eq!(after.reviewers, std::slice::from_ref(retained));
    assert_eq!(f.proposal().await, proposal);
    let view = f
        .people
        .role
        .p()
        .read_decision(
            f.scope.company,
            WorkflowActor::authenticated(f.people.users[0]).unwrap(),
            f.decision,
        )
        .await
        .unwrap();
    assert!(!view.can_submit);
    assert_eq!(view.reviewers, vec![f.people.principals[1].as_uuid()]);
    let rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_reviewer_assignments WHERE company_id=$1 AND decision_id=$2",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.decision.0)
    .fetch_one(f.people.role.p().pool())
    .await
    .unwrap();
    assert_eq!(rows, 2);
    let notices: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_reviewer_notices WHERE company_id=$1 AND decision_id=$2",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.decision.0)
    .fetch_one(f.people.role.p().pool())
    .await
    .unwrap();
    assert_eq!(notices, 2, "unchanged reviewer is not notified twice");
    let members: BTreeSet<_> = after
        .reviewers
        .iter()
        .map(|reviewer| reviewer.principal_id)
        .collect();
    assert_eq!(members, BTreeSet::from([f.people.principals[1]]));
    let removed = f.vote(0).await;
    assert!(
        f.people.role.p().submit_decision(removed).await.is_err(),
        "removed current reviewer cannot vote"
    );
    assert_eq!(
        f.proposal().await,
        proposal,
        "the rejected vote changed nothing"
    );
    f.people
        .role
        .p()
        .submit_decision(f.vote(1).await)
        .await
        .unwrap();
    f.clear(1).await;
    assert_eq!(
        f.assignment().await,
        after,
        "settled decisions never follow later role writes"
    );
    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_pending_follow_decisions WHERE company_id=$1",
    )
    .bind(f.scope.company.as_uuid())
    .fetch_one(f.people.role.p().pool())
    .await
    .unwrap();
    assert_eq!(pending, 0);
}

#[tokio::test]
async fn workflow_reviewer_follow_empty_role_stays_parked_and_repair_has_a_fresh_notice() {
    let f = Box::pin(FollowFixture::new(1)).await;
    let before = f.assignment().await;
    let proposal = f.proposal().await;
    f.clear(0).await;
    let empty = f.assignment().await;
    assert!(empty.reviewers.is_empty());
    assert_eq!(empty.refusal,Some(crate::application::workflow::reviewer_assignment::ReviewerAssignmentRefusal::RoleUnavailable));
    assert_eq!(f.proposal().await, proposal);
    let state: String =
        sqlx::query_scalar("SELECT state FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(f.scope.company.as_uuid())
            .bind(f.scope.run.as_uuid())
            .fetch_one(f.people.role.p().pool())
            .await
            .unwrap();
    assert_eq!(state, "waiting");
    f.assign(0).await;
    let repaired = f.assignment().await;
    assert_eq!(repaired.assignment_revision.get(), 3);
    assert_eq!(repaired.reviewers.len(), 1);
    assert_ne!(
        repaired.reviewers[0].notice_event_id,
        before.reviewers[0].notice_event_id
    );
    assert_eq!(repaired.reviewers[0].notice_assignment_revision.get(), 3);
    assert_eq!(f.proposal().await, proposal);
    let facts:Vec<Value>=sqlx::query_scalar("SELECT content FROM workflow_people_role_resolutions WHERE company_id=$1 AND root_run_id=$2 ORDER BY resolution_revision")
        .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid()).fetch_all(f.people.role.p().pool()).await.unwrap();
    assert_eq!(facts.len(), 2);
    assert_eq!(facts[0]["principals"], json!([]));
    assert_eq!(facts[0]["refusal"], "no_holder");
    assert_eq!(facts[1]["principals"], json!([f.people.principals[0]]));
}

impl FollowFixture {
    fn notification_worker(
        &self,
    ) -> crate::application::services::notification_worker::NotificationWorker {
        crate::adapters::persistence::test_support::notification_worker(
            self.people.role.p(),
            Default::default(),
        )
    }

    async fn set_email_preference(&self, index: usize, enabled: bool) {
        sqlx::query("INSERT INTO user_notification_preferences(user_id,decision_email_enabled) VALUES($1,$2) \
            ON CONFLICT(user_id) DO UPDATE SET decision_email_enabled=EXCLUDED.decision_email_enabled")
            .bind(self.people.users[index]).bind(enabled).execute(self.people.role.p().pool()).await.unwrap();
    }
    async fn receipt_outcomes(&self) -> Vec<String> {
        sqlx::query_scalar("SELECT outcome FROM workflow_reviewer_notice_receipts WHERE company_id=$1 ORDER BY outcome")
            .bind(self.scope.company.as_uuid()).fetch_all(self.people.role.p().pool()).await.unwrap()
    }
}

#[tokio::test]
async fn workflow_reviewer_follow_real_notification_worker_proves_opt_out_open_and_withdrawal() {
    use crate::application::notification::NotificationPersistence;
    use crate::entities::notification::NotificationId;
    let f = Box::pin(FollowFixture::new(2)).await;
    f.set_email_preference(1, false).await;
    let worker = f.notification_worker();
    assert_eq!(worker.drain_once().await.unwrap(), 2);
    let errors: Vec<Option<String>> = sqlx::query_scalar(
        "SELECT last_error_detail FROM notification_events WHERE company_id=$1 ORDER BY id",
    )
    .bind(f.scope.company.as_uuid())
    .fetch_all(f.people.role.p().pool())
    .await
    .unwrap();
    assert_eq!(
        f.receipt_outcomes().await,
        vec!["delivered", "opted_out"],
        "{errors:?}"
    );
    let in_app: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM notifications WHERE company_id=$1 AND state='active'",
    )
    .bind(f.scope.company.as_uuid())
    .fetch_one(f.people.role.p().pool())
    .await
    .unwrap();
    assert_eq!(in_app, 2, "email opt-out keeps the in-app notice");
    let principal = f.people.principals[0];
    let user = f.people.users[0];
    let notification: Uuid = sqlx::query_scalar(
        "SELECT id FROM notifications WHERE company_id=$1 AND recipient_principal_id=$2",
    )
    .bind(f.scope.company.as_uuid())
    .bind(principal.as_uuid())
    .fetch_one(f.people.role.p().pool())
    .await
    .unwrap();
    let href = f
        .people
        .role
        .p()
        .open_notification(
            f.scope.company.as_uuid(),
            user,
            principal,
            &[f.people.role.channel],
            NotificationId::new(notification),
        )
        .await
        .unwrap();
    assert_eq!(href, format!("/ui/workflow-decisions/{}", f.decision.0));
    let read: bool = sqlx::query_scalar(
        "SELECT read_at IS NOT NULL FROM notifications WHERE company_id=$1 AND id=$2",
    )
    .bind(f.scope.company.as_uuid())
    .bind(notification)
    .fetch_one(f.people.role.p().pool())
    .await
    .unwrap();
    assert!(read);
    f.clear(0).await;
    let state: String =
        sqlx::query_scalar("SELECT state FROM notifications WHERE company_id=$1 AND id=$2")
            .bind(f.scope.company.as_uuid())
            .bind(notification)
            .fetch_one(f.people.role.p().pool())
            .await
            .unwrap();
    assert_eq!(state, "withdrawn");
    let cancelled:bool=sqlx::query_scalar("SELECT bool_and(cancellation_reason='recipient_revoked' AND cancellation_requested_at IS NOT NULL AND status='dead_letter') FROM message_deliveries WHERE company_id=$1 AND purpose='notification'")
        .bind(f.scope.company.as_uuid()).fetch_one(f.people.role.p().pool()).await.unwrap();
    let delivery_states:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'status',status,'reason',cancellation_reason,'requested',cancellation_requested_at IS NOT NULL) FROM message_deliveries WHERE company_id=$1 AND purpose='notification'")
        .bind(f.scope.company.as_uuid()).fetch_all(f.people.role.p().pool()).await.unwrap();
    assert!(
        cancelled,
        "a removed reviewer must not receive queued email: {delivery_states:?}"
    );
    assert!(
        f.people
            .role
            .p()
            .open_notification(
                f.scope.company.as_uuid(),
                user,
                principal,
                &[f.people.role.channel],
                NotificationId::new(notification)
            )
            .await
            .is_err()
    );
    assert_eq!(
        worker.drain_once().await.unwrap(),
        0,
        "no assignment notice is resent"
    );
    f.people
        .role
        .p()
        .submit_decision(f.vote(1).await)
        .await
        .unwrap();
    let active: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM notifications WHERE company_id=$1 AND state='active'",
    )
    .bind(f.scope.company.as_uuid())
    .fetch_one(f.people.role.p().pool())
    .await
    .unwrap();
    assert_eq!(active, 0, "settlement closes followed notices");
    assert_eq!(
        f.receipt_outcomes().await,
        vec!["delivered", "opted_out"],
        "immutable notice proofs survive withdrawal"
    );
}

#[path = "follow_retirement_tests.rs"]
mod retirement;
