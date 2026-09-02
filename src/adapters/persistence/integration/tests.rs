//! Database-backed tests for installations, credentials and channel bindings.
//!
//! Every fixture here suffixes its globally-unique values, because the whole suite shares one test
//! database and runs in parallel. Nothing here touches an unscoped queue claim, so no test in this
//! file needs the shared claim guard.

use std::sync::Arc;

use regex::Regex;
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgPool;
use uuid::Uuid;

use super::*;
use crate::{
    adapters::{
        persistence::{
            PostgresPersistence, credentials::CredentialCipher, test_support::test_pool,
        },
        protocols::email::EmailEndpointKey,
    },
    entities::{
        channel::Channel,
        transport::{
            BindingAccessSnapshot, BindingAuditAction, BindingChangeReason, InstallationStatus,
            IntegrationCredentialKind, TransportKind,
        },
        value_objects::ChannelSlug,
    },
    use_cases::{
        agent::{AgentPersistence, AgentWrite, OwnedAgentChannelPersistence},
        channel::{ChannelPersistence, ChannelWrite},
        company::{CompanyPersistence, CompanyWrite},
        integration::{
            BindingStatusChange, BindingWrite, ChannelBindingPersistence, CredentialScope,
            InboundEndpoint, InstallationCredentialStore, InstallationPersistence,
            InstallationStatusChange, InstallationWrite,
        },
        user::UserPersistence,
    },
};

struct TestCompany {
    id: Uuid,
    owner_id: Uuid,
}

async fn company(persistence: &PostgresPersistence, label: &str) -> TestCompany {
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("{label}_{suffix}");
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .unwrap();
    let owner = persistence.get_by_email(&email).await.unwrap().unwrap();
    let created = CompanyPersistence::create(
        persistence,
        owner.id,
        CompanyWrite {
            name: format!("{label} Corp"),
            slug: format!("{label}-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    TestCompany {
        id: created.id,
        owner_id: owner.id,
    }
}

/// A channel with no agent, so the enabled-channel trigger stays out of the way.
async fn channel(persistence: &PostgresPersistence, company_id: Uuid, slug: &str) -> Channel {
    ChannelPersistence::create(
        persistence,
        company_id,
        ChannelWrite {
            name: format!("Channel {slug}"),
            slug: slug.to_string(),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap()
}

async fn slack_installation(
    persistence: &PostgresPersistence,
    company: &TestCompany,
) -> IntegrationInstallation {
    persistence
        .install(InstallationWrite {
            company_id: company.id,
            transport: TransportKind::Slack,
            external_tenant_key: ExternalTenantKey::parse(format!("T{}", Uuid::new_v4().simple()))
                .unwrap(),
            display_name: "Acme Workspace".into(),
            granted_scopes: vec!["groups:history".into(), "chat:write".into()],
            actor: CreationProvenance::user(company.owner_id),
        })
        .await
        .unwrap()
}

fn slack_binding_write(
    company_id: Uuid,
    channel_id: Uuid,
    installation: &IntegrationInstallation,
    conversation: &str,
) -> BindingWrite {
    BindingWrite {
        company_id,
        channel_id,
        installation_id: Some(installation.id),
        transport: TransportKind::Slack,
        namespace: EndpointNamespace::parse(installation.external_tenant_key.as_str()).unwrap(),
        external_endpoint_key: ExternalEndpointKey::parse(conversation).unwrap(),
        display_label: "#support".into(),
        access_policy: BindingAccessPolicy::ConversationMembersReadAndParticipate,
        delivery_policy: BindingDeliveryPolicy::ReplyOnly,
        access_snapshot: BindingAccessSnapshot::provider_conversation(true, false, 4),
        created_by: CreationProvenance::system(),
    }
}

fn endpoint_of(binding: &ChannelBinding) -> InboundEndpoint {
    InboundEndpoint {
        transport: binding.transport,
        installation_id: binding.installation_id,
        namespace: binding.namespace.clone(),
        external_endpoint_key: binding.external_endpoint_key.clone(),
    }
}

fn scope(company_id: Uuid, installation: &IntegrationInstallation) -> CredentialScope {
    CredentialScope {
        company_id,
        installation_id: installation.id,
        transport: installation.transport,
        kind: IntegrationCredentialKind::BotAccessToken,
    }
}

/// Tenancy is a foreign key here, not application code that a future caller could route around.
#[tokio::test]
async fn the_database_rejects_every_cross_tenant_integration_row() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let first = company(&persistence, "tenanti").await;
    let second = company(&persistence, "tenantj").await;
    let installation = slack_installation(&persistence, &first).await;
    let borrowed_channel = channel(&persistence, second.id, "support").await;

    // A credential cannot name another company's installation.
    let cross_tenant_credential = sqlx::query(
        r#"INSERT INTO integration_credentials
               (company_id, installation_id, credential_kind, envelope)
           VALUES ($1, $2, 'bot_access_token', 'enc:v2:1:AAAA:BBBB:CCCC:DDDD')"#,
    )
    .bind(second.id)
    .bind(installation.id.as_uuid())
    .execute(&pool)
    .await;
    assert!(cross_tenant_credential.is_err());

    // A binding cannot point one company's channel at another company's installation.
    let cross_tenant_binding = sqlx::query(
        r#"INSERT INTO channel_bindings
               (id, company_id, channel_id, installation_id, transport, namespace,
                external_endpoint_key, display_label, access_policy, delivery_policy, status,
                created_by, access_snapshot)
           VALUES ($1, $2, $3, $4, 'slack', 'T1', 'C1', '#support', 'conversation_members',
                   'reply_only', 'active', $5, $6)"#,
    )
    .bind(Uuid::new_v4())
    .bind(second.id)
    .bind(borrowed_channel.id)
    .bind(installation.id.as_uuid())
    .bind(serde_json::to_value(CreationProvenance::system()).unwrap())
    .bind(
        serde_json::to_value(BindingAccessSnapshot::provider_conversation(true, false, 1)).unwrap(),
    )
    .execute(&pool)
    .await;
    assert!(cross_tenant_binding.is_err());

    // And an audit row cannot describe a binding that is not its company's.
    let cross_tenant_audit = sqlx::query(
        r#"INSERT INTO binding_audit_events (id, company_id, binding_id, action, actor, metadata)
           VALUES ($1, $2, $3, 'linked', $4, '{"version": 1, "transport": "slack"}'::jsonb)"#,
    )
    .bind(Uuid::new_v4())
    .bind(second.id)
    .bind(Uuid::new_v4())
    .bind(serde_json::to_value(CreationProvenance::system()).unwrap())
    .execute(&pool)
    .await;
    assert!(cross_tenant_audit.is_err());
}

/// The coherence rule, from both sides: an installed transport without an account, and a
/// deployment transport that borrowed one.
#[tokio::test]
async fn a_binding_cannot_disagree_with_its_transport_about_installations() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let acme = company(&persistence, "coherent").await;
    let support = channel(&persistence, acme.id, "support").await;
    let installation = slack_installation(&persistence, &acme).await;

    let unbacked_slack = sqlx::query(
        r#"INSERT INTO channel_bindings
               (id, company_id, channel_id, transport, namespace, external_endpoint_key,
                display_label, access_policy, delivery_policy, status, created_by, access_snapshot)
           VALUES ($1, $2, $3, 'slack', 'T1', 'C1', '#support', 'conversation_members',
                   'reply_only', 'active', $4, $5)"#,
    )
    .bind(Uuid::new_v4())
    .bind(acme.id)
    .bind(support.id)
    .bind(serde_json::to_value(CreationProvenance::system()).unwrap())
    .bind(
        serde_json::to_value(BindingAccessSnapshot::provider_conversation(true, false, 1)).unwrap(),
    )
    .execute(&pool)
    .await;
    assert!(unbacked_slack.is_err());

    let installed_email = sqlx::query(
        r#"INSERT INTO channel_bindings
               (id, company_id, channel_id, installation_id, transport, namespace,
                external_endpoint_key, display_label, access_policy, delivery_policy, status,
                created_by, access_snapshot)
           VALUES ($1, $2, $3, $4, 'email', 'email', 'other@acme', 'Support', 'channel_acl',
                   'reply_and_initiate', 'active', $5, $6)"#,
    )
    .bind(Uuid::new_v4())
    .bind(acme.id)
    .bind(support.id)
    .bind(installation.id.as_uuid())
    .bind(serde_json::to_value(CreationProvenance::system()).unwrap())
    .bind(serde_json::to_value(BindingAccessSnapshot::deployment_endpoint()).unwrap())
    .execute(&pool)
    .await;
    assert!(installed_email.is_err());
}

/// Two channels consuming one conversation would cross-post every message between them, so the
/// second link is refused while the first still claims the endpoint.
#[tokio::test]
async fn two_channels_cannot_hold_the_same_installed_endpoint() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let acme = company(&persistence, "claimed").await;
    let first_channel = channel(&persistence, acme.id, "support").await;
    let second_channel = channel(&persistence, acme.id, "sales").await;
    let installation = slack_installation(&persistence, &acme).await;
    let conversation = format!("C{}", Uuid::new_v4().simple());

    let first = persistence
        .create_binding(slack_binding_write(
            acme.id,
            first_channel.id,
            &installation,
            &conversation,
        ))
        .await
        .unwrap();
    let contested = persistence
        .create_binding(slack_binding_write(
            acme.id,
            second_channel.id,
            &installation,
            &conversation,
        ))
        .await;
    assert!(matches!(contested, Err(AppError::Conflict(_))));

    // Disabling the first binding releases the claim, so the conversation can move channels.
    persistence
        .set_binding_status(BindingStatusChange {
            company_id: acme.id,
            binding_id: first.id,
            status: BindingStatus::Disabled,
            reason: Some(BindingChangeReason::ManagerRequest),
            actor: CreationProvenance::system(),
        })
        .await
        .unwrap();
    let relinked = persistence
        .create_binding(slack_binding_write(
            acme.id,
            second_channel.id,
            &installation,
            &conversation,
        ))
        .await
        .unwrap();
    assert_eq!(relinked.channel_id, second_channel.id);
}

/// Exactly the statuses `BindingStatus::holds_endpoint_claim` names must block a second link.
/// Written as a matrix rather than as a comment, because the same set is spelled out in three
/// partial unique indexes.
#[tokio::test]
async fn the_live_status_set_in_rust_matches_the_one_the_indexes_enforce() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let acme = company(&persistence, "livestat").await;
    let holder = channel(&persistence, acme.id, "support").await;
    let contender = channel(&persistence, acme.id, "sales").await;
    let installation = slack_installation(&persistence, &acme).await;
    let conversation = format!("C{}", Uuid::new_v4().simple());
    let binding = persistence
        .create_binding(slack_binding_write(
            acme.id,
            holder.id,
            &installation,
            &conversation,
        ))
        .await
        .unwrap();

    for status in BindingStatus::ALL {
        persistence
            .set_binding_status(BindingStatusChange {
                company_id: acme.id,
                binding_id: binding.id,
                status: *status,
                reason: (!status.holds_endpoint_claim())
                    .then_some(BindingChangeReason::ManagerRequest),
                actor: CreationProvenance::system(),
            })
            .await
            .unwrap();

        let attempt = persistence
            .create_binding(slack_binding_write(
                acme.id,
                contender.id,
                &installation,
                &conversation,
            ))
            .await;
        assert_eq!(
            attempt.is_err(),
            status.holds_endpoint_claim(),
            "status '{status}' disagrees with holds_endpoint_claim()"
        );
        if let Ok(created) = attempt {
            persistence
                .set_binding_status(BindingStatusChange {
                    company_id: acme.id,
                    binding_id: created.id,
                    status: BindingStatus::Disabled,
                    reason: Some(BindingChangeReason::ManagerRequest),
                    actor: CreationProvenance::system(),
                })
                .await
                .unwrap();
        }
    }
}

/// Disabling an interface is not deleting a channel. The thread history stays, the row stays
/// readable to a manager, and only the routing queries stop seeing it.
#[tokio::test]
async fn disabling_a_binding_removes_it_from_routing_and_leaves_everything_else() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let acme = company(&persistence, "disabled").await;
    let support = channel(&persistence, acme.id, "support").await;
    let installation = slack_installation(&persistence, &acme).await;
    let binding = persistence
        .create_binding(slack_binding_write(
            acme.id,
            support.id,
            &installation,
            &format!("C{}", Uuid::new_v4().simple()),
        ))
        .await
        .unwrap();

    assert_eq!(
        persistence
            .find_active_binding_by_endpoint(&endpoint_of(&binding))
            .await
            .unwrap()
            .map(|found| found.id),
        Some(binding.id)
    );

    persistence
        .set_binding_status(BindingStatusChange {
            company_id: acme.id,
            binding_id: binding.id,
            status: BindingStatus::Disabled,
            reason: Some(BindingChangeReason::AccessRevoked),
            actor: CreationProvenance::user(acme.owner_id),
        })
        .await
        .unwrap();

    assert!(
        persistence
            .find_active_binding_by_endpoint(&endpoint_of(&binding))
            .await
            .unwrap()
            .is_none(),
        "a disabled binding must not route inbound events"
    );
    let active = persistence
        .active_bindings_for_channel(acme.id, support.id)
        .await
        .unwrap();
    assert!(
        active.iter().all(|found| found.id != binding.id),
        "a disabled binding must not be a delivery target"
    );
    // The canonical email binding is untouched by the Slack binding's disablement.
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].transport, TransportKind::Email);

    let stored = persistence
        .get_binding(acme.id, binding.id)
        .await
        .unwrap()
        .expect("a manager can still see the binding they disabled");
    assert_eq!(stored.status, BindingStatus::Disabled);
    assert_eq!(
        stored.disabled_reason,
        Some(BindingChangeReason::AccessRevoked)
    );
    assert!(
        ChannelPersistence::get_by_id(&persistence, support.id)
            .await
            .unwrap()
            .is_some_and(|found| found.id == support.id),
        "the channel itself survives its interface being switched off"
    );

    let events = persistence
        .list_binding_audit_events(acme.id, binding.id, 50)
        .await
        .unwrap();
    let actions: Vec<BindingAuditAction> = events.iter().map(|event| event.action).collect();
    assert_eq!(
        actions,
        vec![BindingAuditAction::Disabled, BindingAuditAction::Linked],
        "newest first, and every lifecycle mutation left a record"
    );
    assert_eq!(events[0].reason, Some(BindingChangeReason::AccessRevoked));
    assert_eq!(
        events[0].metadata.access_policy,
        BindingAccessPolicy::ConversationMembersReadAndParticipate
    );
}

/// Revoking a workspace grant must stop its channels in the same instant, without a mass update
/// that could half-succeed and leave some conversations live.
#[tokio::test]
async fn revoking_an_installation_silences_its_bindings_without_rewriting_them() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let acme = company(&persistence, "revoked").await;
    let support = channel(&persistence, acme.id, "support").await;
    let installation = slack_installation(&persistence, &acme).await;
    let binding = persistence
        .create_binding(slack_binding_write(
            acme.id,
            support.id,
            &installation,
            &format!("C{}", Uuid::new_v4().simple()),
        ))
        .await
        .unwrap();

    let revoked = persistence
        .set_installation_status(InstallationStatusChange {
            company_id: acme.id,
            installation_id: installation.id,
            status: InstallationStatus::Revoked,
            actor: CreationProvenance::user(acme.owner_id),
        })
        .await
        .unwrap();

    assert_eq!(revoked.status, InstallationStatus::Revoked);
    assert!(revoked.revoked_at.is_some() && revoked.revoked_by.is_some());
    assert!(
        persistence
            .find_active_binding_by_endpoint(&endpoint_of(&binding))
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        persistence
            .active_bindings_for_channel(acme.id, support.id)
            .await
            .unwrap()
            .iter()
            .all(|found| found.transport == TransportKind::Email)
    );
    assert_eq!(
        persistence
            .get_binding(acme.id, binding.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        BindingStatus::Active,
        "the binding row is untouched; the installation is what changed"
    );
}

/// One external workspace belongs to one app company. Re-installing into the same company is a
/// refresh; installing into another is refused rather than adopted.
#[tokio::test]
async fn a_workspace_can_be_reinstalled_but_never_adopted_by_a_second_company() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let acme = company(&persistence, "installa").await;
    let globex = company(&persistence, "installb").await;
    let installation = slack_installation(&persistence, &acme).await;

    let refreshed = persistence
        .install(InstallationWrite {
            company_id: acme.id,
            transport: TransportKind::Slack,
            external_tenant_key: installation.external_tenant_key.clone(),
            display_name: "Acme Workspace (renamed)".into(),
            granted_scopes: vec!["groups:history".into()],
            actor: CreationProvenance::user(acme.owner_id),
        })
        .await
        .unwrap();
    assert_eq!(refreshed.id, installation.id);
    assert_eq!(refreshed.display_name, "Acme Workspace (renamed)");
    assert_eq!(refreshed.granted_scopes, vec!["groups:history".to_string()]);

    let stolen = persistence
        .install(InstallationWrite {
            company_id: globex.id,
            transport: TransportKind::Slack,
            external_tenant_key: installation.external_tenant_key.clone(),
            display_name: "Not yours".into(),
            granted_scopes: Vec::new(),
            actor: CreationProvenance::user(globex.owner_id),
        })
        .await;
    assert!(matches!(stolen, Err(AppError::Conflict(_))));

    assert_eq!(
        persistence
            .find_installation_by_tenant(TransportKind::Slack, &installation.external_tenant_key)
            .await
            .unwrap()
            .map(|found| found.company_id),
        Some(acme.id)
    );
    assert!(
        persistence
            .list_installations(globex.id)
            .await
            .unwrap()
            .is_empty()
    );
}

/// The whole point of the separate table: nothing a manager-facing projection can reach carries
/// the ciphertext, let alone the token.
#[tokio::test]
async fn installation_projections_never_carry_credential_material() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence =
        PostgresPersistence::with_credential_cipher(pool.clone(), CredentialCipher::for_test());
    let acme = company(&persistence, "noleak").await;
    let installation = slack_installation(&persistence, &acme).await;
    persistence
        .store_credential(
            &scope(acme.id, &installation),
            SecretString::from("xoxb-not-in-any-projection".to_string()),
        )
        .await
        .unwrap();

    let listed = persistence.list_installations(acme.id).await.unwrap();
    let rendered = format!("{:?} {}", listed, serde_json::to_string(&listed).unwrap());

    assert_eq!(listed.len(), 1);
    assert!(!rendered.contains("xoxb"));
    assert!(!rendered.contains("enc:v2"));
    assert!(
        !super::INSTALLATION_COLUMNS.contains("envelope")
            && !super::BINDING_COLUMNS.contains("envelope"),
        "no broad column list may reach the credential column"
    );
}

/// A credential opens only under the exact scope it was written for, and a row moved between
/// scopes fails loudly rather than reading as "no credential stored".
#[tokio::test]
async fn a_credential_opens_only_under_its_own_scope() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence =
        PostgresPersistence::with_credential_cipher(pool.clone(), CredentialCipher::for_test());
    let acme = company(&persistence, "scoped").await;
    let installation = slack_installation(&persistence, &acme).await;
    let other_installation = slack_installation(&persistence, &acme).await;
    let bot = scope(acme.id, &installation);
    persistence
        .store_credential(&bot, SecretString::from("xoxb-real".to_string()))
        .await
        .unwrap();

    assert_eq!(
        persistence
            .read_credential(&bot)
            .await
            .unwrap()
            .map(|secret| secret.expose_secret().to_string()),
        Some("xoxb-real".to_string())
    );
    assert!(
        persistence
            .read_credential(&CredentialScope {
                kind: IntegrationCredentialKind::UserAccessToken,
                ..bot.clone()
            })
            .await
            .unwrap()
            .is_none(),
        "a different credential kind is a different secret, not the same one"
    );
    assert!(
        persistence
            .read_credential(&CredentialScope {
                company_id: Uuid::new_v4(),
                ..bot.clone()
            })
            .await
            .unwrap()
            .is_none()
    );

    // An operator copying the envelope onto another installation's row gets an error, not a token.
    sqlx::query(
        r#"INSERT INTO integration_credentials
               (company_id, installation_id, credential_kind, envelope)
           SELECT $1, $2, 'bot_access_token', credential.envelope
           FROM integration_credentials AS credential
           WHERE credential.company_id = $1 AND credential.installation_id = $3"#,
    )
    .bind(acme.id)
    .bind(other_installation.id.as_uuid())
    .bind(installation.id.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    let moved = persistence
        .read_credential(&scope(acme.id, &other_installation))
        .await;
    assert!(
        matches!(moved, Err(AppError::Internal(_))),
        "a misplaced envelope must fail closed, not read as absent"
    );

    assert!(persistence.delete_credential(&bot).await.unwrap());
    assert!(persistence.read_credential(&bot).await.unwrap().is_none());
    assert!(!persistence.delete_credential(&bot).await.unwrap());
}

/// There is no plaintext fallback: a deployment with no key ring cannot store or read a token.
#[tokio::test]
async fn credential_access_without_a_cipher_fails_rather_than_falling_back() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let acme = company(&persistence, "nocipher").await;
    let installation = slack_installation(&persistence, &acme).await;

    assert!(
        persistence
            .store_credential(
                &scope(acme.id, &installation),
                SecretString::from("xoxb-token".to_string())
            )
            .await
            .is_err()
    );
    assert!(
        persistence
            .read_credential(&scope(acme.id, &installation))
            .await
            .is_err()
    );
    let stored: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM integration_credentials WHERE company_id = $1")
            .bind(acme.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, 0);
}

/// Every channel is created with exactly one canonical email interface, and no amount of retrying
/// or racing the write produces a second one.
#[tokio::test]
async fn a_channel_gets_exactly_one_canonical_email_binding() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = Arc::new(PostgresPersistence::new(pool.clone()));
    let acme = company(&persistence, "onebind").await;
    let support = channel(&persistence, acme.id, "support").await;

    let bindings = persistence
        .active_bindings_for_channel(acme.id, support.id)
        .await
        .unwrap();
    assert_eq!(bindings.len(), 1);
    let email = &bindings[0];
    assert_eq!(email.transport, TransportKind::Email);
    assert_eq!(email.installation_id, None);
    assert_eq!(email.access_policy, BindingAccessPolicy::ChannelAcl);
    assert_eq!(
        email.delivery_policy,
        BindingDeliveryPolicy::ReplyAndInitiate
    );
    assert_eq!(
        email.external_endpoint_key.as_str(),
        EmailEndpointKey::canonical(&ChannelSlug::from("support"))
            .unwrap()
            .as_str()
    );
    assert_eq!(
        email.namespace,
        EmailEndpointKey::namespace(acme.id),
        "the tenant scope is the company id, not anything a rename can move"
    );

    // The same write, applied concurrently: the row is claimed once and every other contender
    // either updates it or loses the unique index.
    let mut racers = tokio::task::JoinSet::new();
    for _ in 0..6 {
        let persistence = persistence.clone();
        let channel_id = support.id;
        racers.spawn(async move {
            ChannelPersistence::update(
                persistence.as_ref(),
                channel_id,
                ChannelWrite {
                    name: "Support".into(),
                    slug: "support".into(),
                    enabled: false,
                    ..ChannelWrite::default()
                },
            )
            .await
            .map(|_| ())
            .map_err(|_| ())
        });
    }
    while let Some(joined) = racers.join_next().await {
        let _ = joined.unwrap();
    }

    let count: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM channel_bindings
           WHERE company_id = $1 AND channel_id = $2 AND transport = 'email'"#,
    )
    .bind(acme.id)
    .bind(support.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
}

/// Renaming a channel's address moves its interface rather than replacing it, so the interface's
/// history is continuous across the rename.
#[tokio::test]
async fn renaming_a_channel_address_moves_its_binding_and_records_the_move() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let acme = company(&persistence, "renamed").await;
    let support = channel(&persistence, acme.id, "support").await;
    let before = persistence
        .active_bindings_for_channel(acme.id, support.id)
        .await
        .unwrap()
        .remove(0);

    ChannelPersistence::update(
        &persistence,
        support.id,
        ChannelWrite {
            name: "Helpdesk".into(),
            slug: "helpdesk".into(),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let after = persistence
        .active_bindings_for_channel(acme.id, support.id)
        .await
        .unwrap()
        .remove(0);

    assert_eq!(after.id, before.id, "the interface is the same one");
    assert_eq!(
        after.external_endpoint_key.as_str(),
        EmailEndpointKey::canonical(&ChannelSlug::from("helpdesk"))
            .unwrap()
            .as_str()
    );
    assert_eq!(after.display_label, "Helpdesk");

    let actions: Vec<BindingAuditAction> = persistence
        .list_binding_audit_events(acme.id, after.id, 50)
        .await
        .unwrap()
        .iter()
        .map(|event| event.action)
        .collect();
    assert_eq!(
        actions,
        vec![
            BindingAuditAction::EndpointChanged,
            BindingAuditAction::Linked
        ]
    );
}

/// Audit rows record what was seen at the time. Rewriting one would make the log worth less than
/// no log at all.
#[tokio::test]
async fn binding_audit_rows_cannot_be_rewritten() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let acme = company(&persistence, "appendon").await;
    let support = channel(&persistence, acme.id, "support").await;
    let binding = persistence
        .active_bindings_for_channel(acme.id, support.id)
        .await
        .unwrap()
        .remove(0);

    let rewrite =
        sqlx::query("UPDATE binding_audit_events SET action = 'unlinked' WHERE binding_id = $1")
            .bind(binding.id.as_uuid())
            .execute(&pool)
            .await;
    assert!(rewrite.is_err());
}

/// The two places each stored vocabulary is written -- a Rust enum and a SQL `CHECK` -- have to
/// agree in *both* directions. A variant Rust knows and SQL rejects fails at insert time in
/// production; a value SQL allows and Rust cannot parse fails at read time, on a row that already
/// exists.
#[tokio::test]
async fn every_stored_enum_matches_its_database_constraint() {
    let Some(pool) = test_pool().await else {
        return;
    };

    assert_variants(
        &pool,
        "channel_bindings_transport_check",
        TransportKind::ALL,
    )
    .await;
    assert_variants(
        &pool,
        "integration_installations_status_check",
        InstallationStatus::ALL,
    )
    .await;
    assert_variants(&pool, "channel_bindings_status_check", BindingStatus::ALL).await;
    assert_variants(
        &pool,
        "channel_bindings_access_policy_check",
        BindingAccessPolicy::ALL,
    )
    .await;
    assert_variants(
        &pool,
        "channel_bindings_delivery_policy_check",
        BindingDeliveryPolicy::ALL,
    )
    .await;
    assert_variants(
        &pool,
        "binding_audit_events_action_check",
        BindingAuditAction::ALL,
    )
    .await;
    assert_variants(
        &pool,
        "integration_credentials_kind_check",
        IntegrationCredentialKind::ALL,
    )
    .await;

    // The reason vocabulary is a function rather than an inline list, so it is asserted by asking.
    for reason in BindingChangeReason::ALL {
        let accepted: bool = sqlx::query_scalar("SELECT valid_binding_change_reason($1)")
            .bind(reason.as_str())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(accepted, "SQL rejects the '{reason}' reason");
    }
    let unknown: bool = sqlx::query_scalar("SELECT valid_binding_change_reason($1)")
        .bind("because")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!unknown);
}

/// `TransportKind::requires_installation` and `transport_requires_installation()` are the same
/// decision. A transport added to one and not the other would let a binding exist with no account
/// behind it, or refuse one that needs none.
#[tokio::test]
async fn rust_and_sql_agree_on_which_transports_require_an_installation() {
    let Some(pool) = test_pool().await else {
        return;
    };

    for transport in TransportKind::ALL {
        let in_sql: bool = sqlx::query_scalar("SELECT transport_requires_installation($1)")
            .bind(transport.as_str())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            in_sql,
            transport.requires_installation(),
            "'{transport}' disagrees between Rust and SQL"
        );
    }
}

/// Compare a `CHECK (column IN ('a', 'b'))` constraint's literals against an enum's inventory.
async fn assert_variants<T: std::fmt::Display>(pool: &PgPool, constraint: &str, variants: &[T]) {
    let definition: String = sqlx::query_scalar(
        "SELECT pg_get_constraintdef(oid) FROM pg_constraint WHERE conname = $1",
    )
    .bind(constraint)
    .fetch_one(pool)
    .await
    .unwrap_or_else(|_| panic!("{constraint} exists"));

    let literal = Regex::new("'([a-z_]+)'").expect("a valid literal pattern");
    let mut in_sql: Vec<String> = literal
        .captures_iter(&definition)
        .map(|captures| captures[1].to_string())
        .collect();
    in_sql.sort();
    let mut in_rust: Vec<String> = variants.iter().map(ToString::to_string).collect();
    in_rust.sort();

    assert_eq!(in_rust, in_sql, "{constraint} and its Rust enum disagree");
}

/// A company slug is editable in company settings, so nothing durable may be keyed by it. The
/// binding is namespaced by the company *id* precisely so that this rename is a no-op rather than a
/// fan-out of rewrites that a future writer of `companies.slug` could forget to perform.
#[tokio::test]
async fn renaming_a_company_leaves_its_email_bindings_alone_and_resolvable() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let acme = company(&persistence, "corprename").await;
    let support = channel(&persistence, acme.id, "support").await;
    let before = persistence
        .active_bindings_for_channel(acme.id, support.id)
        .await
        .unwrap()
        .remove(0);

    let existing = CompanyPersistence::get_by_id(&persistence, acme.id)
        .await
        .unwrap()
        .unwrap();
    CompanyPersistence::update(
        &persistence,
        acme.id,
        CompanyWrite {
            name: existing.name.clone(),
            slug: format!("globex-{}", Uuid::new_v4().simple()),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();

    let after = persistence
        .active_bindings_for_channel(acme.id, support.id)
        .await
        .unwrap()
        .remove(0);
    assert_eq!(after.external_endpoint_key, before.external_endpoint_key);
    assert_eq!(after.namespace, before.namespace);
    assert_eq!(
        after.updated_at, before.updated_at,
        "the rename must not have touched the binding at all"
    );

    // And the interface is still the one an inbound message for this channel resolves to.
    assert_eq!(
        persistence
            .find_active_binding_by_endpoint(&endpoint_of(&after))
            .await
            .unwrap()
            .map(|found| found.id),
        Some(before.id)
    );

    // No audit noise either: the only event is the original link.
    let actions: Vec<BindingAuditAction> = persistence
        .list_binding_audit_events(acme.id, before.id, 50)
        .await
        .unwrap()
        .iter()
        .map(|event| event.action)
        .collect();
    assert_eq!(actions, vec![BindingAuditAction::Linked]);
}

/// An agent's address *is* its owned channel's address, and `update_agent_and_owned_address` is
/// the second writer of `channel_slugs.is_primary`. It moved the slug without moving the binding
/// until this test existed, which left the channel unreachable by mail while looking healthy in
/// every projection.
#[tokio::test]
async fn renaming_an_agent_moves_its_owned_channel_binding_too() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let acme = company(&persistence, "agentaddr").await;
    let suffix = Uuid::new_v4().simple().to_string();
    let (agent, owned) = OwnedAgentChannelPersistence::create_owned_agent_channel(
        &persistence,
        acme.id,
        AgentWrite {
            name: "Scout".into(),
            slug: format!("scout-{suffix}"),
            ..AgentWrite::default()
        },
        ChannelWrite {
            name: "Scout".into(),
            slug: format!("scout-{suffix}"),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let before = persistence
        .active_bindings_for_channel(acme.id, owned.id)
        .await
        .unwrap()
        .remove(0);

    let renamed = format!("ranger-{suffix}");
    AgentPersistence::update(
        &persistence,
        agent.id,
        AgentWrite {
            name: "Ranger".into(),
            slug: renamed.clone(),
            ..AgentWrite::default()
        },
    )
    .await
    .unwrap();
    OwnedAgentChannelPersistence::update_agent_and_owned_address(
        &persistence,
        agent.id,
        AgentWrite {
            name: "Ranger".into(),
            slug: renamed.clone(),
            ..AgentWrite::default()
        },
    )
    .await
    .unwrap();

    let after = persistence
        .active_bindings_for_channel(acme.id, owned.id)
        .await
        .unwrap()
        .remove(0);
    assert_eq!(after.id, before.id, "the interface is the same one");
    assert_eq!(
        after.external_endpoint_key.as_str(),
        EmailEndpointKey::canonical(&ChannelSlug::from(renamed.as_str()))
            .unwrap()
            .as_str()
    );
    assert_eq!(
        persistence
            .find_active_binding_by_endpoint(&endpoint_of(&after))
            .await
            .unwrap()
            .map(|found| found.id),
        Some(before.id),
        "the new address resolves"
    );
    assert!(
        persistence
            .find_active_binding_by_endpoint(&endpoint_of(&before))
            .await
            .unwrap()
            .is_none(),
        "and the abandoned one does not"
    );
    assert_eq!(
        persistence
            .list_binding_audit_events(acme.id, after.id, 50)
            .await
            .unwrap()
            .first()
            .map(|event| event.action),
        Some(BindingAuditAction::EndpointChanged)
    );
}

/// The invariant behind both rename tests, stated once: no writer of a channel's primary slug may
/// leave its email binding pointing somewhere else.
#[tokio::test]
async fn no_channel_has_an_email_binding_that_disagrees_with_its_primary_slug() {
    let Some(pool) = test_pool().await else {
        return;
    };

    let stale: Vec<(Uuid, String, String)> = sqlx::query_as(
        r#"SELECT binding.channel_id, slug.slug::text, binding.external_endpoint_key
           FROM channel_bindings AS binding
           JOIN channel_slugs AS slug
               ON slug.channel_id = binding.channel_id AND slug.is_primary
           WHERE binding.transport = 'email'
             AND binding.status IN ('active', 'paused')
             AND binding.external_endpoint_key <> slug.slug::text"#,
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    assert!(
        stale.is_empty(),
        "these channels answer at one address and are bound to another, so inbound mail for them \
         resolves to no binding: {stale:?}"
    );
}
