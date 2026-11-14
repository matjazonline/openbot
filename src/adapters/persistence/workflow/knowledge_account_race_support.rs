//! C2 real preparation/mutation helpers; the adversarial storage seam is explicitly separate.
use super::*;
use crate::adapters::persistence::workflow::tests::approval_test_support::composer;
use crate::entities::value_objects::EmailAddress;
use crate::use_cases::user::{
    AccountChange, AccountChangeKind, AccountChangePersistence, PendingAccountChange,
    auth_attempt_key,
};

pub(super) struct Prepared {
    pub action: FrozenAction,
    pub plan: MessagePlan,
}

pub(super) async fn prepare(setup: &Setup) -> Prepared {
    let p = &setup.fixture.persistence;
    let request = p
        .message_step_request(&setup.claim, &setup.kind)
        .await
        .unwrap();
    let kind = MessageKind::parse(setup.kind.0.as_str()).unwrap();
    let arguments = MessageArguments::parse(kind, &request.arguments).unwrap();
    let intent = ActionService::new(p.clone())
        .prepare_step(request.clone())
        .await
        .unwrap();
    let authority = p
        .action_authority(request.scope, &intent.approval_subject())
        .await
        .unwrap();
    let routes = p.message_routes(&request, &arguments).await.unwrap();
    let plan = MessagePlanner::new(composer(p), APP_DOMAIN.into())
        .plan(
            MessagePublicationIdentity {
                company: request.scope.company,
                invocation: intent.approval_subject().invocation,
            },
            &arguments,
            &routes,
        )
        .await
        .unwrap();
    Prepared {
        action: authority.action,
        plan,
    }
}

pub(super) struct MemberGate {
    pub transaction: sqlx::Transaction<'static, sqlx::Postgres>,
    pub backend: i32,
}

pub(super) async fn member_gate(setup: &Setup) -> MemberGate {
    let mut transaction = setup.pool().begin().await.unwrap();
    let backend = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM company_members WHERE company_id=$1 AND user_id=$2 FOR UPDATE")
        .bind(setup.scope().company.as_uuid())
        .bind(setup.user)
        .execute(&mut *transaction)
        .await
        .unwrap();
    MemberGate {
        transaction,
        backend,
    }
}

/// Deliberate raw-storage adversary, not the application's company-serialized address owner.
/// Install valid proof for the NEW address while the prepared route still names the OLD one.
pub(super) async fn adversarial_proof_change(setup: &Setup) {
    let address = format!("adversarial-{}@example.test", Uuid::new_v4().simple());
    let mut tx = setup.pool().begin().await.unwrap();
    sqlx::query("UPDATE users SET email=$2,email_verified_at=clock_timestamp() WHERE id=$1")
        .bind(setup.user)
        .bind(&address)
        .execute(&mut *tx)
        .await
        .unwrap();
    let principal: Uuid = sqlx::query_scalar(
        "SELECT id FROM principals WHERE company_id=$1 AND user_id=$2 AND kind='person'",
    )
    .bind(setup.scope().company.as_uuid())
    .bind(setup.user)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let identity = crate::application::ports::thread::qualified_email_identity(&address).unwrap();
    crate::adapters::persistence::participant::upsert_account_identity_on(
        &mut tx,
        setup.scope().company.as_uuid(),
        crate::entities::transport::PrincipalId::new(principal),
        &identity,
        "recipient",
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

pub(super) async fn confirm_address(setup: &Setup, old: &EmailAddress) -> EmailAddress {
    let address = EmailAddress::from(format!(
        "confirmed-{}@example.test",
        Uuid::new_v4().simple()
    ));
    let p = &setup.fixture.persistence;
    p.save_pending_account_change(PendingAccountChange {
        user_id: setup.user,
        change: AccountChange::Email(&address),
        confirmation_code_hash: "C2-test-code",
        expires_at: chrono::Utc::now() + chrono::Duration::minutes(15),
    })
    .await
    .unwrap();
    let key = auth_attempt_key(
        "C2-test-secret",
        "account-change-code:v1",
        &[
            setup.user.as_bytes(),
            AccountChangeKind::Email.as_str().as_bytes(),
        ],
    );
    let user = p
        .confirm_pending_account_change(setup.user, AccountChangeKind::Email, "C2-test-code", &key)
        .await
        .unwrap()
        .expect("the actual confirmed-address owner commits");
    assert_eq!(user.email, address.as_str());
    assert!(user.email_verified_at.is_some());
    let retired: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM participant_identities AS identity JOIN principals AS principal \
           ON principal.company_id=identity.company_id AND principal.id=identity.principal_id \
         WHERE identity.company_id=$1 AND identity.subject=$2 AND principal.kind='external')",
    )
    .bind(setup.scope().company.as_uuid()).bind(old.as_str()).fetch_one(setup.pool()).await.unwrap();
    assert!(
        retired,
        "the real rekey retired the old normalized person identity"
    );
    address
}

pub(super) async fn assert_no_effects(setup: &Setup) {
    let counts = rows(setup.pool(), setup.scope().run).await;
    assert_eq!(counts.markers, 0);
    assert_eq!(counts.deliveries, 0);
    assert_eq!(counts.messages, 0);
    assert_eq!(counts.receipts, 0);
}

pub(super) async fn account_writer_wait(pool: &sqlx::PgPool, publisher: i32) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let blocked: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity AS activity \
                WHERE $1=ANY(pg_blocking_pids(activity.pid)) AND activity.wait_event_type='Lock' \
                  AND (activity.query LIKE 'SELECT company.id FROM companies AS company WHERE%' \
                    OR activity.query LIKE 'SELECT email::text AS%FROM users WHERE id =%FOR UPDATE%'))",
            )
            .bind(publisher)
            .fetch_one(pool)
            .await
            .unwrap();
            if blocked {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("real confirmed-address writer waits on publication's account/company prefix");
}
