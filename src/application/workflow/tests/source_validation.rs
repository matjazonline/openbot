use super::*;

pub(super) fn validate_source(state: &State, command: &PreparedAdmission) -> AppResult<()> {
    if state.fail_source {
        return Err(AppError::Database("source lookup failed".into()));
    }
    let company = command.company_id();
    let stored_association = match command.trigger().source() {
        TriggerSource::Manual => return Ok(()),
        TriggerSource::Message { message_id } => {
            state.source_messages.get(&(company, *message_id)).copied()
        }
        TriggerSource::Schedule {
            schedule_id,
            occurrence_id,
        } => state
            .schedule_occurrences
            .get(&(company, *schedule_id, *occurrence_id))
            .copied(),
        TriggerSource::Child { parent } => {
            let execution = parent.execution();
            if state
                .parent_executions
                .get(&(company, execution.execution_id()))
                != Some(execution)
            {
                return Err(AppError::NotFound("parent execution".into()));
            }
            let Some(_run) = state.runs.get(&(company, execution.run_id())) else {
                return Err(AppError::NotFound("parent run".into()));
            };
            if let ChildCause::Action(action) = parent
                && state.parent_actions.get(&(company, action.action_id())) != Some(action)
            {
                return Err(AppError::NotFound("parent action".into()));
            }
            // The child target may have a different related resource. Its own
            // association was checked by lifecycle authorization above.
            return Ok(());
        }
    };
    match stored_association {
        Some(association) if association == command.association() => Ok(()),
        _ => Err(AppError::NotFound(
            "workflow trigger source or association".into(),
        )),
    }
}
