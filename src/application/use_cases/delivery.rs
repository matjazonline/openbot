//! Reading the delivery queue.
//!
//! Read-only, and deliberately so: the delivery worker owns these rows, and a page that offered
//! buttons would be racing the transport. The port lives here because the `/ui` routes and the
//! task board are what consume it; the SQLx projection that answers it is an adapter.

use async_trait::async_trait;
use uuid::Uuid;

use crate::{
    app_error::AppResult,
    entities::{
        delivery::{DeliveryEntry, DeliveryFilter},
        transport::DeliveryId,
    },
};

#[async_trait]
pub trait DeliveryReader: Send + Sync {
    /// One filtered page of the company's deliveries, newest first unless the filter says
    /// otherwise.
    ///
    /// The filter carries the caller's probe size, so whether a further page exists comes back
    /// with the page itself; see [`DeliveryFilter::probe_limit`].
    async fn list_company_deliveries(
        &self,
        company_id: Uuid,
        filter: &DeliveryFilter,
    ) -> AppResult<Vec<DeliveryEntry>>;

    /// One delivery by id. The caller checks its `company_id` before showing it -- the id comes
    /// from a URL.
    async fn get_delivery(&self, delivery_id: DeliveryId) -> AppResult<Option<DeliveryEntry>>;

    /// Every delivery one task handed to a transport, oldest first.
    ///
    /// Exists so the task view can show delivery state without the transport writing back into
    /// task state. `company_id` is part of the query rather than checked afterwards, because the
    /// task id originates in a browser.
    async fn list_task_deliveries(
        &self,
        company_id: Uuid,
        task_id: Uuid,
    ) -> AppResult<Vec<DeliveryEntry>>;
}
