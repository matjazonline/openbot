//! Reading the delivery queue.
//!
//! Read-only, and deliberately so: the delivery worker owns these rows, and a page that offered
//! buttons would be racing the transport. The port lives here because the `/ui` Deliveries routes
//! are what consume it; the SQLx projection that answers it is an adapter.

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
        scope: DeliveryReadScope,
        filter: &DeliveryFilter,
    ) -> AppResult<Vec<DeliveryEntry>>;

    /// One delivery by its company-scoped id, with current viewer classification applied.
    async fn get_delivery(
        &self,
        scope: DeliveryReadScope,
        delivery_id: DeliveryId,
    ) -> AppResult<Option<DeliveryEntry>>;
}

#[derive(Debug, Clone, Copy)]
pub struct DeliveryReadScope {
    pub company_id: Uuid,
    pub viewer: crate::application::workflow::WorkflowActor,
}

pub(crate) fn project_delivery(
    entry: &mut DeliveryEntry,
    visibility: crate::application::knowledge::PayloadVisibility,
) {
    if visibility.is_visible() {
        return;
    }
    entry.subject = "Restricted message".into();
    entry.last_error_detail = None;
    for part in &mut entry.parts {
        part.last_error_detail = None;
        part.provider_message_key = None;
    }
}
