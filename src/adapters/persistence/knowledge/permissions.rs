//! Current manual authority. Frozen workflow authority is loaded separately from its sealed run.
use super::*;
use crate::{
    application::workflow::scopes::{
        LeadChildren, MAX_RUN_READ_UNITS, ScopeRoleFacts, ScopeUnit, run_scopes,
    },
    entities::{
        department::{DepartmentId, MAX_DEPARTMENT_DEPTH},
        transport::PrincipalId,
    },
};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub(super) enum UnitSelection {
    All,
    Units(Vec<Uuid>),
}

impl UnitSelection {
    pub(super) fn parameter(&self) -> Option<&[Uuid]> {
        match self {
            Self::All => None,
            Self::Units(units) => Some(units),
        }
    }
}

pub(super) struct Permissions {
    pub(super) actor: LevelActor,
    pub(super) read: UnitSelection,
    write: UnitSelection,
    pub(super) default_unit: Option<DepartmentId>,
}

struct OrdinaryUnits {
    read: Vec<Uuid>,
    write: Vec<Uuid>,
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

#[derive(sqlx::FromRow)]
struct ChainRow {
    origin: Uuid,
    id: Uuid,
    parent_id: Option<Uuid>,
    slug: String,
}

pub(super) async fn load(
    tx: &mut Transaction<'_, Postgres>,
    scope: KnowledgeScope,
) -> AppResult<Permissions> {
    let actor = authorize(tx, scope).await?;
    let root: Uuid =
        sqlx::query_scalar("SELECT id FROM departments WHERE company_id=$1 AND is_root")
            .bind(scope.company.as_uuid())
            .fetch_one(&mut **tx)
            .await
            .map_err(authority::database)?;
    let principal: Uuid = sqlx::query_scalar(
        "SELECT id FROM principals WHERE company_id=$1 AND user_id=$2 AND kind='person'",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.actor_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(authority::database)?;
    let placements: Vec<Uuid> = sqlx::query_scalar(
        "SELECT department_id FROM (
            SELECT department_id FROM department_members WHERE company_id=$1 AND principal_id=$2 AND root_user_id IS NULL
            UNION SELECT department_id FROM department_roles WHERE company_id=$1 AND principal_id=$2
         ) AS placement ORDER BY department_id LIMIT $3",
    ).bind(scope.company.as_uuid()).bind(principal).bind((MAX_RUN_READ_UNITS + 1) as i64)
        .fetch_all(&mut **tx).await.map_err(authority::database)?;
    let default_unit = match placements.as_slice() {
        [unit] => Some(DepartmentId::new(*unit)),
        [] if actor.manages_levels() => Some(DepartmentId::new(root)),
        _ => None,
    };
    if actor.manages_levels() {
        return Ok(Permissions {
            actor,
            read: UnitSelection::All,
            write: UnitSelection::All,
            default_unit,
        });
    }
    if placements.len() > MAX_RUN_READ_UNITS {
        return Err(scope_limit());
    }
    let units = ordinary(tx, scope, principal, &placements)
        .await
        .map_err(projection_error)?;
    Ok(Permissions {
        actor,
        read: UnitSelection::Units(units.read),
        write: UnitSelection::Units(units.write),
        default_unit,
    })
}

async fn ordinary(
    tx: &mut Transaction<'_, Postgres>,
    scope: KnowledgeScope,
    principal: Uuid,
    placements: &[Uuid],
) -> AppResult<OrdinaryUnits> {
    let root: UnitRow = sqlx::query_as(
        "SELECT id,parent_id,slug::text AS slug FROM departments WHERE company_id=$1 AND is_root",
    )
    .bind(scope.company.as_uuid())
    .fetch_one(&mut **tx)
    .await
    .map_err(authority::database)?;
    let children: Vec<UnitRow> = sqlx::query_as(
        "SELECT child.id,child.parent_id,child.slug::text AS slug FROM departments AS child
         JOIN department_roles AS holder ON holder.company_id=child.company_id AND holder.department_id=child.parent_id
         WHERE child.company_id=$1 AND holder.principal_id=$2 AND holder.role='lead' ORDER BY child.id LIMIT $3",
    ).bind(scope.company.as_uuid()).bind(principal).bind((MAX_RUN_READ_UNITS + 1) as i64)
        .fetch_all(&mut **tx).await.map_err(authority::database)?;
    if children.len() > MAX_RUN_READ_UNITS {
        return Err(scope_limit());
    }
    let principal = PrincipalId::new(principal);
    let mut write: BTreeSet<Uuid> = placements.iter().copied().collect();
    let mut facts = ScopeRoleFacts {
        root: root.into(),
        leads: Vec::new(),
    };
    for child in children {
        write.insert(child.id);
        let unit: ScopeUnit = child.into();
        facts.leads.push(LeadChildren {
            principal,
            department: unit.parent.ok_or_else(scope_limit)?,
            children: vec![unit],
        });
    }
    let chains = chains(tx, scope, placements).await?;
    let mut read = BTreeSet::new();
    if chains.is_empty() {
        read.extend(
            run_scopes(&[], principal, &facts)?
                .units
                .into_iter()
                .map(|unit| unit.unit.id.as_uuid()),
        );
    }
    for chain in chains.values() {
        read.extend(
            run_scopes(chain, principal, &facts)?
                .units
                .into_iter()
                .map(|unit| unit.unit.id.as_uuid()),
        );
        if read.len() > MAX_RUN_READ_UNITS {
            return Err(scope_limit());
        }
    }
    Ok(OrdinaryUnits {
        read: read.into_iter().collect(),
        write: write.into_iter().collect(),
    })
}

async fn chains(
    tx: &mut Transaction<'_, Postgres>,
    scope: KnowledgeScope,
    placements: &[Uuid],
) -> AppResult<BTreeMap<Uuid, Vec<ScopeUnit>>> {
    let rows: Vec<ChainRow> = sqlx::query_as(
        "WITH RECURSIVE chain AS (
            SELECT id AS origin,id,parent_id,slug,0 AS depth FROM departments WHERE company_id=$1 AND id=ANY($2)
            UNION ALL SELECT chain.origin,parent.id,parent.parent_id,parent.slug,chain.depth+1
            FROM departments AS parent JOIN chain ON parent.id=chain.parent_id
            WHERE parent.company_id=$1 AND chain.depth<$3
         ) SELECT origin,id,parent_id,slug::text AS slug FROM chain ORDER BY origin,depth LIMIT $4",
    ).bind(scope.company.as_uuid()).bind(placements).bind(MAX_DEPARTMENT_DEPTH as i32)
        .bind((MAX_RUN_READ_UNITS * (MAX_DEPARTMENT_DEPTH + 1) + 1) as i64)
        .fetch_all(&mut **tx).await.map_err(authority::database)?;
    let mut chains: BTreeMap<Uuid, Vec<ScopeUnit>> = BTreeMap::new();
    for row in rows {
        chains.entry(row.origin).or_default().push(
            UnitRow {
                id: row.id,
                parent_id: row.parent_id,
                slug: row.slug,
            }
            .into(),
        );
    }
    if chains.len() != placements.len() {
        return Err(scope_limit());
    }
    Ok(chains)
}

impl Permissions {
    pub(super) fn writable_visible_units(
        &self,
        layers: &[KnowledgeDocumentLayer],
    ) -> Vec<DepartmentId> {
        layers
            .iter()
            .map(|layer| layer.unit_id)
            .filter(|unit| match &self.write {
                UnitSelection::All => true,
                UnitSelection::Units(units) => units.contains(&unit.as_uuid()),
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub(super) fn default_for(&self, selected: Option<DepartmentId>) -> Option<DepartmentId> {
        selected
            .filter(|unit| match &self.write {
                UnitSelection::All => true,
                UnitSelection::Units(units) => units.contains(&unit.as_uuid()),
            })
            .or(self.default_unit)
    }

    pub(super) async fn require_read(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        scope: KnowledgeScope,
        unit: DepartmentId,
    ) -> AppResult<()> {
        authorize_unit(tx, scope, unit, &self.read).await
    }

    pub(super) async fn require_write(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        target: KnowledgeUploadTarget,
    ) -> AppResult<()> {
        authorize_unit(tx, target.scope, target.unit, &self.write).await
    }

    pub(super) async fn choices(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        scope: KnowledgeScope,
        query: KnowledgeUnitQuery,
    ) -> AppResult<KnowledgeUnitPage> {
        #[derive(sqlx::FromRow)]
        struct Choice {
            id: Uuid,
            name: String,
            is_root: bool,
        }
        let mut rows: Vec<Choice> = sqlx::query_as(
            "SELECT unit.id,unit.name,unit.is_root FROM departments AS unit
             WHERE unit.company_id=$1 AND ($2::uuid[] IS NULL OR unit.id=ANY($2))
                AND ($4::uuid IS NULL OR (NOT unit.is_root,lower(unit.name),unit.id) >
                    (SELECT NOT previous.is_root,lower(previous.name),previous.id FROM departments AS previous
                     WHERE previous.company_id=$1 AND previous.id=$4))
             ORDER BY unit.is_root DESC,lower(unit.name),unit.id LIMIT $3",
        ).bind(scope.company.as_uuid()).bind(self.write.parameter()).bind((MAX_RUN_READ_UNITS + 1) as i64)
            .bind(query.after.map(DepartmentId::as_uuid))
            .fetch_all(&mut **tx).await.map_err(authority::database)?;
        let truncated = rows.len() > MAX_RUN_READ_UNITS;
        rows.truncate(MAX_RUN_READ_UNITS);
        let next = if truncated {
            rows.last().map(|row| DepartmentId::new(row.id))
        } else {
            None
        };
        if let Some(selected) = query
            .selected
            .filter(|unit| !rows.iter().any(|row| row.id == unit.as_uuid()))
        {
            let row: Option<Choice> = sqlx::query_as("SELECT id,name,is_root FROM departments WHERE company_id=$1 AND id=$2 AND ($3::uuid[] IS NULL OR id=ANY($3))")
                .bind(scope.company.as_uuid()).bind(selected.as_uuid()).bind(self.write.parameter())
                .fetch_optional(&mut **tx).await.map_err(authority::database)?;
            rows.extend(row);
        }
        Ok(KnowledgeUnitPage {
            units: rows
                .into_iter()
                .map(|row| KnowledgeUnit {
                    id: DepartmentId::new(row.id),
                    name: row.name,
                    is_root: row.is_root,
                })
                .collect(),
            next,
        })
    }
}

async fn authorize_unit(
    tx: &mut Transaction<'_, Postgres>,
    scope: KnowledgeScope,
    unit: DepartmentId,
    selection: &UnitSelection,
) -> AppResult<()> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM departments WHERE company_id=$1 AND id=$2 AND ($3::uuid[] IS NULL OR id=ANY($3)))",
    ).bind(scope.company.as_uuid()).bind(unit.as_uuid()).bind(selection.parameter())
        .fetch_one(&mut **tx).await.map_err(authority::database)?;
    if !exists {
        return Err(AppError::NotFound("Department not found".into()));
    }
    Ok(())
}

fn scope_limit() -> AppError {
    AppError::BadRequest(KNOWLEDGE_SCOPE_LIMIT_EXCEEDED.into())
}

fn projection_error(error: AppError) -> AppError {
    match error {
        AppError::BadRequest(code)
            if code == crate::application::workflow::scopes::RUN_SCOPE_LIMIT_EXCEEDED =>
        {
            scope_limit()
        }
        other => other,
    }
}
