use super::*;

#[tokio::test]
async fn workflow_http_binding_checks_exact_selected_connection_contract() {
    let setup = Box::pin(setup("http://127.0.0.1:1", Entrance::Http)).await;
    let fixture = &setup.fixture;
    let other = Box::pin(configured(fixture, "http://127.0.0.1:2", Entrance::Http)).await;
    let version = setup.version;
    // Both connections advertise the same name, but only the original owns the frozen contract.
    let mut changed = operation(fixture.target.company, "http.request");
    changed.tool.policy.capability = TypeName::parse("changed.capability").unwrap();
    fixture
        .persistence
        .register_http_operation(
            fixture.target.company,
            fixture.target.actor,
            other.id,
            other.revision,
            changed,
        )
        .await
        .unwrap();
    let bindings = BindingService::new(
        Preflight,
        fixture.persistence.clone(),
        fixture.persistence.clone(),
        fixture.persistence.clone(),
    );
    let result = bindings
        .configure(ConfigureBinding {
            target: BindingTarget {
                company: fixture.target.company,
                actor: fixture.target.actor,
                binding: WorkflowBindingId::new(Uuid::new_v4()),
            },
            expected: None,
            association: RelatedAssociation::Company,
            version,
            params: json!({}),
            resources: BTreeMap::from([(
                ResourceName::parse("service").unwrap(),
                RuntimeResourceId::new(other.id),
            )]),
        })
        .await;
    assert!(result.is_err());
    // The unchanged original still binds: the rejection is the selected contract, not the name.
    Box::pin(bound(fixture, &setup.connection, version, Entrance::Http)).await;
}

#[tokio::test]
async fn workflow_http_admission_rechecks_bound_operation_after_activation() {
    let setup = Box::pin(setup("http://127.0.0.1:1", Entrance::Http)).await;
    let fixture = &setup.fixture;
    let target = Box::pin(bound(
        fixture,
        &setup.connection,
        setup.version,
        Entrance::Http,
    ))
    .await;
    // Keep another matching owner live so a global registration lookup alone would pass.
    Box::pin(configured(fixture, "http://127.0.0.1:2", Entrance::Http)).await;
    fixture
        .persistence
        .revoke_http_operation(
            fixture.target.company,
            fixture.target.actor,
            setup.connection.id,
            setup.connection.revision,
            TypeName::parse("http.request").unwrap(),
        )
        .await
        .unwrap();
    let p = &fixture.persistence;
    let result = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight)
        .admit(AdmitWorkflowRequest {
            company_id: target.company,
            actor: target.actor,
            association: RelatedAssociation::Company,
            trigger: TriggerRef::new(
                target.company,
                TriggerId::new(Uuid::new_v4()),
                TriggerSource::Manual,
            )
            .unwrap(),
            correlation_id: CorrelationId::new(),
            binding_id: target.binding,
            idempotency_key: IdempotencyKey::parse("revoked-http-admission").unwrap(),
            input: json!({}),
        })
        .await;
    assert!(result.is_err());
    let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_runs")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(runs, 1, "failed admission must not create another run");
}
