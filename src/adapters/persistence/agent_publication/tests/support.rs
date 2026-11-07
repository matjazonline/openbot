use super::*;
use crate::adapters::persistence::credentials::CredentialCipher;
use crate::adapters::persistence::test_support::test_pool;
use crate::use_cases::user::UserPersistence;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct PublicationCounts {
    pub revisions: i64,
    pub requests: i64,
}

pub(super) struct Fixture {
    pub p: PostgresPersistence,
    pub scope: AgentPublicationScope,
    pub binding: AgentToolBinding,
}

impl Fixture {
    pub async fn new() -> Self {
        let p = PostgresPersistence::with_credential_cipher(
            test_pool().await.unwrap(),
            CredentialCipher::for_test(),
        );
        let username = format!("autopub_{}", Uuid::new_v4().simple());
        let email = format!("{username}@fixture.test");
        p.create_user(&username, &email, "hash").await.unwrap();
        let user = p.get_by_email(&email).await.unwrap().unwrap();
        let company = CompanyPersistence::create(
            &p,
            user.id,
            CompanyWrite {
                name: "Independent publication".into(),
                slug: format!("autopub-{}", Uuid::new_v4().simple()),
                channel_defaults: Default::default(),
                avatar_url: None,
            },
        )
        .await
        .unwrap();
        p.replace_model_connections_for_user(
            user.id,
            company.id,
            vec![CompanyModelConnectionWrite {
                provider: "openai".into(),
                api_key: Some("fixture-only-model-key".into()),
                models: vec!["fixture-model".into()],
                is_default: true,
            }],
        )
        .await
        .unwrap();
        let binding = AgentToolBinding::builtin(AgentBuiltinTool::MessageReply);
        let agent = AgentPersistence::create(
            &p,
            company.id,
            AgentWrite {
                tool_bindings: Some(vec![binding.clone()]),
                ..write()
            },
        )
        .await
        .unwrap();
        Self {
            p,
            scope: AgentPublicationScope {
                company: CompanyId::new(company.id),
                actor: WorkflowActor::authenticated(user.id).unwrap(),
                agent: AgentId::new(agent.id),
            },
            binding,
        }
    }

    pub async fn request(&self, key: &str) -> PublishAgent {
        let draft = AgentPersistence::get_by_id(&self.p, self.scope.agent.as_uuid())
            .await
            .unwrap()
            .unwrap();
        PublishAgent {
            scope: self.scope,
            expected_draft_revision: draft.draft_revision,
            expected_active_revision: self
                .p
                .active_agent_revision(self.scope)
                .await
                .unwrap()
                .map(|revision| revision.id),
            key: AgentPublicationKey::parse(key).unwrap(),
        }
    }
    pub async fn update(&self, write: AgentWrite) {
        AgentPersistence::update(&self.p, self.scope.agent.as_uuid(), write)
            .await
            .unwrap();
    }
    pub async fn skill(&self, required: Vec<TypeName>) -> crate::entities::skill::Skill {
        let skill = self
            .p
            .create_company(
                self.scope.company.as_uuid(),
                SkillWrite {
                    slug: format!("skill-{}", Uuid::new_v4().simple()),
                    name: "Shared instructions".into(),
                    description: "Instructions for this fixture".into(),
                    instructions: "Original shared text.".into(),
                    required_tools: Some(required),
                    created_by: None,
                },
            )
            .await
            .unwrap();
        self.update(AgentWrite {
            skill_ids: vec![skill.id],
            ..write()
        })
        .await;
        skill
    }
    pub async fn counts(&self) -> PublicationCounts {
        let revisions: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM agent_revisions WHERE company_id=$1 AND agent_id=$2",
        )
        .bind(self.scope.company.as_uuid())
        .bind(self.scope.agent.as_uuid())
        .fetch_one(self.p.pool())
        .await
        .unwrap();
        let requests: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM agent_publication_requests WHERE company_id=$1 AND agent_id=$2",
        )
        .bind(self.scope.company.as_uuid())
        .bind(self.scope.agent.as_uuid())
        .fetch_one(self.p.pool())
        .await
        .unwrap();
        PublicationCounts {
            revisions,
            requests,
        }
    }
}

pub(super) fn write() -> AgentWrite {
    AgentWrite {
        name: "Assistant".into(),
        slug: "assistant".into(),
        provider: Some("openai".into()),
        model: Some("fixture-model".into()),
        system_prompt: Some("Original agent instructions.".into()),
        ..Default::default()
    }
}

pub(super) async fn attach_vault(
    f: &Fixture,
    connection: Uuid,
    revision: i64,
    kind: crate::application::use_cases::integration_binding::BindingKind,
) {
    use crate::{
        adapters::{
            integrations::owner::owner_for_user,
            persistence::{
                integration_bindings::PostgresIntegrationBindings, integrations::PostgresVault,
            },
        },
        application::{
            integrations::{
                VerifiedOwner,
                management::{AccountRepository, AccountWrite},
            },
            use_cases::integration_binding::*,
        },
        domain::integrations::*,
    };
    let owner = owner_for_user(f.scope.actor.user_id());
    let vault = PostgresVault::new(f.p.clone());
    let account = vault
        .create(
            &owner,
            AccountWrite {
                key: AccountKey::parse("agent-private").unwrap(),
                provider: ProviderId::parse("static-bearer").unwrap(),
                config: AuthConfigId::parse("static-bearer").unwrap(),
                method: AuthMethod::StaticSecret,
                schema: CredentialSchema::parse("static-v1").unwrap(),
                identity: None,
                metadata: AccountMetadata::new("Agent private".into(), None).unwrap(),
                payload: secrecy::SecretString::from("private-agent-token"),
            },
        )
        .await
        .unwrap();
    let host = PostgresIntegrationBindings {
        persistence: f.p.clone(),
        vault,
    };
    let proposal = host
        .propose(
            f.scope.actor.user_id(),
            ProposeBinding {
                company: f.scope.company.as_uuid(),
                connection,
                kind,
                app: AppId::parse("static-bearer").unwrap(),
                expected_revision: revision,
            },
        )
        .await
        .unwrap();
    host.confirm(
        VerifiedOwner::from_verified_client(owner),
        proposal.id,
        account.id,
        account.authorization_revision,
    )
    .await
    .unwrap();
}
