use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

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

/// The providers this deployment has been configured for, as opposed to the one a company has
/// selected. A set rather than a bool per provider: "is memory available here at all" and "is
/// *this* provider available here" are the only two questions asked of it, and both stay one
/// call as providers are added.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfiguredMemoryProviders(HashSet<MemoryProviderKind>);

impl ConfiguredMemoryProviders {
    pub fn contains(&self, kind: MemoryProviderKind) -> bool {
        self.0.contains(&kind)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl ConfiguredMemoryProviders {
    /// Resolve a submitted provider value against what this deployment can actually run.
    ///
    /// Both the JSON API and the HTML forms take this value, so the decision — including which
    /// spellings mean "off" and how an unconfigured provider is reported — lives here rather than
    /// being written out once per transport.
    pub fn select(&self, value: Option<&str>) -> Result<SelectedMemoryProvider, String> {
        match value.map(str::trim) {
            None | Some("") | Some("none") => Ok(SelectedMemoryProvider::Disabled),
            Some(value) => match MemoryProviderKind::parse(value) {
                Some(kind) if self.contains(kind) => Ok(SelectedMemoryProvider::Provider(kind)),
                Some(kind) => Err(format!(
                    "{} is not configured for this deployment.",
                    kind.label()
                )),
                None => Err("Unsupported memory provider.".into()),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectedMemoryProvider {
    Disabled,
    Provider(MemoryProviderKind),
}

impl FromIterator<MemoryProviderKind> for ConfiguredMemoryProviders {
    fn from_iter<I: IntoIterator<Item = MemoryProviderKind>>(kinds: I) -> Self {
        Self(kinds.into_iter().collect())
    }
}

#[cfg(test)]
mod configured_provider_tests {
    use super::*;

    #[test]
    fn a_submitted_value_resolves_only_against_configured_providers() {
        let configured = ConfiguredMemoryProviders::from_iter([MemoryProviderKind::Hindsight]);
        assert_eq!(
            configured.select(Some("hindsight")),
            Ok(SelectedMemoryProvider::Provider(
                MemoryProviderKind::Hindsight
            ))
        );
        assert_eq!(
            configured.select(Some("hydradb")),
            Err("HydraDB is not configured for this deployment.".into())
        );
        assert_eq!(
            configured.select(Some("shodh")),
            Err("Unsupported memory provider.".into())
        );
        for off in [None, Some(""), Some("none"), Some("  ")] {
            assert_eq!(configured.select(off), Ok(SelectedMemoryProvider::Disabled));
        }
    }

    #[test]
    fn an_unconfigured_deployment_offers_nothing() {
        let configured = ConfiguredMemoryProviders::default();
        assert!(configured.is_empty());
        for kind in MemoryProviderKind::ALL {
            assert!(!configured.contains(kind));
            assert!(configured.select(Some(kind.as_str())).is_err());
        }
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
