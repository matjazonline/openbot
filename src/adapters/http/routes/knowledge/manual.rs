use super::*;
use crate::application::knowledge::search::KnowledgeSearchQuery;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SearchDraft {
    query: String,
    exact_terms: Option<Vec<String>>,
    unit_id: Option<DepartmentId>,
}

impl SearchDraft {
    fn parsed(&self) -> AppResult<KnowledgeSearchQuery> {
        KnowledgeSearchQuery::parse(
            &json!({"query": self.query, "exact_terms": self.exact_terms.as_deref().unwrap_or(&[])}),
        )
    }
}

pub(super) async fn api_search(
    State(s): State<Services>,
    user: AuthenticatedUser,
    Path(company): Path<Uuid>,
    Json(draft): Json<SearchDraft>,
) -> AppResult<Json<Value>> {
    Ok(Json(
        s.library
            .search_manual(scope(&user, company), draft.unit_id, draft.parsed()?)
            .await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SearchForm {
    query: String,
}

pub(super) async fn ui_search(
    State(s): State<Services>,
    user: AuthenticatedUser,
    Query(query): Query<CompanyQuery>,
    axum_extra::extract::Form(draft): axum_extra::extract::Form<SearchForm>,
) -> AppResult<Response> {
    let search = SearchDraft {
        query: draft.query,
        exact_terms: None,
        unit_id: query.unit_id,
    };
    let result = async {
        s.library
            .search_manual(
                scope(&user, query.company_id),
                query.unit_id,
                search.parsed()?,
            )
            .await
    }
    .await;
    match result {
        Ok(result) => {
            render(
                &s,
                &user,
                query.company_id,
                query.unit_id,
                None,
                Some(pages::KnowledgeSearchView {
                    question: search.query,
                    result: Some(result),
                }),
            )
            .await
        }
        Err(error) => {
            let feedback = response_feedback(error.into_response()).await;
            render(
                &s,
                &user,
                query.company_id,
                query.unit_id,
                Some(feedback),
                Some(pages::KnowledgeSearchView {
                    question: search.query,
                    result: None,
                }),
            )
            .await
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MoveDraft {
    expected_revision: i64,
    target_unit_id: DepartmentId,
}

impl MoveDraft {
    fn command(self, scope: KnowledgeScope, id: Uuid) -> AppResult<MoveKnowledgeDocument> {
        let anchor = DeleteDraft {
            expected_revision: self.expected_revision,
        }
        .command(scope, id)?;
        Ok(MoveKnowledgeDocument {
            scope,
            document_id: id,
            expected_revision: anchor.expected_revision,
            target: self.target_unit_id,
        })
    }
}

pub(super) async fn api_move(
    State(s): State<Services>,
    user: AuthenticatedUser,
    Path(path): Path<LayerPath>,
    Json(draft): Json<MoveDraft>,
) -> Response {
    match async {
        s.library
            .documents()
            .move_document(draft.command(scope(&user, path.company), path.id)?)
            .await
    }
    .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => mutation_error(error),
    }
}

pub(super) async fn ui_move_form(
    State(s): State<Services>,
    user: AuthenticatedUser,
    Query(query): Query<MoveQuery>,
    Path(id): Path<Uuid>,
) -> AppResult<Response> {
    render_move(&s, &user, &query, id, None).await
}

pub(super) async fn ui_destinations(
    State(s): State<Services>,
    user: AuthenticatedUser,
    Query(query): Query<MoveQuery>,
    Path(id): Path<Uuid>,
) -> AppResult<Response> {
    render_destinations(&s, &user, &query, id).await
}

pub(super) async fn ui_move(
    State(s): State<Services>,
    user: AuthenticatedUser,
    Query(mut query): Query<MoveQuery>,
    Path(id): Path<Uuid>,
    axum_extra::extract::Form(draft): axum_extra::extract::Form<MoveDraft>,
) -> AppResult<Response> {
    let target = draft.target_unit_id;
    let result = async {
        s.library
            .documents()
            .move_document(draft.command(scope(&user, query.company_id), id)?)
            .await
    }
    .await;
    match result {
        Ok(()) => Ok(Redirect::to(&library_url(
            query.company_id,
            query.unit_id.map(|_| target),
        ))
        .into_response()),
        Err(error) => {
            let feedback = response_feedback(mutation_error(error)).await;
            query.target_unit_id = Some(target);
            match render_move(&s, &user, &query, id, Some(&feedback)).await {
                Err(AppError::NotFound(_)) => {
                    render(
                        &s,
                        &user,
                        query.company_id,
                        query.unit_id,
                        Some(feedback),
                        None,
                    )
                    .await
                }
                other => other,
            }
        }
    }
}
