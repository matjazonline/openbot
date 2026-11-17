//! `address_namespace_`: one address namespace per company, shared by desks and principals.
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    adapters::{
        persistence::{
            PostgresPersistence,
            participant::{create_person_principal_on, resolve_or_create_external_identity_on},
            test_support::{OwnDatabase, own_database, test_pool, wait_until_a_backend_is_blocked},
        },
        protocols::email::EmailIdentity,
    },
    app_error::AppError,
    application::ports::{
        account_deletion::HostAccountDeletion,
        channel::{ChannelPersistence, ChannelUpdate, ChannelWrite},
        participant::IdentityObservation,
    },
    entities::{
        company::Company,
        participant::{IdentityClaimMetadata, IdentityProvenance},
        value_objects::{ChannelSlug, EmailAddress},
    },
    use_cases::{
        company::{CompanyPersistence, CompanyWrite},
        company_invite::CompanyInvitePersistence,
        user::UserPersistence,
    },
};

struct Fixture {
    persistence: PostgresPersistence,
    company: Company,
    owner: Uuid,
    owner_slug: String,
    /// Held for the competing tests, which wait for "some backend is blocked": on a database of
    /// their own, that backend can only be the contended statement under test.
    _database: Option<OwnDatabase>,
}

impl Fixture {
    async fn new() -> Option<Self> {
        let pool = test_pool().await?;
        Some(Self::on(pool, None).await)
    }

    async fn isolated() -> Option<Self> {
        let database = own_database().await?;
        Some(Self::on(database.pool.clone(), Some(database)).await)
    }

    async fn on(pool: PgPool, database: Option<OwnDatabase>) -> Self {
        let persistence = PostgresPersistence::new(pool);
        let (owner, owner_name) = account(&persistence, "owner").await;
        let company = company(&persistence, owner).await;
        Self {
            owner_slug: owner_name.replace('_', "-"),
            persistence,
            company,
            owner,
            _database: database,
        }
    }

    fn pool(&self) -> &PgPool {
        &self.persistence.pool
    }

    /// Join an existing account to the company the way invite acceptance does: the member row,
    /// then the person principal (and with it, the address).
    async fn join(&self, user: Uuid, username: &str) -> Uuid {
        let mut transaction = self.pool().begin().await.unwrap();
        let email = email_of(self.pool(), user).await;
        let principal =
            create_person_principal_on(&mut transaction, self.company.id, user, username, &email)
                .await
                .unwrap();
        root_membership_on(&mut transaction, self.company.id, principal)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        principal.as_uuid()
    }

    async fn desk(&self, slug: &str, aliases: &[&str]) -> Result<Uuid, AppError> {
        ChannelPersistence::create(
            &self.persistence,
            self.company.id,
            desk_write(slug, aliases),
        )
        .await
        .map(|desk| desk.id)
    }

    async fn principal_of(&self, user: Uuid) -> Uuid {
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 AND user_id = $2")
            .bind(self.company.id)
            .bind(user)
            .fetch_one(self.pool())
            .await
            .unwrap()
    }

    /// Every address row the principal has, as `(slug, is_primary)`.
    async fn addresses_of(&self, principal: Uuid) -> Vec<(String, bool)> {
        sqlx::query_as(
            "SELECT slug::text, is_primary FROM addresses \
             WHERE company_id = $1 AND principal_id = $2 ORDER BY slug",
        )
        .bind(self.company.id)
        .bind(principal)
        .fetch_all(self.pool())
        .await
        .unwrap()
    }
}

/// A fresh account named `{prefix}_{hex}`; its derived address is that name with `-` for `_`.
async fn account(persistence: &PostgresPersistence, prefix: &str) -> (Uuid, String) {
    let name = format!("{prefix}_{}", Uuid::new_v4().simple());
    let email = format!("{name}@example.test");
    persistence
        .create_user(&name, &email, "hash")
        .await
        .unwrap();
    let user = persistence.get_by_email(&email).await.unwrap().unwrap();
    (user.id, name)
}

async fn company(persistence: &PostgresPersistence, owner: Uuid) -> Company {
    CompanyPersistence::create(
        persistence,
        owner,
        CompanyWrite {
            name: "Address Co".into(),
            slug: format!("address-co-{}", Uuid::new_v4().simple()),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap()
}

// The resolver's fixture transaction adds its mandatory root membership after creating or
// restoring the person. Keeping this separate leaves address collision/restoration under test.
async fn root_membership_on(
    db: &mut sqlx::PgConnection,
    company: Uuid,
    principal: crate::entities::transport::PrincipalId,
) -> crate::app_error::AppResult<()> {
    sqlx::query(
        "INSERT INTO department_members (company_id, department_id, principal_id, access_level) \
         SELECT $1, root.id, $2, 1 FROM departments AS root WHERE root.company_id = $1 AND root.is_root \
         ON CONFLICT (department_id, principal_id) DO NOTHING",
    )
    .bind(company).bind(principal.as_uuid()).execute(db).await?;
    Ok(())
}

async fn email_of(pool: &PgPool, user: Uuid) -> String {
    sqlx::query_scalar("SELECT email::text FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(pool)
        .await
        .unwrap()
}

fn desk_write(slug: &str, aliases: &[&str]) -> ChannelWrite {
    ChannelWrite {
        name: "Desk".into(),
        slug: slug.into(),
        alias_slugs: aliases.iter().map(|alias| (*alias).to_string()).collect(),
        enabled: false,
        ..ChannelWrite::default()
    }
}

/// A raw row's `created_by`, for the fixtures that write one without going through a writer.
fn system_provenance() -> serde_json::Value {
    serde_json::json!({"actor_type": "system", "actor_id": null, "actor_name": "Test"})
}

fn refused_by(error: sqlx::Error) -> (String, String) {
    let database = error.as_database_error().expect("a database refusal");
    (
        database.code().unwrap_or_default().into_owned(),
        database.constraint().unwrap_or_default().to_owned(),
    )
}

fn taken(result: Result<Uuid, AppError>, slug: &str) {
    assert!(
        matches!(&result, Err(AppError::BadRequest(message))
            if message.contains(slug) && message.contains("a desk, person or agent")),
        "expected the address '{slug}' to be refused as taken, got {result:?}"
    );
}

#[tokio::test]
async fn address_namespace_a_slug_names_one_desk_or_person_per_company() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let owner = f.principal_of(f.owner).await;
    assert_eq!(f.addresses_of(owner).await, [(f.owner_slug.clone(), true)]);

    // A desk cannot take a person's address, as its slug, an alias or by renaming.
    taken(f.desk(&f.owner_slug, &[]).await, &f.owner_slug);
    taken(f.desk("front", &[&f.owner_slug]).await, &f.owner_slug);
    let front = f.desk("front", &[]).await.unwrap();
    let rename = ChannelPersistence::update(
        &f.persistence,
        ChannelUpdate {
            company_id: f.company.id,
            channel_id: front,
            write: desk_write(&f.owner_slug, &[]),
        },
    )
    .await
    .map(|desk| desk.id);
    taken(rename, &f.owner_slug);

    // A person joining at a desk's slug is given the next free one rather than refused.
    let (dana, dana_name) = account(&f.persistence, "dana").await;
    let dana_slug = dana_name.replace('_', "-");
    f.desk(&dana_slug, &[]).await.unwrap();
    let dana_principal = f.join(dana, &dana_name).await;
    assert_eq!(
        f.addresses_of(dana_principal).await,
        [(format!("{dana_slug}-2"), true)]
    );

    // A person cannot rename their address onto a desk's.
    let renamed = f
        .persistence
        .rename_person_address(dana, f.company.id, &ChannelSlug::from("front"))
        .await;
    assert!(
        matches!(&renamed, Err(AppError::Conflict(message)) if message.contains("front")),
        "{renamed:?}"
    );
    let renamed = f
        .persistence
        .rename_person_address(dana, f.company.id, &ChannelSlug::from("dana-own"))
        .await
        .unwrap()
        .expect("the person's own address");
    assert_eq!(renamed.slug.as_str(), "dana-own");
    assert_eq!(
        f.addresses_of(dana_principal).await,
        [("dana-own".into(), true)]
    );

    // Another company is another namespace: the same slugs are free there.
    let other = company(&f.persistence, f.owner).await;
    ChannelPersistence::create(&f.persistence, other.id, desk_write("dana-own", &[]))
        .await
        .unwrap();
    // The owner owns both companies, so the owner has an address in each, each the same slug.
    let other_owner: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 AND user_id = $2")
            .bind(other.id)
            .bind(f.owner)
            .fetch_one(f.pool())
            .await
            .unwrap();
    let other_slug: String = sqlx::query_scalar(
        "SELECT slug::text FROM addresses WHERE company_id = $1 AND principal_id = $2",
    )
    .bind(other.id)
    .bind(other_owner)
    .fetch_one(f.pool())
    .await
    .unwrap();
    assert_eq!(other_slug, f.owner_slug);
}

#[tokio::test]
async fn address_namespace_refuses_external_and_system_principals_and_a_second_address() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let insert = |principal: Uuid, slug: &str| {
        sqlx::query(
            "INSERT INTO addresses (company_id, slug, target_kind, principal_id, is_primary) \
             VALUES ($1, $2, 'principal', $3, true)",
        )
        .bind(f.company.id)
        .bind(slug.to_owned())
        .bind(principal)
    };
    let external = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO principals (id, company_id, kind, display_label) VALUES ($1, $2, 'external', 'Outsider')",
    )
    .bind(external)
    .bind(f.company.id)
    .execute(f.pool())
    .await
    .unwrap();
    let refused = insert(external, "outsider")
        .execute(f.pool())
        .await
        .unwrap_err();
    assert_eq!(
        refused_by(refused),
        ("23514".into(), "addresses_principal_kind".into())
    );

    let system: Uuid = sqlx::query_scalar(
        "INSERT INTO principals (id, company_id, kind, display_label) VALUES ($1, $2, 'system', 'System') RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(f.company.id)
    .fetch_one(f.pool())
    .await
    .unwrap();
    let refused = insert(system, "system-actor")
        .execute(f.pool())
        .await
        .unwrap_err();
    assert_eq!(
        refused_by(refused),
        ("23514".into(), "addresses_principal_kind".into())
    );

    let owner = f.principal_of(f.owner).await;
    let refused = insert(owner, "second-name")
        .execute(f.pool())
        .await
        .unwrap_err();
    assert_eq!(
        refused_by(refused),
        ("23505".into(), "addresses_principal_key".into())
    );

    // A desk keeps one primary; a principal's one row is always primary.
    let desk = f.desk("primary-desk", &[]).await.unwrap();
    let refused = sqlx::query(
        "INSERT INTO addresses (company_id, slug, target_kind, channel_id, is_primary) \
         VALUES ($1, 'primary-two', 'desk', $2, true)",
    )
    .bind(f.company.id)
    .bind(desk)
    .execute(f.pool())
    .await
    .unwrap_err();
    assert_eq!(
        refused_by(refused),
        ("23505".into(), "addresses_desk_primary_idx".into())
    );
    let refused = sqlx::query(
        "INSERT INTO addresses (company_id, slug, target_kind, principal_id, is_primary) \
         VALUES ($1, 'not-primary', 'principal', $2, false)",
    )
    .bind(f.company.id)
    // A person, so the kind guard passes and the target check (before the one-address key) refuses.
    .bind(owner)
    .execute(f.pool())
    .await
    .unwrap_err();
    assert_eq!(
        refused_by(refused),
        ("23514".into(), "addresses_target_check".into())
    );
}

#[tokio::test]
async fn address_namespace_rejects_a_target_from_another_company() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let other = company(&f.persistence, f.owner).await;
    let foreign_desk =
        ChannelPersistence::create(&f.persistence, other.id, desk_write("foreign", &[]))
            .await
            .unwrap();
    let foreign_principal: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 AND user_id = $2")
            .bind(other.id)
            .bind(f.owner)
            .fetch_one(f.pool())
            .await
            .unwrap();
    let mut transaction = f.pool().begin().await.unwrap();
    let refused = sqlx::query(
        "INSERT INTO addresses (company_id, slug, target_kind, channel_id, is_primary) \
         VALUES ($1, 'borrowed-desk', 'desk', $2, false)",
    )
    .bind(f.company.id)
    .bind(foreign_desk.id)
    .execute(&mut *transaction)
    .await
    .unwrap_err();
    assert_eq!(
        refused_by(refused),
        ("23503".into(), "addresses_channel_fk".into())
    );
    transaction.rollback().await.unwrap();

    let mut transaction = f.pool().begin().await.unwrap();
    let refused = sqlx::query(
        "INSERT INTO addresses (company_id, slug, target_kind, principal_id, is_primary) \
         VALUES ($1, 'borrowed-person', 'principal', $2, true)",
    )
    .bind(f.company.id)
    .bind(foreign_principal)
    .execute(&mut *transaction)
    .await
    .unwrap_err();
    assert_eq!(
        refused_by(refused),
        ("23503".into(), "addresses_principal_fk".into())
    );
    transaction.rollback().await.unwrap();
}

#[tokio::test]
async fn address_namespace_person_paths_leave_exactly_one_address() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    // Promotion: an outsider who wrote in before joining keeps their principal and gains one.
    let (erin, erin_name) = account(&f.persistence, "erin").await;
    let erin_email = email_of(f.pool(), erin).await;
    let mut transaction = f.pool().begin().await.unwrap();
    let identity = EmailIdentity::parse(EmailAddress::from(erin_email.as_str()))
        .map(EmailIdentity::qualify_default)
        .unwrap();
    resolve_or_create_external_identity_on(
        &mut transaction,
        f.company.id,
        IdentityObservation {
            identity,
            display_label: None,
            claim_metadata: IdentityClaimMetadata::observation(),
            provenance: IdentityProvenance::ChannelAllowlist,
        },
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    let outsider = f.principal_by_email(&erin_email).await;
    assert!(
        f.addresses_of(outsider).await.is_empty(),
        "an outsider has no address"
    );
    let promoted = f.join(erin, &erin_name).await;
    assert_eq!(
        promoted, outsider,
        "the outsider's principal is promoted, not duplicated"
    );
    let erin_slug = erin_name.replace('_', "-");
    assert_eq!(f.addresses_of(promoted).await, [(erin_slug.clone(), true)]);

    // Reuse: creating the same person again keeps the one address.
    let mut transaction = f.pool().begin().await.unwrap();
    create_person_principal_on(
        &mut transaction,
        f.company.id,
        erin,
        &erin_name,
        &erin_email,
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    assert_eq!(f.addresses_of(promoted).await, [(erin_slug.clone(), true)]);

    // Removal demotes and frees the slug; rejoining derives a fresh one.
    f.persistence
        .remove_member(crate::use_cases::company_invite::RemoveCompanyMember {
            actor_id: f.company.user_id,
            company_id: f.company.id,
            member_user_id: erin,
        })
        .await
        .unwrap();
    assert!(
        f.addresses_of(promoted).await.is_empty(),
        "a removed member keeps no address"
    );
    let free: bool = sqlx::query_scalar(
        "SELECT NOT EXISTS (SELECT 1 FROM addresses WHERE company_id = $1 AND slug = $2)",
    )
    .bind(f.company.id)
    .bind(&erin_slug)
    .fetch_one(f.pool())
    .await
    .unwrap();
    assert!(free, "the slug is free again");
    let rejoined = f.join(erin, &erin_name).await;
    assert_eq!(f.addresses_of(rejoined).await, [(erin_slug, true)]);
}

#[tokio::test]
async fn address_namespace_demotion_of_an_agent_principal_drops_its_address() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    // Native namespace demotion must remove a valid canonical agent address.
    let agent_principal: Uuid = sqlx::query_scalar(
        "WITH agent AS ( \
           INSERT INTO agents (id, company_id, name, slug, created_by) \
           VALUES ($1, $2, 'Helper', $3, $5) RETURNING id) \
         INSERT INTO principals (id, company_id, kind, agent_id, display_label) \
         SELECT $4, $2, 'agent', agent.id, 'Helper' FROM agent RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(f.company.id)
    .bind("helper-direct")
    .bind(Uuid::new_v4())
    .bind(system_provenance())
    .fetch_one(f.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO addresses (company_id, slug, target_kind, principal_id, is_primary) \
         VALUES ($1, 'helper-direct', 'principal', $2, true)",
    )
    .bind(f.company.id)
    .bind(agent_principal)
    .execute(f.pool())
    .await
    .unwrap();
    sqlx::query("UPDATE principals SET kind = 'external', agent_id = NULL WHERE id = $1")
        .bind(agent_principal)
        .execute(f.pool())
        .await
        .unwrap();
    assert!(f.addresses_of(agent_principal).await.is_empty());
}

#[tokio::test]
async fn address_namespace_competing_desk_and_join_at_one_slug_leave_one_owner() {
    let Some(f) = Fixture::isolated().await else {
        return;
    };
    let (gale, gale_name) = account(&f.persistence, "gale").await;
    let gale_slug = gale_name.replace('_', "-");
    // The desk's address is written first and held uncommitted; the join's insert at the same slug
    // waits on it, then moves to the next suffix once the desk commits.
    let mut desk_tx = f.pool().begin().await.unwrap();
    let desk = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO channels (id, company_id, name, enabled, created_by, department_id) \
         VALUES ($1, $2, 'Gale', false, $3, (SELECT id FROM departments WHERE company_id=$2 AND is_root))",
    )
    .bind(desk)
    .bind(f.company.id)
    .bind(system_provenance())
    .execute(&mut *desk_tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO addresses (company_id, slug, target_kind, channel_id, is_primary) \
         VALUES ($1, $2, 'desk', $3, true)",
    )
    .bind(f.company.id)
    .bind(&gale_slug)
    .bind(desk)
    .execute(&mut *desk_tx)
    .await
    .unwrap();
    let joining = {
        let pool = f.pool().clone();
        let company = f.company.id;
        let name = gale_name.clone();
        tokio::spawn(async move {
            let mut transaction = pool.begin().await.unwrap();
            let email = format!("{name}@example.test");
            let principal =
                create_person_principal_on(&mut transaction, company, gale, &name, &email).await;
            if let Ok(id) = principal.as_ref() {
                root_membership_on(&mut transaction, company, *id)
                    .await
                    .unwrap();
            }
            transaction.commit().await.map(|()| principal)
        })
    };
    wait_until_a_backend_is_blocked(f.pool()).await;
    desk_tx.commit().await.unwrap();
    let principal = joining
        .await
        .unwrap()
        .expect("the join commits")
        .expect("the join is not refused");
    assert_eq!(
        f.addresses_of(principal.as_uuid()).await,
        [(format!("{gale_slug}-2"), true)]
    );
    let owner: Option<Uuid> =
        sqlx::query_scalar("SELECT channel_id FROM addresses WHERE company_id = $1 AND slug = $2")
            .bind(f.company.id)
            .bind(&gale_slug)
            .fetch_one(f.pool())
            .await
            .unwrap();
    assert_eq!(
        owner,
        Some(desk),
        "the desk kept the slug it committed first"
    );
}

#[tokio::test]
async fn address_namespace_competing_self_edit_and_removal_leave_no_address_on_an_outsider() {
    let Some(f) = Fixture::isolated().await else {
        return;
    };
    let (hale, hale_name) = account(&f.persistence, "hale").await;
    let principal = f.join(hale, &hale_name).await;
    // The removal demotes first and holds its transaction open, which is what makes the edit wait
    // on the address row the demotion deleted.
    let mut removal = f.pool().begin().await.unwrap();
    sqlx::query("UPDATE principals SET kind = 'external', user_id = NULL WHERE id = $1")
        .bind(principal)
        .execute(&mut *removal)
        .await
        .unwrap();
    let editing = {
        let persistence = PostgresPersistence::new(f.pool().clone());
        let company = f.company.id;
        tokio::spawn(async move {
            persistence
                .rename_person_address(hale, company, &ChannelSlug::from("hale-new"))
                .await
        })
    };
    wait_until_a_backend_is_blocked(f.pool()).await;
    removal.commit().await.unwrap();
    let edited = editing.await.unwrap();
    assert!(
        matches!(edited, Ok(None)),
        "the removed person's edit finds no address of theirs, without a lock error: {edited:?}"
    );
    assert!(f.addresses_of(principal).await.is_empty());
}

#[tokio::test]
async fn address_namespace_competing_insert_and_demotion_leave_no_address_on_an_outsider() {
    let Some(f) = Fixture::isolated().await else {
        return;
    };
    // Insert first: the demotion waits on the guard's key-share lock, then deletes the address.
    let (iris, iris_name) = account(&f.persistence, "iris").await;
    let principal = f.join(iris, &iris_name).await;
    sqlx::query("DELETE FROM addresses WHERE company_id = $1 AND principal_id = $2")
        .bind(f.company.id)
        .bind(principal)
        .execute(f.pool())
        .await
        .unwrap();
    let mut insert = f.pool().begin().await.unwrap();
    sqlx::query(
        "INSERT INTO addresses (company_id, slug, target_kind, principal_id, is_primary) \
         VALUES ($1, 'iris-first', 'principal', $2, true)",
    )
    .bind(f.company.id)
    .bind(principal)
    .execute(&mut *insert)
    .await
    .unwrap();
    let demoting = {
        let pool = f.pool().clone();
        tokio::spawn(async move {
            sqlx::query("UPDATE principals SET kind = 'external', user_id = NULL WHERE id = $1")
                .bind(principal)
                .execute(&pool)
                .await
        })
    };
    wait_until_a_backend_is_blocked(f.pool()).await;
    insert.commit().await.unwrap();
    demoting
        .await
        .unwrap()
        .expect("the demotion commits after the insert");
    assert!(f.addresses_of(principal).await.is_empty());

    // Demotion first: the insert's guard waits for it, then reads `external` and refuses.
    let (jude, jude_name) = account(&f.persistence, "jude").await;
    let principal = f.join(jude, &jude_name).await;
    sqlx::query("DELETE FROM addresses WHERE company_id = $1 AND principal_id = $2")
        .bind(f.company.id)
        .bind(principal)
        .execute(f.pool())
        .await
        .unwrap();
    let mut demotion = f.pool().begin().await.unwrap();
    sqlx::query("UPDATE principals SET kind = 'external', user_id = NULL WHERE id = $1")
        .bind(principal)
        .execute(&mut *demotion)
        .await
        .unwrap();
    let inserting = {
        let pool = f.pool().clone();
        let company = f.company.id;
        tokio::spawn(async move {
            sqlx::query(
                "INSERT INTO addresses (company_id, slug, target_kind, principal_id, is_primary) \
                 VALUES ($1, 'jude-late', 'principal', $2, true)",
            )
            .bind(company)
            .bind(principal)
            .execute(&pool)
            .await
        })
    };
    wait_until_a_backend_is_blocked(f.pool()).await;
    demotion.commit().await.unwrap();
    let refused = inserting.await.unwrap().unwrap_err();
    assert_eq!(
        refused_by(refused),
        ("23514".into(), "addresses_principal_kind".into())
    );
    assert!(f.addresses_of(principal).await.is_empty());
}

#[tokio::test]
async fn address_namespace_competing_held_edit_then_removal_leave_no_address_on_an_outsider() {
    let Some(f) = Fixture::isolated().await else {
        return;
    };
    let (kai, kai_name) = account(&f.persistence, "kai").await;
    let principal = f.join(kai, &kai_name).await;
    // The edit's row lock is held first; the removal's demotion then waits on it to delete the
    // address, and completes once the edit commits. A slug-only update never fires the kind
    // guard, so neither transaction waits on the other's principal and no lock cycle can form.
    let mut edit = f.pool().begin().await.unwrap();
    let edited = sqlx::query(
        "UPDATE addresses SET slug = 'kai-renamed' WHERE company_id = $1 AND principal_id = $2",
    )
    .bind(f.company.id)
    .bind(principal)
    .execute(&mut *edit)
    .await
    .unwrap();
    assert_eq!(edited.rows_affected(), 1);
    let removing = {
        let persistence = PostgresPersistence::new(f.pool().clone());
        let company = f.company.id;
        let removing_owner = f.owner;
        tokio::spawn(async move {
            persistence
                .remove_member(crate::use_cases::company_invite::RemoveCompanyMember {
                    actor_id: removing_owner,
                    company_id: company,
                    member_user_id: kai,
                })
                .await
        })
    };
    wait_until_a_backend_is_blocked(f.pool()).await;
    edit.commit().await.unwrap();
    removing
        .await
        .unwrap()
        .expect("the removal completes after the edit, without a lock error");
    assert!(f.addresses_of(principal).await.is_empty());
}

#[tokio::test]
async fn address_namespace_kind_guard_fires_only_when_a_target_changes() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    // Keep the kind guard narrow. Slug-only writes advance the routing revision through a
    // separate trigger; their production writer locks the principal before the address, matching
    // removal. Re-running the kind guard after locking only the address would invert that order.
    let definition: String = sqlx::query_scalar(
        "SELECT pg_get_triggerdef(oid) FROM pg_trigger \
         WHERE tgrelid = 'public.addresses'::regclass AND tgname = 'guard_address_principal'",
    )
    .fetch_one(f.pool())
    .await
    .unwrap();
    assert!(
        definition.contains(
            "BEFORE INSERT OR UPDATE OF target_kind, channel_id, principal_id ON public.addresses"
        ),
        "{definition}"
    );
}

#[tokio::test]
async fn address_namespace_deleting_an_account_drops_its_addresses() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (lane, lane_name) = account(&f.persistence, "lane").await;
    let principal = f.join(lane, &lane_name).await;
    assert_eq!(f.addresses_of(principal).await.len(), 1);
    f.persistence.delete_user_account(lane).await.unwrap();
    assert!(f.addresses_of(principal).await.is_empty());
}

impl Fixture {
    async fn principal_by_email(&self, email: &str) -> Uuid {
        sqlx::query_scalar(
            "SELECT principal_id FROM participant_identities \
             WHERE company_id = $1 AND transport = 'email' AND subject = $2",
        )
        .bind(self.company.id)
        .bind(email.to_lowercase())
        .fetch_one(self.pool())
        .await
        .unwrap()
    }
}

mod direct;
