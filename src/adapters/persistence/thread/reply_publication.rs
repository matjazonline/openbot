//! One lock order and association protocol for direct and reviewed task replies.

use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::{InsertedMessage, associate_message_on, insert_message_on};
use crate::{
    app_error::{AppError, AppResult},
    entities::message::CanonicalMessageId,
    transport::MAX_THREAD_ASSOCIATIONS,
    use_cases::thread::MessageWrite,
};

pub(crate) struct TaskReplyPublication<'a> {
    pub company_id: Uuid,
    pub task_id: Option<Uuid>,
    pub message: &'a MessageWrite,
    pub also_in_threads: &'a [Uuid],
}

pub(crate) async fn publish_task_reply_on(
    tx: &mut Transaction<'_, Postgres>,
    publication: TaskReplyPublication<'_>,
) -> AppResult<InsertedMessage> {
    let own_threads: Vec<Uuid> = std::iter::once(publication.message.thread_id)
        .chain(publication.also_in_threads.iter().copied())
        .collect();
    if own_threads.len() > MAX_THREAD_ASSOCIATIONS {
        return Err(AppError::BadRequest(
            "Too many reply thread associations".into(),
        ));
    }
    let source_threads = source_threads_on(tx, &publication).await?;
    let mut affected_threads = own_threads.clone();
    affected_threads.extend(&source_threads);
    // Each sibling already owns its own task fence. Lock all threads in the same order before
    // writing even the primary association, so concurrent publications cannot invert the order.
    lock_reply_threads_on(tx, publication.company_id, &affected_threads).await?;
    let stored = insert_message_on(tx, publication.message).await?;
    for &thread_id in publication.also_in_threads {
        associate_message_on(
            tx,
            thread_id,
            stored.canonical_id,
            publication.message.entry_kind,
        )
        .await?;
    }
    let sibling_threads: Vec<Uuid> = source_threads
        .into_iter()
        .filter(|thread_id| !own_threads.contains(thread_id))
        .collect();
    file_in_sibling_threads_on(
        tx,
        publication.company_id,
        stored.canonical_id,
        &sibling_threads,
    )
    .await?;
    Ok(stored)
}

async fn source_threads_on(
    tx: &mut Transaction<'_, Postgres>,
    publication: &TaskReplyPublication<'_>,
) -> AppResult<Vec<Uuid>> {
    let threads = sqlx::query_scalar(
        r#"SELECT filed.thread_id
           FROM background_tasks AS task
           JOIN thread_messages AS filed
             ON filed.company_id = task.company_id
            AND filed.message_id = task.source_message_uuid
           WHERE task.company_id = $1 AND task.id = $2
           ORDER BY filed.thread_id
           LIMIT $3"#,
    )
    .bind(publication.company_id)
    .bind(publication.task_id)
    .bind((MAX_THREAD_ASSOCIATIONS + 1) as i64)
    .fetch_all(&mut **tx)
    .await
    .map_err(AppError::from)?;
    if threads.len() > MAX_THREAD_ASSOCIATIONS {
        return Err(AppError::Internal(
            "Source message exceeds the thread association bound".into(),
        ));
    }
    Ok(threads)
}

async fn lock_reply_threads_on(
    tx: &mut Transaction<'_, Postgres>,
    company_id: Uuid,
    thread_ids: &[Uuid],
) -> AppResult<()> {
    let locked: Vec<Uuid> = sqlx::query_scalar(
        r#"SELECT id FROM threads
           WHERE company_id = $1 AND id = ANY($2)
           ORDER BY id FOR NO KEY UPDATE"#,
    )
    .bind(company_id)
    .bind(thread_ids)
    .fetch_all(&mut **tx)
    .await
    .map_err(AppError::from)?;
    if thread_ids.iter().any(|id| !locked.contains(id)) {
        return Err(AppError::BadRequest(
            "Reply thread is outside the task's company".into(),
        ));
    }
    Ok(())
}

pub(crate) async fn file_in_sibling_threads_on(
    tx: &mut Transaction<'_, Postgres>,
    company_id: Uuid,
    reply_id: CanonicalMessageId,
    sibling_threads: &[Uuid],
) -> AppResult<()> {
    sqlx::query(
        r#"WITH inserted AS (
               INSERT INTO thread_messages (
                   id, company_id, channel_id, thread_id, message_id, created_at, entry_kind
               )
               SELECT gen_random_uuid(), thread.company_id, thread.channel_id, thread.id, $2,
                      CURRENT_TIMESTAMP, 'delegation'
               FROM threads AS thread
               WHERE thread.company_id = $1 AND thread.id = ANY($3)
               ON CONFLICT (channel_id, message_id) DO NOTHING
               RETURNING thread_id
           )
           UPDATE threads AS thread SET updated_at = GREATEST(thread.updated_at, clock_timestamp())
           FROM inserted
           WHERE thread.company_id = $1 AND thread.id = inserted.thread_id"#,
    )
    .bind(company_id)
    .bind(reply_id.as_uuid())
    .bind(sibling_threads)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(())
}
