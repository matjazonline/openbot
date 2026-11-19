use super::*;
use crate::adapters::persistence::test_support::test_pool;
use crate::adapters::protocols::email::EmailIdentity;
use crate::application::ports::participant::PrincipalAccessPersistence;
use crate::entities::company_member::CompanyAccessRole;
use crate::entities::value_objects::EmailAddress;
use crate::use_cases::company_invite::CompanyInvitePersistence;
use crate::use_cases::user::UserPersistence;

/// The accessible-companies query is built at runtime, so nothing but a real database can say
/// whether its `is_owner` projection and its root membership read do what they claim.
#[tokio::test]
async fn company_access_returns_owner_admin_member_and_no_relation() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::with_credential_cipher(
        pool.clone(),
        crate::adapters::persistence::credentials::CredentialCipher::for_test(),
    );

    let owner = create_account(&persistence, "owner").await;
    let admin = create_account(&persistence, "admin").await;
    let member = create_account(&persistence, "member").await;
    let stranger = create_account(&persistence, "stranger").await;
    sqlx::query("UPDATE users SET email_verified_at = CURRENT_TIMESTAMP WHERE id = ANY($1)")
        .bind([admin.0, member.0])
        .execute(persistence.pool())
        .await
        .unwrap();

    let company = persistence
        .create(
            owner.0,
            CompanyWrite {
                name: "Acme".to_string(),
                slug: format!("acme-{}", Uuid::new_v4().simple()),
                ..CompanyWrite::default()
            },
        )
        .await
        .expect("a company");
    let listed = persistence
        .list_by_user_id(owner.0)
        .await
        .expect("the ordinary company list");
    assert_eq!(listed[0].id, company.id);

    // Before the invite is accepted, the member is a stranger to it.
    assert!(accessible_ids(&persistence, member.0).await.is_empty());

    let invite = persistence
        .create_invite(
            crate::adapters::persistence::test_support::invite_command(
                &persistence,
                company.id,
                &member.1,
                CompanyAccessRole::Member,
            )
            .await,
        )
        .await
        .expect("an invite");
    persistence
        .accept_pending_invite(invite.id, member.0, &member.1)
        .await
        .expect("the invite is accepted");
    let admin_invite = persistence
        .create_invite(
            crate::adapters::persistence::test_support::invite_command(
                &persistence,
                company.id,
                &admin.1,
                CompanyAccessRole::Admin,
            )
            .await,
        )
        .await
        .expect("an admin invite");
    persistence
        .accept_pending_invite(admin_invite.id, admin.0, &admin.1)
        .await
        .expect("the admin invite is accepted");

    // The owner owns it, the member was let in, and the stranger sees nothing.
    let owner_view = persistence
        .company_access(owner.0, company.id)
        .await
        .expect("a lookup")
        .expect("the owner reaches their own company");
    assert_eq!(owner_view.membership, CompanyMembership::Owner);
    assert_eq!(owner_view.company.id, company.id);

    let member_view = persistence
        .company_access(member.0, company.id)
        .await
        .expect("a lookup")
        .expect("an accepted invite reaches the company");
    assert_eq!(member_view.membership, CompanyMembership::Member);

    let admin_view = persistence
        .company_access(admin.0, company.id)
        .await
        .expect("a lookup")
        .expect("an accepted admin reaches the company");
    assert_eq!(admin_view.membership, CompanyMembership::Admin);

    assert!(
        persistence
            .company_access(stranger.0, company.id)
            .await
            .expect("a lookup")
            .is_none()
    );

    // The inbound path asks the same question by address, but through the principal the
    // address resolves to -- and must tell the owner apart from the rest of the team:
    // `Channel::participant_access` hands the owner a restricted channel they are not granted,
    // and hands a member nothing.
    let by_address = async |address: &str| {
        persistence
            .access_context_for_identity(
                company.id,
                &EmailIdentity::parse(EmailAddress::from(address))
                    .expect("a parseable mailbox")
                    .qualify_default(),
            )
            .await
            .expect("a lookup")
    };
    // Bootstrap stores the normalized mailbox, and the parser normalizes what it is asked.
    assert_eq!(
        by_address(&owner.1.to_uppercase()).await.membership,
        CompanyMembership::Owner
    );
    assert_eq!(
        by_address(&member.1).await.membership,
        CompanyMembership::Member
    );
    assert_eq!(
        by_address(&admin.1).await.membership,
        CompanyMembership::Admin
    );

    // Somebody with an account but no standing here, and somebody with no account at all, are
    // both strangers -- and neither resolves to a principal in this company.
    for outsider in [stranger.1.as_str(), "nobody@example.com"] {
        let context = by_address(outsider).await;
        assert_eq!(context.membership, CompanyMembership::None);
        assert_eq!(context.principal_id, None);
    }

    // ...and the ownership-scoped listing the administration pages use is unchanged by any
    // of this: an invited member still owns nothing.
    assert!(
        persistence
            .list_by_user_id(member.0)
            .await
            .expect("a listing")
            .is_empty()
    );

    persistence
        .remove_member(crate::use_cases::company_invite::RemoveCompanyMember {
            actor_id: owner.0,
            company_id: company.id,
            member_user_id: member.0,
        })
        .await
        .ok();
    persistence
        .remove_member(crate::use_cases::company_invite::RemoveCompanyMember {
            actor_id: owner.0,
            company_id: company.id,
            member_user_id: admin.0,
        })
        .await
        .ok();
    for invite_id in [invite.id, admin_invite.id] {
        persistence
            .delete_invite(crate::use_cases::company_invite::DeleteCompanyInvite {
                actor_id: owner.0,
                company_id: company.id,
                invite_id,
            })
            .await
            .ok();
    }
    persistence.delete(company.id).await.ok();
}

#[tokio::test]
async fn a_company_can_replace_multiple_encrypted_model_connections_without_resending_keys() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::with_credential_cipher(
        pool.clone(),
        crate::adapters::persistence::credentials::CredentialCipher::for_test(),
    );
    let owner = create_account(&persistence, "model_connections").await;
    let stranger = create_account(&persistence, "model_connections_stranger").await;
    let company = persistence
        .create(
            owner.0,
            CompanyWrite {
                name: "Model Connections".into(),
                slug: format!("models-{}", Uuid::new_v4().simple()),
                ..CompanyWrite::default()
            },
        )
        .await
        .expect("a company");

    let denied = persistence
        .replace_model_connections_for_user(
            stranger.0,
            company.id,
            vec![
                CompanyModelConnectionWrite::new(
                    "openai",
                    Some("stolen-secret".into()),
                    vec!["gpt-a".into()],
                    true,
                )
                .unwrap(),
            ],
        )
        .await;
    assert!(denied.is_err());

    persistence
        .replace_model_connections_for_user(
            owner.0,
            company.id,
            vec![
                CompanyModelConnectionWrite::new(
                    "openai",
                    Some("openai-secret".into()),
                    vec!["gpt-a".into(), "gpt-b".into()],
                    true,
                )
                .unwrap(),
                CompanyModelConnectionWrite::new(
                    "anthropic",
                    Some("anthropic-secret".into()),
                    vec!["claude-a".into()],
                    false,
                )
                .unwrap(),
            ],
        )
        .await
        .expect("both providers save");

    let metadata = persistence
        .list_model_connections(company.id)
        .await
        .expect("connection metadata");
    assert_eq!(metadata.len(), 2);
    assert!(metadata.iter().all(|connection| connection.has_api_key));
    assert_eq!(
        persistence
            .model_api_key(company.id, &ModelProvider::canonical("anthropic"))
            .await
            .unwrap()
            .as_deref(),
        Some("anthropic-secret")
    );
    let stored: Vec<String> = sqlx::query_scalar(
        "SELECT api_key FROM company_model_connections WHERE company_id = $1 ORDER BY provider",
    )
    .bind(company.id)
    .fetch_all(&pool)
    .await
    .expect("stored credentials");
    assert!(stored.iter().all(|key| key.starts_with("enc:v1:1:")));
    assert!(stored.iter().all(|key| !key.contains("secret")));

    // Browsers never receive stored keys. A blank key on an existing row retains its secret
    // while the model allow-list and default selection can still change.
    persistence
        .replace_model_connections_for_user(
            owner.0,
            company.id,
            vec![
                CompanyModelConnectionWrite::new("openai", None, vec!["gpt-b".into()], false)
                    .unwrap(),
                CompanyModelConnectionWrite::new("anthropic", None, vec!["claude-a".into()], true)
                    .unwrap(),
            ],
        )
        .await
        .expect("metadata changes without key material");
    assert_eq!(
        persistence
            .model_api_key(company.id, &ModelProvider::canonical("openai"))
            .await
            .unwrap()
            .as_deref(),
        Some("openai-secret")
    );
    let connections = CompanyPersistence::list_model_connections(&persistence, company.id)
        .await
        .unwrap();
    let default = connections
        .iter()
        .find(|connection| connection.is_default)
        .expect("default connection remains");
    assert_eq!(default.provider.as_ref(), "anthropic");
    assert_eq!(default.models[0].as_ref(), "claude-a");

    // The form's explicit remove operation becomes omission from this wholesale replacement.
    // The omitted provider and its encrypted credential are deleted together.
    persistence
        .replace_model_connections_for_user(
            owner.0,
            company.id,
            vec![
                CompanyModelConnectionWrite::new("openai", None, vec!["gpt-b".into()], true)
                    .unwrap(),
            ],
        )
        .await
        .expect("one provider is deliberately removed");
    assert_eq!(
        persistence
            .model_api_key(company.id, &ModelProvider::canonical("anthropic"))
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        persistence
            .list_model_connections(company.id)
            .await
            .unwrap()
            .len(),
        1
    );

    persistence.delete(company.id).await.ok();
}

#[tokio::test]
async fn a_replace_that_would_strand_a_pinned_agent_is_refused_whole() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::with_credential_cipher(
        pool.clone(),
        crate::adapters::persistence::credentials::CredentialCipher::for_test(),
    );
    let owner = create_account(&persistence, "orphan_guard").await;
    let company = persistence
        .create(
            owner.0,
            CompanyWrite {
                name: "Orphan Guard".into(),
                slug: format!("orphan-{}", Uuid::new_v4().simple()),
                ..CompanyWrite::default()
            },
        )
        .await
        .expect("a company");

    persistence
        .replace_model_connections_for_user(
            owner.0,
            company.id,
            vec![
                CompanyModelConnectionWrite::new(
                    "openai",
                    Some("openai-secret".into()),
                    vec!["gpt-a".into(), "gpt-b".into()],
                    true,
                )
                .unwrap(),
            ],
        )
        .await
        .expect("the initial connection saves");

    crate::use_cases::agent::AgentPersistence::create(
        &persistence,
        company.id,
        crate::use_cases::agent::AgentWrite {
            name: "Pinned".into(),
            slug: "pinned".into(),
            provider: Some("openai".into()),
            model: Some("gpt-b".into()),
            ..Default::default()
        },
    )
    .await
    .expect("an agent pinned to gpt-b");

    // Narrowing the allow-list out from under the agent would leave it unable to run, so the
    // whole replace is refused rather than committing half a configuration.
    let refused = persistence
        .replace_model_connections_for_user(
            owner.0,
            company.id,
            vec![
                CompanyModelConnectionWrite::new("openai", None, vec!["gpt-a".into()], true)
                    .unwrap(),
            ],
        )
        .await
        .expect_err("the pinned agent blocks the narrowing");
    assert!(
        refused.to_string().contains("Pinned"),
        "the refusal should name the agent: {refused}"
    );

    // ... and nothing was written: gpt-b is still enabled.
    let connections = CompanyPersistence::list_model_connections(&persistence, company.id)
        .await
        .expect("connections survive the rolled-back replace");
    assert_eq!(connections.len(), 1);
    assert!(connections[0].models.iter().any(|model| model == "gpt-b"));

    persistence.delete(company.id).await.ok();
}

/// Both saves are positional binds built at runtime, so only a real database can say each
/// value still lands in its own column -- and that an owner-scoped save names its owner.
#[tokio::test]
async fn a_company_save_writes_every_field_to_its_own_column() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let owner = create_account(&persistence, "owner").await;
    let stranger = create_account(&persistence, "stranger").await;
    let company = persistence
        .create(
            owner.0,
            CompanyWrite {
                name: "Acme".to_string(),
                slug: format!("acme-{}", Uuid::new_v4().simple()),
                ..CompanyWrite::default()
            },
        )
        .await
        .expect("a company");
    assert!(company.channel_defaults.add_3rd_party);

    let slug = format!("globex-{}", Uuid::new_v4().simple());
    let saved = persistence
        .update_for_user(
            owner.0,
            company.id,
            CompanyWrite {
                name: "Globex".to_string(),
                slug: slug.clone(),
                channel_defaults: CompanyChannelDefaults {
                    add_3rd_party: false,
                },
                avatar_url: Some(AvatarUrl::from("https://cdn.example.com/globex.png")),
            },
        )
        .await
        .expect("the owner saves");
    assert_eq!(saved.name, "Globex");
    assert_eq!(saved.slug.as_str(), slug);
    assert!(!saved.channel_defaults.add_3rd_party);
    assert_eq!(
        saved.avatar_url,
        Some(AvatarUrl::from("https://cdn.example.com/globex.png"))
    );

    // An unscoped save replaces every field too.
    let renamed = persistence
        .update(
            company.id,
            CompanyWrite {
                name: "Initech".to_string(),
                slug: slug.clone(),
                ..CompanyWrite::default()
            },
        )
        .await
        .expect("an unscoped save");
    assert_eq!(renamed.name, "Initech");
    assert!(renamed.channel_defaults.add_3rd_party);
    assert_eq!(renamed.avatar_url, None);

    assert!(
        persistence
            .update_for_user(
                stranger.0,
                company.id,
                CompanyWrite {
                    name: "Stolen".to_string(),
                    slug,
                    ..CompanyWrite::default()
                },
            )
            .await
            .is_err()
    );
    assert_eq!(
        CompanyPersistence::get_by_id(&persistence, company.id)
            .await
            .unwrap()
            .expect("still there")
            .name,
        "Initech"
    );

    persistence.delete(company.id).await.ok();
}

/// A fresh account, as its id and address.
async fn create_account(persistence: &PostgresPersistence, prefix: &str) -> (Uuid, String) {
    let username = format!("{prefix}_{}", Uuid::new_v4().simple());
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .expect("an account");

    let user = persistence
        .get_by_email(&email)
        .await
        .expect("a lookup")
        .expect("the account just created");

    (user.id, email)
}

async fn accessible_ids(persistence: &PostgresPersistence, user_id: Uuid) -> Vec<Uuid> {
    persistence
        .list_accessible_by_user_id(user_id)
        .await
        .expect("a listing")
        .into_iter()
        .map(|access| access.company.id)
        .collect()
}
