//! Domain-separated HMAC confirmation digests; independent of session signing.
use super::{AccountChangeKind, AuthAttemptSecret, auth_attempt_key};
use uuid::Uuid;
pub(super) fn confirmation_number() -> u32 {
    let bytes = *Uuid::new_v4().as_bytes();
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) % 1_000_000
}
fn digest(secret: &AuthAttemptSecret, domain: &str, components: &[&[u8]]) -> String {
    auth_attempt_key(secret.expose_secret(), domain, components)
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
pub(super) fn account_change_code_hash(
    secret: &AuthAttemptSecret,
    user_id: Uuid,
    kind: AccountChangeKind,
    code: &str,
) -> String {
    let domain = match kind {
        AccountChangeKind::Email => "confirmation:account-email:v1",
        AccountChangeKind::Password => "confirmation:account-password:v1",
    };
    digest(secret, domain, &[user_id.as_bytes(), code.as_bytes()])
}
pub(super) fn confirmation_code_hash(
    secret: &AuthAttemptSecret,
    canonical_email: &str,
    code: &str,
) -> String {
    digest(
        secret,
        "confirmation:registration:v1",
        &[canonical_email.as_bytes(), code.as_bytes()],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn confirmation_hmac_separates_code_kinds_accounts_and_attempt_budgets() {
        let secret = AuthAttemptSecret::from("independent-confirmation-secret-for-tests");
        let id = Uuid::new_v4();
        let code = "123456";
        let email = account_change_code_hash(&secret, id, AccountChangeKind::Email, code);
        let password = account_change_code_hash(&secret, id, AccountChangeKind::Password, code);
        let registration = confirmation_code_hash(&secret, "alice@example.test", code);
        assert_ne!(email, password);
        assert_ne!(email, registration);
        assert_ne!(password, registration);
        assert_ne!(
            email,
            account_change_code_hash(&secret, Uuid::new_v4(), AccountChangeKind::Email, code)
        );
        assert_ne!(
            registration,
            confirmation_code_hash(&secret, "bob@example.test", code)
        );
        let attempt = auth_attempt_key(
            secret.expose_secret(),
            "registration-code:v1",
            &[b"alice@example.test"],
        );
        let attempt_hex: String = attempt
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_ne!(registration, attempt_hex);
        assert_eq!(registration.len(), 64);
        assert!(!format!("{secret:?}").contains("independent-confirmation-secret-for-tests"));
    }
}
