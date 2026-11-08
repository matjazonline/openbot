//! Same-transaction primitives for manual and workflow admission. Callers authorize first.
use super::*;

#[derive(sqlx::FromRow)]
struct ProfileRow {
    id: Uuid,
    name: String,
    selection: Value,
}
impl ProfileRow {
    fn entity(self, company: CompanyId) -> AppResult<LocalAgentProfile> {
        decode(
            json!({"id":self.id,"company_id":company.as_uuid(),"name":self.name,"selection":self.selection}),
            "profile",
        )
    }
}
pub(in crate::adapters::persistence) async fn profile_on(
    db: &mut PgConnection,
    company: CompanyId,
    id: ProfileId,
) -> AppResult<Option<LocalAgentProfile>> {
    sqlx::query_as::<_, ProfileRow>(
        "SELECT id,name,selection FROM local_agent_profiles WHERE company_id=$1 AND id=$2",
    )
    .bind(company.as_uuid())
    .bind(id.as_uuid())
    .fetch_optional(db)
    .await?
    .map(|row| row.entity(company))
    .transpose()
}

pub(in crate::adapters::persistence) struct LockedProfile {
    pub profile: LocalAgentProfile,
    pub machine: LocalMachine,
}
pub(in crate::adapters::persistence) async fn lock_profile_on(
    db: &mut PgConnection,
    company: CompanyId,
    id: ProfileId,
) -> AppResult<LockedProfile> {
    let observed = profile_on(db, company, id).await?.ok_or_else(missing)?;
    let machine = lock_machine(db, company.as_uuid(), observed.selection.machine_id)
        .await?
        .entity()?;
    let profile = sqlx::query_as::<_,ProfileRow>("SELECT id,name,selection FROM local_agent_profiles WHERE company_id=$1 AND id=$2 FOR SHARE")
        .bind(company.as_uuid()).bind(id.as_uuid()).fetch_optional(&mut *db).await?.ok_or_else(missing)?.entity(company)?;
    if profile.selection != observed.selection {
        return Err(AppError::Conflict(
            "Local profile changed during admission".into(),
        ));
    }
    Ok(LockedProfile { profile, machine })
}

pub(in crate::adapters::persistence) async fn validate_selection_on(
    db: &mut PgConnection,
    company: CompanyId,
    machine: MachineId,
    selection: &ExecutionSelection,
) -> AppResult<()> {
    if selection.machine_id != machine {
        return Err(missing());
    }
    let allowed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM local_machine_projects WHERE company_id=$1 AND machine_id=$2 AND project_id=$3)")
        .bind(company.as_uuid()).bind(machine.as_uuid()).bind(selection.project_id.as_str()).fetch_one(&mut *db).await?;
    if !allowed {
        return Err(AppError::Conflict("project is not enabled".into()));
    }
    current_catalog(db, company.as_uuid(), machine)
        .await?
        .validate_selection(selection)
        .map_err(|error| AppError::Conflict(error.into()))
}
pub(in crate::adapters::persistence) async fn validate_capacity_on(
    db: &mut PgConnection,
    company: CompanyId,
    machine: MachineId,
    command: &LocalCommand,
) -> AppResult<()> {
    let counts = protocol::capacity(db, company.as_uuid(), machine).await?;
    validate_capacity(counts.unsettled as usize, counts.queued as usize, command)
        .map_err(|error| AppError::Conflict(error.into()))
}

#[derive(sqlx::FromRow)]
struct SessionRow {
    id: Uuid,
    profile_id: Uuid,
    installation_id: Uuid,
    selection: Value,
    conversation_id: Option<Uuid>,
    workflow_run_id: Option<Uuid>,
    thread_id: Option<String>,
}
pub(in crate::adapters::persistence) async fn session_on(
    db: &mut PgConnection,
    company: CompanyId,
    id: SessionId,
) -> AppResult<Option<LocalAgentSession>> {
    let row = sqlx::query_as::<_,SessionRow>("SELECT id,profile_id,installation_id,selection,conversation_id,workflow_run_id,thread_id FROM local_agent_sessions WHERE company_id=$1 AND id=$2")
        .bind(company.as_uuid()).bind(id.as_uuid()).fetch_optional(db).await?;
    row.map(|row| {
        let association = match (row.conversation_id,row.workflow_run_id) {
            (Some(id),None) => json!({"kind":"conversation","conversation_id":id}),
            (None,Some(id)) => json!({"kind":"workflow","run_id":id}),
            _ => return Err(invalid("session association")),
        };
        decode(json!({"id":row.id,"profile_id":row.profile_id,"company_id":company.as_uuid(),"installation_id":row.installation_id,
            "selection":row.selection,"association":association,"thread_id":row.thread_id}), "session")
    }).transpose()
}
pub(in crate::adapters::persistence) async fn insert_session_on(
    db: &mut PgConnection,
    session: &LocalAgentSession,
) -> AppResult<()> {
    let mut conversation = None;
    let mut workflow_run = None;
    match session.association {
        SessionAssociation::Conversation { conversation_id } => {
            conversation = Some(conversation_id)
        }
        SessionAssociation::Workflow { run_id } => workflow_run = Some(run_id),
    }
    sqlx::query("INSERT INTO local_agent_sessions(company_id,id,machine_id,installation_id,profile_id,selection,conversation_id,workflow_run_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(session.company_id).bind(session.id.as_uuid()).bind(session.selection.machine_id.as_uuid()).bind(session.installation_id.as_uuid())
        .bind(session.profile_id.as_uuid()).bind(encode(&session.selection)?).bind(conversation).bind(workflow_run).execute(db).await
        .map_err(|error| if error.as_database_error().is_some_and(|error| error.is_unique_violation()) {
            AppError::Conflict("Local session identity already exists".into())
        } else { error.into() })?;
    Ok(())
}
