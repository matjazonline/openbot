use super::*;

#[tokio::test]
async fn autonomous_agent_storage_external_targets_round_trip_and_revoke_without_aba() {
    let f = AdmissionFixture::new().await;
    let http = Uuid::new_v4();
    native_http(&f, http).await;
    let mcp = Uuid::new_v4();
    let version = native_mcp(&f, mcp).await;
    let sources = [
        AgentBindingSource::Http {
            connection: http,
            operation: TypeName::parse("lookup").unwrap(),
        },
        AgentBindingSource::Mcp {
            connection: mcp,
            operation: TypeName::parse("lookup").unwrap(),
            reviewed_version: version,
            remote_name: "lookup".to_owned().try_into().unwrap(),
        },
    ];
    let selected: Vec<_> = sources
        .into_iter()
        .enumerate()
        .map(|(index, source)| AgentToolBinding {
            id: AgentBindingId::new(Uuid::new_v4()),
            alias: AgentToolAlias::parse(format!("lookup_{index}")).unwrap(),
            source,
        })
        .collect();
    let agent = new_agent(&f, selected.clone()).await;
    assert_eq!(bindings(&f, agent.id).await, selected);
    for binding in &selected {
        sqlx::query(
            "INSERT INTO agent_binding_grants(company_id,agent_id,binding_id) VALUES($1,$2,$3)",
        )
        .bind(f.binding.target.company.as_uuid())
        .bind(agent.id)
        .bind(binding.id.as_uuid())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    }
    for table in ["workflow_http_operations", "workflow_mcp_operations"] {
        sqlx::query(&format!(
            "UPDATE {table} SET revoked_at=clock_timestamp() WHERE company_id=$1"
        ))
        .bind(f.binding.target.company.as_uuid())
        .execute(f.persistence().pool())
        .await
        .unwrap();
        sqlx::query(&format!(
            "UPDATE {table} SET revoked_at=NULL{} WHERE company_id=$1",
            if table == "workflow_mcp_operations" {
                ",version=gen_random_uuid()"
            } else {
                ""
            }
        ))
        .bind(f.binding.target.company.as_uuid())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    }
    for binding in &selected {
        assert!(!active_grant(&f, binding.id).await);
    }
    native_rejection(
        sqlx::query("UPDATE agent_binding_grants SET revoked_at=NULL WHERE agent_id=$1")
            .bind(agent.id)
            .execute(f.persistence().pool())
            .await
            .unwrap_err(),
    );
    assert_eq!(bindings(&f, agent.id).await, selected);
}

#[tokio::test]
async fn autonomous_agent_storage_workflow_pin_survives_repin_and_revocation_is_permanent() {
    let f = AdmissionFixture::new().await;
    let scope = admitted(&f, "workflow-pin").await;
    let placeholder = new_agent(&f, Vec::new()).await;
    let revision = save_revision(&f, &definition(&f, placeholder.id, Vec::new())).await;
    let value = manifest_for(&f, scope.run, placeholder.id, revision).await;
    let pin = &value.workflows[0];
    sqlx::query("INSERT INTO workflow_tool_registrations(company_id,name,workflow_id,version_id,params) VALUES($1,'child.lookup',$2,$3,'{}')")
        .bind(f.binding.target.company.as_uuid()).bind(pin.workflow_id.as_uuid()).bind(pin.version_id.as_uuid()).execute(f.persistence().pool()).await.unwrap();
    let binding = AgentToolBinding {
        id: AgentBindingId::new(Uuid::new_v4()),
        alias: AgentToolAlias::parse("lookup").unwrap(),
        source: AgentBindingSource::Workflow {
            registration: TypeName::parse("child.lookup").unwrap(),
            workflow: pin.workflow_id,
            version: pin.version_id,
            params: json!({}),
        },
    };
    let agent = new_agent(&f, vec![binding.clone()]).await;
    sqlx::query(
        "INSERT INTO agent_binding_grants(company_id,agent_id,binding_id) VALUES($1,$2,$3)",
    )
    .bind(f.binding.target.company.as_uuid())
    .bind(agent.id)
    .bind(binding.id.as_uuid())
    .execute(f.persistence().pool())
    .await
    .unwrap();
    let mut tx = f.persistence().pool().begin().await.unwrap();
    let frozen = crate::adapters::persistence::workflow::workflow_tools::workflow_tool_snapshot(
        &mut tx,
        f.binding.target.company,
        &TypeName::parse("child.lookup").unwrap(),
    )
    .await
    .unwrap()
    .unwrap();
    tx.commit().await.unwrap();
    let fixture = &f.binding.fixture;
    fixture
        .service()
        .save(fixture.save(Some(DraftRevision::new(1).unwrap())))
        .await
        .unwrap();
    let mut publish = fixture.publish();
    publish.expected = DraftRevision::new(2).unwrap();
    publish.key = IdempotencyKey::parse("pin-v2").unwrap();
    let next = publish.version;
    fixture.service().publish(publish).await.unwrap();
    sqlx::query("UPDATE workflow_tool_registrations SET version_id=$2,params='{\"future\":true}',revision=revision+1 WHERE company_id=$1")
        .bind(f.binding.target.company.as_uuid()).bind(next.as_uuid()).execute(f.persistence().pool()).await.unwrap();
    assert_eq!(bindings(&f, agent.id).await, vec![binding.clone()]);
    assert!(active_grant(&f, binding.id).await);
    let mut tx = f.persistence().pool().begin().await.unwrap();
    crate::adapters::persistence::workflow::workflow_tools::registered_on(
        &mut tx,
        f.binding.target.company,
        &frozen,
        crate::adapters::persistence::workflow::workflow_tools::ToolCheck::Run,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    sqlx::query(
        "UPDATE workflow_tool_registrations SET revoked_at=clock_timestamp() WHERE company_id=$1",
    )
    .bind(f.binding.target.company.as_uuid())
    .execute(f.persistence().pool())
    .await
    .unwrap();
    sqlx::query("UPDATE workflow_tool_registrations SET revoked_at=NULL WHERE company_id=$1")
        .bind(f.binding.target.company.as_uuid())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    assert!(!active_grant(&f, binding.id).await);
}
