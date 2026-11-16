use super::*;
use crate::application::local_agents::lifecycle::{
    COMPANY_PAGE, LocalAgentMaintenance, RETENTION_PAGE, RetentionPage,
};

#[async_trait]
impl LocalAgentMaintenance for PostgresPersistence {
    async fn retain_local_agents(&self, after: Option<Uuid>) -> AppResult<RetentionPage> {
        let companies: Vec<Uuid> = sqlx::query_scalar(
            "SELECT company.id FROM companies AS company WHERE ($1::uuid IS NULL OR company.id>$1)
             AND EXISTS(SELECT 1 FROM local_machines WHERE company_id=company.id)
             ORDER BY company.id LIMIT $2",
        )
        .bind(after)
        .bind(COMPANY_PAGE as i64)
        .fetch_all(&self.pool)
        .await?;
        let mut page = RetentionPage {
            next_company: if companies.len() == COMPANY_PAGE {
                companies.last().copied()
            } else {
                None
            },
            ..Default::default()
        };
        for company in companies {
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(1),
                retain_company(self, company),
            )
            .await;
            match result {
                Ok(Ok(done)) => {
                    page.archived_calls += done.archived_calls;
                    page.compacted_observations += done.compacted_observations;
                    page.removed_operations += done.removed_operations;
                    page.removed_sessions += done.removed_sessions;
                }
                _ => {
                    tracing::warn!(company_id=%company,"local-agent retention skipped a contended or failed company")
                }
            }
        }
        Ok(page)
    }
}

async fn retain_company(p: &PostgresPersistence, company: Uuid) -> AppResult<RetentionPage> {
    let mut tx = begin(&p.pool).await?;
    if sqlx::query_scalar::<_, Uuid>("SELECT id FROM companies WHERE id=$1 FOR UPDATE SKIP LOCKED")
        .bind(company)
        .fetch_optional(&mut *tx)
        .await?
        .is_none()
    {
        return Ok(RetentionPage::default());
    }
    let archived_calls = archive_calls(&mut tx, company).await?;
    let compacted_observations = compact_details(&mut tx, company).await?;
    let removed_operations = remove_operations(&mut tx, company).await?;
    let removed_sessions = remove_sessions(&mut tx, company).await?;
    remove_old_metadata(&mut tx, company).await?;
    tx.commit().await?;
    Ok(RetentionPage {
        archived_calls,
        compacted_observations,
        removed_operations,
        removed_sessions,
        ..Default::default()
    })
}

async fn archive_calls(db: &mut PgConnection, company: Uuid) -> AppResult<u64> {
    let calls: Vec<Uuid> = sqlx::query_scalar(
        "SELECT call.invocation_id FROM workflow_local_agent_calls AS call
         JOIN workflow_runs AS run ON run.company_id=call.company_id AND run.id=call.run_id
         LEFT JOIN local_agent_operations AS operation ON operation.company_id=call.company_id AND operation.id=call.operation_id
         WHERE call.company_id=$1 AND run.state IN ('succeeded','failed','cancelled')
           AND run.finished_at<clock_timestamp()-interval '90 days'
           AND (call.operation_id IS NULL OR operation.settled_at<clock_timestamp()-interval '90 days')
         ORDER BY call.run_id,call.invocation_id LIMIT $2",
    ).bind(company).bind(RETENTION_PAGE).fetch_all(&mut *db).await?;
    if calls.is_empty() {
        return Ok(0);
    }
    // The list is bounded; acquire each layer once rather than holding a database
    // transaction across hundreds of per-call network round trips.
    for statement in [
        "SELECT run.id FROM workflow_runs AS run WHERE run.company_id=$1 AND EXISTS(SELECT 1 FROM workflow_local_agent_calls AS call WHERE call.company_id=run.company_id AND call.run_id=run.id AND call.invocation_id=ANY($2)) ORDER BY run.id FOR UPDATE NOWAIT",
        "SELECT machine.id FROM local_machines AS machine WHERE machine.company_id=$1 AND EXISTS(SELECT 1 FROM workflow_local_agent_calls AS call WHERE call.company_id=machine.company_id AND call.machine_id=machine.id AND call.invocation_id=ANY($2)) ORDER BY machine.id FOR UPDATE NOWAIT",
        "SELECT operation.id FROM local_agent_operations AS operation WHERE operation.company_id=$1 AND EXISTS(SELECT 1 FROM workflow_local_agent_calls AS call WHERE call.company_id=operation.company_id AND call.operation_id=operation.id AND call.invocation_id=ANY($2)) ORDER BY operation.id FOR UPDATE NOWAIT",
        "SELECT invocation_id FROM workflow_local_agent_calls WHERE company_id=$1 AND invocation_id=ANY($2) ORDER BY invocation_id FOR UPDATE NOWAIT",
    ] {
        sqlx::query(statement)
            .bind(company)
            .bind(&calls)
            .execute(&mut *db)
            .await?;
    }
    sqlx::query("INSERT INTO workflow_local_agent_retention_proofs(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,settled_at)
        SELECT call.company_id,call.run_id,call.execution_id,call.invocation_id,call.argument_digest,call.dispatch_id,operation.settled_at
        FROM workflow_local_agent_calls AS call JOIN local_agent_operations AS operation
          ON operation.company_id=call.company_id AND operation.id=call.operation_id
        JOIN workflow_runs AS run ON run.company_id=call.company_id AND run.id=call.run_id
        WHERE call.company_id=$1 AND call.invocation_id=ANY($2)
          AND run.state IN ('succeeded','failed','cancelled') AND run.finished_at<clock_timestamp()-interval '90 days'
          AND operation.settled_at<clock_timestamp()-interval '90 days' ON CONFLICT DO NOTHING")
        .bind(company).bind(&calls).execute(&mut *db).await?;
    Ok(sqlx::query("DELETE FROM workflow_local_agent_calls AS call USING workflow_runs AS run
        WHERE call.company_id=$1 AND call.invocation_id=ANY($2) AND run.company_id=call.company_id AND run.id=call.run_id
          AND run.state IN ('succeeded','failed','cancelled') AND run.finished_at<clock_timestamp()-interval '90 days'
          AND (call.operation_id IS NULL OR EXISTS(SELECT 1 FROM workflow_local_agent_retention_proofs AS proof
            WHERE proof.company_id=call.company_id AND proof.invocation_id=call.invocation_id))")
        .bind(company).bind(&calls).execute(db).await?.rows_affected())
}

async fn compact_details(db: &mut PgConnection, company: Uuid) -> AppResult<u64> {
    Ok(sqlx::query("WITH candidates AS MATERIALIZED (
        SELECT observation.company_id,observation.installation_id,observation.sequence
        FROM local_agent_observations AS observation
        JOIN local_agent_operations AS operation ON operation.company_id=observation.company_id AND operation.id=observation.operation_id
        JOIN local_machines AS machine ON machine.company_id=observation.company_id AND machine.id=observation.machine_id
        WHERE observation.company_id=$1 AND observation.observation IS NOT NULL
          AND observation.accepted_at<clock_timestamp()-interval '7 days'
          AND observation.sequence<=machine.acknowledged_sequence AND operation.settled_at IS NOT NULL
        ORDER BY observation.accepted_at,observation.installation_id,observation.sequence
        LIMIT $2 FOR UPDATE OF observation SKIP LOCKED)
        UPDATE local_agent_observations AS observation SET observation=NULL FROM candidates
        WHERE (observation.company_id,observation.installation_id,observation.sequence)
          =(candidates.company_id,candidates.installation_id,candidates.sequence)")
        .bind(company).bind(RETENTION_PAGE).execute(db).await?.rows_affected())
}

async fn remove_operations(db: &mut PgConnection, company: Uuid) -> AppResult<u64> {
    Ok(sqlx::query("WITH candidates AS MATERIALIZED (
        SELECT operation.company_id,operation.id FROM local_agent_operations AS operation
        LEFT JOIN local_agent_sessions AS session ON session.company_id=operation.company_id AND session.id=operation.session_id
        LEFT JOIN workflow_runs AS original ON original.company_id=session.company_id AND original.id=session.workflow_run_id
        WHERE operation.company_id=$1 AND operation.settled_at<clock_timestamp()-interval '90 days'
          AND (session.workflow_run_id IS NULL OR original.state IN ('succeeded','failed','cancelled'))
          AND NOT EXISTS(SELECT 1 FROM native_mcp_calls AS call WHERE call.company_id=operation.company_id AND call.operation_id=operation.id AND call.retired_at IS NULL)
          AND NOT EXISTS(SELECT 1 FROM workflow_local_agent_calls AS call WHERE call.company_id=operation.company_id AND call.operation_id=operation.id)
          AND NOT EXISTS(SELECT 1 FROM workflow_local_agent_calls AS call JOIN workflow_action_intents AS intent
            ON intent.company_id=call.company_id AND intent.id=call.invocation_id WHERE call.company_id=operation.company_id
            AND (call.session_id=operation.session_id OR intent.operation#>>'{arguments,session_id}'=operation.session_id::text))
        ORDER BY operation.settled_at,operation.id LIMIT $2 FOR UPDATE OF operation SKIP LOCKED)
        DELETE FROM local_agent_operations AS operation USING candidates
        WHERE operation.company_id=candidates.company_id AND operation.id=candidates.id")
        .bind(company).bind(RETENTION_PAGE).execute(db).await?.rows_affected())
}

async fn remove_sessions(db: &mut PgConnection, company: Uuid) -> AppResult<u64> {
    Ok(sqlx::query("WITH candidates AS MATERIALIZED (
        SELECT session.company_id,session.id FROM local_agent_sessions AS session
        LEFT JOIN workflow_runs AS original ON original.company_id=session.company_id AND original.id=session.workflow_run_id
        WHERE session.company_id=$1 AND session.created_at<clock_timestamp()-interval '90 days'
          AND (session.workflow_run_id IS NULL OR (original.state IN ('succeeded','failed','cancelled') AND original.finished_at<clock_timestamp()-interval '90 days'))
          AND NOT EXISTS(SELECT 1 FROM local_agent_operations AS operation WHERE operation.company_id=session.company_id AND operation.session_id=session.id)
          AND NOT EXISTS(SELECT 1 FROM native_mcp_calls AS call WHERE call.company_id=session.company_id AND call.session_id=session.id AND call.retired_at IS NULL)
          AND NOT EXISTS(SELECT 1 FROM workflow_local_agent_calls AS call JOIN workflow_action_intents AS intent
            ON intent.company_id=call.company_id AND intent.id=call.invocation_id WHERE call.company_id=session.company_id
            AND (call.session_id=session.id OR intent.operation#>>'{arguments,session_id}'=session.id::text))
        ORDER BY session.created_at,session.id LIMIT $2 FOR UPDATE OF session SKIP LOCKED)
        DELETE FROM local_agent_sessions AS session USING candidates
        WHERE session.company_id=candidates.company_id AND session.id=candidates.id")
        .bind(company).bind(RETENTION_PAGE).execute(db).await?.rows_affected())
}

async fn remove_old_metadata(db: &mut PgConnection, company: Uuid) -> AppResult<()> {
    for statement in [
        "WITH candidates AS (SELECT company_id,machine_id,installation_id,sequence FROM local_machine_capabilities AS snapshot WHERE company_id=$1 AND observed_at<clock_timestamp()-interval '90 days' AND NOT EXISTS(SELECT 1 FROM local_machines AS machine WHERE machine.company_id=snapshot.company_id AND machine.id=snapshot.machine_id AND machine.capabilities_sequence=snapshot.sequence) ORDER BY observed_at,sequence LIMIT $2 FOR UPDATE SKIP LOCKED) DELETE FROM local_machine_capabilities AS snapshot USING candidates WHERE (snapshot.company_id,snapshot.machine_id,snapshot.installation_id,snapshot.sequence)=(candidates.company_id,candidates.machine_id,candidates.installation_id,candidates.sequence)",
        "WITH candidates AS (SELECT id FROM local_pairings WHERE company_id=$1 AND expires_at<clock_timestamp()-interval '90 days' ORDER BY expires_at,id LIMIT $2 FOR UPDATE SKIP LOCKED) DELETE FROM local_pairings AS pairing USING candidates WHERE pairing.company_id=$1 AND pairing.id=candidates.id",
        "WITH candidates AS (SELECT company_id,machine_id,generation FROM local_machine_credentials WHERE company_id=$1 AND revoked_at<clock_timestamp()-interval '90 days' ORDER BY revoked_at,machine_id,generation LIMIT $2 FOR UPDATE SKIP LOCKED) DELETE FROM local_machine_credentials AS credential USING candidates WHERE (credential.company_id,credential.machine_id,credential.generation)=(candidates.company_id,candidates.machine_id,candidates.generation)",
    ] {
        sqlx::query(statement)
            .bind(company)
            .bind(RETENTION_PAGE)
            .execute(&mut *db)
            .await?;
    }
    Ok(())
}
