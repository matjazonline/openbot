//! `/ui/profile` — the signed-in account's own details.
//!
//! The one `/ui` workspace that is about the reader rather than about a company, so it is reached
//! from the account menu and needs no `company_id`: the company it hands the shell is only what
//! the rail points at, and an account with none still gets its settings.
//!
//! A new address and a new password are both mailed a code before they take effect, so every
//! handler here re-reads what the account is *and* what it is waiting on before rendering — the
//! pane is a picture of stored state, never of what a form hoped to store.

use std::sync::Arc;

use axum::{
    Form, Router,
    extract::{Path, Query, State},
    response::Html,
    routing::{get, post, put},
};
use secrecy::SecretString;
use serde::Deserialize;
use tracing::instrument;
use uuid::Uuid;

use crate::{
    adapters::http::{app_state::AppState, auth::AuthenticatedUser, pages},
    app_error::{AppError, AppResult},
    entities::{
        user::User,
        value_objects::{AvatarUrl, EmailAddress},
    },
    use_cases::{
        company::CompanyUseCases,
        user::{
            AccountChangeKind, PasswordChange, PasswordOutcome, PendingChange, ProfileUpdate,
            UserUseCases,
        },
    },
};

use super::ui::{load_account, load_readable_company, workspace_user};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ui/profile", get(profile_page).put(update_profile))
        .route("/ui/profile/password", put(change_password))
        .route(
            "/ui/profile/changes/{kind}",
            post(confirm_change).delete(cancel_change),
        )
}

/// Only which company the rail should point at, so arriving at `/ui/profile` bare is valid.
#[derive(Debug, Deserialize)]
struct ProfileQuery {
    company_id: Option<Uuid>,
}

/// The account-details form. Blank `avatar_url` means "back to the letter bubble", which is why it
/// is a plain `String` rather than an `Option`.
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

/// The six digits mailed out for a pending change.
#[derive(Debug, Clone, Deserialize)]
pub struct CodeForm {
    pub code: String,
}

/// The account and everything waiting on a code for it, which is what any pane render needs.
///
/// Loaded together because they are one answer: a pane showing the stored address but not the
/// address change waiting beside it would invite the reader to ask for the same thing twice.
struct Account {
    user: User,
    pending: Vec<PendingChange>,
}

impl Account {
    async fn load(user_use_cases: &UserUseCases, user_id: Uuid) -> AppResult<Self> {
        Ok(Self {
            user: load_account(user_use_cases, user_id).await?,
            pending: user_use_cases.pending_account_changes(user_id).await?,
        })
    }

    fn pane(
        &self,
        draft: Option<&pages::ProfileDraft<'_>>,
        outcome: pages::ProfileOutcome<'_>,
    ) -> String {
        pages::profile_pane(&pages::ProfilePane {
            user: &self.user,
            draft,
            pending: &self.pending,
            outcome,
        })
    }

    /// The pane, plus the top bar's account chip out of band -- a rename, a new picture or a
    /// confirmed address has to reach the chrome as well as the form that made it.
    fn pane_with_chip(&self, outcome: pages::ProfileOutcome<'_>) -> Html<String> {
        let email = EmailAddress::from(self.user.email.as_str());

        Html(format!(
            "{}{}",
            self.pane(None, outcome),
            pages::account_chip(
                &workspace_user(&self.user, &email),
                pages::FragmentSwap::OutOfBand,
            ),
        ))
    }
}

/// GET /ui/profile - The account's own settings (Protected).
#[instrument(skip(company_use_cases, user_use_cases, user))]
async fn profile_page(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(user_use_cases): State<Arc<UserUseCases>>,
    user: AuthenticatedUser,
    Query(query): Query<ProfileQuery>,
) -> AppResult<Html<String>> {
    let account = Account::load(&user_use_cases, user.id).await?;
    let (_, company) = load_readable_company(&company_use_cases, user.id, query.company_id).await?;

    let pane_html = account.pane(None, pages::ProfileOutcome::Untouched);
    let account_email = EmailAddress::from(account.user.email.as_str());

    Ok(Html(pages::profile_page(&pages::ProfilePage {
        user: &workspace_user(&account.user, &account_email),
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
        Err(message) => {
            return rejected(
                &user_use_cases,
                user.id,
                pages::ProfileForm::Identity,
                Some(&draft),
                &message,
            )
            .await;
        }
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

    let saved = match saved {
        Ok(saved) => saved,
        Err(err) => {
            return rejected(
                &user_use_cases,
                user.id,
                pages::ProfileForm::Identity,
                Some(&draft),
                &write_error(&err, "save your details"),
            )
            .await;
        }
    };

    // Re-read rather than trust the write: the address only moves once its code comes back, so
    // what the pane shows has to come from the store and the pending list together.
    let account = Account::load(&user_use_cases, user.id).await?;
    let message = match &saved.pending {
        Some(PendingChange::Email { new_email, .. }) => format!(
            "Your name and picture are saved. Check {new_email} for the code that moves your address there."
        ),
        _ => "Your account details are saved.".to_string(),
    };

    Ok(account.pane_with_chip(pages::ProfileOutcome::Saved(
        pages::ProfileForm::Identity,
        &message,
    )))
}

/// PUT /ui/profile/password - Ask to replace the account's password (Protected).
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

    let message = match changed {
        Ok(PasswordOutcome::Changed) => "Your password has been changed.".to_string(),
        Ok(PasswordOutcome::ConfirmationSent(_)) => format!(
            "Check {} for the code that finishes the change.",
            load_account(&user_use_cases, user.id).await?.email
        ),
        // A wrong current password is worded here rather than by `write_error`: `InvalidCredentials`
        // renders as "Invalid credentials", which in a form with three password fields does not say
        // which one was wrong.
        Err(AppError::InvalidCredentials) => {
            return rejected(
                &user_use_cases,
                user.id,
                pages::ProfileForm::Password,
                None,
                "That is not your current password.",
            )
            .await;
        }
        Err(err) => {
            return rejected(
                &user_use_cases,
                user.id,
                pages::ProfileForm::Password,
                None,
                &write_error(&err, "change your password"),
            )
            .await;
        }
    };

    let account = Account::load(&user_use_cases, user.id).await?;
    Ok(Html(account.pane(
        None,
        pages::ProfileOutcome::Saved(pages::ProfileForm::Password, &message),
    )))
}

/// POST /ui/profile/changes/{kind} - Turn a mailed code into the change it was sent for
/// (Protected).
#[instrument(skip(user_use_cases, user, form))]
async fn confirm_change(
    State(user_use_cases): State<Arc<UserUseCases>>,
    user: AuthenticatedUser,
    Path(kind): Path<String>,
    Form(form): Form<CodeForm>,
) -> AppResult<Html<String>> {
    let kind = change_kind(&kind)?;
    let section = section_of(kind);

    let confirmed = user_use_cases
        .confirm_account_change(user.id, kind, form.code.trim())
        .await;

    let Err(err) = confirmed else {
        let account = Account::load(&user_use_cases, user.id).await?;
        let message = match kind {
            AccountChangeKind::Email => format!("Your address is now {}.", account.user.email),
            AccountChangeKind::Password => "Your password has been changed.".to_string(),
        };

        // The chip rides along either way: a confirmed address rewrites what the bar says the
        // reader is signed in as.
        return Ok(account.pane_with_chip(pages::ProfileOutcome::Saved(section, &message)));
    };

    rejected(
        &user_use_cases,
        user.id,
        section,
        None,
        &write_error(&err, "confirm that code"),
    )
    .await
}

/// DELETE /ui/profile/changes/{kind} - Abandon a requested change (Protected).
///
/// No banner: the section coming back as its form is what says the request is gone, and a green
/// "cancelled" would read as something having been saved.
#[instrument(skip(user_use_cases, user))]
async fn cancel_change(
    State(user_use_cases): State<Arc<UserUseCases>>,
    user: AuthenticatedUser,
    Path(kind): Path<String>,
) -> AppResult<Html<String>> {
    user_use_cases
        .discard_account_change(user.id, change_kind(&kind)?)
        .await?;

    let account = Account::load(&user_use_cases, user.id).await?;
    Ok(Html(account.pane(None, pages::ProfileOutcome::Untouched)))
}

/// Which field a `{kind}` path segment names. Anything else is a hand-written URL, not a form.
fn change_kind(kind: &str) -> AppResult<AccountChangeKind> {
    AccountChangeKind::parse(kind)
        .ok_or_else(|| AppError::NotFound(format!("No such account change: {kind}")))
}

/// Which section of the pane a change belongs to, so its answer lands under the form that asked.
fn section_of(kind: AccountChangeKind) -> pages::ProfileForm {
    match kind {
        AccountChangeKind::Email => pages::ProfileForm::Identity,
        AccountChangeKind::Password => pages::ProfileForm::Password,
    }
}

/// The pane after a refused write: the stored account, showing what was submitted and why it was
/// not kept.
///
/// The stored account is re-read rather than assumed, so the header above the form cannot claim a
/// name the refusal means it does not have.
async fn rejected(
    user_use_cases: &UserUseCases,
    user_id: Uuid,
    section: pages::ProfileForm,
    draft: Option<&pages::ProfileDraft<'_>>,
    message: &str,
) -> AppResult<Html<String>> {
    let account = Account::load(user_use_cases, user_id).await?;

    Ok(Html(account.pane(
        draft,
        pages::ProfileOutcome::Rejected(section, message),
    )))
}

/// What a refused write says, for every form on this page.
///
/// A taken name, a malformed address, a too-short password and a stale code are the reader's to
/// fix and arrive already worded; anything else is ours, and `attempted` is what it names so a
/// database fault is not reported as a form the reader filled in wrong.
fn write_error(error: &AppError, attempted: &str) -> String {
    match error {
        AppError::BadRequest(message) | AppError::Conflict(message) => message.clone(),
        other => format!("Failed to {attempted}: {other}"),
    }
}
