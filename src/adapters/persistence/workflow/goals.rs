//! The goal a new run or thread records (Plan08 03B, gate 08G-P03-D1 r2 D1.4–D1.5).
//!
//! Every candidate unit is read under `FOR SHARE`, in id order, so a kind change or an archive of
//! it either waits for this write or is seen by it; the insert guard checks the same two facts
//! under the same lock.
use super::*;
use crate::application::workflow::goals::{GoalCandidate, GoalSources, goal_of};
use crate::entities::department::{DepartmentId, DepartmentKind, MAX_DEPARTMENT_DEPTH};

/// Where a new run comes from, for resolving its goal.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct GoalOrigin {
    /// The run this one reruns.
    pub rerun_of: Option<RunId>,
    /// The parent run, or a native call's source run.
    pub parent: Option<RunId>,
    /// The existing thread the run is on.
    pub thread: Option<Uuid>,
    /// The run's desk and principal target, as its association names them.
    pub desk: Option<Uuid>,
    pub target: Option<Uuid>,
}

impl GoalOrigin {
    pub(super) fn of(association: RelatedAssociation) -> Self {
        let columns = association::association_columns(association);
        Self {
            thread: columns.thread_id,
            desk: columns.channel_id,
            target: columns.principal_id,
            ..Self::default()
        }
    }
}

#[derive(sqlx::FromRow)]
struct UnitState {
    id: Uuid,
    kind: String,
    archived: bool,
}

/// The goal a run with this origin records, or `None`.
pub(super) async fn resolve_on(
    db: &mut PgConnection,
    company: CompanyId,
    origin: GoalOrigin,
) -> AppResult<Option<DepartmentId>> {
    let goal_of_run = |run: Option<RunId>| run.map(RunId::as_uuid);
    let inherited: (Option<Uuid>, Option<Uuid>, Option<Uuid>) = sqlx::query_as(
        "SELECT (SELECT goal_unit_id FROM workflow_runs WHERE company_id = $1 AND id = $2), \
                (SELECT goal_unit_id FROM workflow_runs WHERE company_id = $1 AND id = $3), \
                (SELECT goal_unit_id FROM threads WHERE company_id = $1 AND id = $4)",
    )
    .bind(company.as_uuid())
    .bind(goal_of_run(origin.rerun_of))
    .bind(goal_of_run(origin.parent))
    .bind(origin.thread)
    .fetch_one(&mut *db)
    .await?;
    let chain: Vec<Uuid> = sqlx::query_scalar(
        "WITH RECURSIVE placed AS ( \
            SELECT department FROM workflow_target_departments($1, $2, $3) AS department \
         ), chain AS ( \
            SELECT unit.id, unit.parent_id, 0 AS depth FROM departments AS unit \
              WHERE unit.company_id = $1 AND unit.id IN (SELECT department FROM placed) \
                AND (SELECT count(*) FROM placed) = 1 \
            UNION ALL \
            SELECT parent.id, parent.parent_id, chain.depth + 1 FROM departments AS parent \
              JOIN chain ON parent.company_id = $1 AND parent.id = chain.parent_id \
             WHERE chain.depth + 1 < $4 \
         ) SELECT id FROM chain ORDER BY depth",
    )
    .bind(company.as_uuid())
    .bind(origin.desk)
    .bind(origin.target)
    .bind(MAX_DEPARTMENT_DEPTH as i32)
    .fetch_all(&mut *db)
    .await?;
    let mut units: Vec<Uuid> = [inherited.0, inherited.1, inherited.2]
        .into_iter()
        .flatten()
        .chain(chain.iter().copied())
        .collect();
    units.sort_unstable();
    units.dedup();
    if units.is_empty() {
        return Ok(None);
    }
    let states: Vec<UnitState> = sqlx::query_as(
        "SELECT id, kind, archived_at IS NOT NULL AS archived FROM departments \
          WHERE company_id = $1 AND id = ANY($2) ORDER BY id FOR SHARE",
    )
    .bind(company.as_uuid())
    .bind(&units)
    .fetch_all(&mut *db)
    .await?;
    let candidate = |unit: Option<Uuid>| -> Option<GoalCandidate> {
        let state = states.iter().find(|state| Some(state.id) == unit)?;
        Some(GoalCandidate {
            unit: DepartmentId::new(state.id),
            kind: DepartmentKind::parse(&state.kind).unwrap_or_default(),
            archived: state.archived,
        })
    };
    Ok(goal_of(&GoalSources {
        rerun: candidate(inherited.0),
        parent: candidate(inherited.1),
        thread: candidate(inherited.2),
        chain: chain
            .iter()
            .filter_map(|unit| candidate(Some(*unit)))
            .collect(),
    }))
}

/// The goal a thread created for `caller` records (`channel.call`, 03B.3): the caller's goal while
/// it is still a live goal, under the same share lock.
pub(super) async fn for_thread_of_on(
    db: &mut PgConnection,
    company: CompanyId,
    caller: RunId,
) -> AppResult<Option<Uuid>> {
    let origin = GoalOrigin {
        parent: Some(caller),
        ..GoalOrigin::default()
    };
    Ok(resolve_on(db, company, origin)
        .await?
        .map(DepartmentId::as_uuid))
}
