use super::*;

#[derive(Clone, Copy)]
pub(super) enum Operation {
    Cancel,
    Retry,
}
impl Operation {
    fn name(self) -> &'static str {
        match self {
            Self::Cancel => "cancel",
            Self::Retry => "retry",
        }
    }
}

#[derive(sqlx::FromRow)]
struct Receipt {
    run_id: Uuid,
    actor_id: Uuid,
    operation: String,
    expected_revision: i64,
    result: String,
    result_revision: i64,
}

#[async_trait]
impl WorkflowRunTransitions for PostgresPersistence {
    async fn cancel(&self, command: CancelCommand) -> AppResult<CancelResult> {
        let Some((result, revision)) = control(self, command, Operation::Cancel).await? else {
            return Ok(CancelResult::NotFound);
        };
        Ok(match result.as_str() {
            "applied" => CancelResult::Applied { revision },
            "terminal" => CancelResult::AlreadyTerminalOrApplied { revision },
            "conflict" => CancelResult::RevisionConflict {
                current_revision: revision,
            },
            _ => return Err(invalid()),
        })
    }
    async fn retry(&self, command: RetryCommand) -> AppResult<RetryResult> {
        let command = CancelCommand {
            company_id: command.company_id,
            actor: command.actor,
            run_id: command.run_id,
            command_key: command.command_key,
            expected_revision: command.expected_revision,
        };
        let Some((result, revision)) = control(self, command, Operation::Retry).await? else {
            return Ok(RetryResult::NotFound);
        };
        Ok(match result.as_str() {
            "applied" => RetryResult::Applied { revision },
            "unsafe" => RetryResult::Unsafe { revision },
            "conflict" => RetryResult::RevisionConflict {
                current_revision: revision,
            },
            _ => return Err(invalid()),
        })
    }
}

async fn control(
    p: &PostgresPersistence,
    command: CancelCommand,
    operation: Operation,
) -> AppResult<Option<(String, RunRevision)>> {
    let mut tx = p.pool.begin().await?;
    sqlx::query("SELECT set_config('lock_timeout','1000ms',true),set_config('statement_timeout','1000ms',true)").execute(&mut *tx).await?;
    // Admission takes company authority before its parent run. Inverting this order deadlocks.
    authority::authorize_company(&mut tx, command.company_id, command.actor).await?;
    let head = sqlx::query_as::<_, inspection::HeadRow>(&format!(
        "{} FOR UPDATE OF run",
        inspection::HEAD
    ))
    .bind(command.company_id.as_uuid())
    .bind(command.run_id.as_uuid())
    .fetch_optional(&mut *tx)
    .await?;
    let Some(head) = head else {
        return Ok(None);
    };
    let head = head.restore()?;
    association::authorize_association(
        &mut tx,
        command.company_id,
        command.actor,
        head.association,
    )
    .await?;
    if let Some(saved) = replay(&mut tx, &command, operation).await? {
        tx.commit().await?;
        return Ok(Some(saved));
    }
    let result = if head.revision != command.expected_revision {
        "conflict"
    } else {
        match operation {
            Operation::Cancel if head.state.is_terminal() => "terminal",
            Operation::Cancel => {
                cancel_on(&mut tx, &command).await?;
                "applied"
            }
            Operation::Retry => {
                if control_retry::retry_on(&mut tx, &command, &head).await? {
                    "applied"
                } else {
                    "unsafe"
                }
            }
        }
    };
    let sequence = if result == "applied" {
        Some(audit(&mut tx, &command, operation).await?)
    } else {
        None
    };
    let rev: i64 =
        sqlx::query_scalar("SELECT revision FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(command.company_id.as_uuid())
            .bind(command.run_id.as_uuid())
            .fetch_one(&mut *tx)
            .await?;
    sqlx::query("INSERT INTO workflow_control_commands(company_id,command_key,run_id,actor_id,operation,expected_revision,result,result_revision,audit_sequence) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)")
        .bind(command.company_id.as_uuid()).bind(command.command_key.as_str()).bind(command.run_id.as_uuid()).bind(command.actor.user_id()).bind(operation.name()).bind(revision(command.expected_revision.0)?).bind(result).bind(rev).bind(sequence).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Some((result.into(), RunRevision(rev as u64))))
}

async fn replay(
    db: &mut PgConnection,
    command: &CancelCommand,
    operation: Operation,
) -> AppResult<Option<(String, RunRevision)>> {
    let saved = sqlx::query_as::<_, Receipt>("SELECT run_id,actor_id,operation,expected_revision,result,result_revision FROM workflow_control_commands WHERE company_id=$1 AND command_key=$2")
        .bind(command.company_id.as_uuid()).bind(command.command_key.as_str()).fetch_optional(db).await?;
    let Some(saved) = saved else {
        return Ok(None);
    };
    if saved.run_id != command.run_id.as_uuid()
        || saved.actor_id != command.actor.user_id()
        || saved.operation != operation.name()
        || saved.expected_revision != revision(command.expected_revision.0)?
    {
        return Err(conflict());
    }
    Ok(Some((
        saved.result,
        RunRevision(saved.result_revision as u64),
    )))
}

async fn audit(
    db: &mut PgConnection,
    command: &CancelCommand,
    operation: Operation,
) -> AppResult<i64> {
    Ok(sqlx::query_scalar("INSERT INTO workflow_run_events(company_id,run_id,sequence,event_kind,actor_id) SELECT $1,$2,COALESCE(MAX(sequence),0)+1,$3,$4 FROM workflow_run_events WHERE company_id=$1 AND run_id=$2 RETURNING sequence")
        .bind(command.company_id.as_uuid()).bind(command.run_id.as_uuid()).bind(match operation { Operation::Cancel => "control_cancel", Operation::Retry => "control_retry" }).bind(command.actor.user_id()).fetch_one(db).await?)
}

async fn cancel_on(db: &mut PgConnection, command: &CancelCommand) -> AppResult<()> {
    sqlx::query("SELECT id FROM workflow_executions WHERE company_id=$1 AND run_id=$2 ORDER BY id FOR UPDATE")
        .bind(command.company_id.as_uuid()).bind(command.run_id.as_uuid()).execute(&mut *db).await?;
    sqlx::query("SELECT job.id FROM background_tasks AS job JOIN workflow_executions AS execution ON execution.company_id=job.company_id AND execution.id=job.workflow_execution_id WHERE execution.company_id=$1 AND execution.run_id=$2 AND job.queue_kind='workflow' ORDER BY job.id FOR UPDATE OF job")
        .bind(command.company_id.as_uuid()).bind(command.run_id.as_uuid()).execute(&mut *db).await?;
    maintenance::retire_jobs(
        db,
        command.company_id,
        command.run_id,
        maintenance::RetirementCause::Cancel,
    )
    .await?;
    sqlx::query("UPDATE workflow_waits SET state='cancelled',settled_at=clock_timestamp() WHERE company_id=$1 AND run_id=$2 AND state='waiting'")
        .bind(command.company_id.as_uuid()).bind(command.run_id.as_uuid()).execute(&mut *db).await?;
    sqlx::query("UPDATE workflow_runs SET state='cancelled',waiting_reason=NULL WHERE company_id=$1 AND id=$2")
        .bind(command.company_id.as_uuid()).bind(command.run_id.as_uuid()).execute(&mut *db).await?;
    let code =
        crate::domain::workflow::FailureCode::parse("workflow.cancelled").map_err(|_| invalid())?;
    action_uncertainty::record(db, command.company_id, command.run_id, None, &code).await?;
    Ok(())
}
