use std::collections::HashMap;

use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::AppResult,
    application::thread_activity::{
        THREAD_RUN_LIMIT, ThreadActivityReader, ThreadRunActivity, ThreadRuns,
    },
    entities::thread_activity::{ThreadActivity, ThreadWorkSummary},
};

/// One run per thread: the newest unfinished one, else the newest of all. A failure is the
/// thread's last word only until a later run answers, so finished runs compete by time alone.
/// Served by `workflow_runs_by_thread`.
pub(crate) const THREAD_ACTIVITY_SQL: &str = r#"
SELECT DISTINCT ON (run.thread_id) run.thread_id, run.id AS run_id, run.state,
       run.waiting_reason
  FROM workflow_runs AS run
 WHERE run.company_id = $1 AND run.thread_id = ANY($2)
 ORDER BY run.thread_id, run.state IN ('succeeded', 'failed', 'cancelled'),
          run.created_at DESC, run.id DESC
"#;

#[derive(sqlx::FromRow)]
struct ThreadActivityRow {
    thread_id: Uuid,
    run_id: Uuid,
    state: String,
    waiting_reason: Option<String>,
}

#[async_trait::async_trait]
impl ThreadActivityReader for PostgresPersistence {
    async fn thread_runs(
        &self,
        company_id: Uuid,
        thread_id: Uuid,
        viewer: Uuid,
    ) -> AppResult<ThreadRuns> {
        let mut rows = sqlx::query_as::<_, OpenThreadRunRow>(OPEN_THREAD_RUNS_SQL)
            .bind(company_id)
            .bind(thread_id)
            .bind((THREAD_RUN_LIMIT + 1) as i64)
            .fetch_all(self.pool())
            .await?;
        let has_more = rows.len() > THREAD_RUN_LIMIT;
        rows.truncate(THREAD_RUN_LIMIT);
        let items: Vec<ThreadRunActivity> = rows
            .into_iter()
            .map(|row| {
                let activity = ThreadActivity::from_run(&row.state, row.waiting_reason.as_deref())
                    .ok_or_else(|| {
                        crate::app_error::AppError::Internal("Invalid open-thread run state".into())
                    })?;
                Ok(ThreadRunActivity {
                    run_id: row.run_id,
                    title: row.title,
                    activity,
                })
            })
            .collect::<AppResult<_>>()?;
        let message_runs = message_runs(self.pool(), company_id, thread_id).await?;
        let ids: Vec<Uuid> = items
            .iter()
            .map(|row| row.run_id)
            .chain(
                message_runs
                    .iter()
                    .flat_map(|row| row.run_ids.iter().copied()),
            )
            .collect();
        let inspectable = inspectable_runs(self, company_id, viewer, &ids).await?;
        Ok(ThreadRuns {
            inspectable,
            items,
            has_more,
            message_runs,
        })
    }

    async fn thread_activity(
        &self,
        company_id: Uuid,
        thread_ids: &[Uuid],
    ) -> AppResult<HashMap<Uuid, ThreadWorkSummary>> {
        if thread_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows = sqlx::query_as::<_, ThreadActivityRow>(THREAD_ACTIVITY_SQL)
            .bind(company_id)
            .bind(thread_ids)
            .fetch_all(self.pool())
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| {
                (
                    row.thread_id,
                    ThreadWorkSummary {
                        run_id: row.run_id,
                        activity: ThreadActivity::from_run(
                            &row.state,
                            row.waiting_reason.as_deref(),
                        ),
                    },
                )
            })
            .collect())
    }
}

// Existing workflow_runs_by_thread narrows the history before this bounded read. Immutable
// version titles describe what ran, even if the draft is renamed later.
pub(crate) const OPEN_THREAD_RUNS_SQL: &str = r#"
SELECT run.id AS run_id, version.title, run.state, run.waiting_reason
FROM workflow_runs run
JOIN workflow_versions version ON (version.company_id, version.id) = (run.company_id, run.version_id)
WHERE run.company_id = $1 AND run.thread_id = $2
  AND run.state IN ('queued', 'running', 'waiting', 'failed')
ORDER BY run.created_at DESC, run.id DESC LIMIT $3
"#;

#[derive(sqlx::FromRow)]
struct OpenThreadRunRow {
    run_id: Uuid,
    title: String,
    state: String,
    waiting_reason: Option<String>,
}

async fn message_runs(
    pool: &sqlx::PgPool,
    company: Uuid,
    thread: Uuid,
) -> AppResult<Vec<crate::application::thread_activity::MessageRuns>> {
    let messages: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT association.message_id, message.direction FROM thread_messages association \
         JOIN messages message ON (message.company_id,message.id)=(association.company_id,association.message_id) \
         WHERE association.company_id=$1 AND association.thread_id=$2 \
         ORDER BY association.created_at DESC, association.id DESC LIMIT $3")
        .bind(company).bind(thread).bind(crate::entities::message_view::THREAD_HISTORY_LIMIT as i64)
        .fetch_all(pool).await?;
    let directions: HashMap<Uuid, _> = messages.into_iter().collect();
    let ids: Vec<Uuid> = directions.keys().copied().collect();
    #[derive(sqlx::FromRow)]
    struct Row {
        canonical_id: Uuid,
        run_ids: Vec<Uuid>,
    }
    let rows = sqlx::query_as::<_, Row>(super::thread::THREAD_RUN_LOOKUP_SQL)
        .bind(company)
        .bind(thread)
        .bind(ids)
        .fetch_all(pool)
        .await?;
    rows.into_iter()
        .map(|row| {
            let direction = directions
                .get(&row.canonical_id)
                .expect("lookup uses these message identities")
                .parse()
                .map_err(crate::app_error::AppError::Internal)?;
            Ok(crate::application::thread_activity::MessageRuns {
                canonical_id: row.canonical_id,
                run_ids: row.run_ids,
                direction,
            })
        })
        .collect()
}

/// Apply the same channel policy used by the mailbox. A destination thread can contain a reply
/// produced on a different, private channel; its visible message does not grant run access.
async fn inspectable_runs(
    p: &PostgresPersistence,
    company: Uuid,
    viewer: Uuid,
    ids: &[Uuid],
) -> AppResult<std::collections::HashSet<Uuid>> {
    use crate::use_cases::{channel::ChannelPersistence, participant::PrincipalAccessPersistence};
    let Some(access) = p.access_context_for_user(company, viewer).await? else {
        return Ok(Default::default());
    };
    if !access.membership.manages_company_operations() {
        return Ok(Default::default());
    }
    let channels: std::collections::HashSet<_> = p
        .list_by_company_id(company)
        .await?
        .into_iter()
        .filter(|channel| channel.viewer_access(access))
        .map(|channel| channel.id)
        .collect();
    let runs: Vec<(Uuid, Option<Uuid>)> = sqlx::query_as(
        "SELECT id,channel_id FROM workflow_runs WHERE company_id=$1 AND id=ANY($2)",
    )
    .bind(company)
    .bind(ids)
    .fetch_all(p.pool())
    .await?;
    Ok(runs
        .into_iter()
        .filter(|(_, channel)| channel.is_none_or(|id| channels.contains(&id)))
        .map(|(id, _)| id)
        .collect())
}
