//! The one control that changes a picture, wherever a picture is changed.
//!
//! A picture is chosen from disk rather than typed as a URL, so the field is a file input that
//! uploads on `change` and swaps itself back with what it stored. The URL rides along in a hidden
//! input so the surrounding form still saves the same `avatar_url` field it always did -- the
//! upload puts a file somewhere public, and the form that owns the picture decides when to keep
//! it.
//!
//! The fragment is its own swap target: the upload route re-renders exactly this, so the picked
//! file, a refusal, and the "Remove" button all land in the same shape.

use crate::entities::{upload::ImageFormat, value_objects::AvatarUrl};

use super::layout::{AvatarSize, avatar_bubble, escape_html_text};

/// Where the file goes and what comes back.
pub const AVATAR_UPLOAD_PATH: &str = "/ui/uploads/avatar";

/// Clearing a picture, which is a re-render rather than an upload.
pub const AVATAR_CLEAR_PATH: &str = "/ui/uploads/avatar/clear";

/// The only parameters the upload route reads, so a picker inside a big form does not post that
/// whole form's fields alongside the file.
const UPLOAD_PARAMS: &str = "avatar_file,avatar_url,avatar_field_id,avatar_name";

pub struct AvatarPicker<'a> {
    /// The DOM id this field carries and swaps itself at; also what the route echoes back.
    pub field_id: &'a str,
    /// The picture as it currently stands: stored, just uploaded, or none.
    pub avatar_url: Option<&'a AvatarUrl>,
    /// Whose picture it is, for the letter shown while there is none.
    pub name: &'a str,
    pub label: &'a str,
    /// Why the last pick was refused, shown under the field.
    pub error: Option<&'a str>,
}

/// The file picker, its preview, and the hidden `avatar_url` the surrounding form saves.
pub fn avatar_picker(picker: &AvatarPicker<'_>) -> String {
    let field_id = escape_html_text(picker.field_id);
    let stored_url = picker.avatar_url.map(AvatarUrl::as_str).unwrap_or("");

    // `hx-vals` rather than hidden inputs: these two travel with the upload but must not become
    // extra fields on the form that owns the picker.
    let vals = escape_html_text(
        &serde_json::json!({
            "avatar_field_id": picker.field_id,
            "avatar_name": picker.name,
        })
        .to_string(),
    );

    let remove_button = if picker.avatar_url.is_some() {
        format!(
            r##"<button type="button" class="btn btn-ghost btn-xs"
                            hx-post="{AVATAR_CLEAR_PATH}" hx-target="#{field_id}" hx-swap="outerHTML"
                            hx-params="{UPLOAD_PARAMS}" hx-vals="{vals}">Remove</button>"##
        )
    } else {
        String::new()
    };

    let footnote = match picker.error {
        Some(error) => format!(
            r##"<span class="text-[11px] text-error">{error}</span>"##,
            error = escape_html_text(error),
        ),
        None => format!(
            r##"<span class="text-[11px] opacity-60">PNG, JPEG, GIF or WebP, up to {max} MB. Saved when you save this form.</span>"##,
            max = crate::entities::upload::ImageUpload::MAX_BYTES / (1024 * 1024),
        ),
    };

    format!(
        r##"
                    <div id="{field_id}" class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">{label}</span></div>
                        <div class="flex items-center gap-3">
                            {bubble}
                            <input type="hidden" name="avatar_url" value="{avatar_url}">
                            <input type="file" name="avatar_file" accept="{accept}"
                                class="file-input file-input-sm w-full max-w-xs"
                                hx-post="{AVATAR_UPLOAD_PATH}" hx-encoding="multipart/form-data"
                                hx-target="#{field_id}" hx-swap="outerHTML"
                                hx-params="{UPLOAD_PARAMS}" hx-vals="{vals}"
                                hx-disabled-elt="this" hx-indicator="#{field_id}-spinner">
                            <span id="{field_id}-spinner" class="htmx-indicator loading loading-spinner loading-sm"></span>
                            {remove_button}
                        </div>
                        <div class="label">{footnote}</div>
                    </div>
        "##,
        label = escape_html_text(picker.label),
        bubble = avatar_bubble(picker.avatar_url, picker.name, AvatarSize::Header),
        avatar_url = escape_html_text(stored_url),
        accept = ImageFormat::ACCEPT_ATTRIBUTE,
    )
}
