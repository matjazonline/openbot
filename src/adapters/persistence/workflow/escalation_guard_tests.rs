use super::*;
use crate::application::app_error::WorkflowGuardViolation;
use crate::application::workflow::binding::{EscalationHops, EscalationPolicy};
use crate::application::workflow::guard::{
    GuardFailure, GuardFailureOutcome, GuardSite, WorkflowGuardFailure,
};

#[path = "escalation_command_tests.rs"]
mod commands;
#[path = "escalation_deadline_tests.rs"]
mod deadlines;
#[path = "escalation_decision_deadline_tests.rs"]
mod decision_deadlines;
#[path = "escalation_recovery_tests.rs"]
mod recovery_triggers;
#[path = "escalation_source_guard_tests.rs"]
mod source_guards;
#[path = "escalation_terminal_owner_tests.rs"]
mod terminal_owners;
#[path = "escalation_wait_deadline_tests.rs"]
mod wait_deadlines;

struct GuardFixture {
    people: PeopleFixture,
    scope: ActivationRequest,
}

fn policy() -> EscalationPolicy {
    EscalationPolicy {
        notify: DepartmentRole::Reviewer,
        hops: EscalationHops::try_from(1).unwrap(),
    }
}

impl GuardFixture {
    async fn new(escalation: Option<EscalationPolicy>) -> Self {
        let people = Box::pin(PeopleFixture::new(1)).await;
        // Keep composed fixture futures off the native 2 MiB test stack.
        Box::pin(Self::admit(people, escalation, None)).await
    }

    async fn admit(
        people: PeopleFixture,
        escalation: Option<EscalationPolicy>,
        version: Option<VersionId>,
    ) -> Self {
        let binding = &people.role.admission.binding;
        let mut request = binding.request(Some(binding.state().await.revision));
        request.version = version.unwrap_or(request.version);
        request.association =
            RelatedAssociation::Channel(RelatedChannelId::new(people.role.channel));
        request.escalation = escalation;
        binding.service().configure(request).await.unwrap();
        let run = people.role.admitted("guard-escalation").await;
        let scope = people.scope(run).await;
        Self { people, scope }
    }

    fn p(&self) -> &PostgresPersistence {
        self.people.role.p()
    }

    fn failure(&self) -> GuardFailure {
        GuardFailure {
            scope: self.scope,
            violation: WorkflowGuardViolation::from_constraint(
                "workflow_guard:test_escalation_guard",
                "forced guard",
            )
            .unwrap(),
            site: GuardSite::Step,
            fence: None,
        }
    }

    fn worker(&self) -> crate::application::services::notification_worker::NotificationWorker {
        crate::adapters::persistence::test_support::notification_worker(
            self.p(),
            Default::default(),
        )
    }

    async fn state(&self) -> Value {
        sqlx::query_scalar(
            "SELECT jsonb_build_object('state',run.state, \
             'runs',(SELECT count(*) FROM workflow_runs WHERE company_id=$1), \
             'cases',(SELECT count(*) FROM workflow_escalation_cases WHERE company_id=$1), \
             'controls',(SELECT count(*) FROM workflow_escalation_control_events WHERE company_id=$1), \
             'hops',(SELECT count(*) FROM workflow_run_escalations WHERE company_id=$1), \
             'notices',(SELECT count(*) FROM workflow_escalation_notices WHERE company_id=$1), \
             'guard_events',(SELECT count(*) FROM workflow_run_events WHERE company_id=$1 AND run_id=$2 AND event_kind='guard_violation:test_escalation_guard')) \
             FROM workflow_runs AS run WHERE run.company_id=$1 AND run.id=$2",
        )
        .bind(self.scope.company.as_uuid())
        .bind(self.scope.run.as_uuid())
        .fetch_one(self.p().pool())
        .await
        .unwrap()
    }
}

#[tokio::test]
async fn workflow_escalation_trigger_guard_commits_the_real_failed_run_and_delivers_once() {
    let f = Box::pin(GuardFixture::new(Some(policy()))).await;
    assert_eq!(
        f.p().fail_run_on_guard(f.failure()).await.unwrap(),
        GuardFailureOutcome::Failed
    );
    let source: Value = sqlx::query_scalar(
        "SELECT jsonb_build_object('run',run_id,'execution',execution_id,'step',step_id,'reason',reason_code, \
         'department',origin_department_id,'policy',policy) FROM workflow_escalation_cases WHERE company_id=$1",
    )
    .bind(f.scope.company.as_uuid())
    .fetch_one(f.p().pool())
    .await
    .unwrap();
    assert_eq!(
        source,
        json!({
            "run":f.scope.run.as_uuid(),"execution":f.scope.execution.as_uuid(),"step":"start",
            "reason":"workflow.guard_violation","department":f.people.role.department,
            "policy":{"notify":"reviewer","hops":1}
        })
    );
    assert_eq!(
        f.state().await,
        json!({"state":"failed","runs":1,"cases":1,"controls":1,"hops":0,"notices":0,"guard_events":1})
    );
    assert_eq!(f.worker().drain_once().await.unwrap(), 1);
    assert_eq!(f.worker().drain_once().await.unwrap(), 1);
    assert_eq!(f.worker().drain_once().await.unwrap(), 0);
    assert_eq!(
        f.p().fail_run_on_guard(f.failure()).await.unwrap(),
        GuardFailureOutcome::Inactive
    );
    assert_eq!(
        f.state().await,
        json!({"state":"failed","runs":1,"cases":1,"controls":2,"hops":1,"notices":1,"guard_events":1})
    );
    let delivered: bool = sqlx::query_scalar(
        "SELECT count(*)=1 AND bool_and(receipt.outcome='email_queued' AND notification.state='active' \
          AND notification.recipient_principal_id=$2 AND receipt.email_delivery_id=notification.email_delivery_id) \
         FROM workflow_escalation_person_receipts AS receipt JOIN notifications AS notification \
           ON notification.company_id=receipt.company_id AND notification.event_id=receipt.event_id WHERE receipt.company_id=$1",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.people.principals[0].as_uuid())
    .fetch_one(f.p().pool())
    .await
    .unwrap();
    assert!(
        delivered,
        "the actual worker projected exactly one durable person notice for the failed run"
    );
}

#[tokio::test]
async fn workflow_escalation_competing_guard_owners_create_one_source_and_control() {
    let f = Box::pin(GuardFixture::new(Some(policy()))).await;
    let barrier = tokio::sync::Barrier::new(3);
    let first = async {
        barrier.wait().await;
        f.p().fail_run_on_guard(f.failure()).await.unwrap()
    };
    let second = async {
        barrier.wait().await;
        f.p().fail_run_on_guard(f.failure()).await.unwrap()
    };
    let (first, second, _) = tokio::join!(first, second, barrier.wait());
    assert!(matches!(
        (first, second),
        (GuardFailureOutcome::Failed, GuardFailureOutcome::Inactive)
            | (GuardFailureOutcome::Inactive, GuardFailureOutcome::Failed)
    ));
    assert_eq!(
        f.state().await,
        json!({"state":"failed","runs":1,"cases":1,"controls":1,"hops":0,"notices":0,"guard_events":1})
    );
}

#[tokio::test]
async fn workflow_escalation_trigger_guard_case_failure_rolls_back_terminal_retirement() {
    let f = Box::pin(GuardFixture::new(Some(policy()))).await;
    let before = f.state().await;
    sqlx::raw_sql(
        "CREATE FUNCTION reject_guard_case_test() RETURNS trigger LANGUAGE plpgsql AS $$ \
         BEGIN PERFORM 1/0; RETURN NEW; END $$; \
         CREATE TRIGGER reject_guard_case_test BEFORE INSERT ON workflow_escalation_cases \
         FOR EACH ROW EXECUTE FUNCTION reject_guard_case_test()",
    )
    .execute(f.p().pool())
    .await
    .unwrap();
    let error = f.p().fail_run_on_guard(f.failure()).await.unwrap_err();
    assert!(
        format!("{error:?}").contains("division by zero"),
        "{error:?}"
    );
    assert_eq!(f.state().await, before);
    let pending: bool =
        sqlx::query_scalar("SELECT status='pending' FROM background_tasks WHERE id=$1")
            .bind(f.scope.job.0)
            .fetch_one(f.p().pool())
            .await
            .unwrap();
    assert!(
        pending,
        "case failure rolled back the real job retirement too"
    );
    sqlx::raw_sql(
        "DROP TRIGGER reject_guard_case_test ON workflow_escalation_cases; DROP FUNCTION reject_guard_case_test()",
    )
    .execute(f.p().pool())
    .await
    .unwrap();
    assert_eq!(
        f.p().fail_run_on_guard(f.failure()).await.unwrap(),
        GuardFailureOutcome::Failed
    );
}

#[tokio::test]
async fn workflow_escalation_trigger_guard_without_an_opted_in_policy_creates_no_case() {
    let f = Box::pin(GuardFixture::new(None)).await;
    assert_eq!(
        f.p().fail_run_on_guard(f.failure()).await.unwrap(),
        GuardFailureOutcome::Failed
    );
    assert_eq!(
        f.state().await,
        json!({"state":"failed","runs":1,"cases":0,"controls":0,"hops":0,"notices":0,"guard_events":1})
    );
}
