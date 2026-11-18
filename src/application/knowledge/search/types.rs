use super::ExactKnowledgeTerm;
use crate::{
    app_error::{AppError, AppResult},
    application::workflow::CompanyId,
    entities::{
        access_level::AccessLevel, department::DepartmentId, value_objects::EmbeddingModelId,
    },
};
use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnowledgeSearchLimit(u8);

impl KnowledgeSearchLimit {
    pub fn new(value: usize) -> AppResult<Self> {
        match value {
            1..=8 => Ok(Self(value as u8)),
            _ => Err(AppError::BadRequest(
                "Knowledge search must return between one and eight passages".into(),
            )),
        }
    }

    pub fn get(self) -> usize {
        usize::from(self.0)
    }
}

impl Default for KnowledgeSearchLimit {
    fn default() -> Self {
        Self(5)
    }
}

pub struct StoredKnowledgeChunk {
    pub id: Uuid,
    pub document_id: Uuid,
    pub unit_id: DepartmentId,
    pub document_name: crate::entities::value_objects::KnowledgeDocumentName,
    pub heading: Option<String>,
    pub body: String,
    pub access_level: AccessLevel,
    pub embedding: [f32; 384],
    pub keyword_rank: Option<f32>,
}

/// All authority here originates in the run context, never in tool arguments.
pub struct VisibleKnowledgeQuery {
    pub company: CompanyId,
    pub ceiling: AccessLevel,
    pub model: EmbeddingModelId,
    pub exact_terms: Vec<ExactKnowledgeTerm>,
    pub units: Vec<DepartmentId>,
}

#[async_trait]
pub trait KnowledgeChunks: Send + Sync {
    async fn visible(&self, query: VisibleKnowledgeQuery) -> AppResult<Vec<StoredKnowledgeChunk>>;
}

/// Classification is kept beside the final bounded result and never serialized to the model.
#[derive(Clone)]
pub struct KnowledgeObservation {
    result: Value,
    highest: Option<AccessLevel>,
    items: Vec<KnowledgeSourceItem>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSourceItem {
    pub document_id: Uuid,
    pub chunk_id: Uuid,
    pub unit_id: DepartmentId,
    pub access_level: AccessLevel,
}

impl KnowledgeObservation {
    pub(crate) fn new(
        result: Value,
        highest: Option<AccessLevel>,
        items: Vec<KnowledgeSourceItem>,
    ) -> Self {
        Self {
            result,
            highest,
            items,
        }
    }

    pub fn result(&self) -> &Value {
        &self.result
    }

    pub fn highest(&self) -> Option<AccessLevel> {
        self.highest
    }

    pub fn items(&self) -> &[KnowledgeSourceItem] {
        &self.items
    }
}
