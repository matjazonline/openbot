//! The reader's projection of the delivery queue.
//!
//! What a reader needs beyond the row itself is the subject of the message being delivered and the
//! name of the interface carrying it. Both are joined rather than copied onto the delivery: the
//! canonical message owns what the delivery is about, and the binding owns what its interface is
//! called, so a renamed channel or an edited subject reads correctly without a backfill.

use async_trait::async_trait;
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

use super::{DeliveryDb, PART_COLUMNS, PartDb, qualified_delivery_columns};
use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{
        correlation::CorrelationId,
        delivery::{DeliveryEntry, DeliveryFilter, DeliveryPartEntry},
        transport::{DeliveryId, DeliveryPartId},
    },
    use_cases::delivery::{DeliveryReadScope, DeliveryReader, DeliveryWorkspace, project_delivery},
};

/// The row plus its two joined labels, which is what a reader sees.
#[derive(sqlx::FromRow, Debug)]
struct DeliveryViewDb {
    #[sqlx(flatten)]
    delivery: DeliveryDb,
    subject: String,
    destination_label: String,
}

/// `message_deliveries` with the message it carries and the interface it goes out through.
///
/// Both joins are inner: the composite foreign keys make a delivery whose message or binding is
/// missing unrepresentable, so an outer join would only be describing a state the database forbids.
///
/// Reuse the queue's named projection so readers and claimants decode the same target shape.
fn scoped_deliveries(scope: DeliveryReadScope) -> QueryBuilder<'static, Postgres> {
    let mut query = QueryBuilder::new(format!(
        r#"
    SELECT {}, message.subject AS subject,
           binding.display_label AS destination_label
      FROM message_deliveries AS delivery
      JOIN messages AS message
        ON (message.company_id, message.id) = (delivery.company_id, delivery.message_id)
      JOIN channel_bindings AS binding
        ON (binding.company_id, binding.id)
           = (delivery.company_id, delivery.destination_binding_id)
     WHERE delivery.company_id = "#,
        qualified_delivery_columns("delivery")
    ));
    query.push_bind(scope.company_id);
    // Filter before paging; hidden personal rows must not displace readable entries.
    query.push(
        " AND (delivery.principal_id IS NULL OR EXISTS (\
        SELECT 1 FROM principals AS viewer WHERE viewer.company_id=delivery.company_id \
        AND viewer.kind='person' AND viewer.user_id=",
    );
    query.push_bind(scope.viewer.user_id());
    query.push(
        " AND principal_direct_can_view(delivery.company_id,delivery.principal_id,viewer.id)))",
    );
    query
}

#[async_trait]
impl DeliveryReader for PostgresPersistence {
    async fn workspace(
        &self,
        scope: DeliveryReadScope,
        filter: &DeliveryFilter,
        selected: Option<DeliveryId>,
    ) -> AppResult<DeliveryWorkspace> {
        let mut entries = self.list_company_deliveries(scope, filter).await?;
        let selected = match selected {
            Some(id) => self.get_delivery(scope, id).await?,
            None => None,
        };
        let selected_id = selected.as_ref().map(|entry| entry.id);
        entries.extend(selected);
        // A selection may be outside the page. Recheck every loaded inbox together,
        // so its later payload read cannot leave stale private rows in the sidebar.
        let mut entries = self.project_delivery_entries(scope, entries).await?;
        let selected = selected_id.and_then(|id| {
            entries
                .iter()
                .rposition(|entry| entry.id == id)
                .map(|index| entries.remove(index))
        });
        Ok(DeliveryWorkspace { entries, selected })
    }

    async fn list_company_deliveries(
        &self,
        scope: DeliveryReadScope,
        filter: &DeliveryFilter,
    ) -> AppResult<Vec<DeliveryEntry>> {
        // `QueryBuilder` rather than string interpolation: every filter value is a bind parameter,
        // which `src/adapters/persistence/AGENTS.md` requires of dynamic list filters.
        let mut query = scoped_deliveries(scope);
        if let Some(channel_id) = filter.channel_id {
            query
                .push(" AND delivery.channel_id = ")
                .push_bind(channel_id);
        }
        if let Some(status) = filter.status {
            query
                .push(" AND delivery.status = ")
                .push_bind(status.as_str());
        }
        if let Some(transport) = filter.transport {
            query
                .push(" AND delivery.transport = ")
                .push_bind(transport.as_str());
        }
        if let Some(purpose) = filter.purpose {
            query
                .push(" AND delivery.purpose = ")
                .push_bind(purpose.as_str());
        }
        // Matches `message_deliveries_company_created_idx`, or the channel-qualified
        // `message_deliveries_company_channel_created_idx` when one is asked for; ties are broken
        // by id so paging cannot show the same row twice.
        if filter.sort_asc {
            query.push(" ORDER BY delivery.created_at ASC, delivery.id ASC");
        } else {
            query.push(" ORDER BY delivery.created_at DESC, delivery.id DESC");
        }
        query
            .push(" LIMIT ")
            .push_bind(filter.probe_limit())
            .push(" OFFSET ")
            .push_bind(filter.offset());

        let rows = query
            .build_query_as::<DeliveryViewDb>()
            .fetch_all(&self.pool)
            .await
            .map_err(AppError::from)?;
        let entries = self.with_parts(rows).await?;
        self.project_delivery_entries(scope, entries).await
    }

    async fn get_delivery(
        &self,
        scope: DeliveryReadScope,
        delivery_id: DeliveryId,
    ) -> AppResult<Option<DeliveryEntry>> {
        let mut query = scoped_deliveries(scope);
        query
            .push(" AND delivery.id = ")
            .push_bind(delivery_id.as_uuid());
        let row = query
            .build_query_as::<DeliveryViewDb>()
            .fetch_optional(&self.pool)
            .await
            .map_err(AppError::from)?;

        let entries = self.with_parts(row.into_iter().collect()).await?;
        Ok(self
            .project_delivery_entries(scope, entries)
            .await?
            .into_iter()
            .next())
    }
}

impl PostgresPersistence {
    async fn project_delivery_entries(
        &self,
        scope: DeliveryReadScope,
        mut entries: Vec<DeliveryEntry>,
    ) -> AppResult<Vec<DeliveryEntry>> {
        let deliveries = entries
            .iter()
            .map(|entry| entry.id.as_uuid())
            .collect::<Vec<_>>();
        // Parts and provider diagnostics have already loaded; this is the final authority read.
        let levels = crate::adapters::persistence::view_visibility::delivery_levels_on(
            self.pool(),
            crate::domain::workflow::CompanyId::new(scope.company_id),
            scope.viewer,
            &deliveries,
        )
        .await?;
        entries.retain_mut(|entry| match levels.get(&entry.id.as_uuid()).copied() {
            Some(visibility) => {
                project_delivery(entry, visibility);
                true
            }
            None => false,
        });
        Ok(entries)
    }
    /// Attach every delivery's parts, in one batch read rather than one query per delivery.
    ///
    /// `src/adapters/persistence/AGENTS.md`: replace per-parent query loops with bounded batch
    /// reads. The batch is bounded by the page size the caller already clamped.
    async fn with_parts(&self, rows: Vec<DeliveryViewDb>) -> AppResult<Vec<DeliveryEntry>> {
        attach_parts(&self.pool, rows).await
    }
}

async fn attach_parts(
    pool: &sqlx::PgPool,
    rows: Vec<DeliveryViewDb>,
) -> AppResult<Vec<DeliveryEntry>> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<Uuid> = rows.iter().map(|row| row.delivery.id).collect();
    let parts = sqlx::query_as::<_, PartDb>(&format!(
        "SELECT {PART_COLUMNS} FROM message_delivery_parts
          WHERE delivery_id = ANY($1) ORDER BY delivery_id, part_index"
    ))
    .bind(&ids)
    .fetch_all(pool)
    .await
    .map_err(AppError::from)?;

    rows.into_iter()
        .map(|row| {
            let own = parts
                .iter()
                .filter(|part| part.delivery_id == row.delivery.id)
                .map(part_entry)
                .collect::<AppResult<Vec<_>>>()?;
            entry(row, own)
        })
        .collect()
}

fn entry(row: DeliveryViewDb, parts: Vec<DeliveryPartEntry>) -> AppResult<DeliveryEntry> {
    let DeliveryViewDb {
        delivery,
        subject,
        destination_label,
    } = row;
    let attribution = delivery.record()?.attribution.ok_or_else(|| {
        AppError::Internal(format!(
            "Standalone delivery {} appeared in the tenant delivery view",
            delivery.id
        ))
    })?;
    Ok(DeliveryEntry {
        id: DeliveryId::new(delivery.id),
        company_id: delivery.company_id,
        target: attribution.target,
        message_id: attribution.message_id,
        correlation_id: CorrelationId::from(delivery.correlation_id),
        transport: delivery.transport()?,
        purpose: delivery.purpose()?,
        status: delivery.status()?,
        idempotency_key: delivery.idempotency_key.clone(),
        destination_label,
        external_destination: delivery.external_destination.clone(),
        subject,
        attempt_count: delivery.attempt_count,
        max_attempts: delivery.max_attempts,
        last_error_class: delivery.failure_class()?,
        last_error_detail: delivery.last_error_detail.clone(),
        parts,
        available_at: delivery.available_at,
        delivered_at: delivery.delivered_at,
        created_at: delivery.created_at,
        updated_at: delivery.updated_at,
    })
}

fn part_entry(row: &PartDb) -> AppResult<DeliveryPartEntry> {
    Ok(DeliveryPartEntry {
        id: DeliveryPartId::new(row.id),
        index: u16::try_from(row.part_index).map_err(|_| {
            AppError::Internal(format!(
                "Delivery part {} has an out-of-range index {}",
                row.id, row.part_index
            ))
        })?,
        status: row.status()?,
        provider_message_key: row.provider_message_key.clone(),
        attempt_count: row.attempt_count,
        last_error_class: row.failure_class()?,
        last_error_detail: row.last_error_detail.clone(),
        delivered_at: row.delivered_at,
    })
}
