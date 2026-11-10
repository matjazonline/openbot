//! Versioned projections for conformance tests and a future service boundary.
//! Decoding data never authenticates an owner or constructs runtime authority.
mod command;
#[cfg(test)]
pub(crate) mod control_transport;
mod reply;
#[cfg(test)]
pub(crate) mod test_transport;
mod text;
use crate::domain::integrations::*;
use chrono::{DateTime, Utc};
pub use command::*;
pub use reply::*;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
pub use text::*;

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_FRAME_BYTES: usize = 128 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u16,
    pub identity: DispatchId,
    pub owner: AccountOwner,
    pub deadline: DateTime<Utc>,
    pub cancellation: CancellationId,
    pub command: Command,
}
impl Request {
    pub fn encode(&self) -> IntegrationResult<Vec<u8>> {
        self.validate()?;
        encode(self)
    }
    pub fn decode(bytes: &[u8]) -> IntegrationResult<Self> {
        let request: Self = decode(bytes)?;
        request.validate()?;
        Ok(request)
    }
    fn validate(&self) -> IntegrationResult<()> {
        version(self.version)?;
        self.command.validate()?;
        if let Command::DiscoverMcp { authorization, .. } = &self.command
            && authorization.client != self.owner.namespace
        {
            return Err(IntegrationError::Denied);
        }
        if let Some(invocation) = self.command.invocation()
            && (invocation.dispatch != self.identity
                || invocation.deadline != self.deadline
                || invocation.cancellation != self.cancellation
                || invocation.authorization.client != self.owner.namespace)
        {
            return Err(IntegrationError::Denied);
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub version: u16,
    pub identity: DispatchId,
    pub reply: Reply,
}
impl Response {
    /// A failure here is transport failure, never evidence that business dispatch did not occur.
    pub fn encode(&self) -> IntegrationResult<Vec<u8>> {
        version(self.version)?;
        self.reply.validate()?;
        encode(self)
    }
    pub fn decode(bytes: &[u8], expected: DispatchId) -> IntegrationResult<Self> {
        let response: Self = decode(bytes)?;
        version(response.version)?;
        if response.identity != expected {
            return Err(IntegrationError::Denied);
        }
        response.reply.validate()?;
        Ok(response)
    }
}
fn version(version: u16) -> IntegrationResult<()> {
    if version != PROTOCOL_VERSION {
        return Err(IntegrationError::Invalid);
    }
    Ok(())
}
fn encode(value: &impl Serialize) -> IntegrationResult<Vec<u8>> {
    let mut writer = FrameWriter {
        bytes: Vec::new(),
        overflow: false,
    };
    let result = serde_json::to_writer(&mut writer, value);
    if writer.overflow {
        return Err(IntegrationError::Limit);
    }
    result.map_err(|_| IntegrationError::Invalid)?;
    Ok(writer.bytes)
}
struct FrameWriter {
    bytes: Vec<u8>,
    overflow: bool,
}
impl std::io::Write for FrameWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_FRAME_BYTES - self.bytes.len() {
            self.overflow = true;
            return Err(std::io::Error::other("Integration frame exceeds limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn decode<T: DeserializeOwned>(bytes: &[u8]) -> IntegrationResult<T> {
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(IntegrationError::Limit);
    }
    // Check version before interpreting any command/result variant. serde retains its
    // recursion limit; there is no compatibility fallback or unbounded stream parser.
    #[derive(Deserialize)]
    struct Header {
        version: u16,
    }
    let header: Header = serde_json::from_slice(bytes).map_err(|_| IntegrationError::Invalid)?;
    version(header.version)?;
    // Parse original bytes again so duplicate fields are rejected rather than erased by Value.
    serde_json::from_slice(bytes).map_err(|_| IntegrationError::Invalid)
}
