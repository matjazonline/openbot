//! Database-backed tests for one company's Resend row.
//!
//! Everything worth asserting here needs a real database: the ownership scoping is in the SQL, the
//! "keep the stored secret" contract is a read taken under the same lock as the write, and whether
//! an envelope opens at all depends on the exact row it was sealed against.

use secrecy::{ExposeSecret, SecretString};

use super::*;
use crate::{
    adapters::persistence::{credentials::CredentialCipher, test_support::test_pool},
    use_cases::{
        company::{CompanyPersistence, CompanyWrite},
        user::UserPersistence,
    },
};

const API_KEY: &str = "re_test_key";
const SIGNING_SECRET: &str = "whsec_MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw";

/// A persistence with a cipher, an account, and a company that account owns.
async fn fixture() -> Option<(PostgresPersistence, Uuid, Uuid)> {
    let pool = test_pool().await?;
    let persistence =
        PostgresPersistence::with_credential_cipher(pool, CredentialCipher::for_test());
    let user_id = create_account(&persistence).await;
    let company = persistence
        .create(
            user_id,
            CompanyWrite {
                name: "Acme".to_string(),
                slug: format!("acme-{}", Uuid::new_v4().simple()),
                ..CompanyWrite::default()
            },
        )
        .await
        .expect("a company");
    Some((persistence, user_id, company.id))
}

async fn create_account(persistence: &PostgresPersistence) -> Uuid {
    let username = format!("resend_{}", Uuid::new_v4().simple());
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .expect("an account");
    persistence
        .get_by_email(&email)
        .await
        .expect("a lookup")
        .expect("the account just created")
        .id
}

fn write(api_key: Option<&str>, signing_secret: Option<&str>) -> CompanyResendApiIntegrationWrite {
    CompanyResendApiIntegrationWrite {
        api_key: api_key.map(|value| SecretString::from(value.to_string())),
        signing_secret: signing_secret.map(|value| SecretString::from(value.to_string())),
        authserv_id: AuthservId::new("resend.com"),
        enabled: true,
    }
}

#[tokio::test]
async fn a_first_write_mints_a_token_and_a_second_one_keeps_it() {
    let Some((persistence, user_id, company_id)) = fixture().await else {
        return;
    };

    let connected = persistence
        .upsert_integration_for_user(
            user_id,
            company_id,
            write(Some(API_KEY), Some(SIGNING_SECRET)),
        )
        .await
        .expect("the integration is stored");
    assert_eq!(
        connected.webhook_token.as_str().len(),
        ResendApiWebhookToken::LENGTH
    );

    // Saving again is not re-registering an endpoint: the URL an operator already pasted into
    // Resend has to survive every ordinary save.
    let saved_again = persistence
        .upsert_integration_for_user(user_id, company_id, write(None, None))
        .await
        .expect("the integration is updated");
    assert_eq!(saved_again.webhook_token, connected.webhook_token);
}

#[tokio::test]
async fn a_blank_secret_keeps_the_stored_one_and_a_new_one_replaces_it() {
    let Some((persistence, user_id, company_id)) = fixture().await else {
        return;
    };
    persistence
        .upsert_integration_for_user(
            user_id,
            company_id,
            write(Some(API_KEY), Some(SIGNING_SECRET)),
        )
        .await
        .expect("the integration is stored");

    persistence
        .upsert_integration_for_user(user_id, company_id, write(None, None))
        .await
        .expect("a save that changes neither secret");
    let kept = persistence
        .account_credentials(company_id)
        .await
        .expect("a credential read")
        .expect("an enabled integration");
    assert_eq!(kept.api_key.expose_secret(), API_KEY);

    persistence
        .upsert_integration_for_user(user_id, company_id, write(Some("re_rotated"), None))
        .await
        .expect("a save that replaces the API key");
    let replaced = persistence
        .account_credentials(company_id)
        .await
        .expect("a credential read")
        .expect("an enabled integration");
    assert_eq!(replaced.api_key.expose_secret(), "re_rotated");
}

#[tokio::test]
async fn connecting_without_both_secrets_is_refused_rather_than_half_stored() {
    let Some((persistence, user_id, company_id)) = fixture().await else {
        return;
    };

    let error = persistence
        .upsert_integration_for_user(user_id, company_id, write(Some(API_KEY), None))
        .await
        .expect_err("a first write with no signing secret");
    assert!(matches!(error, AppError::BadRequest(_)), "{error:?}");

    // The rolled-back attempt left nothing behind, token included.
    assert!(
        persistence
            .integration_for_user(user_id, company_id)
            .await
            .expect("a settings read")
            .is_none()
    );
}

#[tokio::test]
async fn the_webhook_token_finds_the_signing_secret_of_its_own_company_only() {
    let Some((persistence, user_id, company_id)) = fixture().await else {
        return;
    };
    let integration = persistence
        .upsert_integration_for_user(
            user_id,
            company_id,
            write(Some(API_KEY), Some(SIGNING_SECRET)),
        )
        .await
        .expect("the integration is stored");

    let found = persistence
        .inbound_credentials(&integration.webhook_token)
        .await
        .expect("a token lookup")
        .expect("the company that owns the token");
    assert_eq!(found.company_id, company_id);
    assert_eq!(found.signing_secret.expose_secret(), SIGNING_SECRET);

    // A token nobody holds resolves to nobody, rather than to whoever happens to sort first.
    assert!(
        persistence
            .inbound_credentials(&ResendApiWebhookToken::generate())
            .await
            .expect("a token lookup")
            .is_none()
    );
}

#[tokio::test]
async fn a_disabled_integration_answers_neither_runtime_lookup_but_stays_in_settings() {
    let Some((persistence, user_id, company_id)) = fixture().await else {
        return;
    };
    let integration = persistence
        .upsert_integration_for_user(
            user_id,
            company_id,
            write(Some(API_KEY), Some(SIGNING_SECRET)),
        )
        .await
        .expect("the integration is stored");
    persistence
        .upsert_integration_for_user(
            user_id,
            company_id,
            CompanyResendApiIntegrationWrite {
                enabled: false,
                ..write(None, None)
            },
        )
        .await
        .expect("the integration is switched off");

    assert!(
        persistence
            .inbound_credentials(&integration.webhook_token)
            .await
            .expect("a token lookup")
            .is_none()
    );
    assert!(
        persistence
            .account_credentials(company_id)
            .await
            .expect("a credential read")
            .is_none()
    );
    // Its owner still sees it, still switched off, with the credentials intact behind it.
    let settings = persistence
        .integration_for_user(user_id, company_id)
        .await
        .expect("a settings read")
        .expect("the integration");
    assert!(!settings.enabled);
    assert_eq!(settings.webhook_token, integration.webhook_token);
}

#[tokio::test]
async fn rotating_replaces_the_token_and_the_old_one_stops_resolving() {
    let Some((persistence, user_id, company_id)) = fixture().await else {
        return;
    };
    let first = persistence
        .upsert_integration_for_user(
            user_id,
            company_id,
            write(Some(API_KEY), Some(SIGNING_SECRET)),
        )
        .await
        .expect("the integration is stored");

    let rotated = persistence
        .rotate_webhook_token_for_user(user_id, company_id)
        .await
        .expect("a rotation");
    assert_ne!(rotated.webhook_token, first.webhook_token);
    assert!(
        persistence
            .inbound_credentials(&first.webhook_token)
            .await
            .expect("a token lookup")
            .is_none()
    );
    assert!(
        persistence
            .inbound_credentials(&rotated.webhook_token)
            .await
            .expect("a token lookup")
            .is_some()
    );
    // Rotating the URL is not re-entering the credentials.
    assert!(
        persistence
            .account_credentials(company_id)
            .await
            .expect("a credential read")
            .is_some()
    );
}

#[tokio::test]
async fn nobody_but_the_owner_can_read_write_rotate_or_delete_the_integration() {
    let Some((persistence, user_id, company_id)) = fixture().await else {
        return;
    };
    persistence
        .upsert_integration_for_user(
            user_id,
            company_id,
            write(Some(API_KEY), Some(SIGNING_SECRET)),
        )
        .await
        .expect("the integration is stored");
    let stranger = create_account(&persistence).await;

    assert!(
        persistence
            .integration_for_user(stranger, company_id)
            .await
            .expect("a settings read")
            .is_none()
    );
    assert!(
        persistence
            .upsert_integration_for_user(
                stranger,
                company_id,
                write(Some("re_stolen"), Some(SIGNING_SECRET))
            )
            .await
            .is_err()
    );
    assert!(
        persistence
            .rotate_webhook_token_for_user(stranger, company_id)
            .await
            .is_err()
    );
    assert!(
        !persistence
            .delete_integration_for_user(stranger, company_id)
            .await
            .expect("a delete attempt")
    );

    // None of that touched the owner's credential.
    let credentials = persistence
        .account_credentials(company_id)
        .await
        .expect("a credential read")
        .expect("an enabled integration");
    assert_eq!(credentials.api_key.expose_secret(), API_KEY);
}

#[tokio::test]
async fn disconnecting_forgets_the_row_and_both_of_its_secrets() {
    let Some((persistence, user_id, company_id)) = fixture().await else {
        return;
    };
    let integration = persistence
        .upsert_integration_for_user(
            user_id,
            company_id,
            write(Some(API_KEY), Some(SIGNING_SECRET)),
        )
        .await
        .expect("the integration is stored");

    assert!(
        persistence
            .delete_integration_for_user(user_id, company_id)
            .await
            .expect("a delete")
    );
    assert!(
        persistence
            .integration_for_user(user_id, company_id)
            .await
            .expect("a settings read")
            .is_none()
    );
    assert!(
        persistence
            .inbound_credentials(&integration.webhook_token)
            .await
            .expect("a token lookup")
            .is_none()
    );
    // Deleting what is not there is not an error; it is the same answer twice.
    assert!(
        !persistence
            .delete_integration_for_user(user_id, company_id)
            .await
            .expect("a second delete")
    );
}

#[tokio::test]
async fn a_credential_moved_between_companies_or_columns_does_not_open() {
    let Some((persistence, user_id, company_id)) = fixture().await else {
        return;
    };
    let Some((_, other_user, other_company)) = fixture().await else {
        return;
    };
    persistence
        .upsert_integration_for_user(
            user_id,
            company_id,
            write(Some(API_KEY), Some(SIGNING_SECRET)),
        )
        .await
        .expect("the integration is stored");
    persistence
        .upsert_integration_for_user(
            other_user,
            other_company,
            write(Some("re_other"), Some(SIGNING_SECRET)),
        )
        .await
        .expect("a second company's integration");

    // The envelope is authenticated against (company, kind), so copying one company's ciphertext
    // into another's row -- the thing an operator with SQL access can trivially do -- produces a
    // row that fails to open rather than one that quietly yields the original key.
    sqlx::query(
        "UPDATE company_resend_api_integrations SET api_key = (
             SELECT api_key FROM company_resend_api_integrations WHERE company_id = $2
         ) WHERE company_id = $1",
    )
    .bind(other_company)
    .bind(company_id)
    .execute(persistence.pool())
    .await
    .expect("the ciphertext is moved");

    assert!(
        persistence
            .account_credentials(other_company)
            .await
            .is_err(),
        "a misplaced envelope must fail loudly rather than decrypt"
    );
}
