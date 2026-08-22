use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryProviderKind {
    #[default]
    None,
    Hydradb,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryRecallMode {
    #[default]
    Fast,
    Thinking,
}

impl MemoryRecallMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Thinking => "thinking",
        }
    }
}

pub fn default_memory_max_results() -> u8 {
    5
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryScope {
    Company,
    Agent(Uuid),
    User,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedMemoryScope {
    pub scope: MemoryScope,
    pub collection: String,
    pub weight: f32,
}

pub fn normalize_sender_email(email: &str) -> String {
    email.trim().to_lowercase()
}

pub fn resolve_scopes(
    company: bool,
    agent: bool,
    user: bool,
    agent_id: Option<Uuid>,
    sender_email: Option<&str>,
) -> (Vec<ResolvedMemoryScope>, Vec<&'static str>) {
    let mut by_collection: HashMap<String, ResolvedMemoryScope> = HashMap::new();
    let mut warnings = Vec::new();
    let requested = [
        (company, MemoryScope::Company, 1.0),
        (
            agent,
            agent_id
                .map(MemoryScope::Agent)
                .unwrap_or(MemoryScope::Company),
            2.0,
        ),
        (user, MemoryScope::User, 3.0),
    ];
    for (enabled, scope, weight) in requested {
        if !enabled {
            continue;
        }
        let collection = match scope {
            MemoryScope::Company => "company".to_string(),
            MemoryScope::Agent(id) => format!("agent_{id}"),
            MemoryScope::User => match sender_email
                .map(normalize_sender_email)
                .filter(|e| !e.is_empty())
            {
                Some(email) => format!("user_{email}"),
                None => {
                    warnings.push("user");
                    "company".to_string()
                }
            },
        };
        if agent && agent_id.is_none() && weight == 2.0 {
            warnings.push("agent");
        }
        by_collection
            .entry(collection.clone())
            .and_modify(|existing| existing.weight = existing.weight.max(weight))
            .or_insert(ResolvedMemoryScope {
                scope,
                collection,
                weight,
            });
    }
    let mut scopes: Vec<_> = by_collection.into_values().collect();
    scopes.sort_by(|a, b| a.collection.cmp(&b.collection));
    warnings.sort_unstable();
    warnings.dedup();
    (scopes, warnings)
}

pub fn stable_memory_id(task_id: Uuid, channel_id: Uuid, agent_id: Option<Uuid>) -> String {
    let mut hash = Sha256::new();
    hash.update(task_id.as_bytes());
    hash.update(channel_id.as_bytes());
    if let Some(id) = agent_id {
        hash.update(id.as_bytes());
    }
    format!("memory_{:x}", hash.finalize())
}

pub fn deduplicate_chunks(chunks: impl IntoIterator<Item = MemoryChunk>) -> Vec<MemoryChunk> {
    let mut ids = HashSet::new();
    let mut contents = HashSet::new();
    let mut total = 0usize;
    let mut result = Vec::new();
    for mut chunk in chunks {
        let normalized = chunk
            .content
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        let unique = chunk
            .source_chunk_id
            .as_ref()
            .map(|id| ids.insert(id.clone()))
            .unwrap_or_else(|| contents.insert(normalized));
        if !unique || total >= 16_000 {
            continue;
        }
        let remaining = 16_000 - total;
        if chunk.content.len() > remaining {
            chunk.content.truncate(remaining);
        }
        total += chunk.content.len();
        result.push(chunk);
    }
    result
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryChunk {
    pub source_chunk_id: Option<String>,
    pub content: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fallback_deduplicates_and_keeps_highest_weight() {
        let (scopes, warnings) = resolve_scopes(true, true, true, None, None);
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].collection, "company");
        assert_eq!(scopes[0].weight, 3.0);
        assert_eq!(warnings, ["agent", "user"]);
    }
    #[test]
    fn all_available_scopes_are_additive() {
        let agent = Uuid::nil();
        let (scopes, warnings) =
            resolve_scopes(true, true, true, Some(agent), Some(" USER@Example.COM "));
        assert_eq!(scopes.len(), 3);
        assert!(warnings.is_empty());
        assert!(
            scopes
                .iter()
                .any(|s| s.collection == "user_user@example.com" && s.weight == 3.0)
        );
    }
    #[test]
    fn memory_id_is_stable_and_agent_sensitive() {
        let task = Uuid::new_v4();
        let channel = Uuid::new_v4();
        assert_eq!(
            stable_memory_id(task, channel, None),
            stable_memory_id(task, channel, None)
        );
        assert_ne!(
            stable_memory_id(task, channel, None),
            stable_memory_id(task, channel, Some(Uuid::nil()))
        );
    }
}
