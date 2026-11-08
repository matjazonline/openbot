use super::*;
use crate::application::local_agents::management::*;
use crate::application::local_agents::protocol::{LogicalInvocationKey, OperationAdmission};
use crate::application::workflow::{RelatedAssociation, RelatedChannelId, RelatedThreadId};

#[cfg(test)]
mod tests;
mod writes;

async fn authorize_association_on(
    db: &mut PgConnection,
    company: Uuid,
    actor: Uuid,
    association: &SessionAssociation,
) -> AppResult<()> {
    match association {
        SessionAssociation::Conversation { conversation_id } => {
            let channel: Uuid =
                sqlx::query_scalar("SELECT channel_id FROM threads WHERE company_id=$1 AND id=$2")
                    .bind(company)
                    .bind(conversation_id)
                    .fetch_optional(&mut *db)
                    .await?
                    .ok_or_else(missing)?;
            super::super::workflow::association::authorize_association(
                db,
                CompanyId::new(company),
                WorkflowActor::authenticated(actor)?,
                RelatedAssociation::Thread {
                    channel_id: RelatedChannelId::new(channel),
                    thread_id: RelatedThreadId::new(*conversation_id),
                },
            )
            .await?;
        }
        SessionAssociation::Workflow { run_id } => {
            super::super::workflow::local_agents::authorize_run_on(
                db,
                CompanyId::new(company),
                *run_id,
                WorkflowActor::authenticated(actor)?,
            )
            .await?;
        }
    }
    Ok(())
}

async fn authorized_session_on(
    db: &mut PgConnection,
    company: Uuid,
    actor: Uuid,
    id: SessionId,
) -> AppResult<LocalAgentSession> {
    authorize(db, company, actor).await?;
    let session = shared::session_on(db, CompanyId::new(company), id)
        .await?
        .ok_or_else(missing)?;
    authorize_association_on(db, company, actor, &session.association).await?;
    Ok(session)
}

const VIEW_SQL: &str = "jsonb_build_object('operation',jsonb_build_object('id',operation.id,'command',operation.command,'deadline',operation.deadline,'payload_digest',operation.payload_digest,'dispatch',operation.dispatch,'outcome',operation.outcome),'origin',operation.origin,'output',operation.output,'output_truncated',operation.output_truncated,'observed_turn_state',operation.observed_turn_state,'measured_usage',operation.measured_usage)";

async fn operation_on(
    db: &mut PgConnection,
    company: Uuid,
    id: OperationId,
) -> AppResult<Option<LocalOperationView>> {
    let value: Option<Value> = sqlx::query_scalar(&format!(
        "SELECT {VIEW_SQL} FROM local_agent_operations AS operation WHERE company_id=$1 AND id=$2"
    ))
    .bind(company)
    .bind(id.as_uuid())
    .fetch_optional(db)
    .await?;
    value
        .map(|value| decode(value, "operation view"))
        .transpose()
}

#[async_trait]
impl LocalAgentManagementStore for PostgresPersistence {
    async fn read_operation(
        &self,
        actor: Uuid,
        company: Uuid,
        id: OperationId,
    ) -> AppResult<LocalOperationView> {
        let mut tx = begin(&self.pool).await?;
        authorize(&mut tx, company, actor).await?;
        let operation = operation_on(&mut tx, company, id)
            .await?
            .ok_or_else(missing)?;
        match operation.operation.command() {
            LocalCommand::Create { session_id, .. }
            | LocalCommand::Send { session_id, .. }
            | LocalCommand::Inspect { session_id, .. }
            | LocalCommand::StopThread { session_id, .. }
            | LocalCommand::ArchiveThread { session_id, .. } => {
                authorized_session_on(&mut tx, company, actor, *session_id).await?;
            }
            LocalCommand::RefreshCatalog {} => {}
        }
        tx.commit().await?;
        Ok(operation)
    }
    async fn refresh_catalog(
        &self,
        actor: Uuid,
        company: Uuid,
        machine: MachineId,
        request: OperationId,
    ) -> AppResult<LocalAgentOperation> {
        writes::refresh(self, actor, company, machine, request).await
    }
    async fn list_profiles(
        &self,
        actor: Uuid,
        company: Uuid,
        after: Option<ProfileId>,
    ) -> AppResult<RecordPage<LocalAgentProfile>> {
        let mut tx = begin(&self.pool).await?;
        authorize(&mut tx, company, actor).await?;
        let values: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',id,'company_id',company_id,'name',name,'selection',selection) FROM local_agent_profiles WHERE company_id=$1 AND ($2::uuid IS NULL OR id>$2) ORDER BY id LIMIT $3")
            .bind(company).bind(after.map(ProfileId::as_uuid)).bind((MAX_PAGE_RECORDS+1) as i64).fetch_all(&mut *tx).await?;
        let mut entries = values
            .into_iter()
            .map(|value| decode(value, "profile"))
            .collect::<AppResult<Vec<_>>>()?;
        let truncated = entries.len() > MAX_PAGE_RECORDS;
        entries.truncate(MAX_PAGE_RECORDS);
        tx.commit().await?;
        Ok(RecordPage { entries, truncated })
    }
    async fn delete_profile(&self, actor: Uuid, company: Uuid, id: ProfileId) -> AppResult<()> {
        let mut tx = begin(&self.pool).await?;
        authorize(&mut tx, company, actor).await?;
        shared::lock_profile_on(&mut tx, CompanyId::new(company), id).await?;
        sqlx::query("DELETE FROM local_agent_profiles WHERE company_id=$1 AND id=$2")
            .bind(company)
            .bind(id.as_uuid())
            .execute(&mut *tx)
            .await
            .map_err(|error| {
                if error
                    .as_database_error()
                    .is_some_and(|error| error.is_foreign_key_violation())
                {
                    AppError::Conflict("Profile is referenced by a session or workflow".into())
                } else {
                    error.into()
                }
            })?;
        tx.commit().await?;
        Ok(())
    }
    async fn start_session(&self, request: &StartSession) -> AppResult<LocalOperationView> {
        writes::start(self, request).await
    }
    async fn control_session(&self, request: &ControlSession) -> AppResult<LocalOperationView> {
        writes::control(self, request).await
    }
    async fn read_session(
        &self,
        actor: Uuid,
        company: Uuid,
        id: SessionId,
    ) -> AppResult<SessionDetails> {
        let mut tx = begin(&self.pool).await?;
        let session = authorized_session_on(&mut tx, company, actor, id).await?;
        let values: Vec<Value> = sqlx::query_scalar(&format!("SELECT {VIEW_SQL} FROM local_agent_operations AS operation WHERE company_id=$1 AND session_id=$2 ORDER BY created_at DESC,id DESC LIMIT $3"))
            .bind(company).bind(id.as_uuid()).bind((SESSION_OPERATION_PAGE+1) as i64).fetch_all(&mut *tx).await?;
        let mut entries = values
            .into_iter()
            .map(|value| decode(value, "operation view"))
            .collect::<AppResult<Vec<_>>>()?;
        let truncated = entries.len() > SESSION_OPERATION_PAGE;
        entries.truncate(SESSION_OPERATION_PAGE);
        bound_outputs(&mut entries)?;
        tx.commit().await?;
        Ok(SessionDetails {
            session,
            recent: RecordPage { entries, truncated },
        })
    }
    async fn list_sessions(
        &self,
        actor: Uuid,
        company: Uuid,
        association: &SessionAssociation,
    ) -> AppResult<RecordPage<SessionSummary>> {
        let mut tx = begin(&self.pool).await?;
        authorize(&mut tx, company, actor).await?;
        authorize_association_on(&mut tx, company, actor, association).await?;
        let (conversation, workflow) = match association {
            SessionAssociation::Conversation { conversation_id } => (Some(*conversation_id), None),
            SessionAssociation::Workflow { run_id } => (None, Some(*run_id)),
        };
        let values: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',id,'profile_id',profile_id,'thread_id',thread_id,'created_at',created_at) FROM local_agent_sessions WHERE company_id=$1 AND (conversation_id=$2 OR workflow_run_id=$3) ORDER BY created_at DESC,id DESC LIMIT $4")
            .bind(company).bind(conversation).bind(workflow).bind((MAX_PAGE_RECORDS+1) as i64).fetch_all(&mut *tx).await?;
        let mut entries = values
            .into_iter()
            .map(|value| decode(value, "session summary"))
            .collect::<AppResult<Vec<_>>>()?;
        let truncated = entries.len() > MAX_PAGE_RECORDS;
        entries.truncate(MAX_PAGE_RECORDS);
        tx.commit().await?;
        Ok(RecordPage { entries, truncated })
    }
    async fn machine_runtime(
        &self,
        actor: Uuid,
        company: Uuid,
        id: MachineId,
    ) -> AppResult<MachineRuntime> {
        let mut tx = begin(&self.pool).await?;
        authorize(&mut tx, company, actor).await?;
        let value: Value = sqlx::query_scalar("SELECT jsonb_build_object(
            'last_issue',machine.last_connection_issue,'connected',COALESCE(machine.connection_expires_at>clock_timestamp(),false),
            'reconciled',machine.connection_generation>0 AND machine.reconciled_generation=machine.connection_generation,
            'unsettled_effects',0,'outstanding_commands',0,'oldest_outstanding_at',queue.oldest,
            'permission_waits',queue.permissions,'uncertain_outcomes',queue.uncertain,'reconciliation_gaps',queue.gaps,
            'backpressured',false,'last_native_success_at',machine.last_native_success_at,
            'projects',COALESCE((SELECT jsonb_agg(project_id ORDER BY project_id) FROM
                (SELECT project_id FROM local_machine_projects WHERE company_id=$1 AND machine_id=$2 ORDER BY project_id LIMIT $3) AS bounded_projects),'[]'::jsonb))
            FROM local_machines AS machine LEFT JOIN LATERAL (
                SELECT min(operation.created_at) FILTER(WHERE public.local_operation_is_outstanding(operation.command,operation.outcome,operation.observed_turn_state)) AS oldest,
                    count(*) FILTER(WHERE operation.observed_turn_state='waiting_for_input' AND public.local_operation_is_outstanding(operation.command,operation.outcome,operation.observed_turn_state)) AS permissions,
                    count(*) FILTER(WHERE operation.outcome='uncertain') AS uncertain,
                    count(*) FILTER(WHERE operation.delivery_attempted_at IS NOT NULL AND operation.last_observed_generation<machine.connection_generation AND public.local_operation_is_outstanding(operation.command,operation.outcome,operation.observed_turn_state)) AS gaps
                FROM local_agent_operations AS operation WHERE operation.company_id=machine.company_id AND operation.machine_id=machine.id
            ) AS queue ON true WHERE machine.company_id=$1 AND machine.id=$2")
            .bind(company).bind(id.as_uuid()).bind((MAX_PROJECTS+1) as i64).fetch_optional(&mut *tx).await?.ok_or_else(missing)?;
        let mut runtime: MachineRuntime = decode(value, "machine runtime")?;
        let counts = protocol::capacity(&mut tx, company, id).await?;
        runtime.unsettled_effects =
            u32::try_from(counts.unsettled).map_err(|_| invalid("workload count"))?;
        runtime.outstanding_commands =
            u32::try_from(counts.queued).map_err(|_| invalid("workload count"))?;
        runtime.backpressured = counts.queued >= MAX_QUEUED_COMMANDS as i64
            || counts.unsettled >= MAX_UNSETTLED_EFFECTS as i64;
        tx.commit().await?;
        Ok(runtime)
    }
}

fn bound_outputs(entries: &mut [LocalOperationView]) -> AppResult<()> {
    let mut remaining = MAX_RESULT_BYTES;
    for entry in entries {
        if let Some(output) = &entry.output {
            let mut length = output.as_str().len().min(remaining);
            while !output.as_str().is_char_boundary(length) {
                length -= 1;
            }
            remaining -= length;
            if length < output.as_str().len() {
                entry.output_truncated = true;
                entry.output = Some(
                    LocalOutput::try_from(output.as_str()[..length].to_owned())
                        .map_err(|_| invalid("bounded output"))?,
                );
            }
        }
    }
    Ok(())
}
