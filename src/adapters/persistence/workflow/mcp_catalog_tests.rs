use super::*;

#[tokio::test]
async fn workflow_mcp_registration_competes_with_grant_removal_refresh_and_delete() {
    for change in ["grant", "refresh", "delete"] {
        let setup = Box::pin(mcp_setup(Call::Json)).await;
        let p = &setup.fixture.persistence;
        let current = setup.current().await;
        let barrier = tokio::sync::Barrier::new(2);
        let register = async {
            barrier.wait().await;
            p.register_mcp_operation(
                setup.company(),
                setup.actor(),
                McpOperationWrite {
                    connection: current.id,
                    catalog_revision: current.revision,
                    expected_version: Some(setup.operation),
                    operation: operation(setup.company()),
                },
            )
            .await
        };
        let revoke = async {
            barrier.wait().await;
            if change == "delete" {
                p.delete_mcp_connection(current.company_id, current.id, current.revision)
                    .await
            } else {
                let mut write = connection_write(&setup.server.url);
                write.tool_grants.clear();
                if change == "refresh" {
                    write.discovered_tools.clear();
                }
                p.update_mcp_connection(current.company_id, current.id, current.revision, write)
                    .await
                    .map(|_| ())
            }
        };
        let (registered, revoked) = tokio::join!(register, revoke);
        revoked.unwrap();
        if let Err(error) = registered {
            assert!(
                matches!(error, AppError::Conflict(_) | AppError::NotFound(_)),
                "{error:?}"
            );
        }
        let approvals: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_mcp_operations")
            .fetch_one(p.pool())
            .await
            .unwrap();
        assert_eq!(
            approvals, 0,
            "{change}: approval cannot outlive its grant/connection"
        );
    }
}

#[tokio::test]
async fn workflow_mcp_operation_cas_cascades_and_tenant_constraints() {
    let setup = Box::pin(mcp_setup(Call::Json)).await;
    let p = &setup.fixture.persistence;
    let company = setup.company();
    let actor = setup.actor();
    let current = setup.current().await;
    let write = |expected, catalog_revision| McpOperationWrite {
        connection: current.id,
        catalog_revision,
        expected_version: expected,
        operation: operation(company),
    };
    // Stale catalog revision, create-only over an existing approval, unknown version.
    for (expected, revision) in [
        (Some(setup.operation), current.revision - 1),
        (None, current.revision),
        (Some(Uuid::new_v4()), current.revision),
    ] {
        assert!(matches!(
            p.register_mcp_operation(company, actor, write(expected, revision))
                .await,
            Err(AppError::Conflict(_) | AppError::NotFound(_))
        ));
    }
    // Two writers holding the same version: exactly one wins.
    let (a, b) = tokio::join!(
        p.register_mcp_operation(
            company,
            actor,
            write(Some(setup.operation), current.revision)
        ),
        p.register_mcp_operation(
            company,
            actor,
            write(Some(setup.operation), current.revision)
        ),
    );
    assert!(a.is_ok() != b.is_ok());
    let live = a.or(b).unwrap();
    // Revoke and re-register: the stale version can neither replace nor revoke (no ABA).
    let revoke = |version| McpOperationRevoke {
        connection: current.id,
        catalog_revision: current.revision,
        name: TypeName::parse(TOOL).unwrap(),
        version,
    };
    p.revoke_mcp_operation(company, actor, revoke(live))
        .await
        .unwrap();
    let recreated = p
        .register_mcp_operation(company, actor, write(None, current.revision))
        .await
        .unwrap();
    assert!(
        p.register_mcp_operation(company, actor, write(Some(live), current.revision))
            .await
            .is_err()
    );
    assert!(
        p.revoke_mcp_operation(company, actor, revoke(live))
            .await
            .is_err()
    );
    // Grant removal cascades the approval; a stale version cannot touch the re-registration.
    let mut ungranted = connection_write(&setup.server.url);
    ungranted.tool_grants.clear();
    let after = p
        .update_mcp_connection(company.as_uuid(), current.id, current.revision, ungranted)
        .await
        .unwrap();
    let approvals = || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM workflow_mcp_operations")
            .fetch_one(p.pool())
            .await
            .unwrap()
    };
    assert_eq!(approvals().await, 0, "grant removal cascades");
    assert!(
        p.register_mcp_operation(company, actor, write(None, after.revision))
            .await
            .is_err(),
        "an ungranted tool cannot be approved"
    );
    let regranted = p
        .update_mcp_connection(
            company.as_uuid(),
            current.id,
            after.revision,
            connection_write(&setup.server.url),
        )
        .await
        .unwrap();
    let again = p
        .register_mcp_operation(company, actor, write(None, regranted.revision))
        .await
        .unwrap();
    for stale in [live, recreated] {
        let mut replace = write(Some(stale), regranted.revision);
        replace.catalog_revision = regranted.revision;
        assert!(
            p.register_mcp_operation(company, actor, replace)
                .await
                .is_err()
        );
    }
    // A drifted reviewed schema cannot be approved.
    let mut drifted = write(Some(again), regranted.revision);
    drifted.operation.tool.contract.input_schema = json!({"type":"object"});
    assert!(matches!(
        p.register_mcp_operation(company, actor, drifted).await,
        Err(AppError::BadRequest(_))
    ));
    // Cross-tenant rows are rejected by the composite foreign keys.
    let other = Fixture::in_database(setup.fixture._db.clone(), "-other").await;
    let mut tx = p.pool().begin().await.unwrap();
    let mut forged = operation(other.target.company).tool;
    forged.contract.connection = None;
    let inserted = sqlx::query("INSERT INTO workflow_mcp_operations(company_id,connection_id,name,snapshot,approval_required,version) VALUES($1,$2,$3,$4,false,$5)")
        .bind(other.target.company.as_uuid())
        .bind(current.id)
        .bind(TOOL)
        .bind(serde_json::to_value(&forged).unwrap())
        .bind(Uuid::new_v4())
        .execute(&mut *tx)
        .await;
    assert!(inserted.is_err());
    tx.rollback().await.unwrap();
    // Soft delete removes the approvals with the connection.
    p.delete_mcp_connection(company.as_uuid(), current.id, regranted.revision)
        .await
        .unwrap();
    assert_eq!(approvals().await, 0);
}

#[tokio::test]
async fn workflow_mcp_binding_rejects_foreign_and_wrong_kind_resources() {
    let setup = Box::pin(mcp_setup(Call::Json)).await;
    let other = Fixture::in_database(setup.fixture._db.clone(), "-foreign").await;
    let p = &other.persistence;
    let foreign = p
        .create_mcp_connection(
            other.target.company.as_uuid(),
            connection_write(&setup.server.url),
        )
        .await
        .unwrap();
    let bindings = BindingService::new(
        Preflight,
        setup.fixture.persistence.clone(),
        setup.fixture.persistence.clone(),
        setup.fixture.persistence.clone(),
    );
    let version = setup.version;
    for resource in [foreign.id, Uuid::new_v4()] {
        let configured = bindings
            .configure(ConfigureBinding {
                target: BindingTarget {
                    company: setup.company(),
                    actor: setup.actor(),
                    binding: WorkflowBindingId::new(Uuid::new_v4()),
                },
                expected: None,
                association: RelatedAssociation::Company,
                version,
                params: json!({}),
                resources: BTreeMap::from([(
                    ResourceName::parse("service").unwrap(),
                    RuntimeResourceId::new(resource),
                )]),
            })
            .await;
        assert!(configured.is_err());
    }
}
