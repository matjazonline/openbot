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
        // Like the SQL owner: children are created only by their parent's runtime step.
        TriggerSource::Child { .. } => {
            return Err(AppError::BadRequest(
                "Child workflow runs are created only by their parent's workflow step".into(),
            ));
        }
    };
    match stored_association {
        Some(association) if association == command.association() => Ok(()),
        _ => Err(AppError::NotFound(
            "workflow trigger source or association".into(),
        )),
    }
}
