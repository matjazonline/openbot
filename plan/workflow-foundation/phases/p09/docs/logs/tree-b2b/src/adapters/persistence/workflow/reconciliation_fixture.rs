//! An HTTP action whose outcome is unknown, with its run waiting on reconciliation, for the
//! reconciliation route tests outside this module (P08-D3 rev 2). It takes the same path as
//! `workflow_http_remote_failure_remains_unknown_without_second_entry`: the server answers 500
//! after the marker, so the outcome is unknown and retiring the job parks the run. Also one the
//! server accepted, for the run pages.
use super::super::action_outcome_support::retire;
use super::support::*;
use super::*;

/// How the operation was published: under a slot (`Connection { http }`) or as a registered tool
/// without one (`Local { http }`).
#[derive(Debug, Clone, Copy)]
pub(crate) enum HttpForm {
    Slot,
    Tool,
    PrivateChannelSlot,
}

/// The uncertain action, its identities, and the fixture's own database.
pub(crate) struct UncertainHttp {
    _setup: Setup,
    pub(crate) persistence: PostgresPersistence,
    pub(crate) company: Uuid,
    pub(crate) owner: Uuid,
    pub(crate) run: Uuid,
    pub(crate) execution: Uuid,
    pub(crate) invocation: Uuid,
    pub(crate) argument_digest: String,
    pub(crate) marker: Uuid,
    pub(crate) connection: Uuid,
}

pub(crate) async fn uncertain_http(form: HttpForm) -> UncertainHttp {
    let (origin, server) = server("HTTP/1.1 500 Error\r\nContent-Length: 0\r\n\r\n").await;
    let entrance = match form {
        HttpForm::Slot | HttpForm::PrivateChannelSlot => Entrance::Http,
        HttpForm::Tool => Entrance::Tool,
    };
    let scope = match form {
        HttpForm::PrivateChannelSlot => HttpTestScope::PrivateChannel,
        _ => HttpTestScope::Company,
    };
    let setup = Box::pin(setup_scoped(&origin, entrance, scope)).await;
    let failure = runtime(&setup)
        .run(&setup.kind, &setup.claim)
        .await
        .completed_result()
        .unwrap_err();
    server.await.unwrap();
    let p = setup.fixture.persistence.clone();
    let retired = retire(&p, &setup.claim, lease(), failure).await;
    assert_eq!(
        (
            retired.run_state.as_str(),
            retired.waiting_reason.as_deref()
        ),
        ("waiting", Some("reconciliation"))
    );
    let scope = setup.claim.fence.scope;
    let (marker, invocation, argument_digest): (Uuid, Uuid, String) = sqlx::query_as(
        "SELECT id, invocation_id, argument_digest FROM workflow_action_dispatches \
         WHERE company_id = $1 AND run_id = $2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    UncertainHttp {
        persistence: p,
        company: scope.company.as_uuid(),
        owner: setup.fixture.target.actor.user_id(),
        run: scope.run.as_uuid(),
        execution: scope.execution.as_uuid(),
        invocation,
        argument_digest,
        marker,
        connection: setup.connection.id,
        _setup: setup,
    }
}

/// An HTTP action the server accepted, with its committed receipt, for the run pages' tests: the
/// path of `workflow_http_real_publication_admission_receipt_restart_and_competitors`.
pub(crate) struct AcceptedHttp {
    _setup: Setup,
    pub(crate) persistence: PostgresPersistence,
    pub(crate) company: Uuid,
    pub(crate) owner: Uuid,
    pub(crate) run: Uuid,
    pub(crate) execution: Uuid,
}

pub(crate) async fn accepted_http() -> AcceptedHttp {
    let (origin, server) =
        server("HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\n{\"ok\":true}").await;
    let setup = Box::pin(setup(&origin, Entrance::Http)).await;
    runtime(&setup)
        .run(&setup.kind, &setup.claim)
        .await
        .completed_result()
        .unwrap();
    server.await.unwrap();
    let scope = setup.claim.fence.scope;
    AcceptedHttp {
        persistence: setup.fixture.persistence.clone(),
        company: scope.company.as_uuid(),
        owner: setup.fixture.target.actor.user_id(),
        run: scope.run.as_uuid(),
        execution: scope.execution.as_uuid(),
        _setup: setup,
    }
}
