//! What a signed-in browser carries, and the only thing that decides whether to believe it.
//!
//! Sessions used to be the user's id written into a cookie in plain text, which meant anyone who
//! could set a cookie was any user they could name. A session is now a token signed with
//! `JWT_SECRET`, so the server can tell a cookie it issued from one somebody typed, and the token
//! carries its own expiry rather than living forever.
//!
//! Minting and believing both live here so they cannot drift: an issued cookie's flags, the
//! lifetime in the claims, and the validation the middleware applies are one object's decisions.

use axum::http::HeaderMap;
use axum_extra::extract::{
    CookieJar,
    cookie::{Cookie, SameSite},
};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use time::Duration;
use tracing::warn;
use uuid::Uuid;

use crate::infra::config::AppConfig;

/// The cookie a session is carried in.
pub const SESSION_COOKIE: &str = "session";

/// The cookie sessions used to be carried in.
///
/// Nothing reads it any more — that is the entire point of this module — but sign-out still clears
/// it, so a browser holding one stops offering it and no forged value lingers.
pub const LEGACY_USER_ID_COOKIE: &str = "user_id";

/// The shortest `JWT_SECRET` worth signing with.
///
/// HS256 accepts a secret of any length, which is the problem: a short one is guessable, and a
/// guessed session key is every account at once. Checked at startup rather than at sign-in, so a
/// deployment cannot get as far as serving pages with a weak one.
pub const MIN_SECRET_BYTES: usize = 32;

/// What a session token says: who, when it was issued, and when to stop believing it.
#[derive(Debug, Serialize, Deserialize)]
struct SessionClaims {
    sub: Uuid,
    iat: i64,
    exp: i64,
}

/// Issues sessions and decides whether one is genuine.
pub struct SessionAuthority {
    encoding: EncodingKey,
    decoding: DecodingKey,
    validation: Validation,
    lifetime: Duration,
    /// Whether issued cookies are marked `Secure`, i.e. never sent over plain HTTP.
    secure_cookies: bool,
}

impl SessionAuthority {
    pub fn new(config: &AppConfig) -> Self {
        let secret = config.jwt_secret.as_bytes();

        // `exp` is what makes a stolen cookie stop working, so a token without one is not a
        // session. `jsonwebtoken` requires it by default; making it explicit keeps that true if
        // the default ever changes.
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_required_spec_claims(&["exp"]);

        Self {
            encoding: EncodingKey::from_secret(secret),
            decoding: DecodingKey::from_secret(secret),
            validation,
            lifetime: config.refresh_token_ttl,
            secure_cookies: config.secure_cookies,
        }
    }

    /// The cookie that signs `user_id` in.
    ///
    /// `Lax` rather than `Strict` because sign-in redirects and links from mail have to arrive
    /// authenticated; it still keeps the cookie off cross-site form posts, which is what makes a
    /// CSRF against these routes fail.
    pub fn cookie(&self, user_id: Uuid) -> Cookie<'static> {
        Cookie::build((SESSION_COOKIE, self.issue(user_id)))
            .path("/")
            .http_only(true)
            .same_site(SameSite::Lax)
            .secure(self.secure_cookies)
            .max_age(self.lifetime)
            .build()
    }

    /// The cookies that sign somebody out: the session, and the one sessions used to live in.
    pub fn cleared_cookies(&self) -> [Cookie<'static>; 2] {
        [
            Cookie::build((SESSION_COOKIE, "")).path("/").build(),
            Cookie::build((LEGACY_USER_ID_COOKIE, "")).path("/").build(),
        ]
    }

    /// A signed token for `user_id`, valid for one session lifetime.
    fn issue(&self, user_id: Uuid) -> String {
        let issued_at = time::OffsetDateTime::now_utc();
        let claims = SessionClaims {
            sub: user_id,
            iat: issued_at.unix_timestamp(),
            exp: (issued_at + self.lifetime).unix_timestamp(),
        };

        encode(&Header::new(Algorithm::HS256), &claims, &self.encoding).unwrap_or_else(|err| {
            // HS256 over an in-memory key has no failure mode that is not a bug in this function.
            warn!(error = %err, "Failed to sign a session token");
            String::new()
        })
    }

    /// Who a token says it is, or `None` for anything not signed by us, expired, or malformed.
    pub fn verify(&self, token: &str) -> Option<Uuid> {
        decode::<SessionClaims>(token, &self.decoding, &self.validation)
            .ok()
            .map(|token| token.claims.sub)
    }

    /// Who a request is signed in as, from the cookies it carries.
    pub fn user_from_headers(&self, headers: &HeaderMap) -> Option<Uuid> {
        let jar = CookieJar::from_headers(headers);
        self.verify(jar.get(SESSION_COOKIE)?.value())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authority(secret: &str, lifetime: Duration) -> SessionAuthority {
        let config = AppConfig {
            jwt_secret: secret.to_string(),
            refresh_token_ttl: lifetime,
            secure_cookies: true,
            ..AppConfig::for_test()
        };

        SessionAuthority::new(&config)
    }

    fn signing(secret: &str) -> SessionAuthority {
        authority(secret, Duration::days(30))
    }

    const SECRET: &str = "a-secret-long-enough-to-sign-with-0123456789";

    #[test]
    fn a_token_we_issued_names_the_user_it_was_issued_for() {
        let sessions = signing(SECRET);
        let user_id = Uuid::new_v4();

        assert_eq!(sessions.verify(&sessions.issue(user_id)), Some(user_id));
    }

    #[test]
    fn a_token_signed_with_another_secret_is_nobody() {
        let user_id = Uuid::new_v4();
        let elsewhere = signing("a-different-secret-of-a-respectable-length");

        assert_eq!(signing(SECRET).verify(&elsewhere.issue(user_id)), None);
    }

    #[test]
    fn a_tampered_token_is_nobody() {
        let sessions = signing(SECRET);
        let token = sessions.issue(Uuid::new_v4());

        // Flip the last character of the signature: the payload still parses, so only the
        // signature check can catch this.
        let mut tampered = token[..token.len() - 1].to_string();
        tampered.push(if token.ends_with('a') { 'b' } else { 'a' });
        assert_eq!(sessions.verify(&tampered), None);

        // A payload edited to name somebody else, with the old signature left in place.
        let mut parts = token.split('.');
        let (header, _, signature) = (
            parts.next().expect("a header"),
            parts.next().expect("a payload"),
            parts.next().expect("a signature"),
        );
        let forged_payload = signing("anything").issue(Uuid::new_v4());
        let forged_payload = forged_payload.split('.').nth(1).expect("a payload");
        assert_eq!(
            sessions.verify(&format!("{header}.{forged_payload}.{signature}")),
            None
        );
    }

    #[test]
    fn an_expired_token_is_nobody() {
        // Issued with a lifetime already behind us, then read by an authority that would
        // otherwise accept it. `jsonwebtoken` allows 60s of clock leeway, so this has to be
        // further back than that.
        let expired = authority(SECRET, Duration::minutes(-5)).issue(Uuid::new_v4());

        assert_eq!(signing(SECRET).verify(&expired), None);
    }

    #[test]
    fn nonsense_is_nobody() {
        let sessions = signing(SECRET);

        assert_eq!(sessions.verify(""), None);
        assert_eq!(sessions.verify("not-a-token"), None);
        // The value the old plaintext cookie held, which must no longer authenticate anything.
        assert_eq!(sessions.verify(&Uuid::new_v4().to_string()), None);
    }

    #[test]
    fn the_issued_cookie_cannot_be_read_by_script_or_sent_in_the_clear() {
        let cookie = signing(SECRET).cookie(Uuid::new_v4());

        assert_eq!(cookie.name(), SESSION_COOKIE);
        assert_eq!(cookie.http_only(), Some(true));
        assert_eq!(cookie.secure(), Some(true));
        assert_eq!(cookie.same_site(), Some(SameSite::Lax));
        assert_eq!(cookie.max_age(), Some(Duration::days(30)));
        assert_eq!(cookie.path(), Some("/"));
    }

    #[test]
    fn a_plain_http_deployment_gets_a_cookie_it_can_actually_send() {
        let config = AppConfig {
            jwt_secret: SECRET.to_string(),
            secure_cookies: false,
            ..AppConfig::for_test()
        };

        assert_eq!(
            SessionAuthority::new(&config)
                .cookie(Uuid::new_v4())
                .secure(),
            Some(false)
        );
    }

    #[test]
    fn signing_out_clears_the_session_and_the_cookie_it_replaced() {
        let cleared = signing(SECRET).cleared_cookies();
        let names: Vec<&str> = cleared.iter().map(|cookie| cookie.name()).collect();

        assert_eq!(names, vec![SESSION_COOKIE, LEGACY_USER_ID_COOKIE]);
        assert!(cleared.iter().all(|cookie| cookie.value().is_empty()));
    }

    #[test]
    fn headers_authenticate_only_through_the_session_cookie() {
        let sessions = signing(SECRET);
        let user_id = Uuid::new_v4();

        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            format!("{SESSION_COOKIE}={}", sessions.issue(user_id))
                .parse()
                .expect("a cookie header"),
        );
        assert_eq!(sessions.user_from_headers(&headers), Some(user_id));

        // The old cookie, on its own, authenticates nobody.
        let mut legacy = HeaderMap::new();
        legacy.insert(
            axum::http::header::COOKIE,
            format!("{LEGACY_USER_ID_COOKIE}={user_id}")
                .parse()
                .expect("a cookie header"),
        );
        assert_eq!(sessions.user_from_headers(&legacy), None);
    }
}
