//! Newtype wrappers around `String` for identifiers that are easy to mix up
//! (company slug vs. channel slug, email address vs. RFC Message-ID, ...).
//! See `src/AGENTS.md` for when to reach for one of these instead of `String`.

use std::borrow::Borrow;
use std::fmt;
use std::ops::Deref;

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

// An opaque `Thread-Index` correlation token (e.g. the Outlook conversation
// index header), distinct from a `MessageId`.
string_newtype!(ThreadIndex);

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

fn starts_with_ignore_case(value: &str, prefix: &str) -> bool {
    value
        .get(..prefix.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
