use super::*;

#[tokio::test]
async fn channel_visibility_is_independent_of_management_access() {
    let company_id = company();
    let version_id = version();
    let store = MemoryStore::new(owned(company_id, version_id, 128));
    let principal = PrincipalId::new(Uuid::new_v4());
    set_membership(
        &store,
        company_id,
        CompanyMembership::Admin,
        Some(principal),
    );
    let restricted = verify_admin_visibility(&store, company_id, version_id, principal).await;
    verify_owner_visibility(&store, company_id, version_id, &restricted).await;
}

async fn verify_admin_visibility(
    store: &MemoryStore,
    company_id: CompanyId,
    version_id: VersionId,
    principal: PrincipalId,
) -> Channel {
    let mut restricted = channel(company_id, ChannelAccessMode::Allowlist);
    store
        .state
        .lock()
        .unwrap()
        .channels
        .insert(restricted.id, restricted.clone());
    assert!(matches!(
        service(store)
            .admit(admitted_request(
                company_id,
                version_id,
                "restricted",
                related(&restricted)
            ))
            .await,
        Err(AppError::NotFound(_))
    ));
    restricted.principal_grants.push(ChannelPrincipalGrant {
        principal_id: principal,
        capability: PrincipalCapability::View,
        provenance: GrantProvenance::ConfiguredAllowlist,
        created_at: chrono::Utc::now(),
    });
    store
        .state
        .lock()
        .unwrap()
        .channels
        .insert(restricted.id, restricted.clone());
    assert!(matches!(
        service(store)
            .admit(admitted_request(
                company_id,
                version_id,
                "granted",
                related(&restricted)
            ))
            .await
            .unwrap(),
        AdmissionResult::Created(_)
    ));
    let team = channel(company_id, ChannelAccessMode::Team);
    store
        .state
        .lock()
        .unwrap()
        .channels
        .insert(team.id, team.clone());
    assert!(matches!(
        service(store)
            .admit(admitted_request(
                company_id,
                version_id,
                "team",
                related(&team)
            ))
            .await
            .unwrap(),
        AdmissionResult::Created(_)
    ));
    restricted
}

async fn verify_owner_visibility(
    store: &MemoryStore,
    company_id: CompanyId,
    version_id: VersionId,
    restricted: &Channel,
) {
    set_membership(store, company_id, CompanyMembership::Owner, None);
    assert!(matches!(
        service(store)
            .admit(admitted_request(
                company_id,
                version_id,
                "owner",
                related(&channel(company_id, ChannelAccessMode::Allowlist))
            ))
            .await,
        Err(AppError::NotFound(_))
    ));
    assert!(matches!(
        service(store)
            .admit(admitted_request(
                company_id,
                version_id,
                "owner-present",
                related(restricted)
            ))
            .await
            .unwrap(),
        AdmissionResult::Created(_)
    ));
}
