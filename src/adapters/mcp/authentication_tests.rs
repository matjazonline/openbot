use super::*;
use crate::application::services::mcp_client::{
    McpPreparation, McpPreparationFailure, McpPreparationResult, McpRequestAuthority,
};

struct Allowed;
#[async_trait]
impl McpRequestAuthority for Allowed {
    async fn check(&self) -> McpPreparationResult<()> {
        Ok(())
    }
}

fn preparation(cancel: tokio_util::sync::CancellationToken) -> McpPreparation {
    McpPreparation::new(
        chrono::Utc::now() + chrono::Duration::seconds(10),
        cancel,
        Arc::new(Allowed),
    )
}

#[tokio::test]
async fn mcp_authentication_prepared_initialization_preserves_typed_failure_and_cleanup() {
    for status in [401, 403, 302, 503] {
        let mut response = ScriptedResponse::json_body("mcp-key remote-secret-body".into());
        response.status = status;
        response
            .headers
            .push(("Location".into(), "http://169.254.169.254/".into()));
        let server = scripted_scenario(vec![ScriptedExchange::new(
            |request| {
                if request.body["method"] == "initialize" {
                    Ok(())
                } else {
                    Err("unexpected request")
                }
            },
            response,
        )])
        .await;
        let adapter = adapter(&server.base_url);
        let work = preparation(Default::default());
        let result = adapter
            .connect_prepared(
                &server.base_url.clone().try_into().unwrap(),
                SecretString::from("mcp-key"),
                work.clone(),
            )
            .await;
        let Err(error) = result else {
            panic!("rejected initialization returned a session");
        };
        assert_eq!(
            error,
            if matches!(status, 401 | 403) {
                McpPreparationFailure::Authentication
            } else {
                McpPreparationFailure::Unavailable
            }
        );
        assert_eq!(work.failure(), Some(error));
        let diagnostic = format!("{error:?} {error}");
        for sensitive in ["mcp-key", "remote-secret-body", "169.254.169.254"] {
            assert!(!diagnostic.contains(sensitive));
        }
        adapter.shutdown().await.unwrap();
        assert!(adapter.workers.lock().await.is_empty());
        assert_eq!(adapter.capacity.available_permits(), 32);
        assert_eq!(server.finish().await, Ok(1));
    }
}

#[tokio::test]
async fn mcp_authentication_preparation_cancelled_before_admission_sends_no_credential() {
    let server = scripted_scenario(vec![]).await;
    let adapter = adapter(&server.base_url);
    let cancel = tokio_util::sync::CancellationToken::new();
    cancel.cancel();
    let work = preparation(cancel);
    let result = adapter
        .connect_prepared(
            &server.base_url.clone().try_into().unwrap(),
            SecretString::from("mcp-key"),
            work.clone(),
        )
        .await;
    assert!(matches!(result, Err(McpPreparationFailure::Cancelled)));
    assert_eq!(work.failure(), Some(McpPreparationFailure::Cancelled));
    adapter.shutdown().await.unwrap();
    assert!(adapter.workers.lock().await.is_empty());
    assert_eq!(server.finish().await, Ok(0));
}

#[test]
fn mcp_authentication_http_error_preserves_the_openbot_session() {
    use axum::response::IntoResponse;
    assert_eq!(
        crate::app_error::AppError::McpAuthentication
            .into_response()
            .status(),
        axum::http::StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[tokio::test]
async fn mcp_authentication_is_typed_through_actual_initialization_discovery_and_call() {
    for phase in 0..3 {
        for status in [401, 403, 302, 503] {
            let mut exchanges = if phase == 0 { vec![] } else { initialize() };
            let mut response = ScriptedResponse::json_body("mcp-key remote-secret-body".into());
            response.status = status;
            response
                .headers
                .push(("Location".into(), "http://169.254.169.254/".into()));
            let method = ["initialize", "tools/list", "tools/call"][phase];
            exchanges.push(ScriptedExchange::new(
                move |request| {
                    if request.body["method"] == method {
                        Ok(())
                    } else {
                        Err("unexpected authentication probe")
                    }
                },
                response,
            ));
            let expected_requests = exchanges.len();
            let server = scripted_scenario(exchanges).await;
            let adapter = adapter(&server.base_url);
            let result = adapter
                .connect(
                    &server.base_url.clone().try_into().unwrap(),
                    Some(SecretString::from("mcp-key")),
                )
                .await;
            let error = match (phase, result) {
                (0, Err(error)) => error,
                (1, Ok(session)) => {
                    let error = session.discover().await.unwrap_err();
                    session.close().await.unwrap();
                    error
                }
                (2, Ok(session)) => {
                    let error = session
                        .call(&"lookup".to_owned().try_into().unwrap(), json!({}))
                        .await
                        .unwrap_err();
                    session.close().await.unwrap();
                    error
                }
                _ => panic!("unexpected setup outcome at phase {phase}"),
            };
            assert_eq!(
                matches!(error, crate::app_error::AppError::McpAuthentication),
                matches!(status, 401 | 403),
                "phase {phase}, HTTP {status}: {error}"
            );
            let diagnostic = format!("{error:?} {error}");
            for sensitive in ["mcp-key", "remote-secret-body", "169.254.169.254"] {
                assert!(!diagnostic.contains(sensitive));
            }
            adapter.shutdown().await.unwrap();
            assert_eq!(server.finish().await, Ok(expected_requests));
        }
    }
}
