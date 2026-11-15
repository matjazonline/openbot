use super::*;
use crate::{
    application::ports::integration::{BindingStatusChange, ChannelBindingPersistence},
    entities::{creation::CreationProvenance, transport::BindingStatus},
};

#[derive(Clone, Copy)]
enum First {
    Status,
    Message,
}

async fn competing(first: First) {
    let Some(f) = Fixture::isolated().await else {
        return;
    };
    let target = f.principal_of(f.owner).await;
    let binding = f
        .persistence
        .active_bindings_for_target(
            f.company.id,
            AddressTarget::Principal(PrincipalId::new(target)),
        )
        .await
        .unwrap()
        .remove(0);
    let service = Arc::new(threads(&f));
    let message = email(
        &f,
        target,
        &email_of(f.pool(), f.owner).await,
        "status-race",
        "root",
    )
    .await;
    let InboundPreflight::Accepted(prepared) = service.preflight_inbound(message).await.unwrap()
    else {
        panic!("preflight refused");
    };
    let change = BindingStatusChange {
        company_id: f.company.id,
        binding_id: binding.id,
        status: BindingStatus::Paused,
        reason: None,
        actor: CreationProvenance::system(),
    };
    let before = principal(&f, target).await.settings_revision;
    let mut gate = f.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM principals WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(f.company.id)
        .bind(target)
        .execute(&mut *gate)
        .await
        .unwrap();
    let persistence = f.persistence.clone();
    let status = async move { persistence.set_binding_status(change).await };
    let commit = async move { service.commit_prepared_inbound(*prepared).await };
    let (message, status) = match first {
        First::Status => {
            let status = tokio::spawn(status);
            wait_until_a_backend_is_blocked(f.pool()).await;
            (tokio::spawn(commit), status)
        }
        First::Message => {
            let message = tokio::spawn(commit);
            wait_until_a_backend_is_blocked(f.pool()).await;
            (message, tokio::spawn(status))
        }
    };
    gate.commit().await.unwrap();
    let (message, status) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        tokio::join!(message, status)
    })
    .await
    .expect("both production contenders must finish without a deadlock or timeout victim");
    assert_eq!(status.unwrap().unwrap().status, BindingStatus::Paused);
    let copies: i64 =
        sqlx::query_scalar("SELECT count(*) FROM thread_messages WHERE company_id=$1")
            .bind(f.company.id)
            .fetch_one(f.pool())
            .await
            .unwrap();
    match first {
        First::Status => {
            assert!(matches!(message.unwrap(), Err(AppError::Superseded(_))));
            assert_eq!(copies, 0);
        }
        First::Message => {
            assert!(message.unwrap().unwrap().accepted);
            assert_eq!(copies, 1);
        }
    }
    assert!(principal(&f, target).await.settings_revision > before);
}

#[tokio::test]
async fn direct_competing_binding_status_and_ingress_finish_in_both_lock_orders() {
    competing(First::Status).await;
    competing(First::Message).await;
}

#[tokio::test]
async fn direct_transport_binding_status_no_op_preserves_revision_and_audit() {
    let Some(f) = Fixture::isolated().await else {
        return;
    };
    let target = f.principal_of(f.owner).await;
    let binding = f
        .persistence
        .active_bindings_for_target(
            f.company.id,
            AddressTarget::Principal(PrincipalId::new(target)),
        )
        .await
        .unwrap()
        .remove(0);
    let change = BindingStatusChange {
        company_id: f.company.id,
        binding_id: binding.id,
        status: BindingStatus::Paused,
        reason: None,
        actor: CreationProvenance::system(),
    };
    f.persistence
        .set_binding_status(change.clone())
        .await
        .unwrap();
    let revision = principal(&f, target).await.settings_revision;
    let audit = f
        .persistence
        .list_binding_audit_events(f.company.id, binding.id, 100)
        .await
        .unwrap();
    f.persistence.set_binding_status(change).await.unwrap();
    assert_eq!(principal(&f, target).await.settings_revision, revision);
    assert_eq!(
        f.persistence
            .list_binding_audit_events(f.company.id, binding.id, 100)
            .await
            .unwrap(),
        audit
    );
}

#[tokio::test]
async fn direct_thread_paused_interface_is_undeliverable_and_reactivation_uses_current_revision() {
    let Some(f) = Fixture::isolated().await else {
        return;
    };
    let target = f.principal_of(f.owner).await;
    let binding = f
        .persistence
        .active_bindings_for_target(
            f.company.id,
            AddressTarget::Principal(PrincipalId::new(target)),
        )
        .await
        .unwrap()
        .remove(0);
    let mut change = BindingStatusChange {
        company_id: f.company.id,
        binding_id: binding.id,
        status: BindingStatus::Paused,
        reason: None,
        actor: CreationProvenance::system(),
    };
    f.persistence
        .set_binding_status(change.clone())
        .await
        .unwrap();
    let paused_revision = principal(&f, target).await.settings_revision;
    assert!(
        !threads(&f)
            .ingest_canonical(canonical(&f, target, f.owner).await)
            .await
            .unwrap()
            .accepted
    );
    change.status = BindingStatus::Active;
    f.persistence.set_binding_status(change).await.unwrap();
    assert!(principal(&f, target).await.settings_revision > paused_revision);
    assert!(
        threads(&f)
            .ingest_canonical(canonical(&f, target, f.owner).await)
            .await
            .unwrap()
            .accepted
    );
}
