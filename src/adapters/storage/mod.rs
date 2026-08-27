//! Storing files somewhere a browser can fetch them.
//!
//! The port is here and the Google Cloud Storage implementation is in [`gcs`], so the pages and
//! routes that let someone pick a picture depend on "a place to put a file" rather than on a
//! specific bucket — and a deployment with no bucket configured simply has no implementation.

pub mod gcs;
#[cfg(test)]
pub mod test_support;

use async_trait::async_trait;

use crate::{
    app_error::AppResult,
    entities::{
        upload::ImageUpload,
        value_objects::{ObjectKey, ObjectUrl},
    },
};

/// Which of the two buckets an object belongs in.
///
/// An enum rather than a `public: bool`, because the two are not interchangeable and the cost of
/// confusing them runs one way: a mail attachment in the public bucket is a leak, while an avatar
/// in the private one is a broken picture. A call site has to name which it means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BucketKind {
    /// Readable by anyone with the URL. Avatars, and nothing that arrived in the mail.
    Public,
    /// Readable only by this app, which serves it on to whoever the channel rules allow.
    Private,
}

/// An object as it comes back out of storage.
pub struct StoredObject {
    pub content_type: String,
    pub bytes: Vec<u8>,
}

/// Somewhere uploaded files are kept.
#[async_trait]
pub trait FileStorage: Send + Sync {
    /// Store `image` in the public bucket, answering the URL it is served from.
    async fn upload_image(&self, key: &ObjectKey, image: &ImageUpload) -> AppResult<ObjectUrl>;

    /// Store bytes at `key`, in the bucket named.
    async fn store_object(
        &self,
        bucket: BucketKind,
        key: &ObjectKey,
        content_type: &str,
        bytes: Vec<u8>,
    ) -> AppResult<()>;

    /// Read back what [`FileStorage::store_object`] wrote.
    async fn read_object(&self, bucket: BucketKind, key: &ObjectKey) -> AppResult<StoredObject>;
}
