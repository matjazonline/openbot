//! Private storage composition for the application-owned Jev image port.
use crate::{
    adapters::storage::{BucketKind, FileStorage},
    app_error::AppError,
    application::workflow::{jev::*, lease::ClaimedWorkflow},
};
use async_trait::async_trait;
use std::{collections::BTreeMap, sync::Arc, time::Duration};

pub struct JevAssetReader<P> {
    directory: P,
    storage: Option<Arc<dyn FileStorage>>,
}
impl<P> JevAssetReader<P> {
    pub fn new(directory: P, storage: Option<Arc<dyn FileStorage>>) -> Self {
        Self { directory, storage }
    }
}
#[async_trait]
impl<P: JevAssetDirectory> JevAssets for JevAssetReader<P> {
    async fn read(
        &self,
        claim: &ClaimedWorkflow,
    ) -> Result<BTreeMap<AssetName, JevImage>, JevAssetError> {
        let locations = self
            .directory
            .locate_jev_assets(claim)
            .await
            .map_err(authority_error)?;
        let storage = self.storage.as_ref().ok_or(JevAssetError::Missing)?;
        let mut images = BTreeMap::new();
        for (name, location) in &locations {
            if location.size_bytes == 0 || location.size_bytes > MAX_IMAGE_BYTES {
                return Err(JevAssetError::TooLarge);
            }
            if !matches!(location.content_type.as_str(), "image/png" | "image/jpeg") {
                return Err(JevAssetError::Unsupported);
            }
            let object = tokio::time::timeout(
                Duration::from_secs(5),
                storage.read_object_bounded(BucketKind::Private, &location.key, MAX_IMAGE_BYTES),
            )
            .await
            .map_err(|_| JevAssetError::Storage)?
            .map_err(storage_error)?;
            if object.content_type != location.content_type {
                return Err(JevAssetError::Unsupported);
            }
            if object.bytes.len() != location.size_bytes {
                return Err(JevAssetError::Changed);
            }
            images.insert(
                name.clone(),
                JevImage::validate(&location.reference, &object.content_type, object.bytes)?,
            );
        }
        // A slow storage read cannot retain a grant revoked while it was in flight.
        let current = self
            .directory
            .locate_jev_assets(claim)
            .await
            .map_err(authority_error)?;
        if current != locations {
            return Err(JevAssetError::Changed);
        }
        Ok(images)
    }
}
fn authority_error(error: AppError) -> JevAssetError {
    match error {
        AppError::NotFound(_) | AppError::BadRequest(_) | AppError::InvalidCredentials => {
            JevAssetError::Unauthorized
        }
        _ => JevAssetError::Storage,
    }
}
fn storage_error(error: AppError) -> JevAssetError {
    match error {
        AppError::NotFound(_) => JevAssetError::Missing,
        AppError::BadRequest(_) => JevAssetError::TooLarge,
        _ => JevAssetError::Storage,
    }
}
