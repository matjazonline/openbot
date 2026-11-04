//! Shared channel persistence contract, independent of use-case orchestration.
use crate::{
    app_error::AppResult,
    entities::{
        channel::Channel,
        creation::CreationProvenance,
        value_objects::{ChannelSlug, CompanySlug},
    },
};
use async_trait::async_trait;
use uuid::Uuid;
/// Everything one channel write sets, so create and update cannot drift apart and so a caller
/// cannot transpose two same-typed arguments in a nine-parameter list.
///
/// Values reach persistence already normalized — see [`ChannelWrite::normalize`].
#[derive(Debug, Clone, Default)]
pub struct ChannelWrite {
    pub name: String,
    /// What the channel is for, in one line. `None` and a blank string mean the same thing, so
    /// every form boundary normalizes blank to `None` before building the write.
    pub description: Option<String>,
    pub slug: String,
    /// Extra local parts the channel also answers on. Replaced wholesale by every write.
    pub alias_slugs: Vec<String>,
    pub participant_emails: Option<Vec<String>>,
    pub enabled: bool,
    /// Whether a trusted sender may pull CC'd outsiders onto this channel's threads.
    pub add_3rd_party: bool,
    pub created_by: Option<CreationProvenance>,
}

/// One authorized channel edit, as persistence receives it.
///
/// The tenant travels with the write rather than beside it: persistence re-checks the channel
/// against `company_id` under its row lock.
#[derive(Debug, Clone)]
pub struct ChannelUpdate {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub write: ChannelWrite,
}

#[async_trait]
pub trait ChannelPersistence: Send + Sync {
    async fn create(&self, company_id: Uuid, write: ChannelWrite) -> AppResult<Channel>;

    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Channel>>;

    async fn get_by_company_slug_and_channel_slug(
        &self,
        company_slug: &CompanySlug,
        channel_slug: &ChannelSlug,
    ) -> AppResult<Option<Channel>>;

    async fn list_by_company_id(&self, company_id: Uuid) -> AppResult<Vec<Channel>>;

    /// Save one channel edit under the channel's gate; any failure leaves it unchanged.
    async fn update(&self, request: ChannelUpdate) -> AppResult<Channel>;

    async fn delete(&self, id: Uuid) -> AppResult<()>;
}
