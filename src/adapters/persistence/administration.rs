//! One database predicate shared by preflights and locked management writes.
use async_trait::async_trait;
use sqlx::PgConnection;
use uuid::Uuid;

use super::{PostgresPersistence, department::PostgresDepartments};
use crate::{
    app_error::{AppError, AppResult},
    application::ports::administration::{
        AdministrationPersistence, ManagementRole, ManagementTarget,
    },
    domain::workflow::CompanyId,
    entities::department::DepartmentId,
};

fn decode(role: Option<String>) -> AppResult<Option<ManagementRole>> {
    role.map(|role| match role.as_str() {
        "owner" => Ok(ManagementRole::Owner),
        "admin" => Ok(ManagementRole::Admin),
        _ => role
            .strip_prefix("lead:")
            .and_then(|unit| Uuid::parse_str(unit).ok())
            .map(|unit| ManagementRole::Lead(DepartmentId::new(unit)))
            .ok_or_else(|| AppError::Internal("Invalid stored management role".into())),
    })
    .transpose()
}

pub(in crate::adapters::persistence) async fn creating_department_on(
    db: &mut PgConnection,
    actor: Uuid,
    company: CompanyId,
) -> AppResult<DepartmentId> {
    sqlx::query_scalar::<_, Option<Uuid>>("SELECT workflow_creating_department($1,$2)")
        .bind(actor)
        .bind(company.as_uuid())
        .fetch_one(db)
        .await?
        .map(DepartmentId::new)
        .ok_or_else(|| AppError::NotFound("Creating department not found".into()))
}

pub(in crate::adapters::persistence) async fn management_role_on(
    db: &mut PgConnection,
    actor: Uuid,
    company: CompanyId,
    target: ManagementTarget,
) -> AppResult<Option<ManagementRole>> {
    let role = match target {
        ManagementTarget::DefinitionCreation(workflow) => {
            sqlx::query_scalar(
                "SELECT CASE WHEN EXISTS(SELECT 1 FROM workflow_definitions WHERE company_id=$2 AND id=$3) \
                 THEN workflow_definition_management_role($1,$2,$3) \
                 ELSE unit_management_role($1,$2,workflow_creating_department($1,$2)) END",
            )
                .bind(actor)
                .bind(company.as_uuid())
                .bind(workflow.as_uuid())
                .fetch_one(db)
                .await?
        }
        ManagementTarget::Definition(workflow) => {
            sqlx::query_scalar("SELECT workflow_definition_management_role($1,$2,$3)")
                .bind(actor)
                .bind(company.as_uuid())
                .bind(workflow.as_uuid())
                .fetch_one(db)
                .await?
        }
        ManagementTarget::AnyUnit => {
            sqlx::query_scalar("SELECT any_unit_management_role($1,$2)")
                .bind(actor)
                .bind(company.as_uuid())
                .fetch_one(db)
                .await?
        }
        ManagementTarget::Root | ManagementTarget::Unit(_) => {
            let unit = match target {
                ManagementTarget::Unit(unit) => Some(unit.as_uuid()),
                _ => None,
            };
            sqlx::query_scalar("SELECT unit_management_role($1,$2,$3)")
                .bind(actor)
                .bind(company.as_uuid())
                .bind(unit)
                .fetch_one(db)
                .await?
        }
        ManagementTarget::Agent(agent) => {
            sqlx::query_scalar("SELECT agent_management_role($1,$2,$3)")
                .bind(actor)
                .bind(company.as_uuid())
                .bind(agent.as_uuid())
                .fetch_one(db)
                .await?
        }
    };
    decode(role)
}

#[async_trait]
impl AdministrationPersistence for PostgresPersistence {
    async fn management_role(
        &self,
        actor: Uuid,
        company: CompanyId,
        target: ManagementTarget,
    ) -> AppResult<Option<ManagementRole>> {
        let mut tx = super::agent_publication::begin_bounded(self).await?;
        let role = management_role_on(&mut tx, actor, company, target).await?;
        tx.commit().await?;
        Ok(role)
    }

    async fn managed_units(&self, actor: Uuid, company: CompanyId) -> AppResult<Vec<DepartmentId>> {
        let units: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM departments WHERE company_id=$2 \
             AND unit_management_role($1,$2,id) IS NOT NULL ORDER BY id",
        )
        .bind(actor)
        .bind(company.as_uuid())
        .fetch_all(self.pool())
        .await?;
        Ok(units.into_iter().map(DepartmentId::new).collect())
    }
}

#[async_trait]
impl AdministrationPersistence for PostgresDepartments {
    async fn management_role(
        &self,
        actor: Uuid,
        company: CompanyId,
        target: ManagementTarget,
    ) -> AppResult<Option<ManagementRole>> {
        self.persistence
            .management_role(actor, company, target)
            .await
    }

    async fn managed_units(&self, actor: Uuid, company: CompanyId) -> AppResult<Vec<DepartmentId>> {
        self.persistence.managed_units(actor, company).await
    }
}
