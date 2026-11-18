use super::*;
use crate::entities::transport::PrincipalId;

#[tokio::test]
async fn memory_scope_subject_admin_uses_exact_provenance_and_reports_ambiguous_deletion() {
    let fake = ContractFake::start(ADMIN).await;
    let http = client(&fake.base_url, ADMIN);
    let tenant = TenantExternalId(Uuid::new_v4());
    let namespace = NamespaceExternalId(Uuid::new_v4());
    http.create_tenant(tenant).await.unwrap();
    http.create_namespace(tenant, namespace).await.unwrap();
    let credential = ClientCredential {
        tenant,
        id: Uuid::nil(),
        token: SecretString::from(fake.client_token(tenant.0, namespace.0)),
    };
    let subject = PrincipalId::new(Uuid::new_v4());
    let request:MemorySaveRequest=serde_json::from_value(json!({"idempotency_key":"subject-save",
        "expires_at":chrono::Utc::now()+chrono::Duration::hours(1),"namespace":namespace.0,
        "context":{"producing_ceiling":6,"consumed":{"kind":"exact","level":3},"external_sources":[]},
        "items":[{"kind":"fact","title":"A fact","body":"saved body","occurred_at":chrono::Utc::now(),"subject":subject.as_uuid()}]})).unwrap();
    let saved = http.save(&credential, &request).await.unwrap();
    let id = saved.observations[0].observation_id;
    let scope = MemorySubjectScope {
        tenant,
        namespace,
        subject,
    };
    let page = http.list_subject(scope, None).await.unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].observation_id, id);
    assert_eq!(page.items[0].access_level, 3);
    assert!(
        http.list_subject(
            MemorySubjectScope {
                subject: PrincipalId::new(Uuid::new_v4()),
                ..scope
            },
            None
        )
        .await
        .unwrap()
        .items
        .is_empty()
    );
    assert!(
        http.delete_subject(
            MemorySubjectScope {
                subject: PrincipalId::new(Uuid::new_v4()),
                ..scope
            },
            &[id]
        )
        .await
        .unwrap()
        .is_empty()
    );
    fake.drop_after_write("observations/subject/delete");
    assert_eq!(
        http.delete_subject(scope, &[id]).await.unwrap_err(),
        MemoryServiceError::Ambiguous
    );
    assert!(
        http.list_subject(scope, None)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert!(
        http.delete_subject(scope, &[id]).await.unwrap().is_empty(),
        "retries of an applied ID batch are safe"
    );
    fake.assert_clean();
}

#[tokio::test]
async fn memory_scope_subject_admin_never_treats_foreign_management_as_an_empty_page() {
    let fake = ContractFake::start(ADMIN).await;
    let http = client(&fake.base_url, ADMIN);
    let tenant = TenantExternalId(Uuid::new_v4());
    fake.foreign_tenant(tenant.0);
    let scope = MemorySubjectScope {
        tenant,
        namespace: NamespaceExternalId(Uuid::new_v4()),
        subject: PrincipalId::new(Uuid::new_v4()),
    };
    assert_eq!(
        http.list_subject(scope, None).await.unwrap_err(),
        MemoryServiceError::NotFound
    );
    assert_eq!(
        http.delete_subject(scope, &[Uuid::new_v4()])
            .await
            .unwrap_err(),
        MemoryServiceError::NotFound
    );
    fake.assert_clean();
}
