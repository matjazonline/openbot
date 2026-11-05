use crate::app_error::{AppError, AppResult};
use sqlx::{Postgres, Transaction};
use std::{
    future::Future,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
const OPERATION_BOUND: Duration = Duration::from_secs(5);
fn uncertain() -> AppError {
    AppError::ServiceUnavailable(
        "The library change may have completed; refresh the library before trying again".into(),
    )
}
pub(super) async fn bounded<T>(
    sent: &AtomicBool,
    operation: impl Future<Output = AppResult<T>>,
) -> AppResult<T> {
    match tokio::time::timeout(OPERATION_BOUND, operation).await {
        Ok(result) => result,
        Err(_) if sent.load(Ordering::Acquire) => Err(uncertain()),
        Err(_) => Err(AppError::Conflict(
            "The library is busy; retry shortly".into(),
        )),
    }
}
pub(super) async fn finish<T>(
    tx: Transaction<'_, Postgres>,
    result: AppResult<T>,
    sent: &AtomicBool,
) -> AppResult<T> {
    let value = match result {
        Ok(value) => value,
        Err(error) => {
            tx.rollback().await.map_err(super::authority::database)?;
            return Err(error);
        }
    };
    sent.store(true, Ordering::Release);
    match tx.commit().await {
        Ok(()) => Ok(value),
        Err(error) if error.as_database_error().is_some() => Err(super::authority::database(error)),
        Err(_) => Err(uncertain()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(start_paused = true)]
    async fn knowledge_upload_client_bound_distinguishes_an_unsent_commit_from_an_unknown_acknowledgment()
     {
        let sent = AtomicBool::new(false);
        let first: AppResult<()> = bounded(&sent, std::future::pending()).await;
        assert!(matches!(first, Err(AppError::Conflict(_))));
        sent.store(true, Ordering::Release);
        let second: AppResult<()> = bounded(&sent, std::future::pending()).await;
        let error = second.unwrap_err();
        assert!(matches!(error, AppError::ServiceUnavailable(_)));
        assert!(error.to_string().contains("may have completed; refresh"));
    }
}
