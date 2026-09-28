//! Legacy source writers must reject workflow jobs before creating derived records.
use crate::app_error::{AppError, AppResult};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

pub(super) async fn require_legacy_task_on(
    tx: &mut Transaction<'_, Postgres>,
    company_id: Uuid,
    task_id: Option<Uuid>,
) -> AppResult<()> {
    let Some(task_id) = task_id else {
        return Ok(());
    };
    // Intentionally select only legacy jobs. The lock preserves the association until
    // the source is written; immutable queue identity preserves the discriminator.
    let found: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM background_tasks WHERE company_id = $1 AND id = $2 \
         AND queue_kind = 'legacy' FOR KEY SHARE",
    )
    .bind(company_id)
    .bind(task_id)
    .fetch_optional(&mut **tx)
    .await?;
    found
        .map(|_| ())
        .ok_or_else(|| AppError::NotFound("Legacy task".into()))
}
