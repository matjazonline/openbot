//! Private desk fixtures use real empty-child ownership and production owner writes.
use crate::domain::workflow::CompanyId;
use crate::entities::creation::CreationProvenance;
use crate::entities::department::DepartmentId;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

async fn empty_on(tx: &mut Transaction<'_, Postgres>, company: CompanyId) -> DepartmentId {
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(company.as_uuid())
        .execute(&mut **tx)
        .await
        .unwrap();
    let root = super::super::company::root_on(tx, company).await.unwrap();
    let department = DepartmentId::random();
    sqlx::query("INSERT INTO departments (id,company_id,parent_id,name,slug,created_by) VALUES($1,$2,$3,'Restricted',$4,$5)")
        .bind(department.as_uuid()).bind(company.as_uuid()).bind(root.as_uuid())
        .bind(format!("restricted-{}", department.as_uuid().simple()))
        .bind(serde_json::to_value(CreationProvenance::system()).unwrap())
        .execute(&mut **tx).await.unwrap();
    department
}

/// A fixture department with no members, role holders or separately provisioned desk.
pub async fn empty_channel_department(pool: &PgPool, company: Uuid) -> DepartmentId {
    let mut tx = pool.begin().await.unwrap();
    let department = empty_on(&mut tx, CompanyId::new(company)).await;
    tx.commit().await.unwrap();
    department
}

/// Preserve configured grants but replace the root audience through the production writer.
pub async fn restrict_channel_to_configured_grants(pool: &PgPool, channel: Uuid) -> DepartmentId {
    let mut tx = pool.begin().await.unwrap();
    // This trusted fixture lookup chooses the production writer's tenant lock.
    let company = CompanyId::new(
        sqlx::query_scalar("SELECT company_id FROM channels WHERE id=$1")
            .bind(channel)
            .fetch_one(&mut *tx)
            .await
            .unwrap(),
    );
    let department = empty_on(&mut tx, company).await;
    super::super::department_desks::set_desk_ownership_on(&mut tx, company, channel, department)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    department
}
