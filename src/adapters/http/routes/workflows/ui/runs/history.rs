use super::super::super::history::{HistoryQuery, HistoryRequest};
use super::*;
use crate::application::workflow::run_views::RunReadScope;

#[derive(Deserialize)]
pub(super) struct HistoryPageQuery {
    company_id: Uuid,
    #[serde(flatten)]
    query: HistoryQuery,
}

pub(super) async fn history(
    ui: Ui,
    Path((run, execution)): Path<(Uuid, Uuid)>,
    Query(query): Query<HistoryPageQuery>,
) -> AppResult<Html<String>> {
    let company = ui.company(query.company_id).await?.id;
    let scope = RunReadScope {
        company: super::company(company),
        run: RunId::new(run),
        viewer: ui.actor()?,
    };
    let anchor = pages::HistoryAnchor {
        company,
        run,
        execution,
    };
    let body = match query.query.request()? {
        HistoryRequest::Page(request) => {
            let supplied_cursor = request.after.clone();
            match ui
                .engine
                .run_views()
                .agent_history_page(scope, execution, request)
                .await?
            {
                Some(page) => pages::agent_history_section(anchor, &page),
                None if supplied_cursor.is_some() => {
                    return Err(AppError::NotFound("Agent history cursor".into()));
                }
                None => {
                    ui.engine
                        .run_views()
                        .step_view(scope, execution)
                        .await?
                        .ok_or_else(|| AppError::NotFound("Workflow step".into()))?;
                    ui.visible_run(company, run).await?;
                    format!(
                        "<section id=\"{}\"><p>This step has no recorded agent exchange. History is automatic for prepared agent.run calls.</p></section>",
                        anchor.region()
                    )
                }
            }
        }
        HistoryRequest::Exchange(exchange) => {
            let detail = ui
                .engine
                .run_views()
                .agent_history_exchange(scope, execution, exchange)
                .await?
                .ok_or_else(|| AppError::NotFound("Agent exchange".into()))?;
            pages::agent_history_detail(anchor, &detail)
        }
    };
    Ok(Html(body))
}
