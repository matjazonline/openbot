use super::*;

#[derive(sqlx::FromRow)]
pub(super) struct Broker {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub version_id: Uuid,
    pub workflow_binding_id: Uuid,
    pub contract: Value,
}

pub(super) async fn prepare_on(
    db: &mut PgConnection,
    owner: &NativeMcpOwner,
    tool: &ResolvedAgentTool,
    arguments: &Value,
) -> AppResult<(Broker, PreparedNativeMcp)> {
    let NativeHarnessSelection::Agent { revision_id, .. } = owner.harness else {
        return Err(missing());
    };
    // A zero-wait key reservation prevents unique-index waits between different machines.
    // It owns no existing run and is released with the enclosing admission transaction.
    let key = format!(
        "native-mcp:{}:{}:{}:{}",
        owner.company.as_uuid(),
        owner.actor.user_id(),
        revision_id.as_uuid(),
        tool.binding.id.as_uuid()
    );
    let reserved: bool =
        sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended($1,0))")
            .bind(key)
            .fetch_one(&mut *db)
            .await?;
    if !reserved {
        return Err(AppError::ServiceUnavailable(
            "Native MCP admission is busy".into(),
        ));
    }
    let existing = sqlx::query_as::<_, Broker>(
        "SELECT id,workflow_id,version_id,workflow_binding_id,contract FROM native_mcp_brokers
        WHERE company_id=$1 AND actor_id=$2 AND agent_revision_id=$3 AND agent_binding_id=$4",
    )
    .bind(owner.company.as_uuid())
    .bind(owner.actor.user_id())
    .bind(revision_id.as_uuid())
    .bind(tool.binding.id.as_uuid())
    .fetch_optional(&mut *db)
    .await?;
    let is_new = existing.is_none();
    let contract = serde_json::to_value(tool).map_err(|_| invalid())?;
    let broker = existing.unwrap_or_else(|| Broker {
        id: Uuid::new_v4(),
        workflow_id: Uuid::new_v4(),
        version_id: Uuid::new_v4(),
        workflow_binding_id: Uuid::new_v4(),
        contract: contract.clone(),
    });
    if broker.contract != contract {
        return Err(conflict());
    }
    let prepared = PreparedNativeMcp::compile(
        &crate::adapters::workflow_source::WorkflowSourceDecoder,
        NativeBrokerIdentity {
            company: owner.company,
            actor: owner.actor,
            workflow: WorkflowId::new(broker.workflow_id),
            version: VersionId::new(broker.version_id),
            binding: WorkflowBindingId::new(broker.workflow_binding_id),
        },
        tool,
        arguments,
        owner.deadline,
    )
    .map_err(|error| match error {
        AppError::BadRequest(_) => AppError::NativeMcpNotAdmitted,
        error => error,
    })?;
    if is_new {
        insert_on(db, owner, &broker, &prepared).await?;
    } else {
        let hash: Option<String> = sqlx::query_scalar(
            "SELECT content_hash FROM workflow_versions WHERE company_id=$1 AND id=$2",
        )
        .bind(owner.company.as_uuid())
        .bind(broker.version_id)
        .fetch_optional(db)
        .await?;
        if hash.as_deref() != Some(prepared.binding().bundle().content_hash().as_str()) {
            return Err(invalid());
        }
    }
    Ok((broker, prepared))
}

async fn insert_on(
    db: &mut PgConnection,
    owner: &NativeMcpOwner,
    broker: &Broker,
    prepared: &PreparedNativeMcp,
) -> AppResult<()> {
    let NativeHarnessSelection::Agent { revision_id, .. } = owner.harness else {
        return Err(missing());
    };
    let bundle = prepared.binding().bundle();
    let source = bundle.compiled().source();
    let title = format!(
        "Native MCP · {}",
        broker.contract["binding"]["alias"]
            .as_str()
            .ok_or_else(invalid)?
    );
    sqlx::query("INSERT INTO workflow_definitions(company_id,id,revision,title,description,source) VALUES($1,$2,1,$3,'Private native MCP broker',$4)")
        .bind(owner.company.as_uuid()).bind(broker.workflow_id).bind(&title).bind(source).execute(&mut *db).await?;
    sqlx::query("INSERT INTO workflow_versions(company_id,workflow_id,id,draft_revision,command_key,actor_id,title,description,source,bundle,content_hash,agent_ids)
        VALUES($1,$2,$3,1,$4,$5,$6,'Private native MCP broker',$7,$8,$9,'{}'::uuid[])")
        .bind(owner.company.as_uuid()).bind(broker.workflow_id).bind(broker.version_id).bind(format!("native-mcp:{}",broker.id))
        .bind(owner.actor.user_id()).bind(title).bind(source).bind(store_bundle(bundle)?).bind(bundle.content_hash().as_str()).execute(&mut *db).await?;
    sqlx::query("INSERT INTO workflow_bindings(company_id,id,state_revision,configuration_revision,active) VALUES($1,$2,1,1,false)")
        .bind(owner.company.as_uuid()).bind(broker.workflow_binding_id).execute(&mut *db).await?;
    sqlx::query("INSERT INTO workflow_binding_revisions(company_id,binding_id,revision,version_id,params,resources,actor_id,run_as_user_id)
        VALUES($1,$2,1,$3,$4,$5,$6,$6)")
        .bind(owner.company.as_uuid()).bind(broker.workflow_binding_id).bind(broker.version_id).bind(prepared.binding().params())
        .bind(serde_json::to_value(prepared.binding().resources()).map_err(|_| invalid())?).bind(owner.actor.user_id()).execute(&mut *db).await?;
    sqlx::query("INSERT INTO native_mcp_brokers(company_id,id,actor_id,agent_revision_id,agent_binding_id,contract,workflow_id,version_id,workflow_binding_id)
        VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)")
        .bind(owner.company.as_uuid()).bind(broker.id).bind(owner.actor.user_id()).bind(revision_id.as_uuid())
        .bind(broker.contract["binding"]["id"].as_str().and_then(|id|Uuid::parse_str(id).ok()).ok_or_else(invalid)?)
        .bind(&broker.contract).bind(broker.workflow_id).bind(broker.version_id).bind(broker.workflow_binding_id).execute(db).await?;
    Ok(())
}
