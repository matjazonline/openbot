//! The company's root and current membership writes. Callers own the transaction and account /
//! company locks; the schema proves the same relationships at commit.
use sqlx::PgConnection;
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    domain::workflow::CompanyId,
    entities::{
        access_level::AccessLevel, company::Company, company_member::CompanyAccessRole,
        creation::CreationProvenance, department::DepartmentId, transport::PrincipalId,
    },
};

pub(crate) struct RootJoin {
    pub company: CompanyId,
    pub user: Uuid,
    pub role: CompanyAccessRole,
    pub access_level: AccessLevel,
}

pub(crate) struct RootMember {
    pub membership_id: Uuid,
    pub department: DepartmentId,
    pub principal: PrincipalId,
}

#[derive(sqlx::FromRow)]
struct RootMemberDb {
    id: Uuid,
    department_id: Uuid,
    principal_id: Uuid,
}

pub(crate) async fn root_member_on(
    db: &mut PgConnection,
    company: CompanyId,
    user: Uuid,
) -> AppResult<Option<RootMember>> {
    let row = sqlx::query_as::<_, RootMemberDb>(
        "SELECT id, department_id, principal_id FROM membership_of($1, $2)",
    )
    .bind(user)
    .bind(company.as_uuid())
    .fetch_optional(db)
    .await?;
    Ok(row.map(|row| RootMember {
        membership_id: row.id,
        department: DepartmentId::new(row.department_id),
        principal: PrincipalId::new(row.principal_id),
    }))
}

pub(crate) async fn root_on(db: &mut PgConnection, company: CompanyId) -> AppResult<DepartmentId> {
    sqlx::query_scalar::<_, Uuid>("SELECT id FROM departments WHERE company_id = $1 AND is_root")
        .bind(company.as_uuid())
        .fetch_optional(db)
        .await?
        .map(DepartmentId::new)
        .ok_or_else(|| AppError::NotFound("Company root not found.".into()))
}

pub(crate) async fn bootstrap_root_on(
    db: &mut PgConnection,
    company: &Company,
) -> AppResult<PrincipalId> {
    sqlx::query(
        "INSERT INTO departments (id, company_id, name, slug, is_root, created_by) \
         VALUES ($1, $2, $3, $4, true, $5)",
    )
    .bind(Uuid::new_v4())
    .bind(company.id)
    .bind(&company.name)
    .bind(company.slug.as_str())
    .bind(
        serde_json::to_value(CreationProvenance::system())
            .map_err(|error| AppError::Internal(error.to_string()))?,
    )
    .execute(&mut *db)
    .await?;
    let joined = join_root_on(
        db,
        RootJoin {
            company: CompanyId::new(company.id),
            user: company.user_id,
            role: CompanyAccessRole::Member,
            access_level: AccessLevel::MAX,
        },
    )
    .await?;
    sqlx::query(
        "INSERT INTO department_roles (company_id, department_id, role, principal_id) \
         VALUES ($1, $2, 'owner', $3)",
    )
    .bind(company.id)
    .bind(joined.department.as_uuid())
    .bind(joined.principal.as_uuid())
    .execute(db)
    .await?;
    Ok(joined.principal)
}

/// Reacceptance keeps the existing membership incarnation and level. A true rejoin creates a
/// fresh row, while person resolution restores the account's lifetime principal.
pub(crate) async fn join_root_on(db: &mut PgConnection, join: RootJoin) -> AppResult<RootMember> {
    let (username, email): (String, String) =
        sqlx::query_as("SELECT username::text, email::text FROM users WHERE id = $1 FOR SHARE")
            .bind(join.user)
            .fetch_one(&mut *db)
            .await?;
    let department = root_on(db, join.company).await?;
    let principal = super::super::participant::create_person_principal_on(
        db,
        join.company.as_uuid(),
        join.user,
        &username,
        &email,
    )
    .await?;
    sqlx::query(
        "INSERT INTO department_members (company_id, department_id, principal_id, access_level) \
         VALUES ($1, $2, $3, CASE WHEN EXISTS (SELECT 1 FROM companies AS company \
             WHERE company.id=$1 AND company.user_id=$5) THEN 9::smallint ELSE $4::smallint END) \
         ON CONFLICT (department_id, principal_id) DO NOTHING",
    )
    .bind(join.company.as_uuid())
    .bind(department.as_uuid())
    .bind(principal.as_uuid())
    .bind(join.access_level.as_i16())
    .bind(join.user)
    .execute(&mut *db)
    .await?;
    let joined = root_member_on(db, join.company, join.user)
        .await?
        .ok_or_else(|| AppError::Internal("Joined root membership is missing.".into()))?;
    set_admin_on(db, join.company, &joined, join.role).await?;
    Ok(joined)
}

pub(crate) async fn set_admin_on(
    db: &mut PgConnection,
    company: CompanyId,
    member: &RootMember,
    role: CompanyAccessRole,
) -> AppResult<()> {
    // The owner's fact cannot be changed by an invite or an ordinary role mutation.
    let owner: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM companies AS company \
           JOIN principals AS principal ON principal.company_id = company.id \
             AND principal.user_id = company.user_id \
           JOIN department_members AS member ON member.company_id = company.id \
             AND member.principal_id = principal.id \
           WHERE company.id = $1 AND principal.id = $2 AND member.id = $3)",
    )
    .bind(company.as_uuid())
    .bind(member.principal.as_uuid())
    .bind(member.membership_id)
    .fetch_one(&mut *db)
    .await?;
    if owner {
        return Ok(());
    }
    match role {
        CompanyAccessRole::Admin => {
            sqlx::query(
                "INSERT INTO department_roles (company_id, department_id, principal_id, role) \
                 VALUES ($1, $2, $3, 'admin') ON CONFLICT DO NOTHING",
            )
            .bind(company.as_uuid())
            .bind(member.department.as_uuid())
            .bind(member.principal.as_uuid())
            .execute(db)
            .await?;
        }
        CompanyAccessRole::Member => {
            sqlx::query(
                "DELETE FROM department_roles WHERE company_id = $1 \
                 AND department_id = $2 AND principal_id = $3 AND role = 'admin'",
            )
            .bind(company.as_uuid())
            .bind(member.department.as_uuid())
            .bind(member.principal.as_uuid())
            .execute(db)
            .await?;
        }
    }
    Ok(())
}
