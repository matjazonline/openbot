//! Storing files somewhere a browser can fetch them.
//!
//! The port is here and the Google Cloud Storage implementation is in [`gcs`], so the pages and
//! routes that let someone pick a picture depend on "a place to put a file" rather than on a
//! specific bucket — and a deployment with no bucket configured simply has no implementation.

pub mod gcs;

use async_trait::async_trait;

use crate::{
    app_error::AppResult,
    entities::{
        upload::ImageUpload,
        value_objects::{ObjectKey, ObjectUrl},
    },
};

/// Somewhere uploaded files are kept and served from.
#[async_trait]
pub trait FileStorage: Send + Sync {
    /// Store `image` at `key`, answering the public URL it is served from.
    async fn upload_image(&self, key: &ObjectKey, image: &ImageUpload) -> AppResult<ObjectUrl>;
}
