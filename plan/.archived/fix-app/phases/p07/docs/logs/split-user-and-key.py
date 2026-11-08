from pathlib import Path
import subprocess,textwrap,re
p=Path('src/application/use_cases/user.rs');original=subprocess.check_output(['git','show','HEAD:'+str(p)]).decode();assert p.read_text()==original
lines=original.splitlines(keepends=True)
def span(a,b):return ''.join(lines[a-1:b])
d=p.with_suffix('');d.mkdir()
(d/'ports.rs').write_text('''//! User/account persistence and request contracts.
use async_trait::async_trait;
use secrecy::SecretString;
use uuid::Uuid;
use crate::{app_error::{AppError, AppResult}, entities::{notification::NotificationPreferences, user::User, value_objects::{AvatarUrl, EmailAddress}}};
use super::AuthAttemptKey;
'''+span(65,385))
auth='''//! Authentication digests and admitted-work ownership.
use std::{any::Any, sync::Arc};
use hmac::{Hmac, Mac};
use secrecy::{ExposeSecret, SecretString};
use sha2::Sha256;

/// Independently configured authentication signing key. Debug is redacted by SecretString.
#[derive(Clone, Debug)]
pub struct AuthAttemptSecret(SecretString);
impl AuthAttemptSecret {
    pub(crate) fn expose_secret(&self) -> &str { self.0.expose_secret() }
}
impl From<String> for AuthAttemptSecret {
    fn from(value: String) -> Self { Self(value.into()) }
}
impl From<&str> for AuthAttemptSecret {
    fn from(value: &str) -> Self { value.to_owned().into() }
}
impl From<Arc<str>> for AuthAttemptSecret {
    fn from(value: Arc<str>) -> Self { value.to_string().into() }
}
'''+span(29,63)
(d/'auth.rs').write_text(auth)
confirmation='''//! Registration and confirmation-code issue paths.
use super::*;
'''+span(387,450)+'''\nimpl UserUseCases {\n'''+span(563,653)+'''\n}\n'''
confirmation=confirmation.replace('    pub config: Arc<AppConfig>,\n','').replace('    async fn request(\n','    pub(super) async fn request(\n        &self,\n        signing_key: &AuthAttemptSecret,\n',1).replace('        signing_key: &AuthAttemptSecret,\n        &self,','        signing_key: &AuthAttemptSecret,')
confirmation=confirmation.replace('&self.config,','signing_key,').replace('        let config = &confirmation.config;\n','').replace('confirmation_code_hash(config,','confirmation_code_hash(&self.attempt_secret,').replace('confirmation_code_hash(&confirmation.config,','confirmation_code_hash(&self.attempt_secret,').replace('&self.attempt_secret,\n            "registration-code', 'self.attempt_secret.expose_secret(),\n            "registration-code')
(d/'confirmation.rs').write_text(confirmation)
external='''//! Verified external identity operations.
use super::*;
impl UserUseCases {\n'''+span(733,823)+'\n}\n'+span(1133,1140)
(d/'external_identity.rs').write_text(external)
profile='''//! Profile and credential changes.
use super::*;
impl UserUseCases {\n'''+span(844,1127)+'\n}\n'
profile=profile.replace('.request(id,','.request(&self.attempt_secret, id,').replace('    async fn account(','    pub(super) async fn account(').replace('&account_change_code_hash(&confirmation.config,','&account_change_code_hash(&self.attempt_secret,').replace('&self.attempt_secret,\n            "account-change-code','self.attempt_secret.expose_secret(),\n            "account-change-code')
(d/'profile.rs').write_text(profile)
codes='''//! Domain-separated HMAC confirmation digests; independent of session signing.
use super::{AccountChangeKind, AuthAttemptSecret, auth_attempt_key};
use uuid::Uuid;
pub(super) fn confirmation_number() -> u32 {
    let bytes = *Uuid::new_v4().as_bytes();
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) % 1_000_000
}
fn digest(secret: &AuthAttemptSecret, domain: &str, components: &[&[u8]]) -> String {
    auth_attempt_key(secret.expose_secret(), domain, components).as_bytes().iter().map(|byte| format!("{byte:02x}")).collect()
}
pub(super) fn account_change_code_hash(secret: &AuthAttemptSecret, user_id: Uuid, kind: AccountChangeKind, code: &str) -> String {
    let domain = match kind {
        AccountChangeKind::Email => "confirmation:account-email:v1",
        AccountChangeKind::Password => "confirmation:account-password:v1",
    };
    digest(secret, domain, &[user_id.as_bytes(), code.as_bytes()])
}
pub(super) fn confirmation_code_hash(secret: &AuthAttemptSecret, canonical_email: &str, code: &str) -> String {
    digest(secret, "confirmation:registration:v1", &[canonical_email.as_bytes(), code.as_bytes()])
}
'''
(d/'codes.rs').write_text(codes)
header='''//! Account orchestration; ports, identity, confirmation and profile operations have their own modules.
use std::sync::Arc;
use secrecy::{ExposeSecret, SecretString};
use tokio::sync::Semaphore;
use tracing::{info, instrument};
use uuid::Uuid;
use crate::{app_error::{AppError, AppResult}, entities::{notification::NotificationPreferences, user::User, value_objects::{AvatarUrl, EmailAddress}}};
mod auth;
mod codes;
mod confirmation;
mod external_identity;
mod ports;
mod profile;
pub use auth::{AuthAttemptKey, AuthAttemptSecret, AuthWorkLifetime};
pub(crate) use auth::auth_attempt_key;
pub use confirmation::EmailConfirmation;
pub use ports::*;
use codes::{account_change_code_hash, confirmation_code_hash, confirmation_number};
#[cfg(test)]
mod tests;
'''
body=span(23,27)+span(452,485)+'\nimpl UserUseCases {\n'+span(488,561)+span(655,731)+span(825,842)+'\n}\n'+span(1142,1154)
body=body.replace('attempt_secret: Arc<str>','attempt_secret: AuthAttemptSecret').replace('attempt_secret: impl Into<Arc<str>>','attempt_secret: impl Into<AuthAttemptSecret>').replace('&self.attempt_secret,\n','self.attempt_secret.expose_secret(),\n')
(d/'mod.rs').write_text(header+body)
test=textwrap.dedent(span(1194,2411))
test='use std::net::IpAddr;\nuse crate::infra::config::AppConfig;\n'+test
# The old confirmation object used JWT config only for digests. The user now owns its one key.
test=re.sub(r'(?m)^\s*config: (?:config(?:\.clone\(\))?|Arc::new\(AppConfig::for_test\(\)\)),\n','\n',test)
(d/'tests.rs').write_text(test)
p.unlink()
print('User module split; stable signing key installed in issue/confirm paths')
