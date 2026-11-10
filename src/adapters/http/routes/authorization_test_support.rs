//! Scoped real-database fixtures shared by p04's HTTP authorization tests.
use std::sync::Arc;

use axum::{http::StatusCode, response::IntoResponse};
use uuid::Uuid;

use crate::adapters::http::auth::AuthenticatedUser;
use crate::adapters::monitoring::InMemoryMonitor;
use crate::adapters::persistence::PostgresPersistence;
use crate::adapters::persistence::test_support::test_pool;
use crate::app_error::AppResult;
use crate::application::attention::AttentionUseCases;
use crate::application::ports::channel::ChannelPersistence;
use crate::application::ports::channel::ChannelWrite;
use crate::application::ports::thread::ThreadPersistence;
use crate::entities::company::Company;
use crate::entities::company_member::CompanyAccessRole;
use crate::entities::user::User;
use crate::entities::user::Viewer;
use crate::entities::value_objects::EmailAddress;
use crate::infra::config::AppConfig;
use crate::use_cases::agent::AgentUseCases;
use crate::use_cases::channel::ChannelUseCases;
use crate::use_cases::company::CompanyPersistence;
use crate::use_cases::company::CompanyUseCases;
use crate::use_cases::company::CompanyWrite;
use crate::use_cases::company_invite::CompanyInvitePersistence;
use crate::use_cases::company_invite::CompanyInviteUseCases;
use crate::use_cases::company_test_support::MockCompanyPersistence;
use crate::use_cases::thread::InboundIngestPorts;
use crate::use_cases::thread::ThreadStores;
use crate::use_cases::thread::ThreadUseCases;
use crate::use_cases::user::UserPersistence;
use crate::use_cases::user::UserUseCases;

pub(super) struct ScopeCase<'a> {
    pub companies: Arc<CompanyUseCases>,
    pub invites: Arc<CompanyInviteUseCases>,
    pub user: &'a User,
    pub company_id: Uuid,
    pub status: StatusCode,
}

pub(super) struct Fixture {
    pub persistence: Arc<PostgresPersistence>,
    pub companies: Arc<CompanyUseCases>,
    pub agents: Arc<AgentUseCases>,
    pub channels: Arc<ChannelUseCases>,
    pub invites: Arc<CompanyInviteUseCases>,
    pub users: Arc<UserUseCases>,
    pub threads: Arc<ThreadUseCases>,
    pub attention: Arc<AttentionUseCases>,
    pub config: Arc<AppConfig>,
    pub owner: User,
    pub member: User,
    pub stranger: User,
    pub company: Company,
    pub foreign: Company,
    pub channel: Uuid,
    pub foreign_channel: Uuid,
    pub thread: Uuid,
    pub foreign_thread: Uuid,
}

impl Fixture {
    /// Department use cases over this fixture's database.
    pub fn departments(&self) -> Arc<crate::use_cases::department::DepartmentUseCases> {
        Arc::new(crate::use_cases::department::DepartmentUseCases::new(
            self.persistence.clone(),
            Arc::new(
                crate::adapters::persistence::department::PostgresDepartments::new(
                    self.persistence.as_ref().clone(),
                    crate::adapters::persistence::test_support::provisioning_templates(),
                ),
            ),
        ))
    }

    pub async fn new() -> Option<Self> {
        let p = Arc::new(PostgresPersistence::new(test_pool().await?));
        let owner = account(&p, "owner").await;
        let member = account(&p, "member").await;
        let stranger = account(&p, "stranger").await;
        let company = create_company(&p, &owner).await;
        let foreign = create_company(&p, &stranger).await;
        let channel = create_channel(&p, company.id).await;
        let foreign_channel = create_channel(&p, foreign.id).await;
        let invite = p
            .create_invite(
                crate::adapters::persistence::test_support::invite_command(
                    &p,
                    company.id,
                    &member.email,
                    CompanyAccessRole::Member,
                )
                .await,
            )
            .await
            .expect("member invitation");
        p.accept_pending_invite(invite.id, member.id, &member.email)
            .await
            .expect("member accepted")
            .expect("pending invitation matched");
        let thread = p
            .create_thread(channel, "Private thread", &[])
            .await
            .unwrap()
            .id;
        let foreign_thread = p
            .create_thread(foreign_channel, "Foreign private thread", &[])
            .await
            .unwrap()
            .id;
        let config = super::ui::stream_tests::test_config();
        let threads = thread_use_cases(&p, &config);
        Some(Self {
            companies: Arc::new(CompanyUseCases::new(p.clone())),
            agents: Arc::new(
                AgentUseCases::new(p.clone(), p.clone(), p.clone()).with_company_agents(
                    crate::adapters::persistence::test_support::company_agents(p.as_ref()),
                ),
            ),
            channels: Arc::new(ChannelUseCases::new(
                p.clone(),
                p.clone(),
                p.clone(),
                (config.clone()).spam_scanning(),
            )),
            invites: Arc::new(CompanyInviteUseCases::new(p.clone(), p.clone())),
            users: Arc::new(UserUseCases::new(
                Arc::new(crate::infra::argon2_password_hasher()),
                p.clone(),
                ("test-secret").into(),
            )),
            attention: Arc::new(AttentionUseCases::new(
                p.clone(),
                p.clone(),
                p.clone(),
                p.clone(),
                Arc::new(InMemoryMonitor::new()),
            )),
            persistence: p,
            threads,
            config,
            owner,
            member,
            stranger,
            company,
            foreign,
            channel,
            foreign_channel,
            thread,
            foreign_thread,
        })
    }

    pub fn denial_cases(&self) -> [ScopeCase<'_>; 3] {
        [
            ScopeCase {
                companies: self.companies.clone(),
                invites: self.invites.clone(),
                user: &self.owner,
                company_id: self.foreign.id,
                status: StatusCode::NOT_FOUND,
            },
            ScopeCase {
                companies: self.companies.clone(),
                invites: self.invites.clone(),
                user: &self.member,
                company_id: self.company.id,
                status: StatusCode::FORBIDDEN,
            },
            ScopeCase {
                companies: self.failing_companies(),
                invites: Arc::new(CompanyInviteUseCases::new(
                    Arc::new(MockCompanyPersistence::failing()),
                    self.persistence.clone(),
                )),
                user: &self.owner,
                company_id: self.company.id,
                status: StatusCode::INTERNAL_SERVER_ERROR,
            },
        ]
    }

    pub fn auth(user: &User) -> AuthenticatedUser {
        AuthenticatedUser { id: user.id }
    }

    pub fn viewer(user: &User) -> Viewer {
        Viewer {
            user_id: user.id,
            email: EmailAddress::from(user.email.as_str()),
            email_verified_at: user.email_verified_at,
        }
    }

    pub fn failing_companies(&self) -> Arc<CompanyUseCases> {
        Arc::new(CompanyUseCases::new(Arc::new(
            MockCompanyPersistence::failing(),
        )))
    }
}

async fn account(p: &PostgresPersistence, role: &str) -> User {
    let name = format!("p04-{role}-{}", Uuid::new_v4().simple());
    let mut user = p
        .create_user(&name, &format!("{name}@example.test"), "hash")
        .await
        .unwrap();
    sqlx::query("UPDATE users SET email_verified_at = CURRENT_TIMESTAMP WHERE id = $1")
        .bind(user.id)
        .execute(p.pool())
        .await
        .expect("verified fixture address");
    user.email_verified_at = Some(chrono::Utc::now());
    user
}

async fn create_company(p: &PostgresPersistence, owner: &User) -> Company {
    CompanyPersistence::create(
        p,
        owner.id,
        CompanyWrite {
            name: format!("Private company {}", Uuid::new_v4().simple()),
            slug: format!("p04-{}", Uuid::new_v4().simple()),
            ..Default::default()
        },
    )
    .await
    .expect("company with owner membership")
}

async fn create_channel(p: &PostgresPersistence, company: Uuid) -> Uuid {
    ChannelPersistence::create(
        p,
        company,
        ChannelWrite {
            name: "Private channel".into(),
            slug: "private".into(),
            enabled: false,
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id
}

pub(super) async fn denied<T>(
    result: AppResult<T>,
    expected: StatusCode,
    hidden_name: &str,
) -> String {
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("expected {expected}, got success"),
    };
    let response = error.into_response();
    assert_eq!(response.status(), expected);
    let body = String::from_utf8(
        axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(
        !body.contains(hidden_name),
        "denial disclosed company name: {body}"
    );
    body
}

fn thread_use_cases(p: &Arc<PostgresPersistence>, config: &Arc<AppConfig>) -> Arc<ThreadUseCases> {
    let renderers = Arc::new(
        crate::transport::ports::TransportRenderers::new()
            .register(Arc::new(
                crate::adapters::protocols::email::EmailRenderer::new(&config.app_domain_name),
            ))
            .unwrap(),
    );
    Arc::new(ThreadUseCases::new(
        ThreadStores {
            threads: p.clone(),
            channels: p.clone(),
            companies: p.clone(),
            participants: p.clone(),
        },
        InboundIngestPorts {
            committer: p.clone(),
            correlation: p.clone(),
            bindings: p.clone(),
            standalone_deliveries: p.clone(),
        },
        renderers,
        (config.clone()).thread_settings(),
    ))
}
