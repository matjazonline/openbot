use super::*;

struct NativeApproval {
    company: CompanyId,
    connection: Uuid,
    name: TypeName,
    version: Uuid,
    snapshot: Value,
}
impl NativeApproval {
    async fn fresh(f: &AdmissionFixture) -> Self {
        let connection = Uuid::new_v4();
        native_mcp(f, connection).await;
        sqlx::query("INSERT INTO company_mcp_tool_grants(company_id,connection_id,tool_name) VALUES($1,$2,'fresh')")
            .bind(f.binding.target.company.as_uuid()).bind(connection).execute(f.persistence().pool()).await.unwrap();
        let mut snapshot = operation_snapshot(f);
        snapshot["contract"]["name"] = json!("fresh");
        snapshot["policy"]["capability"] = json!("fresh");
        Self {
            company: f.binding.target.company,
            connection,
            name: TypeName::parse("fresh").unwrap(),
            version: Uuid::new_v4(),
            snapshot,
        }
    }
    async fn insert(&self, db: &mut sqlx::PgConnection) -> Result<(), sqlx::Error> {
        sqlx::query("INSERT INTO workflow_mcp_operations(company_id,connection_id,name,snapshot,approval_required,version) VALUES($1,$2,$3,$4,false,$5)")
            .bind(self.company.as_uuid()).bind(self.connection).bind(self.name.as_str()).bind(&self.snapshot).bind(self.version)
            .execute(db).await.map(|_| ())
    }
    async fn revoke(&self, db: &mut sqlx::PgConnection) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE company_mcp_tool_grants SET revoked_at=clock_timestamp() WHERE company_id=$1 AND connection_id=$2 AND tool_name=$3")
            .bind(self.company.as_uuid()).bind(self.connection).bind(self.name.as_str()).execute(db).await.map(|_| ())
    }
    async fn assert_consistent(&self, f: &AdmissionFixture) {
        let invalid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_mcp_operations AS operation
            JOIN company_mcp_tool_grants AS permission ON permission.company_id=operation.company_id
                AND permission.connection_id=operation.connection_id AND permission.tool_name=operation.name
            WHERE operation.company_id=$1 AND operation.connection_id=$2 AND operation.name=$3
                AND operation.revoked_at IS NULL AND permission.revoked_at IS NOT NULL)")
            .bind(self.company.as_uuid()).bind(self.connection).bind(self.name.as_str()).fetch_one(f.persistence().pool()).await.unwrap();
        assert!(
            !invalid,
            "active operation cannot outlive its current company permission"
        );
    }
}

async fn lock_deadline(db: &mut sqlx::PgConnection) {
    sqlx::query("SET LOCAL lock_timeout='100ms'")
        .execute(db)
        .await
        .unwrap();
}
fn blocked(error: sqlx::Error) {
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("55P03")
    );
}

#[tokio::test]
async fn autonomous_agent_storage_native_activation_locks_before_competing_revocation() {
    let f = AdmissionFixture::new().await;
    let approval = NativeApproval::fresh(&f).await;
    let mut activation = f.persistence().pool().begin().await.unwrap();
    approval.insert(&mut activation).await.unwrap();
    let mut revocation = f.persistence().pool().begin().await.unwrap();
    lock_deadline(&mut revocation).await;
    // The inserted operation is invisible here: only the grant SHARE lock can block this UPDATE.
    blocked(approval.revoke(&mut revocation).await.unwrap_err());
    revocation.rollback().await.unwrap();
    activation.commit().await.unwrap();
    let binding = AgentToolBinding {
        id: AgentBindingId::new(Uuid::new_v4()),
        alias: AgentToolAlias::parse("fresh").unwrap(),
        source: AgentBindingSource::Mcp {
            connection: approval.connection,
            operation: approval.name.clone(),
            reviewed_version: approval.version,
            remote_name: "fresh".to_owned().try_into().unwrap(),
        },
    };
    let agent = new_agent(&f, vec![binding.clone()]).await;
    sqlx::query(
        "INSERT INTO agent_binding_grants(company_id,agent_id,binding_id) VALUES($1,$2,$3)",
    )
    .bind(approval.company.as_uuid())
    .bind(agent.id)
    .bind(binding.id.as_uuid())
    .execute(f.persistence().pool())
    .await
    .unwrap();
    let mut revocation = f.persistence().pool().begin().await.unwrap();
    approval.revoke(&mut revocation).await.unwrap();
    revocation.commit().await.unwrap();
    assert!(!active_grant(&f, binding.id).await);
    approval.assert_consistent(&f).await;
    let active:bool=sqlx::query_scalar("SELECT revoked_at IS NULL FROM workflow_mcp_operations WHERE company_id=$1 AND connection_id=$2 AND name='fresh'")
        .bind(approval.company.as_uuid()).bind(approval.connection).fetch_one(f.persistence().pool()).await.unwrap();
    assert!(!active);
}

#[tokio::test]
async fn autonomous_agent_storage_native_revocation_locks_before_competing_activation() {
    let f = AdmissionFixture::new().await;
    let approval = NativeApproval::fresh(&f).await;
    let mut revocation = f.persistence().pool().begin().await.unwrap();
    approval.revoke(&mut revocation).await.unwrap();
    let mut activation = f.persistence().pool().begin().await.unwrap();
    lock_deadline(&mut activation).await;
    blocked(approval.insert(&mut activation).await.unwrap_err());
    activation.rollback().await.unwrap();
    revocation.commit().await.unwrap();
    let error = approval
        .insert(&mut f.persistence().pool().acquire().await.unwrap())
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23503")
    );
    approval.assert_consistent(&f).await;
}

#[tokio::test]
async fn autonomous_agent_storage_native_retained_reactivation_serializes_revocation() {
    let f = AdmissionFixture::new().await;
    let approval = NativeApproval::fresh(&f).await;
    approval
        .insert(&mut f.persistence().pool().acquire().await.unwrap())
        .await
        .unwrap();
    sqlx::query("UPDATE workflow_mcp_operations SET revoked_at=clock_timestamp() WHERE company_id=$1 AND connection_id=$2 AND name='fresh'")
        .bind(approval.company.as_uuid()).bind(approval.connection).execute(f.persistence().pool()).await.unwrap();
    let mut activation = f.persistence().pool().begin().await.unwrap();
    sqlx::query("UPDATE workflow_mcp_operations SET revoked_at=NULL,version=$3 WHERE company_id=$1 AND connection_id=$2 AND name='fresh'")
        .bind(approval.company.as_uuid()).bind(approval.connection).bind(Uuid::new_v4()).execute(&mut *activation).await.unwrap();
    let mut revocation = f.persistence().pool().begin().await.unwrap();
    lock_deadline(&mut revocation).await;
    blocked(approval.revoke(&mut revocation).await.unwrap_err());
    revocation.rollback().await.unwrap();
    activation.commit().await.unwrap();
    let mut revocation = f.persistence().pool().begin().await.unwrap();
    approval.revoke(&mut revocation).await.unwrap();
    revocation.commit().await.unwrap();
    approval.assert_consistent(&f).await;
}
