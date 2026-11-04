//! Native issue/confirm through the actual production account-construction helper.
use super::{PostgresPersistence, test_support::own_database};
use crate::app_error::AppResult;
use crate::entities::value_objects::EmailAddress;
use crate::infra::config::AppConfig;
use crate::use_cases::user::{
    AccountChangeKind, ConfirmationCodeSender, ConfirmationPurpose, EmailConfirmation,
    PasswordChange, ProfileUpdate, RegistrationOutcome, UserUseCases,
};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use uuid::Uuid;
#[derive(Default)]
struct Codes(Mutex<Vec<String>>);
impl Codes {
    fn last(&self) -> String {
        self.0.lock().unwrap().last().expect("mailed code").clone()
    }
}
#[async_trait]
impl ConfirmationCodeSender for Codes {
    async fn send_code(
        &self,
        _: &EmailAddress,
        code: &str,
        _: ConfirmationPurpose,
    ) -> AppResult<()> {
        self.0.lock().unwrap().push(code.into());
        Ok(())
    }
}
fn cases(
    config: &AppConfig,
    persistence: &Arc<PostgresPersistence>,
    codes: &Arc<Codes>,
) -> UserUseCases {
    crate::infra::setup::user_use_cases(
        config,
        persistence.clone(),
        EmailConfirmation {
            registrations: persistence.clone(),
            account_changes: persistence.clone(),
            codes: codes.clone(),
            transport_available: true,
            allow_unverified_without_smtp: false,
        },
    )
}
struct Registration {
    email: EmailAddress,
    code: String,
}
struct Change {
    user: Uuid,
    code: String,
}
struct Issued {
    registration: Registration,
    email: Change,
    new_email: EmailAddress,
    password: Change,
    password_email: EmailAddress,
}
async fn issue(cases: &UserUseCases, codes: &Codes) -> Issued {
    let suffix = Uuid::new_v4().simple();
    let email = EmailAddress::from(format!("registration-{suffix}@example.test"));
    assert!(matches!(
        cases
            .register(
                &format!("registration_{suffix}"),
                &email,
                &"old-password".into(),
                None
            )
            .await
            .unwrap(),
        RegistrationOutcome::ConfirmationSent
    ));
    let registration = Registration {
        email,
        code: codes.last(),
    };
    let old_email = EmailAddress::from(format!("before-{suffix}@example.test"));
    let email_owner = cases
        .add(
            &format!("email_{suffix}"),
            &old_email,
            &"old-password".into(),
            None,
        )
        .await
        .unwrap();
    let new_email = EmailAddress::from(format!("after-{suffix}@example.test"));
    cases
        .update_profile(
            email_owner.id,
            ProfileUpdate {
                username: &email_owner.username,
                email: &new_email,
                avatar_url: None,
            },
        )
        .await
        .unwrap();
    let email = Change {
        user: email_owner.id,
        code: codes.last(),
    };
    let password_email = EmailAddress::from(format!("password-{suffix}@example.test"));
    let password_owner = cases
        .add(
            &format!("password_{suffix}"),
            &password_email,
            &"old-password".into(),
            None,
        )
        .await
        .unwrap();
    cases
        .change_password(
            password_owner.id,
            PasswordChange {
                current: &"old-password".into(),
                new: &"new-password".into(),
                confirmation: &"new-password".into(),
            },
            None,
        )
        .await
        .unwrap();
    let password = Change {
        user: password_owner.id,
        code: codes.last(),
    };
    Issued {
        registration,
        email,
        new_email,
        password,
        password_email,
    }
}
#[tokio::test]
async fn auth_key_rotation_pending_codes_survive_jwt_rotation_with_the_independent_key() {
    let database = own_database()
        .await
        .expect("native auth key test requires PostgreSQL");
    let persistence = Arc::new(PostgresPersistence::new(database.pool.clone()));
    let codes = Arc::new(Codes::default());
    let before = AppConfig::for_test();
    let issued = issue(&cases(&before, &persistence, &codes), &codes).await;
    let mut after = AppConfig::for_test();
    after.jwt_secret = "a-rotated-and-independent-session-signing-key".into();
    after.auth_attempt_secret = before.auth_attempt_secret.clone();
    assert_ne!(before.jwt_secret, after.jwt_secret);
    let mut wrong = AppConfig::for_test();
    wrong.jwt_secret = after.jwt_secret.clone();
    wrong.auth_attempt_secret = "a-different-confirmation-signing-key-value".into();
    assert!(
        cases(&wrong, &persistence, &codes)
            .confirm_registration(&issued.registration.email, &issued.registration.code)
            .await
            .is_err()
    );
    let stable = cases(&after, &persistence, &codes);
    let registered = stable
        .confirm_registration(&issued.registration.email, &issued.registration.code)
        .await
        .unwrap();
    assert!(registered.email_verified_at.is_some());
    let email = stable
        .confirm_account_change(
            issued.email.user,
            AccountChangeKind::Email,
            &issued.email.code,
        )
        .await
        .unwrap();
    assert_eq!(email.email, issued.new_email.as_str());
    assert!(email.email_verified_at.is_some());
    stable
        .confirm_account_change(
            issued.password.user,
            AccountChangeKind::Password,
            &issued.password.code,
        )
        .await
        .unwrap();
    assert!(
        stable
            .login(&issued.password_email, &"new-password".into(), None, None)
            .await
            .is_ok()
    );
    assert!(
        stable
            .confirm_registration(&issued.registration.email, &issued.registration.code)
            .await
            .is_err(),
        "one code is spent once"
    );
}
