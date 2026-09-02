use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

use crate::{
    app_error::AppResult,
    entities::{message_contract::NormalizedOutboundMessage, transport::TransportKind},
    use_cases::thread::BounceInfo,
};

pub mod email;

#[async_trait]
pub trait ProtocolEgressAdapter: Send + Sync {
    fn transport(&self) -> TransportKind;
    async fn dispatch(&self, message: &NormalizedOutboundMessage) -> AppResult<()>;
    async fn dispatch_bounce(&self, bounce_info: &BounceInfo) -> AppResult<()>;
}

#[derive(Clone, Default)]
pub struct EgressRegistry {
    adapters: HashMap<TransportKind, Arc<dyn ProtocolEgressAdapter>>,
}

impl EgressRegistry {
    pub fn new() -> Self {
        Self {
            adapters: HashMap::new(),
        }
    }

    pub fn register(mut self, adapter: Arc<dyn ProtocolEgressAdapter>) -> Self {
        self.adapters.insert(adapter.transport(), adapter);
        self
    }

    pub fn get(&self, transport: &TransportKind) -> Option<Arc<dyn ProtocolEgressAdapter>> {
        self.adapters.get(transport).cloned()
    }
}
