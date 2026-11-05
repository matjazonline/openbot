use super::KnowledgeChunk;
use crate::{
    app_error::{AppError, AppResult},
    domain::workflow::CompanyId,
    entities::{
        access_level::{AccessLevel, LevelActor},
        knowledge::*,
        unicode_sanitization::sanitize_string,
        value_objects::{EmbeddingModelId, KnowledgeDocumentName},
    },
};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KnowledgeScope {
    pub company: CompanyId,
    pub actor_id: Uuid,
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
}
#[derive(Debug, Clone, Copy)]
pub struct DeleteKnowledgeLayer {
    pub scope: KnowledgeScope,
    pub document_id: Uuid,
    pub expected_revision: i64,
}

/// Every implementation enforces fresh scoped authority and returns no higher-layer metadata.
#[async_trait]
pub trait KnowledgeDocuments: Send + Sync {
    async fn access(&self, scope: KnowledgeScope) -> AppResult<LevelActor>;
    async fn list(&self, scope: KnowledgeScope) -> AppResult<KnowledgeLibraryView>;
    async fn preflight_upload(
        &self,
        scope: KnowledgeScope,
        layers: &[KnowledgeLayerSize],
    ) -> AppResult<()>;
    async fn replace(
        &self,
        scope: KnowledgeScope,
        documents: &[PreparedKnowledgeDocument],
    ) -> AppResult<Vec<KnowledgeDocumentLayer>>;
    async fn delete(&self, command: DeleteKnowledgeLayer) -> AppResult<()>;
}

/// CPU-only synchronous model operations; callers own bounded blocking work and cancellation.
pub trait TextEmbedder: Send + Sync {
    fn model_id(&self) -> EmbeddingModelId;
    fn passage_token_count(&self, passage: &str) -> AppResult<usize>;
    fn embed_passages(&self, passages: &[&str]) -> AppResult<Vec<[f32; 384]>>;
    fn embed_query(&self, query: &str) -> AppResult<[f32; 384]>;
}
