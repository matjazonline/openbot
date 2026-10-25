//! Authenticated company-manager analytics; the store rechecks current authority.
use crate::{
    adapters::http::{app_state::AppState, auth::AuthenticatedUser, pages},
    app_error::{AppError, AppResult},
    entities::value_objects::{ModelName, ModelProvider},
    services::model_usage::*,
};
use axum::{
    Form, Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::header,
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ui/companies/{company}/usage", get(page))
        .route("/ui/companies/{company}/usage/prices", post(save_price))
        .route("/api/companies/{company}/usage", get(json_report))
        .layer(DefaultBodyLimit::max(4096))
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Filters {
    days: Option<u16>,
    group: Option<String>,
    #[serde(default, deserialize_with = "super::deserialize_empty_string_as_none")]
    agent: Option<Uuid>,
    #[serde(default, deserialize_with = "super::deserialize_empty_string_as_none")]
    workflow: Option<Uuid>,
}
impl Filters {
    fn query(self, scope: UsageScope) -> AppResult<UsageQuery> {
        let days = self.days.unwrap_or(30);
        if !(1..=90).contains(&days) {
            return Err(AppError::BadRequest("Days must be 1–90".into()));
        }
        let grouping = match self.group.as_deref().unwrap_or("model") {
            "model" => UsageGrouping::Model,
            "workflow" => UsageGrouping::Workflow,
            _ => {
                return Err(AppError::BadRequest(
                    "Group must be model or workflow".into(),
                ));
            }
        };
        let end = chrono::Utc::now();
        Ok(UsageQuery {
            scope,
            start: end - chrono::Duration::days(days as i64),
            end,
            grouping,
            agent: self.agent,
            workflow: self.workflow,
        })
    }
}
async fn page(
    State(store): State<Arc<dyn ModelUsageStore>>,
    user: AuthenticatedUser,
    Path(company): Path<Uuid>,
    Query(filters): Query<Filters>,
) -> AppResult<Response> {
    let query = filters.query(UsageScope {
        company,
        actor: user.id,
    })?;
    let report = store.usage_report(query.clone()).await?;
    Ok(private(
        Html(pages::model_usage_page(company, &query, &report)).into_response(),
    ))
}
async fn json_report(
    State(store): State<Arc<dyn ModelUsageStore>>,
    user: AuthenticatedUser,
    Path(company): Path<Uuid>,
    Query(filters): Query<Filters>,
) -> AppResult<Response> {
    let report = store
        .usage_report(filters.query(UsageScope {
            company,
            actor: user.id,
        })?)
        .await?;
    Ok(private(Json(report).into_response()))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PriceForm {
    provider: String,
    model: String,
    #[serde(default, deserialize_with = "super::deserialize_empty_string_as_none")]
    revision: Option<i64>,
    input: String,
    output: String,
    action: String,
}
async fn save_price(
    State(store): State<Arc<dyn ModelUsageStore>>,
    user: AuthenticatedUser,
    Path(company): Path<Uuid>,
    Form(form): Form<PriceForm>,
) -> Response {
    let result = async {
        let rates = match form.action.as_str() {
            "save" => Some(PriceRates {
                input: TokenPrice::parse(&form.input)?,
                output: TokenPrice::parse(&form.output)?,
            }),
            "remove" => None,
            _ => return Err(AppError::BadRequest("Invalid price action".into())),
        };
        store
            .save_model_price(PriceWrite {
                scope: UsageScope {
                    company,
                    actor: user.id,
                },
                provider: ModelProvider::canonical(form.provider),
                model: ModelName::canonical(form.model),
                expected_revision: form.revision,
                rates,
            })
            .await
    }
    .await;
    match result {
        Ok(()) => private(Redirect::to(&format!("/ui/companies/{company}/usage")).into_response()),
        Err(error) => {
            let status = error.clone().into_response().status();
            let feedback = match &error {
                AppError::BadRequest(message) | AppError::Conflict(message) => message.as_str(),
                _ => "Price could not be saved. Reload and try again.",
            };
            private((status, Html(pages::base_layout("Price not saved", &format!("{}<a class=\"btn\" href=\"/ui/companies/{company}/usage\">Return to usage and refresh prices</a>", pages::error_alert(feedback))))).into_response())
        }
    }
}
fn private(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("private, no-store"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
        middleware,
    };
    use tower::ServiceExt;
    struct Store {
        company: Uuid,
        actor: Uuid,
    }
    #[async_trait]
    impl ModelUsageStore for Store {
        async fn usage_report(&self, query: UsageQuery) -> AppResult<UsageReport> {
            if query.scope.company != self.company || query.scope.actor != self.actor {
                return Err(AppError::NotFound("Usage".into()));
            }
            Ok(UsageReport {
                start: query.start,
                end: query.end,
                grouping: query.grouping,
                groups: vec![UsageGroup {
                    provider: Some("<script>".into()),
                    model: Some("\"onfocus=\"evil".into()),
                    workflow: None,
                    calls: 1,
                    completed_calls: 0,
                    unconfirmed_calls: 1,
                    reported_calls: 0,
                    estimated_calls: 0,
                    mixed_calls: 0,
                    input_tokens: 100,
                    output_tokens: 10,
                    priced_subtotal_usd: None,
                    unpriced_calls: 1,
                }],
                scanned_calls: 1,
                truncated: false,
                prices: vec![ModelPrice {
                    provider: "openai".into(),
                    model: "\"onfocus=\"evil".into(),
                    input: TokenPrice::parse("1.25")?,
                    output: TokenPrice::parse("10")?,
                    revision: 1,
                }],
                prices_truncated: false,
            })
        }
        async fn save_model_price(&self, write: PriceWrite) -> AppResult<()> {
            write.validate()?;
            if write.scope.company != self.company || write.scope.actor != self.actor {
                return Err(AppError::NotFound("Usage".into()));
            }
            Err(AppError::Conflict("Price changed".into()))
        }
    }
    #[tokio::test]
    async fn model_usage_http_projection_escaping_foreign_ids_and_price_errors() {
        let company = Uuid::new_v4();
        let actor = Uuid::new_v4();
        let store: Arc<dyn ModelUsageStore> = Arc::new(Store { company, actor });
        let html = page(
            State(store.clone()),
            AuthenticatedUser { id: actor },
            Path(company),
            Query(Filters::default()),
        )
        .await
        .unwrap();
        assert_eq!(html.headers()[header::CACHE_CONTROL], "private, no-store");
        let body =
            String::from_utf8(to_bytes(html.into_body(), 100000).await.unwrap().to_vec()).unwrap();
        export_browser_fixture(&body);
        assert!(body.contains("&lt;script&gt;"));
        assert!(!body.contains("<script>"));
        assert!(body.contains("value=\"&quot;onfocus=&quot;evil\""));
        assert!(body.contains("data-submit=\"busy-once\""));
        assert!(body.contains("data-label"));
        assert!(
            page(
                State(store.clone()),
                AuthenticatedUser { id: actor },
                Path(Uuid::new_v4()),
                Query(Filters::default())
            )
            .await
            .is_err()
        );
        assert!(
            json_report(
                State(store.clone()),
                AuthenticatedUser { id: Uuid::new_v4() },
                Path(company),
                Query(Filters::default())
            )
            .await
            .is_err()
        );
        let json = json_report(
            State(store.clone()),
            AuthenticatedUser { id: actor },
            Path(company),
            Query(Filters::default()),
        )
        .await
        .unwrap();
        let json: serde_json::Value =
            serde_json::from_slice(&to_bytes(json.into_body(), 100000).await.unwrap()).unwrap();
        assert_eq!(json["groups"][0]["unconfirmed_calls"], 1);
        for (input, status) in [("-1", StatusCode::BAD_REQUEST), ("1", StatusCode::CONFLICT)] {
            let response = save_price(
                State(store.clone()),
                AuthenticatedUser { id: actor },
                Path(company),
                Form(PriceForm {
                    provider: "openai".into(),
                    model: "model".into(),
                    revision: None,
                    input: input.into(),
                    output: "0".into(),
                    action: "save".into(),
                }),
            )
            .await;
            assert_eq!(response.status(), status);
            assert_eq!(
                response.headers()[header::CACHE_CONTROL],
                "private, no-store"
            );
        }
    }
    // Opt-in artifact for browser verification of the actual renderer and shared scripts.
    fn export_browser_fixture(html: &str) {
        let Ok(directory) = std::env::var("MODEL_USAGE_BROWSER_FIXTURE_DIR") else {
            return;
        };
        let directory = std::path::Path::new(&directory);
        std::fs::create_dir_all(directory.join("assets")).unwrap();
        std::fs::write(directory.join("index.html"), html).unwrap();
        std::fs::write(
            directory.join("assets/app.js"),
            pages::application_javascript(),
        )
        .unwrap();
        std::fs::write(
            directory.join("assets/theme-init.js"),
            pages::theme_init_javascript(),
        )
        .unwrap();
        for name in [
            "app.css",
            "htmx-2.0.4.min.js",
            "htmx-ext-sse-2.2.3.js",
            "busybots-logo-dark-hor.png",
            "busybots-logo-light.png",
        ] {
            std::fs::copy(
                std::path::Path::new("assets").join(name),
                directory.join("assets").join(name),
            )
            .unwrap();
        }
    }
    #[tokio::test]
    async fn model_usage_cookie_price_mutation_requires_same_origin() {
        let app = Router::new()
            .route(
                "/ui/companies/{company}/usage/prices",
                post(|| async { StatusCode::NO_CONTENT }),
            )
            .layer(middleware::from_fn_with_state(
                super::super::ui::stream_tests::test_config(),
                crate::adapters::http::security::browser_security,
            ));
        let request = Request::post(format!("/ui/companies/{}/usage/prices", Uuid::new_v4()))
            .header(header::HOST, "localhost")
            .header(header::COOKIE, "session=signed")
            .header(header::ORIGIN, "https://foreign.example")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            app.oneshot(request).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }
    #[test]
    fn model_usage_http_query_bounds() {
        let scope = UsageScope {
            company: Uuid::new_v4(),
            actor: Uuid::new_v4(),
        };
        assert!(
            Filters {
                days: Some(91),
                ..Default::default()
            }
            .query(scope)
            .is_err()
        );
        assert!(
            Filters {
                group: Some("arbitrary".into()),
                ..Default::default()
            }
            .query(scope)
            .is_err()
        );
        let query = Filters::default().query(scope).unwrap();
        assert_eq!((query.end - query.start).num_days(), 30);
    }
}
