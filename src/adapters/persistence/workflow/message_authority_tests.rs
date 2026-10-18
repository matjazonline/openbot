//! 04.11-D2 §5.1 row 2 (R006 F015): a routing or authority refusal in publication, while no
//! marker exists, is Terminal `message.rejected` with nothing written, wherever it lands in the
//! attempt. With a marker the same refusal stays the retryable dispatch error.
use super::*;
use crate::adapters::persistence::workflow::{
    action_dispatch::PostgresActionDispatch, message::MessageActionAuthority,
};
use crate::adapters::protocols::email::EmailRenderer;
use crate::adapters::workflow_http::NoLocal;
use crate::application::transport::{DeliveryComposer, TransportRenderers};
use crate::application::workflow::actions::ActionDispatchRequest;
use crate::application::workflow::message::{
    MessageActions, MessageDispatch, MessageOrigin, MessagePlan, MessagePublication,
};
use crate::domain::workflow::{FailureClass, RetrySafety};

/// The production dispatch with SQL committed just before `observe` or `publish`: a revocation
/// landing between the steps of one attempt.
struct Revoking {
    inner: PostgresActionDispatch<MessageActionAuthority, NoLocal>,
    pool: sqlx::PgPool,
    before_observe: Option<String>,
    before_publish: Option<String>,
}
#[async_trait]
impl MessageDispatch for Revoking {
    async fn observe(
        &self,
        request: &ActionDispatchRequest,
        origin: MessageOrigin,
    ) -> AppResult<Option<MessagePublication>> {
        if let Some(sql) = &self.before_observe {
            sqlx::raw_sql(sql).execute(&self.pool).await.unwrap();
        }
        self.inner.observe(request, origin).await
    }
    async fn publish(
        &self,
        request: &ActionDispatchRequest,
        origin: MessageOrigin,
        plan: &MessagePlan,
    ) -> AppResult<MessagePublication> {
        if let Some(sql) = &self.before_publish {
            sqlx::raw_sql(sql).execute(&self.pool).await.unwrap();
        }
        self.inner.publish(request, origin, plan).await
    }
}

fn revoking(
    setup: &Setup,
    before_observe: Option<String>,
    before_publish: Option<String>,
) -> MessageActions<PostgresPersistence, Revoking> {
    let p = &setup.fixture.persistence;
    let renderers = Arc::new(
        TransportRenderers::new()
            .register(Arc::new(EmailRenderer::new(APP_DOMAIN)))
            .unwrap(),
    );
    MessageActions::new(
        p.clone(),
        Revoking {
            inner: PostgresActionDispatch::new(p.clone(), MessageActionAuthority, NoLocal),
            pool: p.pool().clone(),
            before_observe,
            before_publish,
        },
        DeliveryComposer::new(renderers, Arc::new(p.clone())),
        APP_DOMAIN.into(),
        lease(),
    )
}

/// The run actor (the owner) loses owner and admin authority.
fn demote(setup: &Setup) -> String {
    let company = setup.scope().company.as_uuid();
    format!(
        "UPDATE companies SET user_id='{user}' WHERE id='{company}'; \
         UPDATE company_members SET role='member' WHERE company_id='{company}' AND user_id='{owner}'",
        user = setup.user,
        owner = setup.owner,
    )
}

fn unseat(setup: &Setup) -> String {
    format!(
        "DELETE FROM company_members WHERE company_id='{}' AND user_id='{}'",
        setup.scope().company.as_uuid(),
        setup.user
    )
}

fn assert_rejected(outcome: WorkflowHandlerOutcome, case: &str) {
    let WorkflowHandlerOutcome::Completed(Err(failure)) = outcome else {
        panic!("{case}: the step fails");
    };
    assert_eq!(
        (failure.failure.code().as_str(), failure.failure.class()),
        ("message.rejected", FailureClass::Terminal),
        "{case}"
    );
    assert_eq!(failure.safety, RetrySafety::SafeToRetry, "{case}");
}

/// A demoted actor before the step (refused by request preparation), after preparation at
/// `observe`, or between `observe` and `publish`, and a recipient removed between route
/// resolution and the publication recheck: Terminal `message.rejected`, zero markers, messages,
/// deliveries and waits, and the attempt still the supervisor's.
#[tokio::test]
async fn workflow_message_publish_revoked_authority_is_terminal_rejected_before_any_marker() {
    for case in [
        "actor demoted before the step",
        "actor demoted before observe",
        "actor demoted before publish",
        "recipient removed before publish",
    ] {
        // The owner and the member are both email recipients, so either revocation applies.
        let setup = Box::pin(send(&[To::Channel, To::User])).await;
        let outcome = match case {
            "actor demoted before the step" => {
                sqlx::raw_sql(&demote(&setup))
                    .execute(setup.pool())
                    .await
                    .unwrap();
                setup.runtime().execute(&setup.kind, &setup.claim).await
            }
            "actor demoted before observe" => {
                revoking(&setup, Some(demote(&setup)), None)
                    .execute(&setup.kind, &setup.claim)
                    .await
            }
            "actor demoted before publish" => {
                revoking(&setup, None, Some(demote(&setup)))
                    .execute(&setup.kind, &setup.claim)
                    .await
            }
            _ => {
                revoking(&setup, None, Some(unseat(&setup)))
                    .execute(&setup.kind, &setup.claim)
                    .await
            }
        };
        assert_rejected(outcome, case);
        let run = setup.scope().run;
        assert_eq!(
            rows(setup.pool(), run).await,
            Rows {
                markers: 0,
                actions: 0,
                destinations: 0,
                deliveries: 0,
                receipts: 0,
                waits: 0,
                messages: 0
            },
            "{case}"
        );
        assert_eq!(
            retry_safe(setup.pool(), setup.scope()).await,
            None,
            "{case}"
        );
        let after = state(setup.pool(), setup.scope()).await;
        assert_eq!(
            (
                after.run.as_str(),
                after.job.as_str(),
                after.attempt.as_str()
            ),
            ("running", "processing", "processing"),
            "{case}"
        );
    }
}

/// The boundary: once the invocation has a marker (an in-app receipt committed, then the worker
/// died), a revocation landing after preparation, at the replay's `observe`, is the retryable
/// dispatch error, and nothing new is written. (A demotion before the step is refused by the
/// shared request preparation, `http_runtime::facts`, as for every action kind.)
#[tokio::test]
async fn workflow_message_publish_revocation_after_a_marker_stays_retryable() {
    let setup = Box::pin(send(&[To::Channel])).await;
    let p = &setup.fixture.persistence;
    assert!(matches!(
        setup.runtime().execute(&setup.kind, &setup.claim).await,
        WorkflowHandlerOutcome::Completed(Ok(_))
    ));
    assert!(
        p.release_io(
            setup.claim.fence,
            lease(),
            LeaseReleaseCause::LocalInterruption
        )
        .await
        .unwrap()
    );
    sqlx::query("UPDATE background_tasks SET run_at=clock_timestamp() WHERE id=$1")
        .bind(setup.scope().job.0)
        .execute(setup.pool())
        .await
        .unwrap();
    let retried = p
        .claim_io(setup.scope(), WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .expect("the retried job is claimable");
    let before = rows(setup.pool(), setup.scope().run).await;
    let WorkflowHandlerOutcome::Completed(Err(failure)) =
        revoking(&setup, Some(demote(&setup)), None)
            .execute(&setup.kind, &retried)
            .await
    else {
        panic!("the replay is refused");
    };
    assert_eq!(
        (failure.failure.code().as_str(), failure.failure.class()),
        ("message.unresolved", FailureClass::Retryable)
    );
    assert_eq!(rows(setup.pool(), setup.scope().run).await, before);
}
