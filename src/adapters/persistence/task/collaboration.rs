//! Bounded collaboration-status projection.

use chrono::{DateTime, Utc};
use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
    time::{Duration, Instant},
};
use tracing::warn;
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        collaboration::{
            CollaborationNextAction, CollaborationOwner, CollaborationOwnerKind,
            CollaborationProgress, CollaborationSummary, CollaborationTarget,
            CollaborationTargetSummary, NextActionActor, NextActionKind, OutreachBusinessStatus,
            TargetBusinessStatus, outreach_business_status, target_business_status,
            task_business_status,
        },
        outreach::OutreachStatus,
        task::{TaskOwner, TaskStatus},
        transport::{
            DeliveryStatus, FailureClass, IdentityNamespace, IdentitySubject, QualifiedIdentity,
            TransportKind,
        },
    },
    task_queue::CollaborationReadScope,
};

use super::task_owner_from_db;

const MAX_COLLABORATION_NODES: usize = 100;
const MAX_COLLABORATION_DEPTH: usize = 5;
const SLOW_QUERY_THRESHOLD: Duration = Duration::from_millis(500);

#[derive(Clone, sqlx::FromRow)]
struct CollaborationTaskDb {
    id: Uuid,
    channel_id: Uuid,
    correlation_id: Uuid,
    status: String,
    owner_principal_id: Option<Uuid>,
    owner_principal_kind: Option<String>,
    owner_label: Option<String>,
    owner_available: bool,
    run_at: DateTime<Utc>,
}

#[derive(Clone, sqlx::FromRow)]
struct CollaborationTargetDb {
    id: Uuid,
    task_id: Uuid,
    outreach_id: Uuid,
    outreach_version: i64,
    outreach_status: String,
    required_threshold_percent: f64,
    expires_at: DateTime<Utc>,
    target_status: String,
    target_kind: String,
    internal_channel_id: Option<Uuid>,
    internal_channel_name: Option<String>,
    external_transport: Option<String>,
    external_namespace: Option<String>,
    external_subject: Option<String>,
    responded_at: Option<DateTime<Utc>>,
    delivery_status: Option<String>,
    delivery_failure_class: Option<String>,
    delivery_available_at: Option<DateTime<Utc>>,
    child_task_id: Option<Uuid>,
}

struct Projection<'a> {
    scope: CollaborationReadScope<'a>,
    as_of: DateTime<Utc>,
    tasks: HashMap<Uuid, CollaborationTaskDb>,
    targets: HashMap<Uuid, Vec<CollaborationTargetDb>>,
    remaining: usize,
    truncated: bool,
    visited: HashSet<Uuid>,
}

struct ProjectedOutreach {
    children: Vec<CollaborationTargetSummary>,
    statuses: Vec<TargetBusinessStatus>,
    stored_status: Option<OutreachStatus>,
    outreach_id: Option<Uuid>,
    outreach_version: Option<u64>,
    expires_at: Option<DateTime<Utc>>,
    threshold: f64,
    eligible_total: usize,
}

struct RenderedTargetIdentity {
    target: CollaborationTarget,
    label: String,
    visible: bool,
}

impl Projection<'_> {
    fn build(&mut self, task_id: Uuid, depth: usize) -> AppResult<Option<CollaborationSummary>> {
        if self.remaining == 0 || !self.visited.insert(task_id) {
            self.truncated = true;
            return Ok(None);
        }
        let Some(task) = self.tasks.get(&task_id).cloned() else {
            return Ok(None);
        };
        self.remaining -= 1;
        let visible = self.scope.visible_channel_ids.contains(&task.channel_id);
        let owner = collaboration_owner(&task, visible)?;
        let status = TaskStatus::from_str(&task.status).map_err(AppError::Internal)?;

        if !visible {
            return Ok(Some(restricted_summary(&task, owner, status, self.as_of)));
        }

        let projected = self.project_outreach(task_id, depth)?;
        let business_status = projected
            .stored_status
            .map(|stored| outreach_business_status(stored, &projected.statuses))
            .unwrap_or_else(|| task_business_status(status));
        let progress = projected
            .stored_status
            .map(|_| collaboration_progress(&projected));
        Ok(Some(CollaborationSummary {
            task_id,
            outreach_id: projected.outreach_id,
            outreach_version: projected.outreach_version,
            correlation_id: task.correlation_id.into(),
            owner,
            status: business_status,
            progress,
            expires_at: projected.expires_at,
            next_action: summary_next_action(
                &task,
                business_status,
                projected.expires_at,
                &projected.children,
            ),
            children: projected.children,
            as_of: self.as_of,
            truncated: false,
            detail_href: None,
        }))
    }

    fn project_outreach(&mut self, task_id: Uuid, depth: usize) -> AppResult<ProjectedOutreach> {
        let target_rows = self.targets.get(&task_id).cloned().unwrap_or_default();
        let statuses = target_rows
            .iter()
            .map(|row| {
                let stored =
                    OutreachStatus::from_str(&row.outreach_status).map_err(AppError::Internal)?;
                derive_target_status(row, stored, self.as_of)
            })
            .collect::<AppResult<Vec<_>>>()?;
        let stored_status = target_rows
            .first()
            .map(|row| OutreachStatus::from_str(&row.outreach_status))
            .transpose()
            .map_err(AppError::Internal)?;
        let outreach_id = target_rows.first().map(|row| row.outreach_id);
        let outreach_version = target_rows
            .first()
            .map(|row| positive_outreach_version(row.outreach_version))
            .transpose()?;
        let expires_at = target_rows.first().map(|row| row.expires_at);
        let threshold = target_rows
            .first()
            .map(|row| row.required_threshold_percent)
            .unwrap_or_default();
        let eligible_total = target_rows
            .iter()
            .filter(|row| matches!(row.target_status.as_str(), "active" | "responded"))
            .count();
        let mut children = Vec::new();
        for (row, target_status) in target_rows.into_iter().zip(statuses.iter().copied()) {
            if self.remaining == 0 {
                self.truncated = true;
                break;
            }
            self.remaining -= 1;
            children.push(self.project_target(row, depth, target_status)?);
        }
        Ok(ProjectedOutreach {
            children,
            statuses,
            stored_status,
            outreach_id,
            outreach_version,
            expires_at,
            threshold,
            eligible_total,
        })
    }

    fn project_target(
        &mut self,
        row: CollaborationTargetDb,
        depth: usize,
        status: TargetBusinessStatus,
    ) -> AppResult<CollaborationTargetSummary> {
        let identity = target_identity(&row, self.scope)?;
        let child = if depth < MAX_COLLABORATION_DEPTH && identity.visible {
            match row.child_task_id {
                Some(child_id) => self.build(child_id, depth + 1)?.map(Box::new),
                None => None,
            }
        } else {
            if row.child_task_id.is_some() {
                self.truncated = true;
            }
            None
        };
        Ok(CollaborationTargetSummary {
            id: row.id,
            target: identity.target,
            label: identity.label,
            status,
            responded_at: row.responded_at,
            next_action: target_next_action(&row, status),
            child,
        })
    }
}

fn restricted_summary(
    task: &CollaborationTaskDb,
    owner: CollaborationOwner,
    status: TaskStatus,
    as_of: DateTime<Utc>,
) -> CollaborationSummary {
    CollaborationSummary {
        task_id: task.id,
        outreach_id: None,
        outreach_version: None,
        correlation_id: task.correlation_id.into(),
        owner,
        status: task_business_status(status),
        progress: None,
        expires_at: None,
        next_action: None,
        children: Vec::new(),
        as_of,
        truncated: false,
        detail_href: None,
    }
}

fn collaboration_progress(projected: &ProjectedOutreach) -> CollaborationProgress {
    let total = projected.eligible_total;
    let responded = projected
        .statuses
        .iter()
        .filter(|status| **status == TargetBusinessStatus::Responded)
        .count();
    CollaborationProgress {
        responded,
        required: super::required_response_count(total as i64, projected.threshold),
        total,
    }
}

fn derive_target_status(
    row: &CollaborationTargetDb,
    stored: OutreachStatus,
    as_of: DateTime<Utc>,
) -> AppResult<TargetBusinessStatus> {
    let explicit = match row.target_status.as_str() {
        "active" | "responded" => None,
        "cancelled" => Some(TargetBusinessStatus::Cancelled),
        "superseded" => Some(TargetBusinessStatus::Superseded),
        "expired" => Some(TargetBusinessStatus::Expired),
        other => {
            return Err(AppError::Internal(format!(
                "collaboration target {} has invalid status {other}",
                row.id
            )));
        }
    };
    let delivery_status = row
        .delivery_status
        .as_deref()
        .map(DeliveryStatus::from_str)
        .transpose()
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let failure_class = row
        .delivery_failure_class
        .as_deref()
        .map(FailureClass::from_str)
        .transpose()
        .map_err(|error| AppError::Internal(error.to_string()))?;
    Ok(target_business_status(
        explicit,
        row.responded_at,
        stored,
        delivery_status,
        failure_class,
        row.expires_at,
        as_of,
    ))
}

fn positive_outreach_version(value: i64) -> AppResult<u64> {
    u64::try_from(value)
        .ok()
        .filter(|version| *version > 0)
        .ok_or_else(|| AppError::Internal(format!("invalid outreach version {value}")))
}

fn collaboration_owner(task: &CollaborationTaskDb, visible: bool) -> AppResult<CollaborationOwner> {
    if !visible {
        return Ok(CollaborationOwner {
            kind: CollaborationOwnerKind::Restricted,
            principal_id: None,
            label: "Internal specialist".into(),
            available: true,
        });
    }
    let owner = task_owner_from_db(
        task.owner_principal_id,
        task.owner_principal_kind.as_deref(),
        &format!("collaboration task {}", task.id),
    )?;
    let (kind, principal_id, fallback) = match owner {
        TaskOwner::Human(id) => (CollaborationOwnerKind::Human, Some(id), "Teammate"),
        TaskOwner::Agent(id) => (CollaborationOwnerKind::Agent, Some(id), "Agent"),
        TaskOwner::Unassigned => (CollaborationOwnerKind::Unassigned, None, "Unassigned"),
    };
    Ok(CollaborationOwner {
        kind,
        principal_id,
        label: task.owner_label.clone().unwrap_or_else(|| fallback.into()),
        available: task.owner_available,
    })
}

fn target_identity(
    row: &CollaborationTargetDb,
    scope: CollaborationReadScope<'_>,
) -> AppResult<RenderedTargetIdentity> {
    match row.target_kind.as_str() {
        "internal_channel" => {
            let channel_id = row.internal_channel_id.ok_or_else(|| {
                AppError::Internal(format!("collaboration target {} has no channel", row.id))
            })?;
            let visible = scope.visible_channel_ids.contains(&channel_id);
            let target = if visible {
                CollaborationTarget::InternalChannel { channel_id }
            } else {
                CollaborationTarget::RestrictedInternal
            };
            Ok(RenderedTargetIdentity {
                target,
                label: if visible {
                    row.internal_channel_name
                        .clone()
                        .unwrap_or_else(|| "Internal channel".into())
                } else {
                    "Internal specialist".into()
                },
                visible,
            })
        }
        "external" => {
            let transport = parse_required::<TransportKind>(
                row.external_transport.as_deref(),
                row.id,
                "transport",
            )?;
            let namespace = IdentityNamespace::parse(required(
                row.external_namespace.as_deref(),
                row.id,
                "namespace",
            )?)
            .map_err(|error| AppError::Internal(error.to_string()))?;
            let subject = IdentitySubject::parse(required(
                row.external_subject.as_deref(),
                row.id,
                "subject",
            )?)
            .map_err(|error| AppError::Internal(error.to_string()))?;
            let label = subject.as_str().to_string();
            Ok(RenderedTargetIdentity {
                target: CollaborationTarget::External {
                    identity: QualifiedIdentity::new(transport, namespace, subject),
                },
                label,
                visible: true,
            })
        }
        other => Err(AppError::Internal(format!(
            "collaboration target {} has unknown kind {other}",
            row.id
        ))),
    }
}

fn required<'a>(value: Option<&'a str>, id: Uuid, field: &str) -> AppResult<&'a str> {
    value.ok_or_else(|| {
        AppError::Internal(format!("collaboration target {id} has no external {field}"))
    })
}

fn parse_required<T>(value: Option<&str>, id: Uuid, field: &str) -> AppResult<T>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    required(value, id, field)?.parse().map_err(|error| {
        AppError::Internal(format!(
            "collaboration target {id} has invalid external {field}: {error}"
        ))
    })
}

fn target_next_action(
    row: &CollaborationTargetDb,
    status: TargetBusinessStatus,
) -> Option<CollaborationNextAction> {
    let (actor, action, due_at) = match status {
        TargetBusinessStatus::Preparing | TargetBusinessStatus::Sending => (
            NextActionActor::DeliveryQueue,
            NextActionKind::DeliverRequest,
            row.delivery_available_at,
        ),
        TargetBusinessStatus::Waiting => (
            if row.target_kind == "internal_channel" {
                NextActionActor::InternalTarget
            } else {
                NextActionActor::ExternalTarget
            },
            NextActionKind::ProvideResponse,
            Some(row.expires_at),
        ),
        TargetBusinessStatus::NeedsDecision | TargetBusinessStatus::Expired => (
            NextActionActor::CompanyManager,
            if row.delivery_status.as_deref() == Some("outcome_unknown") {
                NextActionKind::ResolveDeliveryOutcome
            } else {
                NextActionKind::ReviewTimeout
            },
            Some(row.expires_at),
        ),
        TargetBusinessStatus::Failed => (
            NextActionActor::CurrentOwner,
            NextActionKind::RepairFailure,
            None,
        ),
        TargetBusinessStatus::Responded
        | TargetBusinessStatus::Cancelled
        | TargetBusinessStatus::Superseded => return None,
    };
    Some(CollaborationNextAction {
        actor,
        action,
        due_at,
        href: None,
    })
}

fn summary_next_action(
    task: &CollaborationTaskDb,
    status: OutreachBusinessStatus,
    expires_at: Option<DateTime<Utc>>,
    children: &[CollaborationTargetSummary],
) -> Option<CollaborationNextAction> {
    match status {
        OutreachBusinessStatus::Waiting => children
            .iter()
            .filter_map(|child| child.next_action.clone())
            .min_by_key(|action| action.due_at),
        OutreachBusinessStatus::ReadyToResume => Some(CollaborationNextAction {
            actor: NextActionActor::TaskQueue,
            action: NextActionKind::RunTask,
            due_at: Some(task.run_at),
            href: None,
        }),
        OutreachBusinessStatus::NeedsDecision => children
            .iter()
            .filter_map(|child| child.next_action.clone())
            .filter(|action| action.actor == NextActionActor::CompanyManager)
            .min_by_key(|action| {
                (
                    action.action != NextActionKind::ResolveDeliveryOutcome,
                    action.due_at,
                )
            })
            .or(Some(CollaborationNextAction {
                actor: NextActionActor::CompanyManager,
                action: NextActionKind::ReviewTimeout,
                due_at: expires_at,
                href: None,
            })),
        OutreachBusinessStatus::Failed => Some(CollaborationNextAction {
            actor: NextActionActor::CurrentOwner,
            action: NextActionKind::RepairFailure,
            due_at: None,
            href: None,
        }),
        OutreachBusinessStatus::Completed | OutreachBusinessStatus::Cancelled => None,
    }
}

async fn load_collaboration_tasks(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    task_id: Uuid,
    visible_channel_ids: &[Uuid],
) -> AppResult<Vec<CollaborationTaskDb>> {
    sqlx::query_as::<_, CollaborationTaskDb>(
        r#"WITH RECURSIVE task_tree(id, depth, path) AS (
               SELECT id, 0, ARRAY[id]
               FROM background_tasks
               WHERE company_id = $1 AND id = $2
               UNION ALL
               SELECT child.id, parent.depth + 1, parent.path || child.id
               FROM task_tree AS parent
               JOIN task_outreaches AS outreach
                 ON outreach.company_id = $1 AND outreach.task_id = parent.id
               JOIN task_outreach_targets AS target
                 ON target.company_id = outreach.company_id
                AND target.outreach_id = outreach.id
               JOIN message_delivery_parts AS request_part
                 ON request_part.company_id = target.company_id
                AND request_part.delivery_id = target.delivery_id
                AND request_part.provider_message_key IS NOT NULL
               JOIN external_messages AS inbound_source
                 ON inbound_source.company_id = target.company_id
                AND inbound_source.external_message_key = request_part.provider_message_key
                AND inbound_source.message_id <> target.request_message_id
               JOIN background_tasks AS child
                 ON child.company_id = target.company_id
                AND child.source_message_uuid = inbound_source.message_id
               WHERE parent.depth < $5
                 AND NOT child.id = ANY(parent.path)
           )
           SELECT task.id, task.channel_id, task.correlation_id, task.status,
                  task.owner_principal_id, task.owner_principal_kind,
                  owner.display_label AS owner_label,
                  CASE WHEN NOT task.channel_id = ANY($4::uuid[]) THEN TRUE
                       WHEN task.owner_principal_id IS NULL THEN TRUE
                       WHEN owner.id IS NULL THEN FALSE
                       WHEN owner.kind = 'agent' THEN agent.id IS NOT NULL
                       WHEN owner.kind = 'person' THEN EXISTS (
                           SELECT 1 FROM participant_identities AS identity
                           WHERE identity.company_id = owner.company_id
                             AND identity.principal_id = owner.id
                             AND identity.status <> 'disabled'
                       )
                       ELSE TRUE END AS owner_available,
                  task.run_at
           FROM background_tasks AS task
           LEFT JOIN principals AS owner
             ON owner.company_id = task.company_id
            AND owner.id = task.owner_principal_id
            AND task.channel_id = ANY($4::uuid[])
           LEFT JOIN agents AS agent ON agent.id = owner.agent_id
           WHERE task.company_id = $1 AND task.id IN (SELECT id FROM task_tree)
           ORDER BY task.created_at, task.id
           LIMIT $3"#,
    )
    .bind(company_id)
    .bind(task_id)
    .bind((MAX_COLLABORATION_NODES + 1) as i64)
    .bind(visible_channel_ids)
    .bind(MAX_COLLABORATION_DEPTH as i32)
    .fetch_all(&mut **tx)
    .await
    .map_err(AppError::from)
}

async fn load_collaboration_targets(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    task_ids: &[Uuid],
    visible_channel_ids: &[Uuid],
) -> AppResult<Vec<CollaborationTargetDb>> {
    sqlx::query_as::<_, CollaborationTargetDb>(
        r#"WITH latest_outreach AS (
               SELECT DISTINCT ON (outreach.task_id)
                      outreach.id, outreach.task_id, outreach.status, outreach.version,
                      outreach.required_threshold_percent,
                      outreach.expires_at, outreach.created_at
               FROM task_outreaches AS outreach
               WHERE outreach.company_id = $1 AND outreach.task_id = ANY($2)
               ORDER BY outreach.task_id, outreach.created_at DESC, outreach.id DESC
           )
           SELECT target.id, outreach.task_id, outreach.id AS outreach_id,
                  outreach.version AS outreach_version, outreach.status AS outreach_status,
                  outreach.required_threshold_percent::double precision,
                  outreach.expires_at, target.status AS target_status,
                  target.target_kind, target.internal_channel_id,
                  channel.name AS internal_channel_name, target.external_transport,
                  target.external_namespace, target.external_subject, target.responded_at,
                  delivery.status AS delivery_status,
                  delivery.last_error_class AS delivery_failure_class,
                  delivery.available_at AS delivery_available_at,
                  child.id AS child_task_id
           FROM latest_outreach AS outreach
           JOIN task_outreach_targets AS target
             ON target.company_id = $1 AND target.outreach_id = outreach.id
           LEFT JOIN channels AS channel
             ON channel.company_id = target.company_id
            AND channel.id = target.internal_channel_id
            AND channel.id = ANY($4::uuid[])
           LEFT JOIN message_deliveries AS delivery
             ON delivery.company_id = target.company_id AND delivery.id = target.delivery_id
           LEFT JOIN LATERAL (
               SELECT child.id
               FROM message_delivery_parts AS request_part
               JOIN external_messages AS inbound_source
                 ON inbound_source.company_id = request_part.company_id
                AND inbound_source.external_message_key = request_part.provider_message_key
                AND inbound_source.message_id <> target.request_message_id
               JOIN background_tasks AS child
                 ON child.company_id = inbound_source.company_id
                AND child.source_message_uuid = inbound_source.message_id
               WHERE request_part.company_id = target.company_id
                 AND request_part.delivery_id = target.delivery_id
                 AND request_part.provider_message_key IS NOT NULL
               ORDER BY child.created_at, child.id
               LIMIT 1
           ) AS child ON TRUE
           ORDER BY outreach.created_at, outreach.id, target.id
           LIMIT $3"#,
    )
    .bind(company_id)
    .bind(task_ids)
    .bind((MAX_COLLABORATION_NODES + 1) as i64)
    .bind(visible_channel_ids)
    .fetch_all(&mut **tx)
    .await
    .map_err(AppError::from)
}

pub(crate) async fn collaboration_summary_on(
    pool: &sqlx::PgPool,
    scope: CollaborationReadScope<'_>,
    task_id: Uuid,
) -> AppResult<Option<CollaborationSummary>> {
    let started = Instant::now();
    let mut tx = pool.begin().await.map_err(AppError::from)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
    let root = sqlx::query_scalar::<_, Uuid>(
        r#"SELECT id FROM background_tasks
           WHERE company_id = $1 AND id = $2 AND channel_id = ANY($3)"#,
    )
    .bind(scope.company_id)
    .bind(task_id)
    .bind(scope.visible_channel_ids)
    .fetch_optional(&mut *tx)
    .await
    .map_err(AppError::from)?;
    let Some(_) = root else {
        return Ok(None);
    };
    let (as_of,): (DateTime<Utc>,) = sqlx::query_as("SELECT CURRENT_TIMESTAMP")
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::from)?;
    let task_rows = load_collaboration_tasks(
        &mut tx,
        scope.company_id,
        task_id,
        scope.visible_channel_ids,
    )
    .await?;
    let task_ids = task_rows.iter().map(|row| row.id).collect::<Vec<_>>();
    let target_rows = load_collaboration_targets(
        &mut tx,
        scope.company_id,
        &task_ids,
        scope.visible_channel_ids,
    )
    .await?;
    tx.commit().await.map_err(AppError::from)?;

    let working_set_nodes = task_rows.len().saturating_add(target_rows.len());
    let source_truncated =
        task_rows.len() > MAX_COLLABORATION_NODES || target_rows.len() > MAX_COLLABORATION_NODES;
    let tasks = task_rows
        .into_iter()
        .take(MAX_COLLABORATION_NODES)
        .map(|row| (row.id, row))
        .collect();
    let mut targets: HashMap<Uuid, Vec<CollaborationTargetDb>> = HashMap::new();
    for row in target_rows.into_iter().take(MAX_COLLABORATION_NODES) {
        targets.entry(row.task_id).or_default().push(row);
    }
    let mut projection = Projection {
        scope,
        as_of,
        tasks,
        targets,
        remaining: MAX_COLLABORATION_NODES,
        truncated: source_truncated,
        visited: HashSet::new(),
    };
    let mut summary = projection.build(task_id, 0)?;
    if let Some(summary) = summary.as_mut() {
        summary.truncated = projection.truncated;
    }
    let elapsed = started.elapsed();
    if elapsed > SLOW_QUERY_THRESHOLD {
        warn!(
            duration_ms = elapsed.as_millis(),
            projected_nodes = MAX_COLLABORATION_NODES - projection.remaining,
            working_set_nodes,
            truncated = projection.truncated,
            "Collaboration summary projection exceeded its query-duration threshold"
        );
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn as_of() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-06T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn task(id: Uuid, channel_id: Uuid) -> CollaborationTaskDb {
        CollaborationTaskDb {
            id,
            channel_id,
            correlation_id: Uuid::new_v4(),
            status: TaskStatus::WaitingForThirdPartyReply.as_str().into(),
            owner_principal_id: None,
            owner_principal_kind: None,
            owner_label: None,
            owner_available: true,
            run_at: as_of(),
        }
    }

    fn target(
        task_id: Uuid,
        channel_id: Uuid,
        child_task_id: Option<Uuid>,
    ) -> CollaborationTargetDb {
        CollaborationTargetDb {
            id: Uuid::new_v4(),
            task_id,
            outreach_id: Uuid::new_v4(),
            outreach_version: 1,
            outreach_status: OutreachStatus::Waiting.as_str().into(),
            required_threshold_percent: 100.0,
            expires_at: as_of() + chrono::Duration::hours(1),
            target_status: "active".into(),
            target_kind: "internal_channel".into(),
            internal_channel_id: Some(channel_id),
            internal_channel_name: Some("Visible channel".into()),
            external_transport: None,
            external_namespace: None,
            external_subject: None,
            responded_at: None,
            delivery_status: None,
            delivery_failure_class: None,
            delivery_available_at: None,
            child_task_id,
        }
    }

    fn projection<'a>(
        company_id: Uuid,
        channel_ids: &'a [Uuid],
        tasks: Vec<CollaborationTaskDb>,
        targets: HashMap<Uuid, Vec<CollaborationTargetDb>>,
    ) -> Projection<'a> {
        Projection {
            scope: CollaborationReadScope {
                company_id,
                visible_channel_ids: channel_ids,
            },
            as_of: as_of(),
            tasks: tasks.into_iter().map(|task| (task.id, task)).collect(),
            targets,
            remaining: MAX_COLLABORATION_NODES,
            truncated: false,
            visited: HashSet::new(),
        }
    }

    fn child_task_depth(summary: &CollaborationSummary) -> usize {
        summary
            .children
            .first()
            .and_then(|target| target.child.as_deref())
            .map_or(0, |child| 1 + child_task_depth(child))
    }

    #[test]
    fn current_owner_keeps_identity_but_reports_an_unavailable_principal() {
        let owner_id = Uuid::new_v4();
        let row = CollaborationTaskDb {
            owner_principal_id: Some(owner_id),
            owner_principal_kind: Some("person".into()),
            owner_label: Some("Former operator".into()),
            owner_available: false,
            ..task(Uuid::new_v4(), Uuid::new_v4())
        };

        let owner = collaboration_owner(&row, true).unwrap();

        assert_eq!(owner.kind, CollaborationOwnerKind::Human);
        assert_eq!(owner.principal_id, Some(owner_id.into()));
        assert_eq!(owner.label, "Former operator");
        assert!(!owner.available);
    }

    #[test]
    fn ambiguous_delivery_outcome_is_the_summarys_next_decision() {
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let root_id = Uuid::new_v4();
        let unknown = CollaborationTargetDb {
            delivery_status: Some(DeliveryStatus::OutcomeUnknown.as_str().into()),
            ..target(root_id, channel_id, None)
        };
        let visible_channel_ids = [channel_id];
        let mut projection = projection(
            company_id,
            &visible_channel_ids,
            vec![task(root_id, channel_id)],
            HashMap::from([(root_id, vec![unknown])]),
        );

        let summary = projection.build(root_id, 0).unwrap().unwrap();

        assert_eq!(summary.status, OutreachBusinessStatus::NeedsDecision);
        assert_eq!(
            summary.next_action.unwrap().action,
            NextActionKind::ResolveDeliveryOutcome
        );
    }

    #[test]
    fn projection_reports_node_truncation_instead_of_silently_omitting_targets() {
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let root_id = Uuid::new_v4();
        let targets = (0..MAX_COLLABORATION_NODES)
            .map(|_| target(root_id, channel_id, None))
            .collect();
        let visible_channel_ids = [channel_id];
        let mut projection = projection(
            company_id,
            &visible_channel_ids,
            vec![task(root_id, channel_id)],
            HashMap::from([(root_id, targets)]),
        );

        let summary = projection.build(root_id, 0).unwrap().unwrap();

        assert_eq!(summary.children.len(), MAX_COLLABORATION_NODES - 1);
        assert_eq!(summary.progress.unwrap().total, MAX_COLLABORATION_NODES);
        assert!(projection.truncated);
        assert_eq!(projection.remaining, 0);
    }

    #[test]
    fn projection_allows_five_nested_task_levels_and_marks_the_sixth() {
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let task_ids = (0..7).map(|_| Uuid::new_v4()).collect::<Vec<_>>();
        let tasks = task_ids
            .iter()
            .map(|id| task(*id, channel_id))
            .collect::<Vec<_>>();
        let targets = task_ids
            .windows(2)
            .map(|pair| (pair[0], vec![target(pair[0], channel_id, Some(pair[1]))]))
            .collect::<HashMap<_, _>>();
        let visible_channel_ids = [channel_id];
        let mut projection = projection(company_id, &visible_channel_ids, tasks, targets);

        let summary = projection.build(task_ids[0], 0).unwrap().unwrap();

        assert_eq!(child_task_depth(&summary), MAX_COLLABORATION_DEPTH);
        assert!(projection.truncated);
    }
}
