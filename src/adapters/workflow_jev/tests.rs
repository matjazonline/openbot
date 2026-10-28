use super::*;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};

pub(crate) fn inputs() -> Value {
    json!({"connection":"service","model":"jev-1.14.0","state":{"ticket":"Payment failed"},"questions":{
        "department":{"type":"choice","instructions":"Which team?","criteria":{"billing":"Payment issues","technical":"Bugs or integrations"}},
        "urgent":{"type":"noul","instructions":"Urgent?"},
        "severity":{"type":"score","instructions":"How severe?","criteria":["Cosmetic","Workaround available","Blocking"]}}})
}
pub(crate) fn output() -> Value {
    json!({"model":"jev-1.14.0","answers":{
        "department":{"type":"choice","choice":"billing","probabilities":{"billing":0.9,"technical":0.1},"confidence":0.8},
        "urgent":{"type":"noul","noul":0.7},
        "severity":{"type":"score","score":1.5,"probabilities":{"0":0.1,"1":0.3,"2":0.6},"legend":{"0":"Cosmetic","1":"Workaround available","2":"Blocking"},"confidence":0.7}},
        "usage":{"input_tokens":120,"output_tokens":12}})
}
fn credential() -> JevCredential {
    JevCredential {
        connection_id: uuid::Uuid::new_v4(),
        model: JevModel::parse("jev-1.14.0").unwrap(),
        bearer: secrecy::SecretString::from("fixture-secret"),
    }
}
pub(crate) async fn server(
    status: u16,
    headers: &str,
    body: Vec<u8>,
    delay: Duration,
) -> (JevHttp, JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1/systemone", listener.local_addr().unwrap());
    let headers = headers.to_owned();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        loop {
            let mut buf = [0; 4096];
            let n = stream.read(&mut buf).await.unwrap();
            if n == 0 {
                break;
            }
            request.extend_from_slice(&buf[..n]);
            if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&request[..end]);
                let length: usize = head
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|s| s.trim().parse().unwrap())
                    })
                    .unwrap_or(0);
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        tokio::time::sleep(delay).await;
        let length = if headers.contains("Transfer-Encoding") {
            String::new()
        } else {
            format!("Content-Length: {}\r\n", body.len())
        };
        let head =
            format!("HTTP/1.1 {status} Fixture\r\n{length}Connection: close\r\n{headers}\r\n");
        let _ = stream.write_all(head.as_bytes()).await;
        let _ = stream.write_all(&body).await;
        String::from_utf8(request).unwrap()
    });
    (JevHttp::loopback(&endpoint), task)
}
#[tokio::test]
async fn jev_http_mixed_types_exact_request_and_auth() {
    let (http, task) = server(
        200,
        "",
        serde_json::to_vec(&output()).unwrap(),
        Duration::ZERO,
    )
    .await;
    let request = JevRequest::prepare(&inputs()).unwrap();
    let result = http
        .evaluate(
            &request,
            &credential(),
            Instant::now() + Duration::from_secs(2),
        )
        .await
        .unwrap();
    assert_eq!(result.value, output());
    let wire = task.await.unwrap();
    assert!(wire.contains("authorization: Bearer fixture-secret"));
    assert!(!wire.to_lowercase().contains("typesafe-danger-zone-images"));
    assert_eq!(
        wire.split_once("\r\n\r\n").unwrap().1.as_bytes(),
        request.body()
    );
}
#[tokio::test]
async fn jev_http_errors_redacted_retry_after_and_classification() {
    for status in [302, 400, 401, 413, 415, 422, 429, 529, 503] {
        let (http, task) = server(
            status,
            "Retry-After: 7\r\n",
            b"fixture-secret state-text".to_vec(),
            Duration::ZERO,
        )
        .await;
        let result = http
            .evaluate(
                &JevRequest::prepare(&inputs()).unwrap(),
                &credential(),
                Instant::now() + Duration::from_secs(2),
            )
            .await;
        let Err(error) = result else { panic!() };
        assert_eq!(error, JevError::Http(status, Some(Duration::from_secs(7))));
        assert_eq!(error.retryable(), matches!(status, 429 | 529 | 503));
        assert!(!format!("{error:?}").contains("secret"));
        task.await.unwrap();
    }
}
#[tokio::test]
async fn jev_http_rejects_malformed_oversized_and_wrong_contract() {
    let mut invalids = vec![b"{".to_vec(), vec![b' '; MAX_RESPONSE_BYTES + 1]];
    for edit in 0..7 {
        let mut value = output();
        match edit {
            0 => value["model"] = json!("jev-1.13.0"),
            1 => {
                value["answers"].as_object_mut().unwrap().remove("urgent");
            }
            2 => value["answers"]["extra"] = json!({"type":"noul","noul":0.2}),
            3 => value["answers"]["urgent"]["noul"] = json!(1.1),
            4 => {
                value["answers"]["department"]["probabilities"]
                    .as_object_mut()
                    .unwrap()
                    .remove("technical");
            }
            5 => value["answers"]["severity"]["type"] = json!("noul"),
            _ => value["usage"]["input_tokens"] = json!(2147483648_u64),
        }
        invalids.push(serde_json::to_vec(&value).unwrap());
    }
    for body in invalids {
        let (http, task) = server(200, "", body, Duration::ZERO).await;
        assert!(matches!(
            http.evaluate(
                &JevRequest::prepare(&inputs()).unwrap(),
                &credential(),
                Instant::now() + Duration::from_secs(2)
            )
            .await,
            Err(JevError::Protocol)
        ));
        task.await.unwrap();
    }
}
#[tokio::test]
async fn jev_http_deadline_cancellation_and_capacity_release() {
    let (http, task) = server(
        200,
        "",
        serde_json::to_vec(&output()).unwrap(),
        Duration::from_millis(80),
    )
    .await;
    assert!(matches!(
        http.evaluate(
            &JevRequest::prepare(&inputs()).unwrap(),
            &credential(),
            Instant::now() + Duration::from_millis(20)
        )
        .await,
        Err(JevError::Timeout)
    ));
    assert_eq!(http.slots.available_permits(), 4);
    task.await.unwrap();
    let (http, task) = server(200, "", vec![], Duration::from_millis(80)).await;
    let request = JevRequest::prepare(&inputs()).unwrap();
    let key = credential();
    {
        let future = http.evaluate(&request, &key, Instant::now() + Duration::from_secs(2));
        tokio::pin!(future);
        tokio::select! { _=&mut future=>panic!(), _=tokio::time::sleep(Duration::from_millis(20))=>{} }
    }
    assert_eq!(http.slots.available_permits(), 4);
    task.await.unwrap();
}
#[test]
fn jev_http_request_limit_and_retry_after_date() {
    let mut value = inputs();
    value["state"] = json!("x".repeat(65536));
    assert!(JevRequest::prepare(&value).is_err());
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "retry-after",
        (chrono::Utc::now() + chrono::Duration::seconds(30))
            .to_rfc2822()
            .parse()
            .unwrap(),
    );
    assert!(retry_after(&headers).unwrap() <= Duration::from_secs(30));
}

#[tokio::test]
async fn jev_http_capacity_wait_is_inside_deadline() {
    let http = JevHttp::loopback("http://127.0.0.1:1/v1/systemone");
    assert!(matches!(
        http.acquire(Instant::now() - Duration::from_secs(1)).await,
        Err(JevError::Timeout)
    ));
    let permits = http.slots.acquire_many(4).await.unwrap();
    assert!(matches!(
        http.evaluate(
            &JevRequest::prepare(&inputs()).unwrap(),
            &credential(),
            Instant::now() + Duration::from_millis(20)
        )
        .await,
        Err(JevError::Timeout)
    ));
    drop(permits);
    assert_eq!(http.slots.available_permits(), 4);
}

#[tokio::test]
async fn jev_http_streamed_body_limit_without_content_length() {
    let mut body = format!("{:x}\r\n", MAX_RESPONSE_BYTES + 1).into_bytes();
    body.extend(vec![b' '; MAX_RESPONSE_BYTES + 1]);
    body.extend(b"\r\n0\r\n\r\n");
    let (http, task) = server(200, "Transfer-Encoding: chunked\r\n", body, Duration::ZERO).await;
    assert!(matches!(
        http.evaluate(
            &JevRequest::prepare(&inputs()).unwrap(),
            &credential(),
            Instant::now() + Duration::from_secs(2)
        )
        .await,
        Err(JevError::Protocol)
    ));
    task.await.unwrap();
}

#[tokio::test]
async fn jev_box_http_mixed_images_header_and_exact_digest() {
    use crate::application::workflow::jev::assets::tests as images;
    use sha2::{Digest, Sha256};
    let bytes = images::png(280, 280);
    let request = images::hydrated(&images::inputs(&bytes), bytes);
    let (http, task) = server(
        200,
        "",
        serde_json::to_vec(&images::output()).unwrap(),
        Duration::ZERO,
    )
    .await;
    let response = http
        .evaluate(
            &request,
            &credential(),
            Instant::now() + Duration::from_secs(2),
        )
        .await
        .unwrap();
    assert_eq!(response.value, images::output());
    let wire = task.await.unwrap();
    assert!(wire.contains("typesafe-danger-zone-images: true"));
    let body = wire.split_once("\r\n\r\n").unwrap().1.as_bytes();
    assert_eq!(body, request.body());
    assert_eq!(Sha256::digest(body).to_vec(), request.digest());
    assert!(!String::from_utf8_lossy(request.identity()).contains("data_base64"));
}
