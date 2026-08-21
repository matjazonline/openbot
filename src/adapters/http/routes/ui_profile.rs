//! `/ui/profile` — the signed-in account's own details.
//!
//! The one `/ui` workspace that is about the reader rather than about a company, so it is reached
//! from the account menu and needs no `company_id`: the company it hands the shell is only what
//! the rail points at, and an account with none still gets its settings.
//!
//! Both writes re-render the whole pane, and both send the top bar's avatar along out of band --
//! a renamed or repictured account has to reach the chrome that no pane swap would otherwise
//! touch.

use std::sync::Arc;

use axum::{
    Form, Router,
    extract::{Query, State},
    response::Html,
    routing::{get, put},
};
use secrecy::SecretString;
use serde::Deserialize;
use tracing::instrument;

use crate::{
    adapters::http::{app_state::AppState, auth::AuthenticatedUser, pages},
    app_error::{AppError, AppResult},
    entities::{
        user::User,
        value_objects::{AvatarUrl, EmailAddress},
    },
    use_cases::{
        company::CompanyUseCases,
        user::{PasswordChange, ProfileUpdate, UserUseCases},
    },
};

use super::ui::{load_account, load_readable_company, workspace_user};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ui/profile", get(profile_page).put(update_profile))
        .route("/ui/profile/password", put(change_password))
}

/// Only which company the rail should point at, so arriving at `/ui/profile` bare is valid.
#[derive(Debug, Deserialize)]
struct ProfileQuery {
    company_id: Option<uuid::Uuid>,
}

/// The account-details form. Blank `avatar_url` means "back to the letter bubble", which is why it is a
/// plain `String` rather than an `Option`.
#[derive(Debug, Clone, Deserialize)]
pub struct IdentityForm {
    pub username: String,
    pub email: String,
    pub avatar_url: String,
}

/// The password form. Never echoed back into the re-rendered page — see
/// [`pages::profile_pane`]'s password section.
#[derive(Deserialize)]
pub struct PasswordForm {
    pub current_password: SecretString,
    pub new_password: SecretString,
    pub confirm_password: SecretString,
}

/// GET /ui/profile - The account's own settings (Protected).
#[instrument(skip(company_use_cases, user_use_cases, user))]
async fn profile_page(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(user_use_cases): State<Arc<UserUseCases>>,
    user: AuthenticatedUser,
    Query(query): Query<ProfileQuery>,
) -> AppResult<Html<String>> {
    let account = load_account(&user_use_cases, user.id).await?;
    let (_, company) = load_readable_company(&company_use_cases, user.id, query.company_id).await?;

    let pane_html = pages::profile_pane(&pages::ProfilePane {
        user: &account,
        draft: None,
        outcome: pages::ProfileOutcome::Untouched,
    });
    let account_email = EmailAddress::from(account.email.as_str());

    Ok(Html(pages::profile_page(&pages::ProfilePage {
        user: &workspace_user(&account, &account_email),
        company: company.as_ref().map(|access| &access.company),
        pane_html: &pane_html,
    })))
}

/// PUT /ui/profile - Save the account's picture, name and address (Protected).
///
/// The account is always the caller's own: there is no id in the URL to point somewhere else.
#[instrument(skip(user_use_cases, user, form))]
async fn update_profile(
    State(user_use_cases): State<Arc<UserUseCases>>,
    user: AuthenticatedUser,
    Form(form): Form<IdentityForm>,
) -> AppResult<Html<String>> {
    let draft = pages::ProfileDraft {
        username: &form.username,
        email: &form.email,
        avatar_url: &form.avatar_url,
    };

    let avatar_url = match AvatarUrl::parse(&form.avatar_url) {
        Ok(avatar_url) => avatar_url,
        Err(message) => return rejected(&user_use_cases, user.id, &draft, &message).await,
    };

    let saved = user_use_cases
        .update_profile(
            user.id,
            ProfileUpdate {
                username: &form.username,
                email: &EmailAddress::from(form.email.as_str()),
                avatar_url: avatar_url.as_ref(),
            },
        )
        .await;

    match saved {
        Ok(saved) => Ok(saved_pane(
            &saved,
            pages::ProfileForm::Identity,
            "Your account details are saved.",
        )),
        Err(err) => {
            rejected(
                &user_use_cases,
                user.id,
                &draft,
                &write_error(&err, "save your details"),
            )
            .await
        }
    }
}

/// PUT /ui/profile/password - Replace the account's password (Protected).
#[instrument(skip(user_use_cases, user, form))]
async fn change_password(
    State(user_use_cases): State<Arc<UserUseCases>>,
    user: AuthenticatedUser,
    Form(form): Form<PasswordForm>,
) -> AppResult<Html<String>> {
    let changed = user_use_cases
        .change_password(
            user.id,
            PasswordChange {
                current: &form.current_password,
                new: &form.new_password,
                confirmation: &form.confirm_password,
            },
        )
        .await;

    // Re-read rather than reuse whatever the form was rendered from: the pane above the password
    // section shows the stored account, and this handler did not change it.
    let account = load_account(&user_use_cases, user.id).await?;

    // Bound before the match so the borrowed message outlives the outcome that points at it.
    let refusal = match &changed {
        Ok(()) => String::new(),
        Err(AppError::InvalidCredentials) => "That is not your current password.".to_string(),
        Err(err) => write_error(err, "change your password"),
    };
    let outcome = match &changed {
        Ok(()) => pages::ProfileOutcome::Saved(
            pages::ProfileForm::Password,
            "Your password has been changed.",
        ),
        Err(_) => pages::ProfileOutcome::Rejected(pages::ProfileForm::Password, &refusal),
    };

    Ok(Html(pages::profile_pane(&pages::ProfilePane {
        user: &account,
        draft: None,
        outcome,
    })))
}

/// The pane after a write, with the top bar's account chip riding along -- a rename or a new
/// picture has to reach the chrome as well as the form that made it.
fn saved_pane(saved: &User, form: pages::ProfileForm, message: &str) -> Html<String> {
    let saved_email = EmailAddress::from(saved.email.as_str());

    Html(format!(
        "{}{}",
        pages::profile_pane(&pages::ProfilePane {
            user: saved,
            draft: None,
            outcome: pages::ProfileOutcome::Saved(form, message),
        }),
        pages::account_chip(
            &workspace_user(saved, &saved_email),
            pages::FragmentSwap::OutOfBand,
        ),
    ))
}

/// The pane after a refused save: the stored account, showing what was submitted and why it was
/// not kept.
///
/// The stored account is re-read rather than assumed, so the header above the form cannot claim a
/// name the refusal means it does not have.
async fn rejected(
    user_use_cases: &UserUseCases,
    user_id: uuid::Uuid,
    draft: &pages::ProfileDraft<'_>,
    message: &str,
) -> AppResult<Html<String>> {
    let account = load_account(user_use_cases, user_id).await?;

    Ok(Html(pages::profile_pane(&pages::ProfilePane {
        user: &account,
        draft: Some(draft),
        outcome: pages::ProfileOutcome::Rejected(pages::ProfileForm::Identity, message),
    })))
}

/// What a refused write says, for both of this page's forms.
///
/// A taken name, a malformed address and a too-short password are the reader's to fix and arrive
/// already worded; anything else is ours, and `attempted` is what it names so a database fault is
/// not reported as a form the reader filled in wrong.
fn write_error(error: &AppError, attempted: &str) -> String {
    match error {
        AppError::BadRequest(message) | AppError::Conflict(message) => message.clone(),
        other => format!("Failed to {attempted}: {other}"),
    }
}
