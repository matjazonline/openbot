//! Observe the actual guarded transport; only the declared post-acceptance receipt loss is injected.
use super::*;
use crate::services::mcp_client::{McpCallResult, McpClient, McpSession};
use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Clone, serde::Serialize)]
pub(super) struct CanvasId(pub(super) String);
#[derive(Clone, serde::Serialize)]
pub(super) struct FrameId(pub(super) String);

#[derive(Clone, Default)]
pub(super) struct CompanyEffects {
    pub(super) calls: Vec<crate::entities::mcp::McpToolName>,
    pub(super) canvases: Vec<CanvasId>,
    pub(super) frame: Option<FrameId>,
    pub(super) html: Option<String>,
    pub(super) accepted_edits: usize,
}
impl CompanyEffects {
    pub(super) fn count(&self, tool: &str) -> usize {
        self.calls
            .iter()
            .filter(|name| name.as_str() == tool)
            .count()
    }
}

#[derive(Default)]
struct Effects {
    companies: [CompanyEffects; 2],
    lose_next_create: Option<usize>,
}

pub(super) struct LiveClient {
    inner: Arc<HttpMcpClient>,
    fingerprints: [[u8; 32]; 2],
    effects: Arc<Mutex<Effects>>,
    sessions: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
}
impl LiveClient {
    pub(super) fn new(inner: Arc<HttpMcpClient>, input: &Input) -> Self {
        Self {
            inner,
            fingerprints: std::array::from_fn(|index| {
                Sha256::digest(input.companies[index].token.expose_secret().as_bytes()).into()
            }),
            effects: Arc::default(),
            sessions: Arc::default(),
            calls: Arc::default(),
        }
    }

    pub(super) fn company(&self, index: usize) -> CompanyEffects {
        self.effects.lock().unwrap().companies[index].clone()
    }

    pub(super) fn lose_next_create(&self, index: usize) {
        let mut effects = self.effects.lock().unwrap();
        assert!(effects.lose_next_create.is_none());
        assert_eq!(
            effects
                .companies
                .iter()
                .map(|company| company.canvases.len())
                .sum::<usize>(),
            2
        );
        effects.lose_next_create = Some(index);
    }
}

#[async_trait]
impl McpClient for LiveClient {
    async fn connect_prepared(
        &self,
        endpoint: &crate::entities::mcp::McpEndpoint,
        token: SecretString,
        work: crate::services::mcp_client::McpPreparation,
    ) -> crate::services::mcp_client::McpPreparationResult<Box<dyn McpSession>> {
        let company = self.company_for_token(&token);
        let inner = Box::pin(self.inner.connect_prepared(endpoint, token, work)).await?;
        Ok(self.observe(inner, company))
    }
    fn validate_endpoint(&self, endpoint: &crate::entities::mcp::McpEndpoint) -> AppResult<()> {
        self.inner.validate_endpoint(endpoint)
    }

    async fn connect(
        &self,
        endpoint: &crate::entities::mcp::McpEndpoint,
        token: Option<SecretString>,
    ) -> AppResult<Box<dyn McpSession>> {
        // Fingerprints select observations only; all credentials reach the real client unchanged.
        let company = token
            .as_ref()
            .and_then(|token| self.company_for_token(token));
        let inner = Box::pin(self.inner.connect(endpoint, token)).await?;
        Ok(self.observe(inner, company))
    }

    async fn shutdown(&self) -> AppResult<()> {
        Box::pin(self.inner.shutdown()).await?;
        assert_eq!(self.sessions.load(Ordering::SeqCst), 0);
        let effects = self.effects.lock().unwrap();
        assert!(
            effects.lose_next_create.is_none(),
            "the declared loss must actually occur"
        );
        assert_eq!(
            self.calls.load(Ordering::SeqCst),
            effects
                .companies
                .iter()
                .map(|company| company.calls.len())
                .sum::<usize>(),
            "all calls used the two separate current company keys"
        );
        Ok(())
    }
}

impl LiveClient {
    fn company_for_token(&self, token: &SecretString) -> Option<usize> {
        let fingerprint: [u8; 32] = Sha256::digest(token.expose_secret().as_bytes()).into();
        self.fingerprints
            .iter()
            .position(|known| *known == fingerprint)
    }

    fn observe(&self, inner: Box<dyn McpSession>, company: Option<usize>) -> Box<dyn McpSession> {
        self.sessions.fetch_add(1, Ordering::SeqCst);
        Box::new(LiveSession {
            inner,
            company,
            effects: self.effects.clone(),
            sessions: self.sessions.clone(),
            calls: self.calls.clone(),
        })
    }
}

struct LiveSession {
    inner: Box<dyn McpSession>,
    company: Option<usize>,
    effects: Arc<Mutex<Effects>>,
    sessions: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
}

fn identifier(value: &Value) -> String {
    let id = value.as_str().expect("real Doop receipt identifier");
    assert!(!id.is_empty() && id.len() <= 128);
    id.to_owned()
}

fn receipt(result: &McpCallResult) -> Value {
    if let Some(value) = &result.structured_content {
        return value.clone();
    }
    text_receipt(result)
}

fn text_receipt(result: &McpCallResult) -> Value {
    let text = result
        .content
        .iter()
        .find_map(|part| {
            if part["type"] == "text" {
                part["text"].as_str()
            } else {
                None
            }
        })
        .expect("real Doop JSON text receipt");
    serde_json::from_str(text).expect("real Doop JSON text receipt")
}

fn assert_edit_accepted(result: &McpCallResult) {
    // Doop's structuredContent is the canvas preview; the business acknowledgment
    // is the first JSON text block, followed by separate human-readable nudges.
    assert_eq!(
        text_receipt(result)["ok"],
        true,
        "actual accepted targeted edit receipt"
    );
}

impl LiveSession {
    fn observe(&self, name: &str, result: &McpCallResult) -> bool {
        let Some(index) = self.company else {
            return false;
        };
        if result.is_error {
            return false;
        }
        if name == "edit_frame_html" {
            assert_edit_accepted(result);
        }
        let mut effects = self.effects.lock().unwrap();
        let company = &mut effects.companies[index];
        match name {
            "create_canvas" => {
                assert!(company.canvases.len() < 3);
                company
                    .canvases
                    .push(CanvasId(identifier(&receipt(result)["id"])));
            }
            "create_frame" => {
                assert!(company.frame.is_none());
                company.frame = Some(FrameId(identifier(&receipt(result)["frame"]["id"])));
            }
            "get_frame" => {
                let value = receipt(result);
                assert_eq!(company.frame.as_ref().unwrap().0, identifier(&value["id"]));
                let html = value["html"].as_str().expect("actual persisted frame HTML");
                assert!(html.len() <= 65536, "bounded synthetic document specimen");
                company.html = Some(html.to_owned());
            }
            "edit_frame_html" => {
                company.accepted_edits += 1;
            }
            _ => {}
        }
        if name == "create_canvas" && effects.lose_next_create == Some(index) {
            effects.lose_next_create = None;
            return true;
        }
        false
    }
}

#[async_trait]
impl McpSession for LiveSession {
    async fn discover(&self) -> AppResult<Vec<crate::entities::mcp::McpDiscoveredTool>> {
        Box::pin(self.inner.discover()).await
    }

    async fn call(
        &self,
        name: &crate::entities::mcp::McpToolName,
        arguments: Value,
    ) -> AppResult<McpCallResult> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(index) = self.company {
            let mut effects = self.effects.lock().unwrap();
            let calls = &mut effects.companies[index].calls;
            assert!(calls.len() < 64, "bounded live workflow call inventory");
            calls.push(name.clone());
        }
        let result = Box::pin(self.inner.call(name, arguments)).await?;
        if self.observe(name.as_str(), &result) {
            return Err(AppError::ServiceUnavailable(
                "declared test loss after actual Doop acceptance".into(),
            ));
        }
        Ok(result)
    }

    async fn close(self: Box<Self>) -> AppResult<()> {
        let Self {
            inner, sessions, ..
        } = *self;
        let result = Box::pin(inner.close()).await;
        sessions.fetch_sub(1, Ordering::SeqCst);
        result
    }
}

#[test]
fn doop_e2e_edit_receipt_reads_business_text_with_separate_preview() {
    let result = McpCallResult {
        content: vec![
            json!({"type":"text","text":"{\"ok\":true}"}),
            json!({"type":"text","text":"Review this draft before publication."}),
        ],
        structured_content: Some(
            json!({"canvas":{"id":"actual-canvas"},"frame":{"id":"actual-frame"}}),
        ),
        is_error: false,
    };
    assert_edit_accepted(&result);
}

#[test]
#[should_panic(expected = "actual accepted targeted edit receipt")]
fn doop_e2e_preview_cannot_substitute_a_failed_business_ack() {
    let result = McpCallResult {
        content: vec![json!({"type":"text","text":"{\"ok\":false}"})],
        structured_content: Some(json!({"ok":true,"canvas":{"id":"actual-canvas"}})),
        is_error: false,
    };
    assert_edit_accepted(&result);
}
