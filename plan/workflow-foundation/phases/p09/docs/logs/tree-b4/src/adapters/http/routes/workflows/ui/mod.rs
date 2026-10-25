//! The browser workspaces over the workflow commands (p09): the same lifecycle commands the
//! JSON routes call, rendered as server-side HTML for htmx. A page is scoped to a company the
//! caller owns or administers (`load_managed_company`), and every command authorizes again.
use super::*;
use crate::adapters::http::pages;
use crate::adapters::http::routes::ui::{
    load_account, load_managed_company, managed_company_membership, workspace_user,
};
use crate::entities::company::Company;
use crate::entities::value_objects::EmailAddress;
use crate::infra::config::AppConfig;
use crate::use_cases::{agent::AgentUseCases, company::CompanyUseCases, user::UserUseCases};
use axum::{
    extract::FromRequestParts,
    http::{HeaderValue, request::Parts},
    response::Html,
};

mod authoring;
mod runs;

pub fn router() -> Router<AppState> {
    authoring::router().merge(runs::router())
}

/// What every workspace handler starts from: the engine, the use cases its pages read, and the
/// caller.
pub(super) struct Ui {
    engine: Engine,
    companies: Arc<CompanyUseCases>,
    users: Arc<UserUseCases>,
    agents: Arc<AgentUseCases>,
    config: Arc<AppConfig>,
    user: Uuid,
}

impl FromRequestParts<AppState> for Ui {
    type Rejection = crate::adapters::http::auth::AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = AuthenticatedUser::from_request_parts(parts, state).await?;
        Ok(Self {
            engine: state.workflow.clone(),
            companies: state.company_use_cases.clone(),
            users: state.user_use_cases.clone(),
            agents: state.agent_use_cases.clone(),
            config: state.config.clone(),
            user: user.id,
        })
    }
}

impl Ui {
    fn actor(&self) -> AppResult<WorkflowActor> {
        WorkflowActor::authenticated(self.user)
    }

    /// A company the caller owns or administers; any other id is not found.
    async fn company(&self, company_id: Uuid) -> AppResult<Company> {
        let (_, company) =
            load_managed_company(&self.companies, self.user, Some(company_id)).await?;
        company.ok_or_else(|| AppError::NotFound("Company not found".into()))
    }

    /// The whole-page frame: the caller as the shell shows them, and the company picked from
    /// those they manage (`None` when they manage none).
    async fn frame(&self, requested: Option<Uuid>) -> AppResult<Frame> {
        let account = load_account(&self.users, self.user).await?;
        let (_, company) = load_managed_company(&self.companies, self.user, requested).await?;
        Ok(Frame { account, company })
    }
}

pub(super) struct Frame {
    account: crate::entities::user::User,
    company: Option<Company>,
}

impl Frame {
    /// The shell's user, with the membership the managed company implies.
    fn render(
        &self,
        config: &AppConfig,
        page: impl FnOnce(&pages::MailboxUser<'_>, &Company) -> String,
    ) -> String {
        let email = EmailAddress::from(self.account.email.as_str());
        let user = workspace_user(&self.account, &email, config);
        match &self.company {
            None => pages::mailbox_no_company_page(&user),
            Some(company) => {
                let user = user
                    .with_company_membership(managed_company_membership(company, self.account.id));
                page(&user, company)
            }
        }
    }
}

/// A urlencoded form as its fields in order, repeated and dynamic names included (the
/// publication selections are `mcp:<slot>:<tool>` and `http:<tool>` fields).
pub(super) struct FormFields(Vec<(String, String)>);

impl FormFields {
    pub(super) fn parse(body: &[u8]) -> Self {
        Self(url::form_urlencoded::parse(body).into_owned().collect())
    }
    pub(super) fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
    pub(super) fn text(&self, name: &str) -> String {
        self.get(name).unwrap_or_default().to_owned()
    }
    pub(super) fn number(&self, name: &str) -> AppResult<u64> {
        self.get(name)
            .and_then(|value| value.trim().parse().ok())
            .ok_or_else(|| AppError::BadRequest(format!("The form's {name} is missing")))
    }
    pub(super) fn pairs(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
    }
}

/// An htmx answer that also moves the address bar, so a reload lands on the same view.
pub(super) fn pushed(html: String, url: &str) -> Response {
    let mut response = Html(html).into_response();
    if let Ok(value) = HeaderValue::from_str(url) {
        response.headers_mut().insert("HX-Push-Url", value);
    }
    response
}
