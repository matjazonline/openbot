use super::*;
use crate::application::ports::account_deletion::HostAccountDeletion;
use crate::application::ports::department::DepartmentRoleAssignment;
use crate::use_cases::company_invite::CompanyInvitePersistence;

struct SponsorPeople {
    people: PeopleFixture,
}
impl SponsorPeople {
    async fn new(follow: bool) -> Self {
        let tracking = if follow {
            ReviewerTracking::FollowRole
        } else {
            ReviewerTracking::Frozen
        };
        let people = Box::pin(PeopleFixture::with_tracking(
            2,
            json!(["sponsor"]),
            tracking,
        ))
        .await;
        let role = &people.role;
        for principal in &people.principals {
            crate::adapters::persistence::test_support::department_use_cases(role.p())
                .assign_role(
                    role.admission.binding.target.actor.user_id(),
                    role.company().as_uuid(),
                    role.department,
                    DepartmentRoleAssignment {
                        role: DepartmentRole::Sponsor,
                        principal: *principal,
                        decider: false,
                    },
                )
                .await
                .unwrap();
        }
        Self { people }
    }
    fn p(&self) -> &PostgresPersistence {
        self.people.role.p()
    }
    async fn park(&self, key: &str) -> RunId {
        let run = self.people.role.admitted(key).await;
        let scope = self.people.scope(run).await;
        self.p()
            .park_decision(scope, &notices(self.p()))
            .await
            .unwrap()
            .unwrap();
        run
    }
    fn worker(&self) -> crate::application::services::notification_worker::NotificationWorker {
        crate::adapters::persistence::test_support::notification_worker(
            self.p(),
            Default::default(),
        )
    }
    async fn drain(&self) {
        for _ in 0..4 {
            if self.worker().drain_once().await.unwrap() == 0 {
                return;
            }
        }
        panic!("owned information/reviewer queue did not drain");
    }
}

#[derive(Clone, Copy)]
enum Retirement {
    Member,
    Account,
}
#[derive(sqlx::FromRow)]
struct NoticeState {
    status: String,
    cancellation_reason: Option<String>,
}

#[tokio::test]
async fn workflow_role_sponsor_admission_information_retirement_cancels_pending_and_inflight_for_both_policies()
 {
    for follow in [false, true] {
        for retirement in [Retirement::Member, Retirement::Account] {
            let f = Box::pin(SponsorPeople::new(follow)).await;
            f.park("information-pending").await;
            f.park("information-sending").await;
            f.drain().await;
            let deliveries:Vec<Uuid>=sqlx::query_scalar("SELECT email_delivery_id FROM notifications WHERE company_id=$1 AND action_kind='decision_information' ORDER BY id")
                .bind(f.people.role.company().as_uuid()).fetch_all(f.p().pool()).await.unwrap();
            assert_eq!(deliveries.len(), 2);
            sqlx::query("UPDATE message_deliveries SET status='sending',execution_id=gen_random_uuid(),owner_worker_id=gen_random_uuid(),locked_at=clock_timestamp(),lock_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
                .bind(deliveries[1]).execute(f.p().pool()).await.unwrap();
            match retirement {
                Retirement::Member => f
                    .p()
                    .remove_member(f.people.role.company().as_uuid(), f.people.users[1])
                    .await
                    .unwrap(),
                Retirement::Account => f.p().delete_user_account(f.people.users[1]).await.unwrap(),
            }
            for (index, id) in deliveries.iter().enumerate() {
                let row: NoticeState = sqlx::query_as(
                    "SELECT status,cancellation_reason FROM message_deliveries WHERE id=$1",
                )
                .bind(id)
                .fetch_one(f.p().pool())
                .await
                .unwrap();
                assert_eq!(
                    row.status,
                    if index == 0 { "dead_letter" } else { "sending" }
                );
                assert_eq!(
                    row.cancellation_reason.as_deref(),
                    Some("recipient_revoked")
                );
            }
            let kept:i64=sqlx::query_scalar("SELECT count(*) FROM workflow_decision_reviewers WHERE company_id=$1 AND principal_id=$2")
                .bind(f.people.role.company().as_uuid()).bind(f.people.principals[0].as_uuid()).fetch_one(f.p().pool()).await.unwrap();
            assert_eq!(
                kept, 2,
                "retiring informed holder preserves actual reviewers"
            );
        }
    }
}

#[tokio::test]
async fn workflow_role_sponsor_admission_retired_information_event_cannot_project_or_hot_loop() {
    for follow in [false, true] {
        let f = Box::pin(SponsorPeople::new(follow)).await;
        f.park("retired-before-information-projection").await;
        f.p()
            .remove_member(f.people.role.company().as_uuid(), f.people.users[1])
            .await
            .unwrap();
        f.drain().await;
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM notifications WHERE company_id=$1 AND action_kind='decision_information'")
            .bind(f.people.role.company().as_uuid()).fetch_one(f.p().pool()).await.unwrap();
        assert_eq!(count, 0);
        assert_eq!(f.worker().drain_once().await.unwrap(), 0);
    }
}

#[tokio::test]
async fn department_sponsor_competing_retirement_and_admission_refuse_or_keep_one_frozen_decider() {
    let f = Box::pin(SponsorPeople::new(false)).await;
    let prepared = f.people.role.prepared("sponsor-retirement-race").await;
    let barrier = tokio::sync::Barrier::new(2);
    let (admitted, retired) = tokio::join!(
        async {
            barrier.wait().await;
            f.p().admit(&prepared).await
        },
        async {
            barrier.wait().await;
            f.p()
                .remove_member(f.people.role.company().as_uuid(), f.people.users[0])
                .await
        }
    );
    retired.unwrap();
    match admitted {
        Ok(_) => {
            let deciders:Vec<Uuid>=sqlx::query_scalar("SELECT principal_id FROM workflow_run_role_bindings WHERE company_id=$1 AND run_id=$2 AND role='sponsor' AND capacity='decides'")
                .bind(f.people.role.company().as_uuid()).bind(prepared.proposed_run_id().as_uuid()).fetch_all(f.p().pool()).await.unwrap();
            assert_eq!(deciders, vec![f.people.principals[0].as_uuid()]);
        }
        Err(AppError::WorkflowRole(refusal)) => assert_eq!(
            refusal.kind,
            crate::entities::department::RoleRefusalKind::NoDecider
        ),
        other => panic!("unexpected admission/retirement result: {other:?}"),
    }
    let current:Vec<bool>=sqlx::query_scalar("SELECT decider FROM department_roles WHERE company_id=$1 AND department_id=$2 AND role='sponsor'")
        .bind(f.people.role.company().as_uuid()).bind(f.people.role.department.as_uuid()).fetch_all(f.p().pool()).await.unwrap();
    assert_eq!(current, vec![false], "retirement promotes nobody");
}

#[tokio::test]
async fn workflow_role_sponsor_admission_information_projection_waits_for_account_before_company() {
    let f = Box::pin(SponsorPeople::new(false)).await;
    f.park("information-account-prefix").await;
    let mut held = f.p().pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(f.people.users[1])
        .execute(&mut *held)
        .await
        .unwrap();
    let worker = f.worker();
    let observation = async {
        crate::adapters::persistence::test_support::wait_until_backends_are_blocked(
            f.p().pool(),
            1,
        )
        .await;
        let mut probe = f.p().pool().begin().await.unwrap();
        sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE NOWAIT")
            .bind(f.people.role.company().as_uuid())
            .execute(&mut *probe)
            .await
            .expect(
                "information projection cannot hold company while awaiting the recipient account",
            );
        probe.rollback().await.unwrap();
        held.rollback().await.unwrap();
    };
    let (projected, ()) = tokio::join!(Box::pin(worker.drain_once()), observation);
    assert_eq!(projected.unwrap(), 1);
    assert_eq!(worker.drain_once().await.unwrap(), 0);
}

#[tokio::test]
async fn workflow_role_sponsor_admission_sealed_capacities_and_information_sources_reject_forgery()
{
    let f = Box::pin(SponsorPeople::new(false)).await;
    let run = f.park("sealed-sponsor-information").await;
    let owner = f.people.role.admission.binding.target.actor.user_id();
    let late = PrincipalId::new(
        f.people
            .role
            .admission
            .binding
            .fixture
            .principal(owner)
            .await,
    );
    let mut tx = f.p().pool().begin().await.unwrap();
    sqlx::query("INSERT INTO workflow_run_role_bindings(company_id,run_id,manifest_id,version_id,step,role,department_id,principal_id,capacity) SELECT company_id,run_id,manifest_id,version_id,step,role,department_id,$3,'informed' FROM workflow_run_role_bindings WHERE company_id=$1 AND run_id=$2 AND role='sponsor' LIMIT 1")
        .bind(f.people.role.company().as_uuid()).bind(run.as_uuid()).bind(late.as_uuid()).execute(&mut *tx).await.unwrap();
    let error = tx.commit().await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("workflow_guard:check_workflow_role_manifest")
    );
    let info: Uuid = sqlx::query_scalar(
        "SELECT id FROM workflow_decision_information_notices WHERE company_id=$1",
    )
    .bind(f.people.role.company().as_uuid())
    .fetch_one(f.p().pool())
    .await
    .unwrap();
    let error=sqlx::query("UPDATE workflow_decision_information_notices SET principal_id=$3 WHERE company_id=$1 AND id=$2")
        .bind(f.people.role.company().as_uuid()).bind(info).bind(f.people.principals[0].as_uuid()).execute(f.p().pool()).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("workflow_information_guard")
    );
    let mut tx = f.p().pool().begin().await.unwrap();
    let source = Uuid::new_v4();
    let event = Uuid::new_v4();
    sqlx::query("INSERT INTO notification_events(id,company_id,source_kind,source_id,action_kind,source_generation) VALUES($1,$2,'decision_information',$3,'decision_information',1)")
        .bind(event).bind(f.people.role.company().as_uuid()).bind(source).execute(&mut *tx).await.unwrap();
    let error=sqlx::query("INSERT INTO workflow_decision_information_notices(company_id,id,decision_id,principal_id,assignment_revision,event_id) SELECT company_id,$2,decision_id,$3,0,$4 FROM workflow_decision_information_notices WHERE company_id=$1 LIMIT 1")
        .bind(f.people.role.company().as_uuid()).bind(source).bind(f.people.principals[0].as_uuid()).bind(event).execute(&mut *tx).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("workflow_information_guard")
    );
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn workflow_escalation_default_sponsor_without_decider_skips_to_the_parent_lead() {
    use crate::application::ports::department::{DepartmentEdit, DepartmentWrite};
    let f = Box::pin(SponsorPeople::new(false)).await;
    let role = &f.people.role;
    let owner = role.admission.binding.target.actor.user_id();
    let owner_principal = PrincipalId::new(role.admission.binding.fixture.principal(owner).await);
    let departments = crate::adapters::persistence::test_support::department_use_cases(f.p());
    let parent = departments
        .create_department(
            owner,
            role.company().as_uuid(),
            DepartmentWrite {
                name: "Sponsor parent".into(),
                slug: "sponsor-parent".into(),
                ..Default::default()
            },
            Some(owner_principal),
        )
        .await
        .unwrap();
    let policy = crate::entities::workflow_escalation::EscalationPolicy {
        notify: DepartmentRole::Sponsor,
        hops: crate::entities::workflow_escalation::EscalationHops::try_from(2).unwrap(),
    };
    departments
        .update_department(
            owner,
            role.company().as_uuid(),
            role.department,
            DepartmentEdit {
                parent_id: Some(Some(parent.id)),
                escalation: Some(Some(policy)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let prepared = role.prepared("no-sponsor-decider-escalates").await;
    f.p()
        .remove_member(role.company().as_uuid(), f.people.users[0])
        .await
        .unwrap();
    assert!(
        matches!(f.p().admit(&prepared).await,Err(AppError::WorkflowRole(refusal))
        if refusal.kind==crate::entities::department::RoleRefusalKind::NoDecider)
    );
    f.drain().await;
    #[derive(sqlx::FromRow)]
    struct Hop {
        hop: i16,
        role: String,
        resolution: String,
        recipient_facts: Value,
    }
    let hops:Vec<Hop>=sqlx::query_as("SELECT hop,role,resolution,recipient_facts FROM workflow_run_escalations WHERE company_id=$1 ORDER BY hop")
        .bind(role.company().as_uuid()).fetch_all(f.p().pool()).await.unwrap();
    assert_eq!(hops.len(), 2);
    assert_eq!(
        (
            hops[0].hop,
            hops[0].role.as_str(),
            hops[0].resolution.as_str()
        ),
        (1, "sponsor", "no_decider")
    );
    assert_eq!(
        (
            hops[1].hop,
            hops[1].role.as_str(),
            hops[1].resolution.as_str()
        ),
        (2, "lead", "holders")
    );
    assert_eq!(
        hops[1].recipient_facts[0]["principal"],
        json!(owner_principal)
    );
}
