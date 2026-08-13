use std::collections::HashMap;
use std::sync::Arc;
use async_trait::async_trait;

use crate::{
    app_error::AppResult,
    entities::{
        channel::ChannelType,
        message_contract::NormalizedOutboundMessage,
    },
    use_cases::thread::BounceInfo,
};

pub mod email;

#[async_trait]
pub trait ProtocolEgressAdapter: Send + Sync {
    fn protocol(&self) -> ChannelType;
    async fn dispatch(&self, message: &NormalizedOutboundMessage) -> AppResult<()>;
    async fn dispatch_bounce(&self, bounce_info: &BounceInfo) -> AppResult<()>;
}

#[derive(Clone, Default)]
pub struct EgressRegistry {
    adapters: HashMap<ChannelType, Arc<dyn ProtocolEgressAdapter>>,
}

impl EgressRegistry {
    pub fn new() -> Self {
        Self {
            adapters: HashMap::new(),
        }
    }

    pub fn register(mut self, adapter: Arc<dyn ProtocolEgressAdapter>) -> Self {
        self.adapters.insert(adapter.protocol(), adapter);
        self
    }

    pub fn get(&self, protocol: &ChannelType) -> Option<Arc<dyn ProtocolEgressAdapter>> {
        self.adapters.get(protocol).cloned()
    }
}
