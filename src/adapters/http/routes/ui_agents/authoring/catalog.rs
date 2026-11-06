use super::*;
use crate::domain::workflow::TypeName;

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/companies/{company_id}/agents/{agent_id}/tool-connections",
            get(connections_json),
        )
        .route(
            "/api/companies/{company_id}/agents/{agent_id}/tool-options",
            get(options_json),
        )
        .route(
            "/api/companies/{company_id}/agents/{agent_id}/history",
            get(history_json),
        )
        .route(
            "/api/companies/{company_id}/agents/directory",
            get(directory_json),
        )
        .route("/ui/agents/{agent_id}/tool-catalog", get(catalog_form))
        .route("/ui/agents/{agent_id}/history", get(history_form))
}

fn builtin() -> AgentToolKind {
    AgentToolKind::Builtin
}

#[derive(Deserialize)]
pub(crate) struct CatalogQuery {
    pub company_id: Uuid,
    #[serde(default = "builtin")]
    pub kind: AgentToolKind,
    pub connection: Option<Uuid>,
    pub after_connection: Option<Uuid>,
    pub after_operation: Option<TypeName>,
}

#[derive(Deserialize)]
struct ConnectionQuery {
    kind: AgentToolKind,
    after: Option<Uuid>,
}
async fn connections_json(
    workspace: Workspace,
    Path((company, agent)): Path<(Uuid, Uuid)>,
    Query(query): Query<ConnectionQuery>,
) -> AppResult<impl IntoResponse> {
    let scope = scope(workspace.user_id, company, agent)?;
    let page = workspace
        .workflow
        .agent_authoring_reads()
        .tool_connections(scope, query.kind, query.after)
        .await?;
    fresh_json(&workspace, scope, page).await
}

#[derive(Deserialize)]
struct OptionsQuery {
    kind: AgentToolKind,
    connection: Option<Uuid>,
    after: Option<TypeName>,
}
async fn options_json(
    workspace: Workspace,
    Path((company, agent)): Path<(Uuid, Uuid)>,
    Query(query): Query<OptionsQuery>,
) -> AppResult<impl IntoResponse> {
    let scope = scope(workspace.user_id, company, agent)?;
    let page = workspace
        .workflow
        .agent_authoring_reads()
        .tool_options(AgentToolPageRequest {
            scope,
            kind: query.kind,
            connection: query.connection,
            after: query.after,
        })
        .await?;
    fresh_json(&workspace, scope, page).await
}

#[derive(Deserialize)]
struct HistoryQuery {
    before_created_at: Option<chrono::DateTime<chrono::Utc>>,
    before_id: Option<AgentRevisionId>,
}
impl HistoryQuery {
    fn cursor(self) -> AppResult<Option<AgentHistoryCursor>> {
        match (self.before_created_at, self.before_id) {
            (None, None) => Ok(None),
            (Some(created_at), Some(id)) => Ok(Some(AgentHistoryCursor { created_at, id })),
            _ => Err(AppError::BadRequest(
                "History continuation requires its timestamp and revision identity".into(),
            )),
        }
    }
}
async fn history_json(
    workspace: Workspace,
    Path((company, agent)): Path<(Uuid, Uuid)>,
    Query(query): Query<HistoryQuery>,
) -> AppResult<impl IntoResponse> {
    let scope = scope(workspace.user_id, company, agent)?;
    let page = workspace
        .workflow
        .agent_authoring_reads()
        .revision_history(scope, query.cursor()?)
        .await?;
    fresh_json(&workspace, scope, page).await
}

#[derive(Deserialize)]
struct DirectoryQuery {
    after: Option<AgentKey>,
}
async fn directory_json(
    workspace: Workspace,
    Path(company): Path<Uuid>,
    Query(query): Query<DirectoryQuery>,
) -> AppResult<impl IntoResponse> {
    let scope = AgentCompanyScope {
        company: CompanyId::new(company),
        actor: WorkflowActor::authenticated(workspace.user_id)?,
    };
    Ok(Json(
        workspace
            .workflow
            .agent_authoring_reads()
            .agent_directory(scope, query.after)
            .await?,
    ))
}

pub(super) async fn catalog_form(
    workspace: Workspace,
    Path(agent): Path<Uuid>,
    Query(query): Query<CatalogQuery>,
) -> AppResult<Html<String>> {
    let scope = scope(workspace.user_id, query.company_id, agent)?;
    let reads = workspace.workflow.agent_authoring_reads();
    let connections = reads
        .tool_connections(scope, query.kind, query.after_connection)
        .await?;
    let tools = if matches!(query.kind, AgentToolKind::Http | AgentToolKind::Mcp)
        && query.connection.is_none()
    {
        AgentToolPage {
            items: Vec::new(),
            next: None,
        }
    } else {
        reads
            .tool_options(AgentToolPageRequest {
                scope,
                kind: query.kind,
                connection: query.connection,
                after: query.after_operation.clone(),
            })
            .await?
    };
    let html = pages::agent_tool_catalog(
        query.company_id,
        agent,
        query.kind,
        query.connection,
        &connections,
        &tools,
    );
    reads.authorize_authoring(scope).await?;
    Ok(Html(html))
}

#[derive(Deserialize)]
struct HistoryFormQuery {
    company_id: Uuid,
    #[serde(flatten)]
    history: HistoryQuery,
}
async fn history_form(
    workspace: Workspace,
    Path(agent): Path<Uuid>,
    Query(query): Query<HistoryFormQuery>,
) -> AppResult<Html<String>> {
    let scope = scope(workspace.user_id, query.company_id, agent)?;
    let reads = workspace.workflow.agent_authoring_reads();
    let page = reads
        .revision_history(scope, query.history.cursor()?)
        .await?;
    let html = pages::agent_revision_history(query.company_id, agent, &page);
    reads.authorize_authoring(scope).await?;
    Ok(Html(html))
}

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod tests;
