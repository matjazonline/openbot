use super::*;
#[tokio::test]
async fn knowledge_upload_multipart_file_count_size_format_and_unknown_fields_are_bounded() {
    let router = parsing_router();
    let oversized = vec![b'a'; MAX_KNOWLEDGE_FILE_BYTES + 1];
    let valid = vec![b'a'; MAX_KNOWLEDGE_FILE_BYTES];
    let cases = vec![
        vec![File {
            name: "0_bad.md",
            bytes: &oversized,
        }],
        vec![File {
            name: "0_bad.md",
            bytes: &[0xff],
        }],
        vec![File {
            name: "0_bad.md",
            bytes: &[0],
        }],
        vec![File {
            name: "0_bad.pdf",
            bytes: b"text",
        }],
        vec![File {
            name: "0_valid.md",
            bytes: &valid,
        }],
        vec![],
    ];
    for (index, files) in cases.into_iter().enumerate() {
        let response = router
            .clone()
            .oneshot(request(
                Uuid::new_v4(),
                Method::POST,
                "/parse",
                multipart(&files),
                "multipart/form-data; boundary=knowledge-test",
            ))
            .await
            .unwrap();
        assert_eq!(
            response.status() == StatusCode::OK,
            index == 4,
            "case {index}"
        );
    }
    for count in [10, 11] {
        let files: Vec<_> = (0..count)
            .map(|_| File {
                name: "0_empty.txt",
                bytes: b"",
            })
            .collect();
        let response = router
            .clone()
            .oneshot(request(
                Uuid::new_v4(),
                Method::POST,
                "/parse",
                multipart(&files),
                "multipart/form-data; boundary=knowledge-test",
            ))
            .await
            .unwrap();
        assert_eq!(response.status() == StatusCode::OK, count == 10);
    }
    let injected=b"--knowledge-test\r\nContent-Disposition: form-data; name=\"level\"\r\n\r\n9\r\n--knowledge-test--\r\n".to_vec();
    let response = router
        .oneshot(request(
            Uuid::new_v4(),
            Method::POST,
            "/parse",
            injected,
            "multipart/form-data; boundary=knowledge-test",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

fn parsing_router() -> Router {
    use axum::extract::{FromRequest, Multipart};
    async fn parse(request: Request) -> Response {
        match upload::files(Multipart::from_request(request, &()).await.unwrap()).await {
            Ok(_) => StatusCode::OK.into_response(),
            Err(error) => error,
        }
    }
    Router::new()
        .route("/parse", post(parse))
        .layer(DefaultBodyLimit::max(upload::BODY_LIMIT))
}
