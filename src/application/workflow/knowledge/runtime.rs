use super::*;
use crate::{
    app_error::{AppError, AppResult},
    application::{
        knowledge::{KnowledgeLibrary, search::*},
        workflow::{agent::AgentToolObservation, lease::WorkflowFence, tools::tool_error},
    },
};
use async_trait::async_trait;
use std::sync::Arc;

pub struct KnowledgeSearchRuntime {
    library: Arc<KnowledgeLibrary>,
    chunks: Arc<dyn KnowledgeChunks>,
    persistence: Arc<dyn KnowledgeSearchPersistence>,
    limit: KnowledgeSearchLimit,
}

impl KnowledgeSearchRuntime {
    pub fn new(
        library: Arc<KnowledgeLibrary>,
        chunks: Arc<dyn KnowledgeChunks>,
        persistence: Arc<dyn KnowledgeSearchPersistence>,
        limit: KnowledgeSearchLimit,
    ) -> Self {
        Self {
            library,
            chunks,
            persistence,
            limit,
        }
    }
}

#[async_trait]
impl KnowledgeToolDispatch for KnowledgeSearchRuntime {
    async fn call_knowledge_tool(
        &self,
        fence: WorkflowFence,
        call: KnowledgeToolCall,
    ) -> AppResult<AgentToolObservation> {
        let access = match self.persistence.knowledge_context(fence, &call).await? {
            KnowledgeSearchContext::Recorded(result) => {
                return Ok(AgentToolObservation::Completed(result));
            }
            KnowledgeSearchContext::Pending(access) => access,
        };
        let query = match KnowledgeSearchQuery::parse(&call.arguments) {
            Ok(query) => query,
            Err(_) => {
                return self
                    .persistence
                    .observe_knowledge(fence, &call, invalid_query())
                    .await
                    .map(AgentToolObservation::Completed);
            }
        };
        let embedding = match self.library.embed_query(query.question()).await {
            Ok(embedding) => embedding,
            Err(AppError::BadRequest(_)) => {
                return self
                    .persistence
                    .observe_knowledge(fence, &call, invalid_query())
                    .await
                    .map(AgentToolObservation::Completed);
            }
            Err(error) => return Err(error),
        };
        // Keep only a boxed external seam in the parent agent future's poll frame.
        let result = Box::pin(async {
            let chunks = self
                .chunks
                .visible(VisibleKnowledgeQuery {
                    company: access.company,
                    ceiling: access.ceiling,
                    model: embedding.model().clone(),
                    exact_terms: query.exact_terms(),
                })
                .await?;
            embedding.ensure_live()?;
            let observation = rank_chunks(chunks, embedding.vector(), self.limit, result_bound())?;
            embedding.ensure_live()?;
            self.persistence
                .observe_knowledge(fence, &call, observation)
                .await
        });
        tokio::select! {
            biased;
            _ = embedding.cancellation().cancelled() => Err(timed_out()),
            result = tokio::time::timeout_at(embedding.deadline(), result) => {
                result.map_err(|_| timed_out())?.map(AgentToolObservation::Completed)
            }
        }
    }
}

fn result_bound() -> usize {
    crate::application::workflow::tools::MAX_TOOL_RESULT_BYTES
        .min(crate::services::harness::runs::MAX_RESULT_BYTES)
}

fn invalid_query() -> KnowledgeObservation {
    KnowledgeObservation::new(
        tool_error(
            "knowledge.invalid_query",
            "Use a shorter question of 1–1000 characters that fits the local model's 512-token limit, with at most eight exact terms of 1–64 characters and no other fields",
        ),
        None,
    )
}

fn timed_out() -> AppError {
    AppError::Timeout("Knowledge search was canceled or exceeded its deadline".into())
}
