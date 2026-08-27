//! A bucket that lives in memory, for the tests of everything that stores or serves a file.
//!
//! Shared rather than re-fabricated per test module, so "what does storage do when it fails" has
//! one answer, and a route test cannot accidentally assert against a fake that is kinder than the
//! real thing.

use std::sync::Mutex;

use async_trait::async_trait;

use crate::{
    adapters::storage::{BucketKind, FileStorage, StoredObject},
    app_error::{AppError, AppResult},
    entities::{
        upload::ImageUpload,
        value_objects::{ObjectKey, ObjectUrl},
    },
};

/// One object as the fake kept it.
#[derive(Debug, Clone, PartialEq)]
pub struct FakeObject {
    pub bucket: BucketKind,
    pub key: ObjectKey,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

/// A bucket that remembers what it was asked to store, and can be told to fail.
pub struct FakeStorage {
    /// What [`FileStorage::upload_image`] answers with.
    image_url: ObjectUrl,
    /// Whether every write and read fails, standing in for an unreachable bucket.
    broken: bool,
    stored: Mutex<Vec<FakeObject>>,
}

impl FakeStorage {
    pub fn new() -> Self {
        Self {
            image_url: ObjectUrl::new("https://cdn.example.test/stored.png"),
            broken: false,
            stored: Mutex::new(Vec::new()),
        }
    }

    /// A bucket whose image uploads land at `url`.
    pub fn returning(url: &str) -> Self {
        Self {
            image_url: ObjectUrl::new(url),
            ..Self::new()
        }
    }

    /// A bucket that refuses everything.
    pub fn failing() -> Self {
        Self {
            broken: true,
            ..Self::new()
        }
    }

    /// Everything stored so far, in the order it was written.
    pub fn objects(&self) -> Vec<FakeObject> {
        self.stored
            .lock()
            .expect("no test panics holding this")
            .clone()
    }

    fn refuse_if_broken(&self) -> AppResult<()> {
        if self.broken {
            return Err(AppError::Internal("bucket on fire".into()));
        }
        Ok(())
    }
}

impl Default for FakeStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl FileStorage for FakeStorage {
    async fn upload_image(&self, key: &ObjectKey, image: &ImageUpload) -> AppResult<ObjectUrl> {
        self.store_object(
            BucketKind::Public,
            key,
            image.format().mime(),
            image.bytes().to_vec(),
        )
        .await?;

        Ok(self.image_url.clone())
    }

    async fn store_object(
        &self,
        bucket: BucketKind,
        key: &ObjectKey,
        content_type: &str,
        bytes: Vec<u8>,
    ) -> AppResult<()> {
        self.refuse_if_broken()?;

        self.stored
            .lock()
            .expect("no test panics holding this")
            .push(FakeObject {
                bucket,
                key: key.clone(),
                content_type: content_type.to_string(),
                bytes,
            });

        Ok(())
    }

    async fn read_object(&self, bucket: BucketKind, key: &ObjectKey) -> AppResult<StoredObject> {
        self.refuse_if_broken()?;

        self.objects()
            .into_iter()
            .find(|object| object.bucket == bucket && &object.key == key)
            .map(|object| StoredObject {
                content_type: object.content_type,
                bytes: object.bytes,
            })
            .ok_or_else(|| AppError::NotFound(format!("No object at {key}")))
    }
}
