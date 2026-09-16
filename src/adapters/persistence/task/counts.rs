use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    application::task_counts::TaskCountsReader,
    entities::{
        task::TaskStatus,
        task_counts::{TaskCountRow, TaskCountSnapshot},
    },
};
use uuid::Uuid;

pub(super) const COUNTS_QUERY: &str = r#"
SELECT task.channel_id, principal.agent_id, task.status, COUNT(*)::bigint AS count
  FROM background_tasks AS task
  LEFT JOIN principals AS principal
    ON principal.company_id = task.company_id AND principal.id = task.owner_principal_id
   AND task.owner_principal_kind = 'agent'
 WHERE task.company_id = $1 AND task.status = ANY($2) AND task.channel_id = ANY($3)
 GROUP BY task.channel_id, principal.agent_id, task.status
"#;

#[derive(sqlx::FromRow)]
struct TaskCountDb {
    channel_id: Uuid,
    agent_id: Option<Uuid>,
    status: String,
    count: i64,
}

pub(crate) async fn task_counts_on(
    pool: &sqlx::PgPool,
    company_id: Uuid,
    visible_channel_ids: &[Uuid],
) -> AppResult<TaskCountSnapshot> {
    let statuses = TaskStatus::OPEN.map(|status| status.as_str());
    let rows = sqlx::query_as::<_, TaskCountDb>(COUNTS_QUERY)
        .bind(company_id)
        .bind(statuses.as_slice())
        .bind(visible_channel_ids)
        .fetch_all(pool)
        .await?;
    let rows = rows
        .into_iter()
        .map(|row| {
            Ok(TaskCountRow {
                channel_id: row.channel_id,
                agent_id: row.agent_id,
                status: row.status.parse().map_err(AppError::Internal)?,
                count: row
                    .count
                    .try_into()
                    .map_err(|_| AppError::Internal("Negative task count".into()))?,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok(TaskCountSnapshot::from_rows(rows))
}

#[async_trait::async_trait]
impl TaskCountsReader for PostgresPersistence {
    async fn task_counts(
        &self,
        company_id: Uuid,
        visible_channel_ids: &[Uuid],
    ) -> AppResult<TaskCountSnapshot> {
        task_counts_on(self.pool(), company_id, visible_channel_ids).await
    }
}

#[cfg(test)]
mod tests {
    use super::super::counts_test_support::CountsFixture;
    use super::*;
    use crate::entities::task_counts::TaskCounts;

    #[tokio::test]
    async fn all_statuses_visibility_and_current_ownership_agree_with_sql() {
        let Some(f) = CountsFixture::new().await else {
            return;
        };
        let statuses = [
            TaskStatus::Pending,
            TaskStatus::Processing,
            TaskStatus::PendingApproval,
            TaskStatus::WaitingForThirdPartyReply,
            TaskStatus::Completed,
            TaskStatus::Failed,
            TaskStatus::DeadLetter,
            TaskStatus::Stopped,
        ];
        for channel in f.channels {
            for status in statuses {
                for principal in f.principals {
                    f.task(channel, Some((principal, "agent")), status).await;
                }
            }
        }
        f.task(f.channels[0], None, TaskStatus::Pending).await;
        let human: Uuid =
            sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 AND user_id = $2")
                .bind(f.company_id)
                .bind(f.owner.user_id)
                .fetch_one(f.persistence.pool())
                .await
                .unwrap();
        f.task(f.channels[0], Some((human, "person")), TaskStatus::Pending)
            .await;
        let counts = f
            .persistence
            .task_counts(f.company_id, &f.channels)
            .await
            .unwrap();
        assert_eq!(
            counts.company,
            TaskCounts {
                pending: 6,
                active: 4,
                waiting: 8
            }
        );
        assert_eq!(
            counts.agents[&f.agents[0]],
            TaskCounts {
                pending: 2,
                active: 2,
                waiting: 4
            }
        );
        let visible = f
            .persistence
            .task_counts(f.company_id, &f.channels[..1])
            .await
            .unwrap();
        assert_eq!(
            visible.company,
            TaskCounts {
                pending: 4,
                active: 2,
                waiting: 4
            }
        );
        assert_eq!(visible.channels.len(), 1);
        assert_eq!(
            visible.agents[&f.agents[0]],
            TaskCounts {
                pending: 1,
                active: 1,
                waiting: 2
            }
        );
        assert!(
            f.persistence
                .task_counts(f.company_id, &[])
                .await
                .unwrap()
                .company
                .is_empty()
        );
        assert!(
            f.persistence
                .task_counts(Uuid::new_v4(), &f.channels)
                .await
                .unwrap()
                .company
                .is_empty()
        );
        sqlx::query("UPDATE background_tasks SET owner_principal_id = $2 WHERE company_id = $1 AND owner_principal_id = $3")
            .bind(f.company_id).bind(f.principals[1]).bind(f.principals[0]).execute(f.persistence.pool()).await.unwrap();
        let transferred = f
            .persistence
            .task_counts(f.company_id, &f.channels)
            .await
            .unwrap();
        assert!(!transferred.agents.contains_key(&f.agents[0]));
        assert_eq!(
            transferred.agents[&f.agents[1]],
            TaskCounts {
                pending: 4,
                active: 4,
                waiting: 8
            }
        );
    }
}
