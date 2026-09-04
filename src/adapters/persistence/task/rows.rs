//! The `background_tasks` and ledger rows as Postgres returns them, and the conversions that
//! parse their `TEXT` statuses into domain enums.
//!
//! Kept together so every read of a table selects the same shape, and one place decides what a
//! stored status string means.

use chrono::{DateTime, Utc};
use serde_json::Value;
use std::str::FromStr;
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::runtime_metrics::{MachineId, MachineIdentity, MachineRegion},
    entities::task::{
        BackgroundTask, ChainStage, TaskAttemptRecord, TaskAttemptRecordStatus, TaskChainCard,
        TaskChainCounts, TaskStatus, TaskStatusEvent, TaskStopReason, TaskTransitionActorKind,
        TaskTransitionReason,
    },
};

#[derive(sqlx::FromRow, Debug)]
pub struct BackgroundTaskDb {
    pub id: Uuid,
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Option<Uuid>,
    pub correlation_id: Uuid,
    pub task_type: String,
    pub status: String,
    pub payload: Value,
    pub retry_count: i32,
    pub max_retries: i32,
    pub last_error: Option<String>,
    pub worker_id: Option<Uuid>,
    pub execution_generation: Option<Uuid>,
    pub locked_at: Option<DateTime<Utc>>,
    pub lock_expires_at: Option<DateTime<Utc>>,
    pub run_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow, Debug)]
pub(crate) struct TaskAttemptRecordDb {
    pub(crate) attempt_number: i32,
    pub(crate) status: String,
    pub(crate) error: Option<String>,
    pub(crate) stop_reason: Option<String>,
    pub(crate) prompt_tokens: Option<i32>,
    pub(crate) completion_tokens: Option<i32>,
    pub(crate) result: Option<Value>,
    pub(crate) started_at: DateTime<Utc>,
    pub(crate) finished_at: Option<DateTime<Utc>>,
    pub(crate) execution_generation: Uuid,
    pub(crate) worker_id: Uuid,
    pub(crate) machine_id: String,
    pub(crate) machine_region: Option<String>,
}

/// One attempt as read by a chain-wide batch, carrying the task it belongs to.
///
/// The flattened row is the same shape the per-task read uses, so the two cannot drift into
/// selecting different columns.
#[derive(sqlx::FromRow, Debug)]
pub(crate) struct ChainAttemptDb {
    pub(crate) task_id: Uuid,
    #[sqlx(flatten)]
    pub(crate) record: TaskAttemptRecordDb,
}

#[derive(sqlx::FromRow, Debug)]
pub(crate) struct TaskStatusEventDb {
    pub(crate) id: Uuid,
    pub(crate) company_id: Uuid,
    pub(crate) task_id: Uuid,
    pub(crate) correlation_id: Uuid,
    pub(crate) sequence: i32,
    pub(crate) from_status: Option<String>,
    pub(crate) to_status: String,
    pub(crate) reason: String,
    pub(crate) actor_kind: String,
    pub(crate) actor_id: Option<Uuid>,
    pub(crate) related_approval_id: Option<Uuid>,
    pub(crate) related_outreach_id: Option<Uuid>,
    pub(crate) retry_count: i32,
    pub(crate) run_at: DateTime<Utc>,
    pub(crate) execution_generation: Option<Uuid>,
    pub(crate) transitioned_at: DateTime<Utc>,
}

impl TryFrom<TaskStatusEventDb> for TaskStatusEvent {
    type Error = AppError;

    fn try_from(db: TaskStatusEventDb) -> AppResult<Self> {
        let row = format!("task_status_events row {}", db.id);
        let parse_status = |value: &str| {
            TaskStatus::from_str(value)
                .map_err(|error| AppError::Internal(format!("{row}: {error}")))
        };
        Ok(Self {
            id: db.id,
            company_id: db.company_id,
            task_id: db.task_id,
            correlation_id: db.correlation_id.into(),
            sequence: db.sequence,
            from_status: db.from_status.as_deref().map(parse_status).transpose()?,
            to_status: parse_status(&db.to_status)?,
            reason: TaskTransitionReason::from_str(&db.reason)
                .map_err(|error| AppError::Internal(format!("{row}: {error}")))?,
            actor_kind: TaskTransitionActorKind::from_str(&db.actor_kind)
                .map_err(|error| AppError::Internal(format!("{row}: {error}")))?,
            actor_id: db.actor_id,
            related_approval_id: db.related_approval_id,
            related_outreach_id: db.related_outreach_id,
            retry_count: db.retry_count,
            run_at: db.run_at,
            execution_generation: db.execution_generation,
            transitioned_at: db.transitioned_at,
        })
    }
}

#[derive(sqlx::FromRow, Debug)]
pub(crate) struct TaskChainCardDb {
    pub(crate) correlation_id: Uuid,
    pub(crate) stage: String,
    pub(crate) title: String,
    pub(crate) channel_names: Vec<String>,
    pub(crate) agent_names: Vec<String>,
    pub(crate) total_tasks: i64,
    pub(crate) pending: i64,
    pub(crate) processing: i64,
    pub(crate) expired_processing: i64,
    pub(crate) pending_approval: i64,
    pub(crate) waiting_reply: i64,
    pub(crate) completed: i64,
    pub(crate) failed: i64,
    pub(crate) dead_letter: i64,
    pub(crate) stopped: i64,
    pub(crate) total_deliveries: i64,
    pub(crate) delivery_queued: i64,
    pub(crate) delivery_sending: i64,
    pub(crate) delivery_delivered: i64,
    pub(crate) delivery_unresolved: i64,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) last_activity_at: DateTime<Utc>,
    pub(crate) next_action_at: Option<DateTime<Utc>>,
    pub(crate) retry_count: i64,
    pub(crate) failure_summary: Option<String>,
    pub(crate) stage_total: i64,
}

impl TryFrom<TaskChainCardDb> for (TaskChainCard, i64) {
    type Error = AppError;

    fn try_from(db: TaskChainCardDb) -> AppResult<Self> {
        let stage = ChainStage::from_str(&db.stage).map_err(|error| {
            AppError::Internal(format!(
                "task chain {} has invalid stage: {error}",
                db.correlation_id
            ))
        })?;
        let counts = TaskChainCounts {
            total_tasks: db.total_tasks,
            pending: db.pending,
            processing: db.processing,
            expired_processing: db.expired_processing,
            pending_approval: db.pending_approval,
            waiting_reply: db.waiting_reply,
            completed: db.completed,
            failed: db.failed,
            dead_letter: db.dead_letter,
            stopped: db.stopped,
            total_deliveries: db.total_deliveries,
            delivery_queued: db.delivery_queued,
            delivery_sending: db.delivery_sending,
            delivery_delivered: db.delivery_delivered,
            delivery_unresolved: db.delivery_unresolved,
        };
        Ok((
            TaskChainCard {
                correlation_id: db.correlation_id.into(),
                stage,
                title: db.title,
                channel_names: db.channel_names,
                agent_names: db.agent_names,
                counts,
                created_at: db.created_at,
                last_activity_at: db.last_activity_at,
                next_action_at: db.next_action_at,
                retry_count: db.retry_count,
                failure_summary: db.failure_summary,
            },
            db.stage_total,
        ))
    }
}
impl TryFrom<TaskAttemptRecordDb> for TaskAttemptRecord {
    type Error = AppError;

    fn try_from(db: TaskAttemptRecordDb) -> AppResult<Self> {
        Ok(Self {
            attempt_number: db.attempt_number,
            status: TaskAttemptRecordStatus::from_str(&db.status).map_err(AppError::Internal)?,
            error: db.error,
            stop_reason: db
                .stop_reason
                .map(|reason| TaskStopReason::from_str(&reason).map_err(AppError::Internal))
                .transpose()?,
            prompt_tokens: db.prompt_tokens,
            completion_tokens: db.completion_tokens,
            result: db.result,
            started_at: db.started_at,
            finished_at: db.finished_at,
            execution_generation: db.execution_generation,
            worker_id: db.worker_id,
            machine: MachineIdentity {
                id: MachineId::new(db.machine_id),
                region: db.machine_region.map(MachineRegion::new),
            },
        })
    }
}
impl TryFrom<BackgroundTaskDb> for BackgroundTask {
    type Error = AppError;

    fn try_from(db: BackgroundTaskDb) -> AppResult<Self> {
        let status =
            TaskStatus::from_str(&db.status).map_err(|e| AppError::Internal(e.to_string()))?;

        Ok(BackgroundTask {
            id: db.id,
            company_id: db.company_id,
            channel_id: db.channel_id,
            thread_id: db.thread_id,
            correlation_id: db.correlation_id.into(),
            task_type: db.task_type,
            status,
            payload: db.payload,
            retry_count: db.retry_count,
            max_retries: db.max_retries,
            last_error: db.last_error,
            worker_id: db.worker_id,
            execution_generation: db.execution_generation,
            locked_at: db.locked_at,
            lock_expires_at: db.lock_expires_at,
            run_at: db.run_at,
            created_at: db.created_at,
            updated_at: db.updated_at,
        })
    }
}
/// One row of the per-thread activity lookup.
#[derive(sqlx::FromRow)]
pub(crate) struct ThreadActivityDb {
    pub(crate) thread_id: Uuid,
    pub(crate) status: String,
    pub(crate) lock_expires_at: Option<DateTime<Utc>>,
}
