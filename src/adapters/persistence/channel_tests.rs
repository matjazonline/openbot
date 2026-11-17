//! PostgreSQL round trips for the channel persistence boundary.
use super::*;
use crate::adapters::persistence::test_support::test_pool;
use crate::use_cases::agent::AgentPersistence;
use crate::use_cases::agent::AgentWrite;
use crate::use_cases::company::{CompanyPersistence, CompanyWrite};
use crate::use_cases::user::UserPersistence;

/// An edit made by a manager these tests do not otherwise need.
fn test_update(company_id: Uuid, channel_id: Uuid, write: ChannelWrite) -> ChannelUpdate {
    ChannelUpdate {
        company_id,
        channel_id,
        write,
    }
}

#[tokio::test]
async fn postgres_channel_persistence_works() {
    let Some(pool) = test_pool().await else {
        return;
    };

    let persistence = PostgresPersistence::new(pool);

    // Create owner & company. The company slug is unique database-wide, so it carries the same
    // per-run suffix as the owner — a fixed slug collides with any concurrent run of this test.
    let suffix = Uuid::new_v4().simple().to_string();
    let owner_username = format!("owner_{suffix}");
    let owner_email = format!("{owner_username}@example.com");
    let _ = persistence
        .create_user(&owner_username, &owner_email, "hash")
        .await;
    let owner = persistence
        .get_by_email(&owner_email)
        .await
        .unwrap()
        .unwrap();

    let company = CompanyPersistence::create(
        &persistence,
        owner.id,
        CompanyWrite {
            name: "Channel Corp".to_string(),
            slug: format!("ch-corp-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();

    // 1. Create Channel
    let emails = vec!["a@example.com".to_string(), "b@example.com".to_string()];

    let channel = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "Inbound Email".into(),
            description: Some("Takes support mail from the website form.".into()),
            slug: "inbound-email".into(),
            alias_slugs: Vec::new(),
            participant_emails: Some(emails.clone()),
            enabled: true,
            // Deliberately the opposite of `enabled`, so a swapped pair of same-typed binds
            // cannot pass this test.
            add_3rd_party: false,
            created_by: None,
            department_id: None,
        },
    )
    .await
    .unwrap();

    assert_eq!(channel.name, "Inbound Email");
    assert_eq!(
        channel.description.as_deref(),
        Some("Takes support mail from the website form.")
    );
    assert_eq!(channel.slug, "inbound-email");
    assert_eq!(
        channel.participant_emails,
        Some(
            emails
                .into_iter()
                .map(EmailAddress::from)
                .collect::<Vec<_>>()
        )
    );
    assert!(channel.enabled);
    assert!(!channel.add_3rd_party);

    // 2. Get by ID
    let fetched = ChannelPersistence::get_by_id(&persistence, channel.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.id, channel.id);

    // 3. List by company ID
    let list = ChannelPersistence::list_by_company_id(&persistence, company.id)
        .await
        .unwrap();
    assert_eq!(list.len(), 1);

    // 4. Update
    let updated = ChannelPersistence::update(
        &persistence,
        test_update(
            company.id,
            channel.id,
            ChannelWrite {
                name: "Inbound Email V2".into(),
                description: Some("Now also handles refund requests.".into()),
                slug: "inbound-email-v2".into(),
                enabled: false,
                add_3rd_party: true,
                created_by: None,
                ..ChannelWrite::default()
            },
        ),
    )
    .await
    .unwrap();
    assert_eq!(updated.name, "Inbound Email V2");
    assert_eq!(
        updated.description.as_deref(),
        Some("Now also handles refund requests."),
        "an edited description must survive a round trip"
    );
    assert_eq!(updated.participant_emails, None);
    assert!(!updated.enabled, "the off switch must survive a round trip");
    assert!(
        updated.add_3rd_party,
        "the third-party switch must survive a round trip"
    );

    let reread = ChannelPersistence::get_by_id(&persistence, channel.id)
        .await
        .unwrap()
        .unwrap();
    assert!(!reread.enabled);
    assert!(reread.add_3rd_party);
    assert_eq!(
        reread.description.as_deref(),
        Some("Now also handles refund requests.")
    );

    // A retained conversation prevents deleting the channel, even after it is disabled.
    let thread = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO threads (id, company_id, channel_id, subject) VALUES ($1,$2,$3,'Retained')",
    )
    .bind(thread)
    .bind(company.id)
    .bind(channel.id)
    .execute(persistence.pool())
    .await
    .unwrap();
    assert!(matches!(
        ChannelPersistence::delete(&persistence, channel.id).await,
        Err(AppError::Conflict(_))
    ));
    assert!(
        ChannelPersistence::get_by_id(&persistence, channel.id)
            .await
            .unwrap()
            .is_some()
    );
    sqlx::query("DELETE FROM threads WHERE id=$1")
        .bind(thread)
        .execute(persistence.pool())
        .await
        .unwrap();

    // 5. Delete an empty channel.
    ChannelPersistence::delete(&persistence, channel.id)
        .await
        .unwrap();
    let list_after = ChannelPersistence::list_by_company_id(&persistence, company.id)
        .await
        .unwrap();
    assert_eq!(list_after.len(), 0);

    // Cleanup
    let _ = CompanyPersistence::delete(&persistence, company.id).await;
}

#[tokio::test]
async fn alias_slugs_are_unique_within_a_company_and_free_across_companies() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);

    let owner_username = format!("owner_{}", Uuid::new_v4().simple());
    let owner_email = format!("{owner_username}@example.com");
    let _ = persistence
        .create_user(&owner_username, &owner_email, "hash")
        .await;
    let owner = persistence
        .get_by_email(&owner_email)
        .await
        .unwrap()
        .unwrap();

    let company_slug = format!("alias-co-{}", Uuid::new_v4().simple());
    let company = CompanyPersistence::create(
        &persistence,
        owner.id,
        CompanyWrite {
            name: "Alias Corp".to_string(),
            slug: company_slug.to_string(),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();

    let other_slug = format!("other-co-{}", Uuid::new_v4().simple());
    let other_company = CompanyPersistence::create(
        &persistence,
        owner.id,
        CompanyWrite {
            name: "Other Corp".to_string(),
            slug: other_slug.to_string(),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();

    let write = |slug: &str, aliases: &[&str]| ChannelWrite {
        name: "Support".into(),
        slug: slug.into(),
        alias_slugs: aliases.iter().map(|a| (*a).to_string()).collect(),
        enabled: false,
        ..ChannelWrite::default()
    };

    let support = ChannelPersistence::create(
        &persistence,
        company.id,
        write("support", &["sales", "help"]),
    )
    .await
    .unwrap();
    assert_eq!(support.slug, "support");
    assert_eq!(support.alias_slugs, ["help", "sales"]);

    // An alias is a real address: looking it up finds the channel that owns it.
    let by_alias = ChannelPersistence::get_by_company_slug_and_channel_slug(
        &persistence,
        &CompanySlug::from(company_slug.clone()),
        &ChannelSlug::from("sales"),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(by_alias.id, support.id);

    // A second channel may not claim a taken alias, nor collide with it via its own slug.
    let alias_conflict =
        ChannelPersistence::create(&persistence, company.id, write("billing", &["sales"])).await;
    assert!(
        matches!(alias_conflict, Err(AppError::BadRequest(ref m)) if m.contains("sales")),
        "expected a named conflict, got {alias_conflict:?}"
    );
    let slug_conflict =
        ChannelPersistence::create(&persistence, company.id, write("help", &[])).await;
    assert!(
        matches!(slug_conflict, Err(AppError::BadRequest(ref m)) if m.contains("help")),
        "expected a named conflict, got {slug_conflict:?}"
    );

    // The failed writes rolled back whole, so neither left a partial channel behind.
    let channels = ChannelPersistence::list_by_company_id(&persistence, company.id)
        .await
        .unwrap();
    assert_eq!(channels.len(), 1);

    // The namespace is per company, so another company may use the same names.
    ChannelPersistence::create(&persistence, other_company.id, write("sales", &["support"]))
        .await
        .unwrap();

    // An update replaces the alias set wholesale, freeing the names it drops.
    let updated = ChannelPersistence::update(
        &persistence,
        test_update(company.id, support.id, write("support", &["contact"])),
    )
    .await
    .unwrap();
    assert_eq!(updated.alias_slugs, ["contact"]);
    ChannelPersistence::create(&persistence, company.id, write("sales", &[]))
        .await
        .expect("a released alias is available again");

    let _ = CompanyPersistence::delete(&persistence, company.id).await;
    let _ = CompanyPersistence::delete(&persistence, other_company.id).await;
}

#[tokio::test]
async fn an_enabled_channel_needs_no_agent() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let suffix = Uuid::new_v4().simple().to_string();
    let owner = persistence
        .create_user(&suffix, &format!("{suffix}@example.com"), "hash")
        .await
        .unwrap()
        .id;
    let company = CompanyPersistence::create(
        &persistence,
        owner,
        CompanyWrite {
            name: "Plain".into(),
            slug: format!("plain-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let plain = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "Plain".into(),
            slug: "plain".into(),
            enabled: true,
            ..ChannelWrite::default()
        },
    )
    .await
    .expect("an enabled plain channel is creatable");
    assert!(plain.enabled);

    let agent = AgentPersistence::create(
        &persistence,
        company.id,
        AgentWrite {
            name: "Only".into(),
            slug: "only".into(),
            ..AgentWrite::default()
        },
    )
    .await
    .unwrap();
    let assigned = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "Assigned".into(),
            slug: "assigned".into(),
            enabled: true,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    AgentPersistence::delete(&persistence, agent.id)
        .await
        .expect("an enabled channel's only agent can be deleted");
    let left = ChannelPersistence::get_by_id(&persistence, assigned.id)
        .await
        .unwrap()
        .expect("the channel stays");
    assert!(left.enabled);
    let _ = CompanyPersistence::delete(&persistence, company.id).await;
}

/// The form still edits addresses; storage no longer does. A round trip therefore has to go
/// address -> identity -> principal -> grant and back, and `@public` has to survive that as an
/// access *mode* rather than turning into a grant for a principal named "@public".
#[tokio::test]
async fn a_channel_form_round_trips_its_email_allowlist_through_principal_grants() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());

    let suffix = Uuid::new_v4().simple().to_string();
    let owner_username = format!("grants_{suffix}");
    let owner_email = format!("{owner_username}@example.com");
    persistence
        .create_user(&owner_username, &owner_email, "hash")
        .await
        .unwrap();
    let owner = persistence
        .get_by_email(&owner_email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        &persistence,
        owner.id,
        CompanyWrite {
            name: "Grants Corp".to_string(),
            slug: format!("grants-corp-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let channel = ChannelPersistence::create(
        &persistence,
        company.id,
        ChannelWrite {
            name: "Front Desk".into(),
            slug: "front-desk".into(),
            // Mixed case and surrounding space, because the form is a text box.
            participant_emails: Some(vec![
                "  Dana@Partner.test ".to_string(),
                "@public".to_string(),
            ]),
            enabled: true,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(channel.access_mode, ChannelAccessMode::Public);
    assert_eq!(
        channel.participant_emails,
        Some(vec![
            EmailAddress::from("@public"),
            EmailAddress::from("dana@partner.test"),
        ]),
        "the form reads back the list it submitted, normalized"
    );

    // Authorization reads the grants, and `@public` is not one of them.
    let dana = channel
        .principal_grants
        .iter()
        .filter(|grant| {
            grant.provenance == crate::entities::participant::GrantProvenance::ConfiguredAllowlist
        })
        .map(|grant| grant.principal_id)
        .collect::<HashSet<_>>();
    assert_eq!(dana.len(), 1, "one address, one principal");
    let capabilities: HashSet<_> = channel
        .principal_grants
        .iter()
        .filter(|grant| {
            grant.provenance == crate::entities::participant::GrantProvenance::ConfiguredAllowlist
        })
        .map(|grant| grant.capability)
        .collect();
    assert_eq!(
        capabilities,
        HashSet::from([PrincipalCapability::Participate, PrincipalCapability::View]),
        "the allowlist form grants participation and UI read together"
    );

    let subject: String = sqlx::query_scalar(
        r#"SELECT identity.subject
           FROM channel_principal_grants AS channel_grant
           JOIN participant_identities AS identity
             ON (identity.company_id, identity.principal_id) =
                (channel_grant.company_id, channel_grant.principal_id)
           WHERE channel_grant.channel_id = $1 AND channel_grant.capability = 'participate' AND channel_grant.provenance='configured_allowlist'"#,
    )
    .bind(channel.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(subject, "dana@partner.test");

    // Editing the list replaces the grants rather than accumulating them.
    let updated = ChannelPersistence::update(
        &persistence,
        test_update(
            company.id,
            channel.id,
            ChannelWrite {
                name: "Front Desk".into(),
                slug: "front-desk".into(),
                participant_emails: Some(vec!["sam@partner.test".to_string()]),
                enabled: true,
                ..ChannelWrite::default()
            },
        ),
    )
    .await
    .unwrap();
    assert_eq!(updated.access_mode, ChannelAccessMode::Members);
    assert_eq!(
        updated.participant_emails,
        Some(vec![EmailAddress::from("sam@partner.test")])
    );
    assert_eq!(
        updated
            .principal_grants
            .iter()
            .filter(|grant| grant.provenance
                == crate::entities::participant::GrantProvenance::ConfiguredAllowlist)
            .count(),
        2
    );
    assert_eq!(
        updated
            .principal_grants
            .iter()
            .filter(|grant| grant.provenance
                == crate::entities::participant::GrantProvenance::DepartmentMembership)
            .count(),
        2
    );

    // A malformed address is a form error, not a silent "not authorized".
    let malformed = ChannelPersistence::update(
        &persistence,
        test_update(
            company.id,
            channel.id,
            ChannelWrite {
                name: "Front Desk".into(),
                slug: "front-desk".into(),
                participant_emails: Some(vec!["not-an-address".to_string()]),
                enabled: true,
                ..ChannelWrite::default()
            },
        ),
    )
    .await;
    assert!(matches!(malformed, Err(AppError::BadRequest(_))));
    let unchanged = ChannelPersistence::get_by_id(&persistence, channel.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        unchanged.participant_emails,
        Some(vec![EmailAddress::from("sam@partner.test")]),
        "the rejected edit rolled back whole"
    );

    let _ = CompanyPersistence::delete(&persistence, company.id).await;
}
