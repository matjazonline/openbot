//! The admission producer and readers of the sealed topology. Authorization policy
//! stays in application::workflow::scopes; no consumer recomputes the live tree.
use super::*;
use crate::application::agent::DependencyManifest;
use crate::application::workflow::scopes::{
    LeadChildren, MAX_RUN_READ_UNITS, ReadUnit, RunScopes, ScopeRoleFacts, ScopeUnit, run_scopes,
    unavailable,
};
use crate::entities::department::{DepartmentId, MAX_DEPARTMENT_DEPTH};
use crate::entities::transport::PrincipalId;

#[derive(Debug, Clone, sqlx::FromRow)]
pub(super) struct ScopeRow {
    pub acting_principal_id: Option<Uuid>,
    pub unit_id: Option<Uuid>,
    pub principal_id: Option<Uuid>,
    pub access: String,
    pub unit_slug: Option<String>,
    pub chain_depth: Option<i16>,
    pub own: bool,
    pub ancestor: bool,
    pub report: bool,
}

#[derive(sqlx::FromRow)]
struct UnitRow {
    id: Uuid,
    parent_id: Option<Uuid>,
    slug: String,
}

impl From<UnitRow> for ScopeUnit {
    fn from(row: UnitRow) -> Self {
        Self {
            id: DepartmentId::new(row.id),
            parent: row.parent_id.map(DepartmentId::new),
            slug: row.slug,
        }
    }
}

pub(super) async fn capture_on(
    db: &mut PgConnection,
    command: &PreparedAdmission,
    manifest: &DependencyManifest,
) -> AppResult<()> {
    let company = command.company_id();
    let run = command.proposed_run_id();
    let root = root_on(db, company).await?;
    let columns = association::association_columns(command.association());
    let base_chain = chain_on(db, company, columns.channel_id, columns.principal_id).await?;
    let facts = ScopeRoleFacts {
        root: root.clone(),
        leads: Vec::new(),
    };
    // No role facts enter the static base projection; only its chain confers authority.
    let base = run_scopes(&base_chain, PrincipalId::new(Uuid::nil()), &facts)?;
    insert_units_on(db, company, run, None, &base).await?;
    for agent in &manifest.agents {
        let principal = crate::adapters::persistence::memory::agent_principal_on(
            db,
            company.as_uuid(),
            agent.agent_id.as_uuid(),
        )
        .await?;
        capture_agent_on(db, company, run, principal, columns.channel_id, &root).await?;
    }
    if let Some(user) =
        super::checkpoints::memory_tools::user_scope_owner_on(db, company, run).await?
    {
        let person =
            crate::adapters::persistence::memory::person_principal_on(db, company.as_uuid(), user)
                .await?;
        insert_personal_on(db, company, run, None, person).await?;
    }
    seal_on(db, company, run, SnapshotState::Available).await
}

async fn root_on(db: &mut PgConnection, company: CompanyId) -> AppResult<ScopeUnit> {
    let row: UnitRow = sqlx::query_as(
        "SELECT id,parent_id,slug::text AS slug FROM departments WHERE company_id=$1 AND is_root",
    )
    .bind(company.as_uuid())
    .fetch_one(db)
    .await?;
    Ok(row.into())
}

async fn chain_on(
    db: &mut PgConnection,
    company: CompanyId,
    desk: Option<Uuid>,
    target: Option<Uuid>,
) -> AppResult<Vec<ScopeUnit>> {
    let placements: Vec<Uuid> = sqlx::query_scalar(
        "SELECT department FROM workflow_target_departments($1,$2,$3) AS department",
    )
    .bind(company.as_uuid())
    .bind(desk)
    .bind(target)
    .fetch_all(&mut *db)
    .await?;
    let [local] = placements.as_slice() else {
        return Ok(Vec::new());
    };
    let rows: Vec<UnitRow> = sqlx::query_as(
        "WITH RECURSIVE chain AS (
            SELECT id,parent_id,slug,0 AS depth FROM departments WHERE company_id=$1 AND id=$2
            UNION ALL SELECT parent.id,parent.parent_id,parent.slug,chain.depth+1
            FROM departments AS parent JOIN chain ON parent.id=chain.parent_id
            WHERE parent.company_id=$1 AND chain.depth<$3
         ) SELECT id,parent_id,slug::text AS slug FROM chain ORDER BY depth",
    )
    .bind(company.as_uuid())
    .bind(local)
    .bind(MAX_DEPARTMENT_DEPTH as i32)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

async fn capture_agent_on(
    db: &mut PgConnection,
    company: CompanyId,
    run: RunId,
    principal: PrincipalId,
    desk: Option<Uuid>,
    root: &ScopeUnit,
) -> AppResult<()> {
    let chain = chain_on(
        db,
        company,
        desk,
        desk.is_none().then_some(principal.as_uuid()),
    )
    .await?;
    let mut facts = ScopeRoleFacts {
        root: root.clone(),
        leads: Vec::new(),
    };
    if !chain.is_empty() {
        let rows: Vec<UnitRow> = sqlx::query_as(
            "SELECT child.id,child.parent_id,child.slug::text AS slug
             FROM departments AS child JOIN department_roles AS holder
                ON holder.company_id=child.company_id AND holder.department_id=child.parent_id
             WHERE child.company_id=$1 AND holder.principal_id=$2 AND holder.role='lead'
             ORDER BY child.id LIMIT $3",
        )
        .bind(company.as_uuid())
        .bind(principal.as_uuid())
        .bind((MAX_RUN_READ_UNITS + 1) as i64)
        .fetch_all(&mut *db)
        .await?;
        if rows.len() > MAX_RUN_READ_UNITS {
            return Err(AppError::BadRequest("memory.scope_limit_exceeded".into()));
        }
        for row in rows {
            let child: ScopeUnit = row.into();
            facts.leads.push(LeadChildren {
                principal,
                department: child.parent.ok_or_else(unavailable)?,
                children: vec![child],
            });
        }
    }
    let scopes = run_scopes(&chain, principal, &facts)?;
    insert_units_on(db, company, run, Some(principal), &scopes).await?;
    insert_personal_on(db, company, run, Some(principal), principal).await
}

async fn insert_units_on(
    db: &mut PgConnection,
    company: CompanyId,
    run: RunId,
    actor: Option<PrincipalId>,
    scopes: &RunScopes,
) -> AppResult<()> {
    for unit in &scopes.units {
        insert_unit_on(db, company, run, actor, unit, "read").await?;
        if scopes.write_unit == Some(unit.unit.id) {
            insert_unit_on(db, company, run, actor, unit, "write").await?;
        }
    }
    Ok(())
}

async fn insert_unit_on(
    db: &mut PgConnection,
    company: CompanyId,
    run: RunId,
    actor: Option<PrincipalId>,
    unit: &ReadUnit,
    access: &str,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO workflow_run_scopes(company_id,run_id,acting_principal_id,unit_id,access,
        unit_slug,chain_depth,own,ancestor,report) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
    )
    .bind(company.as_uuid())
    .bind(run.as_uuid())
    .bind(actor.map(|id| id.as_uuid()))
    .bind(unit.unit.id.as_uuid())
    .bind(access)
    .bind(&unit.unit.slug)
    .bind(unit.depth.map(|depth| depth as i16))
    .bind(unit.own)
    .bind(unit.ancestor)
    .bind(unit.report)
    .execute(db)
    .await?;
    Ok(())
}

async fn insert_personal_on(
    db: &mut PgConnection,
    company: CompanyId,
    run: RunId,
    actor: Option<PrincipalId>,
    principal: PrincipalId,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO workflow_run_scopes(company_id,run_id,acting_principal_id,principal_id,access)
        VALUES($1,$2,$3,$4,'personal')",
    )
    .bind(company.as_uuid())
    .bind(run.as_uuid())
    .bind(actor.map(|id| id.as_uuid()))
    .bind(principal.as_uuid())
    .execute(db)
    .await?;
    Ok(())
}

enum SnapshotState {
    Available,
    Unavailable,
}

async fn seal_on(
    db: &mut PgConnection,
    company: CompanyId,
    run: RunId,
    state: SnapshotState,
) -> AppResult<()> {
    let (state, reason) = match state {
        SnapshotState::Available => ("available", None),
        SnapshotState::Unavailable => ("unavailable", Some("source_scope_unavailable")),
    };
    sqlx::query("INSERT INTO workflow_run_scope_snapshots(company_id,run_id,state,reason) VALUES($1,$2,$3,$4)")
        .bind(company.as_uuid()).bind(run.as_uuid()).bind(state).bind(reason).execute(db).await?;
    Ok(())
}

pub(super) async fn copy_child_on(
    db: &mut PgConnection,
    company: CompanyId,
    parent: RunId,
    child: RunId,
) -> AppResult<()> {
    require_available_on(db, company, parent).await?;
    sqlx::query("INSERT INTO workflow_run_scopes(company_id,run_id,acting_principal_id,unit_id,principal_id,
        access,unit_slug,chain_depth,own,ancestor,report)
        SELECT $1,$3,scope.* FROM workflow_scope_tuples($1,$2) AS scope")
        .bind(company.as_uuid()).bind(parent.as_uuid()).bind(child.as_uuid()).execute(&mut *db).await?;
    seal_on(db, company, child, SnapshotState::Available).await
}

pub(super) async fn require_available_on(
    db: &mut PgConnection,
    company: CompanyId,
    run: RunId,
) -> AppResult<()> {
    let state: Option<String> = sqlx::query_scalar(
        "SELECT state FROM workflow_run_scope_snapshots WHERE company_id=$1 AND run_id=$2",
    )
    .bind(company.as_uuid())
    .bind(run.as_uuid())
    .fetch_optional(db)
    .await?;
    if state.as_deref() != Some("available") {
        return Err(unavailable());
    }
    Ok(())
}

pub(super) async fn native_on(
    db: &mut PgConnection,
    owner: &super::native_mcp::NativeMcpOwner,
    run: RunId,
) -> AppResult<()> {
    use crate::domain::local_agents::{NativeHarnessSelection, SessionAssociation};
    let NativeHarnessSelection::Agent { agent_id, .. } = owner.harness else {
        return Err(unavailable());
    };
    let principal = crate::adapters::persistence::memory::agent_principal_on(
        db,
        owner.company.as_uuid(),
        agent_id.as_uuid(),
    )
    .await?;
    if let SessionAssociation::Workflow { run_id } = owner.source {
        let available: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM workflow_run_scope_snapshots AS seal
                JOIN workflow_run_scopes AS scope ON scope.company_id=seal.company_id AND scope.run_id=seal.run_id
                WHERE seal.company_id=$1 AND seal.run_id=$2 AND seal.state='available'
                    AND scope.acting_principal_id=$3 AND scope.access='personal' AND scope.principal_id=$3)",
        ).bind(owner.company.as_uuid()).bind(run_id).bind(principal.as_uuid()).fetch_one(&mut *db).await?;
        if available {
            sqlx::query("INSERT INTO workflow_run_scopes(company_id,run_id,acting_principal_id,unit_id,principal_id,
                access,unit_slug,chain_depth,own,ancestor,report)
                SELECT $1,$3,scope.* FROM workflow_native_scope_tuples($1,$2,$4,$5) AS scope")
                .bind(owner.company.as_uuid()).bind(run_id).bind(run.as_uuid()).bind(principal.as_uuid())
                .bind(owner.actor.user_id()).execute(&mut *db).await?;
        }
        return seal_on(
            db,
            owner.company,
            run,
            if available {
                SnapshotState::Available
            } else {
                SnapshotState::Unavailable
            },
        )
        .await;
    }
    let root = root_on(db, owner.company).await?;
    let columns = association::association_columns(owner.association);
    let chain = chain_on(db, owner.company, columns.channel_id, columns.principal_id).await?;
    let base = run_scopes(
        &chain,
        principal,
        &ScopeRoleFacts {
            root: root.clone(),
            leads: vec![],
        },
    )?;
    insert_units_on(db, owner.company, run, None, &base).await?;
    capture_agent_on(db, owner.company, run, principal, columns.channel_id, &root).await?;
    seal_on(db, owner.company, run, SnapshotState::Available).await
}

pub(super) async fn rows_on(
    db: &mut PgConnection,
    company: CompanyId,
    run: RunId,
    actor: Option<PrincipalId>,
) -> AppResult<Vec<ScopeRow>> {
    require_available_on(db, company, run).await?;
    let rows: Vec<ScopeRow> = sqlx::query_as("SELECT acting_principal_id,unit_id,principal_id,access,unit_slug,
        chain_depth,own,ancestor,report FROM workflow_run_scopes WHERE company_id=$1 AND run_id=$2
        AND (acting_principal_id IS NOT DISTINCT FROM $3::uuid OR (acting_principal_id IS NULL AND access='personal'))
        ORDER BY access,unit_id,principal_id")
        .bind(company.as_uuid()).bind(run.as_uuid()).bind(actor.map(|id| id.as_uuid())).fetch_all(db).await?;
    if rows.is_empty()
        || (actor.is_some()
            && !rows.iter().any(|row| {
                row.acting_principal_id == actor.map(|id| id.as_uuid()) && row.access == "personal"
            }))
    {
        return Err(unavailable());
    }
    Ok(rows)
}

pub(super) async fn context_on(
    db: &mut PgConnection,
    company: CompanyId,
    run: RunId,
    agent: crate::entities::agent_capability::AgentId,
) -> AppResult<crate::application::workflow::scopes::RunScopeContext> {
    let principal = crate::adapters::persistence::memory::agent_principal_on(
        db,
        company.as_uuid(),
        agent.as_uuid(),
    )
    .await?;
    let mut rows = rows_on(db, company, run, Some(principal)).await?;
    let write_unit = rows
        .iter()
        .find(|row| row.access == "write")
        .and_then(|row| row.unit_slug.clone());
    rows.sort_by(|left, right| {
        left.chain_depth
            .unwrap_or(8)
            .cmp(&right.chain_depth.unwrap_or(8))
            .then(left.unit_slug.cmp(&right.unit_slug))
    });
    let readable_units = rows
        .into_iter()
        .filter(|row| row.access == "read")
        .map(|row| row.unit_slug.ok_or_else(unavailable))
        .collect::<AppResult<_>>()?;
    let context = crate::application::workflow::scopes::RunScopeContext {
        write_unit,
        readable_units,
    };
    context.validate()?;
    Ok(context)
}
