use super::*;
use crate::application::workflow::binding::{EscalationHops, EscalationPolicy};

#[path = "follow_visibility_tests.rs"]
mod visibility;

async fn people() -> PeopleFixture {
    let people = Box::pin(PeopleFixture::with_tracking(
        1,
        json!(["reviewer"]),
        ReviewerTracking::FollowRole,
    ))
    .await;
    let role = &people.role;
    let binding = &role.admission.binding;
    let departments = crate::adapters::persistence::test_support::department_use_cases(role.p());
    departments
        .clear_role(
            binding.target.actor.user_id(),
            role.company().as_uuid(),
            role.department,
            DepartmentRole::Lead,
            role.principal,
        )
        .await
        .unwrap();
    departments
        .assign_role(
            binding.target.actor.user_id(),
            role.company().as_uuid(),
            role.department,
            DepartmentRole::Lead,
            people.principals[0],
        )
        .await
        .unwrap();
    let mut request = binding.request(Some(binding.state().await.revision));
    request.association = RelatedAssociation::Channel(RelatedChannelId::new(role.channel));
    request.escalation = Some(EscalationPolicy {
        notify: DepartmentRole::Lead,
        hops: EscalationHops::try_from(1).unwrap(),
    });
    binding.service().configure(request).await.unwrap();
    people
}

async fn fixture() -> FollowFixture {
    let people = Box::pin(people()).await;
    let run = people.role.admitted("empty-follow").await;
    Box::pin(FollowFixture::park(people, run)).await
}

async fn state(f: &FollowFixture) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_build_object('run',run.state, \
          'cases',(SELECT count(*) FROM workflow_escalation_cases WHERE company_id=$1), \
          'controls',(SELECT count(*) FROM workflow_escalation_control_events WHERE company_id=$1), \
          'state',progress.state,'reason',source.reason_code,'source',source.source_kind, \
          'execution',source.execution_id,'step',source.step_id,'decision',source.empty_decision_id, \
          'reviewers',(SELECT count(*) FROM workflow_decision_reviewers WHERE company_id=$1 AND decision_id=$3)) \
         FROM workflow_runs AS run LEFT JOIN workflow_escalation_cases AS source \
           ON source.company_id=run.company_id AND source.run_id=run.id \
         LEFT JOIN workflow_escalation_progress AS progress ON progress.company_id=source.company_id AND progress.case_id=source.id \
         WHERE run.company_id=$1 AND run.id=$2",
    )
    .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid()).bind(f.decision.0)
    .fetch_one(f.people.role.p().pool()).await.unwrap()
}

#[tokio::test]
async fn workflow_reviewer_follow_empty_case_is_atomic_and_repair_never_restarts_it() {
    let f = Box::pin(fixture()).await;
    let proposal = f.proposal().await;
    assert_eq!(state(&f).await["cases"], 0);
    f.clear(0).await;
    let empty = state(&f).await;
    assert_eq!(
        empty,
        json!({"run":"waiting","cases":1,"controls":1,"state":"pending",
        "reason":"decision.empty_reviewers","source":"empty_reviewers","execution":f.scope.execution.as_uuid(),
        "step":"start","decision":f.decision.0,"reviewers":0})
    );
    f.assign(0).await;
    let repaired = state(&f).await;
    assert_eq!(repaired["state"], "withdrawn");
    assert_eq!(repaired["reviewers"], 1);
    assert_eq!(f.proposal().await, proposal);
    f.clear(0).await;
    let again = state(&f).await;
    assert_eq!(again["state"], "withdrawn");
    assert_eq!(again["cases"], 1);
    assert_eq!(again["controls"], 1);
    assert_eq!(again["reviewers"], 0);
    assert_eq!(again["run"], "waiting");
}

#[tokio::test]
async fn workflow_reviewer_follow_initial_empty_park_creates_the_same_real_source() {
    let people = Box::pin(people()).await;
    let run = people.role.admitted("empty-before-park").await;
    crate::adapters::persistence::test_support::department_use_cases(people.role.p())
        .clear_role(
            people.role.admission.binding.target.actor.user_id(),
            people.role.company().as_uuid(),
            people.role.department,
            DepartmentRole::Reviewer,
            people.principals[0],
        )
        .await
        .unwrap();
    let f = Box::pin(FollowFixture::park(people, run)).await;
    let empty = state(&f).await;
    assert_eq!(empty["cases"], 1);
    assert_eq!(empty["state"], "pending");
    assert_eq!(empty["reviewers"], 0);
    assert_eq!(empty["decision"], f.decision.0.to_string());
    assert_eq!(empty["execution"], f.scope.execution.as_uuid().to_string());
}

#[tokio::test]
async fn workflow_reviewer_follow_repair_withdraws_real_escalation_notice_and_queued_email() {
    let f = Box::pin(fixture()).await;
    f.clear(0).await;
    let worker = f.notification_worker();
    for _ in 0..4 {
        worker.drain_once().await.unwrap();
    }
    let receipt: (Uuid, Uuid) = sqlx::query_as(
        "SELECT notification.id,receipt.email_delivery_id FROM workflow_escalation_person_receipts AS receipt \
         JOIN workflow_escalation_notices AS notice ON notice.company_id=receipt.company_id AND notice.id=receipt.notice_id \
         JOIN notifications AS notification ON notification.company_id=notice.company_id AND notification.event_id=notice.event_id \
         WHERE receipt.company_id=$1 AND receipt.outcome='email_queued'",
    )
    .bind(f.scope.company.as_uuid()).fetch_one(f.people.role.p().pool()).await.unwrap();
    f.assign(0).await;
    let outcomes: Value = sqlx::query_scalar(
        "SELECT jsonb_build_object('notice',notification.state,'delivery',delivery.status, \
           'reason',delivery.cancellation_reason,'requested',delivery.cancellation_requested_at IS NOT NULL, \
           'class',delivery.last_error_class,'attempts',delivery.attempt_count) FROM notifications AS notification \
         JOIN message_deliveries AS delivery ON delivery.company_id=notification.company_id AND delivery.id=$3 \
         WHERE notification.company_id=$1 AND notification.id=$2",
    )
    .bind(f.scope.company.as_uuid()).bind(receipt.0).bind(receipt.1)
    .fetch_one(f.people.role.p().pool()).await.unwrap();
    assert_eq!(
        outcomes,
        json!({"notice":"withdrawn","delivery":"dead_letter","reason":"question_closed",
        "requested":true,"class":"superseded","attempts":0})
    );
    assert_eq!(state(&f).await["state"], "withdrawn");
    let settlement: String = sqlx::query_scalar(
        "SELECT settlement FROM workflow_escalation_decisions WHERE company_id=$1",
    )
    .bind(f.scope.company.as_uuid())
    .fetch_one(f.people.role.p().pool())
    .await
    .unwrap();
    assert_eq!(settlement, "withdrawn");
    for _ in 0..2 {
        worker.drain_once().await.unwrap();
    }
    assert_eq!(state(&f).await["cases"], 1);
}

#[tokio::test]
async fn workflow_reviewer_follow_case_insert_failure_rolls_back_the_real_role_write() {
    let f = Box::pin(fixture()).await;
    let before = f.assignment().await;
    sqlx::raw_sql("CREATE FUNCTION follow_case_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected case failure'; END $$; \
        CREATE TRIGGER follow_case_fault BEFORE INSERT ON workflow_escalation_cases FOR EACH ROW EXECUTE FUNCTION follow_case_fault()")
        .execute(f.people.role.p().pool()).await.unwrap();
    let result =
        crate::adapters::persistence::test_support::department_use_cases(f.people.role.p())
            .clear_role(
                f.people.role.admission.binding.target.actor.user_id(),
                f.scope.company.as_uuid(),
                f.people.role.department,
                DepartmentRole::Reviewer,
                f.people.principals[0],
            )
            .await;
    assert!(result.is_err());
    assert_eq!(f.assignment().await, before);
    assert_eq!(state(&f).await["cases"], 0);
    let role_present: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM department_roles WHERE company_id=$1 AND department_id=$2 AND role='reviewer' AND principal_id=$3)")
        .bind(f.scope.company.as_uuid()).bind(f.people.role.department.as_uuid()).bind(f.people.principals[0].as_uuid())
        .fetch_one(f.people.role.p().pool()).await.unwrap();
    assert!(role_present);
}

#[tokio::test]
async fn workflow_reviewer_follow_repair_and_actual_notice_worker_compete_without_reactivation() {
    let f = Box::pin(fixture()).await;
    f.clear(0).await;
    let worker = f.notification_worker();
    let barrier = tokio::sync::Barrier::new(2);
    let repair = async {
        barrier.wait().await;
        f.assign(0).await;
    };
    let deliver = async {
        barrier.wait().await;
        worker.drain_once().await.unwrap();
    };
    tokio::join!(repair, deliver);
    for _ in 0..3 {
        worker.drain_once().await.unwrap();
    }
    let repaired = state(&f).await;
    assert_eq!(repaired["state"], "withdrawn");
    assert_eq!(repaired["cases"], 1);
    assert_eq!(repaired["reviewers"], 1);
    assert_eq!(repaired["run"], "waiting");
    let live: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_escalation_notices AS notice \
         JOIN notifications AS notification ON notification.company_id=notice.company_id AND notification.event_id=notice.event_id \
         WHERE notice.company_id=$1 AND notification.state='active'",
    )
    .bind(f.scope.company.as_uuid()).fetch_one(f.people.role.p().pool()).await.unwrap();
    assert_eq!(live, 0);
}

#[tokio::test]
async fn workflow_reviewer_follow_empty_case_sql_rejects_false_source_fields_and_repaired_reviewers()
 {
    let f = Box::pin(fixture()).await;
    f.clear(0).await;
    for field in ["reason", "execution", "step", "role"] {
        assert_source_rejected(&f, field).await;
    }
    f.assign(0).await;
    assert_source_rejected(&f, "nonempty").await;
}

async fn assert_source_rejected(f: &FollowFixture, field: &str) {
    let reason = if field == "reason" {
        "decision.forged"
    } else {
        "decision.empty_reviewers"
    };
    let step = if field == "step" { "forged" } else { "start" };
    let execution = (field != "execution").then_some(f.scope.execution.as_uuid());
    let error = sqlx::query(
        "INSERT INTO workflow_escalation_cases \
         (company_id,id,source_kind,run_id,execution_id,empty_decision_id,binding_id,binding_revision, \
          origin_department_id,policy,reason_code,step_id,role) \
         SELECT run.company_id,gen_random_uuid(),'empty_reviewers',run.id,$3,$4,run.binding_id,run.binding_revision, \
                follow.context_department_id,run.escalation,$5,$6,$7 \
         FROM workflow_runs AS run JOIN workflow_follow_decisions AS follow ON follow.company_id=run.company_id AND follow.run_id=run.id \
         WHERE run.company_id=$1 AND run.id=$2 \
         ON CONFLICT (company_id,run_id) WHERE run_id IS NOT NULL DO NOTHING",
    )
    .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid()).bind(execution).bind(f.decision.0)
    .bind(reason).bind(step).bind((field == "role").then_some("reviewer"))
    .execute(f.people.role.p().pool()).await.unwrap_err();
    let error = error.as_database_error().unwrap();
    assert_eq!(error.code().as_deref(), Some("23514"), "{field}");
    assert_eq!(
        error.constraint(),
        Some("workflow_escalation_case_guard"),
        "{field}"
    );
}
