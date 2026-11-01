//! The allowlist form owns only the grants it wrote.

use super::*;
use crate::adapters::persistence::test_support::test_pool;
use crate::use_cases::company::{CompanyPersistence, CompanyWrite};
use crate::use_cases::participant::IdentityDirectory;
use crate::use_cases::user::UserPersistence;

fn allowlist_edit(company_id: Uuid, channel_id: Uuid, emails: &[&str]) -> ChannelUpdate {
    ChannelUpdate {
        company_id,
        channel_id,
        write: ChannelWrite {
            name: "Partners".into(),
            slug: "partners".into(),
            participant_emails: Some(emails.iter().map(|email| email.to_string()).collect()),
            enabled: true,
            ..ChannelWrite::default()
        },
    }
}

async fn provenances(pool: &sqlx::PgPool, channel_id: Uuid, principal_id: Uuid) -> Vec<String> {
    sqlx::query_scalar(
        r#"SELECT provenance FROM channel_principal_grants
           WHERE channel_id = $1 AND principal_id = $2 ORDER BY capability"#,
    )
    .bind(channel_id)
    .bind(principal_id)
    .fetch_all(pool)
    .await
    .unwrap()
}

/// A grant written by a manager survives its address being added to the allowlist and then taken
/// off it again: the allowlist neither relabels it as its own nor deletes it.
#[tokio::test]
async fn channel_allowlist_leaves_a_manager_grant_alone() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let suffix = Uuid::new_v4().simple().to_string();
    let owner = persistence
        .create_user(
            &format!("allow_{suffix}"),
            &format!("allow_{suffix}@example.test"),
            "hash",
        )
        .await
        .unwrap();
    let company_id = CompanyPersistence::create(
        &persistence,
        owner.id,
        CompanyWrite {
            name: "Allow".into(),
            slug: format!("allow-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap()
    .id;
    let channel = ChannelPersistence::create(
        &persistence,
        company_id,
        ChannelWrite {
            name: "Partners".into(),
            slug: "partners".into(),
            participant_emails: Some(vec!["sam@partner.test".into()]),
            enabled: true,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let dana = persistence
        .resolve_or_create_external_identity(
            company_id,
            IdentityObservation {
                identity: EmailIdentity::parse(EmailAddress::from("dana@partner.test"))
                    .unwrap()
                    .qualify_default(),
                display_label: None,
                claim_metadata: IdentityClaimMetadata::observation(),
                provenance: IdentityProvenance::TransportIngress,
            },
        )
        .await
        .unwrap()
        .principal
        .id
        .as_uuid();
    for capability in ["participate", "view"] {
        sqlx::query(
            r#"INSERT INTO channel_principal_grants
                   (company_id, channel_id, principal_id, capability, provenance)
               VALUES ($1, $2, $3, $4, 'manager')"#,
        )
        .bind(company_id)
        .bind(channel.id)
        .bind(dana)
        .bind(capability)
        .execute(&pool)
        .await
        .unwrap();
    }

    ChannelPersistence::update(
        &persistence,
        allowlist_edit(
            company_id,
            channel.id,
            &["sam@partner.test", "dana@partner.test"],
        ),
    )
    .await
    .unwrap();
    assert_eq!(
        provenances(&pool, channel.id, dana).await,
        vec!["manager", "manager"],
        "adding the address does not relabel the manager's grant"
    );

    let edited = ChannelPersistence::update(
        &persistence,
        allowlist_edit(company_id, channel.id, &["sam@partner.test"]),
    )
    .await
    .unwrap();
    assert_eq!(
        provenances(&pool, channel.id, dana).await,
        vec!["manager", "manager"],
        "removing the address does not delete the manager's grant"
    );
    assert_eq!(
        edited.participant_emails,
        Some(vec![EmailAddress::from("sam@partner.test")])
    );

    let _ = CompanyPersistence::delete(&persistence, company_id).await;
}
