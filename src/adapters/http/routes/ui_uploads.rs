//! `/ui/uploads` — turning a picked file into a URL a form can save.
//!
//! The route stores the file and answers with the field it was picked in, re-rendered around what
//! was stored. Nothing here writes to the database: the picture belongs to whichever form owns the
//! picker, and that form's own save is still what attaches it to a user or an agent. That keeps
//! one upload endpoint serving the account pane and both agent forms, and keeps "who may change
//! this picture" where it already lives.

use std::sync::Arc;

use axum::{
    Form, Router,
    extract::{DefaultBodyLimit, Multipart, State},
    response::{Html, IntoResponse, Response},
    routing::post,
};
use serde::Deserialize;
use tracing::{instrument, warn};

use crate::{
    adapters::{
        http::{
            app_state::AppState,
            pages::{self, AvatarPicker},
        },
        storage::FileStorage,
    },
    entities::{
        upload::ImageUpload,
        value_objects::{AvatarUrl, ObjectKey},
    },
    infra::config::AppConfig,
};

/// The label every avatar picker carries, so the field the route sends back is the field that was
/// picked in, whichever pane that was.
const AVATAR_LABEL: &str = "Picture";

/// What a picker falls back to when the id it sent is missing or not an id.
const DEFAULT_FIELD_ID: &str = "avatar-field";

/// The body limit for a picked file, matching what [`ImageUpload`] accepts, plus room for the
/// multipart envelope and the few small fields travelling with it.
const UPLOAD_BODY_LIMIT: usize = ImageUpload::MAX_BYTES + 64 * 1024;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/ui/uploads/avatar",
            post(upload_avatar).layer(DefaultBodyLimit::max(UPLOAD_BODY_LIMIT)),
        )
        .route("/ui/uploads/avatar/clear", post(clear_avatar))
}

/// The picker's own state, sent with the file so the field can be rebuilt exactly as it was.
#[derive(Debug, Default, Deserialize)]
pub struct AvatarFieldForm {
    pub avatar_field_id: Option<String>,
    pub avatar_name: Option<String>,
    /// The picture the field is showing right now, kept so a refused upload does not silently
    /// drop the picture the form was already holding.
    pub avatar_url: Option<String>,
}

impl AvatarFieldForm {
    /// The picker for this field, showing `avatar_url` and, when the pick failed, why.
    fn render(&self, avatar_url: Option<&AvatarUrl>, error: Option<&str>) -> Html<String> {
        Html(pages::avatar_picker(&AvatarPicker {
            field_id: &field_id(self.avatar_field_id.as_deref()),
            avatar_url,
            name: self.avatar_name.as_deref().unwrap_or(""),
            label: AVATAR_LABEL,
            error,
        }))
    }

    /// The picture the field arrived with, dropping anything that is not a URL a page may render.
    fn current_avatar(&self) -> Option<AvatarUrl> {
        self.avatar_url
            .as_deref()
            .and_then(|url| AvatarUrl::parse(url).ok())
            .flatten()
    }

    /// Re-render the field unchanged, with a reason the pick did not take.
    fn refused(&self, message: &str) -> Response {
        self.render(self.current_avatar().as_ref(), Some(message))
            .into_response()
    }
}

/// A field id is written into an `id` attribute and into the `hx-target` selector that finds it, so
/// only the characters an id is made of are accepted; anything else falls back to the default.
fn field_id(submitted: Option<&str>) -> String {
    let is_id = |value: &&str| {
        !value.is_empty()
            && value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    };

    submitted
        .filter(is_id)
        .unwrap_or(DEFAULT_FIELD_ID)
        .to_string()
}

/// POST /ui/uploads/avatar - Store a picked picture and answer with the field around it (Protected).
#[instrument(skip_all)]
async fn upload_avatar(
    State(storage): State<Option<Arc<dyn FileStorage>>>,
    State(config): State<Arc<AppConfig>>,
    multipart: Multipart,
) -> Response {
    let (field, picked) = match read_upload(multipart).await {
        Ok(submitted) => submitted,
        Err(message) => return AvatarFieldForm::default().refused(&message),
    };

    let (Some(storage), Some(gcs)) = (storage, config.gcs.as_ref()) else {
        return field
            .refused("File uploads are not configured on this server (no storage bucket is set).");
    };

    match store_avatar(storage.as_ref(), &gcs.avatar_folder, picked).await {
        Ok(stored) => field.render(Some(&stored), None).into_response(),
        Err(refusal) => field.refused(&refusal),
    }
}

/// Store a picked file as an avatar, answering with the URL to save or with what to tell the
/// person who picked it.
///
/// Takes the storage and the folder rather than the whole app state, so what happens to a file is
/// decided in one place that a test can drive without a running server.
async fn store_avatar(
    storage: &dyn FileStorage,
    folder: &str,
    picked: Vec<u8>,
) -> Result<AvatarUrl, String> {
    let image = ImageUpload::parse(picked)?;
    let key = ObjectKey::generated(folder, image.format().extension());

    let stored = storage.upload_image(&key, &image).await.map_err(|err| {
        // The reason is for the operator's log, not for the person who picked a file: it can name
        // a bucket and a service account.
        warn!(%key, error = %err, "Avatar upload failed");
        "The picture could not be stored. Please try again.".to_string()
    })?;

    match AvatarUrl::parse(stored.as_str()) {
        Ok(Some(avatar_url)) => Ok(avatar_url),
        // A bucket configured with a base URL that is not `http` would otherwise put an
        // unrenderable -- or worse, an active -- URL in front of every page that shows a face.
        Ok(None) | Err(_) => {
            warn!(%stored, "Stored an avatar at a URL no page can render");
            Err("The picture was stored at an address this app cannot show.".to_string())
        }
    }
}

/// POST /ui/uploads/avatar/clear - Go back to the letter bubble (Protected).
///
/// Clearing is a re-render rather than a delete: the form has not saved the URL yet, and the
/// object it points at may still be the picture someone else is using.
#[instrument(skip_all)]
async fn clear_avatar(Form(field): Form<AvatarFieldForm>) -> Response {
    field.render(None, None).into_response()
}

/// The picked file and the field it was picked in, out of the multipart body.
///
/// Kept as one pass over the parts because a multipart body is a stream: the fields cannot be
/// read twice, and the file may arrive before or after the small fields around it.
async fn read_upload(mut multipart: Multipart) -> Result<(AvatarFieldForm, Vec<u8>), String> {
    let mut field = AvatarFieldForm::default();
    let mut picked = Vec::new();

    loop {
        let part = multipart
            .next_field()
            .await
            .map_err(|err| format!("The upload was cut short: {err}"))?;

        let Some(part) = part else { break };

        match part.name().unwrap_or_default().to_string().as_str() {
            "avatar_file" => {
                picked = part
                    .bytes()
                    .await
                    .map_err(|err| format!("The file could not be read: {err}"))?
                    .to_vec();
            }
            "avatar_field_id" => field.avatar_field_id = part.text().await.ok(),
            "avatar_name" => field.avatar_name = part.text().await.ok(),
            "avatar_url" => field.avatar_url = part.text().await.ok(),
            // Anything else is a field of the form the picker happens to sit in.
            _ => {}
        }
    }

    Ok((field, picked))
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, extract::FromRequest, http::Request};

    use crate::adapters::storage::test_support::FakeStorage;

    use super::*;

    const BOUNDARY: &str = "----boundary";

    /// A PNG as far as everything downstream is concerned: the signature is what is checked.
    fn png() -> Vec<u8> {
        b"\x89PNG\r\n\x1a\n........".to_vec()
    }

    /// A multipart body with the picker's fields around a picked file, in the order a browser
    /// sends them: the small fields htmx adds first, the file last.
    async fn multipart_body(parts: &[(&str, Option<&str>, &[u8])]) -> Multipart {
        let mut body: Vec<u8> = Vec::new();

        for (name, filename, content) in parts {
            body.extend(format!("--{BOUNDARY}\r\n").as_bytes());
            match filename {
                Some(filename) => body.extend(
                    format!(
                        "Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
                    )
                    .as_bytes(),
                ),
                None => body.extend(
                    format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
                ),
            }
            body.extend(*content);
            body.extend(b"\r\n");
        }
        body.extend(format!("--{BOUNDARY}--\r\n").as_bytes());

        let request = Request::builder()
            .method("POST")
            .header(
                "content-type",
                format!("multipart/form-data; boundary={BOUNDARY}"),
            )
            .body(Body::from(body))
            .expect("a well-formed multipart request");

        Multipart::from_request(request, &())
            .await
            .expect("axum reads its own multipart")
    }

    #[tokio::test]
    async fn the_picked_file_and_the_fields_around_it_are_read_in_one_pass() {
        let multipart = multipart_body(&[
            ("avatar_field_id", None, b"agent-avatar-new"),
            ("avatar_name", None, b"Triage"),
            ("avatar_url", None, b"https://cdn.example.com/old.png"),
            // The form the picker sits in may send its own fields; they are not ours to read.
            ("name", None, b"Triage"),
            ("avatar_file", Some("me.png"), &png()),
        ])
        .await;

        let (field, picked) = read_upload(multipart).await.expect("a readable body");

        assert_eq!(field.avatar_field_id.as_deref(), Some("agent-avatar-new"));
        assert_eq!(field.avatar_name.as_deref(), Some("Triage"));
        assert_eq!(
            field.avatar_url.as_deref(),
            Some("https://cdn.example.com/old.png")
        );
        assert_eq!(picked, png());
    }

    #[tokio::test]
    async fn a_stored_picture_comes_back_as_the_url_to_save() {
        let storage = FakeStorage::returning("https://cdn.example.com/avatars/new.png");

        let stored = store_avatar(&storage, "avatars", png())
            .await
            .expect("a PNG in a working bucket");

        assert_eq!(
            stored,
            AvatarUrl::from("https://cdn.example.com/avatars/new.png")
        );

        // It went to a generated key in the configured folder, with the extension of what it
        // actually is rather than of what it was called.
        let stored = storage.objects();
        let stored = stored.first().expect("one upload");
        assert!(
            stored.key.starts_with("avatars/") && stored.key.ends_with(".png"),
            "{}",
            stored.key
        );
        assert_eq!(stored.bytes.len(), png().len());
        // An avatar is the one thing that belongs in the bucket anyone can read.
        assert_eq!(stored.bucket, crate::adapters::storage::BucketKind::Public);
    }

    #[tokio::test]
    async fn what_is_not_a_picture_never_reaches_the_bucket() {
        let storage = FakeStorage::returning("https://cdn.example.com/avatars/new.png");

        let refusal = store_avatar(&storage, "avatars", b"<svg onload=alert(1)>".to_vec())
            .await
            .expect_err("not a picture");

        assert!(refusal.contains("PNG, JPEG, GIF or WebP"), "{refusal}");
        assert!(storage.objects().is_empty());
    }

    #[tokio::test]
    async fn a_bucket_failure_is_reported_without_naming_the_bucket() {
        let refusal = store_avatar(&FakeStorage::failing(), "avatars", png())
            .await
            .expect_err("the bucket refused");

        assert!(refusal.contains("could not be stored"), "{refusal}");
        assert!(!refusal.contains("bucket on fire"), "{refusal}");
    }

    #[tokio::test]
    async fn a_url_no_page_could_render_is_refused_rather_than_saved() {
        let storage = FakeStorage::returning("javascript:alert(1)");

        let refusal = store_avatar(&storage, "avatars", png())
            .await
            .expect_err("not a URL a page may render");

        assert!(refusal.contains("cannot show"), "{refusal}");
    }

    #[test]
    fn a_field_id_is_only_ever_an_id() {
        assert_eq!(field_id(Some("agent-avatar-new")), "agent-avatar-new");
        assert_eq!(field_id(Some("member_avatar_1")), "member_avatar_1");

        // Anything that could break out of the attribute or the selector is not used.
        assert_eq!(field_id(Some("a\" onload=\"x")), DEFAULT_FIELD_ID);
        assert_eq!(field_id(Some("#a .b")), DEFAULT_FIELD_ID);
        assert_eq!(field_id(Some("")), DEFAULT_FIELD_ID);
        assert_eq!(field_id(None), DEFAULT_FIELD_ID);
    }

    #[test]
    fn the_field_keeps_a_stored_picture_but_not_a_dangerous_one() {
        let field = AvatarFieldForm {
            avatar_url: Some("https://cdn.example.com/a.png".to_string()),
            ..Default::default()
        };
        assert_eq!(
            field.current_avatar(),
            Some(AvatarUrl::from("https://cdn.example.com/a.png"))
        );

        // A tampered hidden field must not come back out in an `<img src>`.
        let tampered = AvatarFieldForm {
            avatar_url: Some("javascript:alert(1)".to_string()),
            ..Default::default()
        };
        assert_eq!(tampered.current_avatar(), None);
    }
}
