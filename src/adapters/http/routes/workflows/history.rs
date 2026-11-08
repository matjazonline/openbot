//! Existing workflow inspection API; handles do not grant authority.
use super::*;
use crate::application::workflow::{
    agent_history::AgentExchangeId, agent_history_views::HistoryPageRequest,
    run_views::RunReadScope,
};
use crate::domain::workflow::RunId;
use axum::extract::{Path, Query, State};
use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
pub(super) struct HistoryQuery {
    pub after: Option<String>,
    pub limit: Option<u8>,
    pub exchange: Option<Uuid>,
}
pub(super) enum HistoryRequest {
    Page(HistoryPageRequest),
    Exchange(AgentExchangeId),
}
impl HistoryQuery {
    pub fn request(self) -> AppResult<HistoryRequest> {
        if let Some(exchange) = self.exchange {
            if self.after.is_some() || self.limit.is_some() {
                return Err(AppError::BadRequest(
                    "Exchange inspection does not take a page cursor".into(),
                ));
            }
            return Ok(HistoryRequest::Exchange(AgentExchangeId::new(exchange)));
        }
        Ok(HistoryRequest::Page(HistoryPageRequest::new(
            self.after, self.limit,
        )?))
    }
}

pub(super) async fn read(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path((company_id, run, execution)): Path<(Uuid, Uuid, Uuid)>,
    Query(query): Query<HistoryQuery>,
) -> WorkflowResult<Json<Value>> {
    let scope = RunReadScope {
        company: company(company_id),
        run: RunId::new(run),
        viewer: actor(&user)?,
    };
    let value = match query.request()? {
        HistoryRequest::Page(request) => serde_json::to_value(
            engine
                .run_views()
                .agent_history_page(scope, execution, request)
                .await?
                .ok_or_else(|| AppError::NotFound("Agent history".into()))?,
        ),
        HistoryRequest::Exchange(exchange) => serde_json::to_value(
            engine
                .run_views()
                .agent_history_exchange(scope, execution, exchange)
                .await?
                .ok_or_else(|| AppError::NotFound("Agent exchange".into()))?,
        ),
    }
    .map_err(|_| AppError::Internal("Agent history response could not be encoded".into()))?;
    Ok(ok(value))
}
