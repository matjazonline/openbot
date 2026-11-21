use super::KnowledgeChunk;
use crate::{
    app_error::{AppError, AppResult},
    domain::workflow::CompanyId,
    entities::{
        access_level::{AccessLevel, LevelActor},
        department::DepartmentId,
        knowledge::*,
        unicode_sanitization::sanitize_string,
        value_objects::{EmbeddingModelId, KnowledgeDocumentName},
    },
};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const KNOWLEDGE_SCOPE_LIMIT_EXCEEDED: &str = "knowledge.scope_limit_exceeded";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KnowledgeScope {
    pub company: CompanyId,
    pub actor_id: Uuid,
}

#[derive(Debug, Clone, Copy)]
pub struct KnowledgeUploadTarget {
    pub scope: KnowledgeScope,
    pub unit: DepartmentId,
}

pub struct KnowledgeUploadFile {
    file_name: KnowledgeFileName,
    text: String,
    sha256: [u8; 32],
}
impl KnowledgeUploadFile {
    pub fn from_upload(raw_name: &str, bytes: Vec<u8>) -> AppResult<Self> {
        let file_name = KnowledgeFileName::parse(raw_name)
            .map_err(|error| AppError::BadRequest(error.to_string()))?;
        let error =
            |message: &str| AppError::BadRequest(format!("`{}`: {message}", file_name.prefixed()));
        if !file_name.is_supported_text() {
            return Err(error("only UTF-8 .txt and .md files are supported"));
        }
        if bytes.len() > MAX_KNOWLEDGE_FILE_BYTES {
            return Err(error("the file exceeds 1 MiB"));
        }
        if bytes.contains(&0) {
            return Err(error("NUL bytes are not supported"));
        }
        let text = String::from_utf8(bytes).map_err(|_| error("the file must be valid UTF-8"))?;
        let text = sanitize_string(text);
        let sha256 = Sha256::digest(text.as_bytes()).into();
        Ok(Self {
            file_name,
            text,
            sha256,
        })
    }
    pub fn file_name(&self) -> &KnowledgeFileName {
        &self.file_name
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn sha256(&self) -> &[u8; 32] {
        &self.sha256
    }
    pub fn size(&self) -> KnowledgeLayerSize {
        KnowledgeLayerSize {
            name: self.file_name.name().clone(),
            access_level: self.file_name.level(),
            text_bytes: self.text.len() as u64,
        }
    }
}
#[derive(Clone)]
pub struct KnowledgeLayerSize {
    pub name: KnowledgeDocumentName,
    pub access_level: AccessLevel,
    pub text_bytes: u64,
}
pub struct EmbeddedKnowledgeChunk {
    pub chunk: KnowledgeChunk,
    pub embedding: [f32; 384],
}
pub struct PreparedKnowledgeDocument {
    pub file: KnowledgeUploadFile,
    pub embedding_model: EmbeddingModelId,
    pub chunks: Vec<EmbeddedKnowledgeChunk>,
}
#[derive(Debug, Clone)]
pub struct KnowledgeLibraryView {
    pub actor: LevelActor,
    pub layers: Vec<KnowledgeDocumentLayer>,
    pub upload_units: Vec<KnowledgeUnit>,
    pub writable_units: Vec<DepartmentId>,
    pub default_unit: Option<DepartmentId>,
    pub units_truncated: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct KnowledgeUnitQuery {
    pub selected: Option<DepartmentId>,
    pub after: Option<DepartmentId>,
}

#[derive(Debug, Clone)]
pub struct KnowledgeUnitPage {
    pub units: Vec<KnowledgeUnit>,
    pub next: Option<DepartmentId>,
}
#[derive(Debug, Clone, Copy)]
pub struct DeleteKnowledgeLayer {
    pub scope: KnowledgeScope,
    pub document_id: Uuid,
    pub expected_revision: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct MoveKnowledgeDocument {
    pub scope: KnowledgeScope,
    pub document_id: Uuid,
    pub expected_revision: i64,
    pub target: DepartmentId,
}

pub struct ManualKnowledgeQuery {
    pub scope: KnowledgeScope,
    pub unit: Option<DepartmentId>,
    pub model: EmbeddingModelId,
    pub exact_terms: Vec<super::search::ExactKnowledgeTerm>,
}

/// Every implementation enforces fresh scoped authority and returns no higher-layer metadata.
#[async_trait]
pub trait KnowledgeDocuments: Send + Sync {
    async fn access(&self, scope: KnowledgeScope) -> AppResult<LevelActor>;
    async fn preflight_search(
        &self,
        scope: KnowledgeScope,
        unit: Option<DepartmentId>,
    ) -> AppResult<()>;
    async fn unit_choices(
        &self,
        scope: KnowledgeScope,
        query: KnowledgeUnitQuery,
    ) -> AppResult<KnowledgeUnitPage>;
    async fn list(
        &self,
        scope: KnowledgeScope,
        unit: Option<DepartmentId>,
    ) -> AppResult<KnowledgeLibraryView>;
    async fn search_manual(
        &self,
        query: ManualKnowledgeQuery,
    ) -> AppResult<Vec<super::search::StoredKnowledgeChunk>>;
    async fn preflight_upload(
        &self,
        target: KnowledgeUploadTarget,
        layers: &[KnowledgeLayerSize],
    ) -> AppResult<()>;
    async fn replace(
        &self,
        target: KnowledgeUploadTarget,
        documents: &[PreparedKnowledgeDocument],
    ) -> AppResult<Vec<KnowledgeDocumentLayer>>;
    async fn delete(&self, command: DeleteKnowledgeLayer) -> AppResult<()>;
    async fn move_document(&self, command: MoveKnowledgeDocument) -> AppResult<()>;
}

/// CPU-only synchronous model operations; callers own bounded blocking work and cancellation.
pub trait TextEmbedder: Send + Sync {
    fn model_id(&self) -> EmbeddingModelId;
    fn passage_token_count(&self, passage: &str) -> AppResult<usize>;
    fn embed_passages(&self, passages: &[&str]) -> AppResult<Vec<[f32; 384]>>;
    fn embed_query(&self, query: &str) -> AppResult<[f32; 384]>;
}
