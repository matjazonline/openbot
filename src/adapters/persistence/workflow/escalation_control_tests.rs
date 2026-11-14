use super::*;
use crate::application::notification::{
    NotificationPersistence, NotificationProjectionCommit, NotificationProjectionCommitOutcome,
};
use crate::application::workflow::binding::EscalationAckHours;
use crate::entities::notification::NotificationDisposition;
use crate::transport::WorkerId;
use std::time::Duration;

fn worker(
    f: &RoleFixture,
    hours: &str,
) -> crate::application::services::notification_worker::NotificationWorker {
    crate::adapters::persistence::test_support::notification_worker(
        f.p(),
        EscalationAckHours::parse(Some(hours)).unwrap(),
    )
}

async fn refused(hops: u8) -> RoleFixture {
    let f = Box::pin(fixture()).await;
    if hops == 1 {
        let binding = &f.admission.binding;
        let state = binding.state().await;
        let mut request = binding.request(Some(state.revision));
        request.association = RelatedAssociation::Channel(RelatedChannelId::new(f.channel));
        request.escalation = Some(EscalationPolicy {
            notify: DepartmentRole::Reviewer,
            hops: EscalationHops::try_from(1).unwrap(),
        });
        binding.service().configure(request).await.unwrap();
    }
    let command = f.prepared("empty-ladder").await;
    clear(&f).await;
    assert_refused(f.p().admit(&command).await);
    f
}

async fn finish(f: &RoleFixture) {
    let changed_configuration = worker(f, "168");
    for _ in 0..4 {
        if changed_configuration.drain_once().await.unwrap() == 0 {
            return;
        }
    }
    panic!("a two-hop empty ladder must exhaust its control events");
}

#[tokio::test]
async fn workflow_escalation_control_actual_worker_advances_empty_hops_once_and_captures_the_first_window()
 {
    for hops in [1, 2] {
        let f = Box::pin(refused(hops)).await;
        assert_eq!(worker(&f, "1").drain_once().await.unwrap(), 1);
        finish(&f).await;
        let progress: Value = sqlx::query_scalar(
            "SELECT jsonb_build_object('state',state,'hop',current_hop,'hours',ack_hours) \
            FROM workflow_escalation_progress WHERE company_id=$1",
        )
        .bind(f.company().as_uuid())
        .fetch_one(f.p().pool())
        .await
        .unwrap();
        assert_eq!(
            progress,
            json!({"state":"owner_waiting","hop":hops,"hours":1})
        );
        assert_eq!(count(&f, "workflow_run_escalations").await, i64::from(hops));
        assert_eq!(
            count(&f, "workflow_escalation_decisions").await,
            i64::from(hops)
        );
        assert_eq!(
            count(&f, "workflow_escalation_control_events").await,
            i64::from(hops) + 1
        );
        for table in [
            "workflow_escalation_notices",
            "notifications",
            "message_deliveries",
            "workflow_runs",
        ] {
            assert_eq!(
                count(&f, table).await,
                0,
                "empty recipients manufacture no {table}"
            );
        }
        let exact:bool=sqlx::query_scalar("SELECT bool_and(deadline=created_at+INTERVAL '1 hour' AND recipient_facts='[]'::jsonb \
            AND reason_code='workflow.role_no_holder') FROM workflow_run_escalations WHERE company_id=$1")
            .bind(f.company().as_uuid()).fetch_one(f.p().pool()).await.unwrap();
        assert!(
            exact,
            "retry/config change cannot renew the captured acknowledgement window or change the original failure"
        );
        let events:bool=sqlx::query_scalar("SELECT bool_and(status='projected' AND attempt_count=1) FROM notification_events WHERE company_id=$1")
            .bind(f.company().as_uuid()).fetch_one(f.p().pool()).await.unwrap();
        assert!(events);
        assert_eq!(worker(&f, "24").drain_once().await.unwrap(), 0);
    }
}

#[tokio::test]
async fn workflow_escalation_control_competing_actual_workers_leave_one_hop_per_generation() {
    let f = Box::pin(refused(2)).await;
    let first = worker(&f, "1");
    let second = worker(&f, "1");
    let barrier = tokio::sync::Barrier::new(3);
    let (a, b, ()) = tokio::join!(
        async {
            barrier.wait().await;
            first.drain_once().await.unwrap()
        },
        async {
            barrier.wait().await;
            second.drain_once().await.unwrap()
        },
        async {
            barrier.wait().await;
        }
    );
    assert!(a + b >= 1);
    finish(&f).await;
    assert_eq!(count(&f, "workflow_run_escalations").await, 2);
    assert_eq!(count(&f, "workflow_escalation_decisions").await, 2);
    assert_eq!(count(&f, "notification_events").await, 3);
    let state: String =
        sqlx::query_scalar("SELECT state FROM workflow_escalation_progress WHERE company_id=$1")
            .bind(f.company().as_uuid())
            .fetch_one(f.p().pool())
            .await
            .unwrap();
    assert_eq!(state, "owner_waiting");
}

#[tokio::test]
async fn workflow_escalation_control_lost_lease_after_hop_writes_rolls_back_every_control_effect() {
    let f = Box::pin(refused(2)).await;
    sqlx::raw_sql(
        "CREATE SEQUENCE escalation_hop_reached; CREATE FUNCTION slow_escalation_test() RETURNS trigger LANGUAGE plpgsql AS $$ \
        BEGIN PERFORM nextval('escalation_hop_reached'); PERFORM pg_sleep(1.1); RETURN NEW; END $$; \
        CREATE TRIGGER slow_escalation_test AFTER INSERT ON workflow_run_escalations \
        FOR EACH ROW EXECUTE FUNCTION slow_escalation_test()",
    )
    .execute(f.p().pool())
    .await
    .unwrap();
    let claimed = f
        .p()
        .claim_notification_events(WorkerId::random(), Duration::from_secs(1), 1)
        .await
        .unwrap()
        .pop()
        .unwrap();
    let expected = f
        .p()
        .resolve_notification_projection(&claimed.event)
        .await
        .unwrap();
    assert_eq!(expected.disposition, NotificationDisposition::Control);
    let (outcome, _) = f
        .p()
        .commit_notification_projection(NotificationProjectionCommit {
            fence: &claimed.lease,
            expected: &expected,
            email: None,
            agent_message_key: None,
            escalation_ack_hours: EscalationAckHours::parse(Some("1")).unwrap(),
        })
        .await
        .unwrap();
    assert_eq!(outcome, NotificationProjectionCommitOutcome::LeaseLost);
    // Sequence advancement survives rollback and proves the AFTER-hop boundary was reached.
    let reached: bool = sqlx::query_scalar("SELECT is_called FROM escalation_hop_reached")
        .fetch_one(f.p().pool())
        .await
        .unwrap();
    assert!(
        reached,
        "expiry before the write does not prove rollback after effects"
    );
    for table in [
        "workflow_run_escalations",
        "workflow_escalation_decisions",
        "workflow_escalation_notices",
    ] {
        assert_eq!(count(&f, table).await, 0, "lost fence rolls back {table}");
    }
    assert_eq!(count(&f, "notification_events").await, 1);
    let progress: Value = sqlx::query_scalar(
        "SELECT jsonb_build_object('state',state,'hop',current_hop,'hours',ack_hours) \
        FROM workflow_escalation_progress WHERE company_id=$1",
    )
    .bind(f.company().as_uuid())
    .fetch_one(f.p().pool())
    .await
    .unwrap();
    assert_eq!(progress, json!({"state":"pending","hop":0,"hours":null}));
}
