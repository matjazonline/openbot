use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;

use crate::entities::memory::{
    MemoryChunk, MemoryProviderError, MemoryProviderKind, MemoryRecallMode, ResolvedMemoryScope,
};

#[derive(Debug, Clone)]
pub struct MemoryConversation {
    pub id: String,
    pub user: String,
    pub assistant: String,
}

#[async_trait]
pub trait MemoryProvider: Send + Sync {
    async fn provision(&self, database_id: &str) -> Result<(), MemoryProviderError>;
    async fn is_ready(&self, database_id: &str) -> Result<bool, MemoryProviderError>;
    async fn recall(
        &self,
        database_id: &str,
        query: &str,
        scopes: &[ResolvedMemoryScope],
        mode: MemoryRecallMode,
        max_results: u8,
        additional_context: Option<&str>,
    ) -> Result<Vec<MemoryChunk>, MemoryProviderError>;
    async fn persist(
        &self,
        database_id: &str,
        collections: &[String],
        conversation: &MemoryConversation,
    ) -> Vec<Result<(), MemoryProviderError>>;
    async fn delete(&self, database_id: &str) -> Result<(), MemoryProviderError>;
}

#[derive(Default)]
pub struct MemoryProviderRegistry(HashMap<MemoryProviderKind, Arc<dyn MemoryProvider>>);

impl MemoryProviderRegistry {
    pub fn register(mut self, kind: MemoryProviderKind, provider: Arc<dyn MemoryProvider>) -> Self {
        self.0.insert(kind, provider);
        self
    }
    pub fn get(&self, kind: MemoryProviderKind) -> Option<&Arc<dyn MemoryProvider>> {
        self.0.get(&kind)
    }
}
