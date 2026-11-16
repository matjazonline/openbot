use super::*;
use crate::{
    application::ports::integration::ChannelBindingPersistence,
    entities::{
        address::AddressTarget,
        transport::{BindingChangeReason, BindingStatus, ChannelBinding, ChannelBindingId},
    },
};

async fn active(f: &Fixture, principal: Uuid) -> ChannelBinding {
    let bindings = f
        .persistence
        .active_bindings_for_target(
            f.company.id,
            AddressTarget::Principal(PrincipalId::new(principal)),
        )
        .await
        .unwrap();
    assert_eq!(bindings.len(), 1);
    bindings.into_iter().next().unwrap()
}

async fn saved(f: &Fixture, id: ChannelBindingId) -> ChannelBinding {
    f.persistence
        .get_binding(f.company.id, id)
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn direct_transport_binding_person_creation_and_rename_keep_one_real_interface() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (user, name) = account(&f.persistence, "personal_endpoint").await;
    let person = f.join(user, &name).await;
    let initial = active(&f, person).await;
    assert_eq!(
        initial.target,
        AddressTarget::Principal(PrincipalId::new(person))
    );
    assert!(initial.installation_id.is_none());
    let slug = ChannelSlug::from("renamed-person");
    f.persistence
        .rename_person_address(user, f.company.id, &slug)
        .await
        .unwrap()
        .unwrap();
    let renamed = active(&f, person).await;
    assert_eq!(renamed.id, initial.id);
    assert_eq!(renamed.namespace, initial.namespace);
    assert_eq!(
        renamed.external_endpoint_key.as_str(),
        crate::adapters::protocols::email::EmailEndpointKey::canonical(&slug)
            .unwrap()
            .as_str()
    );
    let desks: i64 = sqlx::query_scalar("SELECT count(*) FROM channels WHERE company_id=$1")
        .bind(f.company.id)
        .fetch_one(f.pool())
        .await
        .unwrap();
    assert_eq!(desks, 0, "a personal transport interface creates no desk");
    let audits = f
        .persistence
        .list_binding_audit_events(f.company.id, initial.id, 20)
        .await
        .unwrap();
    assert_eq!(audits.len(), 2);
}

#[tokio::test]
async fn direct_transport_binding_retirement_rejoin_release_endpoint_and_reuse_lifetime_binding() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (user, name) = account(&f.persistence, "lifetime_endpoint").await;
    let person = f.join(user, &name).await;
    let original = active(&f, person).await;
    let slug = ChannelSlug::from(f.addresses_of(person).await[0].0.clone());
    f.persistence
        .remove_member(f.company.id, user)
        .await
        .unwrap();
    let retired = saved(&f, original.id).await;
    assert_eq!(retired.status, BindingStatus::Orphaned);
    assert_eq!(
        retired.disabled_reason,
        Some(BindingChangeReason::PrincipalRetired)
    );
    assert!(
        f.persistence
            .active_bindings_for_target(f.company.id, retired.target)
            .await
            .unwrap()
            .is_empty()
    );

    let (other, other_name) = account(&f.persistence, "endpoint_successor").await;
    let successor = f.join(other, &other_name).await;
    f.persistence
        .rename_person_address(other, f.company.id, &slug)
        .await
        .unwrap()
        .unwrap();
    let replacement = active(&f, successor).await;
    assert_eq!(
        replacement.external_endpoint_key,
        original.external_endpoint_key
    );
    assert_ne!(replacement.id, original.id);

    assert_eq!(f.join(user, &name).await, person);
    let restored = active(&f, person).await;
    assert_eq!(restored.id, original.id);
    assert_eq!(restored.status, BindingStatus::Active);
    assert_eq!(restored.disabled_reason, None);
    assert_ne!(
        restored.external_endpoint_key,
        original.external_endpoint_key
    );
    assert_eq!(active(&f, successor).await.id, replacement.id);
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM channel_bindings WHERE company_id=$1 AND principal_id=$2",
    )
    .bind(f.company.id)
    .bind(person)
    .fetch_one(f.pool())
    .await
    .unwrap();
    assert_eq!(count, 1);
    let audits = f
        .persistence
        .list_binding_audit_events(f.company.id, original.id, 20)
        .await
        .unwrap();
    assert_eq!(
        audits
            .iter()
            .filter(|event| event.reason == Some(BindingChangeReason::PrincipalRetired))
            .count(),
        1
    );
}

struct InvalidTarget {
    principal: Option<Uuid>,
    desk: Option<Uuid>,
    constraint: &'static str,
}

async fn invalid_copy(
    f: &Fixture,
    source: ChannelBindingId,
    target: &InvalidTarget,
) -> sqlx::Error {
    sqlx::query(
        "INSERT INTO channel_bindings(id, company_id, channel_id, principal_id, installation_id, \
         transport, namespace, external_endpoint_key, display_label, access_policy, delivery_policy, \
         status, created_by, access_snapshot) \
         SELECT $1, company_id, $3, $4, installation_id, transport, namespace, $5, display_label, \
         access_policy, delivery_policy, 'active', created_by, access_snapshot FROM channel_bindings \
         WHERE company_id=$2 AND id=$6",
    ).bind(Uuid::new_v4()).bind(f.company.id).bind(target.desk).bind(target.principal)
        .bind(format!("invalid-{}", Uuid::new_v4().simple())).bind(source.as_uuid())
        .execute(f.pool()).await.unwrap_err()
}

#[tokio::test]
async fn direct_transport_binding_native_owner_kind_tenant_and_shape_guards() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let Some(foreign) = Fixture::new().await else {
        return;
    };
    let person = f.principal_of(f.owner).await;
    let binding = active(&f, person).await;
    let outsider = Uuid::new_v4();
    sqlx::query("INSERT INTO principals(id,company_id,kind,display_label) VALUES($1,$2,'external','Outside')")
        .bind(outsider).bind(f.company.id).execute(f.pool()).await.unwrap();
    let desk = f.desk("native-mixed-target", &[]).await.unwrap();
    for target in [
        InvalidTarget {
            principal: None,
            desk: None,
            constraint: "channel_bindings_target_check",
        },
        InvalidTarget {
            principal: Some(person),
            desk: Some(desk),
            constraint: "channel_bindings_target_check",
        },
        InvalidTarget {
            principal: Some(outsider),
            desk: None,
            constraint: "channel_bindings_principal_kind",
        },
        InvalidTarget {
            principal: Some(foreign.principal_of(foreign.owner).await),
            desk: None,
            constraint: "channel_bindings_principal_kind",
        },
        InvalidTarget {
            principal: Some(person),
            desk: None,
            constraint: "channel_bindings_principal_key",
        },
    ] {
        let error = invalid_copy(&f, binding.id, &target).await;
        assert_eq!(
            error.as_database_error().unwrap().constraint(),
            Some(target.constraint)
        );
    }
    let error = sqlx::query("UPDATE channel_bindings SET access_policy='conversation_members_read_and_participate' WHERE company_id=$1 AND id=$2")
        .bind(f.company.id).bind(binding.id.as_uuid()).execute(f.pool()).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("channel_bindings_principal_email_check")
    );
    let error = sqlx::query("UPDATE channel_bindings SET principal_id=NULL, channel_id=$3 WHERE company_id=$1 AND id=$2")
        .bind(f.company.id).bind(binding.id.as_uuid()).bind(desk).execute(f.pool()).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("channel_bindings_principal_owner")
    );
}

#[tokio::test]
async fn direct_transport_binding_unused_principal_delete_cascades_only_its_own_endpoint() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let owner = active(&f, f.principal_of(f.owner).await).await;
    let (user, name) = account(&f.persistence, "unused_endpoint").await;
    let person = f.join(user, &name).await;
    let unused = active(&f, person).await;
    sqlx::query("DELETE FROM principals WHERE company_id=$1 AND id=$2")
        .bind(f.company.id)
        .bind(person)
        .execute(f.pool())
        .await
        .unwrap();
    assert!(
        f.persistence
            .get_binding(f.company.id, unused.id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(saved(&f, owner.id).await.id, owner.id);
}
