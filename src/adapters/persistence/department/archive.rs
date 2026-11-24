//! Archiving and restoring a unit (Plan08 02A/02B.1). Both run under the company write lock that
//! every department write and every run admission takes, so an admission either commits first
//! (its open run refuses the archive) or reads the archived unit and refuses itself.
use super::*;
use crate::application::ports::department::OpenWork;

/// Cancel-and-archive ends at most this many root runs in one write; more are cancelled from the
/// run board first, so the department transaction stays bounded.
pub(crate) const MAX_ARCHIVE_CANCELLATIONS: i64 = 64;

/// The principals with a member or role row in `units`, ordered: those whose role-binding
/// context a change to the units can move.
pub(super) async fn placed_principals_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    units: &[Uuid],
) -> AppResult<Vec<Uuid>> {
    Ok(sqlx::query_scalar(
        "SELECT member.principal_id FROM department_members AS member \
          WHERE member.company_id = $1 AND member.department_id = ANY($2) \
         UNION \
         SELECT holder.principal_id FROM department_roles AS holder \
          WHERE holder.company_id = $1 AND holder.department_id = ANY($2) \
         ORDER BY 1",
    )
    .bind(company.as_uuid())
    .bind(units)
    .fetch_all(&mut **tx)
    .await?)
}

/// `unit` and every unit below it, ordered by id: the subtree a single archive covers.
async fn subtree_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    unit: DepartmentId,
) -> AppResult<Vec<Uuid>> {
    // Depth is bounded by the eight-level tree; the walk stops at ten even on a corrupt tree.
    Ok(sqlx::query_scalar(
        "WITH RECURSIVE subtree(id, depth) AS ( \
             SELECT id, 1 FROM departments WHERE company_id = $1 AND id = $2 \
             UNION ALL \
             SELECT child.id, subtree.depth + 1 FROM subtree \
             JOIN departments AS child ON child.company_id = $1 AND child.parent_id = subtree.id \
             WHERE subtree.depth < 10) \
         SELECT department.id FROM departments AS department JOIN subtree USING (id) \
          WHERE department.company_id = $1 ORDER BY department.id FOR UPDATE OF department",
    )
    .bind(company.as_uuid())
    .bind(unit.as_uuid())
    .fetch_all(&mut **tx)
    .await?)
}

/// Unfinished runs on desks of `units`, and of those the roots and the native MCP calls.
#[derive(sqlx::FromRow)]
struct OpenWorkCount {
    open: i64,
    native: i64,
}

async fn open_work_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    units: &[Uuid],
) -> AppResult<OpenWorkCount> {
    Ok(sqlx::query_as(
        "SELECT count(*) AS open, count(*) FILTER (WHERE run.native_mcp_call_id IS NOT NULL) AS native \
           FROM workflow_runs AS run JOIN channels AS desk \
             ON desk.company_id = run.company_id AND desk.id = run.channel_id \
          WHERE run.company_id = $1 AND desk.department_id = ANY($2) AND run.finished_at IS NULL",
    )
    .bind(company.as_uuid())
    .bind(units)
    .fetch_one(&mut **tx)
    .await?)
}

async fn desks_of_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    units: &[Uuid],
) -> AppResult<Vec<Uuid>> {
    Ok(sqlx::query_scalar(
        "SELECT id FROM channels WHERE company_id = $1 AND department_id = ANY($2) ORDER BY id",
    )
    .bind(company.as_uuid())
    .bind(units)
    .fetch_all(&mut **tx)
    .await?)
}

/// Supersede mail admissions in flight on these desks: routing reads the unit state with the
/// desk's `settings_revision`, so taking each settings gate and bumping the revision makes an
/// admission that read the old state fail its commit-time recheck. `channels.enabled` is not
/// touched, so a restore returns each desk to exactly the state it had.
async fn supersede_routing_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    desks: &[Uuid],
) -> AppResult<()> {
    if desks.is_empty() {
        return Ok(());
    }
    crate::adapters::persistence::channel_gate::acquire_channel_gates_on(
        tx,
        desks.iter().map(|desk| {
            crate::adapters::persistence::channel_gate::ChannelGateKey::new(
                company.as_uuid(),
                *desk,
            )
        }),
        crate::adapters::persistence::channel_gate::ChannelGateAccess::ChangeSettings,
    )
    .await?;
    sqlx::query(
        "UPDATE channels SET settings_revision = settings_revision + 1 \
          WHERE company_id = $1 AND id = ANY($2)",
    )
    .bind(company.as_uuid())
    .bind(desks)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn open_work(count: i64, cancellable: bool) -> AppError {
    AppError::UnitArchiveOpenWork { count, cancellable }
}

pub(crate) async fn archive_department_on(
    tx: &mut Transaction<'_, Postgres>,
    scope: DepartmentScope,
    acting: ManagementRole,
    unit: DepartmentId,
    open: OpenWork,
) -> AppResult<Department> {
    let company = scope.company;
    let current = lock_department_on(tx, company, unit).await?;
    if current.is_root {
        return Err(AppError::BadRequest(
            "The company root cannot be archived.".into(),
        ));
    }
    if current.is_archived() {
        return Err(AppError::UnitArchived);
    }
    let units = subtree_on(tx, company, unit).await?;
    let desks = desks_of_on(tx, company, &units).await?;
    // Desk gates come before any desk or run row lock, as every settings change takes them.
    supersede_routing_on(tx, company, &desks).await?;
    let work = open_work_on(tx, company, &units).await?;
    if work.open > 0 {
        // The roots on the subtree's desks, then the recorded channel-call children on them whose
        // caller is elsewhere (agent-workflow-call p03 D6): a caller on the subtree is cancelled
        // with its own tree. Roots go first, so a cascade locks a parent before its child.
        let cancellable_runs: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM ( \
               SELECT run.id, 0 AS kind FROM workflow_runs AS run JOIN channels AS desk \
                 ON desk.company_id = run.company_id AND desk.id = run.channel_id \
                WHERE run.company_id = $1 AND desk.department_id = ANY($2) \
                  AND run.finished_at IS NULL AND run.parent_run_id IS NULL \
               UNION ALL \
               SELECT child.id, 1 FROM workflow_channel_calls AS call \
                 JOIN workflow_runs AS child ON child.company_id = call.company_id AND child.id = call.child_run_id \
                 JOIN channels AS desk ON desk.company_id = child.company_id AND desk.id = child.channel_id \
                 JOIN workflow_runs AS caller ON caller.company_id = call.company_id AND caller.id = call.run_id \
                 LEFT JOIN channels AS caller_desk \
                   ON caller_desk.company_id = caller.company_id AND caller_desk.id = caller.channel_id \
                WHERE call.company_id = $1 AND desk.department_id = ANY($2) AND child.finished_at IS NULL \
                  AND NOT COALESCE(caller_desk.department_id = ANY($2), false) \
             ) AS cancellable ORDER BY kind, id LIMIT $3",
        )
        .bind(company.as_uuid())
        .bind(&units)
        .bind(MAX_ARCHIVE_CANCELLATIONS + 1)
        .fetch_all(&mut **tx)
        .await?;
        // A native MCP call is ended by its local agent, never by a department write.
        let cancellable =
            work.native == 0 && cancellable_runs.len() as i64 <= MAX_ARCHIVE_CANCELLATIONS;
        if open == OpenWork::Refuse || !cancellable {
            return Err(open_work(work.open, cancellable));
        }
        for run in cancellable_runs {
            crate::adapters::persistence::workflow::cancel_for_archive_on(
                tx,
                company,
                crate::domain::workflow::RunId::new(run),
                scope.actor,
            )
            .await?;
        }
        let remaining = open_work_on(tx, company, &units).await?;
        if remaining.open > 0 {
            return Err(open_work(remaining.open, false));
        }
    }
    // A placement in an archived unit places nothing (02C.4), so an archive moves its agents'
    // role-binding context exactly as removing them would: lock them first and recheck after.
    let placed = placed_principals_on(tx, company, &units).await?;
    lock_principals_on(tx, company, &placed).await?;
    let archived: Vec<Uuid> = sqlx::query_scalar(
        "UPDATE departments AS department SET archived_at = clock_timestamp(), \
             archived_by = jsonb_build_object('actor_type', 'user', 'actor_id', actor.user_id::text, \
                 'actor_name', actor.display_label) \
           FROM principals AS actor \
          WHERE department.company_id = $1 AND department.id = ANY($2) \
            AND department.archived_at IS NULL \
            AND actor.company_id = $1 AND actor.user_id = $3 AND actor.kind = 'person' \
         RETURNING department.id",
    )
    .bind(company.as_uuid())
    .bind(&units)
    .bind(scope.actor.user_id())
    .fetch_all(&mut **tx)
    .await?;
    if archived.is_empty() {
        return Err(AppError::Internal(format!(
            "Archiving department {unit} found no current person actor"
        )));
    }
    super::super::workflow::role_binding::check_principals_on(tx, company, &placed).await?;
    let mut archived = archived;
    archived.sort_unstable();
    for department in archived {
        audit::record_on(
            tx,
            company,
            scope.actor,
            acting,
            DepartmentId::new(department),
            AdministrationEvent::Archived,
        )
        .await?;
    }
    lock_department_on(tx, company, unit).await
}

/// Restore `unit` alone; its parent must be active, and its children stay archived until each is
/// restored in turn.
pub(crate) async fn restore_department_on(
    tx: &mut Transaction<'_, Postgres>,
    scope: DepartmentScope,
    acting: ManagementRole,
    unit: DepartmentId,
) -> AppResult<Department> {
    let company = scope.company;
    let current = lock_department_on(tx, company, unit).await?;
    if !current.is_archived() {
        return Err(AppError::BadRequest(
            "This department is not archived.".into(),
        ));
    }
    if let Some(parent) = current.parent_id
        && lock_parent_on(tx, company, parent).await?.is_archived()
    {
        return Err(AppError::UnitArchived);
    }
    let desks = desks_of_on(tx, company, &[unit.as_uuid()]).await?;
    supersede_routing_on(tx, company, &desks).await?;
    // A restored placement counts again and can make an agent's context ambiguous.
    let placed = placed_principals_on(tx, company, &[unit.as_uuid()]).await?;
    lock_principals_on(tx, company, &placed).await?;
    sqlx::query(
        "UPDATE departments SET archived_at = NULL, archived_by = NULL \
          WHERE company_id = $1 AND id = $2",
    )
    .bind(company.as_uuid())
    .bind(unit.as_uuid())
    .execute(&mut **tx)
    .await?;
    super::super::workflow::role_binding::check_principals_on(tx, company, &placed).await?;
    audit::record_on(
        tx,
        company,
        scope.actor,
        acting,
        unit,
        AdministrationEvent::Restored,
    )
    .await?;
    lock_department_on(tx, company, unit).await
}

/// Refuse a configuration write on an archived unit (Plan08 02B.4). Locks the unit row
/// `FOR KEY SHARE`, which waits for an archive in progress, so callers that do not hold the
/// company write lock are safe too.
pub(crate) async fn require_active_unit_on(
    db: &mut sqlx::PgConnection,
    company: CompanyId,
    unit: DepartmentId,
) -> AppResult<()> {
    let archived: Option<bool> = sqlx::query_scalar(
        "SELECT archived_at IS NOT NULL FROM departments \
          WHERE company_id = $1 AND id = $2 FOR KEY SHARE",
    )
    .bind(company.as_uuid())
    .bind(unit.as_uuid())
    .fetch_optional(&mut *db)
    .await?;
    match archived {
        Some(true) => Err(AppError::UnitArchived),
        _ => Ok(()),
    }
}

/// [`require_active_unit_on`] for the unit that owns `desk`. A missing desk is left to the
/// caller's own lookup.
pub(crate) async fn require_active_desk_on(
    db: &mut sqlx::PgConnection,
    company: CompanyId,
    desk: Uuid,
) -> AppResult<()> {
    let archived: Option<bool> = sqlx::query_scalar(
        "SELECT unit.archived_at IS NOT NULL FROM channels AS desk \
           JOIN departments AS unit ON unit.company_id = desk.company_id AND unit.id = desk.department_id \
          WHERE desk.company_id = $1 AND desk.id = $2 FOR KEY SHARE OF unit",
    )
    .bind(company.as_uuid())
    .bind(desk)
    .fetch_optional(&mut *db)
    .await?;
    match archived {
        Some(true) => Err(AppError::UnitArchived),
        _ => Ok(()),
    }
}
