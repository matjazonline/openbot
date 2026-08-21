//! Google Cloud Storage, reached with a service account key held in the environment.
//!
//! The key is a JSON file, and a JSON file does not survive an environment variable intact — so it
//! is carried base64-encoded (`GCS_SERVICE_ACCOUNT_JSON_BASE64`) and decoded once here, at
//! startup. A malformed or unreadable key fails the boot rather than the first upload: a
//! deployment that cannot store files should say so before it starts serving pages that offer to.
//!
//! Authentication is the two-legged OAuth flow Google documents for service accounts: sign a short
//! assertion with the account's RSA key, exchange it for an access token, and reuse that token
//! until it is nearly expired.

use std::time::Duration;

use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use chrono::{DateTime, Utc};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, instrument};

use crate::{
    adapters::storage::{BucketKind, FileStorage, StoredObject},
    app_error::{AppError, AppResult},
    entities::{
        upload::ImageUpload,
        value_objects::{ObjectKey, ObjectUrl},
    },
    infra::config::GcsConfig,
};

/// The narrowest scope that can write an object; the app never lists or administers a bucket.
const SCOPE: &str = "https://www.googleapis.com/auth/devstorage.read_write";

/// Google's own ceiling on a service-account assertion.
const ASSERTION_TTL_SECS: i64 = 3600;

/// How early a token is treated as expired, so an upload is never started with one that dies
/// in flight.
const EXPIRY_SKEW_SECS: i64 = 60;

/// An upload is a foreground request — someone is watching a spinner — so it fails rather than
/// hangs when the API does not answer.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Uploaded objects are named by a fresh UUID, so a given URL's bytes never change.
const OBJECT_CACHE_CONTROL: &str = "public, max-age=31536000, immutable";

/// The parts of a service account key file this adapter uses.
///
/// `token_uri` is read from the file rather than hardcoded because that is where Google says the
/// tokens for *this* key are bought, and it differs for non-public deployments.
#[derive(Debug, Deserialize)]
struct ServiceAccountKey {
    client_email: String,
    private_key: String,
    private_key_id: Option<String>,
    #[serde(default = "default_token_uri")]
    token_uri: String,
}

fn default_token_uri() -> String {
    "https://oauth2.googleapis.com/token".to_string()
}

/// The assertion exchanged for an access token.
#[derive(Debug, Serialize)]
struct Assertion<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    exp: i64,
    iat: i64,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: i64,
}

/// A token and the moment it stops being usable.
#[derive(Debug, Clone)]
struct AccessToken {
    value: String,
    usable_until: DateTime<Utc>,
}

impl AccessToken {
    fn is_usable(&self) -> bool {
        Utc::now() < self.usable_until
    }
}

pub struct GcsFileStorage {
    bucket: String,
    /// The bucket nothing public may read; `None` when the deployment stores no attachments.
    private_bucket: Option<String>,
    /// What an object's URL is built from — the bucket's own public host, or a CDN in front of it.
    public_base_url: String,
    client_email: String,
    token_uri: String,
    signing_key: EncodingKey,
    header: Header,
    http: Client,
    /// The current access token, minted on the first upload and reused until it nearly expires.
    token: RwLock<Option<AccessToken>>,
}

impl GcsFileStorage {
    /// Build the adapter from deploy configuration, decoding and parsing the key as we go.
    pub fn from_config(config: &GcsConfig) -> anyhow::Result<Self> {
        let key = decode_service_account_key(&config.service_account_json_base64)?;

        let signing_key = EncodingKey::from_rsa_pem(key.private_key.as_bytes()).map_err(|err| {
            anyhow::anyhow!("GCS_SERVICE_ACCOUNT_JSON_BASE64 has an unusable private_key: {err}")
        })?;

        let mut header = Header::new(Algorithm::RS256);
        header.kid = key.private_key_id.clone();

        let http = Client::builder().timeout(REQUEST_TIMEOUT).build()?;

        Ok(Self {
            public_base_url: config.public_base_url(),
            bucket: config.bucket.clone(),
            private_bucket: config.attachments_bucket.clone(),
            client_email: key.client_email,
            token_uri: key.token_uri,
            signing_key,
            header,
            http,
            token: RwLock::new(None),
        })
    }

    /// A usable access token, minted only when the one we hold is gone or nearly expired.
    async fn access_token(&self) -> AppResult<String> {
        if let Some(token) = self.token.read().await.as_ref()
            && token.is_usable()
        {
            return Ok(token.value.clone());
        }

        let mut cached = self.token.write().await;

        // Another request may have minted one while this one waited for the write lock.
        if let Some(token) = cached.as_ref()
            && token.is_usable()
        {
            return Ok(token.value.clone());
        }

        let minted = self.mint_access_token().await?;
        let value = minted.value.clone();
        *cached = Some(minted);

        Ok(value)
    }

    /// Sign an assertion for this service account and exchange it for an access token.
    async fn mint_access_token(&self) -> AppResult<AccessToken> {
        let issued_at = Utc::now();
        let assertion = Assertion {
            iss: &self.client_email,
            scope: SCOPE,
            aud: &self.token_uri,
            iat: issued_at.timestamp(),
            exp: issued_at.timestamp() + ASSERTION_TTL_SECS,
        };

        let signed =
            jsonwebtoken::encode(&self.header, &assertion, &self.signing_key).map_err(|err| {
                AppError::Internal(format!("Failed to sign the GCS assertion: {err}"))
            })?;

        let response = self
            .http
            .post(&self.token_uri)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", &signed),
            ])
            .send()
            .await
            .map_err(|err| AppError::Internal(format!("GCS token request failed: {err}")))?;

        let response = require_success(response, "GCS token request").await?;

        let token: TokenResponse = response
            .json()
            .await
            .map_err(|err| AppError::Internal(format!("Unreadable GCS token response: {err}")))?;

        debug!(expires_in = token.expires_in, "Minted a GCS access token");

        Ok(AccessToken {
            value: token.access_token,
            usable_until: issued_at
                + chrono::Duration::seconds((token.expires_in - EXPIRY_SKEW_SECS).max(0)),
        })
    }
}

impl GcsFileStorage {
    /// Which bucket a [`BucketKind`] names on this deployment.
    ///
    /// A private bucket that was never configured is an error rather than a fallback to the public
    /// one: storing an attachment where anyone could read it is the one outcome worth refusing.
    fn bucket_for(&self, kind: BucketKind) -> AppResult<&str> {
        match kind {
            BucketKind::Public => Ok(&self.bucket),
            BucketKind::Private => self.private_bucket.as_deref().ok_or_else(|| {
                AppError::Internal(
                    "No private bucket is configured (GCS_ATTACHMENTS_BUCKET), so there is nowhere \
                     to keep this that is not public"
                        .to_string(),
                )
            }),
        }
    }

    /// One media upload, whichever bucket it is going to.
    async fn upload(
        &self,
        bucket: &str,
        key: &ObjectKey,
        content_type: &str,
        bytes: Vec<u8>,
    ) -> AppResult<()> {
        let token = self.access_token().await?;

        let response = self
            .http
            .post(format!(
                "https://storage.googleapis.com/upload/storage/v1/b/{bucket}/o"
            ))
            // `query` percent-encodes the object name, which is the only part of this request that
            // is not a constant.
            .query(&[("uploadType", "media"), ("name", key.as_str())])
            .bearer_auth(token)
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .header(reqwest::header::CACHE_CONTROL, OBJECT_CACHE_CONTROL)
            .body(bytes)
            .send()
            .await
            .map_err(|err| AppError::Internal(format!("GCS upload failed: {err}")))?;

        require_success(response, "GCS upload").await?;

        Ok(())
    }
}

#[async_trait]
impl FileStorage for GcsFileStorage {
    #[instrument(skip(self, image), fields(bucket = %self.bucket, key = %key))]
    async fn upload_image(&self, key: &ObjectKey, image: &ImageUpload) -> AppResult<ObjectUrl> {
        self.upload(
            &self.bucket,
            key,
            image.format().mime(),
            image.bytes().to_vec(),
        )
        .await?;

        Ok(object_url(&self.public_base_url, key))
    }

    #[instrument(skip(self, bytes), fields(bytes = bytes.len(), key = %key))]
    async fn store_object(
        &self,
        bucket: BucketKind,
        key: &ObjectKey,
        content_type: &str,
        bytes: Vec<u8>,
    ) -> AppResult<()> {
        self.upload(self.bucket_for(bucket)?, key, content_type, bytes)
            .await
    }

    #[instrument(skip(self), fields(key = %key))]
    async fn read_object(&self, bucket: BucketKind, key: &ObjectKey) -> AppResult<StoredObject> {
        let token = self.access_token().await?;
        let bucket = self.bucket_for(bucket)?;

        let response = self
            .http
            .get(format!(
                "https://storage.googleapis.com/storage/v1/b/{bucket}/o/{key}",
                key = urlencoding(key.as_str()),
            ))
            .query(&[("alt", "media")])
            .bearer_auth(token)
            .send()
            .await
            .map_err(|err| AppError::Internal(format!("GCS read failed: {err}")))?;

        let response = require_success(response, "GCS read").await?;

        // What the object was stored as, which is what the download route serves it as.
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or(DEFAULT_CONTENT_TYPE)
            .to_string();

        let bytes = response
            .bytes()
            .await
            .map_err(|err| AppError::Internal(format!("GCS read was cut short: {err}")))?
            .to_vec();

        Ok(StoredObject {
            content_type,
            bytes,
        })
    }
}

/// What an object with no recorded type is served as: bytes to save, never something to run.
const DEFAULT_CONTENT_TYPE: &str = "application/octet-stream";

/// An object name as a *path segment* of the JSON API's read URL.
///
/// The slashes in a key are part of the name, not path structure, so they have to be encoded --
/// `attachments/a.png` is one object, not a folder and a file, and Google answers 404 for the
/// unencoded form.
fn urlencoding(key: &str) -> String {
    key.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// The public URL an object is served from.
///
/// A free function rather than a method so it can be tested without a signing key: the base URL is
/// the one piece of upload configuration a deployment gets to change, and getting it wrong is what
/// puts a broken `<img src>` on every page.
fn object_url(public_base_url: &str, key: &ObjectKey) -> ObjectUrl {
    ObjectUrl::new(format!(
        "{base}/{key}",
        base = public_base_url.trim_end_matches('/'),
    ))
}

/// The service account key, out of the base64 the environment carries it in.
fn decode_service_account_key(encoded: &str) -> anyhow::Result<ServiceAccountKey> {
    // Trimmed because a key pasted into a secret store arrives with whatever whitespace the
    // terminal that produced it added.
    let decoded = BASE64.decode(encoded.trim().as_bytes()).map_err(|err| {
        anyhow::anyhow!("GCS_SERVICE_ACCOUNT_JSON_BASE64 is not valid base64: {err}")
    })?;

    serde_json::from_slice(&decoded).map_err(|err| {
        anyhow::anyhow!("GCS_SERVICE_ACCOUNT_JSON_BASE64 is not a service account key: {err}")
    })
}

/// Turn a non-2xx answer into an error carrying what Google said about it.
///
/// Google's failures are described in the *body*, not the status line, so a bare status code
/// ("403") would leave a misconfigured bucket indistinguishable from a key without permission.
async fn require_success(response: reqwest::Response, what: &str) -> AppResult<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    let detail = response.text().await.unwrap_or_default();
    let detail = detail.trim();

    Err(AppError::Internal(if detail.is_empty() {
        format!("{what} was refused with {status}")
    } else {
        format!("{what} was refused with {status}: {detail}")
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_base64_key_is_decoded_into_its_fields() {
        let json = r#"{
            "type": "service_account",
            "client_email": "uploader@example.iam.gserviceaccount.com",
            "private_key_id": "abc123",
            "private_key": "-----BEGIN PRIVATE KEY-----\nnot-a-real-key\n-----END PRIVATE KEY-----\n"
        }"#;

        let key = decode_service_account_key(&BASE64.encode(json)).expect("a well-formed key");

        assert_eq!(key.client_email, "uploader@example.iam.gserviceaccount.com");
        assert_eq!(key.private_key_id.as_deref(), Some("abc123"));
        // Absent from the file, so it falls back to Google's public token endpoint.
        assert_eq!(key.token_uri, "https://oauth2.googleapis.com/token");
        // The escaped newlines have to survive as real ones, or the PEM will not parse.
        assert!(key.private_key.contains("-----BEGIN PRIVATE KEY-----\n"));
    }

    #[test]
    fn whitespace_around_the_encoded_key_is_tolerated() {
        let json = r#"{"client_email":"a@b.com","private_key":"x"}"#;
        let padded = format!("\n  {}  \n", BASE64.encode(json));

        assert!(decode_service_account_key(&padded).is_ok());
    }

    #[test]
    fn an_object_url_joins_the_base_and_the_key_exactly_once() {
        let key = ObjectKey::new("avatars/abc.png");

        assert_eq!(
            object_url("https://storage.googleapis.com/pics", &key),
            ObjectUrl::new("https://storage.googleapis.com/pics/avatars/abc.png")
        );
        // A base URL copied out of a console keeps its trailing slash; the URL must not double up.
        assert_eq!(
            object_url("https://cdn.example.com/", &key),
            ObjectUrl::new("https://cdn.example.com/avatars/abc.png")
        );
    }

    #[test]
    fn an_object_name_is_encoded_whole_for_the_read_url() {
        // The slash is part of the name, so it must not read as path structure.
        assert_eq!(
            urlencoding("attachments/9f2c.png"),
            "attachments%2F9f2c.png"
        );
        assert_eq!(urlencoding("a b"), "a%20b");
        assert_eq!(urlencoding("plain-name_1.0~"), "plain-name_1.0~");
    }

    #[test]
    fn a_key_that_is_not_base64_or_not_a_key_is_refused() {
        assert!(decode_service_account_key("not base64 at all!!").is_err());
        assert!(decode_service_account_key(&BASE64.encode("{}")).is_err());
    }
}
