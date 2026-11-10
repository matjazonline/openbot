//! Bearer tokens `msk_<adm|cli>_<id>_<secret>` and their verifiers (decisions D3).
//! The stored verifier is SHA-256 of a 256-bit random secret, compared in constant time.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Admin,
    Client,
}

impl TokenKind {
    const fn tag(self) -> &'static str {
        match self {
            Self::Admin => "adm",
            Self::Client => "cli",
        }
    }
}

/// A parsed bearer token. The secret never appears in `Debug` output.
#[derive(Clone, PartialEq, Eq)]
pub struct ParsedToken {
    pub kind: TokenKind,
    pub id: Uuid,
    secret: [u8; 32],
}

impl std::fmt::Debug for ParsedToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParsedToken")
            .field("kind", &self.kind)
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

/// A freshly issued token: show `token` once, store `verifier`.
pub struct IssuedToken {
    pub id: Uuid,
    pub token: String,
    pub verifier: [u8; 32],
}

pub fn verifier(secret: &[u8; 32]) -> [u8; 32] {
    Sha256::digest(secret).into()
}

pub fn issue(kind: TokenKind) -> Result<IssuedToken, getrandom::Error> {
    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret)?;
    let id = Uuid::now_v7();
    let token = format!(
        "msk_{}_{}_{}",
        kind.tag(),
        id.simple(),
        URL_SAFE_NO_PAD.encode(secret)
    );
    Ok(IssuedToken {
        id,
        token,
        verifier: verifier(&secret),
    })
}

pub fn parse(token: &str) -> Option<ParsedToken> {
    let rest = token.strip_prefix("msk_")?;
    let (tag, rest) = rest.split_once('_')?;
    let kind = match tag {
        "adm" => TokenKind::Admin,
        "cli" => TokenKind::Client,
        _ => return None,
    };
    let (id, secret) = rest.split_once('_')?;
    if id.len() != 32 {
        return None;
    }
    let id = Uuid::try_parse(id).ok()?;
    let decoded = URL_SAFE_NO_PAD.decode(secret).ok()?;
    let secret: [u8; 32] = decoded.try_into().ok()?;
    Some(ParsedToken { kind, id, secret })
}

impl ParsedToken {
    /// Constant-time comparison against a stored verifier.
    pub fn matches(&self, stored: &[u8]) -> bool {
        let computed = verifier(&self.secret);
        stored.len() == computed.len() && bool::from(computed.ct_eq(stored))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_authority_tokens_round_trip_and_reject_tampering() {
        let issued = issue(TokenKind::Client).unwrap();
        let parsed = parse(&issued.token).unwrap();
        assert_eq!(parsed.kind, TokenKind::Client);
        assert_eq!(parsed.id, issued.id);
        assert!(parsed.matches(&issued.verifier));
        assert!(!format!("{parsed:?}").contains(&issued.token[40..]));
        let other = issue(TokenKind::Client).unwrap();
        assert!(!parsed.matches(&other.verifier));
        assert!(!parsed.matches(&issued.verifier[..31]));
        // Kind confusion, truncation and garbage are rejected before any lookup.
        let admin = issued.token.replacen("_cli_", "_adm_", 1);
        assert_eq!(parse(&admin).unwrap().kind, TokenKind::Admin);
        for bad in [
            "",
            "msk_",
            "msk_xyz_00_00",
            &issued.token[..issued.token.len() - 2],
            "Bearer abc",
        ] {
            assert!(parse(bad).is_none(), "{bad}");
        }
    }
}
