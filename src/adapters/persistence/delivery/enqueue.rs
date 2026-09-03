//! Creating a delivery, on the caller's transaction.
//!
//! Deliberately not a port method of its own. Every delivery is owed by some durable state that
//! has to become visible with it: an agent's reply message, an approval row, a schedule's answer,
//! an outreach's target list. Writing the queue row in a second transaction is how a thread ends
//! up showing an answer that was never queued, or a delivery goes out for work whose record says
//! it never ran -- so this takes a `&mut Transaction` and the caller decides what it lands with.

use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::PART_COLUMNS;
use crate::{
    app_error::{AppError, AppResult},
    entities::transport::{DeliveryId, DeliveryPartId},
    transport::{DeliveryCreation, NewDelivery, RenderedPart},
};

/// Write one delivery and its frozen parts, or recognise the one that is already queued.
///
/// The unique index on `(destination_binding_id, idempotency_key)` is the lock: two workers racing
/// the same logical delivery compute the same key, so the first insert wins and the second is
/// absorbed. The loser's rendered parts are discarded rather than merged -- the delivery that
/// exists already has its own, frozen by whoever got there first, and re-freezing them under a
/// second id is how one message becomes two sends.
pub async fn insert_delivery_on(
    tx: &mut Transaction<'_, Postgres>,
    delivery: &NewDelivery,
) -> AppResult<DeliveryCreation> {
    let inserted: Option<(Uuid,)> = sqlx::query_as(
        r#"INSERT INTO message_deliveries (
                id, company_id, channel_id, message_id, source_binding_id,
                destination_binding_id, external_destination, task_id, depends_on_delivery_id,
                correlation_id, transport, purpose, idempotency_key, max_attempts
           ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
           ON CONFLICT ON CONSTRAINT message_deliveries_destination_key_key DO NOTHING
           RETURNING id"#,
    )
    .bind(delivery.id.as_uuid())
    .bind(delivery.company_id)
    .bind(delivery.channel_id)
    .bind(delivery.message_id.as_uuid())
    .bind(delivery.source_binding_id.as_uuid())
    .bind(delivery.destination_binding_id.as_uuid())
    .bind(
        delivery
            .external_destination
            .as_ref()
            .map(|destination| destination.as_str()),
    )
    .bind(delivery.task_id)
    .bind(delivery.depends_on_delivery_id.map(DeliveryId::as_uuid))
    .bind(delivery.correlation_id.as_uuid())
    .bind(delivery.transport.as_str())
    .bind(delivery.purpose.as_str())
    .bind(delivery.idempotency_key.as_str())
    .bind(delivery.max_attempts)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?;

    let Some((id,)) = inserted else {
        return absorbed(tx, delivery).await;
    };

    for part in delivery.parts.iter() {
        insert_part_on(tx, delivery, part).await?;
    }
    Ok(DeliveryCreation::Created(DeliveryId::new(id)))
}

/// The id of the delivery that already holds this key.
///
/// Read back rather than assumed to be `delivery.id`: the row that exists was written by whoever
/// won the race, under *their* id, and a caller told its own id would record a join to a delivery
/// that is not there.
async fn absorbed(
    tx: &mut Transaction<'_, Postgres>,
    delivery: &NewDelivery,
) -> AppResult<DeliveryCreation> {
    let existing: Option<Uuid> = sqlx::query_scalar(
        r#"SELECT id FROM message_deliveries
           WHERE destination_binding_id = $1 AND idempotency_key = $2"#,
    )
    .bind(delivery.destination_binding_id.as_uuid())
    .bind(delivery.idempotency_key.as_str())
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?;

    existing
        .map(|id| DeliveryCreation::Absorbed(DeliveryId::new(id)))
        .ok_or_else(|| {
            // The insert conflicted, so a row with this key exists in some transaction; not
            // finding it means the conflict came from somewhere this read cannot see, which is not
            // a state to paper over with a fabricated id.
            AppError::Internal(format!(
                "Delivery '{}' conflicted on its idempotency key but no such delivery is visible",
                delivery.idempotency_key
            ))
        })
}

/// One frozen part.
///
/// The part key, not the delivery id, is what makes this stable across a re-render: it is derived
/// from the delivery's own idempotency key, so whoever queued the row could predict the provider
/// key it would go out under before the row existed.
async fn insert_part_on(
    tx: &mut Transaction<'_, Postgres>,
    delivery: &NewDelivery,
    part: &RenderedPart,
) -> AppResult<()> {
    if part.payload.transport() != delivery.transport {
        // The renderer and the row disagree about which provider will be called. Refused here
        // rather than at claim time, because a payload that cannot be decoded by the adapter the
        // transport column names is a delivery that can only ever dead-letter.
        return Err(AppError::Internal(format!(
            "A {} delivery cannot carry a {} part payload",
            delivery.transport,
            part.payload.transport()
        )));
    }
    let payload = serde_json::to_value(&part.payload).map_err(|error| {
        AppError::Internal(format!("Could not store a rendered delivery part: {error}"))
    })?;

    sqlx::query(
        r#"INSERT INTO message_delivery_parts (
                id, company_id, delivery_id, part_index, part_key, payload, content_digest
           ) VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
    )
    .bind(DeliveryPartId::random().as_uuid())
    .bind(delivery.company_id)
    .bind(delivery.id.as_uuid())
    .bind(i32::from(part.index.get()))
    .bind(part.key.as_str())
    .bind(payload)
    .bind(part.digest.as_str())
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(())
}

/// Every frozen part of one delivery, in index order.
pub(crate) async fn load_parts_on(
    tx: &mut Transaction<'_, Postgres>,
    delivery_id: DeliveryId,
) -> AppResult<Vec<super::PartDb>> {
    sqlx::query_as::<_, super::PartDb>(&format!(
        "SELECT {PART_COLUMNS} FROM message_delivery_parts
          WHERE delivery_id = $1 ORDER BY part_index"
    ))
    .bind(delivery_id.as_uuid())
    .fetch_all(&mut **tx)
    .await
    .map_err(AppError::from)
}
