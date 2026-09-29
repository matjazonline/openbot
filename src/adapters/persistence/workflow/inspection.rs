use super::*;
use crate::domain::entities::{correlation::CorrelationId, message::CanonicalMessageId};
use serde_json::Value;

#[derive(sqlx::FromRow)]
pub(super) struct HeadRow {
    pub company_id: Uuid,
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub version_id: Uuid,
    pub channel_id: Option<Uuid>,
    pub thread_id: Option<Uuid>,
    pub trigger_id: Uuid,
    pub correlation_id: Uuid,
    pub state: String,
    pub waiting_reason: Option<String>,
    pub revision: i64,
    pub source_key: String,
}

pub(super) const HEAD: &str = "SELECT run.company_id,run.id,run.workflow_id,run.version_id,run.channel_id,run.thread_id,run.trigger_id,run.correlation_id,run.state,run.waiting_reason,run.revision,admission.source_key FROM workflow_runs AS run JOIN workflow_admissions AS admission ON admission.company_id=run.company_id AND admission.run_id=run.id WHERE run.company_id=$1 AND run.id=$2";

#[async_trait]
impl WorkflowInspection for PostgresPersistence {
    async fn head(&self, company_id: CompanyId, run_id: RunId) -> AppResult<Option<RunHead>> {
        sqlx::query_as::<_, HeadRow>(HEAD)
            .bind(company_id.as_uuid())
            .bind(run_id.as_uuid())
            .fetch_optional(&self.pool)
            .await?
            .map(HeadRow::restore)
            .transpose()
    }
}

impl HeadRow {
    pub(super) fn restore(self) -> AppResult<RunHead> {
        let company = CompanyId::new(self.company_id);
        let trigger = TriggerRef::new(
            company,
            TriggerId::new(self.trigger_id),
            source(company, self.trigger_id, &self.source_key)?,
        )
        .map_err(|_| invalid())?;
        let state = match self.state.as_str() {
            "queued" => RunState::Queued,
            "running" => RunState::Running,
            "succeeded" => RunState::Succeeded,
            "failed" => RunState::Failed,
            "cancelled" => RunState::Cancelled,
            "waiting" => RunState::Waiting(match self.waiting_reason.as_deref() {
                Some("decision") => WaitingReason::Decision,
                Some("event") => WaitingReason::Event,
                Some("timer") => WaitingReason::Timer,
                Some("child_run") => WaitingReason::ChildRun,
                Some("effect") => WaitingReason::Effect,
                Some("reconciliation") => WaitingReason::Reconciliation,
                _ => return Err(invalid()),
            }),
            _ => return Err(invalid()),
        };
        let association = match (self.channel_id, self.thread_id) {
            (None, None) => RelatedAssociation::Company,
            (Some(channel), None) => RelatedAssociation::Channel(RelatedChannelId::new(channel)),
            (Some(channel), Some(thread)) => RelatedAssociation::Thread {
                channel_id: RelatedChannelId::new(channel),
                thread_id: RelatedThreadId::new(thread),
            },
            _ => return Err(invalid()),
        };
        Ok(RunHead {
            causality: RunCausality::new(
                RunId::new(self.id),
                trigger,
                CorrelationId::from(self.correlation_id),
            )
            .map_err(|_| invalid())?,
            workflow_id: WorkflowId::new(self.workflow_id),
            version_id: VersionId::new(self.version_id),
            association,
            state,
            revision: RunRevision(u64::try_from(self.revision).map_err(|_| invalid())?),
        })
    }
}

fn source(company: CompanyId, trigger: Uuid, source: &str) -> AppResult<TriggerSource> {
    let value: Value = serde_json::from_str(source.strip_prefix("v1:").ok_or_else(invalid)?)
        .map_err(|_| invalid())?;
    let fields = value.as_array().ok_or_else(invalid)?;
    let uuid = |index: usize| -> AppResult<Uuid> {
        Uuid::parse_str(
            fields
                .get(index)
                .and_then(Value::as_str)
                .ok_or_else(invalid)?,
        )
        .map_err(|_| invalid())
    };
    Ok(match fields.first().and_then(Value::as_str) {
        Some("manual") if fields.len() == 2 && uuid(1)? == trigger => TriggerSource::Manual,
        Some("message") if fields.len() == 2 => TriggerSource::Message {
            message_id: CanonicalMessageId::new(uuid(1)?),
        },
        Some("schedule") if fields.len() == 3 => TriggerSource::Schedule {
            schedule_id: ScheduleId::new(uuid(1)?),
            occurrence_id: ScheduleOccurrenceId::new(uuid(2)?),
        },
        Some("child") if fields.len() == 5 => {
            let execution = ExecutionRef::new(
                company,
                RunId::new(uuid(1)?),
                ExecutionId::new(uuid(2)?),
                StepId::parse(fields[3].as_str().ok_or_else(invalid)?).map_err(|_| invalid())?,
            );
            let parent = if fields[4].is_null() {
                ChildCause::Execution(execution)
            } else {
                ChildCause::Action(ActionRef::new(execution, ActionInvocationId::new(uuid(4)?)))
            };
            TriggerSource::Child { parent }
        }
        _ => return Err(invalid()),
    })
}
