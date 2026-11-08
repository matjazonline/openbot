//! One-use proof of an actual parent-owned fixture listener; ordinary endpoint guards stay strict.
use super::{ScriptedRequest, TcpStream};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

pub(super) const PATH: &str = "/__openbot_parent_fixture";
#[derive(Clone, Serialize, Deserialize)]
struct ListenerOrigin(String);
#[derive(Clone, Serialize, Deserialize)]
pub struct InheritedScriptedListener {
    origin: ListenerOrigin,
    nonce: Uuid,
    owner: u32,
}
impl InheritedScriptedListener {
    pub(super) fn new(origin: String) -> Self {
        Self {
            origin: ListenerOrigin(origin),
            nonce: Uuid::new_v4(),
            owner: std::process::id(),
        }
    }
    fn proof(&self, challenge: Uuid, direction: &[u8]) -> String {
        let mut mac = Hmac::<sha2::Sha256>::new_from_slice(self.nonce.as_bytes()).unwrap();
        mac.update(b"openbot-scripted-listener-v1");
        mac.update(direction);
        mac.update(&self.owner.to_be_bytes());
        mac.update(challenge.as_bytes());
        format!("{:x}", mac.finalize().into_bytes())
    }
    /// Authenticate the live issuing listener before allowing any model credentials or request.
    pub async fn register_agent(&self, agent: Uuid) -> Result<(), &'static str> {
        let endpoint =
            url::Url::parse(&self.origin.0).map_err(|_| "invalid parent fixture origin")?;
        if endpoint.scheme() != "http"
            || endpoint.host_str() != Some("127.0.0.1")
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
        {
            return Err("parent fixture must be credential-free loopback HTTP");
        }
        let port = endpoint.port().ok_or("parent fixture port is required")?;
        tokio::time::timeout(Duration::from_secs(1), self.prove(port))
            .await
            .map_err(|_| "parent fixture proof timed out")??;
        super::super::register_origin(&self.origin.0);
        super::super::register_scripted_agent_base_url(agent, &self.origin.0);
        Ok(())
    }
    async fn prove(&self, port: u16) -> Result<(), &'static str> {
        let mut stream = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
            .await
            .map_err(|_| "parent fixture listener unavailable")?;
        let challenge = Uuid::new_v4();
        let body = json!({"challenge":challenge,"owner":self.owner,
            "proof":self.proof(challenge,b"request")})
        .to_string();
        let request = format!(
            "POST {PATH} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(request.as_bytes())
            .await
            .map_err(|_| "parent fixture proof write failed")?;
        let mut bytes = Vec::new();
        stream
            .take(4097)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| "parent fixture proof read failed")?;
        if bytes.len() > 4096 {
            return Err("parent fixture proof exceeded bound");
        }
        let response = std::str::from_utf8(&bytes).map_err(|_| "invalid parent fixture proof")?;
        let (headers, body) = response
            .split_once("\r\n\r\n")
            .ok_or("invalid parent fixture framing")?;
        let proof: Value =
            serde_json::from_str(body).map_err(|_| "invalid parent fixture proof body")?;
        if !headers.starts_with("HTTP/1.1 200 ")
            || proof
                != json!({"challenge":challenge,"owner":self.owner,
                "proof":self.proof(challenge,b"response")})
        {
            return Err("parent fixture proof refused");
        }
        Ok(())
    }
    pub(super) async fn respond(
        &self,
        request: ScriptedRequest,
        stream: &mut TcpStream,
    ) -> Result<(), String> {
        let challenge = request
            .body
            .get("challenge")
            .and_then(Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
            .ok_or_else(|| "untrusted inherited fixture request".to_string())?;
        if request.method != "POST"
            || request.body
                != json!({"challenge":challenge,"owner":self.owner,
                "proof":self.proof(challenge,b"request")})
        {
            return Err("untrusted inherited fixture request".into());
        }
        let body = json!({"challenge":challenge,"owner":self.owner,
            "proof":self.proof(challenge,b"response")})
        .to_string();
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .map_err(|_| "parent fixture proof response failed".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::services::test_support::{
        ScriptedExchange, ScriptedResponse, scripted_scenario,
    };

    #[tokio::test]
    async fn inherited_listener_proof_does_not_consume_or_capture_a_model_exchange() {
        let mut fixture = scripted_scenario(vec![ScriptedExchange::new(
            |request| {
                if request.path == "/model" {
                    Ok(())
                } else {
                    Err("model path")
                }
            },
            ScriptedResponse::json(json!({"accepted":true})),
        )])
        .await;
        let agent = Uuid::new_v4();
        fixture
            .inherited_listener()
            .register_agent(agent)
            .await
            .unwrap();
        let body: Value = reqwest::Client::new()
            .post(format!("{}model", fixture.base_url))
            .json(&json!({"fixture":"one model request"}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(body, json!({"accepted":true}));
        assert_eq!(
            fixture.observed(),
            vec![json!({"fixture":"one model request"})]
        );
        assert_eq!(fixture.finish().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn inherited_listener_forged_proof_registers_no_agent_and_fails_the_actual_owner() {
        let fixture = scripted_scenario(vec![]).await;
        let mut forged = fixture.inherited_listener();
        forged.nonce = Uuid::new_v4();
        let agent = Uuid::new_v4();
        assert!(forged.register_agent(agent).await.is_err());
        assert!(super::super::super::scripted_agent_base_url(agent).is_none());
        assert!(
            fixture
                .finish()
                .await
                .unwrap_err()
                .contains("untrusted inherited fixture")
        );
    }

    #[tokio::test]
    async fn inherited_listener_refuses_external_origin_before_connection_or_registration() {
        let reference = InheritedScriptedListener::new("https://example.invalid/".into());
        let agent = Uuid::new_v4();
        assert_eq!(
            reference.register_agent(agent).await.unwrap_err(),
            "parent fixture must be credential-free loopback HTTP"
        );
        assert!(super::super::super::scripted_agent_base_url(agent).is_none());
    }
    #[tokio::test]
    async fn inherited_listener_rejects_a_reflecting_listener_without_disclosing_its_secret() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let reference =
            InheritedScriptedListener::new(format!("http://{}/", listener.local_addr().unwrap()));
        let secret = reference.nonce.to_string();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = super::super::read_request(&mut stream).await.unwrap();
            let body = request.body.to_string();
            assert!(
                !body.contains(&secret),
                "capability secret must not be sent to an unproved listener"
            );
            stream.write_all(format!("HTTP/1.1 200 Echo\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        });
        let agent = Uuid::new_v4();
        let result = reference.register_agent(agent).await;
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.unwrap_err(), "parent fixture proof refused");
        assert!(super::super::super::scripted_agent_base_url(agent).is_none());
    }
    #[tokio::test]
    async fn inherited_listener_capability_is_one_use_and_rejects_a_second_registration() {
        let fixture = scripted_scenario(vec![]).await;
        let reference = fixture.inherited_listener();
        reference.register_agent(Uuid::new_v4()).await.unwrap();
        let second = Uuid::new_v4();
        assert!(reference.register_agent(second).await.is_err());
        assert!(super::super::super::scripted_agent_base_url(second).is_none());
        assert!(
            fixture
                .finish()
                .await
                .unwrap_err()
                .contains("already consumed")
        );
    }
}
