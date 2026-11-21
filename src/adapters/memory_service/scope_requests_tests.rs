use super::*;

#[tokio::test]
async fn memory_integration_many_scope_requests_keep_the_transport_bound_and_input_order() {
    let fake = ContractFake::start(ADMIN).await;
    let http = client(&fake.base_url, ADMIN);
    let tenant = TenantExternalId(Uuid::new_v4());
    http.create_tenant(tenant).await.unwrap();
    let mut credentials = Vec::new();
    let mut wanted = Vec::new();
    let mut namespaces = Vec::new();
    for index in 0..=MAX_PARALLEL_SCOPE_REQUESTS {
        let namespace = NamespaceExternalId(Uuid::new_v4());
        http.create_namespace(tenant, namespace).await.unwrap();
        credentials.push(ClientCredential {
            tenant,
            id: Uuid::nil(),
            token: SecretString::from(fake.client_token(tenant.0, namespace.0)),
        });
        let id = Uuid::new_v4();
        wanted.push(id);
        namespaces.push(namespace);
        fake.state.lock().unwrap().observations.push((namespace.0.to_string(),
            json!({"observation_id":id,"kind":"fact","title":format!("scope {index}"),"body":"scope body",
                "occurred_at":"2026-11-01T00:00:00Z","access_level":2,"revision":1,
                "content_sha256":"0".repeat(64),"sources":[],"index_state":"pending"})));
    }
    fake.state
        .lock()
        .unwrap()
        .holds
        .insert("observations/search".into(), Duration::from_millis(500));
    let searches: Vec<_> = credentials
        .iter()
        .zip(namespaces)
        .map(|(credential, namespace)| ScopedSearch {
            credential,
            search: MemorySearch {
                namespace,
                read_ceiling: 6,
                query: "scope".into(),
                limit: 1,
            },
        })
        .collect();
    let answers = http.search_scopes(&searches).await.unwrap();
    assert_eq!(answers.len(), MAX_PARALLEL_SCOPE_REQUESTS + 1);
    assert_eq!(
        answers
            .iter()
            .map(|answer| answer.items[0].observation_id)
            .collect::<Vec<_>>(),
        wanted
    );
    assert_eq!(
        fake.state.lock().unwrap().held_peak["observations/search"],
        MAX_PARALLEL_SCOPE_REQUESTS
    );
    fake.assert_clean();
}
