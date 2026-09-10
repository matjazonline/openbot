//! Bounded, request-checked wire scenarios shared by both runtime adapters.
use super::MAX_REQUEST_BYTES;
use serde_json::Value;
use std::{collections::HashMap, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

const WAIT: Duration = Duration::from_secs(10);
const MAX_EXCHANGES: usize = 64;

/// No Debug: headers can contain synthetic authentication values.
pub struct ScriptedRequest {
    pub method: String,
    pub path: String,
    pub body: Value,
    headers: HashMap<String, String>,
}
impl ScriptedRequest {
    pub fn header_matches(&self, name: &str, expected: &str) -> bool {
        self.headers
            .get(&name.to_ascii_lowercase())
            .is_some_and(|value| value == expected)
    }
}

pub struct ScriptedResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub disconnect: bool,
    /// Signals arrival and waits for explicit release; both waits are bounded.
    pub barrier: Option<(oneshot::Sender<()>, oneshot::Receiver<()>)>,
}
impl ScriptedResponse {
    pub fn turn(turn: super::LlmTurn, index: usize) -> Self {
        Self::json_body(turn.into_body(index))
    }
    pub fn json_body(body: String) -> Self {
        Self {
            status: 200,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body,
            disconnect: false,
            barrier: None,
        }
    }
    pub fn json(body: Value) -> Self {
        Self::json_body(body.to_string())
    }
}

type RequestCheck = Box<dyn FnOnce(&ScriptedRequest) -> Result<(), &'static str> + Send>;
pub struct ScriptedExchange {
    check: RequestCheck,
    response: ScriptedResponse,
}
impl ScriptedExchange {
    pub fn new(
        check: impl FnOnce(&ScriptedRequest) -> Result<(), &'static str> + Send + 'static,
        response: ScriptedResponse,
    ) -> Self {
        Self {
            check: Box::new(check),
            response,
        }
    }
}

pub struct ScriptedLlm {
    pub base_url: String,
    pub requests: mpsc::Receiver<Value>,
    stop: Option<oneshot::Sender<()>>,
    worker: Option<JoinHandle<Result<usize, String>>>,
}
impl ScriptedLlm {
    pub fn observed(&mut self) -> Vec<Value> {
        let mut requests = Vec::new();
        while let Ok(request) = self.requests.try_recv() {
            requests.push(request);
        }
        requests
    }
    /// Call after the run settles. Unused, mismatched and extra exchanges are failures.
    pub async fn finish(mut self) -> Result<usize, String> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        // Keep ownership in self: cancelling finish must still abort the listener in Drop.
        let worker = self.worker.as_mut().expect("scenario worker");
        match tokio::time::timeout(WAIT, &mut *worker).await {
            Ok(result) => result.map_err(|_| "scenario worker failed".to_string())?,
            Err(_) => {
                worker.abort();
                let _ = worker.await;
                Err("scenario shutdown timed out".into())
            }
        }
    }
}
impl Drop for ScriptedLlm {
    fn drop(&mut self) {
        super::unregister_origin(&self.base_url);
        if let Some(worker) = &self.worker {
            worker.abort();
        }
        if let Some(registry) = super::SCRIPTED_AGENT_BASE_URLS.get() {
            registry
                .lock()
                .expect("scripted endpoint registry lock")
                .retain(|_, endpoint| endpoint != &self.base_url);
        }
    }
}

pub async fn scripted_scenario(exchanges: Vec<ScriptedExchange>) -> ScriptedLlm {
    assert!(exchanges.len() <= MAX_EXCHANGES);
    for exchange in &exchanges {
        assert!(exchange.response.body.len() <= MAX_REQUEST_BYTES);
        assert!(exchange.response.headers.len() <= 32);
        assert!(
            exchange
                .response
                .headers
                .iter()
                .map(|(name, value)| name.len() + value.len() + 4)
                .sum::<usize>()
                <= 16 * 1024
        );
    }
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("scripted listener");
    let base_url = format!("http://{}/", listener.local_addr().unwrap());
    super::register_origin(&base_url);
    let (sender, requests) = mpsc::channel(MAX_EXCHANGES);
    let (stop, stopped) = oneshot::channel();
    let worker = tokio::spawn(serve(listener, exchanges, sender, stopped));
    ScriptedLlm {
        base_url,
        requests,
        stop: Some(stop),
        worker: Some(worker),
    }
}

async fn serve(
    listener: TcpListener,
    exchanges: Vec<ScriptedExchange>,
    sender: mpsc::Sender<Value>,
    mut stopped: oneshot::Receiver<()>,
) -> Result<usize, String> {
    let expected = exchanges.len();
    let mut exchanges = exchanges.into_iter();
    let mut count = 0;
    let lifetime = tokio::time::sleep(Duration::from_secs(60));
    tokio::pin!(lifetime);
    loop {
        let accepted = tokio::select! {
            biased;
            accepted = listener.accept() => accepted.map_err(|_| "scenario accept failed")?,
            _ = &mut stopped => break,
            _ = &mut lifetime => return Err("scenario lifetime exceeded".into()),
        };
        let Some(exchange) = exchanges.next() else {
            return Err("unexpected extra request".into());
        };
        tokio::select! {
            biased;
            result = tokio::time::timeout(WAIT, exchange_one(accepted.0, exchange, &sender)) => {
                result.map_err(|_| "scenario exchange timed out")??;
            }
            _ = &mut stopped => return Err("scenario stopped during an exchange".into()),
            _ = &mut lifetime => return Err("scenario lifetime exceeded".into()),
        }
        count += 1;
    }
    if count != expected {
        return Err(format!(
            "unused required exchanges: completed {count} of {expected}"
        ));
    }
    Ok(count)
}

async fn exchange_one(
    mut stream: TcpStream,
    exchange: ScriptedExchange,
    sender: &mpsc::Sender<Value>,
) -> Result<(), String> {
    let request = read_request(&mut stream)
        .await
        .ok_or("invalid scripted request framing")?;
    (exchange.check)(&request).map_err(|reason| format!("request mismatch: {reason}"))?;
    sender
        .try_send(request.body)
        .map_err(|_| "scenario capture capacity exceeded")?;
    let mut response = exchange.response;
    if let Some((arrived, release)) = response.barrier.take() {
        let _ = arrived.send(());
        release.await.map_err(|_| "scenario barrier dropped")?;
    }
    if response.disconnect {
        return Ok(());
    }
    let mut headers = format!(
        "HTTP/1.1 {} Scripted\r\nContent-Length: {}\r\nConnection: close\r\n",
        response.status,
        response.body.len()
    );
    for (name, value) in response.headers {
        if name.contains(['\r', '\n']) || value.contains(['\r', '\n']) {
            return Err("invalid fixture header".into());
        }
        headers.push_str(&format!("{name}: {value}\r\n"));
    }
    stream
        .write_all(format!("{headers}\r\n{}", response.body).as_bytes())
        .await
        .map_err(|_| "scenario write failed".into())
}

async fn read_request(stream: &mut TcpStream) -> Option<ScriptedRequest> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    let header_end = loop {
        if let Some(position) = buffer.windows(4).position(|part| part == b"\r\n\r\n") {
            if position + 4 > 64 * 1024 {
                return None;
            }
            break position + 4;
        }
        if buffer.len() > 64 * 1024 {
            return None;
        }
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
    };
    let header_text = std::str::from_utf8(&buffer[..header_end]).ok()?;
    let mut lines = header_text.lines();
    let mut first = lines.next()?.split_whitespace();
    let method = first.next()?.to_string();
    let path = first.next()?.to_string();
    if first.next()? != "HTTP/1.1" || first.next().is_some() {
        return None;
    }
    let mut headers = HashMap::new();
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':')?;
        if headers
            .insert(name.to_ascii_lowercase(), value.trim().to_string())
            .is_some()
        {
            return None;
        }
    }
    if headers.contains_key("transfer-encoding") {
        return None;
    }
    let length: usize = headers
        .get("content-length")
        .map_or(Some(0), |value| value.parse().ok())?;
    if length > MAX_REQUEST_BYTES {
        return None;
    }
    while buffer.len() < header_end + length {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    let body = if length == 0 {
        Value::Null
    } else {
        serde_json::from_slice(&buffer[header_end..header_end + length]).ok()?
    };
    Some(ScriptedRequest {
        method,
        path,
        headers,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unused_and_mismatched_exchanges_fail_explicit_completion() {
        let unused = scripted_scenario(vec![ScriptedExchange::new(
            |_| Ok(()),
            ScriptedResponse::json(Value::Null),
        )])
        .await;
        assert!(
            unused
                .finish()
                .await
                .unwrap_err()
                .contains("unused required")
        );
        let mismatch = scripted_scenario(vec![ScriptedExchange::new(
            |_| Err("expected tool result"),
            ScriptedResponse::json(Value::Null),
        )])
        .await;
        let _ = reqwest::Client::new()
            .post(&mismatch.base_url)
            .json(&Value::Null)
            .send()
            .await;
        assert_eq!(
            mismatch.finish().await.unwrap_err(),
            "request mismatch: expected tool result"
        );
    }

    #[tokio::test]
    async fn a_listener_remains_active_to_detect_extra_requests() {
        let server = scripted_scenario(vec![]).await;
        let _ = reqwest::Client::new()
            .post(&server.base_url)
            .json(&Value::Null)
            .send()
            .await;
        assert_eq!(
            server.finish().await.unwrap_err(),
            "unexpected extra request"
        );
    }
}

#[cfg(test)]
#[path = "scenario_tests.rs"]
mod lifecycle_tests;
