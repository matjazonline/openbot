use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;

use crate::entities::memory::{
    MAX_MEMORY_ADDITIONAL_CONTEXT_CHARS, MAX_MEMORY_PERSIST_ASSISTANT_ANSWER_CHARS,
    MAX_MEMORY_PERSIST_USER_CONTEXT_CHARS, MAX_MEMORY_RECALL_QUERY_CHARS, MemoryChunk,
    MemoryProviderError, MemoryProviderKind, MemoryRecallMode, MemoryScope, ResolvedMemoryScope,
    truncate_memory_text,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct BoundedMemoryText {
    value: String,
    truncated: bool,
}

impl BoundedMemoryText {
    fn new(value: &str, max_chars: usize) -> Self {
        let (value, truncated) = truncate_memory_text(value, max_chars);
        Self { value, truncated }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryRecallQuery(BoundedMemoryText);

impl MemoryRecallQuery {
    pub fn new(value: &str) -> Self {
        Self(BoundedMemoryText::new(value, MAX_MEMORY_RECALL_QUERY_CHARS))
    }

    pub fn as_str(&self) -> &str {
        &self.0.value
    }

    pub fn was_truncated(&self) -> bool {
        self.0.truncated
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryAdditionalContext(BoundedMemoryText);

impl MemoryAdditionalContext {
    pub fn new(value: &str) -> Self {
        Self(BoundedMemoryText::new(
            value,
            MAX_MEMORY_ADDITIONAL_CONTEXT_CHARS,
        ))
    }

    pub fn as_str(&self) -> &str {
        &self.0.value
    }

    pub fn was_truncated(&self) -> bool {
        self.0.truncated
    }
}

#[derive(Debug, Clone)]
pub struct MemoryConversation {
    pub id: String,
    user: BoundedMemoryText,
    assistant: BoundedMemoryText,
}

impl MemoryConversation {
    pub fn new(id: String, user: &str, assistant: &str) -> Self {
        Self {
            id,
            user: BoundedMemoryText::new(user, MAX_MEMORY_PERSIST_USER_CONTEXT_CHARS),
            assistant: BoundedMemoryText::new(assistant, MAX_MEMORY_PERSIST_ASSISTANT_ANSWER_CHARS),
        }
    }

    pub fn user(&self) -> &str {
        &self.user.value
    }

    pub fn assistant(&self) -> &str {
        &self.assistant.value
    }

    pub fn user_was_truncated(&self) -> bool {
        self.user.truncated
    }

    pub fn assistant_was_truncated(&self) -> bool {
        self.assistant.truncated
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryPersistenceTarget {
    pub scope: MemoryScope,
    pub collection: String,
    pub custom_instructions: Option<&'static str>,
}

#[async_trait]
pub trait MemoryProvider: Send + Sync {
    async fn provision(&self, database_id: &str) -> Result<(), MemoryProviderError>;
    async fn is_ready(&self, database_id: &str) -> Result<bool, MemoryProviderError>;
    async fn recall(
        &self,
        database_id: &str,
        query: &MemoryRecallQuery,
        scopes: &[ResolvedMemoryScope],
        mode: MemoryRecallMode,
        max_results: u8,
        additional_context: Option<&MemoryAdditionalContext>,
    ) -> Result<Vec<MemoryChunk>, MemoryProviderError>;
    async fn persist(
        &self,
        database_id: &str,
        targets: &[MemoryPersistenceTarget],
        conversation: &MemoryConversation,
    ) -> Vec<Result<(), MemoryProviderError>>;
    async fn delete(&self, database_id: &str) -> Result<(), MemoryProviderError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::memory::{
        MAX_MEMORY_ADDITIONAL_CONTEXT_CHARS, MAX_MEMORY_PERSIST_ASSISTANT_ANSWER_CHARS,
        MAX_MEMORY_PERSIST_USER_CONTEXT_CHARS, MAX_MEMORY_RECALL_QUERY_CHARS,
        MEMORY_TRUNCATION_MARKER,
    };

    fn assert_unicode_bound(value: &str, expected_chars: usize) {
        assert_eq!(value.chars().count(), expected_chars);
        assert!(value.ends_with(MEMORY_TRUNCATION_MARKER));
    }

    #[test]
    fn recall_query_and_additional_context_boundaries_are_explicit() {
        let exact = MemoryRecallQuery::new(&"é".repeat(MAX_MEMORY_RECALL_QUERY_CHARS));
        assert!(!exact.was_truncated());
        let over = MemoryRecallQuery::new(&"é".repeat(MAX_MEMORY_RECALL_QUERY_CHARS + 1));
        assert!(over.was_truncated());
        assert_unicode_bound(over.as_str(), MAX_MEMORY_RECALL_QUERY_CHARS);

        let additional =
            MemoryAdditionalContext::new(&"🦀".repeat(MAX_MEMORY_ADDITIONAL_CONTEXT_CHARS + 1));
        assert!(additional.was_truncated());
        assert_unicode_bound(additional.as_str(), MAX_MEMORY_ADDITIONAL_CONTEXT_CHARS);
        let exact_additional =
            MemoryAdditionalContext::new(&"🦀".repeat(MAX_MEMORY_ADDITIONAL_CONTEXT_CHARS));
        assert!(!exact_additional.was_truncated());
    }

    #[test]
    fn conversation_bounds_user_and_assistant_independently() {
        let exact = MemoryConversation::new(
            "id".into(),
            &"é".repeat(MAX_MEMORY_PERSIST_USER_CONTEXT_CHARS),
            &"🦀".repeat(MAX_MEMORY_PERSIST_ASSISTANT_ANSWER_CHARS),
        );
        assert!(!exact.user_was_truncated());
        assert!(!exact.assistant_was_truncated());

        let conversation = MemoryConversation::new(
            "id".into(),
            &"é".repeat(MAX_MEMORY_PERSIST_USER_CONTEXT_CHARS + 1),
            &"🦀".repeat(MAX_MEMORY_PERSIST_ASSISTANT_ANSWER_CHARS + 1),
        );
        assert!(conversation.user_was_truncated());
        assert!(conversation.assistant_was_truncated());
        assert_unicode_bound(conversation.user(), MAX_MEMORY_PERSIST_USER_CONTEXT_CHARS);
        assert_unicode_bound(
            conversation.assistant(),
            MAX_MEMORY_PERSIST_ASSISTANT_ANSWER_CHARS,
        );
    }
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
