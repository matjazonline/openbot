use crate::{
    app_error::{AppError, AppResult},
    application::local_agents::{
        bounds::{FRAME_VERSION, MAX_CHUNK_BYTES, MAX_FRAME_BYTES, REASSEMBLY_SECONDS},
        protocol::MAX_LOGICAL_BYTES,
    },
    domain::local_agents::PayloadDigest,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Chunk {
    version: u16,
    transfer_id: Uuid,
    index: usize,
    parts: usize,
    total_bytes: usize,
    digest: PayloadDigest,
    data: String,
}

fn invalid() -> AppError {
    AppError::BadRequest("invalid local-machine transport frame".into())
}
fn digest(bytes: &[u8]) -> AppResult<PayloadDigest> {
    PayloadDigest::try_from(format!("{:x}", Sha256::digest(bytes))).map_err(|_| invalid())
}

pub(super) fn encode<T: Serialize>(message: &T) -> AppResult<Vec<String>> {
    let bytes = serde_json::to_vec(message).map_err(|_| invalid())?;
    if bytes.is_empty() || bytes.len() > MAX_LOGICAL_BYTES {
        return Err(invalid());
    }
    let transfer_id = Uuid::new_v4();
    let digest = digest(&bytes)?;
    let mut frames = Vec::new();
    for (index, data) in bytes.chunks(MAX_CHUNK_BYTES).enumerate() {
        let chunk = Chunk {
            version: FRAME_VERSION,
            transfer_id,
            index,
            parts: bytes.len().div_ceil(MAX_CHUNK_BYTES),
            total_bytes: bytes.len(),
            digest: digest.clone(),
            data: STANDARD.encode(data),
        };
        let frame = serde_json::to_string(&chunk).map_err(|_| invalid())?;
        if frame.len() > MAX_FRAME_BYTES {
            return Err(invalid());
        }
        frames.push(frame);
    }
    Ok(frames)
}

struct Pending {
    transfer_id: Uuid,
    next: usize,
    parts: usize,
    total_bytes: usize,
    digest: PayloadDigest,
    bytes: Vec<u8>,
    deadline: Instant,
}

pub(super) struct Assembler {
    pending: Option<Pending>,
    limit: usize,
}
impl Default for Assembler {
    fn default() -> Self {
        Self {
            pending: None,
            limit: MAX_LOGICAL_BYTES,
        }
    }
}
impl Assembler {
    pub(super) fn handshake() -> Self {
        Self {
            pending: None,
            limit: 4096,
        }
    }
    pub(super) fn deadline(&self) -> Option<Instant> {
        self.pending.as_ref().map(|p| p.deadline)
    }
    pub(super) fn push(&mut self, frame: &str) -> AppResult<Option<Vec<u8>>> {
        if frame.len() > MAX_FRAME_BYTES {
            return Err(invalid());
        }
        let chunk: Chunk = serde_json::from_str(frame).map_err(|_| invalid())?;
        if chunk.version != FRAME_VERSION
            || chunk.transfer_id.is_nil()
            || chunk.total_bytes == 0
            || chunk.total_bytes > self.limit
            || chunk.parts != chunk.total_bytes.div_ceil(MAX_CHUNK_BYTES)
            || chunk.index >= chunk.parts
            || chunk.data.len() > MAX_CHUNK_BYTES.div_ceil(3) * 4
        {
            return Err(invalid());
        }
        let bytes = STANDARD.decode(&chunk.data).map_err(|_| invalid())?;
        let expected = if chunk.index + 1 == chunk.parts {
            chunk.total_bytes - chunk.index * MAX_CHUNK_BYTES
        } else {
            MAX_CHUNK_BYTES
        };
        if bytes.len() != expected {
            return Err(invalid());
        }
        if self.pending.is_none() {
            if chunk.index != 0 {
                return Err(invalid());
            }
            self.pending = Some(Pending {
                transfer_id: chunk.transfer_id,
                next: 0,
                parts: chunk.parts,
                total_bytes: chunk.total_bytes,
                digest: chunk.digest.clone(),
                bytes: Vec::with_capacity(chunk.total_bytes),
                deadline: Instant::now() + Duration::from_secs(REASSEMBLY_SECONDS),
            });
        }
        let pending = self.pending.as_mut().ok_or_else(invalid)?;
        if Instant::now() >= pending.deadline
            || pending.transfer_id != chunk.transfer_id
            || pending.next != chunk.index
            || pending.parts != chunk.parts
            || pending.total_bytes != chunk.total_bytes
            || pending.digest != chunk.digest
        {
            return Err(invalid());
        }
        pending.bytes.extend_from_slice(&bytes);
        pending.next += 1;
        if pending.next < pending.parts {
            return Ok(None);
        }
        let pending = self.pending.take().ok_or_else(invalid)?;
        if pending.bytes.len() != pending.total_bytes || digest(&pending.bytes)? != pending.digest {
            return Err(invalid());
        }
        Ok(Some(pending.bytes))
    }
}

#[cfg(test)]
#[path = "framing_tests.rs"]
mod tests;
