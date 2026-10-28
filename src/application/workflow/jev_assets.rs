//! Inert image references and application-owned private image read/authorization ports.
use crate::app_error::AppResult;
use crate::application::workflow::lease::ClaimedWorkflow;
use crate::application::workflow::{RelatedChannelId, RelatedThreadId};
use crate::domain::entities::value_objects::ObjectKey;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const MAX_ASSETS: usize = 4;
pub const MAX_IMAGE_BYTES: usize = 24 * 1024;
pub const MAX_IMAGE_SIDE: u32 = 1024;
pub const MAX_IMAGE_PIXELS: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AssetName(String);
impl AssetName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ImageDigest(String);
impl ImageDigest {
    pub fn of(bytes: &[u8]) -> Self {
        Self(format!("{:x}", Sha256::digest(bytes)))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JevAssetRef {
    #[serde(with = "channel_id")]
    pub channel_id: RelatedChannelId,
    #[serde(with = "thread_id")]
    pub thread_id: RelatedThreadId,
    pub sha256: ImageDigest,
}
mod channel_id {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        value: &RelatedChannelId,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        value.as_uuid().serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<RelatedChannelId, D::Error> {
        uuid::Uuid::deserialize(deserializer).map(RelatedChannelId::new)
    }
}
mod thread_id {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        value: &RelatedThreadId,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        value.as_uuid().serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<RelatedThreadId, D::Error> {
        uuid::Uuid::deserialize(deserializer).map(RelatedThreadId::new)
    }
}
pub type JevAssetRefs = BTreeMap<AssetName, JevAssetRef>;
pub fn parse_assets(value: Option<&Value>) -> Result<JevAssetRefs, &'static str> {
    let Some(value) = value else {
        return Ok(BTreeMap::new());
    };
    crate::domain::workflow::validate_context_value(
        value,
        crate::domain::workflow::ContextLimits {
            output_bytes: super::super::registry::jev::MAX_REQUEST_BYTES,
            ..Default::default()
        },
    )
    .map_err(|_| "Image references exceed limits")?;
    let map = value
        .as_object()
        .ok_or("Assets must be a literal map of private attachment references")?;
    if map.is_empty() || map.len() > MAX_ASSETS {
        return Err("Use between 1 and 4 image assets");
    }
    let refs: JevAssetRefs =
        serde_json::from_value(value.clone()).map_err(|_| "Invalid private image reference")?;
    for (name, asset) in &refs {
        let valid_name = !name.0.is_empty()
            && name.0.len() <= 64
            && name
                .0
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            && name.0.as_bytes()[0].is_ascii_alphabetic();
        if !valid_name
            || asset.channel_id.as_uuid().is_nil()
            || asset.thread_id.as_uuid().is_nil()
            || asset.sha256.0.len() != 64
            || !asset
                .sha256
                .0
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(
                "Use an asset identifier, nonnil channel/thread UUIDs and lowercase SHA256 digest",
            );
        }
    }
    Ok(refs)
}
/// Located only after current company/actor/thread authorization; never user-supplied keys.
#[derive(PartialEq, Eq)]
pub struct JevAssetLocation {
    pub reference: JevAssetRef,
    pub key: ObjectKey,
    pub content_type: String,
    pub size_bytes: usize,
}
#[async_trait]
pub trait JevAssetDirectory: Send + Sync {
    async fn locate_jev_assets(
        &self,
        claim: &ClaimedWorkflow,
    ) -> AppResult<BTreeMap<AssetName, JevAssetLocation>>;
}
#[async_trait]
pub trait JevAssets: Send + Sync {
    async fn read(
        &self,
        claim: &ClaimedWorkflow,
    ) -> Result<BTreeMap<AssetName, JevImage>, JevAssetError>;
}
/// Validated ephemeral bytes. Neither Debug nor Serialize can disclose this object.
pub struct JevImage {
    bytes: Vec<u8>,
    format: &'static str,
    pixels: u64,
}
impl JevImage {
    pub fn validate(
        reference: &JevAssetRef,
        mime: &str,
        bytes: Vec<u8>,
    ) -> Result<Self, JevAssetError> {
        use image::{ImageDecoder, ImageFormat, ImageReader};
        if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES {
            return Err(JevAssetError::TooLarge);
        }
        if ImageDigest::of(&bytes) != reference.sha256 {
            return Err(JevAssetError::Changed);
        }
        let format = image::guess_format(&bytes).map_err(|_| JevAssetError::Unsupported)?;
        let wire_format = match (format, mime) {
            (ImageFormat::Png, "image/png") => "png",
            (ImageFormat::Jpeg, "image/jpeg") => "jpeg",
            _ => return Err(JevAssetError::Unsupported),
        };
        if format == ImageFormat::Png
            && image::codecs::png::PngDecoder::with_limits(
                std::io::Cursor::new(&bytes),
                image_limits(),
            )
            .map_err(|_| JevAssetError::Unsupported)?
            .is_apng()
            .map_err(|_| JevAssetError::Unsupported)?
        {
            return Err(JevAssetError::Unsupported);
        }
        let mut reader = ImageReader::with_format(std::io::Cursor::new(&bytes), format);
        reader.limits(image_limits());
        let decoder = reader
            .into_decoder()
            .map_err(|_| JevAssetError::Unsupported)?;
        let (width, height) = decoder.dimensions();
        let pixels = u64::from(width) * u64::from(height);
        if !(280..=MAX_IMAGE_SIDE).contains(&width)
            || !(280..=MAX_IMAGE_SIDE).contains(&height)
            || pixels > MAX_IMAGE_PIXELS
            || decoder.total_bytes() > 8 * MAX_IMAGE_PIXELS
        {
            return Err(JevAssetError::Unsupported);
        }
        let mut decoded = vec![0; decoder.total_bytes() as usize];
        decoder
            .read_image(&mut decoded)
            .map_err(|_| JevAssetError::Unsupported)?;
        Ok(Self {
            bytes,
            format: wire_format,
            pixels,
        })
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn format(&self) -> &'static str {
        self.format
    }
    pub fn pixels(&self) -> u64 {
        self.pixels
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JevAssetError {
    Unauthorized,
    Missing,
    Changed,
    Unsupported,
    TooLarge,
    Storage,
}
impl JevAssetError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Unauthorized => "jev.asset_unauthorized",
            Self::Missing => "jev.asset_missing",
            Self::Changed => "jev.asset_changed",
            Self::Unsupported => "jev.asset_unsupported",
            Self::TooLarge => "jev.asset_too_large",
            Self::Storage => "jev.asset_storage",
        }
    }
    pub fn retryable(self) -> bool {
        self == Self::Storage
    }
}

fn image_limits() -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_IMAGE_SIDE);
    limits.max_alloc = Some(16 * 1024 * 1024);
    limits
}
#[cfg(test)]
#[path = "jev_assets_tests.rs"]
pub(crate) mod tests;
