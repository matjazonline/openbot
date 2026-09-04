//! Newtype wrappers around `String` for identifiers that are easy to mix up
//! (company slug vs. channel slug, email address vs. RFC Message-ID, ...).
//! See `src/AGENTS.md` for when to reach for one of these instead of `String`.

use std::borrow::Borrow;
use std::fmt;
use std::ops::Deref;

use base64::{Engine, engine::general_purpose::STANDARD as BASE64_STANDARD};
use serde::{Deserialize, Serialize};

macro_rules! string_newtype {
    ($name:ident) => {
        #[derive(
            Debug, Default, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl Deref for $name {
            type Target = str;

            fn deref(&self) -> &str {
                &self.0
            }
        }

        impl Borrow<str> for $name {
            fn borrow(&self) -> &str {
                &self.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_string())
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }

        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.0 == *other
            }
        }

        impl PartialEq<String> for $name {
            fn eq(&self, other: &String) -> bool {
                &self.0 == other
            }
        }
    };
}

// A company's URL slug, e.g. the `acme` in `support@acme.mailagents.com`.
string_newtype!(CompanySlug);

// A model vendor selected independently from the model name it offers.
string_newtype!(ModelProvider);

impl ModelProvider {
    /// Providers are matched case-insensitively wherever they are compared, so the fold happens
    /// once, here. Scattering `trim().to_ascii_lowercase()` across call sites is how the two
    /// halves of a comparison drift apart.
    pub fn canonical(value: impl AsRef<str>) -> Self {
        Self(value.as_ref().trim().to_ascii_lowercase())
    }
}

// One provider-specific model identifier. Kept distinct from `ModelProvider` because the two are
// submitted and persisted beside each other throughout model-connection configuration.
string_newtype!(ModelName);

impl ModelName {
    /// Trims but does not fold case: model identifiers are assigned by the provider and are
    /// case-sensitive (`gpt-4o` is not `GPT-4o`). The asymmetry with [`ModelProvider::canonical`]
    /// is deliberate, and stating it once here is what stops each comparison re-deciding it.
    pub fn canonical(value: impl AsRef<str>) -> Self {
        Self(value.as_ref().trim().to_string())
    }
}

// A channel's URL slug, e.g. the `support` in `support@acme.mailagents.com`.
string_newtype!(ChannelSlug);

// An email address as seen on the wire (not necessarily normalized/lowercased
// by construction — callers that need a normalized form should still trim /
// lowercase explicitly, this type only prevents mixing addresses up with
// unrelated strings such as slugs or message IDs).
string_newtype!(EmailAddress);

impl EmailAddress {
    /// Compare two addresses the way a mail system does: ignoring case.
    ///
    /// The derived `PartialEq` is case-*sensitive*, which is right for a `String` and wrong for an
    /// address — `Ops@Example.com` and `ops@example.com` are one mailbox. The columns these are
    /// matched against are `CITEXT`, so a `==` here would disagree with the same comparison made in
    /// Postgres. Anything deciding *who someone is* must go through this rather than `==`.
    pub fn eq_ignore_case(&self, other: &Self) -> bool {
        self.0.trim().eq_ignore_ascii_case(other.0.trim())
    }
}

// An RFC 5322 `Message-ID` (or `In-Reply-To` / one entry of `References`).
string_newtype!(MessageId);

// An Outlook conversation index, distinct from an RFC Message-ID. Construction and transparent
// deserialization stay permissive so an old malformed durable payload remains readable; use
// `parse` before storage or lookup.
string_newtype!(ThreadIndex);

const THREAD_INDEX_ROOT_BYTES: usize = 22;
const THREAD_INDEX_RESPONSE_BYTES: usize = 5;
pub const MAX_THREAD_INDEX_RESPONSE_LEVELS: usize = 128;
pub const MAX_THREAD_INDEX_DECODED_BYTES: usize =
    THREAD_INDEX_ROOT_BYTES + THREAD_INDEX_RESPONSE_BYTES * MAX_THREAD_INDEX_RESPONSE_LEVELS;
pub const MAX_THREAD_INDEX_ENCODED_BYTES: usize = MAX_THREAD_INDEX_DECODED_BYTES.div_ceil(3) * 4;
/// Enough room for RFC 2045 line folding while keeping work independent of the message-size cap.
pub const THREAD_INDEX_FOLDING_ALLOWANCE_BYTES: usize = 64;
pub const MAX_THREAD_INDEX_RAW_BYTES: usize =
    MAX_THREAD_INDEX_ENCODED_BYTES + THREAD_INDEX_FOLDING_ALLOWANCE_BYTES;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ThreadIndexParseError {
    #[error("empty")]
    Empty,
    #[error("raw_length_limit")]
    RawLengthLimit,
    #[error("invalid_base64")]
    InvalidBase64,
    #[error("invalid_length")]
    InvalidLength,
    #[error("response_level_limit")]
    ResponseLevelLimit,
    #[error("invalid_version")]
    InvalidVersion,
}

impl ThreadIndexParseError {
    pub const fn metric_reason(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::RawLengthLimit => "raw_length_limit",
            Self::InvalidBase64 => "invalid_base64",
            Self::InvalidLength => "invalid_length",
            Self::ResponseLevelLimit => "response_level_limit",
            Self::InvalidVersion => "invalid_version",
        }
    }

    pub const fn warning_slot(self) -> usize {
        match self {
            Self::Empty => 0,
            Self::RawLengthLimit => 1,
            Self::InvalidBase64 => 2,
            Self::InvalidLength => 3,
            Self::ResponseLevelLimit => 4,
            Self::InvalidVersion => 5,
        }
    }
}

impl ThreadIndex {
    /// Parse the bounded Microsoft MAPI shape and return canonical padded Base64.
    pub fn parse(raw: &str) -> Result<Self, ThreadIndexParseError> {
        let bytes = Self::decode_and_validate(raw)?;
        Ok(Self(BASE64_STANDARD.encode(bytes)))
    }

    /// Every binary ancestor, root first and the exact value last.
    ///
    /// Revalidates because the transparent newtype may have come from an old durable payload or a
    /// permissive `From<String>` call rather than [`Self::parse`].
    pub fn ancestor_chain(&self) -> Result<Vec<Self>, ThreadIndexParseError> {
        let bytes = Self::decode_and_validate(&self.0)?;
        let response_levels = (bytes.len() - THREAD_INDEX_ROOT_BYTES) / THREAD_INDEX_RESPONSE_BYTES;
        let mut ancestors = Vec::with_capacity(response_levels + 1);
        for level in 0..=response_levels {
            let prefix_len = THREAD_INDEX_ROOT_BYTES + level * THREAD_INDEX_RESPONSE_BYTES;
            ancestors.push(Self(BASE64_STANDARD.encode(&bytes[..prefix_len])));
        }
        Ok(ancestors)
    }

    fn decode_and_validate(raw: &str) -> Result<Vec<u8>, ThreadIndexParseError> {
        if raw.len() > MAX_THREAD_INDEX_RAW_BYTES {
            return Err(ThreadIndexParseError::RawLengthLimit);
        }

        let compact: Vec<u8> = raw
            .bytes()
            .filter(|byte| !matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
            .collect();
        if compact.is_empty() {
            return Err(ThreadIndexParseError::Empty);
        }

        let decoded = BASE64_STANDARD
            .decode(compact)
            .map_err(|_| ThreadIndexParseError::InvalidBase64)?;
        if decoded.len() < THREAD_INDEX_ROOT_BYTES
            || !(decoded.len() - THREAD_INDEX_ROOT_BYTES)
                .is_multiple_of(THREAD_INDEX_RESPONSE_BYTES)
        {
            return Err(ThreadIndexParseError::InvalidLength);
        }

        let response_levels =
            (decoded.len() - THREAD_INDEX_ROOT_BYTES) / THREAD_INDEX_RESPONSE_BYTES;
        if response_levels > MAX_THREAD_INDEX_RESPONSE_LEVELS {
            return Err(ThreadIndexParseError::ResponseLevelLimit);
        }
        if decoded[0] != 0x01 {
            return Err(ThreadIndexParseError::InvalidVersion);
        }

        Ok(decoded)
    }
}

// A profile picture's URL, for a user or an agent. Rendered straight into an `<img src>`, which is
// why it is parsed rather than taken as typed -- see [`AvatarUrl::parse`].
string_newtype!(AvatarUrl);

impl AvatarUrl {
    /// What a submitted avatar field means: `Ok(None)` for a blank one (the letter bubble), and an
    /// error for anything that is not an `http`/`https` URL.
    ///
    /// The scheme check is the point of the type. The value ends up in an `<img src>` on every page
    /// that shows the owner, so `javascript:` and `data:` are refused here, at the one place a URL
    /// enters the system, rather than at each of the places that render one.
    pub fn parse(value: &str) -> Result<Option<Self>, String> {
        let trimmed = value.trim();

        if trimmed.is_empty() {
            return Ok(None);
        }

        let scheme_ok = ["http://", "https://"]
            .iter()
            .any(|scheme| trimmed.len() > scheme.len() && starts_with_ignore_case(trimmed, scheme));

        if !scheme_ok {
            return Err("An avatar URL must start with http:// or https://.".to_string());
        }

        Ok(Some(Self(trimmed.to_string())))
    }
}

// Where an uploaded file lives inside its bucket, e.g. `avatars/1f0c....png`. Distinct from the
// URL it is served from: one is a key the storage API takes, the other is what a browser fetches.
string_newtype!(ObjectKey);

impl ObjectKey {
    /// A fresh key under `folder`, named by a UUID rather than by anything the uploader sent.
    ///
    /// The name is generated, not derived from the client's file name, so `../` and a colliding
    /// upload are both impossible by construction rather than by sanitizing a submitted string.
    pub fn generated(folder: &str, extension: &str) -> Self {
        Self(format!(
            "{folder}/{name}.{extension}",
            folder = folder.trim_matches('/'),
            name = uuid::Uuid::new_v4(),
        ))
    }
}

// The opaque last segment of one company's Resend webhook URL, e.g. the `9f2c...` in
// `https://example.com/webhooks/email/resend_api/9f2c...`.
//
// Distinct from every other identifier a webhook could be addressed by -- and from `CompanySlug`
// in particular -- because it is the one string that says which tenant an unauthenticated request
// belongs to. Deliberately carries no company name: it is registered in a Resend dashboard the
// application does not control, and a token survives a rename that a slug would not.
string_newtype!(ResendApiWebhookToken);

impl ResendApiWebhookToken {
    /// The exact length the column's own check constraint enforces.
    pub const LENGTH: usize = 32;

    /// A fresh token: 122 bits of randomness, spelled as the 32 lowercase hex digits the column
    /// accepts. Generated rather than derived from anything about the company, so knowing a
    /// tenant tells an attacker nothing about the URL its mail arrives on.
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().simple().to_string())
    }

    /// What a URL path segment means. `None` for anything this application would not have
    /// written, so a malformed segment is refused before it reaches a query.
    pub fn parse(value: &str) -> Option<Self> {
        let trimmed = value.trim();
        (trimmed.len() == Self::LENGTH
            && trimmed
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit()))
        .then(|| Self(trimmed.to_string()))
    }
}

// The `authserv-id` an `Authentication-Results` header must carry for its verdicts to be believed,
// e.g. `resend.com`.
//
// A newtype because it travels beside [`ResendApiWebhookToken`] as a sibling field of the same
// integration, and because it has a comparison rule of its own: the header names it
// case-insensitively, and re-deciding that at each reader is how a forged header becomes a pass.
string_newtype!(AuthservId);

impl AuthservId {
    /// The longest value the column stores.
    pub const MAX_BYTES: usize = 255;

    /// One `authserv-id` token, trimmed. Rejects the whitespace-bearing values that could never
    /// match a header, rather than storing one that silently authenticates nothing.
    pub fn parse(value: &str) -> Result<Self, String> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err("An authserv-id is required.".to_string());
        }
        if trimmed.len() > Self::MAX_BYTES {
            return Err(format!(
                "An authserv-id may be at most {} characters.",
                Self::MAX_BYTES
            ));
        }
        if trimmed.chars().any(char::is_whitespace) {
            return Err("An authserv-id is one token, such as resend.com.".to_string());
        }
        Ok(Self(trimmed.to_string()))
    }
}

// The public URL an uploaded object is served from. Becomes an [`AvatarUrl`] only by going through
// [`AvatarUrl::parse`], so a misconfigured base URL cannot put a non-`http` scheme in an `<img>`.
string_newtype!(ObjectUrl);

fn starts_with_ignore_case(value: &str, prefix: &str) -> bool {
    value
        .get(..prefix.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conversation_index(response_levels: usize) -> Vec<u8> {
        let mut bytes: Vec<u8> = (0..THREAD_INDEX_ROOT_BYTES)
            .map(|index| index as u8)
            .collect();
        bytes[0] = 0x01;
        for level in 0..response_levels {
            bytes.extend((0..THREAD_INDEX_RESPONSE_BYTES).map(|offset| {
                (THREAD_INDEX_ROOT_BYTES + level * THREAD_INDEX_RESPONSE_BYTES + offset) as u8
            }));
        }
        bytes
    }

    fn encoded_index(response_levels: usize) -> String {
        BASE64_STANDARD.encode(conversation_index(response_levels))
    }

    #[test]
    fn thread_index_canonicalizes_and_derives_binary_ancestors() {
        let root = encoded_index(0);
        let direct_reply = encoded_index(1);
        let second_reply = encoded_index(2);
        let third_reply = encoded_index(3);

        assert_eq!(ThreadIndex::parse(&root).unwrap().as_str(), root);
        assert_eq!(
            ThreadIndex::parse(&direct_reply)
                .unwrap()
                .ancestor_chain()
                .unwrap(),
            [root.clone(), direct_reply.clone()].map(ThreadIndex::from)
        );
        assert_eq!(
            ThreadIndex::parse(&second_reply)
                .unwrap()
                .ancestor_chain()
                .unwrap(),
            [root.clone(), direct_reply.clone(), second_reply.clone()].map(ThreadIndex::from)
        );
        assert_eq!(
            ThreadIndex::parse(&third_reply)
                .unwrap()
                .ancestor_chain()
                .unwrap(),
            [root, direct_reply, second_reply, third_reply].map(ThreadIndex::from)
        );
    }

    #[test]
    fn thread_index_normalizes_rfc_2045_folding() {
        let canonical = encoded_index(3);
        let folded = format!("{}\r\n\t{}", &canonical[..28], &canonical[28..]);
        assert_eq!(ThreadIndex::parse(&folded).unwrap().as_str(), canonical);
    }

    #[test]
    fn thread_index_rejects_malformed_and_unbounded_values() {
        assert_eq!(ThreadIndex::parse(""), Err(ThreadIndexParseError::Empty));
        assert_eq!(
            ThreadIndex::parse("not base64!!"),
            Err(ThreadIndexParseError::InvalidBase64)
        );

        let mut wrong_version = conversation_index(0);
        wrong_version[0] = 0x02;
        assert_eq!(
            ThreadIndex::parse(&BASE64_STANDARD.encode(wrong_version)),
            Err(ThreadIndexParseError::InvalidVersion)
        );
        assert_eq!(
            ThreadIndex::parse(&BASE64_STANDARD.encode([0x01; 21])),
            Err(ThreadIndexParseError::InvalidLength)
        );
        assert_eq!(
            ThreadIndex::parse(&BASE64_STANDARD.encode([0x01; 23])),
            Err(ThreadIndexParseError::InvalidLength)
        );
        assert_eq!(
            ThreadIndex::parse(&encoded_index(MAX_THREAD_INDEX_RESPONSE_LEVELS + 1)),
            Err(ThreadIndexParseError::ResponseLevelLimit)
        );
        assert_eq!(
            ThreadIndex::parse(&"A".repeat(MAX_THREAD_INDEX_RAW_BYTES + 1)),
            Err(ThreadIndexParseError::RawLengthLimit)
        );
    }

    #[test]
    fn thread_index_work_is_bounded_at_the_accepted_maximum() {
        let raw = encoded_index(MAX_THREAD_INDEX_RESPONSE_LEVELS);
        assert_eq!(raw.len(), MAX_THREAD_INDEX_ENCODED_BYTES);
        let parsed = ThreadIndex::parse(&raw).unwrap();
        let ancestors = parsed.ancestor_chain().unwrap();
        assert_eq!(ancestors.len(), MAX_THREAD_INDEX_RESPONSE_LEVELS + 1);
        assert_eq!(ancestors.last(), Some(&parsed));
    }

    #[test]
    fn thread_index_json_stays_permissive_until_the_decision_boundary() {
        let malformed: ThreadIndex = serde_json::from_str(r#""legacy-invalid""#).unwrap();
        assert_eq!(malformed.as_str(), "legacy-invalid");
        assert!(ThreadIndex::parse(malformed.as_str()).is_err());

        let canonical = ThreadIndex::parse(&encoded_index(1)).unwrap();
        let json = serde_json::to_string(&canonical).unwrap();
        let restored: ThreadIndex = serde_json::from_str(&json).unwrap();
        assert_eq!(ThreadIndex::parse(restored.as_str()).unwrap(), canonical);
    }

    /// An avatar URL is rendered into an `<img src>`, so what `parse` refuses is a security
    /// boundary and not a formatting preference.
    #[test]
    fn an_avatar_url_is_an_http_url_or_nothing() {
        assert_eq!(
            AvatarUrl::parse("https://example.com/me.png").unwrap(),
            Some(AvatarUrl::from("https://example.com/me.png"))
        );
        assert_eq!(
            AvatarUrl::parse("  http://example.com/me.png  ").unwrap(),
            Some(AvatarUrl::from("http://example.com/me.png"))
        );
        // A scheme is matched the way URLs are: case-insensitively.
        assert_eq!(
            AvatarUrl::parse("HTTPS://example.com/me.png").unwrap(),
            Some(AvatarUrl::from("HTTPS://example.com/me.png"))
        );

        // Blank is how a picture is removed, not an error.
        assert_eq!(AvatarUrl::parse("").unwrap(), None);
        assert_eq!(AvatarUrl::parse("   ").unwrap(), None);

        // Everything a page must never be pointed at.
        assert!(AvatarUrl::parse("javascript:alert(1)").is_err());
        assert!(AvatarUrl::parse("data:image/svg+xml,<svg/>").is_err());
        assert!(AvatarUrl::parse("//example.com/me.png").is_err());
        assert!(AvatarUrl::parse("example.com/me.png").is_err());
        // A scheme with nothing after it is not a URL either.
        assert!(AvatarUrl::parse("https://").is_err());
    }

    #[test]
    fn a_generated_object_key_is_named_by_us_not_by_the_uploader() {
        let key = ObjectKey::generated("avatars", "png");

        assert!(key.starts_with("avatars/"), "{key}");
        assert!(key.ends_with(".png"), "{key}");
        // Nothing but the folder, a UUID and the extension, so no upload can walk out of the
        // folder or land on another one's name.
        assert_eq!(key.matches('/').count(), 1);
        assert_eq!(key.len(), "avatars/".len() + 36 + ".png".len());
        assert_ne!(ObjectKey::generated("avatars", "png"), key);

        // A folder written with slashes around it still produces one clean join.
        assert!(ObjectKey::generated("/pictures/", "jpg").starts_with("pictures/"));
    }
}
